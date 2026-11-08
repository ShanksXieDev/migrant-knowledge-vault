//! 检索结果精确定位（PRD §7.1）与双链图谱快照（F-10）。
//!
//! 本模块只依赖 `serde` 与 [`crate::format::Format`]，**无任何原生 / C 依赖**，
//! 因此 Web（wasm32）与 Desktop / Mobile 三端都能编译。
//!
//! ## 为什么是「文本片段」而不是改抽取器
//!
//! 六格式抽取器（`parse.rs`）已经把原文规范化成一份纯文本 `text`，
//! 索引与预览都以它为准。若为了定位去改抽取输出，会让既有索引、双链偏移
//! 与全部预览回归同时失效。因此这里把**已有的 `text` 再做一次结构化切分**：
//! 按格式识别段落 / 页 / 单元格行 / 节点，得到带标签的片段，
//! 每段的 `text` 是原文的一个连续切片，拼接后与原文严格相等。
//!
//! 这样：
//! - 索引产物新增的是**可选字段**（`#[serde(default)]`），旧产物仍可解析；
//! - 旧产物在运行期也能用同一套切分函数补算片段，不需要重新导出；
//! - Web 只读端与 Desktop 端共用完全相同的定位语义。

use serde::{Deserialize, Serialize};

use crate::format::Format;

/// 原文中的一个可定位片段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextSegment {
    /// 从 0 开始的段序号，UI 用它做高亮与定位。
    pub index: usize,
    /// 人类可读的位置标签，例如「第 3 段」「Sheet1 · 第 4 行」「第 2 页」。
    pub label: String,
    /// 片段类型：heading / paragraph / row / page / node。
    pub kind: String,
    /// 原文连续切片；所有片段拼接后等于原文。
    pub text: String,
}

/// 一次检索命中的精确位置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HitLocation {
    pub label: String,
    pub kind: String,
    pub segment_index: usize,
    /// 实际命中的查询片段（查询词可能被分词，取最长可命中子串）。
    pub matched: String,
    /// 命中处上下文，供结果列表与定位条展示。
    pub snippet: String,
}

/// 图谱节点（已带布局坐标）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphNodeView {
    pub path: String,
    pub title: String,
    pub x: f32,
    pub y: f32,
    /// 入边 + 出边总数。
    pub degree: usize,
}

/// 图谱边（节点下标）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphEdgeView {
    pub from: usize,
    pub to: usize,
}

/// 可直接交给 UI 渲染的图谱快照。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphSnapshot {
    pub nodes: Vec<GraphNodeView>,
    pub edges: Vec<GraphEdgeView>,
    pub width: f32,
    pub height: f32,
}

impl Default for GraphSnapshot {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            width: 440.0,
            height: 360.0,
        }
    }
}

/// 把已抽取的纯文本切分为带位置标签的片段。
///
/// 返回的片段 `text` 拼接后与输入 `text` **严格相等**（含换行），
/// 保证预览区改成按段渲染后不丢字。
pub fn segment_text(text: &str, format: Format) -> Vec<TextSegment> {
    if text.is_empty() {
        return Vec::new();
    }
    // split_inclusive 保留行尾换行，切片拼接才不会丢换行。
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let groups = group_lines(&lines, format);

    groups
        .into_iter()
        .enumerate()
        .map(|(index, (start, end, label, kind))| TextSegment {
            index,
            label,
            kind: kind.to_string(),
            text: lines[start..end].concat(),
        })
        .collect()
}

