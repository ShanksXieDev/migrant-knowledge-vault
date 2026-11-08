# 上游 MCP 适配器：进程外后端的协议与接入指南

> 本文说明 Migrant Knowledge Vault 如何与 `storytold/{gridcraft,deckcraft,wordcraft}`
> 的进程外后端通信，以及第三方如何为自己的编辑器实现这个适配层。

## 1. 为什么是「进程外」而不是「链接上游 crate」

2026-11-08 对三个上游仓库的实地核对结论如下（证据固化在
`packages/core/src/backend.rs` 的 `UPSTREAM_LINKING_FINDINGS` 常量里）：

| crate | crates.io | 依赖闭包 | 写回方式 |
|---|---|---|---|
| `gridcraft-xlsx` | **不存在** | 112 crates | 全量重写 |
| `deckcraft-pptx` | 0.6.1（落后 main 两周） | 37 crates | 全量重写 |
| `wordcraft-docx` | **不存在** | 105 crates | 全量重写 |

三条各自独立、任意一条都足以否决「直接链接」：

1. **拿不到**：`gridcraft-xlsx` 与 `wordcraft-docx` 的 workspace 依赖是纯 path、无 version，
   `cargo package` 直接报 `all dependencies must have a version requirement specified`。
   `deckcraft-pptx` 虽已发布，但落后 main 约两周（quick-xml 0.38 vs 0.41、zip 8.6 vs 3.0），
   API 覆盖面也更小。
2. **保真度倒退**：三者的写回接口（`write_xlsx` / `export` / `write`）都是
   「解析 → 内存模型 → 从模型重建整个包」。而本项目的核心价值主张是
   **包级外科手术**（README §9.2：未编辑部件字节级原样保留）。
   链接上游等于用「整包重写」替换「外科改写」，这是产品价值的倒退而非升级。
3. **节奏不匹配**：上游是 1–3 天一版的 alpha，README 明示没有 semver 承诺。
   引入它们等于把本项目的依赖树绑在别人的发布节奏上。

而三个上游**都已经实现了 MCP server**（JSON-RPC 2.0 over stdio），
且 README 明确承诺 **“Every action is a command with a stable id”**——
**稳定的是命令 id，不是 crate API**。这正好是契约层需要的边界。

## 2. 架构

```text
┌───────────────────────────────────────────────────────┐
│ MKV 宿主                                              │
│  mkv-core::plugin（契约 v1，稳定）                     │
│    EditorPlugin ── BackendPlugin ── PluginBackend      │
│                                     └─ ExternalBackend │
│  mkv-core::rpc::JsonRpcClient（JSON-RPC 2.0 over stdio）│
└──────────────────────────┬────────────────────────────┘
                           │ 换行分隔的 JSON 报文
                           ▼
┌───────────────────────────────────────────────────────┐
│ 上游进程                                               │
│  gridcraft-mcp / deckcraft-mcp / wordcraft-mcp         │
│  （或任何实现了下述 5 个方法的适配器）                   │
└───────────────────────────────────────────────────────┘
```

关键性质：

- **状态留在宿主**。`PluginDocument.state` 是宿主内存里的不透明字节，
  只有「打开 / 取视图 / 应用动作 / 序列化」四类操作跨进程。
  因此上游**不需要**实现状态序列化。
- **进程隔离**。上游 alpha 崩溃 / 内存爆炸只影响该子进程，不拖垮宿主；
  进程异常退出时客户端下次调用会重新 spawn（自愈）。
- **可降级**。`builtin` 后端始终可用，上游缺失或启动失败不会让编辑器不可用。

## 3. 传输协议

**换行分隔的 JSON**，每行一条报文。不是 LSP 的 `Content-Length` 分帧——
用错会导致对端完全无法解析。

请求：

```json
{"jsonrpc":"2.0","id":1,"method":"mkv/editor/open","params":{"bytes_b64":"..."}}
```

成功响应：

```json
{"jsonrpc":"2.0","id":1,"result":{"state_b64":"..."}}
```

错误响应：

```json
{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"..."}}
```

`id` 必须原样回传，宿主据此匹配响应。`error` 与 `result` 互斥。

### 3.1 超时

`ExternalConfig.timeout_ms` 是**单次调用的时限**，默认 10,000 ms。
超过时限即返回错误，**绝不无限期挂起**——客户端被 `Mutex` 保护，
一次挂起会锁死整个宿主 UI。

