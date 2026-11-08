//! Migrant Knowledge Vault 共享 UI 组件。
//!
//! 三端（Web / Desktop / Mobile）复用同一套组件；平台差异仅体现在入口 crate。
//!
//! 组件划分对应 PRD 第 5 章的信息架构：
//! - [`SearchBar`]      检索中心输入
//! - [`ResultList`]     检索结果列表
//! - [`PreviewPane`]    预览区
//! - [`BacklinkPanel`]  双链面板（反链）
//! - [`StatusBar`]      状态与索引进度
//!
//! 依赖约束：本 crate 只依赖 `mkv-core` 的**纯 Rust**部分
//! （`types` / `service` / `format`），因此可在 wasm32 下编译。

use dioxus::prelude::*;
use mkv_core::editor::EditSession;
use mkv_core::service::KnowledgeService;
use mkv_core::types::{DocumentRecord, LinkRecord, SearchHit};

pub mod components;

pub use components::editors::{DeckCraftPanel, EditorHost, GridCraftPanel, WordCraftPanel};
pub use components::SearchBar;

/// 内联样式表。
///
/// 通过 `include_str!` 在编译期嵌入：三端（Desktop / Web / Mobile）都能直接生效，
/// 不依赖 `dx` 打包资源目录，避免各端构建配置分叉。
pub const STYLESHEET: &str = include_str!("../assets/main.css");

/// 视图状态。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// 预览。
    Preview,
    /// 编辑。
    Edit,
    /// 版本历史。
    History,
}

/// 侧栏列表模式。
///
/// Web 端没有本地目录可浏览，文档列表来自预构建索引；
/// 桌面端则可同时浏览「全部文档」与「检索结果」。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ListMode {
    /// 全部文档。
    Documents,
    /// 检索结果。
    Results,
    /// 双链关系图谱（F-10）。
    Graph,
}

/// 应用级共享状态。
///
/// 注意：dioxus `#[component]` 要求 props 实现 `PartialEq`，
/// 因此这里只保存 `Signal`（其 `PartialEq` 为指针比较，与内容类型无关）。
#[derive(Clone, Copy, PartialEq)]
pub struct VaultContext {
    /// 知识库服务（由各端注入：native 用 `NativeService`，web 用预构建索引实现）。
    pub service: Signal<Box<dyn KnowledgeService>>,
    /// 当前打开的知识库路径（空串表示未打开）。
    pub root: Signal<String>,
    /// 当前选中的文档路径。
    pub current_doc: Signal<String>,
    /// 检索关键词。
    pub query: Signal<String>,
    /// 检索结果。
    pub results: Signal<Vec<SearchHit>>,
    /// 当前文档的反链。
    pub backlinks: Signal<Vec<LinkRecord>>,
    /// 状态消息。
    pub status: Signal<String>,
    /// 索引进度（0.0 - 1.0）。
    pub progress: Signal<f32>,
    /// 当前编辑会话（None 表示不在编辑态）。
    pub edit_session: Signal<Option<EditSession>>,
    /// 视图模式。
    pub view_mode: Signal<ViewMode>,
    /// 侧栏列表模式（全部文档 / 检索结果）。
    pub list_mode: Signal<ListMode>,
    /// 索引修订号：后台文件监听每批变更后 +1。
    ///
    /// 各视图组件订阅它，从而在索引变化时**自动重渲染**，
    /// 不再需要用户手动切页签 / 重新检索。Web 端只读索引恒为 0。
    pub revision: Signal<u64>,
    /// 检索命中的精确位置（PRD §7.1）；`None` 表示查看全文。
    pub focus: Signal<Option<mkv_core::locate::HitLocation>>,
}

impl VaultContext {
    /// 以指定服务创建上下文。
    pub fn with_service(service: Box<dyn KnowledgeService>) -> Self {
        Self {
            service: Signal::new(service),
            root: Signal::new(String::new()),
            current_doc: Signal::new(String::new()),
            query: Signal::new(String::new()),
            results: Signal::new(Vec::new()),
            backlinks: Signal::new(Vec::new()),
            status: Signal::new(String::from("就绪")),
            progress: Signal::new(0.0),
            edit_session: Signal::new(None),
            view_mode: Signal::new(ViewMode::Preview),
            list_mode: Signal::new(ListMode::Documents),
            revision: Signal::new(0),
            focus: Signal::new(None),
        }
    }

    /// 使用空服务创建上下文（Web 端占位）。
    pub fn new() -> Self {
        Self::with_service(Box::new(mkv_core::service::NullService))
    }

