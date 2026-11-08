//! 检索结果精确定位（PRD §7.1）与双链图谱快照（F-10）。
//!
//! 本模块只依赖 `serde` 与 [`crate::format::Format`]，**无任何原生 / C 依赖**，
//! 因此 Web（wasm32）与 Desktop / Mobile 三端都能编译。
//!
//! ## 图谱坐标：构建期预计算 vs 运行期计算
//!
//! 力导向布局引擎（`crate::graph_layout`）**只在 native（构建期）参与编译**：
//! wasm32 目标不携带它，Web 端的图谱坐标由构建期算好后随预构建索引产物发布
//! （见 [`crate::prebuilt::PrebuiltIndex::graph_positions`]）。
//!
//! 非 native 构建下 [`build_graph_snapshot_with_positions`] 拿不到预计算坐标时
//! （schema v1 旧产物 / 远端热替换拿到旧产物），回退到**确定性环形布局**：
//! 它不含浮点迭代引擎、体积极小，保证图谱面板不白屏、不 panic。
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
// 布局引擎只在 native（构建期）侧参与编译：wasm 产物不携带它，
// 图谱坐标改为构建期预计算后随索引产物发布（见 `PrebuiltIndex::graph_positions`）。
#[cfg(feature = "native")]
use crate::graph_layout::{force_directed_layout, LayoutConfig, LayoutNode, LayoutPoint};

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

/// 图谱画布宽度。
///
/// 旧的环形布局用 440×360；力导向布局需要更大的活动空间，
/// 否则斥力项会把节点挤在中心一小团里，因此统一放大到 720×560。
pub const GRAPH_CANVAS_WIDTH: f32 = 720.0;
/// 图谱画布高度（与 [`GRAPH_CANVAS_WIDTH`] 配套）。
pub const GRAPH_CANVAS_HEIGHT: f32 = 560.0;
/// 力导向布局的固定随机种子。
///
/// 固定 seed 是**确定性**的前提：布局模块内部的随机性只来自自带的
/// splitmix64 PRNG，种子固定 ⇒ 同一输入产出逐字节相同的坐标。
#[cfg(feature = "native")]
const GRAPH_LAYOUT_SEED: u64 = 0x5EED_1A7E_C0DE_1234;
/// 力导向布局的迭代轮数（与布局模块默认值同量级）。
#[cfg(feature = "native")]
const GRAPH_LAYOUT_ITERATIONS: usize = 240;

/// 图谱布局方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GraphLayout {
    /// 确定性环形布局：节点均分在一个圆周上，度数越高半径越小（枢纽靠中心）。
    ///
    /// 优点是坐标可手算、视觉稳定；缺点是表达不了「簇」。
    Ring,
    /// 力导向布局（Fruchterman–Reingold 变体，见 `crate::graph_layout`）。
    ///
    /// - **分簇**：同一文档格式的节点聚成一族（和 UI 的按格式着色一致）；
    /// - **枢纽居中**：节点权重取度数，度数越高越靠近画布中心；
    /// - **确定性**：同一输入 + 同一参数 ⇒ 逐字节相同的坐标。
    ///
    /// 注意：布局引擎只在 `native`（构建期）参与编译，非 native 构建下
    /// 该选项退化为**环形布局**（见 [`build_graph_snapshot_with`]）。
    #[default]
    ForceDirected,
}

/// 图谱节点坐标（构建期预计算，随预构建索引产物发布）。
///
/// Web（wasm32）端不携带浮点布局引擎，坐标由 native 侧在**构建期**算好后
/// 随索引产物一起打包；运行期只做查表，不做任何迭代。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphPosition {
    /// 文档相对路径（与 [`PrebuiltDoc`](crate::prebuilt::PrebuiltDoc) 的 `path` 一致）。
    pub path: String,
    /// 画布横坐标。
    pub x: f32,
    /// 画布纵坐标。
    pub y: f32,
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
            width: GRAPH_CANVAS_WIDTH,
            height: GRAPH_CANVAS_HEIGHT,
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

