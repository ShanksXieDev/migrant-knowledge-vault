//! 图谱画布（V2）：按格式着色 + 悬停高亮 + 大库视口裁剪 + 拖拽平移（F-10 进阶）。
//!
//! 与旧的环形图谱（`GraphPane`）相比，本组件解决三件事：
//!
//! 1. **按格式着色**：节点颜色由**路径扩展名**推导（Markdown / Excel / PowerPoint /
//!    Word / Visio / Drawio / 其他），与核心侧力导向布局的「按格式分簇」呼应——
//!    图上看得见的簇 = 图例里的颜色。
//! 2. **悬停高亮**：鼠标悬停节点时高亮该节点与它的相邻边，其余部分压暗。
//! 3. **大库保护**：节点数超过 [`GRAPH_RENDER_THRESHOLD`] 时只渲染视口内的节点，
//!    并在界面上写明「已渲染 N / 共 M 个节点」，避免用户误以为图谱不完整。
//!
//! 缩放（0.5× ~ 2.5×）、重置与**拖拽平移**都在本组件内完成。
//!
//! 样式自包含：本组件需要的 CSS 全部写在 [`GRAPH_STYLES`] 里，
//! 不依赖 `assets/main.css`（避免与并发编辑冲突）。
//!
//! ## 坐标与视口
//!
//! 画布上的点 `p` 经 `translate(cx+tx, cy+ty) scale(z) translate(-cx, -cy)` 变换后
//! 落在 SVG 的 `0..width` / `0..height` 区间内才算可见；把这个不等式反解回来，
//! 就得到**当前可见的图形坐标矩形**（[`visible_rect`]），用它裁掉视口外的节点。
//!
//! 拖拽平移时鼠标位移是**屏幕像素**，而画布坐标在缩放 `z` 下 1 像素等于 `1/z` 图形单位，
//! 因此平移量要除以 `z` 才能让图形跟手。

use dioxus::prelude::*;

use crate::VaultContext;

/// 节点数超过该阈值时启用「只渲染视口内节点」的保护。
///
/// 300 是权衡值：低于它时全量渲染（DOM 压力可忽略，图谱一眼看全）；
/// 高于它时若仍全量渲染，数千个 `<circle>` 会让浏览器掉帧。
pub const GRAPH_RENDER_THRESHOLD: usize = 300;

/// 视口裁剪时向外扩张的边距（图形坐标）。
///
/// 留一点余量，避免节点正好压在视口边缘时被「闪烁」式地裁进裁出。
const GRAPH_VIEWPORT_MARGIN: f32 = 48.0;

/// 缩放下限。
const GRAPH_ZOOM_MIN: f32 = 0.5;
/// 缩放上限。
const GRAPH_ZOOM_MAX: f32 = 2.5;
/// 缩放步长。
const GRAPH_ZOOM_STEP: f32 = 1.25;

/// 图谱节点在图例里的格式分类。
///
/// 由**路径扩展名**推导（UI 侧推导，不依赖核心侧的 `GraphNodeView` 结构）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GraphFormat {
    Markdown,
    Excel,
    PowerPoint,
    Word,
    Visio,
    Drawio,
    /// 未识别的扩展名（纯文本、未知格式等）。
    Other,
}

/// 图例中固定的格式顺序（保证图例顺序稳定，不随节点顺序抖动）。
pub const GRAPH_FORMATS: [GraphFormat; 7] = [
    GraphFormat::Markdown,
    GraphFormat::Excel,
    GraphFormat::PowerPoint,
    GraphFormat::Word,
    GraphFormat::Visio,
    GraphFormat::Drawio,
    GraphFormat::Other,
];

impl GraphFormat {
    /// 由路径扩展名推导格式。
    ///
    /// 只认扩展名、不看目录：`a/b/note.MD` 与 `note.md` 同色。
    pub fn from_path(path: &str) -> Self {
        let ext = path.rsplit('.').next().unwrap_or_default();
        let ext = ext.trim_start_matches('.').to_ascii_lowercase();
        match ext.as_str() {
            "md" | "markdown" | "mdx" => Self::Markdown,
            "xlsx" | "xlsm" | "csv" => Self::Excel,
            "pptx" | "pptm" => Self::PowerPoint,
            "docx" | "docm" => Self::Word,
            "vsdx" | "vssx" => Self::Visio,
            "drawio" | "dio" => Self::Drawio,
            _ => Self::Other,
        }
    }