超时后宿主会**终止该上游进程并丢弃读缓冲**，下次调用重新拉起一个干净的进程。
这一步是必需的：若只丢弃「本次响应」而留着缓冲，
下一次读到的会是**上一条请求的半截响应**，请求与响应从此错位。

超时实现是**跨平台**的（把读一行交给独立线程，主线程用 `recv_timeout` 等待）。
之所以不用 `set_nonblocking` + 轮询：那是 Unix 独有的 API，
Windows 上没有——写成 `#[cfg(unix)]` 就等于让 Windows 继续裸奔。

### 3.2 stdout 的洁净性

协议约定 stdout 只走 JSON-RPC 报文。上游若往 stdout 打日志，
会污染响应流。宿主当前**不做**跳过非 JSON 行的处理——
遇到无法解析的行会直接报「上游进程响应不是合法 JSON-RPC」，
宁可显式失败也不静默错位。

## 4. 方法映射

宿主约定 5 个方法（定义在 `packages/core/src/backend.rs` 的 `methods` 模块）。
**这 5 个方法是 MKV 自己的契约**，由适配层翻译成上游的实际命令 id。

| 方法 | params | result | 语义 |
|---|---|---|---|
| `mkv/editor/open` | `{"bytes_b64": "<原文件字节>"}` | `{"state_b64": "<状态>"}` | 打开文件，返回可编辑状态 |
| `mkv/editor/view` | `{"state_b64": "<状态>"}` | `PluginView` 的 JSON | 生成 UI 视图 |
| `mkv/editor/apply` | `{"state_b64": "<状态>", "action": <PluginAction>}` | `{"state_b64": "<新状态>"}` | 应用一次用户操作 |
| `mkv/editor/serialize` | `{"state_b64": "<状态>", "original_b64": "<原文件字节>"}` | `{"bytes_b64": "<新文件字节>"}` | 序列化回原生文件 |
| `mkv/editor/version` | `{}` | `{"version": "..."}` | 探测上游版本（健康检查用） |

> **`open` 的 params 里为什么没有 `path`**：
> **路径归宿主，内容归上游。**
> 1. 契约层的 `PluginBackend::open(&self, bytes: &[u8])` 签名里**没有路径**——
>    宿主把文件读成字节交给后端，路径不在这一层的语境里；
> 2. 宿主读出的字节可能已包含宿主侧的处理结果，让上游按路径**重新读盘**
>    会得到与宿主手里那份**不一致**的内容；
> 3. 写回时宿主把字节写回它自己知道的原路径，上游不需要知道文件在哪儿。
>
> 早期文档写的是 `{"path": ..., "bytes_b64": ...}`，与实现不符，已纠正。

### 4.1 `PluginView` 的 JSON 形态

`PluginView` 是带标签的枚举，直接 `serde_json::to_value` 即可：

```json
{ "Word": { "paragraphs": [ {"index":0,"text":"...","style":"Normal"} ],
            "style_options": ["Normal","Heading1"] } }
{ "Grid": { "sheets": [ {"name":"Sheet1","rows":2,"cols":3,
                         "cells":[{"row":0,"col":0,"text":"..."}]} ] } }
{ "Deck": { "slides": [ {"index":0,"title":"...","shapes":[{"index":0,"text":"..."}]} ] } }
```

### 4.2 `PluginAction` 的 JSON 形态

```json
{"SetCell":{"sheet":0,"row":0,"col":0,"text":"..."}}
{"SetParagraph":{"index":0,"text":"..."}}
{"InsertParagraph":{"at":0,"style":"Normal"}}
```

其余动作见 `packages/core/src/plugin.rs` 的 `PluginAction` 枚举。

### 4.3 状态编码

`state_b64` 是 **base64 编码的 JSON 字节**。宿主不解释其内容——
它只在 `open` 之后原样持有，在 `view` / `apply` / `serialize` 之间传递。
适配层可以自由选择内部表示（JSON 模型、protobuf、序列化后的内存镜像……）。

### 4.4 base64

标准 base64（含 `=` 补位），实现在 `packages/core/src/backend.rs` 的 `base64` 模块。
之所以自己写而不用 `base64` crate：依赖树在 wasm 目标共享，
多一个依赖就多一份体积预算，而这个实现只有几十行。

## 5. 如何把某个插件切到进程外后端

```bash
# 查看当前后端与健康状态（会真实拉起上游进程做版本探测）
mkv editors backend <root>

# 切换（逐插件生效，不影响其它两个）
mkv editors backend <root> --set gridcraft --kind external --command gridcraft-mcp
mkv editors backend <root> --set gridcraft --kind external --command gridcraft-mcp --arg --serve-mcp

# 退回内置实现
mkv editors backend <root> --set gridcraft --kind builtin
```