/// 由文档列表与双链边构建可直接渲染的图谱快照（**力导向布局**）。
///
/// 等价于 [`build_graph_snapshot_with`] 且 `layout = GraphLayout::ForceDirected`。
/// 想要旧的环形布局请显式调用 [`build_graph_snapshot_with`]。
pub fn build_graph_snapshot(
    nodes: &[(String, String)],
    links: &[(String, String)],
) -> GraphSnapshot {
    build_graph_snapshot_with(nodes, links, GraphLayout::default())
}

/// 由文档列表与双链边构建图谱快照，可选择布局方式。
///
/// - 节点按路径排序并按路径去重；
/// - 悬空目标（不在文档列表里）与自环会被忽略；
/// - 相同边去重，避免重复链接把图谱画粗；
/// - 坐标由 [`GraphLayout`] 决定，两种布局都保证坐标落在画布内且非 `NaN`。
///
/// 画布固定为 [`GRAPH_CANVAS_WIDTH`] × [`GRAPH_CANVAS_HEIGHT`]，
/// 切换布局不会改变画布尺寸（UI 侧无需重新适配视口）。
///
/// 非 native（wasm32）构建不携带力导向引擎，此时 `ForceDirected` 退化为
/// 环形布局（见模块文档「图谱坐标：构建期预计算 vs 运行期计算」）。
pub fn build_graph_snapshot_with(
    nodes: &[(String, String)],
    links: &[(String, String)],
    layout: GraphLayout,
) -> GraphSnapshot {
    let topo = graph_topology(nodes, links);
    let views = match layout {
        GraphLayout::Ring => ring_positions(&topo.sorted, &topo.degree),
        GraphLayout::ForceDirected => force_positions(&topo.sorted, &topo.degree, &topo.edges),
    };
    GraphSnapshot {
        nodes: views,
        edges: topo.edges,
        width: GRAPH_CANVAS_WIDTH,
        height: GRAPH_CANVAS_HEIGHT,
    }
}

/// 由文档列表、双链边与**构建期预计算的坐标**构建图谱快照。
///
/// 这是 Web（wasm32）端的正式入口：坐标随预构建索引产物一起发布
/// （见 [`crate::prebuilt::PrebuiltIndex::graph_positions`]），
/// 运行期只做查表，**不再执行 240 轮浮点迭代**。
///
/// 容错策略：
/// - 坐标表为空（schema v1 旧产物 / 远端热替换拿到旧产物）⇒ 整体退回环形布局；
/// - 个别节点缺坐标或坐标非法（`NaN` / 越界）⇒ 该节点单独退回环形布局。
///
/// 拓扑（节点 / 边 / 度数）仍在本函数现算：它只涉及字符串比较与查表，
/// 代价极低，且保证图谱结构永远与当前产物一致。
pub fn build_graph_snapshot_with_positions(
    nodes: &[(String, String)],
    links: &[(String, String)],
    positions: &[GraphPosition],
) -> GraphSnapshot {
    let topo = graph_topology(nodes, links);
    // 基线坐标：native 走力导向布局，非 native 走环形布局（体积小、无迭代）。
    let mut views = base_positions(&topo.sorted, &topo.degree, &topo.edges);
    if !positions.is_empty() {
        let table: std::collections::HashMap<&str, (f32, f32)> = positions
            .iter()
            .map(|p| (p.path.as_str(), (p.x, p.y)))
            .collect();
        for (i, (path, _)) in topo.sorted.iter().enumerate() {
            let Some(&(x, y)) = table.get(path.as_str()) else {
                continue;
            };
            if !x.is_finite() || !y.is_finite() {
                continue;
            }
            views[i].x = x.clamp(0.0, GRAPH_CANVAS_WIDTH);
            views[i].y = y.clamp(0.0, GRAPH_CANVAS_HEIGHT);
        }
    }
    GraphSnapshot {
        nodes: views,
        edges: topo.edges,
        width: GRAPH_CANVAS_WIDTH,
        height: GRAPH_CANVAS_HEIGHT,
    }
}

/// 基线坐标：native 用力导向布局，非 native 用环形布局。
#[cfg(feature = "native")]
fn base_positions(
    sorted: &[(String, String)],
    degree: &[usize],
    edges: &[GraphEdgeView],
) -> Vec<GraphNodeView> {
    force_positions(sorted, degree, edges)
}