/// 返回 (起始行, 结束行, 标签, 类型) 的分组。
fn group_lines(
    lines: &[&str],
    format: Format,
) -> Vec<(usize, usize, String, &'static str)> {
    let mut out: Vec<(usize, usize, String, &'static str)> = Vec::new();

    match format {
        Format::PowerPoint => {
            // pptx 抽取器用 "--- ppt/slides/slideN.xml ---" 分隔每页。
            for (i, line) in lines.iter().enumerate() {
                if let Some(page) = slide_number(line) {
                    out.push((i, i + 1, format!("第 {page} 页"), "page"));
                } else if line.trim().is_empty() {
                    if let Some(last) = out.last_mut() {
                        last.1 = i + 1;
                    }
                } else if let Some(last) = out.last_mut() {
                    last.1 = i + 1;
                } else {
                    out.push((i, i + 1, "第 1 页".to_string(), "page"));
                }
            }
        }
        Format::Markdown => {
            let mut start: Option<usize> = None;
            let mut para = 0usize;
            for (i, line) in lines.iter().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    if let Some(s) = start.take() {
                        out.push((s, i, String::new(), "paragraph"));
                    }
                    continue;
                }
                if trimmed.starts_with('#') {
                    if let Some(s) = start.take() {
                        out.push((s, i, String::new(), "paragraph"));
                    }
                    out.push((i, i + 1, format!("标题 · {}", trim_heading(trimmed)), "heading"));
                    continue;
                }
                if start.is_none() {
                    start = Some(i);
                }
            }
            if let Some(s) = start {
                out.push((s, lines.len(), String::new(), "paragraph"));
            }
            // 段落统一编号（标题不占段号）
            for g in out.iter_mut() {
                if g.3 == "paragraph" {
                    para += 1;
                    g.2 = format!("第 {para} 段");
                }
            }
        }
        Format::Excel => {
            // 抽取器每行格式为 "{sheet}: 单元格文本 | ..."
            let mut row = 0usize;
            let mut sheet = String::new();
            for (i, line) in lines.iter().enumerate() {
                if line.trim().is_empty() {
                    if let Some(last) = out.last_mut() {
                        last.1 = i + 1;
                    }
                    continue;
                }
                if let Some((name, _)) = line.split_once(": ") {
                    if !name.trim().is_empty() && name.len() < 64 {
                        sheet = name.trim().to_string();
                    }
                }
                row += 1;
                let label = if sheet.is_empty() {
                    format!("第 {row} 行")
                } else {
                    format!("{sheet} · 第 {row} 行")
                };
                out.push((i, i + 1, label, "row"));
            }
        }
        Format::Word | Format::Visio | Format::Drawio => {
            let (unit, kind) = match format {
                Format::Word => ("第 {} 段", "paragraph"),
                Format::Visio => ("第 {} 页", "page"),
                _ => ("节点 {}", "node"),
            };
            let mut n = 0usize;
            for (i, line) in lines.iter().enumerate() {
                if line.trim().is_empty() {
                    if let Some(last) = out.last_mut() {
                        last.1 = i + 1;
                    }
                    continue;
                }
                n += 1;
                let label = unit.replace("{}", &n.to_string());
                out.push((i, i + 1, label, kind));
            }
        }
    }

    seal_groups(out, lines.len())
}