配置写入知识库的 `.kbindex/editors.json`（结构与仓库根的 `editors.lock` 一致，
`backends` 段描述每个插件的后端选择）。**重启应用后生效**——
插件在进程启动时按后端配置注入注册表。

## 6. 参考适配器

`mkv editors stub` 是**同一套协议**的最小可运行实现：

- 把文件当 UTF-8 文本打开，按行切段；
- 支持段落级动作（`SetParagraph` / `SetParagraphStyle` / `InsertParagraph` / `RemoveParagraph`）；
- 序列化回以换行连接的文本。

它不还原上游的 OOXML 保真度（那是上游自己的职责），只保证**协议语义正确**，
因此可以用来端到端验证「宿主 ↔ 上游进程」这条链路：

```bash
# 1. 把 gridcraft 切到进程外后端，指向参考适配器
mkv editors backend <root> --set gridcraft --kind external --command mkv --arg editors --arg stub

# 2. 健康检查（会真实起进程、探测版本）
mkv editors backend <root>
```

## 7. 写一个自己的适配器

最小要求：

1. 从 stdin 逐行读 JSON-RPC 请求，逐行写 JSON-RPC 响应；
2. 实现 §4 的 5 个方法；
3. `id` 原样回传；出错时用 `error` 字段而不是静默不响应
   （宿主会一直等待该 id）。

宿主侧的失败模式都是**可降级**的：

| 情况 | 宿主行为 |
|---|---|
| 命令不存在 | 健康检查报「不可用」，其它插件不受影响 |
| 进程中途退出 | 当前调用返回「上游进程已退出（无响应）」，下次调用重新 spawn |
| 响应缺字段 | 返回明确的 `Error::Config`（如「上游进程响应缺少 state_b64」） |
| 方法不支持 | 返回 JSON-RPC `error`，宿主原样转成错误信息 |
| **上游挂起不回应** | 当前调用在 `timeout_ms` 后返回「调用上游进程超时（N 毫秒）」，**并终止该进程、丢弃读缓冲**；下次调用重新 spawn 干净的进程 |

## 8. 端到端验证

`packages/core/tests/rpc_e2e.rs` 是**跨平台**的端到端测试
（Windows / Linux / macOS 跑的是同一份代码），覆盖：

| 用例 | 验证内容 |
|---|---|
| `calls_method_and_returns_result` | 写入 → 读回 → 解析 → `result` 取值；同客户端复用同一进程 |
| `surfaces_remote_error` | 上游 `error` 字段原样透传 |
| `detects_process_exit` | 上游提前退出能被识别 |
| `times_out_when_upstream_hangs` | 上游挂起时按时返回明确错误，**不永久挂起** |
| `client_recovers_after_timeout` | 超时后客户端仍可继续使用（状态清理干净，不读到错位响应） |
| `health_reports_upstream_version` | 健康检查能取到上游版本 |
| `full_open_view_apply_serialize_chain` | **`open → view → apply → serialize` 完整四步**，全程走真实进程 |

> **为什么用 `harness = false` 的自制 runner**：
> 该测试需要**同时**扮演「测试运行器」与「被客户端 spawn 的迷你上游」，
> 默认 libtest harness 无法在两者间切换；
> 而用 `sh -c` / `cmd /C` 起服务器在两端语法不同（`cmd` 没有 `while read`），
> 历史上正是这一点让端到端测试全部退化成 `#[cfg(unix)]`——
> **Windows 上零覆盖**，而 Windows 正是本项目的主力开发环境。
> 现在的做法是「启动自身」（`std::env::current_exe()`）并带 `--server=<模式>` 参数，
> 因为 harness 是自制的，argv 由我们自己解析，不受 libtest 参数规则约束。

## 9. 相关位置

| 内容 | 位置 |
|---|---|
| 后端 trait 与适配器 | `packages/core/src/backend.rs` |
| JSON-RPC 客户端 | `packages/core/src/rpc.rs`（仅 native） |
| 契约 DTO | `packages/core/src/plugin.rs` |
| 后端选择配置 | `editors.lock` 的 `backends` 段 / `.kbindex/editors.json` |
| 聚合与切换 | `packages/editors/src/lib.rs` |
| CLI | `packages/cli/src/main.rs`（`editors backend`）、`packages/cli/src/stub.rs` |
| 调研证据 | `packages/core/src/backend.rs` 的 `UPSTREAM_LINKING_FINDINGS` |
