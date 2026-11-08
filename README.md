# Migrant Knowledge Vault

> 本地优先 · 文件优先 · Git 优先  
> 统一管理、检索、预览、编辑多格式文档的个人知识底座。

v0.1 工程脚手架：**Rust 核心库 + Dioxus 三端（Desktop / Web / Mobile）+ 命令行工具**。  
当前已跑通主链路 —— 扫描 → 解析 → 索引 → 检索 → 预览 → 双链 → Git 版本，
并已落地**内置编辑器（GridCraft / DeckCraft / WordCraft）**与
**Web 端预构建只读索引**。

---

## 1. 能力现状（v0.1）

| 能力      | 状态 | 说明                                                             |
| ------- | -- | -------------------------------------------------------------- |
| 六格式扫描   | ✅  | Markdown / Excel / PPT / Word / Visio / Drawio                 |
| 文本抽取    | ✅  | OOXML 解包抽 `w:t`/`a:t`；Visio 抽 `<Text>`；Drawio 抽 `mxCell@value` |
| 中文全文检索  | ✅  | Tantivy + jieba 自定义分词器，支持领域词典                                  |
| 双链      | ✅  | `[[双链]]` / \`[[双链|别名]]`/`@文件名.ext\` 三种写法                       |
| 反链 / 出链 | ✅  | 目标解析支持路径、文件名、主干、标题四级匹配                                         |
| Git 版本  | ✅  | 提交、历史、差异、回滚；`.kbindex/` 自动忽略                                   |
| 内置编辑器   | ✅  | 三个独立插件 crate（GridCraft / DeckCraft / WordCraft），契约 v1，始终读写原生文件 |
| Web 端索引 | ✅  | 预构建只读索引：构建期内嵌二进制，运行时零网络加载；**本地与线上各 35/35 项真实浏览器验证通过** |
| 文件监听    | ✅  | `notify` 增量索引 + 双链重算；实测索引 51ms、端到端 175ms（PRD 要求 1s 内）        |
| 静态发布 / CI | ✅  | 6 文件纯静态站点；CI 门禁 + Pages 自动发布；wasm 经 fat LTO + wasm-opt 压到约 677KB（[线上站点](https://shanksxiedev.github.io/migrant-knowledge-vault/)） |
| UI 自动刷新 | ✅  | 索引修订号 + 250ms 轮询，文档列表 / 反链 / 检索结果自动重渲染 |
| 外部唤起 F-08 | ✅  | 用系统默认应用打开原始文件（Windows `start` / macOS `open` / Linux `xdg-open`） |
| 检索精确定位 | ✅  | 结果列表位置标签 + 预览区定位条与片段高亮；跳到段落 / 页 / 单元格行 / 节点（§13.4） |
| 双链图谱 F-10 | ✅  | SVG 力导向图谱（按格式分簇）：缩放、点选节点跳转、格式图例；浏览器 B5–B8 验证（§13.5） |
| 跨格式引用 F-06 | ✅  | `@文件#片段` 内联显示源内容（行 / 页 / 节点 / 段）、刷新与跳转；编辑器内「插入引用」入口；浏览器 C1–C3 验证（§13.6） |
| 多维过滤 F-07 | ✅  | 格式 / 标签 / 修改时间窗 / 来源路径四维过滤，三端共用同一套判定；浏览器 D4–D6 验证（§13.7） |
| 流程图节点 F-11 | ✅  | Visio / Drawio 抽取稳定节点 id，支撑节点级引用（§13.8） |
| 正则检索 F-12 | ✅  | 零依赖自研回溯正则引擎（`regex` crate 闭包会拖垮 wasm 体积）；与 Python `re` 差分对拍真实差异 0；浏览器 D1–D3 验证（§13.9） |

---

## 2. 工程结构

```
Migrant-Knowledge-Vault/
├── Cargo.toml                    # workspace 根
├── scripts/msvc-env.sh           # MSVC 构建环境注入（本机 cl.exe 不在 PATH）
├── packages/
│   ├── core/                     # mkv-core：零 UI 依赖的业务核心
│   │   ├── src/types.rs          #   跨端共享 DTO
│   │   ├── src/service.rs        #   KnowledgeService trait（平台差异抽象）
│   │   ├── src/storage.rs        #   Storage trait + FsStorage（路径逃逸防护）
│   │   ├── src/scan.rs           #   文件发现
│   │   ├── src/parse.rs          #   六格式文本抽取 + 双链抽取
│   │   ├── src/tokenizer.rs      #   jieba 自定义分词器（支持领域词典）
│   │   ├── src/index.rs          #   Tantivy 全文索引（native）
│   │   ├── src/meta.rs           #   SQLite 元数据 / 双链（native）
│   │   ├── src/git.rs            #   git2 版本管理（native）
│   │   ├── src/graph.rs          #   双链图
│   │   ├── src/editor.rs         #   编辑器路由 + EditSession
│   │   ├── src/xml.rs            #   带字节偏移的 XML 节点树解析器（OOXML 外科改写底座）
│   │   ├── src/ooxml.rs          #   OOXML 包管理：部件读写 / 关系 / 内容类型
│   │   ├── src/plugin.rs         #   编辑器插件契约 v1 + 注册表
│   │   ├── src/editor_updates.rs #   上游版本检测 / 用户确认升级状态机
│   │   ├── src/editor.rs         #   插件路由 + EditSession（不透明状态）
│   │   ├── src/prebuilt.rs       #   预构建只读索引（Web 端只读检索）
│   │   ├── src/prebuilt_export.rs#   预构建索引生成端（native，需 jieba）
│   │   ├── src/demo.rs           #   六格式示例知识库生成器
│   │   └── src/vault.rs          #   Vault 门面：统一 API + 索引文件锁
│   ├── ui/                       # mkv-ui：Dioxus 共享组件（三端复用）
│   │   ├── src/components/editors.rs # 按 PluginView 通用渲染的插件面板 + 升级横幅
│   │   └── assets/main.css       #   内联样式表（include_str!）
│   ├── cli/                      # mkv-cli：命令行工具（mkv.exe）
│   ├── desktop/                  # mkv-desktop：Dioxus desktop
│   ├── web/                      # mkv-web：Dioxus web（wasm32）
│   │   ├── build.rs              #   把索引产物嵌入 wasm 二进制
│   │   ├── assets/mkv-index.json #   约定路径的预构建索引产物
│   │   └── src/loader.rs         #   索引加载器（内嵌 / 远端热替换）
│   ├── mobile/                   # mkv-mobile：Dioxus mobile
│   ├── plugins/                  # 三个独立编辑器插件 crate
│   │   ├── gridcraft/            #   mkv-editor-gridcraft（XLSX 外科读写）
│   │   ├── deckcraft/            #   mkv-editor-deckcraft（PPTX 外科读写）
│   │   └── wordcraft/            #   mkv-editor-wordcraft（DOCX 外科读写）
│   └── editors/                  # mkv-editors：启动注入插件（Web 不链接）
├── scripts/build-web.sh          # Web 端一键构建（导出产物 → 编译 wasm）
└── examples/sample-vault/        # 示例知识库（含已落盘的 .kbindex/）
```

### 特性门控（feature gating）

`mkv-core` 用特性区分目标，避免 wasm32 拉入需要 C 编译器的原生依赖：

| 特性           | 目标               | 启用内容                         |
| ------------ | ---------------- | ---------------------------- |
| `native`（默认） | Desktop / Mobile | Tantivy + SQLite + Git + fs2 |
| `web`        | Web (wasm32)     | 仅纯 Rust 能力；索引采用**预构建只读**     |

---

## 3. 环境准备

本机 Rust 位于非标准路径 `C:\Program Files\Rust`，且 MSVC 的 `cl.exe` / `link.exe`  
**不在系统 PATH** 中（`rusqlite` bundled、libgit2 等需要 C 工具链）。  
仓库自带注入脚本：

```bash
cd Migrant-Knowledge-Vault
source scripts/msvc-env.sh
```

脚本会自动定位 VS2022 Community 与 Windows SDK 10.0.26100，注入 `PATH` / `INCLUDE` / `LIB`。  
若你已从 *Visual Studio Developer Command Prompt* 打开终端，则无需 source。

> PowerShell 用户：脚本是 Bash 版本，可在 Git Bash 中执行一次后切回 PowerShell；  
> 或直接用 `VsDevCmd.bat` 初始化环境。

---

## 4. 构建与测试

```bash
source scripts/msvc-env.sh

cargo test --workspace     # 112 个测试（core 89 / ui 7 / web 16）
cargo build --workspace    # 产出 mkv / mkv-desktop / mkv-web / mkv-mobile

# Web 端（wasm32）需先安装目标
rustup target add wasm32-unknown-unknown
cargo build -p mkv-web --target wasm32-unknown-unknown
```

> Web 端依赖图已剔除全部 C 依赖（`zip` 关闭默认特性仅留 deflate；jieba 分词器  
> 收进 `native` 特性；`gloo-net` 仅 wasm32 且只开 `http` 特性），  
> wasm32 构建**无需 clang**。可用 `cargo tree -p mkv-web --target wasm32-unknown-unknown` 复核。

---

## 5. 命令行工具

```bash
# 一键生成示例知识库并建立索引（产出 .kbindex/ 落盘示例）
cargo run -p mkv-cli -- demo examples/sample-vault

# 初始化一个空知识库
cargo run -p mkv-cli -- init D:/MyVault

# 重建索引
cargo run -p mkv-cli -- index D:/MyVault

# 中文全文检索
cargo run -p mkv-cli -- search D:/MyVault "模流分析"

# 查看双链（反链 + 出链）
cargo run -p mkv-cli -- links D:/MyVault "01-笔记/模流分析-入门.md"

# 列出全部文档
cargo run -p mkv-cli -- docs D:/MyVault

# Git 提交与历史
cargo run -p mkv-cli -- commit D:/MyVault -m "补充评审流程" -a "mkv-bot"
cargo run -p mkv-cli -- log D:/MyVault

# 载入自定义领域词典并重建索引
cargo run -p mkv-cli -- dict D:/MyVault ./domain-dict.txt

# 生成 Web 端预构建只读索引产物（会自动刷新索引）
cargo run -p mkv-cli -- export-web D:/MyVault packages/web/assets/mkv-index.json examples/domain-dict.txt

# 监听文件变化，增量更新索引与双链（Ctrl+C 退出）
cargo run -p mkv-cli -- watch D:/MyVault --debounce 250 --auto-commit
```

---

## 6. `.kbindex/` 目录规范

索引是**派生产物**：不入 Git、可随时重建。库根目录即 Git 仓库。

```
<vault>/
├── .gitignore              # 自动写入，含 .kbindex/
├── .kbindex/               # 派生索引目录（不入库）
│   ├── .lock               # 独占文件锁，防多端并发写坏索引
│   ├── meta.db             # SQLite：documents / links / tags / state
│   ├── dict.txt            # 自定义领域词典（可选，随索引一同加载）
│   └── search/             # Tantivy 索引段文件
└── <用户文档>…              # 纯文件，无隐藏元数据
```

