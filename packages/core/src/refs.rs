//! F-06 跨格式引用：在文档中引用其他格式文档的**指定片段**。
//!
//! ## 语法
//!
//! `@相对路径#选择器`，选择器对应 [`crate::locate::TextSegment::label`]：
//!
//! - `@03-台账/模具台账.xlsx#第2行` → Excel 第 2 行
//! - `@05-方案/项目方案.pptx#第1页` → PPT 第 1 页
//! - `@02-图示/评审流程.drawio#节点3` → Drawio 第 3 个节点
//! - `@01-笔记/工艺参数基线.md#2` → 纯数字按 1-based 段序号
//!
//! 选择器按「去空白后的标签包含匹配」解析，因此 `第2行` 能命中
//! `台账 · 第 2 行`；标签匹配不到时再退回选择器里的数字序号。
//!
//! ## 为什么不动 `parse.rs`
//!
//! 既有 `extract_links()` 的 token 字符集不含 `#`，遇到新语法会自然截断到
//! 扩展名，得到**正确的目标路径**，因此带选择器的引用仍会正常进入双链 /
//! 反链 / 图谱，不会产生悬空目标。F-06 只额外解析 `#` 之后的选择器，
//! 与既有索引、双链偏移完全解耦；不带 `#` 的 `@引用` 行为完全不变。
//!
//! 本模块只依赖 `serde` 与纯 Rust 的 `locate` / `format`，wasm32 可编译。

use serde::{Deserialize, Serialize};

use crate::format::Format;
use crate::locate::{segment_text, TextSegment};

/// 从正文里解析出的引用（尚未解析目标片段）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// 原文中的完整 token，例如 `@03-台账/模具台账.xlsx#第2行`。
    pub raw: String,
    /// 目标路径（不含 `#选择器`）。
    pub target: String,
    /// 选择器原文（不含 `#`）。
    pub selector: String,
    /// token 在原文中的字节偏移。
    pub offset: usize,
}

/// 可直接交给 UI 渲染的引用视图。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReferenceView {
    /// 完整 token，UI 用它把正文切成「文本 + 卡片」。
    pub raw: String,
    /// 解析后的目标文档相对路径（解析失败时等于原 target）。
    pub target_path: String,
    /// 选择器原文。
    pub selector: String,
    /// 命中片段的位置标签，例如「台账 · 第 2 行」。
    pub label: String,
    /// 源片段内容快照（截断，供卡片内联显示）。
    pub snippet: String,
    /// 目标文档中的片段下标，用于点选跳转。
    pub segment_index: usize,
    /// 源片段内容指纹；源文件更新后会变化，用于「标记变更」。
    pub source_hash: u64,
    /// 目标与选择器是否都成功解析。
    pub resolved: bool,
    /// token 在源文档中的字节偏移。
    pub offset: usize,
}

impl ReferenceView {
    fn unresolved(r: &Reference) -> Self {
        Self {
            raw: r.raw.clone(),
            target_path: r.target.clone(),
            selector: r.selector.clone(),
            label: String::new(),
            snippet: String::new(),
            segment_index: 0,
            source_hash: 0,
            resolved: false,
            offset: r.offset,
        }
    }
}

/// 正文渲染片段：普通文本，或一条内联引用卡片。
#[derive(Debug, Clone, PartialEq)]
pub enum TextChunk {
    Plain(String),
    Reference(ReferenceView),
}

/// 从文本中抽取**带 `#选择器`** 的 @ 引用。
///
/// 不带选择器的 `@文件` / `[[双链]]` 一律忽略——它们归既有双链系统处理，
/// F-06 只负责「引用处显示源片段内容」。
pub fn parse_references(text: &str) -> Vec<Reference> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'@' {
            i += 1;
            continue;
        }
        // '@' 前紧邻字母/数字时视为邮箱的一部分（与 parse.rs 保持一致）。
        if i > 0
            && text[..i]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '.' || c == '_' || c == '-')
        {
            i += 1;
            continue;
        }

        let rest = &text[i + 1..];
        // 目标部分：到 '#' 或第一个非 token 字符为止。
        let stop = rest.find(|c: char| !is_target_char(c)).unwrap_or(rest.len());
        let target = &rest[..stop];
        if !has_known_extension(target) || !rest[stop..].starts_with('#') {
            i += 1;
            continue;
        }

        // 选择器部分：中文 / 字母 / 数字，不含空白与标点。
        let after_hash = &rest[stop + 1..];
        let sel_len = after_hash
            .find(|c: char| !is_selector_char(c))
            .unwrap_or(after_hash.len());
        let selector = &after_hash[..sel_len];
        if selector.is_empty() {
            i += 1;
            continue;
        }

        let raw_len = 1 + stop + 1 + sel_len; // '@' + target + '#' + selector
        out.push(Reference {
            raw: text[i..i + raw_len].to_string(),
            target: target.to_string(),
            selector: selector.to_string(),
            offset: i,
        });
        i += raw_len;
    }
    out
}