    /// 索引变更后刷新所有派生视图（Desktop 自动刷新调用）。
    ///
    /// 递增 `revision` 让订阅它的组件重渲染；同时重算缓存的反链与检索结果，
    /// 避免它们停留在旧快照。用 `peek()` 读取以免本方法自己订阅信号、
    /// 被自己触发的 `set` 反向唤醒形成循环。
    pub fn refresh(&self) {
        let mut revision = self.revision;
        let next = revision.peek().wrapping_add(1);
        revision.set(next);

        let service = self.service.peek();
        let current = self.current_doc.peek().clone();
        if !current.is_empty() {
            let mut backlinks = self.backlinks;
            backlinks.set(service.backlinks(&current));
        }
        let query = self.query.peek().clone();
        if !query.trim().is_empty() {
            let mut results = self.results;
            results.set(service.search(&query, 20));
        }
        let count = service.document_count();
        drop(service);

        let mut status = self.status;
        status.set(format!("已自动刷新 · {count} 个文档"));
        // 索引重排后旧的片段下标不再可信，清掉定位态避免指错位置。
        let mut focus = self.focus;
        focus.set(None);
    }
}

impl Default for VaultContext {
    fn default() -> Self {
        Self::new()
    }
}

/// 应用外壳：内联样式表 + 三栏布局 + 状态栏。
///
/// 三端入口统一复用此组件，保证 Web / Desktop / Mobile 视觉一致。
#[component]
pub fn AppShell(ctx: VaultContext) -> Element {
    rsx! {
        style { "{STYLESHEET}" }
        div { class: "mkv-app",
            TopBar { ctx }
            div { class: "mkv-body",
                SideBar { ctx }
                MainPane { ctx }
            }
            StatusBar { ctx }
        }
    }
}

/// 顶栏：知识库路径 + 状态。
#[component]
pub fn TopBar(ctx: VaultContext) -> Element {
    let status = ctx.status.read().clone();
    let root = ctx.root.read().clone();

    rsx! {
        header { class: "mkv-topbar",
            div { class: "mkv-topbar-title",
                span { class: "mkv-logo", "MKV" }
                span { class: "mkv-app-name", "Migrant Knowledge Vault" }
            }
            div { class: "mkv-topbar-root",
                if root.is_empty() {
                    span { class: "mkv-muted", "未打开知识库" }
                } else {
                    span { class: "mkv-root-path", "{root}" }
                }
            }
            div { class: "mkv-topbar-status", "{status}" }
        }
    }
}

/// 左侧栏：检索 + 文档/结果列表。
#[component]
pub fn SideBar(ctx: VaultContext) -> Element {
    let mode = ctx.list_mode.read().clone();

    rsx! {
        aside { class: "mkv-sidebar",
            SearchBar { ctx }
            div { class: "mkv-list-tabs",
                button {
                    class: if mode == ListMode::Documents { "mkv-tab mkv-tab-active" } else { "mkv-tab" },
                    onclick: move |_| ctx.list_mode.set(ListMode::Documents),
                    "全部文档"
                }
                button {
                    class: if mode == ListMode::Results { "mkv-tab mkv-tab-active" } else { "mkv-tab" },
                    onclick: move |_| ctx.list_mode.set(ListMode::Results),
                    "检索结果"
                }
                button {
                    class: if mode == ListMode::Graph { "mkv-tab mkv-tab-active" } else { "mkv-tab" },
                    onclick: move |_| ctx.list_mode.set(ListMode::Graph),
                    "关系图谱"
                }
            }
            // 注意：rsx 的子节点不支持 match（会被当成原始 Rust 表达式，
            // 使 `DocList { ctx }` 解析到组件补全枚举上），这里用 if/else 分支。
            if mode == ListMode::Documents {
                DocList { ctx }
            } else if mode == ListMode::Results {
                ResultList { ctx }
            } else {
                GraphLegend { ctx }
            }
        }
    }
}

/// 全部文档列表。
///
/// Web 端的文档列表来自预构建索引（只读）；
/// 桌面端来自 Vault 的文档表。
#[component]
pub fn DocList(ctx: VaultContext) -> Element {
    // 订阅索引修订号：后台增量更新后文档列表自动重渲染
    let _revision = *ctx.revision.read();
    let docs = ctx.service.read().documents();

    rsx! {
        div { class: "mkv-results",
            if docs.is_empty() {
                div { class: "mkv-muted mkv-results-empty", "暂无文档" }
            } else {
                for doc in docs.iter() {
                    DocItem { ctx, doc: doc.clone() }
                }
            }
        }
    }
}

