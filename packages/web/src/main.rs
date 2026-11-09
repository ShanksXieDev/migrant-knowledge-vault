//! Migrant Knowledge Vault —— Web 端入口（wasm32）。
//!
//! 索引策略：**预构建只读索引**（技术方案设计 §4 / 决策 ②）。
//! 构建期由 native 端（`mkv export-web`）生成索引产物并随站打包，
//! 运行时只读加载，从而规避 wasm 环境实时建索引的性能瓶颈。
//!
//! 加载路径（实现细节见 [`loader`]）：
//! - **主路径**：构建期内嵌产物（`build.rs` + `include_str!`）——零网络、完全离线；
//!   由于产物是编译期常量，可在**首帧同步载入**，因此首屏即完整可用、无空态闪烁；
//! - **可选增强**：运行期远端拉取（`MKV_WEB_INDEX_URL`），失败静默保留内嵌版本。
//!
//! 检索能力完全来自索引产物自带的倒排表：建索引侧（native）用 jieba + 领域词典切词，
//! 查询侧（Web）用产物词表做正向最大匹配，因此 Web 端**不需要** jieba，
//! 从而彻底避开 zstd / C 编译器依赖。

use dioxus::prelude::*;
use mkv_core::prebuilt::{PrebuiltIndex, PrebuiltService};

use mkv_ui::{AppShell, VaultContext};

pub mod loader;

fn main() {
    dioxus::launch(App);
}

/// 应用根组件。
#[component]
fn App() -> Element {
    // 内嵌产物是编译期常量，同步载入 → 首帧即拥有完整索引，
    // 不会出现「先空库、后灌数据」的闪烁，也无需等待网络。
    let ctx = use_hook(|| {
        let index = loader::load_embedded().unwrap_or_else(|_| PrebuiltIndex::empty());
        let doc_count = index.doc_count();
        let root_name = index.root.clone();

        let ctx = VaultContext::with_service(Box::new(PrebuiltService::new(index)));
        let mut status = ctx.status;
        let mut root = ctx.root;
        status.set(loader::describe(doc_count, &loader::IndexSource::Embedded));
        root.set(root_name);
        ctx
    });

    // 远端索引热替换（可选增强）：仅在编译时配置了 MKV_WEB_INDEX_URL 才发起请求。
    // 失败时静默保留内嵌版本 —— 内嵌产物始终是兜底，站点不会因此不可用。
    use_future(move || async move {
        let Some(url) = loader::remote_url() else {
            return;
        };
        let Ok(index) = loader::fetch_remote(url).await else {
            return;
        };
        if index.doc_count() == 0 {
            // 远端返回空索引：视为无效，保留内嵌版本。
            return;
        }

        let doc_count = index.doc_count();
        // 产物自带知识库名，热替换后顶栏也要同步，否则会出现
        // 「文档来自远端新库、标题却是旧库」的不一致。
        let root_name = index.root.clone();
        // Signal::set 需要可变绑定；Signal 本身是 Copy，因此先取副本再写。
        let mut service = ctx.service;
        let mut status = ctx.status;
        let mut root = ctx.root;
        service.set(Box::new(PrebuiltService::new(index)));
        root.set(root_name);
        status.set(loader::describe(
            doc_count,
            &loader::IndexSource::Remote(url.to_string()),
        ));
    });

    rsx! {
        AppShell { ctx }
    }
}

#[cfg(test)]
mod tests {
    // 不再单独 `use dioxus::prelude::*`——`use super::*` 已经把它带进来了，
    // 重复导入反而会触发「unused import」警告。
    use super::*;

    /// 把整个 Web 端界面渲染成 HTML（SSR）。
    ///
    /// 索引在首帧同步载入，因此渲染结果应直接包含真实文档列表 ——
    /// 这验证了「产物 → 内嵌 → 服务 → 侧栏列表」的完整 UI 链路，
    /// 而不只是「JSON 能被解析」。
    fn render_html() -> String {
        let mut dom = VirtualDom::new(App);
        dom.rebuild(&mut dioxus_core::NoOpMutations);
        dioxus_ssr::render(&dom)
    }

    #[test]
    fn renders_shell_with_list_tabs() {
        let html = render_html();
        assert!(html.contains("全部文档"), "应渲染文档页签：{html}");
        assert!(html.contains("检索结果"), "应渲染结果页签：{html}");
        assert!(
            html.contains("Migrant Knowledge Vault"),
            "应渲染应用外壳：{html}"
        );
    }

    #[test]
    fn renders_document_list_from_embedded_index() {
        let html = render_html();

        if !loader::HAS_EMBEDDED_INDEX {
            // 未内嵌产物时只能渲染空态提示
            assert!(
                html.contains("暂无文档"),
                "未内嵌产物时应显示空态：{html}"
            );
            return;
        }

        let index = loader::load_embedded().expect("内嵌索引应可解析");
        let first = index.docs.first().expect("真实产物应含文档");
        assert!(
            html.contains(&first.path),
            "侧栏应列出索引中的文档 {}：{html}",
            first.path
        );
    }

    #[test]
    fn renders_status_with_document_count() {
        let html = render_html();
        let count = loader::load_embedded().expect("内嵌索引应可解析").doc_count();
        if count > 0 {
            assert!(
                html.contains(&format!("{count} 个文档")),
                "状态栏应显示文档数：{html}"
            );
        }
    }

    #[test]
    fn web_ui_has_no_builtin_editor_entry() {
        let html = render_html();
        assert!(
            !html.contains(">内置编辑<"),
            "Web 端为只读索引，不应出现内置编辑入口：{html}"
        );
    }
}
