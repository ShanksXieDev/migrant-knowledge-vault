//! 新增能力的 UI 级回归（SSR）：
//! - PRD §7.1 检索结果精确定位：结果列表位置标签、预览区定位条与片段高亮；
//! - F-10 双链关系图谱：页签、SVG 节点 / 边、图例。

use dioxus::prelude::*;
use mkv_core::locate::{build_graph_snapshot, GraphSnapshot, HitLocation};
use mkv_core::service::KnowledgeService;
use mkv_core::types::{DocumentRecord, LinkRecord, SearchHit};
use mkv_ui::{AppShell, ListMode, PreviewPane, ResultList, VaultContext};

fn render(app: fn() -> Element) -> String {
    let mut dom = VirtualDom::new(app);
    dom.rebuild(&mut dioxus_core::NoOpMutations);
    dioxus_ssr::render(&dom)
}

const DOC_TEXT: &str = "第一段开场。\n\n第二段工艺。\n\n第三段讲模流分析与浇口位置。\n\n第四段参数。\n\n第五段收尾。\n";

fn hit_location() -> HitLocation {
    HitLocation {
        label: "第 3 段".into(),
        kind: "paragraph".into(),
        segment_index: 2,
        matched: "浇口".into(),
        snippet: "第三段讲模流分析与浇口位置。".into(),
    }
}

/// 同时支持定位与图谱的后端（模拟注入了完整能力的 PrebuiltService）。
struct LocateGraphService;

impl KnowledgeService for LocateGraphService {
    fn root(&self) -> String {
        "测试库".into()
    }

    fn document_count(&self) -> usize {
        2
    }

    fn documents(&self) -> Vec<DocumentRecord> {
        vec![
            DocumentRecord {
                path: "a.md".into(),
                title: "笔记A".into(),
                format: "Markdown".into(),
                size: 1,
                modified_at: 0,
                content_hash: String::new(),
            },
            DocumentRecord {
                path: "b.md".into(),
                title: "笔记B".into(),
                format: "Markdown".into(),
                size: 1,
                modified_at: 0,
                content_hash: String::new(),
            },
        ]
    }

    fn search(&self, _query: &str, _limit: usize) -> Vec<SearchHit> {
        vec![SearchHit {
            path: "a.md".into(),
            title: "笔记A".into(),
            format: "Markdown".into(),
            score: 1.0,
            snippet: "第三段讲模流分析与浇口位置。".into(),
        }]
    }

    fn read_document(&self, _rel_path: &str) -> Option<String> {
        Some(DOC_TEXT.into())
    }

    fn backlinks(&self, _rel_path: &str) -> Vec<LinkRecord> {
        Vec::new()
    }

    fn outgoing_links(&self, _rel_path: &str) -> Vec<LinkRecord> {
        Vec::new()
    }

    fn locate(&self, _rel_path: &str, _query: &str) -> Option<HitLocation> {
        Some(hit_location())
    }

    fn graph_snapshot(&self) -> GraphSnapshot {
        let nodes = vec![
            ("a.md".to_string(), "笔记A".to_string()),
            ("b.md".to_string(), "笔记B".to_string()),
        ];
        let links = vec![("a.md".to_string(), "b.md".to_string())];
        build_graph_snapshot(&nodes, &links)
    }
}

fn ctx() -> VaultContext {
    VaultContext::with_service(Box::new(LocateGraphService))
}

// ---------------------------------------------------------------- 图谱

#[component]
fn ShellHost() -> Element {
    let ctx = ctx();
    rsx! { AppShell { ctx } }
}

#[test]
fn shell_exposes_graph_tab() {
    let html = render(ShellHost);
    assert!(
        html.contains("关系图谱"),
        "侧栏应提供图谱入口：{html}"
    );
}

#[component]
fn GraphHost() -> Element {
    let ctx = ctx();
    let mut mode = ctx.list_mode;
    mode.set(ListMode::Graph);
    rsx! { AppShell { ctx } }
}

#[test]
fn graph_pane_renders_svg_nodes_and_edges() {
    let html = render(GraphHost);
    assert!(html.contains("双链关系图谱"), "应渲染图谱标题：{html}");
    assert!(html.contains("<svg"), "应渲染 SVG 画布：{html}");
    assert!(html.contains("mkv-graph-node"), "应渲染节点：{html}");
    assert!(html.contains("mkv-graph-edge"), "应渲染双链边：{html}");
    assert!(html.contains("笔记A") && html.contains("笔记B"));
    assert!(html.contains("2 个节点 · 1 条边"), "应给出图谱统计：{html}");
}

// ---------------------------------------------------------------- 精确定位

#[component]
fn ResultHost() -> Element {
    let ctx = ctx();
    let mut query = ctx.query;
    query.set("浇口".to_string());
    let mut results = ctx.results;
    results.set(LocateGraphService.search("浇口", 20));
    rsx! { ResultList { ctx } }
}

#[test]
fn result_item_shows_position_label() {
    let html = render(ResultHost);
    assert!(html.contains("mkv-hit-location"), "结果应带位置区域：{html}");
    assert!(html.contains("第 3 段"), "应显示精确位置标签：{html}");
}

#[component]
fn FocusHost() -> Element {
    let ctx = ctx();
    let mut current_doc = ctx.current_doc;
    current_doc.set("a.md".to_string());
    let mut focus = ctx.focus;
    focus.set(Some(hit_location()));
    rsx! { PreviewPane { ctx } }
}

#[test]
fn preview_shows_location_bar_and_active_segment() {
    let html = render(FocusHost);
    assert!(html.contains("mkv-location-bar"), "应显示定位条：{html}");
    assert!(html.contains("已定位"), "定位条应说明命中位置：{html}");
    assert!(
        html.contains("mkv-segment-active"),
        "命中片段应高亮：{html}"
    );
    assert!(html.contains("第三段讲模流分析"), "应显示命中片段：{html}");
    assert!(
        !html.contains("第一段开场"),
        "聚焦模式应只显示命中附近片段：{html}"
    );
    assert!(html.contains("查看全文"), "应提供查看全文出口：{html}");
}

#[component]
fn FullTextHost() -> Element {
    let ctx = ctx();
    let mut current_doc = ctx.current_doc;
    current_doc.set("a.md".to_string());
    rsx! { PreviewPane { ctx } }
}

#[test]
fn preview_without_focus_shows_full_text() {
    let html = render(FullTextHost);
    assert!(!html.contains("mkv-location-bar"), "无定位时不显示定位条");
    assert!(html.contains("第一段开场") && html.contains("第五段收尾"));
}