fn is_target_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '\\')
}

fn is_selector_char(c: char) -> bool {
    c.is_alphanumeric()
}

fn has_known_extension(target: &str) -> bool {
    let Some((_, ext)) = target.rsplit_once('.') else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "md" | "markdown" | "xlsx" | "pptx" | "docx" | "vsdx" | "drawio"
    )
}

/// 内容指纹（FNV-1a，纯 Rust、确定性、跨端一致）。
pub fn hash_text(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// 在片段列表里解析选择器，返回片段下标。
pub fn match_selector(segments: &[TextSegment], selector: &str) -> Option<usize> {
    let s = compact(selector);
    if s.is_empty() {
        return None;
    }
    // 1) 纯数字 = 1-based 段序号
    if let Ok(n) = s.parse::<usize>() {
        return (n >= 1 && n <= segments.len()).then_some(n - 1);
    }
    // 2) 标签包含匹配（双方都去空白）
    if let Some(seg) = segments.iter().find(|g| compact(&g.label).contains(&s)) {
        return Some(seg.index);
    }
    // 3) 选择器里的数字 → 序号兜底
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    if let Ok(n) = digits.parse::<usize>() {
        return (n >= 1 && n <= segments.len()).then_some(n - 1);
    }
    None
}

/// 目标路径解析：精确路径 → 路径后缀 → 文件名（不含扩展名）。
fn resolve_target<'a>(
    target: &str,
    docs: &'a [(String, String, Format)],
) -> Option<&'a (String, String, Format)> {
    if let Some(d) = docs.iter().find(|(p, _, _)| p == target) {
        return Some(d);
    }
    if let Some(d) = docs
        .iter()
        .find(|(p, _, _)| p.ends_with(target) || target.ends_with(p.as_str()))
    {
        return Some(d);
    }
    let stem = target.rsplit('/').next().unwrap_or(target);
    let stem = stem.rsplit_once('.').map(|(s, _)| s).unwrap_or(stem);
    docs.iter().find(|(p, _, _)| {
        let name = p.rsplit('/').next().unwrap_or(p);
        let base = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
        base == stem
    })
}

/// 生成一份文档的全部引用视图。
///
/// `docs` 为全库 `(相对路径, 抽取后的纯文本, 格式)`，用于解析目标片段。
/// 构建期（导出索引）与运行期（Desktop 实时）共用这一套语义。
pub fn build_reference_views(
    source_text: &str,
    docs: &[(String, String, Format)],
) -> Vec<ReferenceView> {
    parse_references(source_text)
        .into_iter()
        .map(|r| {
            let Some((path, text, format)) = resolve_target(&r.target, docs) else {
                return ReferenceView::unresolved(&r);
            };
            let segments = segment_text(text, *format);
            let Some(idx) = match_selector(&segments, &r.selector) else {
                return ReferenceView::unresolved(&r);
            };
            let seg = &segments[idx];
            let snippet: String = seg.text.chars().take(120).collect();
            ReferenceView {
                raw: r.raw,
                target_path: path.clone(),
                selector: r.selector,
                label: seg.label.clone(),
                snippet: snippet.trim().to_string(),
                segment_index: seg.index,
                source_hash: hash_text(&seg.text),
                resolved: true,
                offset: r.offset,
            }
        })
        .collect()
}

