//! 参考 MCP 适配器：进程外后端的**可运行样板**。
//!
//! # 它解决什么问题
//!
//! 进程外后端（JSON-RPC 2.0 over stdio）要真正闭环，光有协议定义不够——
//! 需要一个能跑的对话端，才能验证「宿主 ↔ 上游进程」这条链路真的通。
//!
//! 真实的 `gridcraft-mcp` / `deckcraft-mcp` / `wordcraft-mcp` 需要另外安装；
//! 本模块实现的是**同一套协议**的最小参考实现：
//! 把文件当纯文本打开、按行切段、支持段落级增删改、序列化回文本。
//!
//! 它不追求还原上游的 OOXML 保真度（那正是上游自己的职责），
//! 只保证**协议语义正确**，从而可以：
//!
//! ```text
//! mkv editors backend --set gridcraft --kind external --command mkv --arg editors --arg stub
//! mkv editors backend <root>            # 真实拉起进程做健康检查
//! ```
//!
//! # 协议
//!
//! 每行一条 JSON（**换行分隔**，不是 LSP 的 `Content-Length` 分帧）：
//!
//! ```text
//! → {"jsonrpc":"2.0","id":1,"method":"mkv/editor/open","params":{"bytes_b64":"..."}}
//! ← {"jsonrpc":"2.0","id":1,"result":{"state_b64":"..."}}
//! ```
//!
//! 状态用 JSON 编码后 base64 传输：`{"paragraphs":[{"text":"...","style":"Normal"}]}`。

use std::io::{BufRead, Write};

use serde_json::{json, Value};

use mkv_core::backend::{decode_base64, encode_base64, methods};

/// 运行参考适配器（读 stdin，写 stdout），直到 stdin 关闭。
pub fn run() -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                // 解析失败也要回一条错误，否则宿主会一直等这个 id。
                let err = json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": {"code": -32700, "message": format!("JSON 解析失败: {e}")}
                });
                writeln!(stdout, "{err}")?;
                stdout.flush()?;
                continue;
            }
        };
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or_default()
            .to_string();
        let params = request.get("params").cloned().unwrap_or(json!({}));

        let response = match dispatch(&method, &params) {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(message) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32000, "message": message}
            }),
        };
        writeln!(stdout, "{response}")?;
        stdout.flush()?;
    }
    Ok(())
}

/// 分发一个方法调用。
fn dispatch(method: &str, params: &Value) -> Result<Value, String> {
    match method {
        methods::VERSION => Ok(json!({"version": env!("CARGO_PKG_VERSION")})),
        methods::OPEN => {
            let bytes_b64 = params
                .get("bytes_b64")
                .and_then(|v| v.as_str())
                .ok_or("open 缺少 bytes_b64")?;
            let bytes = decode_base64(bytes_b64).map_err(|e| e.to_string())?;
            let text = String::from_utf8_lossy(&bytes).to_string();
            let paragraphs: Vec<Value> = text
                .lines()
                .map(|l| json!({"text": l, "style": "Normal"}))
                .collect();
            let state = json!({"paragraphs": paragraphs});
            Ok(json!({"state_b64": encode_base64(state.to_string().as_bytes())}))
        }
        methods::VIEW => {
            let state = decode_state(params)?;
            let paragraphs = state["paragraphs"].as_array().cloned().unwrap_or_default();
            let views: Vec<Value> = paragraphs
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    json!({
                        "index": i,
                        "text": p.get("text").and_then(|t| t.as_str()).unwrap_or_default(),
                        "style": p.get("style").and_then(|t| t.as_str()).unwrap_or("Normal"),
                    })
                })
                .collect();
            Ok(json!({
                "Word": {
                    "paragraphs": views,
                    "style_options": ["Normal", "Heading1", "Heading2"]
                }
            }))
        }
        methods::APPLY => {
            let mut state = decode_state(params)?;
            let action = params
                .get("action")
                .ok_or("apply 缺少 action")?
                .clone();
            apply_action(&mut state, &action)?;
            Ok(json!({"state_b64": encode_base64(state.to_string().as_bytes())}))
        }
        methods::SERIALIZE => {
            let state = decode_state(params)?;
            let text = state["paragraphs"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            let mut bytes = text.into_bytes();
            bytes.push(b'\n');
            Ok(json!({"bytes_b64": encode_base64(&bytes)}))
        }
        other => Err(format!("不支持的方法: {other}")),
    }
}

/// 从 params 里取出并解码状态。
fn decode_state(params: &Value) -> Result<Value, String> {
    let state_b64 = params
        .get("state_b64")
        .and_then(|v| v.as_str())
        .ok_or("缺少 state_b64")?;
    let raw = decode_base64(state_b64).map_err(|e| e.to_string())?;
    serde_json::from_slice(&raw).map_err(|e| format!("状态解析失败: {e}"))
}

/// 应用一个动作到状态上。
fn apply_action(state: &mut Value, action: &Value) -> Result<(), String> {
    let mut paragraphs = state["paragraphs"].as_array().cloned().unwrap_or_default();
    match action.get("SetParagraph").or_else(|| action.get("SetParagraphStyle")) {
        Some(_) => {
            let index = action
                .get("SetParagraph")
                .and_then(|a| a.get("index"))
                .or_else(|| action.get("SetParagraphStyle").and_then(|a| a.get("index")))
                .and_then(|v| v.as_u64())
                .ok_or("动作缺少 index")? as usize;
            if index >= paragraphs.len() {
                return Err(format!("段落下标越界: {index}"));
            }
            if let Some(a) = action.get("SetParagraph") {
                let text = a.get("text").and_then(|v| v.as_str()).unwrap_or_default();
                paragraphs[index]["text"] = Value::String(text.to_string());
            }
            if let Some(a) = action.get("SetParagraphStyle") {
                let style = a.get("style").and_then(|v| v.as_str()).unwrap_or("Normal");
                paragraphs[index]["style"] = Value::String(style.to_string());
            }
        }
        None => match action.get("InsertParagraph") {
            Some(a) => {
                let at = a.get("at").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let style = a
                    .get("style")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Normal")
                    .to_string();
                let at = at.min(paragraphs.len());
                paragraphs.insert(at, json!({"text": "", "style": style}));
            }
            None => match action.get("RemoveParagraph") {
                Some(a) => {
                    let index = a.get("index").and_then(|v| v.as_u64()).ok_or("动作缺少 index")?
                        as usize;
                    if index >= paragraphs.len() {
                        return Err(format!("段落下标越界: {index}"));
                    }
                    paragraphs.remove(index);
                }
                None => {
                    return Err(format!(
                        "参考适配器只支持段落级动作，收到: {}",
                        action
                            .as_object()
                            .and_then(|o| o.keys().next().cloned())
                            .unwrap_or_else(|| "?".into())
                    ));
                }
            },
        },
    }
    state["paragraphs"] = Value::Array(paragraphs);
    Ok(())
}