`state` 表记录 `schema_version`、`last_indexed_at` 与 `domain_dict_hash`；  
词典哈希变化时索引被判定为过期，需重建（`mkv index`）。

> **领域词典必须与索引一起加载**：索引与查询共用同一个分词器实例规则。  
> 若查询侧用默认词典、索引侧用领域词典，同一段文本会被切成不同的词，  
> tantivy 的多词短语查询将直接零命中。因此词典持久化在 `.kbindex/dict.txt`，  
> 打开索引时自动载入。

---

## 7. 领域词典

中文分词使用 jieba，默认词典对专业术语切分不佳。词典文件支持两种写法，可混用：

```
# 以 # 开头的行为注释，空行忽略
模流分析              # 纯词条：词频由 jieba 自动推算
熔接线
冷却水路 2000 n      # jieba 原生格式：词 词频 [词性]
```

```bash
cargo run -p mkv-cli -- dict D:/MyVault examples/domain-dict.txt
```

命令会载入词典并**自动重建索引**（词典变更后旧索引的分词规则已失效）。  
词典内容保存在 `.kbindex/dict.txt`，打开索引时自动载入，保证跨进程行为一致。

若手工编辑了 `dict.txt`，下次检索会提示索引已过期，执行 `mkv index <root>` 重建。

---

## 8. 检索语义

- **多词查询取交集**：查询中的每个词都必须出现（AND 语义）。  
  不用 tantivy 默认的短语查询——短语要求词序相邻，对中文检索过严。
- **标题加权**：标题字段权重 3.0。
- **分词模式**：jieba `Search` 模式，同时产出整词与子词，兼顾精确与召回。

---

## 9. 内置编辑器（GridCraft / DeckCraft / WordCraft）

### 9.1 定位

按已锁定决策 §8-③，内置编辑器**始终读写原生文件**（`.xlsx` / `.pptx` / `.docx`），  
不另存私有格式 —— 这是「文件优先」原则的硬约束：用户随时可以用 Excel / WPS /  
PowerPoint 打开同一个文件，两边看到的内容一致。

复杂版式（母版、动画、修订、宏）仍建议**外部唤起** Office 编辑，
内置编辑器负责的是「结构化、可控的字段级编辑」。

### 9.2 实现路线：OOXML 包级外科手术

不采用「解析 → 内存模型 → 全量重写」的做法（那会丢掉用户已有的排版、样式、主题）。  
而是走**包级外科改写**：

1. 解包 `.xlsx` / `.pptx` / `.docx`（本质是 ZIP），保留**每个部件的原始字节**；
2. 只重写被编辑的那一个 XML 部件（+ 必要的关系 / 清单文件）；
3. 其余部件字节级原样写回。

为此自研了带**字节偏移**的 XML 扫描器（`packages/core/src/xml.rs`）——
第三方 quick-xml 0.42 已移除 `buffer_position`、事件流不暴露字节偏移，无法做外科改写。  
属性保留**限定名**（`r:id` 与 `id` 严格区分），否则 OOXML 关系会被误伤。

### 9.3 三个编辑器的范围

| 编辑器      | 目标格式   | 支持的操作                                       |
| -------- | ------ | ------------------------------------------- |
| GridCraft | `.xlsx`  | 单元格读写、增删行列、工作表管理                        |
| DeckCraft | `.pptx`  | 页级增删 / 重排、形状级文本框编辑（**完整编辑**）         |
| WordCraft | `.docx`  | 段落级编辑（文本、样式、增删段落）                        |

Markdown / 纯文本 / Visio / Drawio **不提供**内置编辑器（按设计矩阵），
走外部唤起。

### 9.4 入口与保存

- 桌面端：预览区右上角「内置编辑」按钮 → 打开对应编辑器面板 → 保存写回原生文件；
- 保存后触发 `index.update`，增量重建该文档的索引与双链（PRD 要求 1 秒内）；
- Web 端：预构建只读索引，**不提供**编辑入口（界面显示「Web 端为只读索引」）。

### 9.5 插件化改造与上游跟随（2026-11-08）

三个名字不是本项目自研模块，而是**真实存在的上游 Rust 项目**：

| 上游仓库 | 许可 | 本次对齐的 commit |
|---|---|---|
| `storytold/gridcraft` | MIT OR Apache-2.0 | `640fbf22f64e` |
| `storytold/deckcraft` | MIT OR Apache-2.0 | `c16db2ef3e00` |
| `storytold/wordcraft` | MIT OR Apache-2.0 | `4afff61f1097` |

三者都是 edition 2024 / rust-version 1.95 的大型 workspace（egui 应用 + 格式 crate +
`craft-libs` git 依赖），且在持续推送。直接把整个 workspace 拉进本项目会破坏
`wasm < 800KB` 门禁与「包级外科手术」保真约束（上游格式 crate 是
「解析 → 内存模型 → 全量重写」，会丢排版 / 主题 / 样式），因此采用
**独立插件 crate + 稳定契约**：

| 层 | 位置 | 职责 |
|---|---|---|
| 契约 | `packages/core/src/plugin.rs` | `EditorPlugin` trait、契约版本 v1、`PluginView` / `PluginAction` DTO、`PluginRegistry` |
| 插件 | `packages/plugins/{gridcraft,deckcraft,wordcraft}` | 各自保留外科手术读写；私有模型用 JSON 字节交换 |
| 聚合 | `packages/editors` | `mkv_editors::install()`，Desktop / CLI / Mobile 启动时注入 |
| UI | `packages/ui/src/components/editors.rs` | 按 `PluginView` 通用渲染，不认识任何插件私有类型 |
| 升级 | `packages/core/src/editor_updates.rs` + `editors.lock` | 启动检测、用户确认、状态持久化 |

Web 只读端**不链接** `mkv-editors`，编辑器实现因此完全不进入 wasm 产物。
`EditSession` 改为 `{ rel_path, original, plugin_id, document: PluginDocument }`，
`PluginDocument.state` 对宿主不透明；换实现、加编辑器都不用改 core / UI。

#### 上游更新跟随：启动检测 + 用户确认

`editors.lock`（仓库根）记录三个上游 repo / 分支 / pin 的 commit SHA 与契约版本。
应用启动时的 `EditorUpdateBanner` 检测远端 HEAD，发现差异后由**用户确认**是否升级：

- 双通道探测：`git ls-remote` 优先，失败回退 GitHub commits API。
  实测本机代理对 git 的 CONNECT 隧道返回 502，但普通 HTTPS GET 正常；
  离线 / 双通道失败时静默降级，不打断启动、不弹横幅。
- 用户可选「更新 / 跳过此版本 / 稍后」；决定写入知识库
  `.kbindex/editors.json`（pin rev、`skipped_revisions`、`pending`），跨启动持久。
- **边界**：插件是编译期 crate，运行时无法替换已链接进二进制的代码。
  确认升级只落地目标 rev 与 `requires_rebuild` 标记，源码同步和重建由下面命令完成，
  重建后重启应用生效；不做「点一下就立刻换代码」的虚假承诺。

```bash
mkv editors plugins                  # 已装插件与对齐的 upstream rev
mkv editors check   <root>           # 与启动检测同款逻辑，CLI 复核
mkv editors confirm <root>           # 无 GUI 时确认升级全部待更新插件
mkv editors skip    <root>           # 跳过当前版本
mkv editors upgrade <root>           # 按已确认 rev 同步源码并清除待重建标记
bash scripts/sync-editors.sh --check # 只读探测，输出 docs/editor-updates/report.json
cargo build --workspace              # 重建插件，重启应用生效
```

> 实测（2026-11-08）：会话期间上游 `deckcraft` / `wordcraft` 又各产生新提交，
> 双通道检测稳定报出 2 项更新，`confirm` 后 pin rev 与 `pending` 正确落盘、
> 再次检测归零 —— 跟随链路端到端可用。

### 9.6 上游适配器后端（进程外 MCP，架构闭环）

插件化解决的是「**怎么组织代码**」，但没有解决「**上游更新怎么进来**」：
插件是**编译期** crate，确认升级后仍需「同步源码 + 重建 + 重启」，
迭代节奏完全受限于本项目的发版。

上游三个项目**都已经实现了 MCP server**（JSON-RPC 2.0 over stdio），
且 README 明确承诺「Every action is a command with a stable id」——
**稳定的是命令 id，不是 crate API**。这正好是契约层需要的边界：

```text
  MKV 宿主（契约 v1 稳定）
       │  JSON-RPC 2.0 over stdio
       ▼
  上游进程（gridcraft-mcp / deckcraft-mcp / wordcraft-mcp）
```

于是「上游更新 → 内置编辑器随之更新」变成：**换上游进程版本，不动宿主代码**。

| 层 | 位置 | 职责 |
|---|---|---|
| 后端 trait | `packages/core/src/backend.rs` 的 `PluginBackend` | 与插件同形，但状态留在进程内、跨调用由宿主持有 |
| 客户端 | `packages/core/src/rpc.rs` | JSON-RPC over stdio；惰性 spawn、进程退出后自愈、**单次调用超时** |
| 适配器 | `packages/core/src/backend.rs` 的 `ExternalBackend` | 把上游 MCP server 适配成 `PluginBackend` |
| 选择 | `editors.lock` 的 `backends` 段 / `.kbindex/editors.json` | 逐插件切换 `builtin` / `external` |

```bash
mkv editors backend <root>                                        # 查看后端与健康状态
mkv editors backend <root> --set gridcraft --kind external --command mkv --arg editors --arg stub
mkv editors stub                                                    # 参考适配器（同协议最小实现）
```

> **为什么不能链接上游 crate**（证据保留在 `backend.rs` 的 `UPSTREAM_LINKING_FINDINGS`）：
> `gridcraft-xlsx` / `wordcraft-docx` **从未发布到 crates.io**
> （workspace 依赖是纯 path、无 version，`cargo package` 直接失败）；
> `deckcraft-pptx` 只发布 0.6.1、**落后 main 约两周**；
> 且三者写回均为**全量重写**，与本项目**包级外科手术**（未编辑部件字节级原样保留）
> 直接冲突——链接上游等于用「整包重写」替换「外科改写」。

#### 9.6.1 超时：上游挂起不得锁死宿主 UI

`ExternalConfig.timeout_ms`（默认 10 秒）是**单次调用的时限**。
早期实现定义了它却**从未使用**——`read_line` 是阻塞调用，
上游挂起不回应就会**永久阻塞**，而客户端被 `Mutex` 保护，
于是整个宿主 UI 被锁死。

现按「**读一行交给独立线程 + 主线程 `recv_timeout`**」实现，
**跨平台**：Unix 的 `set_nonblocking` 在 Windows 上不存在，
写成 `#[cfg(unix)]` 就等于让 Windows 继续裸奔。

超时后**整个客户端作废**：终止进程 + 丢弃读缓冲。
这一步是必需的——若只丢弃「本次响应」而留着缓冲，
下一次读到的会是**上一条请求的半截响应**，请求与响应从此错位。

