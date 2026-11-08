//! 检索输入框与多维过滤面板（F-12 正则检索 / F-07 多维过滤的前端部分）。
//!
//! 交互契约（与既有 UI 约定一致，见 `tests/readonly_ui.rs`）：
//! - **仅回车触发检索**，检索区不出现任何 `<button>`。因此模式切换用 `span`
//!   承载，过滤面板做成独立组件（由 `SideBar` 挂载），不进 `SearchBar` 的渲染树。
//! - 模式（普通词条 / 正则）与过滤条件的**变更不自动检索**：只更新
//!   `VaultContext` 里的状态并提示「按回车重新检索」；只有「清空过滤」会主动重跑。
//! - 「正则无效」与「没有命中」必须区分：前者用 core 的 `validate_query` 拿到
//!   明确错误文案并清空结果，后者才是零命中。
//! - UI **不引入正则引擎**：命中高亮靠 core 在摘要里夹的哨兵字符切分，
//!   UI 只负责把命中段渲染成 `<mark>`。

use dioxus::prelude::*;
use mkv_core::query::{QueryFilter, QueryMode, SearchQuery};

use crate::VaultContext;

/// 过滤维度：过滤面板里的一枚筹码属于哪一维。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FilterDim {
    /// 格式。
    Format,
    /// 标签。
    Tag,
}

/// 检索输入框：**仅回车触发检索**（无搜索按钮）。
///
/// 中文输入法选词时也会按下回车，此时 `is_composing()` 为 true，
/// 必须跳过，否则用户还没打完词就被检索打断。
///
/// F-12：顶部是「普通词条 / 正则」两态切换；正则模式下输入框下方实时显示
/// 正则错误（由 core 的 `validate_query` 判定），避免用户按下回车才发现写错。
#[component]
pub fn SearchBar(ctx: VaultContext) -> Element {
    let query = ctx.query.read().clone();
    let mode = ctx.query_mode.read().clone();
    let regex_error = validate_regex(ctx, &query, mode);
    let placeholder = if mode == QueryMode::Regex {
        "正则检索…（回车执行，支持 .* 与字符类）"
    } else {
        "搜索知识库…（回车检索，支持中文分词）"
    };
    let hint = if mode == QueryMode::Regex {
        "输入正则表达式后按回车检索"
    } else {
        "输入关键词后按回车检索"
    };

    rsx! {
        div { class: "mkv-searchbar",
            div { class: "mkv-mode-tabs",
                span {
                    class: if mode == QueryMode::Terms { "mkv-mode-tab mkv-mode-active" } else { "mkv-mode-tab" },
                    title: "普通词条检索：查询词分词后按 AND 匹配",
                    onclick: move |_| switch_mode(ctx, QueryMode::Terms),
                    "普通词条"
                }
                span {
                    class: if mode == QueryMode::Regex { "mkv-mode-tab mkv-mode-active" } else { "mkv-mode-tab" },
                    title: "正则检索：查询被当作正则表达式在原文上匹配",
                    onclick: move |_| switch_mode(ctx, QueryMode::Regex),
                    "正则"
                }
            }
            input {
                class: "mkv-search-input",
                placeholder: "{placeholder}",
                title: "{hint}",
                value: "{query}",
                oninput: move |e| ctx.query.set(e.value()),
                onkeydown: move |e| {
                    if e.key() == Key::Enter && !e.is_composing() {
                        run_search(ctx);
                    }
                },
            }
            if let Some(err) = regex_error.as_ref() {
                div { class: "mkv-search-error", "正则无效：{err}" }
            }
        }
    }
}

