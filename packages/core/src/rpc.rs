//! JSON-RPC 2.0 over stdio 客户端（仅 native 目标）。
//!
//! 用于连接上游 MCP server（`gridcraft-mcp` / `deckcraft-mcp` / `wordcraft-mcp`）。
//!
//! # 协议
//!
//! 每行一条 JSON（**换行分隔**，不是 LSP 那种 `Content-Length` 头）：
//!
//! ```text
//! → {"jsonrpc":"2.0","id":1,"method":"mkv/editor/open","params":{...}}
//! ← {"jsonrpc":"2.0","id":1,"result":{...}}
//! ```
//!
//! # 生命周期
//!
//! 客户端**惰性启动**子进程：第一次调用时 spawn，之后复用同一进程。
//! 进程异常退出时，下次调用会重新 spawn（自愈）。
//!
//! # 超时
//!
//! [`ExternalConfig::timeout_ms`] 是**单次调用的时限**，本次调用超过时限即返回错误，
//! 绝不无限期挂起——客户端被 `Mutex` 保护，一次挂起会锁死整个宿主 UI。
//!
//! 超时的实现是**跨平台**的：把「读一行」交给独立线程，
//! 主线程用 `mpsc::recv_timeout` 等待。
//! 之所以不用 `set_nonblocking` + 轮询：那是 **Unix 独有**的 API，
//! Windows 上没有，写进 `#[cfg(unix)]` 就等于让 Windows 继续裸奔。
//!
//! # 读线程的生命周期
//!
//! stdout 是**一次性流**：一个进程只有一条响应序列，不能为每次调用各起一条读线程
//! （两条线程会抢同一份字节，响应从此错位）。因此这里的做法是：
//!
//! - 每次调用**只读一行**，读线程读完后把 `BufReader` **原样交还**给客户端
//!   （连同它内部可能已缓冲的字节），下一次调用接着用——等于读线程「随客户端存活」；
//! - 客户端被终止时（`shutdown()`），读线程阻塞在 `read_line` 上，
//!   上游进程一死，管道 EOF 让 `read_line` 返回 0，线程**自然结束**，不会泄漏。
//!
//! # 超时后为什么整个客户端作废
//!
//! 超时后**整个客户端作废**：进程被终止、`BufReader` 连同其中可能已读了一半的
//! 字节一起丢弃。下次调用会重新 spawn 一个干净的进程。
//! 这一步是必需的——若只是丢弃「本次响应」而留着缓冲，
//! 下一次读到的会是**上一条请求的半截响应**，请求与响应从此错位。
//!
//! 「作废」的判定不需要额外的布尔标志：`self.stdout` 被 `take()` 走后就是 `None`，
//! 而 `ensure_started()` 的复用条件是「stdin 与 stdout **都在**」——
//! stdout 为 `None` 时复用条件自然不成立，下次调用必定重新 spawn。
//! 这也是为什么超时时**绝不能**把读取器放回 `self.stdout`。
//!
//! # 为什么不用 LSP 的 Content-Length 分帧
//!
//! MCP 的 stdio 传输用换行分隔 JSON。LSP 的 `Content-Length` 头是另一套协议，
//! 用错会导致对端完全无法解析。跟随上游的既有实现是最省事也最不容易错的选择。

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Duration;

use serde_json::{json, Value};

use crate::backend::{BackendHealth, BackendKind, ExternalConfig, JsonRpcRequest, JsonRpcResponse};
use crate::error::{Error, Result};

/// JSON-RPC over stdio 客户端。
pub struct JsonRpcClient {
    config: ExternalConfig,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    /// stdout 读取器。
    ///
    /// 取走（`take`）意味着「当前不在本线程手里」——读取期间它在读线程中。
    /// 超时后它**不会被放回**：缓冲里可能残留半截响应，放回就会造成响应错位。
    stdout: Option<BufReader<ChildStdout>>,
    next_id: u64,
}

