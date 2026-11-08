//! Migrant Knowledge Vault —— 移动端入口（iOS / Android）。
//!
//! v0.1 状态：已打通「与 Desktop 共用 core + ui」的骨架。
//! 移动端受限项与后续适配列入 v1.0（PRD §7.2）：
//! - 文件访问依赖平台沙箱授权（Android SAF / iOS Documents）。
//! - 桌面端「外部唤起编辑」在移动端不可用，仅内置编辑器可用。

use dioxus::prelude::*;
use mkv_core::native::NativeService;
use mkv_ui::{AppShell, VaultContext};

fn main() {
    // 移动端只能用内置编辑器（无法外部唤起），同样需要注入插件。
    if let Err(e) = mkv_editors::install() {
        eprintln!("[mkv] 编辑器插件安装失败: {e}");
    }
    dioxus::launch(App);
}

/// 应用根组件。
#[component]
fn App() -> Element {
    let ctx = use_hook(|| VaultContext::with_service(Box::new(NativeService)));

    use_effect(move || {
        // Signal::set 需要可变绑定；Signal 为 Copy，可先取副本再写。
        let mut root = ctx.root;
        let mut status = ctx.status;
        let service = ctx.service.read();
        root.set(service.root());
        let count = service.document_count();
        status.set(format!("已就绪 · {count} 个文档"));
    });

    rsx! {
        AppShell { ctx }
    }
}
