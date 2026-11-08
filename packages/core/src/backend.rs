//! 编辑器后端抽象与进程外后端（架构闭环）。
//!
//! # 为什么是「进程外」而不是「链接上游 crate」
//!
//! 本项目最初的设想是：把 `storytold/{gridcraft,deckcraft,wordcraft}` 的
//! 格式 crate 作为依赖引入，让内置编辑器直接用上游实现。2026-11-08 的
//! 实地核对否掉了这条路径（证据见本模块末尾「为什么不能链接上游 crate」）：
//!
//! 1. `gridcraft-xlsx` 与 `wordcraft-docx` **从未发布到 crates.io**
//!    （workspace 依赖是纯 path、无 version，`cargo package` 直接失败）；
//! 2. `deckcraft-pptx` 只发布了 0.6.1，**落后 main 约两周**，API 更小；
//! 3. 三者**全部是全量重写**（`write_xlsx` / `export` / `write` 从模型重建整个包），
//!    没有「打开文件 → 改一处 → 原样存回」的接口。而本项目的核心价值主张
//!    恰恰是**包级外科手术**（README §9.2：未编辑部件字节级原样保留）。
//!    链接上游 = 用「整包重写」替换「外科改写」，是产品价值的倒退而非升级。
//!
//! # 闭环的正确形态：进程外后端
//!
//! 三个上游项目**都已经实现了 MCP server**（JSON-RPC 2.0 over stdio），
//! 且 README 明确承诺「Every action is a command with a stable id」——
//! **稳定的是命令 id，不是 crate API**。这正好是契约层需要的边界：
//!
//! ```text
//!   MKV 宿主（契约 v1 稳定）
//!        │  JSON-RPC 2.0 over stdio
//!        ▼
//!   上游进程（gridcraft-mcp / deckcraft-mcp / wordcraft-mcp）
//! ```
//!
//! 于是「上游更新 → 内置编辑器随之更新」变成：
//! **换上游进程版本，不动宿主代码**。这才是真正的架构闭环：
//!
//! - 上游 1–3 天一版的迭代节奏不会污染我们的依赖树；
//! - 上游 alpha 崩溃 / 内存爆炸只影响该进程，不拖垮宿主；
//! - 上游缺失或启动失败时，`builtin` 后端仍然可用（降级路径）。
//!
//! # 本模块提供什么
//!
//! - [`PluginBackend`]：后端 trait（与 [`crate::plugin::EditorPlugin`] 同形，
//!   但状态留在进程内、跨调用由宿主持有）。
//! - [`JsonRpcClient`]：JSON-RPC 2.0 over stdio 客户端（**仅 native**，
//!   因为它要起子进程）。
//! - [`ExternalBackend`]：把上游 MCP server 适配成 [`PluginBackend`]。
//!
//! Web 端不链接本模块的任何进程相关部分，wasm 体积门禁不受影响。

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::format::Format;
use crate::plugin::{PluginAction, PluginDocument, PluginView};

/// 后端标识。
///
/// `builtin` = 本项目自研的外科手术实现（始终可用）；
/// `external` = 进程外上游实现（MCP over stdio，可选）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    /// 本项目内置实现。
    Builtin,
    /// 进程外上游实现。
    External,
}

impl BackendKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            BackendKind::Builtin => "builtin",
            BackendKind::External => "external",
        }
    }
}

/// 编辑器后端：与插件同形，但**状态由宿主持有**。
///
/// 与 [`crate::plugin::EditorPlugin`] 的区别：
/// - `EditorPlugin` 是「进程内插件」，自带状态序列化；
/// - `PluginBackend` 是「后端」，只管纯粹的格式读写，
///   状态（`PluginDocument`）由调用方持有并在调用之间传递。
///
/// 这样进程外后端无需实现状态序列化——状态留在宿主内存里，
/// 只有「打开 / 取视图 / 应用动作 / 写回」四类操作需要跨进程。
pub trait PluginBackend: Send + Sync {
    /// 后端类型。
    fn kind(&self) -> BackendKind;

    /// 是否支持该格式。
    fn supports(&self, format: Format) -> bool;

    /// 从原生文件字节打开。
    fn open(&self, bytes: &[u8]) -> Result<PluginDocument>;

    /// 生成 UI 视图。
    fn view(&self, doc: &PluginDocument) -> Result<PluginView>;

    /// 应用一次用户操作。
    fn apply(&self, doc: &mut PluginDocument, action: &PluginAction) -> Result<()>;