/// 单条文档条目。
#[component]
pub fn DocItem(ctx: VaultContext, doc: DocumentRecord) -> Element {
    let current = ctx.current_doc.read().clone();
    let is_active = current == doc.path;

    rsx! {
        button {
            class: if is_active { "mkv-result mkv-result-active" } else { "mkv-result" },
            onclick: move |_| {
                let mut current_doc = ctx.current_doc;
                let mut backlinks = ctx.backlinks;
                ctx.focus.set(None);
                current_doc.set(doc.path.clone());
                backlinks.set(ctx.service.read().backlinks(&doc.path));
            },
            div { class: "mkv-result-title", "{doc.title}" }
            div { class: "mkv-result-meta",
                span { "{doc.format}" }
                span { class: "mkv-muted", "{doc.path}" }
            }
        }
    }
}

/// 检索结果列表。
#[component]
pub fn ResultList(ctx: VaultContext) -> Element {
    let _revision = *ctx.revision.read();
    let results = ctx.results.read().clone();

    rsx! {
        div { class: "mkv-results",
            if results.is_empty() {
                div { class: "mkv-muted mkv-results-empty", "输入关键词开始检索" }
            } else {
                for hit in results.iter() {
                    ResultItem { ctx, hit: hit.clone() }
                }
            }
        }
    }
}

/// 单条检索结果。
#[component]
pub fn ResultItem(ctx: VaultContext, hit: SearchHit) -> Element {
    let current = ctx.current_doc.read().clone();
    let is_active = current == hit.path;
    // PRD §7.1：结果列表直接给出命中位置标签，点击后预览区跳到该片段并高亮。
    let query = ctx.query.peek().clone();
    let location = ctx.service.read().locate(&hit.path, &query);
    let hit_path = hit.path.clone();

    rsx! {
        button {
            class: if is_active { "mkv-result mkv-result-active" } else { "mkv-result" },
            onclick: move |_| {
                let mut current_doc = ctx.current_doc;
                let mut backlinks = ctx.backlinks;
                let mut focus = ctx.focus;
                let query = ctx.query.peek().clone();
                let loc = ctx.service.read().locate(&hit_path, &query);
                current_doc.set(hit_path.clone());
                backlinks.set(ctx.service.read().backlinks(&hit_path));
                focus.set(loc);
            },
            div { class: "mkv-result-title", "{hit.title}" }
            div { class: "mkv-result-meta",
                span { "{hit.format}" }
                span { class: "mkv-muted", "{hit.path}" }
            }
            if let Some(loc) = location.as_ref() {
                div { class: "mkv-hit-location",
                    span { class: "mkv-hit-loc-label", "📍 {loc.label}" }
                    span { class: "mkv-muted", "{loc.snippet}" }
                }
            }
        }
    }
}

/// 右侧主区：预览 + 双链面板。
#[component]
pub fn MainPane(ctx: VaultContext) -> Element {
    let current = ctx.current_doc.read().clone();
    let mode = ctx.list_mode.read().clone();

    rsx! {
        main { class: "mkv-main",
            if mode == ListMode::Graph {
                GraphPane { ctx }
            } else if current.is_empty() {
                div { class: "mkv-empty",
                    h2 { "欢迎使用 Migrant Knowledge Vault" }
                    p { "本地优先 · 文件优先 · Git 优先" }
                    p { class: "mkv-muted", "打开一个知识库目录开始使用。" }
                }
            } else {
                PreviewPane { ctx }
                BacklinkPanel { ctx }
            }
            if let Some(session) = ctx.edit_session.read().clone() {
                // key 让切换文档时组件整体重挂载：
                // 编辑器内部用 use_signal 持有编辑态，不随 prop 自动重置。
                EditorHost { ctx, session, key: "{session.rel_path}" }
            }
        }
    }
}