/// F-07 多维过滤面板：格式 / 标签 / 修改时间窗 / 来源路径 + 清空过滤。
///
/// 面板可折叠（`<details>`）。过滤条件的变更**不自动检索**——每勾一下就全库
/// 重扫代价太高且会让用户失去「先设条件再检索」的控制权；只更新
/// `ctx.query_filter` 并在面板底部提示按回车重新检索。
/// 只有「清空过滤」在点击后会主动重跑一次检索。
#[component]
pub fn FilterPanel(ctx: VaultContext) -> Element {
    let filter = ctx.query_filter.read().clone();
    let formats = format_candidates(&ctx);
    let tags = tag_candidates(&ctx);
    // 日期与来源保存「用户正在输入的原文」：core 存的是 Unix 秒 / 路径前缀，
    // 直接回填会丢掉用户没输完的半截文本。
    let from_text = use_signal(String::new);
    let to_text = use_signal(String::new);
    let source_text = use_signal(String::new);
    let from_value = from_text.read().clone();
    let to_value = to_text.read().clone();
    let source_value = source_text.read().clone();

    let active_count = filter.formats.len()
        + filter.tags.len()
        + usize::from(filter.modified_from.is_some())
        + usize::from(filter.modified_to.is_some())
        + usize::from(
            filter
                .source
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false),
        );
    let summary_text = if active_count == 0 {
        "过滤（F-07）".to_string()
    } else {
        format!("过滤（F-07）· 已启用 {active_count} 项")
    };

    rsx! {
        details { class: "mkv-filter-panel",
            summary { class: "mkv-filter-summary", "{summary_text}" }
            div { class: "mkv-filter-body",
                div { class: "mkv-filter-group",
                    span { class: "mkv-filter-label", "格式" },
                    if formats.is_empty() {
                        span { class: "mkv-muted mkv-filter-empty", "没有可选格式" }
                    } else {
                        div { class: "mkv-chips",
                            for f in formats.iter() {
                                FilterChip {
                                    ctx,
                                    value: f.clone(),
                                    dim: FilterDim::Format,
                                    active: filter.formats.iter().any(|x| x.eq_ignore_ascii_case(f)),
                                }
                            }
                        }
                    }
                }
                // 后端不支持标签过滤时（all_tags 为空）整块不渲染，
                // 而不是渲染一个空的标签区误导用户「没有标签可选」。
                if !tags.is_empty() {
                    div { class: "mkv-filter-group",
                        span { class: "mkv-filter-label", "标签" },
                        div { class: "mkv-chips",
                            for t in tags.iter() {
                                FilterChip {
                                    ctx,
                                    value: t.0.clone(),
                                    dim: FilterDim::Tag,
                                    active: filter.tags.iter().any(|x| x.eq_ignore_ascii_case(&t.0)),
                                }
                            }
                        }
                    }
                }
                div { class: "mkv-filter-group",
                    span { class: "mkv-filter-label", "修改时间窗" },
                    div { class: "mkv-filter-row",
                        input {
                            class: "mkv-filter-input mkv-filter-date",
                            placeholder: "起始 YYYY-MM-DD",
                            title: "修改时间下界：UTC 当天 00:00:00，含当天",
                            value: "{from_value}",
                            oninput: move |e| {
                                let text = e.value();
                                let mut sig = from_text;
                                sig.set(text.clone());
                                set_time_filter(ctx, &text, &to_text.peek().clone());
                            },
                        }
                        input {
                            class: "mkv-filter-input mkv-filter-date",
                            placeholder: "截止 YYYY-MM-DD",
                            title: "修改时间上界：UTC 当天 23:59:59，含当天",
                            value: "{to_value}",
                            oninput: move |e| {
                                let text = e.value();
                                let mut sig = to_text;
                                sig.set(text.clone());
                                set_time_filter(ctx, &from_text.peek().clone(), &text);
                            },
                        }
                    }
                    span { class: "mkv-muted mkv-filter-hint", "按 UTC 日期筛选（闭区间）" }
                }
                div { class: "mkv-filter-group",
                    span { class: "mkv-filter-label", "来源路径" },
                    input {
                        class: "mkv-filter-input",
                        placeholder: "来源前缀，如 03-台账/",
                        title: "只检索来源路径以该前缀开头的文档",
                        value: "{source_value}",
                        oninput: move |e| {
                            let text = e.value();
                            let mut sig = source_text;
                            sig.set(text.clone());
                            set_source_filter(ctx, &text);
                        },
                    }
                }
                div { class: "mkv-filter-actions",
                    button {
                        class: "mkv-btn mkv-btn-sm",
                        onclick: move |_| {
                            let mut f = from_text;
                            f.set(String::new());
                            let mut t = to_text;
                            t.set(String::new());
                            let mut s = source_text;
                            s.set(String::new());
                            let mut query_filter = ctx.query_filter;
                            query_filter.set(QueryFilter::default());
                            // 「清空过滤」是唯一会主动重跑检索的入口。
                            run_search(ctx);
                        },
                        "清空过滤"
                    }
                    span { class: "mkv-muted", "过滤条件变更后按回车重新检索" }
                }
            }
        }
    }
}

/// 过滤面板里的一枚筹码（格式或标签）。
#[component]
pub fn FilterChip(ctx: VaultContext, value: String, dim: FilterDim, active: bool) -> Element {
    let chip_value = value.clone();
    rsx! {
        span {
            class: if active { "mkv-chip mkv-chip-on" } else { "mkv-chip" },
            title: "切换过滤条件：{value}",
            onclick: move |_| toggle_chip(ctx, chip_value.clone(), dim),
            "{value}"
        }
    }
}

/// 切换一枚筹码的选中态。
fn toggle_chip(ctx: VaultContext, value: String, dim: FilterDim) {
    let mut filter = ctx.query_filter.peek().clone();
    match dim {
        FilterDim::Format => {
            if let Some(pos) = filter
                .formats
                .iter()
                .position(|x| x.eq_ignore_ascii_case(&value))
            {
                filter.formats.remove(pos);
            } else {
                filter.formats.push(value);
            }
        }
        FilterDim::Tag => {
            if let Some(pos) = filter.tags.iter().position(|x| x.eq_ignore_ascii_case(&value)) {
                filter.tags.remove(pos);
            } else {
                filter.tags.push(value);
            }
        }
    }
    apply_filter(ctx, filter);
}