    /// 图例里显示的名字。
    pub fn label(&self) -> &'static str {
        match self {
            Self::Markdown => "Markdown",
            Self::Excel => "Excel",
            Self::PowerPoint => "PowerPoint",
            Self::Word => "Word",
            Self::Visio => "Visio",
            Self::Drawio => "Drawio",
            Self::Other => "其他",
        }
    }

    /// 稳定的颜色值（十六进制）。
    ///
    /// 取自一套高对比度的主色，同一格式在任何图谱里都是同一个颜色，
    /// 用户能形成「绿色 = 台账」的肌肉记忆。
    pub fn color(&self) -> &'static str {
        match self {
            Self::Markdown => "#2563eb",
            Self::Excel => "#16a34a",
            Self::PowerPoint => "#ea580c",
            Self::Word => "#7c3aed",
            Self::Visio => "#0891b2",
            Self::Drawio => "#db2777",
            Self::Other => "#6b7280",
        }
    }

    /// CSS 类名后缀（配合 [`GRAPH_STYLES`] 里的 `.mkv-graph-v2-dot-*` 规则）。
    pub fn css_key(&self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Excel => "xls",
            Self::PowerPoint => "ppt",
            Self::Word => "doc",
            Self::Visio => "vsd",
            Self::Drawio => "dio",
            Self::Other => "other",
        }
    }
}

/// 视口矩形（图形坐标）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GraphViewport {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

impl GraphViewport {
    /// 点是否落在矩形内。
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }

    /// 向外扩张 `margin`，避免边缘节点被裁进裁出。
    pub fn expanded(&self, margin: f32) -> Self {
        Self {
            min_x: self.min_x - margin,
            min_y: self.min_y - margin,
            max_x: self.max_x + margin,
            max_y: self.max_y + margin,
        }
    }
}

/// 由缩放与平移推算当前可见的图形坐标矩形。
///
/// `pan_x` / `pan_y` 是**图形单位**的平移量（与 SVG `transform` 里的 `tx` / `ty` 同一个值）。
pub fn visible_rect(
    width: f32,
    height: f32,
    zoom: f32,
    pan_x: f32,
    pan_y: f32,
) -> GraphViewport {
    let cx = width / 2.0;
    let cy = height / 2.0;
    let z = if zoom.is_finite() && zoom > 0.0 {
        zoom
    } else {
        1.0
    };
    GraphViewport {
        min_x: cx - (cx + pan_x) / z,
        min_y: cy - (cy + pan_y) / z,
        max_x: cx + (width - cx - pan_x) / z,
        max_y: cy + (height - cy - pan_y) / z,
    }
}

/// 统计各格式的节点数量（按 [`GRAPH_FORMATS`] 的固定顺序，只输出出现过的格式）。
pub fn count_formats(paths: &[String]) -> Vec<(GraphFormat, usize)> {
    let mut counts = [0usize; 7];
    for path in paths {
        let idx = GraphFormat::from_path(path) as usize;
        counts[idx] += 1;
    }
    GRAPH_FORMATS
        .iter()
        .map(|format| (*format, counts[*format as usize]))
        .filter(|(_, count)| *count > 0)
        .collect()
}

/// 由边表构造无向邻接表。
///
/// 越界的端点与自环会被忽略（与核心侧「剔除悬空边与自环」的语义一致）。
pub fn adjacency(node_count: usize, edges: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let mut adj = vec![Vec::new(); node_count];
    for &(a, b) in edges {
        if a == b || a >= node_count || b >= node_count {
            continue;
        }
        adj[a].push(b);
        adj[b].push(a);
    }
    adj
}

/// 选出需要渲染的节点与边的下标。
///
/// - 节点：坐标落在（扩张过的）视口矩形内；
/// - 边：至少有一个端点落在视口内（枢纽在视口外时，它连向视口的边依然要画出来）。
pub fn select_visible(
    positions: &[(f32, f32)],
    edges: &[(usize, usize)],
    viewport: &GraphViewport,
) -> (Vec<usize>, Vec<usize>) {
    let area = viewport.expanded(GRAPH_VIEWPORT_MARGIN);
    let visible_nodes: Vec<usize> = positions
        .iter()
        .enumerate()
        .filter(|(_, &(x, y))| area.contains(x, y))
        .map(|(i, _)| i)
        .collect();
    let mut visible_set = vec![false; positions.len()];
    for &i in &visible_nodes {
        visible_set[i] = true;
    }
    let visible_edges: Vec<usize> = edges
        .iter()
        .enumerate()
        .filter(|(_, &(a, b))| {
            (a < visible_set.len() && visible_set[a]) || (b < visible_set.len() && visible_set[b])
        })
        .map(|(i, _)| i)
        .collect();
    (visible_nodes, visible_edges)
}

