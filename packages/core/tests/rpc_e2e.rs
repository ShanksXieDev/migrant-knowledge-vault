//! 进程外 JSON-RPC 后端的**跨平台**端到端测试。
//!
//! # 为什么用自制 runner（`harness = false`）
//!
//! 本文件需要同时扮演两个角色：
//! ① 测试运行器；② 被测客户端 spawn 起来的「迷你上游」。
//! 默认 libtest harness 无法在两者之间切换；而用 `sh -c` / `cmd /C` 起服务器
//! 在两端语法完全不同（`cmd` 没有 `while read`）——历史上正是这一点让
//! 端到端测试全部退化成 `#[cfg(unix)]`，**Windows 上零覆盖**，
//! 而本项目的主力开发环境恰恰是 Windows。
//!
//! 这里的做法：**启动自身**（`std::env::current_exe()`）并带上 `--server=<模式>`
//! 参数，服务器模式与测试模式由该参数区分。
//! 因为 `harness = false`，argv 由我们自己解析，不受 libtest 参数规则约束。
//! 于是 Windows / Linux / macOS 跑的是**同一份**测试代码。
//!
//! # 覆盖的用例
//!
//! - 单次调用的写入 → 读回 → 解析 → `result` 取值；
//! - 上游返回 JSON-RPC `error` 时的错误透传；
//! - 上游进程提前退出时的识别；
//! - **超时**：上游挂起不回应时，调用必须在 `timeout_ms` 内返回明确错误，
//!   且**不得**永久挂起（客户端被 `Mutex` 保护，挂起会锁死宿主 UI）；
//! - 超时后的**自愈**：再次调用仍能返回（而不是被上一次的残留状态卡死）；
//! - `open → view → apply → serialize` **完整四步链路**（走真实进程）。

use std::io::{BufRead, Write};

use mkv_core::backend::{
    decode_base64, encode_base64, methods, ExternalBackend, ExternalConfig, JsonRpcRequest,
    PluginBackend,
};
use mkv_core::format::Format;
use mkv_core::plugin::{ParagraphView, PluginAction, PluginView};
use mkv_core::rpc::JsonRpcClient;
use serde_json::{json, Value};

/// 服务器模式参数前缀。
const SERVER_FLAG: &str = "--server=";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(arg) = args.iter().find(|a| a.starts_with(SERVER_FLAG)) {
        server_main(&arg[SERVER_FLAG.len()..]);
        return;
    }
    run_tests();
}

// ---------------------------------------------------------------- 服务器

/// 迷你上游：按 JSON-RPC 2.0 over stdio 应答。
///
/// 模式：
/// - `echo`：把 `method` 原样放进 `result.echo`；
/// - `error`：返回 JSON-RPC 错误；
/// - `exit`：立刻退出（不读 stdin）；
/// - `hang`：读到请求但**不回应**（用于超时测试）；
/// - `mini`：完整的迷你上游，实现 open / view / apply / serialize / version。
fn server_main(mode: &str) {
    if mode == "exit" {
        std::process::exit(0);
    }
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let mut stdout = std::io::stdout();
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return, // 客户端关闭 stdin 或出错
            Ok(_) => {}
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(req) = serde_json::from_str::<JsonRpcRequest>(trimmed) else {
            continue; // 不是 JSON-RPC 请求，忽略
        };
        let resp = match mode {
            "hang" => continue, // 故意不回应
            "error" => rpc_response(
                req.id,
                None,
                Some((-32000, "boom")),
            ),
            "mini" => mini_dispatch(&req),
            _ => rpc_response(req.id, Some(json!({ "echo": req.method })), None),
        };
        let text = serde_json::to_string(&resp).expect("响应可序列化");
        if writeln!(stdout, "{text}").is_err() || stdout.flush().is_err() {
            return;
        }
    }
}

/// 构造一条 JSON-RPC 响应。
fn rpc_response(id: u64, result: Option<Value>, error: Option<(i64, &str)>) -> Value {
    let mut v = json!({ "jsonrpc": "2.0", "id": id });
    if let Some(r) = result {
        v["result"] = r;
    }
    if let Some((code, message)) = error {
        v["error"] = json!({ "code": code, "message": message });
    }
    v
}

