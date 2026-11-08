//! F-12 正则模式 / F-07 多维过滤的 UI 级回归（SSR）：
//! - 模式切换（普通词条 / 正则）与正则错误提示；
//! - 过滤面板的四个维度与「清空过滤」出口；
//! - 结果项的标签 / 最后修改时间 / 正则命中高亮。

use dioxus::prelude::*;
use mkv_core::query::{MATCH_MARK_BEGIN, MATCH_MARK_END, QueryMode, SearchQuery};
use mkv_core::service::KnowledgeService;
use mkv_core::types::{DocumentRecord, LinkRecord, SearchHit};
use mkv_ui::{FilterPanel, ResultList, SearchBar, VaultContext};

fn render(app: fn() -> Element) -> String {
    let mut dom = VirtualDom::new(app);
    dom.rebuild(&mut dioxus_core::NoOpMutations);
    dioxus_ssr::render(&dom)
}

/// 带过滤能力与正则校验的后端。
struct FilterService;

impl KnowledgeService for FilterService {
    fn root(&self) -> String {
        "测试库".into()
    }

    fn document_count(&self) -> usize {
        2
    }

    fn documents(&self) -> Vec<DocumentRecord> {
        vec![
            DocumentRecord {
                path: "01-笔记/a.md".into(),
                title: "笔记A".into(),
                format: "Markdown".into(),
                size: 1,
                modified_at: 1000,
                content_hash: String::new(),
            },
            DocumentRecord {
                path: "03-台账/b.xlsx".into(),
                title: "台账B".into(),
                format: "Excel".into(),
                size: 1,
                modified_at: 2000,
                content_hash: String::new(),
            },
        ]
    }

    fn search(&self, _query: &str, _limit: usize) -> Vec<SearchHit> {
        Vec::new()
    }

    fn search_query(&self, _query: &SearchQuery) -> Vec<SearchHit> {
        Vec::new()
    }

    /// 只在表达式含未闭合字符类时判为非法正则。
    fn validate_query(&self, query: &SearchQuery) -> Option<String> {
        if query.mode == QueryMode::Regex && query.text.contains('[') {
            Some("未闭合的字符类".to_string())
        } else {
            None
        }
    }

    fn all_tags(&self) -> Vec<(String, usize)> {
        vec![("模具".to_string(), 2), ("工艺".to_string(), 1)]
    }

    fn read_document(&self, _rel_path: &str) -> Option<String> {
        None
    }

    fn backlinks(&self, _rel_path: &str) -> Vec<LinkRecord> {
        Vec::new()
    }

    fn outgoing_links(&self, _rel_path: &str) -> Vec<LinkRecord> {
        Vec::new()
    }
}

fn ctx() -> VaultContext {
    VaultContext::with_service(Box::new(FilterService))
}

// ---------------------------------------------------------------- 模式切换

#[component]
fn TermsSearchHost() -> Element {
    let ctx = ctx();
    rsx! { SearchBar { ctx } }
}

#[test]
fn search_bar_offers_both_query_modes_without_a_button() {
    let html = render(TermsSearchHost);

    assert!(html.contains("普通词条"), "应提供普通词条模式：{html}");
    assert!(html.contains("正则"), "应提供正则模式：{html}");
    assert!(
        !html.contains("<button"),
        "检索区仍不得出现按钮（既有契约）：{html}"
    );
    assert!(html.contains("回车检索"), "placeholder 应提示回车检索：{html}");
}

#[component]
fn RegexSearchHost() -> Element {
    let ctx = ctx();
    let mut mode = ctx.query_mode;
    mode.set(QueryMode::Regex);
    let mut query = ctx.query;
    query.set("模流[".to_string());
    rsx! { SearchBar { ctx } }
}

#[test]
fn regex_mode_shows_invalid_expression_hint() {
    let html = render(RegexSearchHost);

    assert!(
        html.contains("正则无效"),
        "非法正则应给出错误提示而不是「0 条命中」：{html}"
    );
    assert!(html.contains("未闭合的字符类"), "应透出 core 给出的原因：{html}");
}

// ---------------------------------------------------------------- 过滤面板

#[component]
fn FilterHost() -> Element {
    let ctx = ctx();
    rsx! { FilterPanel { ctx } }
}

#[test]
fn filter_panel_exposes_all_four_dimensions_and_clear_entry() {
    let html = render(FilterHost);

    assert!(html.contains("格式"), "应有格式维度：{html}");
    assert!(html.contains("Markdown") && html.contains("Excel"), "应列出格式候选：{html}");
    assert!(html.contains("标签"), "应有标签维度：{html}");
    assert!(html.contains("模具") && html.contains("工艺"), "应列出标签候选：{html}");
    assert!(html.contains("修改时间窗"), "应有修改时间窗：{html}");
    assert!(html.contains("起始 YYYY-MM-DD") && html.contains("截止 YYYY-MM-DD"));
    assert!(html.contains("按 UTC 日期筛选"), "应标明时间窗按 UTC：{html}");
    assert!(html.contains("来源路径"), "应有来源路径维度：{html}");
    assert!(html.contains("清空过滤"), "应提供清空过滤出口：{html}");
    assert!(
        html.contains("按回车重新检索"),
        "应说明过滤变更不自动检索：{html}"
    );
}

// ---------------------------------------------------------------- 结果项

#[component]
fn ResultHost() -> Element {
    let ctx = ctx();
    let snippet = format!(
        "前面的内容{MATCH_MARK_BEGIN}模流分析{MATCH_MARK_END}后面的内容"
    );
    let mut results = ctx.results;
    results.set(vec![SearchHit {
        path: "01-笔记/a.md".into(),
        title: "笔记A".into(),
        format: "Markdown".into(),
        score: 1.0,
        snippet,
        modified_at: 1_700_000_000,
        tags: vec!["模具".into(), "待评审".into()],
    }]);
    rsx! { ResultList { ctx } }
}

#[test]
fn result_item_shows_tags_modified_time_and_marked_hit() {
    let html = render(ResultHost);

    assert!(html.contains("mkv-tag"), "应显示文档标签：{html}");
    assert!(html.contains("模具") && html.contains("待评审"), "应显示标签名：{html}");
    assert!(html.contains("2023-11-14"), "应显示最后修改时间：{html}");
    assert!(html.contains("<mark"), "正则命中段应高亮：{html}");
    assert!(html.contains("模流分析"), "应保留命中内容：{html}");
}
