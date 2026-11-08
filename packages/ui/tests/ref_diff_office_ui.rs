//! F-06 进阶②/③ 的 UI 级回归（SSR）：
//!
//! - 进阶②「源变更差异高亮」：引用卡片在源片段相对上次快照发生变化时，
//!   必须渲染差异块（新增行 / 删除行分开着色），没有变化时不渲染。
//! - 进阶③「原生 Office 应用内定位」：只有**支持本地进程**的后端才显示
//!   「在 Office 中打开」按钮；Web 只读端必须隐藏（不谎报能力）。

use std::collections::HashMap;

use dioxus::prelude::*;
use mkv_core::format::Format;
use mkv_core::prebuilt::{PrebuiltDoc, PrebuiltIndex, PrebuiltService};
use mkv_core::refs::{build_reference_views_with_snapshots, snapshot_key, ReferenceView};
use mkv_core::service::KnowledgeService;
use mkv_core::types::{LinkRecord, SearchHit};
use mkv_ui::{PreviewPane, VaultContext};

fn render(app: fn() -> Element) -> String {
    let mut dom = VirtualDom::new(app);
    dom.rebuild(&mut dioxus_core::NoOpMutations);
    dioxus_ssr::render(&dom)
}

const EXCEL: &str = "台账: 零件号 | 模具编号\n台账: PN-1024 | MDL-08\n台账: PN-2048 | MDL-11\n";
const NOTE: &str = "台账行：@台账.xlsx#第2行\n结尾文本。\n";

/// 源片段「上次看到」的旧内容——与当前 EXCEL 第 2 行不同，因此会产生差异。
const OLD_SNAPSHOT: &str = "台账: PN-1024 | MDL-99";

/// 构造一份带 `refs` 的索引产物。
fn index_with_refs(refs: Vec<ReferenceView>) -> PrebuiltIndex {
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
                text: NOTE.into(),
                segments: Vec::new(),
                refs,
                tags: Vec::new(),
                nodes: Vec::new(),
            },
        ],
        links: Vec::new(),
        postings: Vec::new(),
        graph_positions: Vec::new(),
        ref_snapshots: Default::default(),
    }
}

/// 生成引用视图；`snapshots` 为空表示「首次见到该引用」。
fn refs_with_snapshots(snapshots: &HashMap<String, String>) -> Vec<ReferenceView> {
    let all = vec![
        ("台账.xlsx".to_string(), EXCEL.to_string(), Format::Excel),
        ("note.md".to_string(), NOTE.to_string(), Format::Markdown),
    ];
    let empty: HashMap<String, Vec<mkv_core::diagram::DiagramNode>> = HashMap::new();
    build_reference_views_with_snapshots(NOTE, &all, &empty, snapshots)
}

fn host(index: PrebuiltIndex) -> VaultContext {
    let ctx = VaultContext::with_service(Box::new(PrebuiltService::new(index)));
    let mut current_doc = ctx.current_doc;
    current_doc.set("note.md".to_string());
    ctx
}

// ---------- 进阶②：源变更差异高亮 ----------

#[component]
fn DiffHost() -> Element {
    let mut snapshots = HashMap::new();
    snapshots.insert(
        snapshot_key("台账.xlsx", "第2行"),
        OLD_SNAPSHOT.to_string(),
    );
    let ctx = host(index_with_refs(refs_with_snapshots(&snapshots)));
    rsx! { PreviewPane { ctx } }
}

#[test]
fn changed_source_renders_diff_block_with_added_and_removed_lines() {
    let html = render(DiffHost);
    assert!(
        html.contains("mkv-ref-diff"),
        "源片段变更时应渲染差异块：{html}"
    );
    assert!(
        html.contains("mkv-ref-diff-added"),
        "新增行应被单独着色：{html}"
    );
    assert!(
        html.contains("mkv-ref-diff-removed"),
        "删除行应被单独着色：{html}"
    );
    assert!(
        html.contains("源已变更"),
        "差异块应给出可读标题：{html}"
    );
    assert!(
        html.contains("PN-1024"),
        "差异块应包含差异行的实际文本：{html}"
    );
}

