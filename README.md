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
| 内置编辑器   | ✅  | GridCraft（Excel）/ DeckCraft（PPT 完整编辑）/ WordCraft（Word），始终读写原生文件 |
| Web 端索引 | ✅  | 预构建只读索引：构建期内嵌二进制，运行时零网络加载；**真实浏览器 28/28 项验证通过** |
| 文件监听    | ✅  | `notify` 增量索引 + 双链重算；实测索引 51ms、端到端 175ms（PRD 要求 1s 内）        |
| 静态发布 / CI | ✅  | 6 文件纯静态站点；CI 门禁 + Pages 自动发布；wasm 经 wasm-opt 压到约 705KB（[线上站点](https://shanksxiedev.github.io/migrant-knowledge-vault/)） |
| UI 自动刷新 | ✅  | 索引修订号 + 250ms 轮询，文档列表 / 反链 / 检索结果自动重渲染 |
| 外部唤起 F-08 | ✅  | 用系统默认应用打开原始文件（Windows `start` / macOS `open` / Linux `xdg-open`） |
| 检索精确定位 | ✅  | 结果列表位置标签 + 预览区定位条与片段高亮；跳到段落 / 页 / 单元格行 / 节点（§13.4） |
| 双链图谱 F-10 | ✅  | SVG 环形图谱：缩放、点选节点跳转；浏览器 B5–B7 验证（§13.5） |
| 跨格式引用 F-06 | ✅  | `@文件#片段` 内联显示源内容（行 / 页 / 节点 / 段）、刷新与跳转；浏览器 C1–C3 验证（§13.6） |

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
│   │   ├── src/gridcraft.rs      #   GridCraft：Excel 内置编辑器内核
│   │   ├── src/deckcraft.rs      #   DeckCraft：PPT 内置编辑器内核（完整编辑）
│   │   ├── src/wordcraft.rs      #   WordCraft：Word 内置编辑器内核
│   │   ├── src/prebuilt.rs       #   预构建只读索引（Web 端只读检索）
│   │   ├── src/prebuilt_export.rs#   预构建索引生成端（native，需 jieba）
│   │   ├── src/demo.rs           #   六格式示例知识库生成器
│   │   └── src/vault.rs          #   Vault 门面：统一 API + 索引文件锁
│   ├── ui/                       # mkv-ui：Dioxus 共享组件（三端复用）
│   │   ├── src/components/editors.rs # 内置编辑器面板（Grid/Deck/WordCraft）
│   │   └── assets/main.css       #   内联样式表（include_str!）
│   ├── cli/                      # mkv-cli：命令行工具（mkv.exe）
│   ├── desktop/                  # mkv-desktop：Dioxus desktop
│   ├── web/                      # mkv-web：Dioxus web（wasm32）
│   │   ├── build.rs              #   把索引产物嵌入 wasm 二进制
│   │   ├── assets/mkv-index.json #   约定路径的预构建索引产物
│   │   └── src/loader.rs         #   索引加载器（内嵌 / 远端热替换）
│   └── mobile/                   # mkv-mobile：Dioxus mobile
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

回归脚本覆盖 **28 项交互断言**：A1–A18 为既有核心链路（首屏 9 篇跨 6 格式、顶栏索引来源、
预览切换、Web 只读语义、双链反链、回车检索 / 二次回车 / 清空恢复、Tab 切换、无未捕获异常），
B1–B4 为检索结果精确定位（位置标签、跳转高亮、命中正文、查看全文恢复），
B5–B7 为 F-10 关系图谱（节点与边渲染、缩放、点选跳转），
C1–C3 为 F-06 跨格式引用（内联源片段内容 / 刷新 / 点选跳到源文档并定位）。
截图与 JSON 报告落在 `docs/browser-validation/`（本轮：`f06/`）。

**已验证结果（2026-11-08 下午）**：**28/28 全通过**，0 console error、0 未捕获异常。

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
# 实测 6 个文件 / 约 760 KB，零后端、完全离线
# （F-06 后 wasm 经 wasm-opt 从 1,291,799B 压到 705,285B；见 §12.7）
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

> ⚠️ Git Bash 下不要写 `MKV_WEB_INDEX_URL=/remote-index.json` —— MSYS 会把它转成
> `C:/Program Files/.../PortableGit/remote-index.json` 污染编译期常量。
> 用完整 URL，或 `./mkv-index.json` 这类相对路径。

### 12.4 CI（`.github/workflows/web.yml`）

| 门禁 | 内容 |
|---|---|
| 测试 | `cargo test --workspace`（139 项全绿） |
| 移动端 | `cargo build -p mkv-mobile` 编译门禁；**真机验证仍需人工** |
| 依赖 | wasm32 依赖树零 `tantivy / jieba / zstd / rusqlite / git2 / notify / cc` |
| 构建 | `scripts/publish-web.sh` + 产物清单校验 + **wasm < 800KB 体积门禁** |
| 浏览器 | `dist/` 起静态服务 → Playwright Chromium 跑 28 项交互回归 |
| 发布 | main 分支自动 `actions/deploy-pages`（run #7 / #8 / #9 连续实测 success）；PR 只验证不发布 |

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

### 12.7 wasm 体积优化（wasm-opt）

`dx build --release` 默认 `--debug-symbols=true`，产物里带大量 DWARF 调试段；
而 `[profile.release] strip = false` 不能改（`strip = true` 会剥掉 wasm-bindgen 自定义段，见 §12.5）。
因此优化放在**发布脚本**里，作用于 wasm-bindgen 输出之后：`wasm-opt -Oz --strip-debug`。

| 项 | 值 |
|---|---|
| 优化前 | 1,245,191 bytes（含新增位置片段与图谱代码） |
| 优化后 | **667,161 bytes（约 651 KB）** |
| 压缩率 | **-46.4%** |
| 工具来源 | 优先 dx 自动下载的 `~/.dx/tools/binaryen-*/bin/wasm-opt`（版本与 wasm-bindgen 产物匹配），PATH 上的 binaryen 仅作兜底 |
| 失败策略 | 找不到工具 / 未产出更小产物 → 只警告并保留原始 wasm，绝不阻断本地发布；CI 的 `wasm < 800KB` 门禁会据此判定失败 |

`deploy-info.json` 新增 `wasm_optimization` 记录前后体积与参数；CI 断言 wasm < 800 KB
（未优化约 1.2 MB），防止优化悄悄失效。CI 侧 ubuntu runner 额外 `apt install binaryen` 兜底，
但脚本优先用 dx 自带版本，避免 runner 预装的较老 binaryen 解析不了新版 wasm 特性；
优化未采用时会把 wasm-opt 的 stderr 打进日志便于排障。

> Windows / Git Bash 注意：shell 传给 Python 的是 `/c/...` POSIX 路径，
> 需在 Python 侧转成 `C:/...` 并补 `.exe` 后缀，否则会误判「未找到 wasm-opt」（本轮实测踩过）。

> **F-06 之后**：新增引用解析与示例文档，本地 wasm 1,291,799B → **705,285B（-45.4%）**，
> 仍低于 800KB 门禁；索引 JSON 因新增 `refs` 字段从 14,174B 增至 19,464B。

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

- `locate::build_graph_snapshot()`：确定性环形布局，高度数节点靠中心；
  悬空目标 / 自环 / 重复边剔除，节点按路径排序保证产物可复现。
- `KnowledgeService::graph_snapshot()` 默认空图；Web 用索引 links、Desktop 用 Vault 出链。
- UI 侧栏新增「关系图谱」页签；主区 SVG 渲染节点与有向边，支持放大 / 缩小 / 重置，
  **点选节点跳回文档预览**。浏览器 B5–B7 覆盖 9 节点 / 12 边、缩放 transform 变化与点选跳转。


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

> 边界：Web 只读端显示构建期快照，「源更新后刷新」在 Desktop 生效；
> 定位到的是**抽取后的文本片段**，不是原生 Office 应用内的单元格 / 幻灯片对象。

---

## 14. 相关文档

- `../本地个人知识库-需求描述.md` —— 需求主文档（v1.2）
- `../技术方案设计.md` —— 技术方案与已确认决策清单
- `../PRD.md` —— 产品需求文档（七章节）

---

## 15. 下一步

1. **移动端真机验证**：CI 只能保证可编译
2. **Desktop GUI 人工验收**：自动刷新与外部唤起的实际窗口行为
3. **F-06 进阶**：编辑器内插入引用的入口、源变更差异高亮、原生 Office 应用内定位
4. **F-10 图谱进阶**：力导向聚类、按标签着色、大库虚拟渲染
5. **Node.js 20 警告**：CI 中 `actions/*` 的 Node 20 deprecation 提示，待官方迁移后评估升级

> 2026-11-08：技术债务已清理，P0 文件监听、P1 静态发布（方案 A）+ CI 门禁、
> Desktop UI 自动刷新与 F-08 外部唤起均已落地。代码已推送到 GitHub，
> **CI 真实首跑已核对**并修掉 dx 校验和、wasm strip 等 Linux 专属问题（复盘见 §12.5）。
> **Pages 自动发布已闭环**：run #7 的 `deploy-pages` 实测 success；run #9 复核 A/B 上线后
> 站点 <https://shanksxiedev.github.io/migrant-knowledge-vault/> **25/25 项回归全绿**（§12.6）。
> **A + B 均已完成**（§12.7 / §13.4 / §13.5）：wasm 本地 1,245,191B → 667,161B（-46.4%）、
> CI 1,192,134B → 667,487B；检索结果精确定位与 F-10 双链图谱落地；
> **F-06 跨格式引用已完成**（§13.6）：引用处内联源片段、刷新与点选跳转；
> **139 项测试 + 本地 28/28 项浏览器回归全绿**；线上结论随本次 main 推送的 CI 复核回填。
