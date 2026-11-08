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
//! # 为什么不用 LSP 的 Content-Length 分帧
//!
//! MCP 的 stdio 传输用换行分隔 JSON。LSP 的 `Content-Length` 头是另一套协议，
//! 用错会导致对端完全无法解析。跟随上游的既有实现是最省事也最不容易错的选择。

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value};

use crate::backend::{BackendHealth, BackendKind, ExternalConfig, JsonRpcRequest, JsonRpcResponse};
use crate::error::{Error, Result};

/// JSON-RPC over stdio 客户端。
pub struct JsonRpcClient {
    config: ExternalConfig,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
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

    /// 启动（或复用）子进程。
    fn ensure_started(&mut self) -> Result<()> {
        if self.stdin.is_some() && self.stdout.is_some() {
            return Ok(());
        }
        // 上一次进程可能已退出：先清理再重启
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

        let stdout = self
            .stdout
            .as_mut()
            .ok_or_else(|| Error::Config("上游进程 stdout 已关闭".into()))?;
        let mut response_line = String::new();
        let n = stdout.read_line(&mut response_line).map_err(|e| {
            Error::Config(format!("读取上游进程响应失败（进程可能已退出）: {e}"))
        })?;
        if n == 0 {
            // 进程已退出：清空状态，下次调用会重新启动
            self.shutdown();
            return Err(Error::Config("上游进程已退出（无响应）".into()));
        }

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

    /// 终止子进程（幂等）。
    pub fn shutdown(&mut self) {
        self.stdin.take();
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

    /// 起一个「回声服务器」：每读一行 JSON，返回一个固定结果。
    /// 用系统自带的 shell 拼一个最小 JSON-RPC 服务端，
    /// 验证客户端的写入 / 读回 / 解析 / 错误处理链路。
    #[cfg(unix)]
    fn spawn_echo_server(script: &str) -> JsonRpcClient {
        JsonRpcClient::new(ExternalConfig {
            command: "sh".to_string(),
            args: vec!["-c".to_string(), script.to_string()],
            timeout_ms: 5_000,
        })
    }

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

    #[cfg(unix)]
    #[test]
    fn calls_method_and_returns_result() {
        // 服务端：读一行，解析出 method，原样回一个 result
        let server = r#"while read -r line; do
  printf '{"jsonrpc":"2.0","id":1,"result":{"echo":%s}}\n' "$(echo "$line" | sed 's/.*"method":"\([^"]*\)".*/"\1"/')"
done"#;
        let mut client = spawn_echo_server(server);
        let out = client.call("mkv/editor/open", json!({"x": 1})).unwrap();
        assert_eq!(out["echo"], json!("mkv/editor/open"));
    }

    #[cfg(unix)]
    #[test]
    fn surfaces_remote_error() {
        let server = r#"while read -r line; do
  printf '{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"boom"}}\n'
done"#;
        let mut client = spawn_echo_server(server);
        let err = client.call("mkv/editor/view", json!({})).unwrap_err();
        assert!(err.to_string().contains("boom"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn detects_process_exit() {
        let server = "exit 0";
        let mut client = spawn_echo_server(server);
        let err = client.call("mkv/editor/open", json!({})).unwrap_err();
        assert!(
            err.to_string().contains("退出") || err.to_string().contains("失败"),
            "{err}"
        );
    }
}
