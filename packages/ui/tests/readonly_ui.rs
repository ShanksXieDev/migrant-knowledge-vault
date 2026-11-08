//! Web 端只读语义的 UI 级验证。
//!
//! Web 端使用预构建只读索引，**不应**出现内置编辑入口（GridCraft / DeckCraft /
//! WordCraft）。这个约束由 `KnowledgeService::supports_editing()` 控制，
//! 若哪天被误改，界面会给出「保存不了」的假象 —— 因此在这里用真实渲染守住。

use dioxus::prelude::*;
use mkv_core::prebuilt::{PrebuiltIndex, PrebuiltService};
use mkv_ui::{PreviewPane, SearchBar, VaultContext};

/// 渲染一个无 props 的组件为 HTML。
fn render(app: fn() -> Element) -> String {
    let mut dom = VirtualDom::new(app);
    dom.rebuild(&mut dioxus_core::NoOpMutations);
    dioxus_ssr::render(&dom)
}

/// 构造一个只读（Web 端）上下文，并选中一篇文档。
fn readonly_ctx() -> VaultContext {
    let index = PrebuiltIndex {
        schema_version: 1,
        generated_at: 0,
        root: "测试库".into(),
        docs: vec![mkv_core::prebuilt::PrebuiltDoc {
            path: "台账.xlsx".into(),
            title: "模具台账".into(),
            format: "Excel".into(),
            size: 1,
            modified_at: 0,
            text: "零件号 / 模具编号".into(),
            segments: Vec::new(),
        }],
        links: Vec::new(),
        postings: Vec::new(),
    };
    let ctx = VaultContext::with_service(Box::new(PrebuiltService::new(index)));
    let mut current_doc = ctx.current_doc;
    current_doc.set("台账.xlsx".to_string());
    ctx
}

/// 预览区宿主组件（`VirtualDom::new` 需要无捕获的 `fn() -> Element`）。
#[component]
fn PreviewHost() -> Element {
    let ctx = readonly_ctx();
    rsx! { PreviewPane { ctx } }
}

#[test]
fn readonly_backend_hides_builtin_editor_entry() {
    let html = render(PreviewHost);

    assert!(
        !html.contains(">内置编辑<"),
        "只读后端不应渲染「内置编辑」按钮：{html}"
    );
    assert!(
        html.contains("Web 端为只读索引"),
        "应给出只读提示：{html}"
    );
}

#[test]
fn preview_still_shows_document_body() {
    let html = render(PreviewHost);

    assert!(html.contains("台账.xlsx"), "应显示文档路径：{html}");
    assert!(html.contains("零件号"), "应显示抽取出的正文：{html}");
}

/// 检索框宿主组件。
#[component]
fn SearchHost() -> Element {
    let ctx = readonly_ctx();
    rsx! { SearchBar { ctx } }
}

/// 检索交互契约：**仅回车触发**，检索区不得再出现「搜索」按钮。
/// 浏览器回归脚本也会验证，这里用 SSR 让 `cargo test --workspace` 同样守住它。
#[test]
fn search_bar_has_no_button_and_requires_enter() {
    let html = render(SearchHost);

    assert!(
        html.contains("mkv-searchbar") && html.contains("mkv-search-input"),
        "应渲染检索输入框：{html}"
    );
    assert!(
        !html.contains("<button") && !html.contains("mkv-search-btn") && !html.contains(">搜索<"),
        "检索区不应有搜索按钮：{html}"
    );
    assert!(
        html.contains("回车检索"),
        "placeholder 应提示按回车检索：{html}"
    );
}
