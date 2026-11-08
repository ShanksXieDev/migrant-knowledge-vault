//! F-06 跨格式引用的 UI 级回归（SSR）：
//! 预览区在引用处内联显示源片段、刷新入口、未解析状态，以及旧索引产物的运行期补算。

use dioxus::prelude::*;
use mkv_core::format::Format;
use mkv_core::prebuilt::{PrebuiltDoc, PrebuiltIndex, PrebuiltService};
use mkv_core::refs::build_reference_views;
use mkv_ui::{PreviewPane, VaultContext};

fn render(app: fn() -> Element) -> String {
    let mut dom = VirtualDom::new(app);
    dom.rebuild(&mut dioxus_core::NoOpMutations);
    dioxus_ssr::render(&dom)
}

const EXCEL: &str = "台账: 零件号 | 模具编号\n台账: PN-1024 | MDL-08\n台账: PN-2048 | MDL-11\n";
const NOTE: &str = "台账行：@台账.xlsx#第2行\n结尾文本。\n";
const BROKEN: &str = "缺目标：@不存在.xlsx#第1行\n";
const PLAIN: &str = "台账行：普通说明，没有引用语法。\n结尾文本。\n";

/// `with_refs = false` 模拟「旧产物没有 refs 字段」。
fn index(note: &str, with_refs: bool) -> PrebuiltIndex {
    let all = vec![
        ("台账.xlsx".to_string(), EXCEL.to_string(), Format::Excel),
        ("note.md".to_string(), note.to_string(), Format::Markdown),
    ];
    let refs = if with_refs {
        build_reference_views(note, &all)
    } else {
        Vec::new()
    };
    PrebuiltIndex {
        schema_version: 1,
        generated_at: 0,
        root: "测试库".into(),
        docs: vec![
            PrebuiltDoc {
                path: "台账.xlsx".into(),
                title: "模具台账".into(),
                format: "Excel".into(),
                size: 1,
                modified_at: 0,
                text: EXCEL.into(),
                segments: Vec::new(),
                refs: Vec::new(),
                tags: Vec::new(),
                nodes: Vec::new(),
            },
            PrebuiltDoc {
                path: "note.md".into(),
                title: "引用笔记".into(),
                format: "Markdown".into(),
                size: 1,
                modified_at: 0,
                text: note.into(),
                segments: Vec::new(),
                refs,
                tags: Vec::new(),
                nodes: Vec::new(),
            },
        ],
        links: Vec::new(),
        postings: Vec::new(),
        graph_positions: Vec::new(),
    }
}

fn host(note: &'static str, with_refs: bool) -> VaultContext {
    let ctx = VaultContext::with_service(Box::new(PrebuiltService::new(index(note, with_refs))));
    let mut current_doc = ctx.current_doc;
    current_doc.set("note.md".to_string());
    ctx
}

#[component]
fn RefHost() -> Element {
    let ctx = host(NOTE, true);
    rsx! { PreviewPane { ctx } }
}

#[test]
fn preview_inlines_reference_card_with_source_content() {
    let html = render(RefHost);
    assert!(html.contains("mkv-ref-bar"), "应显示引用条：{html}");
    assert!(html.contains("mkv-ref-card"), "应内联引用卡片：{html}");
    assert!(html.contains("PN-1024"), "卡片应显示源片段内容：{html}");
    assert!(html.contains("第 2 行"), "卡片应显示位置标签：{html}");
    assert!(html.contains("刷新引用"), "应提供刷新入口：{html}");
    assert!(html.contains("台账行：") && html.contains("结尾文本"), "正文切分必须无损：{html}");
}

#[component]
fn LegacyHost() -> Element {
    let ctx = host(NOTE, false);
    rsx! { PreviewPane { ctx } }
}

#[test]
fn legacy_index_without_refs_recomputes_at_runtime() {
    let html = render(LegacyHost);
    assert!(
        html.contains("mkv-ref-card") && html.contains("PN-1024"),
        "旧产物缺 refs 字段时应在运行期补算并照常显示：{html}"
    );
}

#[component]
fn PlainHost() -> Element {
    let ctx = host(PLAIN, false);
    rsx! { PreviewPane { ctx } }
}

#[test]
fn document_without_reference_syntax_has_no_ref_ui() {
    let html = render(PlainHost);
    assert!(!html.contains("mkv-ref-bar"), "无引用不应出现引用条：{html}");
    assert!(!html.contains("mkv-ref-card"), "无引用不应出现卡片：{html}");
    assert!(html.contains("结尾文本"), "普通文档正文应完整：{html}");
}

#[component]
fn BrokenHost() -> Element {
    let ctx = host(BROKEN, true);
    rsx! { PreviewPane { ctx } }
}

#[test]
fn unresolved_reference_is_marked_instead_of_faking_content() {
    let html = render(BrokenHost);
    assert!(html.contains("mkv-ref-unresolved"), "应标注未解析：{html}");
    assert!(html.contains("引用未解析"), "应给出明确提示：{html}");
    assert!(!html.contains("PN-1024"), "未解析时不得编造源内容：{html}");
}