/// 预览区（含编辑入口）。
#[component]
pub fn PreviewPane(ctx: VaultContext) -> Element {
    let _revision = *ctx.revision.read();
    let current = ctx.current_doc.read().clone();
    let content = ctx.service.read().read_document(&current).unwrap_or_default();

    // 该格式是否支持内置编辑：
    // ① 后端需支持编辑（Web 端预构建只读索引 → false）；
    // ② 格式需有对应编辑器（md/txt/rtf 无内置编辑器 → false）。
    let backend_editable = ctx.service.read().supports_editing();
    let format_editable = current
        .rsplit('.')
        .next()
        .and_then(mkv_core::Format::from_extension)
        .map(|f| f.has_builtin_editor())
        .unwrap_or(false);
    let editable = backend_editable && format_editable;
    // F-08：外部唤起只在有本地文件的后端可用（Web 只读索引恒为 false）。
    // 单独克隆一份路径，避免与「内置编辑」的事件闭包争用同一个 String。
    let external_path = current.clone();
    let external_available = ctx.service.read().supports_external_open();

    // PRD §7.1：按格式把原文切成带位置标签的片段（与索引产物共用同一套切分函数）。
    // 有定位时只渲染命中片段及其前后各一段，视觉上即「跳到精确位置」。
    let format = current
        .rsplit('.')
        .next()
        .and_then(mkv_core::Format::from_extension)
        .unwrap_or(mkv_core::Format::Markdown);
    let segments = mkv_core::locate::segment_text(&content, format);
    let focus = ctx.focus.read().clone();
    let focus_idx = focus.as_ref().map(|f| f.segment_index);
    let visible: Vec<mkv_core::locate::TextSegment> = match focus_idx {
        Some(i) if i < segments.len() => {
            let lo = i.saturating_sub(1);
            let hi = (i + 2).min(segments.len());
            segments[lo..hi].to_vec()
        }
        _ => segments.clone(),
    };

    rsx! {
        section { class: "mkv-preview",
            div { class: "mkv-preview-header",
                h2 { "{current}" }
                div { class: "mkv-preview-actions",
                    if editable {
                        button {
                            class: "mkv-btn mkv-btn-sm",
                            onclick: move |_| {
                                let mut session = ctx.edit_session;
                                let mut status = ctx.status;
                                match ctx.service.read().open_editor(&current) {
                                    Some(s) => {
                                        session.set(Some(s));
                                        status.set("已进入内置编辑模式".to_string());
                                    }
                                    None => {
                                        status.set("该文档不支持内置编辑".to_string());
                                    }
                                }
                            },
                            "内置编辑"
                        }
                    } else if !backend_editable {
                        span { class: "mkv-muted", "Web 端为只读索引" }
                    }
                    if external_available {
                        button {
                            class: "mkv-btn mkv-btn-sm",
                            onclick: move |_| {
                                let mut status = ctx.status;
                                match ctx.service.read().open_externally(&external_path) {
                                    Ok(()) => status.set("已用系统默认应用打开".to_string()),
                                    Err(e) => status.set(format!("外部唤起失败: {e}")),
                                }
                            },
                            "用系统应用打开"
                        }
                    }
                }
            }
            div { class: "mkv-preview-body",
                if let Some(loc) = focus.as_ref() {
                    div { class: "mkv-location-bar",
                        span { class: "mkv-location-target", "📍 已定位：{loc.label}" }
                        span { class: "mkv-muted mkv-location-snippet", "{loc.snippet}" }
                        button {
                            class: "mkv-btn mkv-btn-sm",
                            onclick: move |_| ctx.focus.set(None),
                            "查看全文"
                        }
                    }
                }
                pre { class: "mkv-doc-content",
                    for seg in visible.iter() {
                        span {
                            class: if Some(seg.index) == focus_idx {
                                "mkv-segment mkv-segment-active"
                            } else {
                                "mkv-segment"
                            },
                            "{seg.text}"
                        }
                    }
                }
            }
        }
    }
}

/// 侧栏图谱图例：节点数 / 边数与节点清单（纯展示，点选跳转在图谱画布里完成）。
#[component]
pub fn GraphLegend(ctx: VaultContext) -> Element {
    let _revision = *ctx.revision.read();
    let graph = ctx.service.read().graph_snapshot();

    rsx! {
        div { class: "mkv-graph-legend",
            p { class: "mkv-muted", "{graph.nodes.len()} 个节点 · {graph.edges.len()} 条边" }
            if graph.nodes.is_empty() {
                p { class: "mkv-muted", "当前知识库还没有双链关系。" }
            } else {
                for node in graph.nodes.clone() {
                    div { class: "mkv-graph-legend-item",
                        span { "{node.title}" }
                        span { class: "mkv-muted", "{node.degree} 条连接" }
                    }
                }
            }
        }
    }
}

