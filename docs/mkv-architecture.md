# 本地个人知识库 · 架构设计

> 技术定调：**Cordis 插件内核 + Tauri 2 桌面壳 + React UI**
> 版本：v1.0（2026-11-10） · 状态：设计定稿

## 1. 需求与约束

| 项 | 内容 |
|---|---|
| 形态 | 本地离线桌面应用，无云端依赖 |
| 仓库 | 本地文件夹即文档结构（Git 友好，无私有数据库容器） |
| 存储格式 | `.docx` / `.xlsx` / `.pptx`（OOXML 通用格式）、飞书 docxXML（智能文档）、`.base`（多维表格）、`.pdf` |
| 编辑能力 | Office 三件套原子级编辑、块级智能文档、多维表格、PDF 查看 |
| 架构原则 | 一切皆插件，能力可替换、可重组 |

## 2. 分层架构

```
┌─────────────────────────────────────────────┐
│ UI 层 · React（文件树 / 标签页 / 命令面板）    │
├─────────────────────────────────────────────┤
│ Cordis 运行时（Context / Services / Events） │
│   └─ 插件层：仓库 / 各格式引擎 / 检索 / 设置   │
├─────────────────────────────────────────────┤
│ Tauri IPC 桥（invoke / 事件 / sidecar 管理）  │
├─────────────────────────────────────────────┤
│ Rust 壳 · Tauri 2（fs+notify / SQLite / 沙箱）│
└─────────────────────────────────────────────┘
```

- **Cordis 运行在 WebView 渲染进程**（cordis 支持 browser 环境，dsh Web UI 已验证此路径）。它不碰 OS 能力，OS 能力全部经 Tauri 命令获取——保证插件在浏览器/Node 中可单测。
- **Rust 壳是唯一拥有系统权限的一层**：文件读写限制在工作区路径作用域内。
- 依赖的是 **cordis 本体**（@cordisjs/core），不依赖 dsh 的任何包。

## 3. Cordis 运行时设计

### 3.1 Host / Client 拆分

- `host/`：宿主侧类型扩展（仓库服务、引擎注册表等 `Context` 服务声明）
- `client/`：UI 侧类型扩展（视图状态、编辑器会话等）
- 两侧各自 `declare module 'cordis'` 扩展同一 Context 接口，避免类型冲突

### 3.2 配置组合

`cordis.yml` 决定加载哪些插件、以什么参数加载。换引擎 = 改配置 + 换包，不改内核：

```yaml
# cordis.yml 示例
plugins:
  core:repo:
    workspace: D:/Docs/MyVault
  core:engine-pdf: {}
  core:engine-block: {}
  core:engine-onlyoffice:
    sidecarPort: 8888
    enabled: false        # 未安装 OnlyOffice 时禁用，降级为外跳
  core:engine-base:
    baseInterop: xlsx     # .base 互通策略：xlsx
```

### 3.3 核心服务契约（TypeScript）

```ts
import type { Context } from 'cordis'

declare module 'cordis' {
  interface Context {
    repo: RepoService
    engines: EngineRegistry
    search: SearchService
  }
  interface Events {
    'repo/file-changed': (e: FileChangeEvent) => void
    'engine/session-opened': (s: EditorSession) => void
  }
}

interface RepoService {
  list(dir: string): Promise<FileNode[]>
  read(path: string): Promise<Uint8Array>
  write(path: string, data: Uint8Array): Promise<void>
  watch(dir: string): Disposable   // 底层走 Tauri notify 事件
}

type FormatExt = '.docx' | '.xlsx' | '.pptx' | '.pdf' | '.fdoc' | '.base'
type Capability = 'edit-native' | 'edit-adapter' | 'view'

interface EngineRegistry {
  register(engine: DocEngine): Disposable
  resolve(ext: FormatExt, want: Capability): DocEngine | undefined
}

interface DocEngine {
  id: string                                  // 'core:engine-onlyoffice'
  formats: Array<{ ext: FormatExt; capability: Capability }>
  open(ctx: Context, file: FileRef): Promise<EditorSession>
}

interface EditorSession {
  mount(el: HTMLElement): void
  save(): Promise<void>
  close(): Promise<void>
  onDirty(cb: () => void): Disposable
}
```