    /// 序列化回原生文件字节。
    fn serialize(&self, doc: &PluginDocument, original: &[u8]) -> Result<Vec<u8>>;
}

/// 后端健康状态（供 CLI / UI 展示）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendHealth {
    /// 后端 ID（`gridcraft` 等）。
    pub id: String,
    /// 后端类型。
    pub kind: BackendKind,
    /// 是否可用。
    pub available: bool,
    /// 不可用时的原因（如「进程未启动」「命令不存在」）。
    pub reason: String,
    /// 上游版本（若能探测到）。
    pub version: String,
}

/// 进程外后端的连接配置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalConfig {
    /// 可执行程序名或路径（如 `deckcraft-cli`）。
    pub command: String,
    /// 启动参数。
    #[serde(default)]
    pub args: Vec<String>,
    /// 单次调用的超时（毫秒）。
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_timeout_ms() -> u64 {
    10_000
}

/// JSON-RPC 2.0 请求。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 响应。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    #[serde(default)]
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 错误。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// 上游 MCP server 暴露的方法名。
///
/// 上游承诺「命令 id 稳定」，因此这里约定一组**方法名**，
/// 由适配层（我们写的薄壳）翻译成上游的实际命令。
/// 这样即使上游改了内部命令名，只需改适配层，宿主不变。
pub mod methods {
    /// 打开文档：`{"bytes_b64": ...}` → `{"state_b64": ...}`
    ///
    /// # 为什么 params 里没有 `path`
    ///
    /// 早期文档写的是 `{"path": ..., "bytes_b64": ...}`，但实现只传 `bytes_b64`——
    /// 文档与实现不一致，属于**会误导适配器作者**的错误，这里明确纠正为只传内容。
    ///
    /// 理由有三：
    /// 1. **契约层拿不到路径**。[`PluginBackend::open`] 的签名只接受字节
    ///    （`fn open(&self, bytes: &[u8]) -> Result<PluginDocument>`），
    ///    宿主把文件读成字节后交给后端，**路径不在这一层的语境里**。
    /// 2. **内容优先于路径**。宿主读出的字节可能已经包含宿主侧的解析结果
    ///    （如已展开的加密、已解码的流），让上游按路径**重新读盘**会得到
    ///    与宿主手里那份**不一致**的内容——两份状态并存是更糟的错。
    /// 3. **写回路径由宿主掌握**。保存时宿主把字节写回它自己知道的原路径，
    ///    上游完全不需要知道文件在哪儿。
    ///
    /// 因此：**路径归宿主，内容归上游**。
    pub const OPEN: &str = "mkv/editor/open";
    /// 取视图：`{"state_b64": ...}` → `PluginView`
    pub const VIEW: &str = "mkv/editor/view";
    /// 应用动作：`{"state_b64": ..., "action": ...}` → `{"state_b64": ...}`
    pub const APPLY: &str = "mkv/editor/apply";
    /// 序列化：`{"state_b64": ..., "original_b64": ...}` → `{"bytes_b64": ...}`
    pub const SERIALIZE: &str = "mkv/editor/serialize";
    /// 探测版本：→ `{"version": "..."}`
    pub const VERSION: &str = "mkv/editor/version";
}

// ---------------------------------------------------------------- base64