#[component]
fn NoDiffHost() -> Element {
    // 空快照 = 首次见到该引用：没有可比对的基线，不应编造差异。
    let ctx = host(index_with_refs(refs_with_snapshots(&HashMap::new())));
    rsx! { PreviewPane { ctx } }
}

#[test]
fn first_sighting_has_no_diff_block() {
    let html = render(NoDiffHost);
    assert!(
        !html.contains("mkv-ref-diff"),
        "没有快照基线时不应出现差异块：{html}"
    );
    assert!(
        html.contains("mkv-ref-card"),
        "引用卡片本身仍应正常渲染：{html}"
    );
}

#[component]
fn UnchangedHost() -> Element {
    // 快照内容与当前完全一致 → 指纹未变 → 不产出差异。
    let mut snapshots = HashMap::new();
    snapshots.insert(
        snapshot_key("台账.xlsx", "第2行"),
        "台账: PN-1024 | MDL-08".to_string(),
    );
    let ctx = host(index_with_refs(refs_with_snapshots(&snapshots)));
    rsx! { PreviewPane { ctx } }
}

#[test]
fn unchanged_source_keeps_diff_block_absent() {
    let html = render(UnchangedHost);
    assert!(
        !html.contains("mkv-ref-diff"),
        "源片段未变时不应出现差异块：{html}"
    );
}

// ---------- 进阶③：原生 Office 应用内定位 ----------

/// 支持本地进程的后端（模拟 Desktop / Mobile）。
struct OfficeCapable {
    inner: PrebuiltService,
}

impl OfficeCapable {
    fn new(index: PrebuiltIndex) -> Self {
        Self {
            inner: PrebuiltService::new(index),
        }
    }
}

impl KnowledgeService for OfficeCapable {
    fn root(&self) -> String {
        self.inner.root()
    }
    fn document_count(&self) -> usize {
        self.inner.document_count()
    }
    fn documents(&self) -> Vec<mkv_core::types::DocumentRecord> {
        self.inner.documents()
    }
    fn search(&self, query: &str, limit: usize) -> Vec<SearchHit> {
        self.inner.search(query, limit)
    }
    fn search_query(&self, query: &mkv_core::query::SearchQuery) -> Vec<SearchHit> {
        self.inner.search_query(query)
    }
    fn read_document(&self, rel_path: &str) -> Option<String> {
        self.inner.read_document(rel_path)
    }
    fn backlinks(&self, rel_path: &str) -> Vec<LinkRecord> {
        self.inner.backlinks(rel_path)
    }
    fn outgoing_links(&self, rel_path: &str) -> Vec<LinkRecord> {
        self.inner.outgoing_links(rel_path)
    }
    fn status(&self) -> String {
        self.inner.status()
    }
    fn locate(&self, rel_path: &str, query: &str) -> Option<mkv_core::locate::HitLocation> {
        self.inner.locate(rel_path, query)
    }
    fn graph_snapshot(&self) -> mkv_core::locate::GraphSnapshot {
        self.inner.graph_snapshot()
    }
    fn references(&self, rel_path: &str) -> Vec<ReferenceView> {
        self.inner.references(rel_path)
    }

    // —— 进阶③ 覆写 ——
    fn supports_office_locate(&self) -> bool {
        true
    }
}

#[component]
fn OfficeHost() -> Element {
    let ctx = VaultContext::with_service(Box::new(OfficeCapable::new(index_with_refs(
        refs_with_snapshots(&HashMap::new()),
    ))));
    let mut current_doc = ctx.current_doc;
    current_doc.set("note.md".to_string());
    rsx! { PreviewPane { ctx } }
}

#[test]
fn office_button_shown_when_backend_supports_local_process() {
    let html = render(OfficeHost);
    assert!(
        html.contains("mkv-ref-office"),
        "支持原生唤起的后端应显示「在 Office 中打开」：{html}"
    );
    assert!(
        html.contains("在 Office 中打开"),
        "按钮应有可读文案：{html}"
    );
}

#[test]
fn office_button_hidden_on_readonly_web_service() {
    // Web 只读端用默认 trait 实现（supports_office_locate = false）。
    let html = render(NoDiffHost);
    assert!(
        !html.contains("mkv-ref-office"),
        "Web 只读端不得显示「在 Office 中打开」（会点了必然失败）：{html}"
    );
}