/// 基线坐标：非 native 构建没有布局引擎，退回确定性环形布局。
#[cfg(not(feature = "native"))]
fn base_positions(
    sorted: &[(String, String)],
    degree: &[usize],
    _edges: &[GraphEdgeView],
) -> Vec<GraphNodeView> {
    ring_positions(sorted, degree)
}

/// 图谱拓扑：与「坐标怎么算」无关的那一半。
///
/// 把拓扑与坐标拆开是「构建期预计算」的前提：拓扑可以任意端现算
/// （纯字符串处理，微秒级），坐标则要么来自构建期产物，要么退化成环形布局。
struct GraphTopology {
    /// 按路径排序并按路径去重后的节点。
    sorted: Vec<(String, String)>,
    /// 去重并剔除悬空目标与自环后的边。
    edges: Vec<GraphEdgeView>,
    /// 每个节点的度数（入边 + 出边）。
    degree: Vec<usize>,
}

/// 计算图谱拓扑：排序去重节点、剔除悬空 / 自环 / 重复边、统计度数。
fn graph_topology(
    nodes: &[(String, String)],
    links: &[(String, String)],
) -> GraphTopology {
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

    GraphTopology {
        sorted,
        edges,
        degree,
    }
}

/// 确定性环形布局：节点均分在圆周上，度数越高半径越小（枢纽靠中心）。
fn ring_positions(sorted: &[(String, String)], degree: &[usize]) -> Vec<GraphNodeView> {
    let cx = GRAPH_CANVAS_WIDTH / 2.0;
    let cy = GRAPH_CANVAS_HEIGHT / 2.0;
    let n = sorted.len().max(1);
    // 最大半径取画布短边的 42%，给节点标签留出边距。
    let max_radius = GRAPH_CANVAS_WIDTH.min(GRAPH_CANVAS_HEIGHT) * 0.42;
    sorted
        .iter()
        .enumerate()
        .map(|(i, (path, title))| {
            let angle = -std::f32::consts::FRAC_PI_2
                + 2.0 * std::f32::consts::PI * (i as f32) / (n as f32);
            // 度数越高半径越小 → 枢纽节点靠中心，视觉上形成簇
            let radius = max_radius * (1.0 - 0.37 * (degree[i].min(4) as f32 / 4.0));
            GraphNodeView {
                path: path.clone(),
                title: title.clone(),
                x: (cx + radius * angle.cos()).clamp(0.0, GRAPH_CANVAS_WIDTH),
                y: (cy + radius * angle.sin()).clamp(0.0, GRAPH_CANVAS_HEIGHT),
                degree: degree[i],
            }
        })
        .collect()
}

/// 力导向布局：按文档格式分簇，度数作为节点权重（枢纽更靠中心）。
///
/// 只在 **native（构建期）** 编译：布局引擎（浮点迭代 + Barnes–Hut 网格）
/// 体积不小，wasm32 产物不携带它。
#[cfg(feature = "native")]
fn force_positions(
    sorted: &[(String, String)],
    degree: &[usize],
    edges: &[GraphEdgeView],
) -> Vec<GraphNodeView> {
    let layout_nodes: Vec<LayoutNode> = sorted
        .iter()
        .enumerate()
        .map(|(i, (path, _))| LayoutNode {
            index: i,
            // 权重 = 度数：度数越高，簇内初始排布越靠核心，且居中力越大。
            weight: degree[i] as f32,
            // 分簇标识：同一格式的文档聚成一族。
            group: format_group(path),
        })
        .collect();
    let layout_edges: Vec<(usize, usize)> = edges.iter().map(|e| (e.from, e.to)).collect();
    let config = LayoutConfig {
        width: GRAPH_CANVAS_WIDTH,
        height: GRAPH_CANVAS_HEIGHT,
        iterations: GRAPH_LAYOUT_ITERATIONS,
        repulsion: 1.0,
        spring: 1.0,
        group_attraction: 2.0,
        damping: 0.85,
        seed: GRAPH_LAYOUT_SEED,
    };
    let points = force_directed_layout(&layout_nodes, &layout_edges, &config);
    sorted
        .iter()
        .enumerate()
        .map(|(i, (path, title))| {
            // 布局模块已保证坐标在画布内且非 NaN；这里再兜一层，
            // 保证快照结构在任何输入下都自洽。
            let point = points.get(i).copied().unwrap_or(LayoutPoint {
                x: GRAPH_CANVAS_WIDTH / 2.0,
                y: GRAPH_CANVAS_HEIGHT / 2.0,
            });
            GraphNodeView {
                path: path.clone(),
                title: title.clone(),
                x: point.x.clamp(0.0, GRAPH_CANVAS_WIDTH),
                y: point.y.clamp(0.0, GRAPH_CANVAS_HEIGHT),
                degree: degree[i],
            }
        })
        .collect()
}