/// 最小 base64 编解码（纯 Rust，无依赖）。
///
/// 之所以自己写：跨进程传文件字节必须 base64，
/// 而引入 `base64` crate 会给 wasm 产物增加体积（虽然本模块仅 native 编译，
/// 但依赖树是共享的）。实现只有几十行，且能顺带控制错误信息。
mod base64 {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    /// 编码为标准 base64（含 `=` 补位）。
    pub fn encode(data: &[u8]) -> String {
        let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
        for chunk in data.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = *chunk.get(1).unwrap_or(&0) as u32;
            let b2 = *chunk.get(2).unwrap_or(&0) as u32;
            let n = (b0 << 16) | (b1 << 8) | b2;
            out.push(ALPHABET[(n >> 18) as usize & 63] as char);
            out.push(ALPHABET[(n >> 12) as usize & 63] as char);
            if chunk.len() > 1 {
                out.push(ALPHABET[(n >> 6) as usize & 63] as char);
            } else {
                out.push('=');
            }
            if chunk.len() > 2 {
                out.push(ALPHABET[n as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
        out
    }

    /// 解码标准 base64。非法输入返回 `Err`（中文信息）。
    pub fn decode(text: &str) -> Result<Vec<u8>, String> {
        let mut rev = [255u8; 256];
        for (i, &c) in ALPHABET.iter().enumerate() {
            rev[c as usize] = i as u8;
        }
        // 严格校验补位：总长必须是 4 的倍数，`=` 只能出现在末尾且至多两个。
        // 早期实现直接过滤掉 `=`，于是 "Zg===" 这种非法输入会被当成 "Zg" 接受——
        // 跨进程传字节时，一个被静默截断的载荷比一次明确的报错危险得多。
        let raw = text.as_bytes();
        if raw.len() % 4 != 0 {
            return Err("base64 长度非法（应为 4 的倍数）".to_string());
        }
        let pad = raw.iter().rev().take_while(|&&b| b == b'=').count();
        if pad > 2 {
            return Err("base64 补位过多（至多两个 `=`）".to_string());
        }
        if pad > 0 {
            // 补位之后不能再出现非补位字符
            let body = &raw[..raw.len() - pad];
            if body.iter().any(|&b| b == b'=') {
                return Err("base64 的 `=` 只能出现在末尾".to_string());
            }
        }
        let bytes: Vec<u8> = raw.iter().copied().filter(|b| *b != b'=').collect();
        if bytes.len() % 4 == 1 {
            return Err("base64 长度非法".to_string());
        }
        let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
        for chunk in bytes.chunks(4) {
            let mut n: u32 = 0;
            for (i, &b) in chunk.iter().enumerate() {
                let v = rev[b as usize];
                if v == 255 {
                    return Err(format!("base64 含非法字符: {}", b as char));
                }
                n |= (v as u32) << (18 - 6 * i);
            }
            out.push((n >> 16) as u8);
            if chunk.len() > 2 {
                out.push((n >> 8) as u8);
            }
            if chunk.len() > 3 {
                out.push(n as u8);
            }
        }
        Ok(out)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn round_trips_arbitrary_bytes() {
            for len in 0..40usize {
                let data: Vec<u8> = (0..len).map(|i| (i * 37 % 251) as u8).collect();
                let enc = encode(&data);
                let dec = decode(&enc).expect("应能解码");
                assert_eq!(dec, data, "len={len}");
            }
        }

        #[test]
        fn matches_known_vectors() {
            assert_eq!(encode(b""), "");
            assert_eq!(encode(b"f"), "Zg==");
            assert_eq!(encode(b"fo"), "Zm8=");
            assert_eq!(encode(b"foo"), "Zm9v");
            assert_eq!(encode(b"foobar"), "Zm9vYmFy");
            assert_eq!(decode("Zm9vYmFy").unwrap(), b"foobar");
        }

        #[test]
        fn rejects_invalid_input() {
            assert!(decode("Zm9vYmFy!").is_err());
            assert!(decode("Zg===").is_err());
        }
    }
}

/// 供外部 crate 复用的 base64 编解码。
pub fn encode_base64(data: &[u8]) -> String {
    base64::encode(data)
}

/// 供外部 crate 复用的 base64 解码。
pub fn decode_base64(text: &str) -> Result<Vec<u8>> {
    base64::decode(text).map_err(Error::Config)
}

/// 为什么不能链接上游 crate —— 保留证据，避免后人重复调研。
///
/// 2026-11-08 对三个上游仓库的实地核对结论：
///
/// | crate | crates.io | 依赖闭包 | 局部修改 |
/// |---|---|---|---|
/// | `gridcraft-xlsx` | **不存在** | 112 crates | ❌ 全量重写 |
/// | `deckcraft-pptx` | 0.6.1（落后两周） | 37 crates | ❌ 全量重写 |
/// | `wordcraft-docx` | **不存在** | 105 crates | ❌ 全量重写 |
///
/// 三者均无 async runtime、无 git 源依赖、无真正 C 依赖，wasm 可编译——
/// 但这些优点都**不足以抵消**「全量重写」与「无法通过 crates.io 获取」两条。
/// 而它们**已经有 MCP server**，且承诺命令 id 稳定：
/// 这正是进程外后端能成立的前提。
pub const UPSTREAM_LINKING_FINDINGS: &str = "\
gridcraft-xlsx / wordcraft-docx 未发布到 crates.io（path 依赖缺 version，cargo package 失败）；\
deckcraft-pptx 仅发布 0.6.1 且落后 main 两周；\
三者写回均为全量重写，与本项目『包级外科手术』写回策略冲突。\
改用进程外 MCP 后端：上游已有 MCP server 且命令 id 稳定，进程隔离避免上游 alpha 崩溃影响宿主。";

/// 进程外后端：把上游 MCP server 适配成 [`PluginBackend`]。
///
/// 状态（`PluginDocument.state`）留在**宿主内存**里，
/// 只有四类操作跨进程：打开 / 取视图 / 应用动作 / 序列化。
#[cfg(feature = "native")]
pub struct ExternalBackend {
    id: String,
    format: Format,
    client: std::sync::Mutex<crate::rpc::JsonRpcClient>,
}

#[cfg(feature = "native")]
impl ExternalBackend {
    /// 创建进程外后端。
    pub fn new(id: &str, format: Format, config: ExternalConfig) -> Self {
        Self {
            id: id.to_string(),
            format,
            client: std::sync::Mutex::new(crate::rpc::JsonRpcClient::new(config)),
        }
    }

    /// 健康检查。
    pub fn health(&self) -> BackendHealth {
        let guard = self
            .client
            .lock()
            .map_err(|_| Error::Config("上游进程客户端锁已损坏".into()));
        match guard {
            Ok(mut c) => c.health(&self.id),
            Err(e) => BackendHealth {
                id: self.id.clone(),
                kind: BackendKind::External,
                available: false,
                reason: e.to_string(),
                version: String::new(),
            },
        }
    }

    fn call(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let mut guard = self
            .client
            .lock()
            .map_err(|_| Error::Config("上游进程客户端锁已损坏".into()))?;
        guard.call(method, params)
    }
}

#[cfg(feature = "native")]
impl PluginBackend for ExternalBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::External
    }

    fn supports(&self, format: Format) -> bool {
        format == self.format
    }

    fn open(&self, bytes: &[u8]) -> Result<PluginDocument> {
        // 只传内容、不传路径：理由见 `methods::OPEN` 的文档（路径归宿主、内容归上游）。
        let params = serde_json::json!({
            "bytes_b64": encode_base64(bytes),
        });
        let result = self.call(methods::OPEN, params)?;
        let state = result
            .get("state_b64")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("上游进程响应缺少 state_b64".into()))?;
        Ok(PluginDocument {
            plugin_id: self.id.clone(),
            state: decode_base64(state)?,
        })
    }