#### 9.6.2 端到端验证：Windows 上也要真的跑

`packages/core/tests/rpc_e2e.rs` 是**跨平台**端到端测试，
覆盖 `open → view → apply → serialize` 完整四步 + 超时 + 自愈 + 错误透传 + 进程退出。

> **为什么用 `harness = false` 的自制 runner**：
> 该测试需要**同时**扮演「测试运行器」与「被客户端 spawn 的迷你上游」，
> 默认 libtest harness 无法在两者间切换；
> 而用 `sh -c` / `cmd /C` 起服务器在两端语法不同（`cmd` 没有 `while read`），
> 历史上正是这一点让端到端测试全部退化成 `#[cfg(unix)]`——
> **Windows 上零覆盖**，而 Windows 正是本项目的主力开发环境。
> 现在的做法是「启动自身」（`std::env::current_exe()`）并带 `--server=<模式>` 参数：
> 因为 harness 是自制的，argv 由我们自己解析，不受 libtest 参数规则约束。

完整协议与接入指南见 [`docs/upstream-mcp-adapter.md`](docs/upstream-mcp-adapter.md)。

---

## 10. Web 端：预构建只读索引

### 10.1 为什么

Web 端（wasm32）无法运行 Tantivy（tantivy → zstd → C 编译器），  
也不适合在浏览器里实时建索引（性能不可接受）。  
因此按技术方案设计 §4 / 决策②，采用**预构建只读索引**：

```
native 端（构建期）                Web 端（运行时）
─────────────────────            ─────────────────────────
mkv export-web                    include_str! 内嵌
  ├ 刷新索引                        └ 首帧同步载入，零网络
  ├ jieba + 领域词典切词
  └ 写 JSON 产物  ────────────►   查询侧用产物词表切词 → 命中
```

### 10.2 关键设计：查询与索引共用同一套分词

上一版排查过一个隐蔽缺陷：若**建索引**与**查查询**用了不同的分词器，  
会出现「进程内正常、跨进程零命中」。本方案因此：

- **建索引侧**（native）：用 jieba + 领域词典切词，把倒排表写进产物；
- **查询侧**（Web）：用**正向最大匹配**在产物自带的词表上切词。

词表来自索引产物本身（只收录语料中真实出现过的词），因此两侧切分结果一致，  
且 Web 端**不需要**引入 jieba（顺带省掉 zstd / C 编译器依赖）。

### 10.3 构建与部署

```bash
# 一键：导出产物 → 编译 wasm（产物被嵌入二进制）
scripts/build-web.sh examples/sample-vault packages/web/assets/mkv-index.json examples/domain-dict.txt

# 或手动
cargo run -p mkv-cli -- export-web examples/sample-vault packages/web/assets/mkv-index.json examples/domain-dict.txt
cargo build -p mkv-web --target wasm32-unknown-unknown
```

产物路径按以下顺序查找（`packages/web/build.rs`）：

1. 环境变量 `MKV_WEB_INDEX` 指定的路径（相对路径会同时相对**包目录**与**工作区根目录**解析）；
2. 约定路径 `packages/web/assets/mkv-index.json`。

找不到时写入**空索引占位**，构建**不中断**，并在构建输出里打印提示。

### 10.4 远端热替换（可选）

设置 `MKV_WEB_INDEX_URL=https://example.com/mkv-index.json` 后重新编译，
页面启动时会尝试从该地址拉取更新的产物并热替换内嵌版本 ——
这样「站点发布后仍可更新索引、无需重新编译 wasm」成为可能。  
拉取失败时静默保留内嵌版本，站点不会因此不可用。

### 10.5 本地运行与真实浏览器回归

仓库根目录已配好 `Dioxus.toml`（Web 端纯客户端配置，`out_dir = target/dx`）。
`index.html` 无需自建 —— dx 内置 `dev.index.html` 已提供 `<div id="main">`，
与 dioxus-web 默认 `rootname = "main"` 一致。

#### 安装 dx（版本必须与 dioxus 严格一致）

```bash
# 方案 A（Windows 推荐）：官方预编译二进制
# 下载 v0.7.10 的 dx-x86_64-pc-windows-msvc.zip 与同名 .sha256，校验通过后
# 把 dx.exe 放入 cargo 全局 bin（本机 C://Program Files\Rust\.cargo\bin\）
dx --version   # dioxus 0.7.10 (57d6794)

# 方案 B：源码编译（Windows MSVC 下可能卡在 proc-macro 的 link.exe）
cargo install dioxus-cli --version 0.7.10 --locked
```

> ⚠️ 本机实测：`cargo install --locked` 编译 631 个依赖后，`link.exe` 在链接
> `equator_macro` 的 proc-macro DLL 时挂起（CPU≈0、20+ 分钟无产出），rustc 与 cargo 一起卡住。
> 已改用 GitHub release 的预编译二进制（sha256 校验通过）全局安装。
> **不要装 0.8 alpha** —— 与 dioxus 0.7.10 的 wasm-bindgen 版本不兼容。

#### 启动与回归

```bash
# 启动开发服务器（首次自动装 wasm-bindgen-cli 0.2.130 与 esbuild 0.27.3，约 90 秒）
dx serve --web --package mkv-web --port 8080 --addr 127.0.0.1 \
  --open false --watch false --hot-reload false --interactive false

# 另开终端：真实浏览器点击回归（Playwright 驱动系统 Edge）
MKV_URL=http://127.0.0.1:8080 node scripts/browser-check.cjs
```

> **检索交互（2026-11-08 变更）**：已取消「搜索」按钮，**仅回车触发检索**。
> placeholder 明示「回车检索」；中文输入法选词时的回车（`is_composing()`）会被忽略，
> 不会打断输入；清空条件后回车自动回到「全部文档」页签。
> 该契约同时由 `mkv-ui` 的 SSR 测试与浏览器回归脚本守住。

回归脚本覆盖 **35 项交互断言**：A1–A18 为既有核心链路（首屏 9 篇跨 6 格式、顶栏索引来源、
预览切换、Web 只读语义、双链反链、回车检索 / 二次回车 / 清空恢复、Tab 切换、无未捕获异常），
B1–B4 为检索结果精确定位（位置标签、跳转高亮、命中正文、查看全文恢复），
B5–B8 为 F-10 关系图谱（节点与边渲染、缩放、点选跳转、格式图例），
C1–C3 为 F-06 跨格式引用（内联源片段内容 / 刷新 / 点选跳到源文档并定位），
D1–D3 为 F-12 正则检索（模式页签切换 / 合法正则命中 / 非法正则显示「正则无效」而非零命中），
D4–D6 为 F-07 多维过滤（面板可展开且候选非空 / 勾选后需回车重跑且结果只剩该格式 / 清空后恢复）。
截图与 JSON 报告落在 `docs/browser-validation/`。

远程回归脚本 `scripts/browser-remote-check.cjs` 覆盖 **15 项**：R1–R5 为远端热替换
（有效产物 / HTTP 500 回落 / 空索引回落 / 无未捕获异常），G1–G4 为图谱 V2，
D1–D6 为正则与过滤。

**已验证结果（2026-11-09）**：本地静态站点 **35/35 全通过**，0 console error、0 未捕获异常；
远端脚本 **15/15 全通过**（R1–R5 热替换与回落 + G1–G4 图谱 V2 + D1–D6 正则与过滤，
5.79s，报告 `docs/browser-validation/remote-report.json`）。

> ⚠️ 远端脚本的前置条件是**构建期注入了远端地址常量**的产物（`MKV_WEB_INDEX_URL`），
> 且必须用 `MSYS_NO_PATHCONV=1` 规避 Git Bash 的路径转换（见下方 §12 处的警告）。
> 线上站点的 `deploy-info.json` 里 `remote_index_url = null`（构建时未注入常量），
> Web 端运行时**不会发起**远端请求，因此远端脚本**不适用于线上站点** ——
> 线上回归请用 `scripts/browser-check.cjs`（35 项，`MKV_URL` 指向线上 URL）。

---

## 11. 文件监听与增量索引（P0）

按 PRD 硬指标：文档保存后**索引与双链 1 秒内更新**。

### 11.1 用法

```bash
# CLI：持续监听，Ctrl+C 退出
cargo run -p mkv-cli -- watch examples/sample-vault
cargo run -p mkv-cli -- watch D:/MyVault --debounce 200 --auto-commit

# Desktop 启动时自动在后台开启监听，无需手工开启
```

### 11.2 实现要点

| 环节 | 做法 |
|---|---|
| 依赖隔离 | `notify` 挂在 `mkv-core` 的 `native` feature 下；wasm32 依赖树仍是**零 C 依赖** |
| 事件过滤 | 只处理六种受支持格式；过滤 `.kbindex/`、`.git/`、隐藏文件、Office 临时锁文件 `~$xxx` |
| 去抖 | 250ms 窗口内同一路径只保留最后一个动作（Windows 一次保存会产生多个事件） |
| 增量路径 | 纯内容修改 → `Vault::update_document`（最快）；出现新路径或删除 → 额外 `Vault::relink_all` 重算全库双链 |
| 文件锁 | Desktop 通过 `native::apply_changes` 复用全局 `Vault`，不另开实例抢 `.kbindex/.lock` |
| 改名 | 旧路径按磁盘存在性自动翻转为删除，避免全文索引留下幽灵条目 |

### 11.3 实测指标

`packages/core/tests/watcher_e2e.rs`（5 项）：单文档修改索引耗时 **51ms**、真实 notify 端到端
**175ms**（含 OS 事件与 80ms 去抖），远低于 1 秒硬指标；覆盖新建 / 删除 / 改名 /
「后来才新建」的链接目标补链 / 目标删除后反链归零。

> UI 刷新已闭环：Desktop 端由 `VaultContext::revision` + 250ms 轮询自动重渲染（见 §13），
> 不再需要手动交互。

### 11.4 顺带修掉的两个缺陷

- `reindex` 原先不清空旧索引，删除 / 改名后仍返回幽灵命中 → 全量重建前先 `clear()`；
- `remove_document` 不清理别人指向它的入链 → 新增 `relink_all()`，在结构变更批次后重算。

---

## 12. Web 静态发布与 CI 质量门禁（P1 · 方案 A）

### 12.1 产物

```bash
scripts/publish-web.sh [vault_dir] [dict.txt]
# → dist/：index.html + 1 个 JS + 1 个 wasm + mkv-index.json + .nojekyll + deploy-info.json
# 实测 6 个文件 / 约 795 KB，零后端、完全离线
# （F-06 进阶②/③ 接线后实测：wasm 706,686B + JS 39,327B + 索引 21,102B ≈ 751 KB；见 §12.7）
```

脚本串联「导出索引 → `dx build --release` → 精确复制 → 路径相对化」。
dx 生成的资源路径是 `/./assets/...`，部署到 GitHub Pages 子路径会 404，脚本统一改成 `./assets/...`；
并按 `index.html` 的实际引用挑选文件，剔除 dx public 目录里残留的旧 hash 资源。
产物内嵌空 favicon，避免 `/favicon.ico` 404 造成 console error。