/// 非 native 构建下的力导向替身：**退化为确定性环形布局**。
///
/// wasm32 产物不携带浮点布局引擎，Web 端的坐标改由构建期预计算
/// （见 [`build_graph_snapshot_with_positions`]）；这里只保证
/// `GraphLayout::ForceDirected` 这个选项在非 native 构建下仍然可用、
/// 且不会把布局引擎拖进 wasm 产物。
#[cfg(not(feature = "native"))]
fn force_positions(
    sorted: &[(String, String)],
    degree: &[usize],
    _edges: &[GraphEdgeView],
) -> Vec<GraphNodeView> {
    ring_positions(sorted, degree)
}

/// 由路径扩展名推导布局用的分簇标识：同一格式的文档聚成一族。
///
/// 判定与 UI 侧的「按格式着色」一致（都走 [`Format::from_extension`]），
/// 因此图上看得见的簇 = 图例里的颜色。
#[cfg(feature = "native")]
fn format_group(path: &str) -> u32 {
    let ext = path.rsplit('.').next().unwrap_or_default();
    match Format::from_extension(ext) {
        Some(Format::Markdown) => 0,
        Some(Format::Excel) => 1,
        Some(Format::PowerPoint) => 2,
        Some(Format::Word) => 3,
        Some(Format::Visio) => 4,
        Some(Format::Drawio) => 5,
        None => 6,
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

    // ------------------------------------------------------------ 图谱布局

    /// 40 个节点的链式图：够大能触发布局收敛，又不至于让测试变慢。
    fn chain_graph(n: usize) -> (Vec<(String, String)>, Vec<(String, String)>) {
        let nodes = (0..n)
            .map(|i| (format!("doc{i}.md"), format!("D{i}")))
            .collect::<Vec<_>>();
        let links = (0..n.saturating_sub(1))
            .map(|i| (format!("doc{i}.md"), format!("doc{}.md", i + 1)))
            .collect::<Vec<_>>();
        (nodes, links)
    }

    #[test]
    fn force_layout_is_deterministic_bit_for_bit() {
        let (nodes, links) = chain_graph(40);
        let a = build_graph_snapshot_with(&nodes, &links, GraphLayout::ForceDirected);
        let b = build_graph_snapshot_with(&nodes, &links, GraphLayout::ForceDirected);
        assert_eq!(a.nodes.len(), b.nodes.len());
        for (x, y) in a.nodes.iter().zip(b.nodes.iter()) {
            assert_eq!(x.x.to_bits(), y.x.to_bits(), "x 必须逐字节相同");
            assert_eq!(x.y.to_bits(), y.y.to_bits(), "y 必须逐字节相同");
        }
        assert_eq!(a, b, "同一输入必须产出完全一致的快照");
        // 默认入口走的就是力导向布局。
        assert_eq!(build_graph_snapshot(&nodes, &links), a);
    }

    #[test]
    fn force_layout_coordinates_stay_inside_canvas() {
        let (nodes, links) = chain_graph(60);
        let g = build_graph_snapshot_with(&nodes, &links, GraphLayout::ForceDirected);
        assert_eq!(g.width, GRAPH_CANVAS_WIDTH);
        assert_eq!(g.height, GRAPH_CANVAS_HEIGHT);
        for node in &g.nodes {
            assert!(node.x.is_finite() && node.y.is_finite(), "坐标不能是 NaN/Inf");
            assert!(
                (0.0..=g.width).contains(&node.x),
                "x 越界: {}",
                node.x
            );
            assert!(
                (0.0..=g.height).contains(&node.y),
                "y 越界: {}",
                node.y
            );
        }
    }

    #[test]
    fn isolated_nodes_are_still_placed_by_both_layouts() {
        // 没有任何边的孤立节点：不能塌成 NaN，也必须落在画布内。
        let nodes = vec![
            ("a.md".to_string(), "A".to_string()),
            ("b.xlsx".to_string(), "B".to_string()),
            ("c.drawio".to_string(), "C".to_string()),
        ];
        for layout in [GraphLayout::Ring, GraphLayout::ForceDirected] {
            let g = build_graph_snapshot_with(&nodes, &[], layout);
            assert_eq!(g.nodes.len(), 3, "{layout:?}");
            for node in &g.nodes {
                assert!(
                    node.x.is_finite() && node.y.is_finite(),
                    "{layout:?}: 孤立节点坐标非法"
                );
                assert!((0.0..=g.width).contains(&node.x), "{layout:?}");
                assert!((0.0..=g.height).contains(&node.y), "{layout:?}");
            }
        }
    }

    #[test]
    fn both_layouts_drop_self_loops_and_dangling_edges() {
        let nodes = vec![
            ("a.md".to_string(), "A".to_string()),
            ("b.md".to_string(), "B".to_string()),
            ("c.md".to_string(), "C".to_string()),
        ];
        let links = vec![
            ("a.md".to_string(), "b.md".to_string()),
            ("a.md".to_string(), "b.md".to_string()), // 重复边
            ("b.md".to_string(), "missing.md".to_string()), // 悬空目标
            ("c.md".to_string(), "c.md".to_string()), // 自环
        ];
        for layout in [GraphLayout::Ring, GraphLayout::ForceDirected] {
            let g = build_graph_snapshot_with(&nodes, &links, layout);
            assert_eq!(g.nodes.len(), 3, "{layout:?}");
            assert_eq!(
                g.edges.len(),
                1,
                "{layout:?}: 重复 / 悬空 / 自环边应被剔除"
            );
            assert_eq!(g.edges[0].from, 0);
            assert_eq!(g.edges[0].to, 1);
            assert_eq!(g.nodes[0].degree, 1, "{layout:?}: 度数统计不受布局影响");
            assert_eq!(g.nodes[2].degree, 0, "{layout:?}: 自环不计入度数");
        }
    }

    #[test]
    fn high_degree_nodes_sit_closer_to_the_canvas_center() {
        // 星形图：doc0 与其余 19 个节点相连，度数远高于其他节点。
        let mut nodes: Vec<(String, String)> = Vec::new();
        let mut links: Vec<(String, String)> = Vec::new();
        for i in 0..20 {
            nodes.push((format!("doc{i}.md"), format!("D{i}")));
        }
        for i in 1..20 {
            links.push(("doc0.md".to_string(), format!("doc{i}.md")));
        }
        let g = build_graph_snapshot_with(&nodes, &links, GraphLayout::ForceDirected);
        let cx = g.width / 2.0;
        let cy = g.height / 2.0;
        let dist = |n: &GraphNodeView| ((n.x - cx).powi(2) + (n.y - cy).powi(2)).sqrt();
        let hub = dist(&g.nodes[0]);
        let others: f32 = g.nodes[1..].iter().map(|n| dist(n)).sum::<f32>() / 19.0;
        assert!(
            hub < others,
            "枢纽节点离中心 {hub} 应小于其他节点的平均距离 {others}"
        );
    }

    #[test]
    fn ring_layout_keeps_hubs_nearer_to_the_center() {
        // 环形布局的语义：度数越高半径越小 → 越靠近中心。
        let mut nodes: Vec<(String, String)> = Vec::new();
        let mut links: Vec<(String, String)> = Vec::new();
        for i in 0..12 {
            nodes.push((format!("doc{i}.md"), format!("D{i}")));
        }
        for i in 1..12 {
            links.push(("doc0.md".to_string(), format!("doc{i}.md")));
        }
        let g = build_graph_snapshot_with(&nodes, &links, GraphLayout::Ring);
        let cx = g.width / 2.0;
        let cy = g.height / 2.0;
        let dist = |n: &GraphNodeView| ((n.x - cx).powi(2) + (n.y - cy).powi(2)).sqrt();
        let hub = dist(&g.nodes[0]);
        let others: f32 = g.nodes[1..].iter().map(|n| dist(n)).sum::<f32>() / 11.0;
        assert!(
            hub < others,
            "环形布局下枢纽离中心 {hub} 应小于其他节点平均距离 {others}"
        );
    }

    // ------------------------------------------------ 预计算坐标（构建期产物）

    #[test]
    fn precomputed_positions_are_applied_to_the_matching_nodes() {
        let nodes = vec![
            ("a.md".to_string(), "A".to_string()),
            ("b.md".to_string(), "B".to_string()),
        ];
        let links = vec![("a.md".to_string(), "b.md".to_string())];
        let positions = vec![
            GraphPosition {
                path: "a.md".into(),
                x: 111.0,
                y: 222.0,
            },
            GraphPosition {
                path: "b.md".into(),
                x: 333.0,
                y: 444.0,
            },
        ];
        let g = build_graph_snapshot_with_positions(&nodes, &links, &positions);
        assert_eq!(g.nodes.len(), 2);
        let a = g.nodes.iter().find(|n| n.path == "a.md").unwrap();
        assert_eq!(a.x, 111.0, "a.md 应使用预计算坐标");
        assert_eq!(a.y, 222.0);
        let b = g.nodes.iter().find(|n| n.path == "b.md").unwrap();
        assert_eq!(b.x, 333.0);
        assert_eq!(b.y, 444.0);
        // 拓扑仍由当前输入现算，不受坐标来源影响。
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.nodes[0].degree, 1);
    }

    #[test]
    fn precomputed_positions_fall_back_when_missing_or_invalid() {
        let nodes = vec![
            ("a.md".to_string(), "A".to_string()),
            ("b.md".to_string(), "B".to_string()),
            ("c.md".to_string(), "C".to_string()),
        ];
        let links = vec![("a.md".to_string(), "b.md".to_string())];
        // b.md 缺坐标；c.md 坐标是 NaN；d.md 是产物里多出来的节点。
        let positions = vec![
            GraphPosition {
                path: "a.md".into(),
                x: 111.0,
                y: 222.0,
            },
            GraphPosition {
                path: "c.md".into(),
                x: f32::NAN,
                y: 444.0,
            },
            GraphPosition {
                path: "d.md".into(),
                x: 10.0,
                y: 20.0,
            },
        ];
        let g = build_graph_snapshot_with_positions(&nodes, &links, &positions);
        assert_eq!(g.nodes.len(), 3, "产物里多出来的节点不应进入快照");
        let a = g.nodes.iter().find(|n| n.path == "a.md").unwrap();
        assert_eq!(a.x, 111.0, "有合法坐标的节点应使用预计算值");
        for n in &g.nodes {
            assert!(
                n.x.is_finite() && n.y.is_finite(),
                "{} 坐标必须是有限值",
                n.path
            );
            assert!((0.0..=g.width).contains(&n.x), "{} x 越界", n.path);
            assert!((0.0..=g.height).contains(&n.y), "{} y 越界", n.path);
        }
    }

    #[test]
    fn empty_precomputed_positions_still_produce_a_usable_snapshot() {
        // 旧产物（schema v1）没有坐标字段：必须仍能出图，不白屏、不 panic。
        let nodes = vec![
            ("a.md".to_string(), "A".to_string()),
            ("b.md".to_string(), "B".to_string()),
        ];
        let links = vec![("a.md".to_string(), "b.md".to_string())];
        let g = build_graph_snapshot_with_positions(&nodes, &links, &[]);
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.edges.len(), 1);
        for n in &g.nodes {
            assert!(n.x.is_finite() && n.y.is_finite(), "{} 坐标非法", n.path);
            assert!((0.0..=g.width).contains(&n.x));
            assert!((0.0..=g.height).contains(&n.y));
        }
    }
}