/// 图谱画布：SVG 环形布局 + 缩放 + 点选节点跳转（F-10）。
#[component]
pub fn GraphPane(ctx: VaultContext) -> Element {
    let _revision = *ctx.revision.read();
    let graph = ctx.service.read().graph_snapshot();
    let mut zoom = use_signal(|| 1.0f32);
    let scale = *zoom.read();
    let cx = graph.width / 2.0;
    let cy = graph.height / 2.0;
    let view_box = format!("0 0 {} {}", graph.width, graph.height);
    let transform = format!(
        "translate({cx} {cy}) scale({scale}) translate(-{cx} -{cy})"
    );
    let node_count = graph.nodes.len();
    let edge_count = graph.edges.len();

    rsx! {
        section { class: "mkv-graph",
            div { class: "mkv-graph-header",
                h2 { "双链关系图谱" }
                span { class: "mkv-muted", "{node_count} 个节点 · {edge_count} 条边" }
                div { class: "mkv-graph-tools",
                    button {
                        class: "mkv-btn mkv-btn-sm",
                        onclick: move |_| {
                            let current = *zoom.peek();
                            zoom.set((current * 1.25).min(2.5));
                        },
                        "放大 +"
                    }
                    button {
                        class: "mkv-btn mkv-btn-sm",
                        onclick: move |_| {
                            let current = *zoom.peek();
                            zoom.set((current / 1.25).max(0.5));
                        },
                        "缩小 −"
                    }
                    button {
                        class: "mkv-btn mkv-btn-sm",
                        onclick: move |_| { zoom.set(1.0); },
                        "重置"
                    }
                }
            }
            div { class: "mkv-graph-canvas",
                svg {
                    class: "mkv-graph-svg",
                    view_box: "{view_box}",
                    g { transform: "{transform}",
                        for edge in graph.edges.clone() {
                            if let (Some(a), Some(b)) =
                                (graph.nodes.get(edge.from), graph.nodes.get(edge.to))
                            {
                                line {
                                    class: "mkv-graph-edge",
                                    x1: "{a.x}", y1: "{a.y}",
                                    x2: "{b.x}", y2: "{b.y}",
                                }
                            }
                        }
                        for node in graph.nodes.clone() {
                            GraphNodeMark { ctx, node }
                        }
                    }
                }
                if node_count == 0 {
                    p { class: "mkv-muted", "知识库为空，暂无可绘制的双链关系。" }
                }
            }
        }
    }
}

/// 单个图谱节点：点击后回到预览视图并选中该文档。
#[component]
pub fn GraphNodeMark(ctx: VaultContext, node: mkv_core::locate::GraphNodeView) -> Element {
    let path = node.path.clone();
    let title: String = node.title.chars().take(10).collect();
    let x = node.x;
    let y = node.y;
    let r = 7.0 + (node.degree.min(4) as f32) * 1.8;

    rsx! {
        g {
            class: "mkv-graph-node",
            onclick: move |_| {
                let mut current_doc = ctx.current_doc;
                let mut backlinks = ctx.backlinks;
                let mut list_mode = ctx.list_mode;
                current_doc.set(path.clone());
                backlinks.set(ctx.service.read().backlinks(&path));
                ctx.focus.set(None);
                list_mode.set(ListMode::Documents);
            },
            circle { class: "mkv-graph-node-dot", cx: "{x}", cy: "{y}", r: "{r}" }
            text { class: "mkv-graph-node-label", x: "{x}", y: "{y}", "{title}" }
        }
    }
}

/// 双链面板：显示当前文档的反链。
#[component]
pub fn BacklinkPanel(ctx: VaultContext) -> Element {
    let _revision = *ctx.revision.read();
    let current = ctx.current_doc.read().clone();
    let backlinks = ctx.backlinks.read().clone();

    rsx! {
        aside { class: "mkv-backlinks",
            h3 { "反向链接" }
            if current.is_empty() {
                p { class: "mkv-muted", "选择一个文档以查看引用它的笔记。" }
            } else if backlinks.is_empty() {
                p { class: "mkv-muted", "暂无文档引用它。" }
            } else {
                ul { class: "mkv-backlink-list",
                    for link in backlinks.iter() {
                        li { class: "mkv-backlink",
                            span { class: "mkv-backlink-target", "{link.from_path}" }
                            span { class: "mkv-muted", " → {link.target}" }
                        }
                    }
                }
            }
        }
    }
}

/// 底部状态栏：索引进度。
#[component]
pub fn StatusBar(ctx: VaultContext) -> Element {
    let progress = ctx.progress.read().clone();
    let pct = (progress * 100.0).round() as i64;

    rsx! {
        footer { class: "mkv-statusbar",
            if pct > 0 && pct < 100 {
                div { class: "mkv-progress",
                    div { class: "mkv-progress-bar", style: "width:{pct}%" }
                    span { "{pct}%" }
                }
            }
        }
    }
}