    fn view(&self, doc: &PluginDocument) -> Result<PluginView> {
        let params = serde_json::json!({
            "state_b64": encode_base64(&doc.state),
        });
        let result = self.call(methods::VIEW, params)?;
        serde_json::from_value(result)
            .map_err(|e| Error::Config(format!("上游进程视图解析失败: {e}")))
    }

    fn apply(&self, doc: &mut PluginDocument, action: &PluginAction) -> Result<()> {
        let params = serde_json::json!({
            "state_b64": encode_base64(&doc.state),
            "action": action,
        });
        let result = self.call(methods::APPLY, params)?;
        let state = result
            .get("state_b64")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("上游进程响应缺少 state_b64".into()))?;
        doc.state = decode_base64(state)?;
        Ok(())
    }

    fn serialize(&self, doc: &PluginDocument, original: &[u8]) -> Result<Vec<u8>> {
        let params = serde_json::json!({
            "state_b64": encode_base64(&doc.state),
            "original_b64": encode_base64(original),
        });
        let result = self.call(methods::SERIALIZE, params)?;
        let bytes = result
            .get("bytes_b64")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("上游进程响应缺少 bytes_b64".into()))?;
        decode_base64(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_kind_round_trips_as_json() {
        let json = serde_json::to_string(&BackendKind::External).unwrap();
        assert_eq!(json, "\"external\"");
        let back: BackendKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, BackendKind::External);
    }

    #[test]
    fn json_rpc_error_is_optional_in_response() {
        let ok: JsonRpcResponse =
            serde_json::from_str(r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#).unwrap();
        assert!(ok.error.is_none());
        assert_eq!(ok.result.unwrap()["ok"], serde_json::json!(true));

        let bad: JsonRpcResponse =
            serde_json::from_str(r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"boom"}}"#)
                .unwrap();
        let err = bad.error.expect("应解析出错误");
        assert_eq!(err.code, -32000);
        assert_eq!(err.message, "boom");
    }

    #[test]
    fn external_config_defaults_timeout() {
        let cfg: ExternalConfig =
            serde_json::from_str(r#"{"command":"deckcraft-cli"}"#).unwrap();
        assert_eq!(cfg.command, "deckcraft-cli");
        assert!(cfg.args.is_empty());
        assert_eq!(cfg.timeout_ms, 10_000);
    }
}