### 12.2 部署与验证

把 `dist/` 整个目录上传到任意静态托管即可：GitHub Pages、对象存储、nginx，或本地预览

```bash
python -m http.server 8080 --directory dist
```

已用真实静态服务器 + Edge 跑过 **18/18 项浏览器回归，0 console error、0 未捕获异常**。
报告：`docs/browser-validation/static-dist/`。

**线上站点（已上线）**：<https://shanksxiedev.github.io/migrant-knowledge-vault/>
（仓库 Settings → Pages → Source 设为 **GitHub Actions** 后，main 每次推送自动发布）。
部署完成后又对**线上 URL** 跑了一次完整回归：**18/18 全通过、0 console error、
0 未捕获异常、3.4s**；`index.html` / JS / wasm / `mkv-index.json` / `.nojekyll` /
`deploy-info.json` 全部 HTTP 200。报告：`docs/browser-validation/pages-live/`。

### 12.3 索引更新：替换 JSON 即可

默认索引内嵌于 wasm，更新需要重编译。若要「替换 JSON 即更新、免重编 wasm」：

```bash
MKV_WEB_INDEX_URL="https://your.site/mkv-index.json" scripts/publish-web.sh
```

把新的 `mkv-index.json` 放到该 URL 即可热替换；拉取失败会静默保留内嵌版本。
该链路已用 Playwright 拦截请求做过**真实浏览器实测（5/5 全通过）**：有效产物热替换、
HTTP 500 回落、空索引回落、顶栏知识库名同步。报告：`docs/browser-validation/remote-report.json`。

> ⚠️ **Git Bash 下不要写 `MKV_WEB_INDEX_URL=/remote-index.json`** —— 它会被 MSYS 转成
> `C:/Program Files/.../PortableGit/remote-index.json` 污染编译期常量。
>
> **机理（2026-11-09 实测，务必理解）**：路径转换发生在**启动原生 Windows 程序
> （也就是 `dx`）的那一刻**，MSYS 会把环境变量里形如 `/foo` 的值转成 Windows 绝对路径。
> 所以 **bash 脚本内部读到的值仍然是干净的** —— 在 shell 里 `echo $MKV_WEB_INDEX_URL`
> **查不出污染**，被污染的是**传进 dx 进程的那份**（也就是编译进 wasm 的常量）。
> 判据很简单：**值以 `/` 开头 = 会被转换**。
>
> 四种正确写法（任选其一）：
> ```bash
> MSYS_NO_PATHCONV=1 MKV_WEB_INDEX_URL=/remote-index.json scripts/publish-web.sh        # ① 全局关闭转换
> MSYS2_ENV_CONV_EXCL=MKV_WEB_INDEX_URL MKV_WEB_INDEX_URL=/remote-index.json \
>   scripts/publish-web.sh                                                              # ② 只排除本变量
> MKV_WEB_INDEX_URL="https://your.site/mkv-index.json" scripts/publish-web.sh          # ③ 用完整 URL
> MKV_WEB_INDEX_URL=./mkv-index.json scripts/publish-web.sh                             # ④ 用 ./ 相对路径
> ```
> `scripts/publish-web.sh` 已内置 **fail fast 自检**：在 MSYS 环境下若值以 `/` 开头
> 且未关闭转换，脚本会在**任何构建之前**直接报错退出，不再让坏常量流入产物。
>
> **这个坑的表现极具误导性**：常量被污染后浏览器会去请求一个 `file://` 本地路径，
> 页面**一切正常**（内嵌产物照常渲染），但状态栏永远不会出现「远端产物」——
> 看起来像产品缺陷，实际是环境变量写法问题。2026-11-09 曾因此误判过一次，
> 最终靠「浏览器原生 `new Request('/remote-index.json').url` 解析正确、
> 而 Rust 侧请求却指向 `file://`」这一对照实验才定位到真因。

### 12.4 CI（`.github/workflows/web.yml`）

| 门禁 | 内容 |
|---|---|
| 测试 | `cargo test --workspace`（270 项全绿） |
| 正则对拍 | 4000 例与 Python `re` 差分对拍，**真实差异必须为 0**（§13.9） |
| 移动端 | `cargo build -p mkv-mobile` 编译门禁；**真机验证仍需人工** |
| 依赖 | wasm32 依赖树零 `tantivy / jieba / zstd / rusqlite / git2 / notify / cc` |
| 构建 | `scripts/publish-web.sh` + 产物清单校验 + **wasm < 800KB 体积门禁** |
| 浏览器 | `dist/` 起静态服务 → Playwright Chromium 跑 35 项交互回归 |
| 发布 | main 分支自动 `actions/deploy-pages`（run #7–#12 连续实测 success）；PR 只验证不发布 |

CI 固定在 **Linux runner + 官方预编译 dx 0.7.10**（sha256 校验）—— Windows MSVC 的
`link.exe` 链接 proc-macro DLL 会挂起，源码编译不可靠。
仓库 Settings → Pages 的 Source 已设为 **GitHub Actions**（2026-11-08 核对通过）。
`deploy` job 先用 `GITHUB_TOKEN` 探测 `/repos/{owner}/{repo}/pages`，未配置时跳过发布并给
warning、不影响 `verify` 门禁；配置后 `actions/deploy-pages@v4` 自动执行，workflow 无需改动。

### 12.5 CI 首跑复盘（2026-11-08，真实 GitHub Actions）

仓库 `ShanksXieDev/migrant-knowledge-vault`（public），用 SSH 推送、匿名 API 查 runs。
首次真实执行连续暴露了几个**只在 Linux runner 上出现**的问题，逐一修复：

| # | 失败步骤 | 根因 | 修复 |
|---|---|---|---|
| 1 | 安装 dx | 官方 `.sha256` 同时列出 `tar.gz` 与 `zip` 两行；`cut -f1` 取到的是两行拼接串，与单文件 hash 永不相等 | 按资产名 `awk '$2 == f'` 精确取行，并加空值防呆 |
| 2 | 全量测试 | 首次 Linux 冷构建 exit 101；加诊断后重跑即通过，**根因未确认**（疑似 runner 冷启动资源波动） | 失败时输出关键错误 annotations；rust-cache 命中后稳定 |
| 3 | 构建静态站点 | `[profile.release] strip = true` 让较新 rustc 剥掉 wasm 的 `__wasm_bindgen_unstable` / `target_features` 自定义段 → `failed to read wasm-bindgen data` | 改为 `strip = false`；体积优化交给 dx 的 wasm-opt（作用于 wasm-bindgen 输出之后） |
| 4 | 同上（诊断轮） | 为定位 #3 补充日志收集 | 保留为长期可观测性能力 |

> 公开仓库的 **job logs 下载接口**（403 `Must have admin rights`）与 **artifact zip**（401）
> 都要认证，匿名只能读 `check-runs` annotations。因此 CI 现在会把失败步骤的末尾输出
> 写成 `::error::` annotations，后续任何失败都能直接定位，无需 admin 权限。
> Pages 未启用时 `deploy` 会跳过，不影响 `verify` 门禁。

**第 5 次 run（`12b49c1`）全绿**：总耗时 6m15s，`verify` 全部 15 步通过
（含 Playwright Chromium 的 18 项交互回归）；`deploy` 因 Pages 未启用按设计跳过并给出 warning。

### 12.6 Pages 自动发布闭环（2026-11-08）

| 项 | 事实 |
|---|---|
| 触发 | Source 设为 GitHub Actions 后，空提交 `54664c4` 推送 main → **run #7** |
| verify | 全部必需步骤通过（112 项测试 / 移动端编译 / wasm32 零 C 依赖 / release 构建 / Chromium 18 项回归 / 上传 Pages 产物） |
| deploy | `pages_check` 探测到 Pages 已启用 → `actions/deploy-pages@v4` **success**（不再是 skipped） |
| 线上产物 | `index.html` 560B、JS 48,795B、wasm 1,067,623B、`mkv-index.json` 10,565B，全部 HTTP 200 |
| 线上回归 | 对线上 URL 用系统 Edge 跑 **18/18 全通过**，0 console error、0 未捕获异常，3.4s |

> 从「push main」到「站点可访问」全自动，无人工上传步骤。
> 后续每次 main 推送都会重跑门禁并重新发布；PR 只验证不发布。

**A/B 上线复核（run #9，提交 `9ba2653`）**：wasm 体积优化 + 精确定位 + 图谱推送后，
**run #9 `42450219477` Success**（约 3 分钟）—— verify 全部必需步骤通过，新增的
「产物清单 + wasm < 800KB 体积门禁」与「Playwright Chromium 25 项交互回归」均为 success，
`actions/deploy-pages@v4` 再次 success。线上 `deploy-info.json` 的 `generated_at`
由 `04:33:14Z` 刷新为 `05:20:01Z`，`wasm_optimization` 记录
`1,192,134B → 667,487B`（CI/Linux 产物，与本地 Windows 的 667,161B 有平台差异），
`mkv-index.json` 14,174B（含 `segments`），首页 / 索引 / 元数据 / `.nojekyll` 全部 HTTP 200。
对**线上 URL** 用系统 Edge 跑完整 **25/25 全通过**，0 console error、0 page error，4.88s，
报告与截图：`docs/browser-validation/pages-live-ab/`。
至此 #6 / #7 / #8 / #9 连续 success，功能迭代后自动发布闭环依然稳定。

**F-06 上线复核（run #11，提交 `dc2067c`）**：跨格式引用推送后，
**run #11 `42452731879` Success**（约 3 分钟）—— `verify` job 全部必需步骤 success
（139 项测试 / 移动端编译 / wasm32 零 C 依赖 / release 构建 / 产物清单 + `wasm < 800KB` 门禁 /
Playwright Chromium 28 项交互回归 / 上传 Pages 产物），`deploy` job 的 `pages_check` 与
`actions/deploy-pages@v4` 再次 success。线上 `deploy-info.json` 的 `generated_at`
由 `05:27:09Z` 刷新为 `05:59:01Z`，`wasm_optimization` 记录
`1,239,166B → 706,213B`（CI/Linux；本地 Windows 为 `1,291,799B → 705,285B`），
`mkv-index.json` 19,464B（含 `refs`，9 docs / 12 links / 212 postings；示例文档 4 条引用
全部 `resolved = true`）；首页 / 索引 / 元数据 / `.nojekyll` 全部 HTTP 200。
对**线上 URL** 用系统 Edge 跑完整 **28/28 全通过**（含 C1–C3 引用卡片 / 刷新 / 点选跳转），
0 console error、0 page error、6.32s，报告与截图：`docs/browser-validation/pages-live-f06/`。
至此 #7–#11 连续 success，第 5 次实测「push main → 自动重新发布」闭环稳定。