impl JsonRpcClient {
    /// 创建客户端（**不启动进程**，首次调用时才 spawn）。
    pub fn new(config: ExternalConfig) -> Self {
        Self {
            config,
            child: None,
            stdin: None,
            stdout: None,
            next_id: 0,
        }
    }

    /// 单次调用的时限。
    fn timeout(&self) -> Duration {
        Duration::from_millis(self.config.timeout_ms.max(1))
    }

    /// 启动（或复用）子进程。
    fn ensure_started(&mut self) -> Result<()> {
        if self.stdin.is_some() && self.stdout.is_some() {
            return Ok(());
        }
        // 上一次进程可能已退出，或上次调用超时后已被作废：先清理再重启。
        self.shutdown();

        let mut cmd = Command::new(&self.config.command);
        cmd.args(&self.config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child = cmd.spawn().map_err(|e| {
            Error::Config(format!(
                "无法启动上游编辑器进程 `{}`：{e}",
                self.config.command
            ))
        })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Config("上游进程未提供 stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Config("上游进程未提供 stdout".into()))?;

        self.child = Some(child);
        self.stdin = Some(stdin);
        self.stdout = Some(BufReader::new(stdout));
        Ok(())
    }

    /// 调用一个方法，返回 `result` 字段。
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.ensure_started()?;

        self.next_id += 1;
        let id = self.next_id;
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.to_string(),
            params: Some(params),
        };
        let line = serde_json::to_string(&req)
            .map_err(|e| Error::Config(format!("JSON-RPC 请求序列化失败: {e}")))?;

        {
            let stdin = self
                .stdin
                .as_mut()
                .ok_or_else(|| Error::Config("上游进程 stdin 已关闭".into()))?;
            stdin
                .write_all(line.as_bytes())
                .and_then(|_| stdin.write_all(b"\n"))
                .and_then(|_| stdin.flush())
                .map_err(|e| {
                    Error::Config(format!("向上游进程写入失败（进程可能已退出）: {e}"))
                })?;
        }

        let response_line = self.read_line_with_timeout()?;
        let resp: JsonRpcResponse = serde_json::from_str(response_line.trim()).map_err(|e| {
            Error::Config(format!("上游进程响应不是合法 JSON-RPC: {e}"))
        })?;

        if let Some(err) = resp.error {
            return Err(Error::Config(format!(
                "上游进程返回错误（code {}）：{}",
                err.code, err.message
            )));
        }

        resp.result
            .ok_or_else(|| Error::Config("上游进程响应缺少 result 字段".into()))
    }

