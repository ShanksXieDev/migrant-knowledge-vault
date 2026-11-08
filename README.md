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
| Web 端索引 | ✅  | 预构建只读索引：构建期内嵌二进制，运行时零网络加载；**dx serve + Edge 真实浏览器 18/18 项验证通过** |
| 文件监听    | ✅  | `notify` 增量索引 + 双链重算；实测索引 51ms、端到端 175ms（PRD 要求 1s 内）        |
| 静态发布 / CI | ✅  | `scripts/publish-web.sh` 产出 6 文件纯静态站点；GitHub Actions 测试 + 浏览器回归门禁 |
| 双链图谱视图  | 🚧 | 可视化引用关系，列入 v0.2                                                    |

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

cargo test --workspace     # 103 个测试（core 84 / ui 3 / web 16）
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

回归脚本覆盖 **18 项交互断言**：首屏 8 篇跨 6 格式文档、顶栏索引来源、点击切换预览、
正文非空、Web 端只读语义（无「内置编辑」按钮）、双链反链、回车触发中文检索与自动切页签、
点击结果打开文档、检索区无搜索按钮、**仅输入不触发检索**、二次回车检索、清空回车恢复全部文档、
Tab 切换、无未捕获 JS 异常。
截图与 JSON 报告落在 `docs/browser-validation/`。

**已验证结果（2026-11-08）**：18/18 全通过，0 console error、0 未捕获异常，耗时约 2.0 秒。

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

> 已知限制：Desktop 端索引在后台更新，UI 需一次交互（切页签 / 重新检索）才刷新列表，
> 自动重渲染列入后续增强。

### 11.4 顺带修掉的两个缺陷

- `reindex` 原先不清空旧索引，删除 / 改名后仍返回幽灵命中 → 全量重建前先 `clear()`；
- `remove_document` 不清理别人指向它的入链 → 新增 `relink_all()`，在结构变更批次后重算。

---

## 12. Web 静态发布与 CI 质量门禁（P1 · 方案 A）

### 12.1 产物

```bash
scripts/publish-web.sh [vault_dir] [dict.txt]
# → dist/：index.html + 1 个 JS + 1 个 wasm + mkv-index.json + .nojekyll + deploy-info.json
# 实测 6 个文件 / 约 646 KB，零后端、完全离线
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
| 测试 | `cargo test --workspace`（103 项全绿） |
| 移动端 | `cargo build -p mkv-mobile` 编译门禁；**真机验证仍需人工** |
| 依赖 | wasm32 依赖树零 `tantivy / jieba / zstd / rusqlite / git2 / notify / cc` |
| 构建 | `scripts/publish-web.sh` + 产物清单校验 |
| 浏览器 | `dist/` 起静态服务 → Playwright Chromium 跑 18 项交互回归 |
| 发布 | main 分支自动 `actions/deploy-pages`；PR 只验证不发布 |

CI 固定在 **Linux runner + 官方预编译 dx 0.7.10**（sha256 校验）—— Windows MSVC 的
`link.exe` 链接 proc-macro DLL 会挂起，源码编译不可靠。
启用 Pages 前需在仓库 Settings → Pages 把 Source 设为 **GitHub Actions**。

---

## 13. 相关文档

- `../本地个人知识库-需求描述.md` —— 需求主文档（v1.2）
- `../技术方案设计.md` —— 技术方案与已确认决策清单
- `../PRD.md` —— 产品需求文档（七章节）

---

## 14. 下一步

1. **Desktop UI 自动刷新**：文件监听已更新索引，列表 / 反链目前需交互触发重渲染
2. **F-08 外部唤起编辑**：路由层已就绪，接系统默认应用
3. **双链图谱视图 F-10**：可视化引用关系
4. **检索结果精确定位**：跳到页 / 单元格 / 段落（PRD §7.1 唯一未勾选项，依赖 F-06）
5. **移动端真机验证**：CI 只能保证可编译

> 2026-11-08：技术债务已清理，P0 文件监听与 P1 静态发布（方案 A）+ CI 门禁均已落地。