**编辑器插件化上线复核（run #14，提交 `c17a19b` + `8bf2320`）**：三个编辑器拆成
独立插件 crate 后推送，**run #13 `42465836598` Failed**——但失败点与代码无关：
「安装 dx」步骤从 GitHub Release 直链拉取预编译二进制时，CDN 返回 504/502，
后续全部门禁被跳过（annotations 为空，因为该步骤未写诊断）。根因确认后改为
**双通道下载**（release 直链优先 + `api.github.com` 按 asset id 回退，各带重试，
共用同一份官方 sha256 校验），并在失败路径写 `::error::` annotation。
修复后 **run #14 `42466535003` Success**（5 分 38 秒）——verify 全部必需步骤通过
（157 项测试 / 移动端编译 / wasm32 零 C 依赖 / release 构建 / 产物清单 +
`wasm < 800KB` 门禁 / Playwright Chromium 28 项交互回归 / 上传 Pages 产物），
`deploy` job 的 `pages_check` 与 `actions/deploy-pages@v4` 均 success。
线上 `deploy-info.json` 的 `generated_at` 刷新为 `09:21:43Z`，
`wasm_optimization` 记录 `1,274,581B → 724,483B`（CI/Linux），仍低于 800KB 门禁；
首页 / 索引 / 元数据 / `.nojekyll` 全部 HTTP 200。对**线上 URL** 用系统 Edge 跑
完整 **28/28 全通过**，0 console error、0 page error，报告与截图：
`docs/browser-validation/pages-live-plugins/`。至此 #7–#12、#14 连续 success。

**CI 兜底通道上线复核（run #23，提交 `0a6dffa` + `c720b5e`）**：这一轮先修了一个
**误判的根因**，再补齐三条与 GitHub 完全无关的外部依赖兜底通道。

*run #21 的真正根因不是「GitHub 下载失败」*：该步骤的注解里出现了
「wasm-bindgen 预装失败」警告，而 wasm-bindgen 预装位于 dx 二进制下载**之后**——
说明 dx 实际下载成功、binaryen 也预装成功，步骤是在最后三行才崩的。
`303c2a4` 重构预装段时删掉了 `TARGET_DIR` 的**定义**，却留下了尾部的三行引用：

```bash
if [ -x "$TARGET_DIR/bin/wasm-opt" ]; then   # ← TARGET_DIR 已无定义
```

该步骤开头是 `set -euo pipefail`，未定义变量展开会直接以 `unbound variable`
终止步骤并返回 1。表象是「安装 dx 失败」、后续门禁全部 skipped，
而 `/tmp/mkv-ci.log` 又是空的（该步骤没有 tee 到日志），
所以「汇总失败诊断」注解也查不到任何线索——三重误导叠在一起。
修复：删除残留引用，统一用语义等价的 `$BIN_TARGET`。

*三条兜底通道*（详见 §12.8）：dx 本体 → crates.io 源码编译；
binaryen → apt 的 wasm-opt（**必须复制进 dx 缓存目录**，否则 dx 会自己去下载）；
wasm-bindgen-cli → crates.io 源码编译。

**run #22** 因修复推送触发并发组取消（`concurrency: pages` + `cancel-in-progress`）；
**run #23 `42473188254` Success**（10:51:31Z → 10:57:07Z，5 分 36 秒）——
`verify` 全部必需步骤 success（安装 dx / 全量测试 / 移动端编译 / wasm32 零 C 依赖 /
release 构建 / 产物清单 + `wasm < 800KB` 门禁 / Playwright Chromium 28 项交互回归 /
上传 Pages 产物），`deploy` job 的 `pages_check` 与 `actions/deploy-pages@v4` 均 success。

run #23 的注解里有一条**实证**：

```text
[warning] wasm-bindgen 预装失败，改用 crates.io 源码编译
```

即 GitHub 通道这次真的失败了，crates.io 兜底**在真实 CI 中触发并救活了流水线**——
不是纸面设计。

线上 `deploy-info.json` 的 `generated_at` 刷新为 `10:56:26Z`，
`wasm_optimization` 记录 `1,274,581B → 724,441B`（`-Oz --strip-debug`，
wasm-opt `version 127 (version_127)`），`wasm_opt_issue = None`，
`mkv-index.json` 19,464B，首页 / 索引 / 元数据 / `.nojekyll` 全部 HTTP 200。
对**线上 URL** 用系统 Edge 跑完整 **28/28 全通过**，0 console error、0 page error，
4.33s，报告与截图：`docs/browser-validation/pages-live-ci-fallback/`。

**二次实证（run #25，提交 `aad3f52`）**：文档回填提交再次触发自动发布，
**run #25 `42474866526` Success**（11:14:32Z → 11:21:13Z）——全部门禁 + `deploy-pages`
再次 success。线上 `generated_at` 刷新为 `11:20:35Z`，wasm 仍为
`1,274,581B → 724,441B`（与 run #23 逐字节一致，说明产物可复现），
首页 / 索引 / 元数据 / `.nojekyll` 全部 HTTP 200，线上回归再次 **28/28 全通过**
（报告与截图：`docs/browser-validation/pages-live-run25/`）。
即修复后连续两次（#23 / #25）全绿，CI 兜底通道与工作流稳定性得到实证。

### 12.7 wasm 体积优化（fat LTO + wasm-opt）

wasm 体积门禁是 **800,000 B** 的硬约束。本轮新增五项功能后一度逼近门禁
（891,480 B，超 91KB），靠猜是低效的，因此先建立了两把「尺子」再动手：

- `scripts/wasm-size-by-name.py`：解析 wasm 的 `name` 自定义段拿到
  「函数下标 → 函数名」，再与 `code` 段的函数体大小对齐，按模块路径归因。
  （旧的 DWARF 归因脚本在本项目的 rustc 输出上 99.9% 落到 unknown，已移除。）
  > ⚠️ **必须对 `cargo` 的原始产物运行**，即
  > `cargo build --release --target wasm32-unknown-unknown -p mkv-web` 后的
  > `target/wasm32-unknown-unknown/release/mkv-web.wasm`。
  > **wasm-bindgen CLI 的输出里 `name` 段已被剥离**（实测段结构中只剩
  > `__wasm_bindgen_unstable` / `producers` / `target_features` 三个自定义段），
  > 此时所有函数都会落到「未命名」，归因结果毫无意义。
  > 对照：`cargo` 原始产物的 `name` 段约 418 KB，`wasm-bindgen` 输出为 0。
- **消融测量法**：改一项配置 → 重新构建 → 用同一套参数跑 `wasm-opt` → 比对字节数。

归因结论（治理前基线：cargo wasm，未优化 3.31 MB）：

| 桶 | 字节 | 说明 |
|---|--:|---|
| `wasm_bindgen` | 485,218 | 主要是 `__wbindgen_describe_*`；**wasm-bindgen CLI 会剥离 `__wasm_bindgen_unstable` 自定义段**，最终产物里没有它 |
| `mkv_ui` | 213,926 | Dioxus 组件代码 |
| `mkv_core::regex` | 12,269 | 自研正则引擎只有 12KB，**不是**体积元凶 |
| `mkv_core::query` | 21,047 | 其中 `QueryFilter::clone` 单函数 10,131 B（派生 Clone 展开异常，已记录待优化） |

治理后复测（同一脚本，fat LTO + 自有 crate `opt-level="z"`，raw 2.28 MB / 代码段 940,757 B）：

| 桶 | 字节 | 占比 |
|---|--:|---:|
| `wasm_bindgen` | 250,499 | 26.6% |
| `core`（dioxus / std 等） | 200,920 | 21.4% |
| `mkv_ui` | 116,440 | 12.4% |
| `mkv_core::editor` | 72,638 | 7.7% |
| **`mkv_core::regex`** | **4,661** | **0.5%** |
| `mkv_core::query` | 4,266 | 0.5% |

两轮复测的结论一致：**自研正则引擎始终只占 0.5%~1%，从来不是体积元凶**——
真正的问题是缺少 fat LTO。

于是真正要动的不是业务代码，而是**编译配置**：

| 配置 | wasm（wasm-opt 后） | 相对基线 |
|---|--:|---|
| `opt-level=3` + `lto="thin"`（原配置） | 891,480 B | 超门禁 91KB |
| \+ 自有 crate `opt-level="z"` | 822,067 B | 超门禁 22KB |
| \+ `lto="fat"` | **693,304 B** | **余量 107KB（13.3%）** |

**关键结论：fat LTO 的收益（-129KB）远大于单改 opt-level（-69KB）**——
跨 crate 的重复代码与未达代码只有 LTO 能消除。

影响面控制（`Cargo.toml`）：

```toml
[profile.release]
lto = "fat"            # 全局：跨 crate 内联与去重
opt-level = 3          # 依赖 crate 保持默认

[profile.release.package.mkv-core]   # 仅自有 crate 做体积优化
opt-level = "z"
[profile.release.package.mkv-ui]
opt-level = "z"
[profile.release.package.mkv-web]
opt-level = "z"
```

- 依赖 crate（tantivy / sqlite / dioxus）仍是 `opt-level=3`，**不拖慢检索热路径**。
- 实测正则引擎吞吐下降 6%~22%（多数 8%~10%）：线性类 19.7→17.3、词类 7.7→7.0、
  字符类 10.5→9.9、惰性量词 8.6→7.9、嵌套重复 11.7→9.1 M字符/s。
  绝对速度仍远超交互需求（40KB 文档单次扫描约 4ms）。
- 桌面端同样受益（跨 crate 内联更多），代价是编译时间 17s → 45s。

`wasm-opt` 侧的改动：`scripts/publish-web.sh` 的参数链加入 `--converge`
（-483 B），并把激进参数排在前面、基础组 `-Oz --strip-debug` 兜底，
兼容不认识该参数的旧版 binaryen。

| 项 | 值 |
|---|---|
| 历史基线（插件化后，run #14 / #23） | 1,274,581 B → 724,441 B |
| 本轮五项功能落地后（修复前） | 891,480 B（超门禁） |
| 本轮体积治理后（本地） | **693,304 B**（余量 107KB） |
| F-06 进阶②/③ 接线后（本地） | **706,686 B**（余量 93KB / 11.7%） |
| 工具来源 | 本地优先 dx 自动下载的 `~/.dx/tools/binaryen-*/bin/wasm-opt`（版本与 wasm-bindgen 产物匹配），PATH 上的 binaryen 仅作兜底；CI 侧改为**预装进 dx 缓存目录**（含 apt 兜底，见 §12.8），避免构建期再联网 |
| 失败策略 | 找不到工具 / 未产出更小产物 → 只警告并保留原始 wasm，绝不阻断本地发布；CI 的 `wasm < 800KB` 门禁会据此判定失败 |

`deploy-info.json` 记录 `wasm_optimization`（前后体积与参数）。CI 断言 wasm < 800 KB，
防止优化悄悄失效。CI 侧 ubuntu runner 额外 `apt install binaryen` 兜底，
但脚本优先用 dx 自带版本，避免 runner 预装的较老 binaryen 解析不了新版 wasm 特性；
优化未采用时会把 wasm-opt 的 stderr 打进日志便于排障。

