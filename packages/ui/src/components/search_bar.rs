//! 检索输入框组件。

use dioxus::prelude::*;

use crate::VaultContext;

/// 检索输入框：**仅回车触发检索**（无搜索按钮）。
///
/// 中文输入法选词时也会按下回车，此时 `is_composing()` 为 true，
/// 必须跳过，否则用户还没打完词就被检索打断。
#[component]
pub fn SearchBar(ctx: VaultContext) -> Element {
    let query = ctx.query.read().clone();

    rsx! {
        div { class: "mkv-searchbar",
            input {
                class: "mkv-search-input",
                placeholder: "搜索知识库…（回车检索，支持中文分词）",
                title: "输入关键词后按回车检索",
                value: "{query}",
                oninput: move |e| ctx.query.set(e.value()),
                onkeydown: move |e| {
                    if e.key() == Key::Enter && !e.is_composing() {
                        run_search(ctx);
                    }
                },
            }
        }
    }
}

/// 执行检索：调用注入的知识库服务。
fn run_search(ctx: VaultContext) {
    let query = ctx.query.read().clone();
    // Signal::set 需要可变绑定；Signal 本身是 Copy，因此先取副本再写。
    let mut results = ctx.results;
    let mut status = ctx.status;
    let mut list_mode = ctx.list_mode;

    if query.trim().is_empty() {
        results.set(Vec::new());
        // 清空条件后回到「全部文档」，避免停在一个空的「检索结果」页签里。
        list_mode.set(crate::ListMode::Documents);
        status.set(String::from("就绪"));
        return;
    }

    let hits = ctx.service.read().search(&query, 50);
    let count = hits.len();
    results.set(hits);
    // 检索后自动切到「检索结果」页签，避免用户在「全部文档」里找结果。
    list_mode.set(crate::ListMode::Results);
    status.set(format!("检索「{query}」命中 {count} 条"));
}