/// 拖拽平移的中间状态。
#[derive(Debug, Clone, Copy)]
struct DragState {
    /// 按下时的鼠标客户端坐标。
    start_x: f64,
    start_y: f64,
    /// 按下时的平移量（图形单位）。
    pan_x: f32,
    pan_y: f32,
}

/// 图例的一行（格式 → 颜色 + 数量）。
struct LegendRow {
    /// 颜色对应的 CSS 类名后缀。
    css_key: &'static str,
    /// 格式显示名。
    label: &'static str,
    /// 该格式的节点数。
    count: usize,
}

/// 一条待绘制的边（坐标已算好，rsx 里直接用）。
struct EdgeDraw {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    /// 是否与当前悬停节点相邻（相邻边高亮）。
    active: bool,
}

/// 一个待绘制的节点（坐标、颜色类名、样式类名都已算好）。
struct NodeDraw {
    /// 节点在快照里的下标（悬停高亮用）。
    index: usize,
    x: f32,
    y: f32,
    radius: f32,
    /// 节点标题（截断后用于画标签）。
    label: String,
    /// 节点的样式类名（含高亮 / 压暗状态）。
    class_name: String,
    /// 格式对应的颜色类名后缀。
    css_key: &'static str,
    /// 完整路径（点击跳转用）。
    path: String,
    /// 完整标题（点击后的状态提示用）。
    title: String,
    /// 度数。
    degree: usize,
}