/// 把一段正文按引用 token 切成可顺序渲染的片段。
///
/// 只按解析阶段记录的**字节偏移**切分，不做二次字符串查找，
/// 因此同一 token 重复出现时也不会错位；切片均落在 UTF-8 字符边界。
pub fn render_chunks(text: &str, views: &[ReferenceView]) -> Vec<TextChunk> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    for v in views {
        let end = v.offset.saturating_add(v.raw.len());
        if v.offset < cursor
            || end > text.len()
            || !text.is_char_boundary(v.offset)
            || !text.is_char_boundary(end)
        {
            continue;
        }
        if v.offset > cursor {
            out.push(TextChunk::Plain(text[cursor..v.offset].to_string()));
        }
        out.push(TextChunk::Reference(v.clone()));
        cursor = end;
    }
    if cursor < text.len() {
        out.push(TextChunk::Plain(text[cursor..].to_string()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_docs() -> Vec<(String, String, Format)> {
        vec![
            (
                "03-台账/模具台账.xlsx".into(),
                "台账: 零件号 | 模具编号\n台账: PN-1024 | MDL-08\n台账: PN-2048 | MDL-11\n"
                    .into(),
                Format::Excel,
            ),
            (
                "05-方案/项目方案.pptx".into(),
                "--- ppt/slides/slide1.xml ---\n项目方案\n模流分析结论\n".into(),
                Format::PowerPoint,
            ),
        ]
    }

    #[test]
    fn parses_only_refs_with_selectors() {
        let text = "见 @03-台账/模具台账.xlsx#第2行 与 @05-方案/项目方案.pptx#第1页；\n\
                    普通 @工艺参数基线.md 不算，邮箱 someone@example.com 也不算。";
        let refs = parse_references(text);
        assert_eq!(refs.len(), 2, "got {refs:?}");
        assert_eq!(refs[0].target, "03-台账/模具台账.xlsx");
        assert_eq!(refs[0].selector, "第2行");
        assert_eq!(refs[1].selector, "第1页");
        assert_eq!(&text[refs[0].offset..refs[0].offset + refs[0].raw.len()], refs[0].raw);
    }

    #[test]
    fn selector_matches_labels_and_numeric_index() {
        let docs = sample_docs();
        let excel = segment_text(&docs[0].1, Format::Excel);
        assert_eq!(match_selector(&excel, "第2行"), Some(1));
        assert_eq!(match_selector(&excel, "2"), Some(1));
        let ppt = segment_text(&docs[1].1, Format::PowerPoint);
        assert_eq!(match_selector(&ppt, "第1页"), Some(0));
        assert_eq!(match_selector(&ppt, "不存在的片段"), None);
    }

    #[test]
    fn views_resolve_real_segments_and_detect_source_change() {
        let docs = sample_docs();
        let source = "台账行：@03-台账/模具台账.xlsx#第2行\n方案页：@05-方案/项目方案.pptx#第1页\n";
        let views = build_reference_views(source, &docs);
        assert_eq!(views.len(), 2);
        assert!(views.iter().all(|v| v.resolved), "got {views:#?}");
        assert!(views[0].snippet.contains("PN-1024"), "{}", views[0].snippet);
        assert!(views[0].label.contains("第 2 行"), "{}", views[0].label);

        // 源文档第 2 行内容变化 → 指纹必须变化（PRD「标记变更」）
        let mut changed = docs.clone();
        changed[0].1 = changed[0].1.replace("PN-1024", "PN-9999");
        let after = build_reference_views(source, &changed);
        assert_ne!(views[0].source_hash, after[0].source_hash);
        assert!(after[0].snippet.contains("PN-9999"));
    }

    #[test]
    fn unresolved_target_or_selector_is_reported_not_panicking() {
        let views = build_reference_views(
            "缺目标 @不存在.xlsx#第1行，缺片段 @03-台账/模具台账.xlsx#第99行",
            &sample_docs(),
        );
        assert_eq!(views.len(), 2);
        assert!(views.iter().all(|v| !v.resolved), "got {views:#?}");
    }

    #[test]
    fn render_chunks_is_lossless() {
        let docs = sample_docs();
        let source = "台账行：@03-台账/模具台账.xlsx#第2行\n方案页：@05-方案/项目方案.pptx#第1页\n结尾";
        let views = build_reference_views(source, &docs);
        let chunks = render_chunks(source, &views);
        let rebuilt: String = chunks
            .iter()
            .map(|c| match c {
                TextChunk::Plain(s) => s.as_str(),
                TextChunk::Reference(v) => v.raw.as_str(),
            })
            .collect();
        assert_eq!(rebuilt, source, "切分后必须能无损拼回原文");
        assert_eq!(
            chunks.iter().filter(|c| matches!(c, TextChunk::Reference(_))).count(),
            2
        );
    }

    #[test]
    fn hash_is_stable_across_calls_and_sensitive_to_content() {
        assert_eq!(hash_text("冷却水路"), hash_text("冷却水路"));
        assert_ne!(hash_text("冷却水路"), hash_text("冷却水道"));
    }
}