> Windows / Git Bash 注意：shell 传给 Python 的是 `/c/...` POSIX 路径，
> 需在 Python 侧转成 `C:/...` 并补 `.exe` 后缀，否则会误判「未找到 wasm-opt」（本轮实测踩过）。

> **F-06 之后**：新增引用解析与示例文档，本地 wasm 1,291,799B → **705,285B（-45.4%）**、
> CI/Linux 1,239,166B → **706,213B**；索引 JSON 因新增 `refs` 字段增至 19,464B。
>
> **插件化之后**：编辑器实现移出 core，但新增契约层与启动升级横幅，
> 本地 wasm 1,338,199B → **724,696B（-45.8%）**，仍低于 800KB 门禁（余量约 75KB）；
> CI/Linux（run #23）1,274,581B → **724,441B**，同样过门禁。
>
> **五项进阶功能之后**：新增正则引擎、查询过滤、图谱布局、流程图节点、
> 引用插入入口，wasm 一度涨到 891,480B（超门禁 91KB）；
> 经体积归因 + 消融测量，改用 **fat LTO + 自有 crate `opt-level="z"`**，
> 降到 **693,304B**，比插件化后的历史基线（724,441B）**还小 31KB**。
>
> **F-06 进阶②/③ 之后**：新增差异计算（`SourceDiff` / `diff_lines`）、
> 原生应用唤起（`office_locator` + `launch_office_plan`）与 UI 差异块渲染，
> 本地 wasm 693,304B → **706,686B**（余量 93KB / 11.7%），仍过门禁。
> 索引 JSON 因新增 `ref_snapshots` 字段增至 21,102B（+1.6KB，4 条引用基线）。


### 12.8 CI 外部依赖的三条兜底通道

CI 构建期要拉三类外部产物，它们过去全部托管在 `objects.githubusercontent.com`
这一个 CDN 上，链路抖动就整条流水线报废（run #13 直链 504、run #21 前置排查期间双通道皆败）。
现在每个依赖都有「主通道 + 与 GitHub 无关的备胎」：

| 依赖 | 主通道 | 兜底通道 | 关键点 |
|---|---|---|---|
| dx 0.7.10 | release 直链 → `api.github.com` 资产（各 3 次重试，共用官方 sha256） | **crates.io 源码编译** `cargo install dioxus-cli --locked --no-default-features` | dioxus-cli 0.7.10 已发布 `Cargo.lock`，`--locked` 可用且解析可复现；`default` feature 集为空，`--no-default-features` 等价 |
| binaryen（wasm-opt） | release 直链 → API 资产 → 整目录安装到 dx 缓存 | **apt 的 wasm-opt** | 必须复制进 `~/.dx/tools/binaryen-<索引号>/bin/`，只留在 PATH 上没用——dx 只认缓存目录，目录里没有它就会自己去 GitHub 下载 |
| wasm-bindgen-cli | release 直链 → API 资产 | **crates.io 源码编译** `cargo install wasm-bindgen-cli --locked` | 版本号取自项目 `Cargo.lock`（dx 也是按项目锁文件决定装哪个版本），避免版本漂移导致 dx 重装 |

两个容易被忽略的细节：

1. **两个版本号不是一回事**。dx 二进制里硬编码 `wasm-opt129`（工具索引号 → 缓存目录名
   `binaryen-129`）与 `binaryen-version_127`（上游 release tag → 下载 URL）。
   混用会让 dx 认为「未安装」而重新下载（run #19 卡死根因）。
2. **`CARGO_HOME` 可能不可写**。GitHub runner 的 `CARGO_HOME` 默认是 `/usr/local/cargo`
   （镜像系统路径），普通用户未必有写权限，`cargo install` 会报 Permission denied。
   因此 `cargo install` 统一走一个封装：先试默认路径，失败则换 `$HOME/.cargo` 重试，
   并同步更新 `CARGO_BIN` / `PATH` / `DX_PATH`，保证「装到哪」和「后面去哪找」是同一个位置。

配套的静态检查：把工作流里所有 `run` 块的变量引用与块内赋值做比对，
可以稳定报出未定义变量（用 `303c2a4` 跑能复现 `$TARGET_DIR`，当前版本为 0）。
这类 bug 在 CI 上表现为「莫名其妙的失败 + 空日志」，靠肉眼看日志极难定位，
静态比对能在提交前就拦下来。

### 12.9 CI 偶发失败复盘：run #28 的 watcher 用例（2026-11-09）

文档提交触发的 **run #28 `42531894566` 失败于第 7 步「全量测试」**，
唯一失败用例是 `real_notify_watcher_updates_index_end_to_end`
（`packages/core/tests/watcher_e2e.rs`）。

**第一步是辨析 panic 文案，而不是照搬上一次的修法**：

| panic 文案 | 失败模式 | 修法方向 |
|---|---|---|
| 「端到端 X 秒超过阈值」 | **慢** | 放宽阈值 / 分级判定（run #17 的 `003242e`） |
| 「10 秒内应收到文件变更批次」 | **没来** | 修等待就绪的**方式**（本次） |

两者是**不同的失败模式**。run #17 加的「超过 2 秒只告警」式分级判定，
对 run #28 这种「事件根本没送到」的情况**完全无效**——所以它第一次出现时并没有被治好。

**根因**：等待监听器就绪用的是固定 `std::thread::sleep(400ms)`，这是一次**竞态假设**。
监听线程若在 sleep 结束时还没建立好 inotify watch，紧接着的写入就发生在 watch 建立之前，
而 **inotify 不追溯**——那次写入永远不会再产生事件。
CI runner 负载高 → 监听器就绪慢 → 命中；本地 Windows 与 run #27 均未命中。

**修法**：把「等待就绪」从**时间假设**换成**事件确认**——
「写探针文件 → 等报告」循环，直到收到第一个批次为止（总上限 60 秒；
超时仍会 panic，从而保留「监听完全没生效」这一真实缺陷的捕获能力）。
探针文件不参与后续断言，探针耗时不计入端到端计时。

**复核**：`cargo test -p mkv-core --test watcher_e2e` 5/5 通过；
**run #30 `42532487967` Success**（19 步 + `deploy-pages` 全过），
线上 wasm 690,486 B，线上回归再次 35/35 全绿。

> 另一条同期教训：Git Bash 下 `MKV_WEB_INDEX_URL=/remote-index.json` 会被 MSYS
> 转成 Windows 路径**污染编译期常量**，使远端热替换静默失效。
> `scripts/publish-web.sh` 已内置 fail fast 自检（详见 §12.3 的警告）。

---

## 13. UI 能力增强（自动刷新 / 外部唤起 / 精确定位 / 关系图谱 / 跨格式引用）

### 13.1 UI 自动刷新（消除「需一次交互才刷新」）

| 环节 | 做法 |
|---|---|
| 变更信号 | `mkv-core::native::revision()`：`AtomicU64`，每批成功落盘的增量变更 / 内置编辑保存后 +1 |
| UI 订阅 | `VaultContext::revision: Signal<u64>`；`DocList` / `ResultList` / `PreviewPane` / `BacklinkPanel` 均读取它 |
| 刷新时机 | Desktop 在 **UI runtime 内**用 `use_future` + `tokio::time::sleep(250ms)` 轮询，变化即 `ctx.refresh()` |
| 刷新内容 | 递增修订号触发重渲染；重算反链、重跑当前检索、更新状态栏文档数 |

> 关键约束：后台监听线程**不能**直接写 Dioxus Signal（必须在 runtime 内），
> 因此用原子修订号做桥，而不是跨线程 `set`。250ms 轮询与 P0 的 1 秒指标对齐。

### 13.2 F-08 用系统默认应用打开原始文档

预览区新增「用系统应用打开」按钮，仅在有本地文件的后端出现（Web 只读索引不显示）。

| 平台 | 命令 |
|---|---|
| Windows | `cmd /C start "" <绝对路径>`（第一个空串是窗口标题） |
| macOS | `open <绝对路径>` |
| Linux / BSD | `xdg-open <绝对路径>` |

- 路由层不再硬编码 `excel.exe` / `visio.exe`（既不在 PATH、也只在 Windows 存在），
  改为交给系统文件关联；需要指定程序仍可用 `EditorRouter::set_external_program`。
- 三重校验：格式在六种受支持格式内、`..` 路径逃逸被拒、文件真实存在。
- 只 `spawn` 不 `wait`；用户在外部应用保存后由 P0 文件监听增量更新索引与双链，
  再由 §13.1 自动刷新界面，形成闭环。
- `KnowledgeService` 新增 `supports_external_open()` / `open_externally()`；
  Web 只读后端默认返回明确错误，不静默失败。

### 13.3 验证与未覆盖项

`packages/core/tests/external_open.rs`（2 项）+ `packages/ui/tests/desktop_features.rs`（4 项）
+ `editor.rs` 新增 3 项单测；`cargo test --workspace` 共 **112 项全绿**。

> ⚠️ 未做真机验证：Desktop GUI 窗口与「真的打开 Office / Visio」需要人工点击验收；
> CI 只能保证编译通过、命令构造与路径安全正确。

---

### 13.4 检索结果精确定位（PRD §7.1 最后一项）

新增跨端纯 Rust 模块 `packages/core/src/locate.rs`：

- `segment_text(text, format)` 把已抽取的纯文本切成**带位置标签的片段**：
  Markdown 标题 / 段落、Excel `{sheet} · 第 N 行`、PPT `第 N 页`、Word `第 N 段`、
  Visio `第 N 页`、Drawio `节点 N`。片段拼接后与原文严格相等（单测覆盖）。
- 不改抽取器、不改倒排表：`PrebuiltDoc.segments` 用 `#[serde(default)]`，
  **v1 旧索引产物仍可解析**，运行期用同一函数补算片段。
- `KnowledgeService::locate()` 默认 `None`，Web / Desktop 各自实现；查询词按
  「完整词 → 最长片段 → 单字」降级，**正文优先、标题兜底**。
- UI：结果列表显示 `📍 第 N 段` 等位置标签；点击后预览区显示定位条、只渲染命中片段
  及前后各一段并高亮，「查看全文」可恢复全文。浏览器 B1–B4 覆盖标签 / 跳转 / 高亮 / 恢复。

PRD §7.1 唯一未勾选项由此补齐；F-06「引用处动态显示内容」另见 §13.6（已实现）。

### 13.5 双链关系图谱 F-10

- `core/graph_layout.rs`：Fruchterman–Reingold 变体的**确定性**力导向布局
  （固定 seed → 同一输入产出逐字节相同的坐标）。按文档格式分簇，度数作权重，
  枢纽节点更靠中心。
- `locate::build_graph_snapshot_with_positions()`：悬空目标 / 自环 / 重复边剔除，
  节点按路径排序保证产物可复现。
- **坐标在构建期算好**：布局引擎只在 `native` 特性下参与编译，Web 端坐标随
  预构建索引产物发布（`PrebuiltIndex.graph_positions`，`#[serde(default)]` 保 v1 兼容）；
  运行期不再做 240 轮浮点迭代。旧产物 / 远端热替换拿到旧 schema 时回退到
  **确定性环形布局**，保证图谱面板不白屏、不 panic。