/// 图谱画布（V2）。
///
/// 挂载方式：`GraphPaneV2 { ctx }`（与旧的 `GraphPane { ctx }` 完全同形）。
#[component]
pub fn GraphPaneV2(ctx: VaultContext) -> Element {
    let _revision = *ctx.revision.read();
    let graph = ctx.service.read().graph_snapshot();

    let mut zoom = use_signal(|| 1.0f32);
    let mut pan = use_signal(|| (0.0f32, 0.0f32));
    let mut hovered = use_signal(|| None::<usize>);
    let mut drag = use_signal(|| None::<DragState>);

    let width = graph.width;
    let height = graph.height;
    let cx = width / 2.0;
    let cy = height / 2.0;
    let (pan_x, pan_y) = *pan.read();
    let scale = *zoom.read();
    let transform = format!(
        "translate({} {}) scale({}) translate({} {})",
        cx + pan_x,
        cy + pan_y,
        scale,
        -cx,
        -cy
    );
    let view_box = format!("0 0 {width} {height}");

    let positions: Vec<(f32, f32)> = graph.nodes.iter().map(|n| (n.x, n.y)).collect();
    let edge_pairs: Vec<(usize, usize)> = graph.edges.iter().map(|e| (e.from, e.to)).collect();
    let neighbors = adjacency(graph.nodes.len(), &edge_pairs);
    let hovered_index = *hovered.read();

    // 大库保护：节点太多时只渲染视口内的节点。
    let viewport = visible_rect(width, height, scale, pan_x, pan_y);
    let (visible_nodes, visible_edges) = if graph.nodes.len() > GRAPH_RENDER_THRESHOLD {
        select_visible(&positions, &edge_pairs, &viewport)
    } else {
        (
            (0..graph.nodes.len()).collect::<Vec<usize>>(),
            (0..graph.edges.len()).collect::<Vec<usize>>(),
        )
    };
    let rendered_nodes = visible_nodes.len();
    let total_nodes = graph.nodes.len();
    let total_edges = graph.edges.len();

    // 图例用的格式统计（与画布上的颜色一一对应）。
    let paths: Vec<String> = graph.nodes.iter().map(|n| n.path.clone()).collect();
    let legend: Vec<LegendRow> = count_formats(&paths)
        .into_iter()
        .map(|(format, count)| LegendRow {
            css_key: format.css_key(),
            label: format.label(),
            count,
        })
        .collect();

    // rsx 的 `for` 体里不能写 `let`，因此先把要绘制的内容算好。
    let edge_draws: Vec<EdgeDraw> = visible_edges
        .iter()
        .filter_map(|edge_index| {
            let edge = graph.edges.get(*edge_index)?;
            let a = graph.nodes.get(edge.from)?;
            let b = graph.nodes.get(edge.to)?;
            Some(EdgeDraw {
                x1: a.x,
                y1: a.y,
                x2: b.x,
                y2: b.y,
                active: hovered_index
                    .map(|h| h == edge.from || h == edge.to)
                    .unwrap_or(false),
            })
        })
        .collect();
    let node_draws: Vec<NodeDraw> = visible_nodes
        .iter()
        .filter_map(|node_index| {
            let node = graph.nodes.get(*node_index)?;
            let format = GraphFormat::from_path(&node.path);
            let is_hovered = hovered_index == Some(*node_index);
            let is_neighbor = hovered_index
                .map(|h| {
                    neighbors
                        .get(h)
                        .map(|list| list.contains(node_index))
                        .unwrap_or(false)
                })
                .unwrap_or(false);
            let dimmed = hovered_index.is_some() && !is_hovered && !is_neighbor;
            let class_name = if is_hovered {
                "mkv-graph-node mkv-graph-v2-node mkv-graph-v2-node-active"
            } else if is_neighbor {
                "mkv-graph-node mkv-graph-v2-node mkv-graph-v2-node-neighbor"
            } else if dimmed {
                "mkv-graph-node mkv-graph-v2-node mkv-graph-v2-node-dimmed"
            } else {
                "mkv-graph-node mkv-graph-v2-node"
            };
            Some(NodeDraw {
                index: *node_index,
                x: node.x,
                y: node.y,
                radius: 6.0 + (node.degree.min(6) as f32) * 1.4,
                label: node.title.chars().take(10).collect(),
                class_name: class_name.to_string(),
                css_key: format.css_key(),
                path: node.path.clone(),
                title: node.title.clone(),
                degree: node.degree,
            })
        })
        .collect();

    rsx! {
        style { "{GRAPH_STYLES}" }
        section { class: "mkv-graph-v2",
            div { class: "mkv-graph-v2-header",
                h2 { "双链关系图谱" }
                span { class: "mkv-graph-v2-stats", "{total_nodes} 个节点 · {total_edges} 条边" }
                div { class: "mkv-graph-v2-tools",
                    button {
                        class: "mkv-btn mkv-btn-sm",
                        onclick: move |_| {
                            let current = *zoom.peek();
                            zoom.set((current * GRAPH_ZOOM_STEP).min(GRAPH_ZOOM_MAX));
                        },
                        "放大 +"
                    }
                    button {
                        class: "mkv-btn mkv-btn-sm",
                        onclick: move |_| {
                            let current = *zoom.peek();
                            zoom.set((current / GRAPH_ZOOM_STEP).max(GRAPH_ZOOM_MIN));
                        },
                        "缩小 −"
                    }
                    button {
                        class: "mkv-btn mkv-btn-sm",
                        onclick: move |_| {
                            zoom.set(1.0);
                            pan.set((0.0, 0.0));
                        },
                        "重置"
                    }
                }
            }
            div { class: "mkv-graph-v2-canvas",
                svg {
                    class: "mkv-graph-v2-svg",
                    view_box: "{view_box}",
                    // SVG 按图形单位定尺（1 图形单位 = 1 CSS 像素），
                    // 这样拖拽平移时「像素 → 图形单位」的换算才是精确的。
                    style: "width: {width}px; height: {height}px;",
                    onmousedown: move |e| {
                        let point = e.client_coordinates();
                        let (pan_x, pan_y) = *pan.peek();
                        drag.set(Some(DragState {
                            start_x: point.x,
                            start_y: point.y,
                            pan_x,
                            pan_y,
                        }));
                    },
                    onmousemove: move |e| {
                        let Some(state) = *drag.peek() else {
                            return;
                        };
                        let point = e.client_coordinates();
                        // 缩放 z 之下 1 屏幕像素等于 1/z 图形单位。
                        let factor = if scale > 0.0 { 1.0 / scale } else { 1.0 };
                        let next_x = state.pan_x - ((point.x - state.start_x) as f32) * factor;
                        let next_y = state.pan_y - ((point.y - state.start_y) as f32) * factor;
                        // 平移量夹在画布尺寸内，避免把图拖丢。
                        pan.set((next_x.clamp(-width, width), next_y.clamp(-height, height)));
                    },
                    onmouseup: move |_| {
                        drag.set(None);
                    },
                    onmouseleave: move |_| {
                        drag.set(None);
                    },
                    g { transform: "{transform}",
                        for edge in edge_draws {
                            line {
                                class: if edge.active {
                                    "mkv-graph-edge mkv-graph-v2-edge mkv-graph-v2-edge-active"
                                } else {
                                    "mkv-graph-edge mkv-graph-v2-edge"
                                },
                                x1: "{edge.x1}", y1: "{edge.y1}",
                                x2: "{edge.x2}", y2: "{edge.y2}",
                            }
                        }
                        for node in node_draws {
                            g {
                                class: "{node.class_name}",
                                onmouseenter: move |_| {
                                    hovered.set(Some(node.index));
                                },
                                onmouseleave: move |_| {
                                    hovered.set(None);
                                },
                                onclick: move |_| {
                                    let path = node.path.clone();
                                    let title = node.title.clone();
                                    let degree = node.degree;
                                    let mut current_doc = ctx.current_doc;
                                    let mut backlinks = ctx.backlinks;
                                    let mut list_mode = ctx.list_mode;
                                    current_doc.set(path.clone());
                                    backlinks.set(ctx.service.read().backlinks(&path));
                                    ctx.focus.set(None);
                                    list_mode.set(crate::ListMode::Documents);
                                    ctx.status.set(format!("已跳到 {title}（{degree} 条连接）"));
                                },
                                circle {
                                    class: "mkv-graph-v2-dot mkv-graph-v2-dot-{node.css_key}",
                                    cx: "{node.x}", cy: "{node.y}", r: "{node.radius}",
                                }
                                text {
                                    class: "mkv-graph-v2-label",
                                    x: "{node.x}", y: "{node.y}",
                                    "{node.label}",
                                }
                            }
                        }
                    }
                }
                div { class: "mkv-graph-v2-legend",
                    div { class: "mkv-graph-v2-legend-title", "格式图例" }
                    for row in legend {
                        div { class: "mkv-graph-v2-legend-item",
                            span { class: "mkv-graph-v2-swatch mkv-graph-v2-swatch-{row.css_key}" }
                            span { class: "mkv-graph-v2-legend-label", "{row.label}" }
                            span { class: "mkv-graph-v2-legend-count", "{row.count}" }
                        }
                    }
                }
                div { class: "mkv-graph-v2-status",
                    span { "已渲染 {rendered_nodes} / 共 {total_nodes} 个节点" }
                    if rendered_nodes < total_nodes {
                        span { class: "mkv-graph-v2-status-hint", "（仅显示视口内节点，缩放或拖拽查看其余部分）" }
                    }
                }
                if total_nodes == 0 {
                    p { class: "mkv-graph-v2-empty", "知识库为空，暂无可绘制的双链关系。" }
                }
            }
        }
    }
}