    /// 读一行上游响应，**带超时**。
    ///
    /// 读操作在独立线程里做（跨平台唯一的可靠做法），
    /// 主线程用 [`std::sync::mpsc::RecvTimeoutError`] 区分三种结局：
    ///
    /// - 正常收到 → 把读取器**放回** `self.stdout`，进程继续复用；
    /// - **超时** → 整个客户端作废（终止进程 + 丢弃缓冲），下次调用重启；
    /// - 读取出错 / 线程结束 → 同样作废（进程多半已退出）。
    ///
    /// 「作废时丢弃缓冲」是这套设计的关键：缓冲里可能已经有半截响应，
    /// 留着它下一次就会读到**错位的响应**。
    fn read_line_with_timeout(&mut self) -> Result<String> {
        let Some(reader) = self.stdout.take() else {
            return Err(Error::Config("上游进程 stdout 已关闭".into()));
        };
        let timeout = self.timeout();
        let timeout_ms = self.config.timeout_ms;

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut line = String::new();
            // 保留读到的字节数：`0` 表示 EOF（上游已退出且没写任何东西），
            // 与「读到了空行」是两回事——前者要报「进程已退出」。
            let res = reader.read_line(&mut line).map(|n| (n, line));
            // 把读取器一并回传：主线程超时则不取，本线程结束时它随 BufReader 一起释放。
            let _ = tx.send((res, reader));
        });

        let started = std::time::Instant::now();
        match rx.recv_timeout(timeout) {
            Ok((Ok((n, line)), reader)) => {
                if n == 0 {
                    // EOF：上游进程已退出且没有写任何内容。
                    // 作废客户端，下次调用会重新 spawn（自愈）。
                    self.shutdown();
                    return Err(Error::Config("上游进程已退出（无响应）".into()));
                }
                self.stdout = Some(reader);
                Ok(line)
            }
            Ok((Err(e), _reader)) => {
                // 读取出错：进程多半已退出，作废客户端以便下次自愈重启。
                self.shutdown();
                Err(Error::Config(format!(
                    "读取上游进程响应失败（进程可能已退出）: {e}"
                )))
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // 超时：进程与缓冲一并作废（见模块文档「超时」一节）。
                self.shutdown();
                // 错误文案同时给出**配置时限**与**实测等待时长**：
                // 前者是契约，后者让排查者能分清「等满时限仍未响应」
                // 与其它失败原因。
                let waited = started.elapsed().as_millis();
                Err(Error::Config(format!(
                    "调用上游进程超时（{timeout_ms} 毫秒）：上游未在时限内响应，\
                     已等待 {waited} 毫秒，已终止该进程并将在下次调用时重启"
                )))
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                self.shutdown();
                Err(Error::Config(
                    "上游进程读取线程已结束（进程可能已退出）".into(),
                ))
            }
        }
    }

    /// 终止子进程（幂等）。
    pub fn shutdown(&mut self) {
        self.stdin.take();
        // stdout 若正在读线程中，这里拿不到——那正是「超时」分支的形态，
        // 读线程会在进程被终止后自行结束并释放读取器。
        self.stdout.take();
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// 健康检查：能否启动并取到版本。
    pub fn health(&mut self, id: &str) -> BackendHealth {
        match self.call(crate::backend::methods::VERSION, json!({})) {
            Ok(result) => {
                let version = result
                    .get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("未知")
                    .to_string();
                BackendHealth {
                    id: id.to_string(),
                    kind: BackendKind::External,
                    available: true,
                    reason: String::new(),
                    version,
                }
            }
            Err(e) => BackendHealth {
                id: id.to_string(),
                kind: BackendKind::External,
                available: false,
                reason: e.to_string(),
                version: String::new(),
            },
        }
    }
}

impl Drop for JsonRpcClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 命令不存在时健康检查必须失败并给出原因（不 spawn 任何进程）。
    #[test]
    fn reports_health_failure_when_command_missing() {
        let mut client = JsonRpcClient::new(ExternalConfig {
            command: "definitely-not-a-real-command-xyz".to_string(),
            args: vec![],
            timeout_ms: 1_000,
        });
        let health = client.health("gridcraft");
        assert!(!health.available);
        assert!(!health.reason.is_empty());
    }

    /// 超时时限来自配置，而不是硬编码。
    #[test]
    fn timeout_follows_configuration() {
        let client = JsonRpcClient::new(ExternalConfig {
            command: "whatever".to_string(),
            args: vec![],
            timeout_ms: 2_500,
        });
        assert_eq!(client.timeout().as_millis(), 2_500);
    }

    /// 超时时限为 0 时按 1 毫秒处理，避免 `recv_timeout(0)` 立即误判为超时。
    #[test]
    fn zero_timeout_is_clamped_to_one_millisecond() {
        let client = JsonRpcClient::new(ExternalConfig {
            command: "whatever".to_string(),
            args: vec![],
            timeout_ms: 0,
        });
        assert_eq!(client.timeout().as_millis(), 1);
    }

    // 说明：端到端用例（写入 → 读回 → 解析 → 错误透传 → 进程退出 → 超时 → 自愈 →
    // open/view/apply/serialize 完整链路）已迁到 `tests/rpc_e2e.rs`。
    // 迁移原因：那批用例原先全部带 `#[cfg(unix)]`，**Windows 上零覆盖**，
    // 而本项目的主力开发环境正是 Windows。现由「启动自身」的迷你上游实现，
    // Windows / Linux / macOS 跑的是**同一份**代码。
}