- UI 侧栏新增「关系图谱」页签；主区 SVG 渲染节点与有向边，支持放大 / 缩小 / 重置、
  **点选节点跳回文档预览**、**格式图例**、**视口裁剪**（大库只渲染视口内节点，
  状态栏显示「已渲染 N / 共 M 个节点」）。
- 浏览器 B5–B8 覆盖 9 节点 / 12 边、缩放 transform 变化、点选跳转与图例存在。


### 13.6 跨格式引用 F-06

- 语法 `@相对路径#选择器`，例如 `@03-台账/模具台账.xlsx#第2行`、
  `@05-方案/项目方案.pptx#第1页`、`@02-图示/评审流程.drawio#节点3`；
  `#2` 这类纯数字按 1-based 段序号定位。选择器与 `locate::TextSegment.label`
  做去空白包含匹配，因此直接复用六格式的位置标签。
- `core/refs.rs`：`parse_references()` 只抽取**带选择器**的引用，不带 `#` 的
  `@文件` 仍归既有双链。既有 `extract_links()` 的 token 字符集不含 `#`，遇到新语法
  会自然截断到扩展名得到正确目标路径，双链 / 反链 / 图谱完全不受影响。
- `build_reference_views()` 生成内联视图（位置标签、源片段快照、内容指纹、目标片段下标）；
  `render_chunks()` 按字节偏移无损切分正文。`source_hash`（FNV-1a）随源内容变化，用于标记变更。
- `PrebuiltDoc.refs` 用 `#[serde(default)]`，**schema 仍为 v1**，旧产物运行期补算。
  `KnowledgeService::references()` 默认空；Web 返回构建期快照，Desktop 每次实时读取源文件，
  配合文件监听的 `revision` 自动刷新。
- UI 在引用处渲染 `.mkv-ref-card`（源片段内容 + 位置标签 + 指纹），提供「↻ 刷新引用」入口；
  点选卡片跳到源文档并高亮该片段。未解析的引用明确标注，绝不编造内容。
- 验证：core 6 项单测 + 预构建兼容 2 项 + `refs_ui.rs` 4 项 SSR；
  浏览器 C1–C3（内联显示 `PN-1024` / 刷新 / 跳转定位）。
  进阶① 见 §13.6.1，进阶② 见 §13.6.2，进阶③ 见 §13.6.3。

> 边界：Web 只读端显示构建期快照，「源更新后刷新」在 Desktop 生效；
> 定位到的是**抽取后的文本片段**，不是原生 Office 应用内的单元格 / 幻灯片对象。

#### 13.6.1 进阶①：编辑器内「插入引用」入口

- **core**：新增 `types::ReferenceTarget { path, title, candidates }`，
  `candidates` 是 `(1-based 序号, 标签文本, 类型)` 三元组（`refs::selector_candidates`）。
  `KnowledgeService::reference_targets()` 默认空；`NativeService` 覆写为
  「遍历文档 → 判定格式 → 读原文 → `locate::segment_text` → 取候选」。
- **UI**：`InsertTarget` 枚举（表格 `(sheet,row,col)` / 演示 `(slide,shape)` /
  文档 `paragraph`）由三个面板通过 `Signal<Option<InsertTarget>>` 共享；
  单元格 / 形状 / 段落旁各有一个「引用」小按钮设置插入位置，
  工具栏「插入引用」按钮展开面板（目标文档下拉 + 片段候选下拉 + 插入按钮）。
- **动作复用**：token 由 `refs::format_reference_token` 生成，**追加**到目标文本末尾，
  然后复用既有的 `PluginAction::SetCell / SetShapeText / SetParagraph` 提交——
  **刻意不给 `PluginAction` 加新变体**，插件契约 v1 保持冻结（见 §9.5 的补充约束 30）。
- **性能**：`reference_targets()` 要读全部文档原文，因此面板**条件挂载**，
  只在用户点开时才求值，不进渲染热路径。
- 验证：`packages/ui/tests/ref_insert_ui.rs` 11 项 SSR（含 mock service 与 mock 插件，
  动作过**真实 `plugin::apply_action`** 后读回视图确认文本已追加）。

> 已知限制：追加不带分隔符（`PN-1024@03-台账/模具台账.xlsx#2`）；
> 候选序号是**去重后**列表位置，目标文档若含重复标签，`#N` 可能落到相邻片段。

#### 13.6.2 进阶②：源变更差异高亮

- **能力**：引用卡片在源片段相对「上次看到的内容」发生变化时，直接显示
  **哪些行被改了**（新增行 / 删除行分别着色），而不只是「指纹变了」这个提示。
- **core**：`core/refs.rs` 新增 `SourceDiff { added, removed, lines }` 与
  `DiffLine { kind, text }`（`DiffLineKind` = Context / Added / Removed）；
  `diff_lines(old, new)` 做**行级**差异：先剥公共前后缀（真实场景源文件通常只改一小块），
  剩余区间做 LCS 动态规划；区间过大时**放弃精细比较**、判定为整体替换——
  宁可显示粗一点的结果，也不让 UI 卡住。
  `build_reference_views_with_snapshots()` 在命中快照且内容有变化时给引用视图挂上 `diff`。
- **快照的键**：`snapshot_key(target_path, selector)` = `mkv:refsnap:{path}\u{1f}{selector}`，
  用 ASCII 单元分隔符（U+001F）而不是 `#`，避免与选择器本身含 `#` 时产生歧义。
  快照由调用方提供，本模块只做**纯计算**，因此三端都能用。
- **Desktop 端基线**：持久化在库内 `.kbindex` 的 `ref_snapshots` 表
  （`meta.rs`）。全量重建时刷新基线（「我确认已看到当前内容」），
  增量修改（文件监听 / 编辑保存）**不**刷新——于是源文件被改动时能被识别为变更。
- **Web 端基线**：浏览器没有持久化存储，「上次是什么样」只能由**构建端**带过来。
  因此产物新增顶层字段 `PrebuiltIndex.ref_snapshots`（`#[serde(default)]`，
  旧产物缺该字段时为空表 = 首次见到，不产出差异，不是错误），
  存的是**源片段全文**。`export-web` 每次构建读上一份产物的 `ref_snapshots` 当基线，
  算出的差异写进产物的 `refs[].diff`，Web 端**零运行时开销**即可显示高亮。

> **踩过的坑：基线必须存全文，不能存 `snippet`。**
> `ReferenceView.snippet` 是给卡片内联显示用的**截断版**（120 字符）。
> 最初实现拿它当基线，跟当前**全文**做差异比对，结果任何超过 120 字符的段落
> 都会在截断处被判成「删除 + 新增」——用户看到的是**根本不存在的变更**。
> 现在基线存全文，`long_paragraph_against_full_text_baseline_produces_no_false_diff`
> 这条单测专门盯这个不变式。

- **UI**：`ReferenceCard` 在卡片内渲染 `.mkv-ref-diff` 差异块
  （只列变化的行，上下文行不展示），标题给出「新增 N 行 / 删除 M 行」。
- **验证**：
  - core 单测 3 项（基线是全文 / 长段落无假差异 / 源真变了产出差异）；
  - CLI 端到端：`export-web` 连跑三次——① 建基线无差异 → ② 内容不变**零假差异**
    → ③ 改源文件后精确产出 `removed '| 注射速度 | 80 mm/s |'` +
    `added '| 注射速度 | 95 mm/s |'`，其余 3 条引用不受影响；
  - `packages/ui/tests/ref_diff_office_ui.rs` 5 项 SSR；
  - 浏览器 C5（源未变更时不出现差异块）。

#### 13.6.3 进阶③：原生 Office 应用内定位

- **能力**：从引用卡片直接唤起**原生 Office 应用**并尽量定位到目标片段。
- **能力矩阵（诚实降级，不假装能定位）**：

  | 格式 | 能否在应用内定位 | 锚点形态 |
  |------|------------------|----------|
  | Excel | ✅ 单元格 | `ms-excel:ofe|u|<file-url>#<工作表>!<单元格>` |
  | PowerPoint | ✅ 页 | `ms-powerpoint:ofe|u|<file-url>#slide=<n>` |
  | Word / Visio / Drawio / Markdown | ❌ 只打开文件 | 无公开的应用内定位协议 |

- **三层结构**（推导与执行分离，因此纯函数可在任意宿主单测）：
  1. **推导**：`core/office_locator.rs` 纯函数——`fragment_from_label()` /
     `fragment_from_selector()` / `plan_office_launch()` 产出 `OfficeLaunchPlan`；
     `plan.is_precise()` 回答「能不能定位到片段级」，`plan.note` 说明「为什么这样降级」。
  2. **执行**：`core/editor.rs` 的 `EditorRouter::launch_office_plan()` 只 `spawn` 不 `wait`
     （Office 由用户自行关闭，保存后由 P0 文件监听增量更新索引）；
     命令以 **argv** 形态下发、不经 shell，路径里的 `%` / `&` 不会被二次解析
     （这一点对 Windows 的协议 URI 尤其重要）。
  3. **跨端抽象**：`KnowledgeService::supports_office_locate()` / `open_in_office()`
     默认 false / 报错（Web 只读端**不显示**按钮，不谎报能力）；
     `NativeService` 覆写为 true 并委派 `Vault::open_in_office()`（标签优先、选择器兜底）。
- **UI**：`ReferenceCard` 在卡片旁渲染 `.mkv-ref-office`「在 Office 中打开」按钮；
  点击后按 `plan.is_precise()` 如实回报「已定位到片段」或
  「已打开文件（该格式无法在应用内定位到片段）」。
- **验证**：`packages/ui/tests/ref_diff_office_ui.rs` 5 项 SSR
  （支持本地进程的后端显示按钮 / Web 只读端**不显示**）；
  浏览器 C4（Web 端 `mkv-ref-office` 数为 0）。

> **已知限制**：目前只做了 Desktop（`NativeService`）接线；
> Mobile 端尚未接入（`mkv-mobile` 未实现 `open_in_office`）。
> 「原生 Office 应用内定位」在 Word / Visio / Drawio 上仍只打开文件——
> 这是**格式本身没有公开协议**导致的硬约束，不是实现缺失。


### 13.7 多维过滤 F-07

- **core**：`core/query.rs` 定义 `SearchQuery`（文本 + 模式 + 过滤器 + limit）与
  `QueryFilter`（格式 / 标签 / 修改时间窗 / 来源路径）。
  `QueryFilter::matches()` 是**三端共用**的判定函数：过滤发生在**检索结果**上，
  而不是索引查询里——这样 Web 预构建索引与 Native 索引的过滤语义完全一致，
  无需在两条链路上各实现一遍。
- **服务层**：`KnowledgeService::search_query()` 默认实现 = 「旧 `search` + 过滤」，
  因此**既有后端无需修改**；`validate_query()` 区分「正则写错」与「正则合法但没命中」，
  否则 UI 只能给用户一句含糊提示。