/// 侧栏图例（V2）：节点数 / 边数 + 每种格式的颜色与数量。
///
/// 挂载方式：`GraphLegendV2 { ctx }`（替换旧的 `GraphLegend { ctx }`）。
#[component]
pub fn GraphLegendV2(ctx: VaultContext) -> Element {
    let _revision = *ctx.revision.read();
    let graph = ctx.service.read().graph_snapshot();
    let paths: Vec<String> = graph.nodes.iter().map(|n| n.path.clone()).collect();
    let legend: Vec<LegendRow> = count_formats(&paths)
        .into_iter()
        .map(|(format, count)| LegendRow {
            css_key: format.css_key(),
            label: format.label(),
            count,
        })
        .collect();
    let legend_nodes: Vec<(String, String, usize)> = graph
        .nodes
        .iter()
        .map(|node| {
            (
                GraphFormat::from_path(&node.path).css_key().to_string(),
                node.title.clone(),
                node.degree,
            )
        })
        .collect();

    rsx! {
        style { "{GRAPH_STYLES}" }
        div { class: "mkv-graph-v2-legend-panel",
            p { class: "mkv-graph-v2-legend-stats",
                "{graph.nodes.len()} 个节点 · {graph.edges.len()} 条边"
            }
            if graph.nodes.is_empty() {
                p { class: "mkv-graph-v2-legend-empty", "当前知识库还没有双链关系。" }
            } else {
                div { class: "mkv-graph-v2-legend-rows",
                    for row in legend {
                        div { class: "mkv-graph-v2-legend-item",
                            span { class: "mkv-graph-v2-swatch mkv-graph-v2-swatch-{row.css_key}" }
                            span { class: "mkv-graph-v2-legend-label", "{row.label}" }
                            span { class: "mkv-graph-v2-legend-count", "{row.count}" }
                        }
                    }
                }
                div { class: "mkv-graph-v2-legend-list",
                    for (css_key, title, degree) in legend_nodes {
                        div { class: "mkv-graph-v2-legend-node",
                            span { class: "mkv-graph-v2-swatch mkv-graph-v2-swatch-{css_key}" }
                            span { class: "mkv-graph-v2-legend-label", "{title}" }
                            span { class: "mkv-graph-v2-legend-count", "{degree} 条连接" }
                        }
                    }
                }
            }
        }
    }
}