/// 写入过滤器并提示「按回车重新检索」。
fn apply_filter(ctx: VaultContext, filter: QueryFilter) {
    let mut query_filter = ctx.query_filter;
    query_filter.set(filter);
    let mut status = ctx.status;
    status.set("过滤条件已更新，按回车重新检索".to_string());
}

/// 把日期文本写进过滤器（UTC 闭区间）。
fn set_time_filter(ctx: VaultContext, from_text: &str, to_text: &str) {
    let mut filter = ctx.query_filter.peek().clone();
    filter.modified_from = crate::date_to_unix_start(from_text);
    filter.modified_to = crate::date_to_unix_end(to_text);
    apply_filter(ctx, filter);
}

/// 把来源前缀写进过滤器。
fn set_source_filter(ctx: VaultContext, text: &str) {
    let mut filter = ctx.query_filter.peek().clone();
    let trimmed = text.trim();
    filter.source = if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    };
    apply_filter(ctx, filter);
}

/// 格式候选项：优先从后端文档表聚合去重。
///
/// 候选项必须与命中里的 `format` 字符串**一致**才能过滤生效（core 用
/// `eq_ignore_ascii_case` 比较），因此这里取后端给的原值，而不是另造一套
/// 别名表——否则会出现「勾了却过滤不到」的假象。
fn format_candidates(ctx: &VaultContext) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    {
        let service = ctx.service.read();
        for doc in service.documents() {
            let f = doc.format.trim();
            if f.is_empty() {
                continue;
            }
            if !out.iter().any(|x| x.eq_ignore_ascii_case(f)) {
                out.push(f.to_string());
            }
        }
    }
    if out.is_empty() {
        // 后端不提供文档表时，退回知识库支持的六种格式。
        out = vec![
            format!("{:?}", mkv_core::Format::Markdown),
            format!("{:?}", mkv_core::Format::Excel),
            format!("{:?}", mkv_core::Format::PowerPoint),
            format!("{:?}", mkv_core::Format::Word),
            format!("{:?}", mkv_core::Format::Visio),
            format!("{:?}", mkv_core::Format::Drawio),
        ];
    }
    out.sort();
    out
}

/// 标签候选项：来自后端的 `all_tags()`（含出现次数）。
///
/// 后端不支持标签过滤时返回空 vec，调用方据此整块隐藏标签区。
fn tag_candidates(ctx: &VaultContext) -> Vec<(String, usize)> {
    let service = ctx.service.read();
    let mut tags = service.all_tags();
    tags.sort_by(|a, b| a.0.cmp(&b.0));
    tags
}

/// 切换检索模式（普通词条 / 正则）。
///
/// 模式切换**不自动检索**：正则与分词语义不同，自动跑一次既慢又容易在用户
/// 还没写完表达式时抛错，只提示按回车。
fn switch_mode(ctx: VaultContext, mode: QueryMode) {
    let mut query_mode = ctx.query_mode;
    query_mode.set(mode);
    let mut status = ctx.status;
    status.set(if mode == QueryMode::Regex {
        "已切换到正则检索，按回车执行".to_string()
    } else {
        "已切换到普通词条检索，按回车重新检索".to_string()
    });
}

/// 正则模式下的实时校验：表达式非法时返回错误说明，合法（或非正则模式）返回 None。
fn validate_regex(ctx: VaultContext, query: &str, mode: QueryMode) -> Option<String> {
    if mode != QueryMode::Regex || query.trim().is_empty() {
        return None;
    }
    let probe = SearchQuery {
        text: query.to_string(),
        mode,
        filter: QueryFilter::default(),
        limit: 1,
    };
    ctx.service.read().validate_query(&probe)
}

/// 执行检索：把「文本 + 模式 + 过滤器」一起交给 core（F-12 / F-07）。
fn run_search(ctx: VaultContext) {
    let query = ctx.query.read().clone();
    let mode = ctx.query_mode.read().clone();
    let filter = ctx.query_filter.read().clone();
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

    let search_query = SearchQuery {
        text: query.clone(),
        mode,
        filter,
        limit: 50,
    };

    // 正则写错 ≠ 没有命中：先用 validate_query 把两者区分开。
    // 非法正则时清空结果并给出明确错误，绝不能显示成「0 条命中」。
    if let Some(err) = ctx.service.read().validate_query(&search_query) {
        results.set(Vec::new());
        list_mode.set(crate::ListMode::Results);
        status.set(format!("正则无效：{err}"));
        return;
    }

    let hits = ctx.service.read().search_query(&search_query);
    let count = hits.len();
    results.set(hits);
    // 检索后自动切到「检索结果」页签，避免用户在「全部文档」里找结果。
    list_mode.set(crate::ListMode::Results);
    if count == 0 {
        status.set(format!("检索「{query}」没有命中，试试放宽过滤条件"));
    } else {
        status.set(format!("检索「{query}」命中 {count} 条"));
    }
}
