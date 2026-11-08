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
        }
    }

    /// 使用空服务创建上下文（Web 端占位）。
    pub fn new() -> Self {
        Self::with_service(Box::new(mkv_core::service::NullService))
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
            }
            // 注意：rsx 的子节点不支持 match（会被当成原始 Rust 表达式，
            // 使 `DocList { ctx }` 解析到组件补全枚举上），这里用 if/else 分支。
            if mode == ListMode::Documents {
                DocList { ctx }
            } else {
                ResultList { ctx }
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

    rsx! {
        button {
            class: if is_active { "mkv-result mkv-result-active" } else { "mkv-result" },
            onclick: move |_| {
                let mut current_doc = ctx.current_doc;
                let mut backlinks = ctx.backlinks;
                current_doc.set(hit.path.clone());
                backlinks.set(ctx.service.read().backlinks(&hit.path));
            },
            div { class: "mkv-result-title", "{hit.title}" }
            div { class: "mkv-result-meta",
                span { "{hit.format}" }
                span { class: "mkv-muted", "{hit.path}" }
            }
        }
    }
}

/// 右侧主区：预览 + 双链面板。
#[component]
pub fn MainPane(ctx: VaultContext) -> Element {
    let current = ctx.current_doc.read().clone();

    rsx! {
        main { class: "mkv-main",
            if current.is_empty() {
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
                }
            }
            div { class: "mkv-preview-body",
                pre { class: "mkv-doc-content", "{content}" }
            }
        }
    }
}

/// 双链面板：显示当前文档的反链。
#[component]
pub fn BacklinkPanel(ctx: VaultContext) -> Element {
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