- **标签来源**：`core/tags.rs` 从 front-matter、目录名（只取最后一级）、文档格式
  三处抽取标签；`tags_of()` / `all_tags()` 默认空，`NativeService` 与
  `PrebuiltService` 各自覆写（后端不支持标签过滤时整块不渲染，不显示空标签区）。
- **UI**：可折叠的 `<details class="mkv-filter-panel">`，四个维度分组 + 「清空过滤」。
  过滤条件变更**不自动检索**（每勾一下就全库重扫代价太高），只更新条件并提示按回车；
  「清空过滤」点击后主动重跑一次。
- 验证：浏览器 D4–D6（面板可展开且候选非空 / 勾选后需回车且结果只剩该格式 /
  清空后恢复）；实测格式候选 6 个，勾选 Markdown 后 6 条 → 4 条，
  且每条结果的格式标识都等于所选格式。


### 13.8 流程图节点 F-11

- `core/diagram.rs`：从 Visio 的 shape `ID` / Drawio 的 `mxCell@id` 抽取**稳定节点 id**，
  同时保留显示文本、所属层级与页码。
- 为什么需要独立模块：纯文本抽取会把这些信息全丢掉——只剩一串标签文本，
  「节点 3」既可能是 id 也可能是序号，引用方无从判断。节点清单作为**可选补充信息**
  供引用解析使用，`parse.rs` 的纯文本抽取保持不变（不影响既有索引与回归）。
- 只依赖 `quick-xml` 与 `zip`，**无原生依赖**，因此 wasm32 可编译
  （Web 端构建期抽取节点清单打进索引产物）。
- `PrebuiltDoc.nodes` 用 `#[serde(default)]`：旧产物没有该字段时为空数组，
  节点级引用退回到「页 / 段」级匹配，语义不丢。


### 13.9 正则检索 F-12

- **为什么自研**：`regex` crate 会带来 `regex-syntax` + `regex-automata` +
  `aho-corasick` 的依赖闭包，对 wasm 体积的贡献是数十 KB 级，
  而正则检索只是**一个可选的检索模式**——不值得为此牺牲体积门禁。
  `core/regex.rs` 是零依赖（仅 `std`）、无 `unsafe` 的回溯式引擎：
  递归下降解析器 → AST → 扁平指令表 → 迭代式回溯 VM（回溯栈在堆上，
  长文本不会爆调用栈），带**回溯步数上限**杜绝灾难性回溯卡死界面。
- **语义**：左优先 + 贪婪优先，与 Python `re` 对齐；`$` 采用**严格末尾**语义
  （与 Rust `regex` crate 一致），因此与 Python `re` 的差异是**有意偏差**。
- **循环状态用回滚日志整体还原**：这是本轮修掉的实质缺陷。
  早期实现只在回溯时恢复「当前循环」的轮数，嵌套循环里内层重放上一轮循环体时
  会把**外层**的轮数多算一次，`{2,}` 的最小次数被虚假满足 → 匹配结果比参考引擎短
  （差分对拍 9 处真实差异）。改成「任何循环状态修改先记旧值、回溯弹帧时逆序还原」
  之后，真实差异 **9 → 0**。这与 CPython `sre` 把重复计数压进回溯栈的做法等价。
- **差分对拍固化为 CI 门禁**：`packages/core/examples/regex_fuzz_{ref.py,run.rs,cmp.py}`
  做 4000 例确定性对拍（seed 固定），**真实差异必须为 0**。
  为什么单列一道门禁：这个引擎是「手写回溯 VM + 计数式循环状态」，
  嵌套重复的最小次数约束曾绕过 9 处真实差异，而 270 项单测**全部绿**——
  单测覆盖不到「引擎与参考引擎语义不一致」这一类缺陷。
- **UI**：检索栏新增「普通词条 / 正则」两个模式页签；正则写错时显示
  `正则无效：…` 而**不是**「0 条命中」；命中结果带正则模式的高亮。
- 验证：浏览器 D1–D3（模式切换 / 合法正则命中 / 非法正则报错并可切回）；
  CI 门禁「正则引擎差分对拍（与 Python re 对齐）」。

---

## 14. 相关文档

- `../本地个人知识库-需求描述.md` —— 需求主文档（v1.2）
- `../技术方案设计.md` —— 技术方案与已确认决策清单
- `../PRD.md` —— 产品需求文档（七章节）
- `docs/upstream-mcp-adapter.md` —— 上游编辑器 MCP 适配器（进程外后端协议与后端选择基线）

---

## 15. 下一步

1. **移动端真机验证**：CI 只能保证可编译
2. **Desktop GUI 人工验收**：自动刷新与外部唤起的实际窗口行为
3. **F-10 图谱进阶**：大库虚拟渲染已具备视口裁剪，按标签着色仍未做
4. **wasm 体积**：门禁余量已回到 11.7%，但 `QueryFilter::clone` 单函数 10,131 B
   属异常膨胀，值得单独优化
5. **Node.js 20 警告**：CI 中 `actions/*` 的 Node 20 deprecation 提示，待官方迁移后评估升级
6. **插件升级链路真机验收**：Desktop GUI 确认横幅、重建后重启生效的实际窗口行为
7. **CI 兜底通道的常态化验证**：crates.io / apt 兜底目前只在 run #23 真实触发过一次
   （wasm-bindgen），其余分支只经过本地仿真。可考虑做成一条可手动触发的 workflow，
   强制走兜底路径，避免「平时不跑、要用时已经腐化」
8. **F-06 进阶③ 的 Mobile 接线**：`mkv-mobile` 尚未实现 `open_in_office`
9. **上游适配器后端的常态化验证**：进程外 JSON-RPC 后端已闭环，
   但真实上游（`gridcraft-mcp` 等）尚未在真机跑通

> 2026-11-09 补记（第二轮）：**F-06 进阶②/③ 已接线闭环**——
> 进阶②「源变更差异高亮」core 计算 + UI 渲染 + Web 构建期快照链路全通
> （§13.6.2），并修掉一个**会产生假变更**的实现缺陷：
> 快照基线原先取的是 120 字符的 `snippet`（展示用截断版），
> 跟当前全文比对会让长段落每次都被误报为「已变更」；
> 现改为基线存**全文**并随产物发布（`PrebuiltIndex.ref_snapshots`）。
> 进阶③「原生 Office 应用内定位」按「推导 / 执行 / 跨端抽象」三层接线（§13.6.3），
> Excel 能落到单元格、PowerPoint 能落到页，Word / Visio / Drawio **如实降级为只打开文件**。
> **386 项测试全绿**；**浏览器回归本地 37/37 全绿**（新增 C4 / C5）；
> **wasm 706,686 B**（门禁余量 11.7%）；CLI 端到端 `export-web` 连跑三次验证
> 「建基线 → 零假差异 → 精确产出变更」三种状态。
>
> 2026-11-09：**五项进阶功能已全部落地并闭环**，架构随之收敛：
> F-07 多维过滤、F-10 双链图谱（力导向 + 构建期坐标）、F-11 流程图节点、
> F-12 正则检索（自研零依赖引擎）、F-06 进阶① 编辑器内插入引用。
> **270 项测试全绿**；**浏览器回归本地 35/35、远端 15/15**；
> **正则差分对拍真实差异 0**（并固化为 CI 门禁）；
> **wasm 891,480B → 693,304B**（fat LTO + 自有 crate `opt-level="z"`），
> 门禁余量从「超 91KB」回到「富余 107KB」，比插件化后的历史基线还小 31KB（§12.7）。
> 五项功能里唯一需要动**编译配置**而非业务代码的是 wasm 体积——
> 根因是 fat LTO 缺失，不是功能代码膨胀（自研正则引擎只占 12KB）。
>
> 2026-11-09 补记：**run #28 `42531894566` 偶发失败**于「全量测试」，
> 唯一失败用例是 watcher 端到端测试——但 panic 文案是「10 秒内收不到事件批次」
> 而不是「太慢」，属**另一种失败模式**（详见 §12.9）。根因是用固定 `sleep(400ms)`
> 等监听器就绪的**竞态假设**，CI 高负载下 watch 尚未建立，写入事件被 inotify 永久丢弃；
> 已改为「写探针 → 等报告」的事件确认式等待。
> **run #30 `42532487967` Success**（19 步 + `deploy-pages` 全过），
> 线上 wasm 690,486 B，线上回归再次 **35/35 全绿**。

> 2026-11-08：技术债务已清理，P0 文件监听、P1 静态发布（方案 A）+ CI 门禁、
> Desktop UI 自动刷新与 F-08 外部唤起均已落地。代码已推送到 GitHub，
> **CI 真实首跑已核对**并修掉 dx 校验和、wasm strip 等 Linux 专属问题（复盘见 §12.5）。
> **Pages 自动发布已闭环**：run #7 的 `deploy-pages` 实测 success；run #11 复核 F-06 上线后
> 站点 <https://shanksxiedev.github.io/migrant-knowledge-vault/> **28/28 项回归全绿**（§12.6）。
> **A + B 均已完成**（§12.7 / §13.4 / §13.5）：wasm 本地 1,245,191B → 667,161B（-46.4%）、
> CI 1,192,134B → 667,487B；检索结果精确定位与 F-10 双链图谱落地；
> **F-06 跨格式引用已完成并上线**（§13.6）：引用处内联源片段、刷新与点选跳转；
> **139 项测试 + 本地 28/28、线上 28/28 项浏览器回归全绿**（run #11 `42452731879` Success，
> #7–#12 连续 success）；线上结论已回填。
> **编辑器已插件化并上线**（§9.5）：三个独立插件 crate + 契约 v1 + 启动检测 / 用户确认升级，
> 对齐 `storytold/{gridcraft,deckcraft,wordcraft}` 的真实 commit；
> **157 项测试 + 本地 28/28、线上 28/28 项浏览器回归全绿**（run #14 `42466535003` Success，
> CI/Linux wasm 1,274,581B → 724,483B 仍过 800KB 门禁）；
> **CI 下载双通道已加固**（§12.6）：release 直链 504 时回退 `api.github.com` 资产通道，
> 修掉 run #13 因 dx 二进制下载失败而全流水线跳过的问题。
> **CI 兜底通道已补齐并上线**（§12.6 / §12.8）：先定位出 run #21 的真正根因是
> **未定义变量 `$TARGET_DIR`**（重构时删了定义却留下引用，在 `set -u` 下终止步骤，
> 表象却像「下载失败」，日志还恰好是空的，三重误导叠加）；
> 随后为 dx / binaryen / wasm-bindgen 三类外部依赖各补一条与 GitHub 无关的链路
> （crates.io 源码编译 / apt 兜底），**run #23 `42473188254` Success**，
> 其中 wasm-bindgen 的 crates.io 兜底在真实 CI 中确实触发并救活了流水线；
> 线上 28/28 项浏览器回归全绿，CI/Linux wasm 1,274,581B → 724,441B；
> **run #25 `42474866526` 二次实证 Success**，产物逐字节一致、线上回归再次 28/28。
