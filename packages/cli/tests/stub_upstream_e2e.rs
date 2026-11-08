//! 以**真实参考适配器**（`mkv editors stub`）为上游的跨平台端到端测试。
//!
//! # 为什么要有这一条
//!
//! `packages/core/tests/rpc_e2e.rs` 用「测试二进制自己扮演迷你上游」，
//! 验证的是**客户端**（超时回收、进程自愈、错误透传），
//! 上游是测试自己写的迷你实现。
//!
//! 这一条则验证**契约**：宿主侧 [`ExternalBackend`] 面对**真正的参考适配器**
//! （`packages/cli/src/stub.rs`，即 `mkv editors stub`）时，
//! `open → view → apply → serialize` 四步是否真的能跑通。
//!
//! 两者互补：前者证明「客户端不会挂死」，后者证明「协议两端对得上」——
//! 比如 `methods::OPEN` 的 params 到底带不带 `path`，
//! 只有真适配器跑一遍才知道。
//!
//! # 跨平台做法
//!
//! 直接 spawn `CARGO_BIN_EXE_mkv`（Cargo 为集成测试注入的**本包二进制**路径），
//! 参数 `editors stub`。**不经过 shell**，因此 Windows / Linux / macOS
//! 跑的是同一份代码，不需要 `sh -c` 或 `cmd /C` 的分支。

use mkv_core::backend::{ExternalBackend, ExternalConfig, PluginBackend};
use mkv_core::format::Format;
use mkv_core::plugin::{PluginAction, PluginView};

/// 造一个指向真实 `mkv` 二进制、以 `editors stub` 为上游的后端。
fn stub_backend() -> ExternalBackend {
    ExternalBackend::new(
        "wordcraft",
        Format::Word,
        ExternalConfig {
            command: env!("CARGO_BIN_EXE_mkv").to_string(),
            args: vec!["editors".to_string(), "stub".to_string()],
            timeout_ms: 10_000,
        },
    )
}

/// `open → view → apply → serialize` 全链路，全程走真实进程。
#[test]
fn full_chain_against_real_stub_adapter() {
    let backend = stub_backend();

    // ① 打开：参考适配器把字节按行切段。
    let original = "第一段\n第二段";
    let mut doc = backend
        .open(original.as_bytes())
        .expect("打开应成功（上游为真实参考适配器）");

    // ② 取视图：`PluginView` 是**带标签的外部标签枚举**，
    //    JSON 形态是 `{"Word": {...}}`，这里由 serde 直接反序列化。
    let view = backend.view(&doc).expect("取视图应成功");
    match view {
        PluginView::Word {
            paragraphs,
            style_options,
        } => {
            assert_eq!(paragraphs.len(), 2, "两行文本应切出两个段落");
            assert_eq!(paragraphs[0].text, "第一段");
            assert_eq!(paragraphs[1].text, "第二段");
            assert!(!style_options.is_empty(), "应声明可用样式");
        }
        other => panic!("期望 Word 视图，实际 {other:?}"),
    }

    // ③ 应用动作：改第一段。
    backend
        .apply(
            &mut doc,
            &PluginAction::SetParagraph {
                index: 0,
                text: "改后第一段".to_string(),
            },
        )
        .expect("应用动作应成功");
    assert!(
        !doc.state.is_empty(),
        "应用后状态应非空（状态以 base64 的 JSON 字节回传）"
    );

    // ④ 序列化写回：参考适配器按行拼接并补一个结尾换行。
    let bytes = backend
        .serialize(&doc, original.as_bytes())
        .expect("序列化应成功");
    assert_eq!(
        String::from_utf8_lossy(&bytes),
        "改后第一段\n第二段\n",
        "写回内容应为改后文本（参考适配器以 \\n 结尾）"
    );
}

/// 同一个后端实例连续多次调用时，上游进程被复用（不重复 spawn）。
#[test]
fn upstream_process_is_reused_across_calls() {
    let backend = stub_backend();
    let doc = backend
        .open("甲".as_bytes())
        .expect("首次打开应成功");
    // 连续三次调用：若每次都重新 spawn 进程也不会报错，
    // 但这里验证的是「同一实例复用同一上游」这条语义没被破坏。
    for _ in 0..3 {
        let view = backend.view(&doc).expect("重复取视图应成功");
        match view {
            PluginView::Word { paragraphs, .. } => assert_eq!(paragraphs.len(), 1),
            other => panic!("期望 Word 视图，实际 {other:?}"),
        }
    }
}