/// 图谱组件的自包含样式表。
///
/// 类名统一带 `mkv-graph-v2` 前缀，与旧图谱（`mkv-graph-*`）互不干扰；
/// 选择器写成「后代 + 自身」两级，特异性高于全局样式表里的单类规则。
const GRAPH_STYLES: &str = r#"
.mkv-graph-v2 {
  display: flex;
  flex-direction: column;
  flex: 1 1 auto;
  min-height: 0;
  position: relative;
}
.mkv-graph-v2-header {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 16px;
  border-bottom: 1px solid var(--mkv-border, #e5e7eb);
}
.mkv-graph-v2-header h2 { margin: 0; font-size: 15px; }
.mkv-graph-v2-stats { font-size: 12px; color: var(--mkv-muted, #6b7280); }
.mkv-graph-v2-tools { display: flex; gap: 6px; margin-left: auto; }
.mkv-graph-v2-canvas {
  flex: 1 1 auto;
  min-height: 0;
  overflow: auto;
  padding: 12px;
  position: relative;
}
.mkv-graph-v2 .mkv-graph-v2-svg {
  display: block;
  max-width: none;
  background: var(--mkv-surface, #ffffff);
  border: 1px solid var(--mkv-border, #e5e7eb);
  border-radius: var(--mkv-radius, 6px);
  cursor: grab;
  touch-action: none;
}
.mkv-graph-v2 .mkv-graph-v2-svg:active { cursor: grabbing; }
.mkv-graph-v2 .mkv-graph-v2-edge {
  stroke: var(--mkv-border, #d1d5db);
  stroke-width: 1.4;
  opacity: 0.65;
}
.mkv-graph-v2 .mkv-graph-v2-edge-active {
  stroke: var(--mkv-accent, #2563eb);
  stroke-width: 2.4;
  opacity: 1;
}
.mkv-graph-v2 .mkv-graph-v2-node { cursor: pointer; }
.mkv-graph-v2 .mkv-graph-v2-node-dot { stroke: #ffffff; stroke-width: 1.4; }
.mkv-graph-v2 .mkv-graph-v2-node-dimmed { opacity: 0.25; }
.mkv-graph-v2 .mkv-graph-v2-node-active .mkv-graph-v2-dot { stroke-width: 2.4; }
.mkv-graph-v2 .mkv-graph-v2-node-neighbor .mkv-graph-v2-dot { stroke-width: 2; }
.mkv-graph-v2 .mkv-graph-v2-dot-md { fill: #2563eb; }
.mkv-graph-v2 .mkv-graph-v2-dot-xls { fill: #16a34a; }
.mkv-graph-v2 .mkv-graph-v2-dot-ppt { fill: #ea580c; }
.mkv-graph-v2 .mkv-graph-v2-dot-doc { fill: #7c3aed; }
.mkv-graph-v2 .mkv-graph-v2-dot-vsd { fill: #0891b2; }
.mkv-graph-v2 .mkv-graph-v2-dot-dio { fill: #db2777; }
.mkv-graph-v2 .mkv-graph-v2-dot-other { fill: #6b7280; }
.mkv-graph-v2 .mkv-graph-v2-label {
  font-size: 10px;
  fill: var(--mkv-text, #1f2937);
  text-anchor: middle;
  pointer-events: none;
}
.mkv-graph-v2 .mkv-graph-v2-legend {
  position: absolute;
  top: 20px;
  right: 20px;
  padding: 8px 10px;
  font-size: 12px;
  background: var(--mkv-surface, #ffffff);
  border: 1px solid var(--mkv-border, #e5e7eb);
  border-radius: var(--mkv-radius, 6px);
  box-shadow: 0 1px 4px rgba(15, 23, 42, 0.08);
  pointer-events: none;
}
.mkv-graph-v2 .mkv-graph-v2-legend-title {
  font-weight: 600;
  margin-bottom: 4px;
  color: var(--mkv-text, #1f2937);
}
.mkv-graph-v2 .mkv-graph-v2-legend-item {
  display: flex;
  align-items: center;
  gap: 6px;
  line-height: 1.6;
}
.mkv-graph-v2 .mkv-graph-v2-swatch {
  display: inline-block;
  width: 10px;
  height: 10px;
  border-radius: 50%;
  flex: 0 0 auto;
}
.mkv-graph-v2 .mkv-graph-v2-swatch-md { background: #2563eb; }
.mkv-graph-v2 .mkv-graph-v2-swatch-xls { background: #16a34a; }
.mkv-graph-v2 .mkv-graph-v2-swatch-ppt { background: #ea580c; }
.mkv-graph-v2 .mkv-graph-v2-swatch-doc { background: #7c3aed; }
.mkv-graph-v2 .mkv-graph-v2-swatch-vsd { background: #0891b2; }
.mkv-graph-v2 .mkv-graph-v2-swatch-dio { background: #db2777; }
.mkv-graph-v2 .mkv-graph-v2-swatch-other { background: #6b7280; }
.mkv-graph-v2 .mkv-graph-v2-legend-label { flex: 1 1 auto; }
.mkv-graph-v2 .mkv-graph-v2-legend-count { color: var(--mkv-muted, #6b7280); }
.mkv-graph-v2 .mkv-graph-v2-status {
  padding: 6px 4px 0;
  font-size: 12px;
  color: var(--mkv-muted, #6b7280);
}
.mkv-graph-v2 .mkv-graph-v2-status-hint { margin-left: 6px; }
.mkv-graph-v2 .mkv-graph-v2-empty { padding: 12px 4px; color: var(--mkv-muted, #6b7280); }
.mkv-graph-v2 .mkv-graph-v2-legend-panel { padding: 10px; overflow: auto; }
.mkv-graph-v2 .mkv-graph-v2-legend-stats { margin: 0 0 8px; font-size: 12px; color: var(--mkv-muted, #6b7280); }
.mkv-graph-v2 .mkv-graph-v2-legend-empty { font-size: 12px; color: var(--mkv-muted, #6b7280); }
.mkv-graph-v2 .mkv-graph-v2-legend-rows { margin-bottom: 10px; }
.mkv-graph-v2 .mkv-graph-v2-legend-node {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 0;
  font-size: 12px;
  border-bottom: 1px solid var(--mkv-border, #e5e7eb);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_is_derived_from_extension() {
        assert_eq!(GraphFormat::from_path("a/b/note.md"), GraphFormat::Markdown);
        assert_eq!(GraphFormat::from_path("note.MD"), GraphFormat::Markdown);
        assert_eq!(GraphFormat::from_path("台账.xlsx"), GraphFormat::Excel);
        assert_eq!(GraphFormat::from_path("deck.pptx"), GraphFormat::PowerPoint);
        assert_eq!(GraphFormat::from_path("spec.docx"), GraphFormat::Word);
        assert_eq!(GraphFormat::from_path("chart.vsdx"), GraphFormat::Visio);
        assert_eq!(GraphFormat::from_path("flow.drawio"), GraphFormat::Drawio);
        assert_eq!(GraphFormat::from_path("notes.txt"), GraphFormat::Other);
        assert_eq!(GraphFormat::from_path("README"), GraphFormat::Other);
        assert_eq!(GraphFormat::from_path("dir.v2/file"), GraphFormat::Other);
    }

    #[test]
    fn every_format_has_a_distinct_stable_color() {
        let mut colors: Vec<&str> = GRAPH_FORMATS.iter().map(|f| f.color()).collect();
        colors.sort_unstable();
        let count = colors.len();
        colors.dedup();
        assert_eq!(colors.len(), count, "每种格式的颜色必须互不相同");
        // 颜色值必须稳定（同一格式反复取色一致）。
        assert_eq!(GraphFormat::Markdown.color(), "#2563eb");
        assert_eq!(GraphFormat::Other.color(), "#6b7280");
    }

    #[test]
    fn format_counts_follow_the_fixed_legend_order() {
        let paths = vec![
            "a.md".to_string(),
            "b.md".to_string(),
            "c.xlsx".to_string(),
            "d.drawio".to_string(),
        ];
        let counts = count_formats(&paths);
        assert_eq!(counts.len(), 3);
        assert_eq!(counts[0], (GraphFormat::Markdown, 2));
        assert_eq!(counts[1], (GraphFormat::Excel, 1));
        assert_eq!(counts[2], (GraphFormat::Drawio, 1));
    }

    #[test]
    fn visible_rect_covers_the_whole_canvas_at_default_zoom() {
        let rect = visible_rect(720.0, 560.0, 1.0, 0.0, 0.0);
        assert!(rect.contains(0.0, 0.0));
        assert!(rect.contains(720.0, 560.0));
        assert!(rect.contains(360.0, 280.0));
    }

    #[test]
    fn zooming_in_shrinks_the_visible_rect_around_the_center() {
        let full = visible_rect(720.0, 560.0, 1.0, 0.0, 0.0);
        let zoomed = visible_rect(720.0, 560.0, 2.0, 0.0, 0.0);
        assert!(zoomed.max_x - zoomed.min_x < full.max_x - full.min_x);
        assert!(zoomed.max_y - zoomed.min_y < full.max_y - full.min_y);
        // 缩放后仍以画布中心为中心。
        let mid_x = (zoomed.min_x + zoomed.max_x) / 2.0;
        let mid_y = (zoomed.min_y + zoomed.max_y) / 2.0;
        assert!((mid_x - 360.0).abs() < 0.001, "中心 x = {mid_x}");
        assert!((mid_y - 280.0).abs() < 0.001, "中心 y = {mid_y}");
    }

    #[test]
    fn panning_shifts_the_visible_rect() {
        let rect = visible_rect(720.0, 560.0, 1.0, 100.0, 0.0);
        assert!(rect.contains(-100.0, 280.0), "平移后左侧露出更多内容");
        assert!(!rect.contains(700.0, 280.0), "平移后右侧内容被推出视野");
    }

    #[test]
    fn only_nodes_inside_the_viewport_are_selected() {
        let positions = vec![(10.0, 10.0), (700.0, 540.0), (360.0, 280.0)];
        let edges = vec![(0usize, 1usize), (0, 2)];
        let viewport = visible_rect(720.0, 560.0, 1.0, 0.0, 0.0);
        let (nodes, edges) = select_visible(&positions, &edges, &viewport);
        assert_eq!(nodes.len(), 3, "小图默认全量在视口内");
        assert_eq!(edges.len(), 2);
    }

    #[test]
    fn viewport_culling_drops_far_away_nodes_but_keeps_edges_reaching_them() {
        // 视口被缩小到中心一小块：远处的节点应被裁掉，
        // 但「一端在视口内」的边仍然要画出来（枢纽在视口外时连线才有意义）。
        let positions = vec![(0.0, 0.0), (700.0, 540.0), (360.0, 280.0)];
        let edges = vec![(0usize, 1usize), (0, 2)];
        let viewport = visible_rect(720.0, 560.0, 4.0, 0.0, 0.0);
        let (nodes, edges) = select_visible(&positions, &edges, &viewport);
        assert!(nodes.contains(&2), "中心节点应保留");
        assert!(!nodes.contains(&0), "远处节点应被裁掉");
        assert!(!nodes.contains(&1), "远处节点应被裁掉");
        assert_eq!(edges, vec![1], "只保留有可见端点的边 (0,2)");
    }

    #[test]
    fn adjacency_ignores_self_loops_and_out_of_range_edges() {
        let adj = adjacency(3, &[(0usize, 0usize), (0, 1), (0, 9), (2, 1)]);
        assert_eq!(adj[0], vec![1]);
        assert_eq!(adj[1], vec![0, 2]);
        assert_eq!(adj[2], vec![1]);
    }
}
