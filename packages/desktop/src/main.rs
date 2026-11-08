//! Migrant Knowledge Vault —— 桌面端入口。
//!
//! v0.1 目标：跑通「打开知识库 → 扫描 → 建立索引 → 检索 → 预览 → 双链」主链路。
//!
//! 用法：
//! ```text
//! mkv-desktop <知识库目录>
//! ```
//! 不带参数时使用当前工作目录。

use dioxus::prelude::*;
use mkv_core::native::NativeService;
use mkv_ui::{AppShell, VaultContext};

fn main() {
    // 插件化编辑器：启动时把三个独立插件 crate 注入 core 注册表。
    // Web 只读端不链接 mkv-editors，因此 wasm 产物里没有编辑器实现。
    if let Err(e) = mkv_editors::install() {
        eprintln!("[mkv] 编辑器插件安装失败: {e}");
    }

    let args: Vec<String> = std::env::args().collect();
    let root = args.get(1).cloned().unwrap_or_else(|| ".".to_string());

    // 启动即打开知识库并建立索引
    match mkv_core::native::open(&root, true) {
        Ok(()) => match mkv_core::native::reindex() {
            Ok(report) => println!(
                "[mkv] 已索引 {} 个文档（{} 条链接，{} ms）",
                report.indexed, report.links_found, report.elapsed_ms
            ),
            Err(e) => eprintln!("[mkv] 建立索引失败: {e}"),
        },
        Err(e) => eprintln!("[mkv] 打开知识库失败: {e}"),
    }

    // P0：后台文件监听 —— 文档保存后增量更新索引与双链（PRD 要求 1 秒内）。
    // 复用 native 的全局 Vault，不另开一个实例去抢 `.kbindex/.lock`。
    let watch_root = std::path::PathBuf::from(&root);
    std::thread::spawn(move || {
        let options = mkv_core::watcher::WatchOptions::default();
        let result = mkv_core::watcher::run_with(
            &watch_root,
            &options,
            |changes| mkv_core::native::apply_changes(changes),
            |report| {
                println!(
                    "[mkv-watch] {}",
                    mkv_core::watcher::describe_report(report)
                )
            },
        );
        if let Err(e) = result {
            eprintln!("[mkv-watch] 监听结束: {e}");
        }
    });

    dioxus::launch(App);
}

/// 应用根组件。
#[component]
fn App() -> Element {
    let ctx = use_hook(|| VaultContext::with_service(Box::new(NativeService)));

    // 同步知识库路径与文档数
    use_effect(move || {
        // Signal::set 需要可变绑定；Signal 为 Copy，可先取副本再写。
        let mut root = ctx.root;
        let mut status = ctx.status;
        let service = ctx.service.read();
        root.set(service.root());
        let count = service.document_count();
        status.set(format!("已就绪 · {count} 个文档"));
    });

    // 自动刷新：后台监听线程只负责更新索引并 bump 原子修订号，
    // 这里在 UI runtime 内轮询并写 Signal（Dioxus Signal 不能跨线程写）。
    // 250ms 轮询与 P0 的 1 秒指标对齐：索引落盘后最多 250ms 界面即刷新。
    use_future(move || async move {
        let mut last = mkv_core::native::revision();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            let now = mkv_core::native::revision();
            if now != last {
                last = now;
                ctx.refresh();
            }
        }
    });

    rsx! {
        AppShell { ctx }
    }
}
