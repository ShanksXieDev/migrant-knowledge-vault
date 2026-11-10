# 设计图生成工具

`docs/design/` 下的 **SVG 图**与 **HTML 图册**由本目录脚本生成。Markdown（`01-原型图.md` / `02-详细设计.md`）是**唯一可编辑源**，SVG/HTML 是**派生产物**。

## 产物

| 脚本 | 作用 | 依赖 |
| :-- | :-- | :-- |
| `gen-wireframes.mjs` | 生成 11 张原型线框 SVG（`svg/p01..p11-*.svg`） | 仅 Node（内置模块） |
| `render-mermaid.mjs` | 把 Markdown 中的 mermaid 代码块渲染为独立 SVG（`svg/fig-*.svg`） | `mermaid@12`、`puppeteer`、本机 Chrome/Edge |
| `gen-html.mjs` | 组装三份自包含 HTML 图册（`prototype.html` / `detailed-design.html` / `index.html`） | 仅 Node（内置模块） |
| `check-svg.mjs` | 校验 `svg/` 下所有文件的 XML 合法性 | `jsdom` |

## 使用

```powershell
# 0) 首次准备（仅渲染 mermaid / 校验需要）
cd docs/design/tools
npm init -y
npm i mermaid@12 puppeteer jsdom
# 无需下载 Chromium：直接复用本机浏览器
$env:MKV_CHROME = "C:\Program Files\Google\Chrome\Application\chrome.exe"

# 1) 生成线框 → 2) 渲染设计图 → 3) 组装 HTML → 4) 校验
node gen-wireframes.mjs
node render-mermaid.mjs
node gen-html.mjs
node check-svg.mjs
```

## 环境变量

| 变量 | 默认 | 说明 |
| :-- | :-- | :-- |
| `MKV_DESIGN_DIR` | 脚本上级目录（`docs/design`） | 产物输出目录 |
| `MKV_CHROME` | `C:\Program Files\Google\Chrome\Application\chrome.exe` | 无头浏览器可执行文件（也可指向 Edge） |

## 约定

- 线框 SVG 的样式令牌内联在 `gen-wireframes.mjs` 顶部的 `STYLE` 常量，颜色遵循 **Miro 设计规范**（Near Black `#1c1c1e`、Blue 450 `#5b76fe`、pastel 强调等）。
- mermaid 主题变量内联在 `render-mermaid.mjs` 的 `themeVars`，与线框共用同一套色板。
- 修改 Markdown 中的 mermaid 源码后，必须重跑 `render-mermaid.mjs` + `gen-html.mjs`，否则 HTML 图册与 Markdown 会脱节。
- 输出目录（`docs/design/`）下的 `.pages.json` / `.figs.json` 为脚本生成的清单（页面与图块的 id、标题、来源行号），供 HTML 组装与后续扩展使用。