/// 迷你上游的四类操作实现。
fn mini_dispatch(req: &JsonRpcRequest) -> Value {
    let params = req.params.clone().unwrap_or(Value::Null);
    let state_of = |key: &str| -> Vec<u8> {
        params
            .get(key)
            .and_then(|v| v.as_str())
            .and_then(|s| decode_base64(s).ok())
            .unwrap_or_default()
    };
    match req.method.as_str() {
        methods::OPEN => rpc_response(
            req.id,
            Some(json!({ "state_b64": encode_base64(&state_of("bytes_b64")) })),
            None,
        ),
        methods::VIEW => {
            let text = String::from_utf8_lossy(&state_of("state_b64")).to_string();
            let view = PluginView::Word {
                paragraphs: vec![ParagraphView {
                    index: 0,
                    text,
                    style: "body".to_string(),
                }],
                style_options: vec!["body".to_string(), "title".to_string()],
            };
            rpc_response(
                req.id,
                Some(serde_json::to_value(&view).expect("视图可序列化")),
                None,
            )
        }
        methods::APPLY => {
            let mut state = state_of("state_b64");
            let action = params
                .get("action")
                .cloned()
                .and_then(|a| serde_json::from_value::<PluginAction>(a).ok());
            if let Some(PluginAction::SetParagraph { text, .. }) = action {
                state = text.into_bytes();
            }
            rpc_response(
                req.id,
                Some(json!({ "state_b64": encode_base64(&state) })),
                None,
            )
        }
        methods::SERIALIZE => rpc_response(
            req.id,
            Some(json!({ "bytes_b64": encode_base64(&state_of("state_b64")) })),
            None,
        ),
        methods::VERSION => rpc_response(req.id, Some(json!({ "version": "0.1.0-mini" })), None),
        _ => rpc_response(req.id, None, Some((-32601, "method not found"))),
    }
}

// ---------------------------------------------------------------- 测试 runner

fn run_tests() {
    let cases: Vec<(&str, fn())> = vec![
        ("calls_method_and_returns_result", case_calls_method),
        ("surfaces_remote_error", case_remote_error),
        ("detects_process_exit", case_process_exit),
        ("times_out_when_upstream_hangs", case_timeout),
        ("client_recovers_after_timeout", case_recover_after_timeout),
        ("health_reports_upstream_version", case_health),
        ("full_open_view_apply_serialize_chain", case_full_chain),
    ];
    let total = cases.len();
    let mut failed = 0usize;
    for (name, case) in cases {
        // 每个用例独立捕获 panic：一个用例失败不应中断其余用例。
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(case));
        match outcome {
            Ok(()) => println!("PASS  {name}"),
            Err(e) => {
                failed += 1;
                let msg = e
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "未知 panic".to_string());
                println!("FAIL  {name} :: {msg}");
            }
        }
    }
    println!("\n===== rpc_e2e：{}/{} 项通过 =====", total - failed, total);
    if failed > 0 {
        std::process::exit(1);
    }
}

/// 造一个指向**本测试二进制自身**的客户端。
fn client(mode: &str, timeout_ms: u64) -> JsonRpcClient {
    let exe = std::env::current_exe().expect("能取到当前测试二进制路径");
    JsonRpcClient::new(ExternalConfig {
        command: exe.to_string_lossy().to_string(),
        args: vec![format!("{SERVER_FLAG}{mode}")],
        timeout_ms,
    })
}

fn case_calls_method() {
    let mut c = client("echo", 5_000);
    let out = c
        .call(methods::OPEN, json!({ "x": 1 }))
        .expect("调用应成功");
    assert_eq!(out["echo"], json!(methods::OPEN));
    // 同��客户端应复用同一进程（第二次调用同样成功）。
    let out2 = c
        .call(methods::VIEW, json!({ "x": 2 }))
        .expect("第二次调用应成功");
    assert_eq!(out2["echo"], json!(methods::VIEW));
}