- `.docxxml` = 飞书 docxXML 的本地封装（信封格式自定义：`{ version, blocks: Block[], assets: {...} }`）。
- 引擎按 `capability` 分级注册，UI 据此决定"可编辑 / 只读 / 外跳"。

## 4. 插件清单

| 插件 | 职责 | 依赖的 Tauri 命令 |
|---|---|---|
| `core:repo` | 文件树、监听变更、重命名/移动/删除 | `fs_*`、`watch_*` |
| `core:engine-onlyoffice` | sidecar 生命周期 + 嵌入 DS 编辑器（iframe 指向 127.0.0.1:port）+ JWT 回调 | `sidecar_*` |
| `core:engine-block` | TipTap 编辑器 + 飞书块 JSON 双向序列化（含 callout 等特有块的自定义块降级） | `fs_read/write` |
| `core:engine-base` | Univer RT 多维表格 + xlsx 互通 + `.base` 导入器（experimental） | `fs_*` |
| `core:engine-pdf` | pdf.js 渲染、页码/缩放/批注查看 | `fs_read` |
| `core:search` | 索引调度（文本抽取后写 SQLite FTS5） | `search_*` |
| `core:ui-shell` | 设置、主题、i18n、命令面板注册 | `settings_*` |

插件分发：内部先用 pnpm workspace monorepo（`packages/plugins/*`），后续如需开放生态再发 npm 并打 topic 标签（类似 dsh-plugin 的发现机制）。

## 5. Rust 壳（Tauri 2）设计

命令组（tauri-specta 生成 TS 类型）：

- `fs_list / fs_read / fs_write / fs_rename / fs_remove`（作用域强制在工作区内）
- `watch_start / watch_stop` → `tauri::Emitter` 推 `repo://changed` 事件（notify-rs，含防抖）
- `sidecar_start / sidecar_stop / sidecar_status`（OnlyOffice DS 独立进程，健康检查 + 崩溃重启）
- `search_index / search_query`（SQLite FTS5，存于 app data 目录，非工作区）
- `settings_get / settings_set`

OnlyOffice sidecar 说明：DS Community Edition 以外部二进制随应用分发（安装包 +1~2GB，这是已知代价）；首次启动向用户明示。JWT 回调端点由 Rust 侧实现（`/callback` 保存命令），编辑器经 `iframe` 嵌入。

## 6. 五格式数据流

| 格式 | 打开 | 编辑 | 保存 |
|---|---|---|---|
| .docx/.xlsx/.pptx | OnlyOffice DS 直接打开磁盘文件 | OOXML 原生内核 | callback 写回原文件（原子替换） |
| .docxxml | fs_read → 反序列化 → TipTap | 块编辑 | 序列化写回（pretty JSON，Git diff 友好） |
| .base | 导入器解析 → 自研编辑器 | 多维表格编辑 | 导出器生成 .base |
| .pdf | fs_read → pdf.js | 只读 | — |

## 7. 里程碑

| 阶段 | 内容 | 验收 |
|---|---|---|
| M0 | 脚手架：Tauri 2 + React + Cordis + pnpm workspace，cordis.yml 驱动加载空插件 | 插件可加载/卸载/热替换 |
| M1 | core:repo + core:engine-pdf + ui-shell | 文件树 + PDF 查看 + 监听刷新 |
| M2 | core:engine-onlyoffice sidecar | 真实 docx/xlsx/pptx 打开-编辑-保存闭环 |
| M3 | core:engine-block（TipTap + 飞书块序列化） | .docxxml 创建/编辑/保存，与飞书 API 块结构往返一致 |
| M4 | core:engine-base + search + 打磨 | 多维表格 + 全文检索 |

## 8. 风险与对策

| 风险 | 对策 |
|---|---|
| OnlyOffice 包体大（+1~2GB）、AGPL v3 | 个人自用无碍；未来若闭源分发，客户端仅经本地 HTTP API 通信、服务端本体保持开源分发 |
| `.base` schema 未公开 | 不作主存储；导入器标注 experimental；主存自定义公开 schema + xlsx 互通 |
| Cordis 在浏览器环境的学习/生态成本 | 参考 dsh 源码的 Host/Client 工程拆分；插件契约保持薄 |
| 飞书特有块（callout/okr/jira/chat_card）无对应物 | .docxxml 信封保留原始 block_type，编辑器降级渲染，导出时无损还原 |