/// 收尾：把所有未被任何片段覆盖的行并入相邻片段，保证拼接严格等于原文。
///
/// 各格式的分隔行（Markdown 空行、PPT 页眉、Excel 空行等）容易留下空隙，
/// 与其在每个分支里小心翼翼地处理，不如统一在这里封口。
fn seal_groups(
    mut groups: Vec<(usize, usize, String, &'static str)>,
    total: usize,
) -> Vec<(usize, usize, String, &'static str)> {
    if groups.is_empty() {
        return vec![(0, total, "全文".to_string(), "paragraph")];
    }
    groups[0].0 = 0;
    for i in 1..groups.len() {
        if groups[i].0 > groups[i - 1].1 {
            // 空隙并入前一段（通常是空行 / 分隔行）
            groups[i - 1].1 = groups[i].0;
        }
        if groups[i].0 < groups[i - 1].1 {
            groups[i].0 = groups[i - 1].1;
        }
    }
    groups.last_mut().expect("groups 非空").1 = total;
    groups
}

fn trim_heading(s: &str) -> String {
    let t = s.trim_start_matches('#').trim();
    let chars: Vec<char> = t.chars().collect();
    let short: String = chars.iter().take(24).collect();
    if chars.len() > 24 {
        format!("{short}…")
    } else {
        short
    }
}

fn slide_number(line: &str) -> Option<u32> {
    let t = line.trim();
    if !t.starts_with("---") || !t.ends_with("---") {
        return None;
    }
    let inner = t.trim_matches('-').trim();
    let name = inner.rsplit('/').next()?;
    let digits: String = name
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// 在片段中定位查询词。
///
/// 中文查询可能被分词，因此按「完整查询 → 最长子串 → 单字」逐级降级，
/// 忽略大小写；返回第一个命中的片段。
pub fn locate_in_segments(segments: &[TextSegment], query: &str) -> Option<HitLocation> {
    let q = query.trim();
    if q.is_empty() {
        return None;
    }
    let candidates = query_candidates(q);
    // 正文优先、标题兜底：查询词常常也出现在文档标题里，若按原始顺序命中，
    // 会永远只跳到标题块，而不是真正讨论该词的段落（PRD 要的是原文位置）。
    let ordered: Vec<&TextSegment> = segments
        .iter()
        .filter(|s| s.kind != "heading")
        .chain(segments.iter().filter(|s| s.kind == "heading"))
        .collect();
    for seg in ordered {
        let lower = seg.text.to_lowercase();
        for cand in &candidates {
            if let Some(byte_pos) = lower.find(&cand.to_lowercase()) {
                let char_pos = lower[..byte_pos].chars().count();
                return Some(HitLocation {
                    label: seg.label.clone(),
                    kind: seg.kind.clone(),
                    segment_index: seg.index,
                    matched: cand.clone(),
                    snippet: make_location_snippet(&seg.text, char_pos, cand.chars().count()),
                });
            }
        }
    }
    None
}

fn query_candidates(query: &str) -> Vec<String> {
    let mut out = vec![query.to_string()];
    // 按空白与中英文标点切成词，长词优先
    let mut words: Vec<String> = query
        .split(|c: char| c.is_whitespace() || c.is_ascii_punctuation() || is_cjk_punct(c))
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    words.sort_by(|a, b| b.chars().count().cmp(&a.chars().count()));
    for w in words {
        if w != query && !out.contains(&w) {
            out.push(w);
        }
    }
    // 单字兜底（中文查询至少能定位到某个含该字的片段）
    for c in query.chars() {
        if !c.is_whitespace() {
            let s = c.to_string();
            if !out.contains(&s) {
                out.push(s);
            }
        }
    }
    out
}

fn is_cjk_punct(c: char) -> bool {
    matches!(c, '，' | '。' | '、' | '；' | '：' | '？' | '！' | '（' | '）' | '《' | '》')
}

fn make_location_snippet(text: &str, hit_char: usize, match_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let start = hit_char.saturating_sub(30);
    let end = (hit_char + match_chars + 60).min(chars.len());
    let body: String = chars[start..end].iter().collect();
    let body = body.trim().replace('\n', " ");
    if start > 0 {
        format!("…{body}")
    } else {
        body
    }
}

/// 由文档列表与双链边构建可直接渲染的图谱快照。
///
/// - 节点按路径排序，布局为确定性环形（高度数节点更靠中心）；
/// - 悬空目标（不在文档列表里）与自环会被忽略；
/// - 相同边去重，避免重复链接把图谱画粗。
pub fn build_graph_snapshot(
    nodes: &[(String, String)],
    links: &[(String, String)],
) -> GraphSnapshot {
    let mut sorted: Vec<(String, String)> = nodes.to_vec();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    sorted.dedup_by(|a, b| a.0 == b.0);

    let index: std::collections::HashMap<&str, usize> = sorted
        .iter()
        .enumerate()
        .map(|(i, (p, _))| (p.as_str(), i))
        .collect();

    let mut edges: Vec<GraphEdgeView> = Vec::new();
    let mut degree = vec![0usize; sorted.len()];
    for (from, to) in links {
        let (Some(&a), Some(&b)) = (index.get(from.as_str()), index.get(to.as_str())) else {
            continue;
        };
        if a == b {
            continue;
        }
        let edge = GraphEdgeView { from: a, to: b };
        if edges.contains(&edge) {
            continue;
        }
        edges.push(edge);
        degree[a] += 1;
        degree[b] += 1;
    }

    let width = 440.0f32;
    let height = 360.0f32;
    let cx = width / 2.0;
    let cy = height / 2.0;
    let n = sorted.len().max(1);
    let views: Vec<GraphNodeView> = sorted
        .iter()
        .enumerate()
        .map(|(i, (path, title))| {
            let angle = -std::f32::consts::FRAC_PI_2
                + 2.0 * std::f32::consts::PI * (i as f32) / (n as f32);
            // 度数越高半径越小 → 枢纽节点靠中心，视觉上形成簇
            let radius = 150.0 - 55.0 * (degree[i].min(4) as f32 / 4.0);
            GraphNodeView {
                path: path.clone(),
                title: title.clone(),
                x: cx + radius * angle.cos(),
                y: cy + radius * angle.sin(),
                degree: degree[i],
            }
        })
        .collect();

    GraphSnapshot {
        nodes: views,
        edges,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_concatenate_back_to_original_text() {
        let samples = [
            "# 标题\n\n第一段文字。\n第二段文字。\n\n## 小节\n结尾。\n",
            "Sheet1: A1 | B1\nSheet1: A2 | B2\n",
            "--- ppt/slides/slide1.xml ---\n第一页文字\n--- ppt/slides/slide2.xml ---\n第二页文字\n",
            "段落一\n段落二\n",
            "节点甲\n节点乙\n",
        ];
        let formats = [
            Format::Markdown,
            Format::Excel,
            Format::PowerPoint,
            Format::Word,
            Format::Drawio,
        ];
        for (text, format) in samples.iter().zip(formats.iter()) {
            let segs = segment_text(text, *format);
            assert!(!segs.is_empty(), "{format:?} 应产生片段");
            let joined: String = segs.iter().map(|s| s.text.as_str()).collect();
            assert_eq!(joined, *text, "{format:?} 片段拼接必须无损");
            assert!(segs.iter().all(|s| !s.label.is_empty()));
        }
    }

    #[test]
    fn format_specific_labels() {
        let md = segment_text("# 模流分析\n\n正文段落。\n", Format::Markdown);
        assert_eq!(md[0].kind, "heading");
        assert!(md[0].label.contains("模流分析"));
        assert!(md.iter().any(|s| s.label == "第 1 段"));

        let xls = segment_text("Sheet1: 浇口 | 位置\n", Format::Excel);
        assert_eq!(xls[0].kind, "row");
        assert!(xls[0].label.contains("Sheet1"), "{}", xls[0].label);

        let ppt = segment_text(
            "--- ppt/slides/slide3.xml ---\n第三页\n",
            Format::PowerPoint,
        );
        assert_eq!(ppt[0].kind, "page");
        assert!(ppt[0].label.contains("3"), "{}", ppt[0].label);
    }

    #[test]
    fn locate_finds_correct_segment() {
        let text = "第一段讲工艺。\n\n第二段讲模流分析与浇口位置。\n\n第三段收尾。\n";
        let segs = segment_text(text, Format::Markdown);
        let hit = locate_in_segments(&segs, "浇口").expect("应命中");
        assert!(hit.label.contains("第 2 段"), "{}", hit.label);
        assert!(hit.snippet.contains("浇口"));
        assert_eq!(segs[hit.segment_index].label, hit.label);
    }

    #[test]
    fn locate_falls_back_to_shorter_query_parts() {
        let segs = segment_text("模流分析用于评估熔接线。\n", Format::Markdown);
        // 完整词不连续出现时，仍应能靠词片段定位
        let hit = locate_in_segments(&segs, "模流分析 熔接线").expect("应能降级命中");
        assert_eq!(hit.segment_index, 0);
    }

    #[test]
    fn graph_snapshot_is_deterministic_and_skips_dangling_edges() {
        let nodes = vec![
            ("b.md".to_string(), "B".to_string()),
            ("a.md".to_string(), "A".to_string()),
            ("c.md".to_string(), "C".to_string()),
        ];
        let links = vec![
            ("a.md".to_string(), "b.md".to_string()),
            ("a.md".to_string(), "b.md".to_string()), // 重复边
            ("b.md".to_string(), "missing.md".to_string()), // 悬空目标
            ("c.md".to_string(), "c.md".to_string()), // 自环
        ];
        let g = build_graph_snapshot(&nodes, &links);
        assert_eq!(g.nodes.len(), 3);
        assert_eq!(g.nodes[0].path, "a.md", "节点应按路径排序");
        assert_eq!(g.edges.len(), 1, "重复 / 悬空 / 自环边应被剔除");
        assert_eq!(g.edges[0].from, 0);
        assert_eq!(g.edges[0].to, 1);
        assert_eq!(g, build_graph_snapshot(&nodes, &links), "布局必须确定性");
    }

    #[test]
    fn locate_prefers_body_over_heading() {
        let segs = segment_text(
            "# 模流分析入门\n\n模流分析用于评估浇口位置。\n",
            Format::Markdown,
        );
        let hit = locate_in_segments(&segs, "模流分析").expect("应命中");
        assert_ne!(hit.kind, "heading", "标题也含查询词时应优先定位正文段落");
        assert!(hit.label.contains("第 1 段"), "实际标签 {}", hit.label);
    }
}