fn case_remote_error() {
    let mut c = client("error", 5_000);
    let err = c
        .call(methods::VIEW, json!({}))
        .expect_err("上游错误应透传为 Err");
    assert!(
        err.to_string().contains("boom"),
        "错误信息应含上游消息，实际：{err}"
    );
}

fn case_process_exit() {
    let mut c = client("exit", 5_000);
    let err = c
        .call(methods::OPEN, json!({}))
        .expect_err("进程退出应报错");
    let text = err.to_string();
    assert!(
        text.contains("退出") || text.contains("失败") || text.contains("超时"),
        "应能识别进程退出，实际：{text}"
    );
}

/// 超时：上游挂起不回应时，调用必须在时限内返回明确错误。
fn case_timeout() {
    let started = std::time::Instant::now();
    let mut c = client("hang", 800);
    let err = c
        .call(methods::OPEN, json!({}))
        .expect_err("上游挂起应超时");
    let elapsed = started.elapsed();
    let text = err.to_string();
    assert!(text.contains("超时"), "错误信息应说明超时，实际：{text}");
    assert!(
        text.contains("800"),
        "错误信息应含超时毫秒数，实际：{text}"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "超时应按时返回，实际耗时 {elapsed:?}"
    );
}

/// 超时之后客户端必须**可继续使用**：再次调用仍应返回错误而不是永久挂起。
///
/// 这一条盯的是「超时不只是返回了错误，还必须把状态清理干净」——
/// 若残留的读线程或半截响应还在，下一次调用就会读到错位的内容。
fn case_recover_after_timeout() {
    let mut c = client("hang", 600);
    let first = c.call(methods::OPEN, json!({})).expect_err("首次应超时");
    assert!(first.to_string().contains("超时"), "{first}");

    let started = std::time::Instant::now();
    let second = c
        .call(methods::OPEN, json!({}))
        .expect_err("超时后再次调用仍应返回错误（而不是挂死）");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "超时后的再次调用不应挂死，实际耗时 {:?}",
        started.elapsed()
    );
    assert!(
        second.to_string().contains("超时") || second.to_string().contains("退出"),
        "第二次调用的错误应可解释，实际：{second}"
    );
}

fn case_health() {
    let mut c = client("mini", 5_000);
    let h = c.health("mini");
    assert!(h.available, "迷你上游应可用：{h:?}");
    assert_eq!(h.version, "0.1.0-mini");
}

/// `open → view → apply → serialize` **完整四步**，全程走真实进程。
fn case_full_chain() {
    let exe = std::env::current_exe().expect("能取到当前测试二进制路径");
    let backend = ExternalBackend::new(
        "mini",
        Format::Excel,
        ExternalConfig {
            command: exe.to_string_lossy().to_string(),
            args: vec![format!("{SERVER_FLAG}mini")],
            timeout_ms: 5_000,
        },
    );

    // ① 打开
    let mut doc = backend.open("原始内容".as_bytes()).expect("打开应成功");
    assert_eq!(doc.state, "原始内容".as_bytes().to_vec());

    // ② 取视图
    let view = backend.view(&doc).expect("取视图应成功");
    match view {
        PluginView::Word { paragraphs, .. } => {
            assert_eq!(paragraphs.len(), 1);
            assert_eq!(paragraphs[0].text, "原始内容");
        }
        other => panic!("期望 Word 视图，实际 {other:?}"),
    }

    // ③ 应用动作
    backend
        .apply(
            &mut doc,
            &PluginAction::SetParagraph {
                index: 0,
                text: "改后内容".to_string(),
            },
        )
        .expect("应用动作应成功");
    assert_eq!(doc.state, "改后内容".as_bytes().to_vec());

    // ④ 序列化写回
    let bytes = backend
        .serialize(&doc, "原始内容".as_bytes())
        .expect("序列化应成功");
    assert_eq!(bytes, "改后内容".as_bytes().to_vec());
}
