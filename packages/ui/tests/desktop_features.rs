//! Desktop 端新增能力的 UI 级回归：
//! - F-08 外部唤起入口只在「有本地文件」的后端出现；
//! - 自动刷新的 `VaultContext::refresh()` 会递增修订号并重算缓存视图。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use dioxus::prelude::*;
use mkv_core::service::KnowledgeService;
use mkv_core::types::{DocumentRecord, LinkRecord, SearchHit};
use mkv_ui::{PreviewPane, VaultContext};

fn render(app: fn() -> Element) -> String {
    let mut dom = VirtualDom::new(app);
    dom.rebuild(&mut dioxus_core::NoOpMutations);
    dioxus_ssr::render(&dom)
}

/// 模拟 Desktop 的本地知识库服务：支持外部唤起、可编辑。
struct LocalService {
    opened: Arc<AtomicUsize>,
}

impl KnowledgeService for LocalService {
    fn root(&self) -> String {
        "D:/MyVault".into()
    }

    fn document_count(&self) -> usize {
        3
    }

    fn documents(&self) -> Vec<DocumentRecord> {
        Vec::new()
    }

    fn search(&self, _query: &str, _limit: usize) -> Vec<SearchHit> {
        Vec::new()
    }

    fn read_document(&self, _rel_path: &str) -> Option<String> {
        Some("模具台账正文".into())
    }

    fn backlinks(&self, _rel_path: &str) -> Vec<LinkRecord> {
        vec![LinkRecord {
            from_path: "01-笔记/双链示例.md".into(),
            to_path: "03-台账/模具台账.xlsx".into(),
            target: "模具台账".into(),
            granularity: "doc".into(),
        }]
    }

    fn outgoing_links(&self, _rel_path: &str) -> Vec<LinkRecord> {
        Vec::new()
    }

    fn supports_editing(&self) -> bool {
        true
    }

    fn supports_external_open(&self) -> bool {
        true
    }

    fn open_externally(&self, _rel_path: &str) -> mkv_core::Result<()> {
        self.opened.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

/// 模拟 Web 只读后端：trait 默认实现即「不支持外部唤起」。
struct ReadOnlyService;

impl KnowledgeService for ReadOnlyService {
    fn root(&self) -> String {
        String::new()
    }
    fn document_count(&self) -> usize {
        0
    }
    fn search(&self, _query: &str, _limit: usize) -> Vec<SearchHit> {
        Vec::new()
    }
    fn read_document(&self, _rel_path: &str) -> Option<String> {
        Some("只读正文".into())
    }
    fn backlinks(&self, _rel_path: &str) -> Vec<LinkRecord> {
        Vec::new()
    }
    fn outgoing_links(&self, _rel_path: &str) -> Vec<LinkRecord> {
        Vec::new()
    }
}

fn local_ctx(opened: Arc<AtomicUsize>) -> VaultContext {
    let ctx = VaultContext::with_service(Box::new(LocalService { opened }));
    let mut current_doc = ctx.current_doc;
    current_doc.set("03-台账/模具台账.xlsx".to_string());
    ctx
}

#[component]
fn LocalPreviewHost() -> Element {
    let ctx = local_ctx(Arc::new(AtomicUsize::new(0)));
    rsx! { PreviewPane { ctx } }
}

#[component]
fn ReadOnlyPreviewHost() -> Element {
    let ctx = VaultContext::with_service(Box::new(ReadOnlyService));
    let mut current_doc = ctx.current_doc;
    current_doc.set("台账.xlsx".to_string());
    rsx! { PreviewPane { ctx } }
}

#[test]
fn native_backend_shows_external_open_button() {
    let html = render(LocalPreviewHost);
    assert!(
        html.contains("用系统应用打开"),
        "本地知识库应提供 F-08 外部唤起入口：{html}"
    );
}

#[test]
fn readonly_backend_hides_external_open_button() {
    let html = render(ReadOnlyPreviewHost);
    assert!(
        !html.contains("用系统应用打开"),
        "只读后端没有本地文件，不应出现外部唤起按钮：{html}"
    );
}

/// `Signal::set` 必须在 Dioxus runtime 内执行，因此用真实组件承载该用例。
#[component]
fn RefreshHost() -> Element {
    let ctx = local_ctx(Arc::new(AtomicUsize::new(0)));
    let mut query = ctx.query;
    query.set("模具".to_string());
    use_hook(|| {
        ctx.refresh();
        ctx.refresh();
    });
    let rev = *ctx.revision.read();
    let links = ctx.backlinks.read().len();
    let status = ctx.status.read().clone();
    rsx! {
        div { class: "refresh-probe",
            span { "rev={rev}" }
            span { "links={links}" }
            span { "{status}" }
        }
    }
}

#[test]
fn refresh_bumps_revision_and_recomputes_backlinks() {
    let html = render(RefreshHost);

    assert!(html.contains("rev=2"), "两次变更应递增到 2：{html}");
    assert!(html.contains("links=1"), "反链应按新索引重算：{html}");
    assert!(
        html.contains("已自动刷新 · 3 个文档"),
        "状态栏应反映自动刷新：{html}"
    );
}

#[test]
fn readonly_backend_external_open_returns_explicit_error() {
    let err = ReadOnlyService
        .open_externally("台账.xlsx")
        .expect_err("只读后端必须明确报错，不能静默失败");
    assert!(err.to_string().contains("不支持外部唤起"));
}
