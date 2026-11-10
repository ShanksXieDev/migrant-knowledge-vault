// 组装自包含 HTML 图册（临时构建工具）
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const OUT = process.env.MKV_DESIGN_DIR || path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const SVG_DIR = path.join(OUT, 'svg')
const pages = JSON.parse(fs.readFileSync(path.join(OUT, '.pages.json'), 'utf8'))
const figs = JSON.parse(fs.readFileSync(path.join(OUT, '.figs.json'), 'utf8'))

const readSvg = (f) => fs.readFileSync(path.join(SVG_DIR, f), 'utf8')
const figById = (id) => figs.find((f) => f.id === id)

const CSS = `
:root{
  --ink:#1c1c1e; --slate:#555a6a; --ph:#a5a8b5; --white:#ffffff;
  --blue:#5b76fe; --blue-press:#2a41b6; --ok:#00b473;
  --border:#c7cad5; --ring:rgb(224,226,232); --line:#e9eaef;
  --coral:#ffc6c6; --coral-dk:#600000; --teal:#c3faf5; --teal-dk:#187574;
  --rose:#ffd8f4; --orange:#ffe6cd; --pink:#fde0f0; --red:#fbd4d4; --red-dk:#8a2020;
  --yellow-dk:#746019; --sunken:#f7f8fb; --bar:#fbfbfd;
  --display:'Roobert PRO Medium','Noto Sans','Segoe UI','Microsoft YaHei',sans-serif;
  --body:'Noto Sans','Segoe UI','Microsoft YaHei',sans-serif;
  --mono:'Cascadia Code',Consolas,'Courier New',monospace;
}
*{box-sizing:border-box}
html{scroll-behavior:smooth}
body{margin:0;background:var(--white);color:var(--ink);font-family:var(--body);font-size:16px;line-height:1.5;letter-spacing:-.16px}
a{color:var(--blue);text-decoration:none}
a:hover{color:var(--blue-press)}
.hero{padding:56px 40px 40px;border-bottom:1px solid var(--line);background:linear-gradient(180deg,#fbfbfd 0%,#fff 100%)}
.hero h1{font-family:var(--display);font-size:56px;line-height:1.15;letter-spacing:-1.68px;font-weight:400;margin:0 0 12px}
.hero .sub{font-size:22px;line-height:1.35;letter-spacing:-.44px;color:var(--slate);max-width:880px;margin:0 0 20px}
.hero .meta{display:flex;flex-wrap:wrap;gap:8px}
.chip{border:1px solid var(--border);border-radius:20px;padding:6px 14px;font-size:13px;color:var(--slate);background:#fff}
.chip.acc{background:var(--coral);border-color:transparent;color:var(--coral-dk)}
.chip.tl{background:var(--teal);border-color:transparent;color:var(--teal-dk)}
.layout{display:grid;grid-template-columns:264px minmax(0,1fr);gap:0;align-items:start}
.toc{position:sticky;top:0;max-height:100vh;overflow:auto;padding:28px 20px;border-right:1px solid var(--line);background:var(--bar)}
.toc h4{font-family:var(--display);font-size:12px;letter-spacing:.5px;text-transform:uppercase;color:var(--ph);margin:20px 0 8px;font-weight:400}
.toc a{display:block;padding:5px 8px;border-radius:8px;color:var(--slate);font-size:13.5px}
.toc a:hover{background:var(--sunken);color:var(--ink)}
.toc a.sub{padding-left:22px;font-size:13px;color:var(--ph)}
main{padding:40px 48px 96px;max-width:1180px}
section{margin-bottom:64px;scroll-margin-top:24px}
h2{font-family:var(--display);font-size:48px;line-height:1.15;letter-spacing:-1.44px;font-weight:400;margin:0 0 8px}
h3{font-family:var(--display);font-size:24px;line-height:1.15;letter-spacing:-.72px;font-weight:400;margin:40px 0 12px}
h4{font-family:var(--display);font-size:18px;line-height:1.35;font-weight:600;margin:24px 0 8px}
p{margin:0 0 14px;color:var(--slate)}
.lead{font-size:18px;line-height:1.45;color:var(--ink)}
.card{background:#fff;border-radius:16px;box-shadow:0 0 0 1px var(--ring);padding:24px;margin:16px 0}
.shot{background:#fff;border-radius:16px;box-shadow:0 0 0 1px var(--ring);padding:12px;margin:16px 0;overflow:auto}
.shot svg{display:block;width:100%;height:auto;max-width:100%}
.shot .cap{display:flex;justify-content:space-between;align-items:center;gap:12px;padding:8px 8px 2px;color:var(--ph);font-size:12px}
.shot .cap a{font-size:12px}
table{border-collapse:collapse;width:100%;font-size:14px;margin:12px 0 20px}
th{text-align:left;font-family:var(--display);font-weight:600;font-size:12px;letter-spacing:.4px;text-transform:uppercase;color:var(--slate);border-bottom:1px solid var(--border);padding:8px 10px}
td{border-bottom:1px solid var(--line);padding:9px 10px;vertical-align:top;color:var(--ink)}
tbody tr:nth-child(even){background:var(--sunken)}
code,.mono{font-family:var(--mono);font-size:12.5px;background:var(--sunken);padding:1px 5px;border-radius:5px;color:var(--ink)}
ul{margin:0 0 14px;padding-left:20px;color:var(--slate)}
li{margin:4px 0}
.swatches{display:grid;grid-template-columns:repeat(auto-fill,minmax(168px,1fr));gap:12px;margin:16px 0}
.sw{border-radius:12px;box-shadow:0 0 0 1px var(--ring);padding:14px;min-height:84px;display:flex;flex-direction:column;justify-content:flex-end}
.sw b{font-family:var(--display);font-size:14px;font-weight:600}
.sw span{font-size:11px;opacity:.78;font-family:var(--mono)}
.grid2{display:grid;grid-template-columns:1fr 1fr;gap:16px}
.grid3{display:grid;grid-template-columns:repeat(3,1fr);gap:16px}
.note{border-radius:12px;padding:16px 18px;background:var(--teal);color:var(--teal-dk);margin:16px 0;font-size:15px}
.note.warn{background:var(--coral);color:var(--coral-dk)}
.note.plain{background:var(--sunken);color:var(--slate)}
.kbd{font-family:var(--mono);font-size:12px;border:1px solid var(--border);border-bottom-width:2px;border-radius:6px;padding:1px 6px;background:#fff}
footer{border-top:1px solid var(--line);padding:32px 48px;color:var(--ph);font-size:13px;background:var(--bar)}
@media (max-width:1024px){.layout{grid-template-columns:1fr}.toc{position:static;max-height:none;border-right:none;border-bottom:1px solid var(--line)}main{padding:28px 20px 64px}.hero{padding:36px 20px 28px}.hero h1{font-size:38px}h2{font-size:32px}}
@media print{.toc{display:none}.layout{grid-template-columns:1fr}}
`

const shell = (title, sub, chips, toc, body, footerNote) => `<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>${title}</title>
<style>${CSS}</style>
</head>
<body>
<header class="hero">
  <h1>${title}</h1>
  <p class="sub">${sub}</p>
  <div class="meta">${chips.map((c) => `<span class="chip ${c[1] || ''}">${c[0]}</span>`).join('')}</div>
</header>
<div class="layout">
  <nav class="toc">${toc}</nav>
  <main>${body}</main>
</div>
<footer>${footerNote}</footer>
</body>
</html>
`

const tocList = (items) => items.map((i) => (i[2] ? `<a class="sub" href="#${i[0]}">${i[1]}</a>` : `<a href="#${i[0]}">${i[1]}</a>`)).join('\n')

const shot = (file, title, note, id) => `
<section id="${id}">
  <h3>${title}</h3>
  ${note ? `<p class="lead">${note}</p>` : ''}
  <div class="shot">
    ${readSvg(file)}
    <div class="cap"><span>${file}</span><a href="./svg/${file}" download>下载 SVG</a></div>
  </div>
  ${pages.find((p) => p.file === file)?.notes?.length ? '<ul>' + pages.find((p) => p.file === file).notes.map((n) => `<li>${n}</li>`).join('') + '</ul>' : ''}
</section>`

/* ============================ 原型图册 ============================ */
{
  const toc = [
    ['design-tokens', '设计令牌'],
    ['ia', '信息架构'],
    ['shell', '全局骨架'],
    ['pages', '页面原型'],
    ['p02', 'P-2 文件树'], ['p03', 'P-3 文档工作区'], ['p04', 'P-4 跨格式引用条'],
    ['p05', 'P-5 搜索中心'], ['p06', 'P-6 双链与反链'], ['p07', 'P-7 关系图谱'],
    ['p08', 'P-8 Git 时间线'], ['p09', 'P-9 命令面板'], ['p10', 'P-10 设置'],
    ['p11', 'P-11 降级与异常态'],
    ['states', '页面状态矩阵'],
    ['flows', '交互流程'],
    ['visual', '视觉规范'],
    ['shortcuts', '快捷键'],
  ].map((i) => [i[0], i[1], i[2]])

  const sw = (bg, name, hex, fg) => `<div class="sw" style="background:${bg};color:${fg || '#1c1c1e'}"><b>${name}</b><span>${hex}</span></div>`

  const body = `
<section id="design-tokens">
  <h2>设计令牌</h2>
  <p class="lead">Miro 设计规范落地：白色画布 + 近黑文本 + 柔和 pastel 强调 + Blue 450 交互色。规则：每屏最多 2 种 pastel 强调；状态色与品牌 pastel 不混用。</p>
  <div class="swatches">
    ${sw('#1c1c1e', 'Near Black', '#1c1c1e', '#fff')}
    ${sw('#555a6a', 'Slate', '#555a6a', '#fff')}
    ${sw('#ffffff', 'White', '#ffffff', '#1c1c1e')}
    ${sw('#5b76fe', 'Blue 450', '#5b76fe', '#fff')}
    ${sw('#2a41b6', 'Actionable Pressed', '#2a41b6', '#fff')}
    ${sw('#00b473', 'Success', '#00b473', '#fff')}
    ${sw('#c7cad5', 'Border', '#c7cad5', '#1c1c1e')}
    ${sw('#ffc6c6', 'Coral', '#ffc6c6', '#600000')}
    ${sw('#c3faf5', 'Teal / Moss', '#c3faf5', '#187574')}
    ${sw('#ffd8f4', 'Rose', '#ffd8f4', '#1c1c1e')}
    ${sw('#ffe6cd', 'Orange', '#ffe6cd', '#1c1c1e')}
    ${sw('#fde0f0', 'Pink', '#fde0f0', '#1c1c1e')}
    ${sw('#fbd4d4', 'Red', '#fbd4d4', '#8a2020')}
  </div>
</section>

<section id="ia">
  <h2>信息架构</h2>
  <p class="lead">单窗口六视图：工作台（默认）、搜索中心、关系图谱、版本时间线，叠加命令面板与设置。</p>
  <div class="shot">${readSvg('fig-01-flow.svg')}<div class="cap"><span>fig-01-flow.svg</span><a href="./svg/fig-01-flow.svg" download>下载 SVG</a></div></div>
</section>

<section id="shell">
  <h2>全局骨架</h2>
  <p class="lead">标题栏常驻分支与提交状态；左栏 240–320px；中栏标签页 + 文档区；右栏 280–360px 随选中项联动；状态栏常驻索引 / Git / 插件 / 扫描四类状态。</p>
  ${shot('p01-workbench.svg', 'P-1 工作台（默认首屏）', '一屏完成「找文件 → 看内容 → 追关系 → 存版本」。', 'p01')}
</section>

<section id="pages">
  <h2>页面原型</h2>
  <p class="lead">以下 10 个页面与主工作台共同覆盖需求 §3 P0/P1 全部能力面。</p>
</section>
${shot('p02-file-tree.svg', 'P-2 文件树侧栏', '统一知识入口：所有格式在同一个树里，拖拽即生成引用。', 'p02')}
${shot('p03-document-workspace.svg', 'P-3 文档工作区（预览 = 编辑同页）', '统一容器按格式分发渲染器；能力不足即降级，布局不变。', 'p03')}
${shot('p04-reference-bar.svg', 'P-4 跨格式引用条', '引用是动态的：源变更即标记，可刷新 / 看差异 / 重定位。', 'p04')}
${shot('p05-search.svg', 'P-5 搜索中心', '跨格式全文检索 + 上下文片段 + 多维过滤，点击命中跳转原文精确位置。', 'p05')}
${shot('p06-backlinks.svg', 'P-6 内联双链与反链面板', '双链语法、反链条目与失效分组；搜到 → 跳转 → 看反链 → 顺藤摸瓜。', 'p06')}
${shot('p07-graph.svg', 'P-7 关系图谱视图', '节点=文档，边=引用关系；默认 2 度邻域保护渲染性能。', 'p07')}
${shot('p08-git-timeline.svg', 'P-8 Git 时间线与 diff', '自动快照与手动提交共用时间线；二进制给结构化 diff；回滚生成新提交。', 'p08')}
${shot('p09-command-palette.svg', 'P-9 命令面板', '统一入口：文件 / 命令 / 插件 / 设置项四类候选聚合。', 'p09')}
${shot('p10-settings.svg', 'P-10 设置（插件与格式能力）', '插件清单即能力清单；未安装与实验性插件显式标注。', 'p10')}
${shot('p11-degraded-states.svg', 'P-11 降级与异常态', '降级不阻断：占位卡保留引用与反链链路，只读提供「另存副本」。', 'p11')}

<section id="states">
  <h2>页面状态矩阵</h2>
  <table>
    <thead><tr><th>页面</th><th>空态</th><th>加载态</th><th>错误态</th><th>只读 / 降级</th></tr></thead>
    <tbody>
      <tr><td>工作台</td><td>未打开知识库 → 选择文件夹 / 新建</td><td>骨架屏（文件树 + 文档区）</td><td>库路径不可读</td><td>插件缺失 → 占位卡</td></tr>
      <tr><td>文件树</td><td>无文件 → 新建 / 导入</td><td>懒加载子树</td><td>目录读取失败</td><td>部分子目录不可读</td></tr>
      <tr><td>文档区</td><td>未选择文档 → 引导</td><td>渲染骨架</td><td>解析失败 → 降级为纯文本</td><td>只读横幅</td></tr>
      <tr><td>搜索</td><td>无查询词 → 提示与最近搜索</td><td>结果流式追加</td><td>索引不可用 → 重建引导</td><td>部分格式未索引（明示覆盖范围）</td></tr>
      <tr><td>反链</td><td>无引用 → 尚无双链</td><td>—</td><td>关系数据损坏 → 重建索引</td><td>失效链接单独归组</td></tr>
      <tr><td>图谱</td><td>无关系 → 引导创建引用</td><td>渐进渲染</td><td>规模超限 → 降深度</td><td>过滤后为空</td></tr>
      <tr><td>时间线</td><td>非 Git 仓库 → 初始化引导</td><td>载入提交中</td><td>Git 冲突 / 锁</td><td>无 remote 时仅本地</td></tr>
    </tbody>
  </table>
</section>

<section id="flows">
  <h2>交互流程</h2>
  <p class="lead">五条主干流程：首次打开、编辑保存、搜索跳转、插入引用、Git 提交回滚。</p>
  ${['fig-02-seq', 'fig-03-seq', 'fig-04-seq', 'fig-05-seq', 'fig-06-seq'].map((id) => {
    const f = figById(id)
    return `<div class="shot">${readSvg(f.file)}<div class="cap"><span>${f.file} · ${f.heading}</span><a href="./svg/${f.file}" download>下载 SVG</a></div></div>`
  }).join('\n')}
</section>

<section id="visual">
  <h2>视觉规范</h2>
  <div class="grid2">
    <div class="card">
      <h4>字体层级</h4>
      <table>
        <thead><tr><th>角色</th><th>字体</th><th>字号 / 字距</th></tr></thead>
        <tbody>
          <tr><td>Hero</td><td>Roobert PRO Medium</td><td>56px / -1.68px</td></tr>
          <tr><td>区块标题</td><td>Roobert PRO Medium</td><td>48px / -1.44px</td></tr>
          <tr><td>卡片标题</td><td>Roobert PRO Medium</td><td>24px / -0.72px</td></tr>
          <tr><td>子标题</td><td>Noto Sans</td><td>22px / -0.44px</td></tr>
          <tr><td>正文</td><td>Noto Sans</td><td>18px / 1.45</td></tr>
          <tr><td>正文标准</td><td>Noto Sans</td><td>16px / -0.16px</td></tr>
          <tr><td>按钮</td><td>Roobert PRO Medium 700</td><td>17.5px / 0.175px</td></tr>
          <tr><td>说明 / 小字</td><td>Roobert PRO Medium</td><td>14px / 12px</td></tr>
          <tr><td>微标（胶囊）</td><td>Roobert PRO</td><td>10.5px / uppercase</td></tr>
        </tbody>
      </table>
    </div>
    <div class="card">
      <h4>组件与尺度</h4>
      <table>
        <thead><tr><th>组件</th><th>规格</th></tr></thead>
        <tbody>
          <tr><td>主按钮</td><td>蓝底 #5b76fe、白字、8px 圆角、7px 12px 内边距</td></tr>
          <tr><td>次按钮</td><td>透明底、1px #c7cad5、8px 圆角</td></tr>
          <tr><td>圆形图标按钮</td><td>50% 圆角、白底 + 轻阴影、32–40px</td></tr>
          <tr><td>卡片</td><td>12–24px 圆角、pastel 底、ring shadow 无重投影</td></tr>
          <tr><td>输入框</td><td>白底、1px #e9eaef、8px 圆角、16px 内边距</td></tr>
          <tr><td>状态条</td><td>10px 圆角 + 4px 左色条</td></tr>
          <tr><td>间距</td><td>1 / 2 / 4 / 8 / 12 / 16 / 20 / 24 px</td></tr>
          <tr><td>断点</td><td>425 / 576 / 768 / 896 / 1024 / 1200 / 1280 / 1366 / 1700 / 1920</td></tr>
        </tbody>
      </table>
    </div>
  </div>
</section>

<section id="shortcuts">
  <h2>快捷键</h2>
  <table>
    <thead><tr><th>域</th><th>快捷键</th><th>动作</th></tr></thead>
    <tbody>
      <tr><td>全局</td><td><span class="kbd">Ctrl+K</span></td><td>命令面板</td></tr>
      <tr><td>全局</td><td><span class="kbd">Ctrl+Shift+F</span></td><td>搜索中心</td></tr>
      <tr><td>全局</td><td><span class="kbd">Ctrl+P</span></td><td>快速打开文件</td></tr>
      <tr><td>全局</td><td><span class="kbd">Ctrl+S</span></td><td>保存 /（工作区）打开提交面板</td></tr>
      <tr><td>全局</td><td><span class="kbd">Ctrl+B</span> / <span class="kbd">Ctrl+J</span></td><td>折叠左栏 / 右栏</td></tr>
      <tr><td>全局</td><td><span class="kbd">Ctrl+Shift+G</span></td><td>关系图谱</td></tr>
      <tr><td>文件</td><td><span class="kbd">Ctrl+N</span></td><td>新建文档</td></tr>
      <tr><td>编辑</td><td><span class="kbd">Ctrl+Shift+R</span></td><td>插入引用</td></tr>
      <tr><td>编辑</td><td><span class="kbd">[[</span></td><td>触发双链补全</td></tr>
      <tr><td>导航</td><td><span class="kbd">Alt+↑/↓</span></td><td>上 / 下一个命中</td></tr>
    </tbody>
  </table>
</section>
`

  const html = shell(
    'MKV 原型图册',
    'Migrant Knowledge Vault · 低保真交互原型 + Miro 视觉规范。覆盖信息架构、11 个页面线框、页面状态矩阵、交互流程与快捷键。',
    [['Prototype v1.0', 'acc'], ['2026-11-10'], ['Desktop 优先 · Windows'], ['离线可看 · 自包含 HTML', 'tl'],
      ['源文档：需求描述 v1.0 + 架构设计 v1.0']],
    `<h4>目录</h4>${tocList(toc)}`,
    body,
    'MKV 原型图册 v1.0 · 产物：<code>docs/design/prototype.html</code> 与 <code>docs/design/svg/p*.svg</code> · 详细设计见 <a href="./detailed-design.html">detailed-design.html</a>'
  )
  fs.writeFileSync(path.join(OUT, 'prototype.html'), html)
  console.log('prototype.html written')
}

/* ============================ 详细设计图册 ============================ */
{
  const toc = [
    ['context', '系统上下文'], ['layer', '分层与进程架构'], ['hostclient', 'Cordis Host / Client'],
    ['plugins', '插件与能力矩阵'], ['lifecycle', '插件生命周期'],
    ['sequences', '关键时序'], ['domain', '领域模型与索引表'], ['states', '状态机'],
    ['extract', '索引与检索'], ['links', '双链与跨格式引用'], ['git', 'Git 集成'],
    ['security', '插件运行时与安全'], ['nfr', '性能预算与可观测性'], ['risks', '风险与待决项'],
  ]

  const body = `
<section id="context">
  <h2>系统上下文</h2>
  <p class="lead">实线 = 必需依赖；虚线 = 可选或降级路径。出网仅允许「插件更新检查」与「可选 Git remote」两条，其余一律本地。</p>
  <div class="shot">${readSvg('fig-07-flow.svg')}<div class="cap"><span>fig-07-flow.svg</span><a href="./svg/fig-07-flow.svg" download>下载 SVG</a></div></div>
</section>

<section id="layer">
  <h2>分层与进程架构</h2>
  <p class="lead">依赖方向铁律：UI → Cordis → 插件 → Tauri IPC → Rust 壳；反向只允许事件。Rust 壳是唯一持有系统权限的一层。</p>
  <div class="shot">${readSvg('fig-08-flow.svg')}<div class="cap"><span>fig-08-flow.svg</span><a href="./svg/fig-08-flow.svg" download>下载 SVG</a></div></div>
  <div class="grid3">
    <div class="card"><h4>WebView 渲染进程</h4><p>半可信：UI + Cordis + 插件。只能通过 IPC 调用系统能力，不得直接触碰文件系统、启动进程或联网。</p></div>
    <div class="card"><h4>Rust 壳（Tauri 2）</h4><p>完全可信：受限 fs、notify 监听、子进程、SQLite FTS5、Git。所有路径经工作区作用域校验。</p></div>
    <div class="card"><h4>进程外组件</h4><p>OnlyOffice DS：<code>127.0.0.1:port</code> + JWT 回调；崩溃自动重启，连续失败降级为外部唤起。</p></div>
  </div>
</section>

<section id="hostclient">
  <h2>Cordis Host / Client 拆分</h2>
  <p class="lead">两侧各自 <code>declare module 'cordis'</code> 扩展同一 Context 接口，避免类型冲突。</p>
  <div class="shot">${readSvg('fig-09-flow.svg')}<div class="cap"><span>fig-09-flow.svg</span><a href="./svg/fig-09-flow.svg" download>下载 SVG</a></div></div>
</section>

<section id="plugins">
  <h2>插件与能力矩阵</h2>
  <p class="lead">「标记【补充】的插件用于补齐需求 P0/P1 缺口（Git、双链、抽取、轻量编辑引擎）」。引擎按 <code>capability</code> 分级注册，UI 据此决定可编辑 / 只读 / 外跳。</p>
  <table>
    <thead><tr><th>插件</th><th>格式</th><th>能力</th><th>依赖命令</th><th>来源</th></tr></thead>
    <tbody>
      <tr><td><code>core:repo</code></td><td>全部</td><td>文件树 / 监听 / 增删改</td><td><code>fs_*</code> <code>watch_*</code></td><td>文档</td></tr>
      <tr><td><code>core:engine-pdf</code></td><td>.pdf</td><td>view</td><td><code>fs_read</code></td><td>文档</td></tr>
      <tr><td><code>core:engine-block</code></td><td>.docxxml</td><td>edit-native</td><td><code>fs_read/write</code></td><td>文档</td></tr>
      <tr><td><code>core:engine-base</code></td><td>.base</td><td>edit-native / view</td><td><code>fs_*</code></td><td>文档</td></tr>
      <tr><td><code>core:engine-onlyoffice</code></td><td>.docx/.xlsx/.pptx</td><td>edit-adapter</td><td><code>sidecar_*</code></td><td>文档</td></tr>
      <tr><td><code>core:engine-deck / word / grid / md</code></td><td>.pptx/.docx/.xlsx/.md</td><td>edit-native / view</td><td><code>fs_*</code></td><td>补充</td></tr>
      <tr><td><code>core:extract-drawio / visio</code></td><td>.drawio/.vsdx</td><td>extract（只读）</td><td><code>fs_read</code></td><td>补充</td></tr>
      <tr><td><code>core:search</code></td><td>全部</td><td>检索</td><td><code>search_*</code></td><td>文档</td></tr>
      <tr><td><code>core:link</code></td><td>md 为主</td><td>双链 / 反链 / 图谱</td><td><code>search_*</code> <code>fs_read</code></td><td>补充</td></tr>
      <tr><td><code>core:git</code></td><td>全部</td><td>版本</td><td><code>git_*</code></td><td>补充</td></tr>
      <tr><td><code>core:ui-shell</code></td><td>—</td><td>设置 / 主题 / 命令面板</td><td><code>settings_*</code></td><td>文档</td></tr>
    </tbody>
  </table>
  <h4>格式 × 能力矩阵</h4>
  <table>
    <thead><tr><th>格式</th><th>预览</th><th>编辑</th><th>引用粒度</th><th>主引擎</th><th>降级链</th></tr></thead>
    <tbody>
      <tr><td>.xlsx</td><td>表格</td><td>单元格</td><td>单元格 / 区域</td><td>engine-grid</td><td>onlyoffice → 外部唤起</td></tr>
      <tr><td>.pptx</td><td>幻灯片</td><td>页 / 元素</td><td>页级</td><td>engine-deck</td><td>onlyoffice → 外部唤起</td></tr>
      <tr><td>.docx</td><td>文档</td><td>章节 / 段落</td><td>章节 / 段落级</td><td>engine-word</td><td>onlyoffice → 外部唤起</td></tr>
      <tr><td>.docxxml</td><td>原生</td><td>块级</td><td>块级</td><td>engine-block</td><td>纯文本只读</td></tr>
      <tr><td>.base</td><td>原生</td><td>记录 / 块</td><td>记录 / 块级</td><td>engine-base</td><td>xlsx 互通导出</td></tr>
      <tr><td>.pdf</td><td>原生</td><td>只读</td><td>页级</td><td>engine-pdf</td><td>—</td></tr>
      <tr><td>.drawio</td><td>只读</td><td>—</td><td>节点级</td><td>extract-drawio</td><td>—</td></tr>
      <tr><td>.vsdx</td><td>只读</td><td>—</td><td>节点级</td><td>extract-visio</td><td>—</td></tr>
      <tr><td>.md</td><td>原生</td><td>块级</td><td>标题 / 代码块 / 列表项</td><td>engine-md</td><td>纯文本只读</td></tr>
    </tbody>
  </table>
</section>

<section id="lifecycle">
  <h2>插件生命周期</h2>
  <p class="lead">单个插件崩溃不得影响宿主与其他插件；<code>register()</code> 返回 Disposable 供卸载时释放订阅与命令。</p>
  <div class="shot">${readSvg('fig-10-state.svg')}<div class="cap"><span>fig-10-state.svg</span><a href="./svg/fig-10-state.svg" download>下载 SVG</a></div></div>
</section>

<section id="sequences">
  <h2>关键时序</h2>
  ${['fig-11-seq', 'fig-12-seq', 'fig-13-seq', 'fig-14-seq', 'fig-15-seq', 'fig-16-seq'].map((id) => {
    const f = figById(id)
    return `<div class="shot">${readSvg(f.file)}<div class="cap"><span>${f.file} · ${f.heading}</span><a href="./svg/${f.file}" download>下载 SVG</a></div></div>`
  }).join('\n')}
</section>

<section id="domain">
  <h2>领域模型与索引表</h2>
  <div class="shot">${readSvg('fig-17-er.svg')}<div class="cap"><span>fig-17-er.svg</span><a href="./svg/fig-17-er.svg" download>下载 SVG</a></div></div>
  <table>
    <thead><tr><th>实体</th><th>关键字段</th><th>说明</th></tr></thead>
    <tbody>
      <tr><td>Document</td><td>path(PK)、format、title、size、mtime、git_status、extractor_version</td><td>文件本体元数据</td></tr>
      <tr><td>Anchor</td><td>doc_path、anchor_id、kind、locator、snippet、bbox</td><td>文档内可定位位置</td></tr>
      <tr><td>Link</td><td>src_path、dst_path、anchor_id、syntax、context、updated_at</td><td>出链记录</td></tr>
      <tr><td>Backlink</td><td>dst_path、src_path、anchor_id、status(live/broken/stale)</td><td>反链（由 Link 派生）</td></tr>
      <tr><td>Extract</td><td>doc_path、title、body、format、tags、mtime、anchors_json</td><td>FTS5 索引行</td></tr>
      <tr><td>Commit</td><td>hash、author、date、message、snapshot_type</td><td>Git 时间线</td></tr>
      <tr><td>EditorSession</td><td>session_id、doc_path、engine_id、capability、dirty</td><td>运行时会话（内存态）</td></tr>
      <tr><td>PluginRecord</td><td>id、version、state、formats[]、permissions[]、checksum</td><td>插件清单与状态</td></tr>
    </tbody>
  </table>
  <div class="note plain"><b>索引落位（双层策略）</b>：<code>.index/</code> 内保存 JSONL 纯文本快照（Git 友好、可 diff、可重建），SQLite FTS5 作为运行时加速缓存置于 app data，随时可由快照重建。需求与架构在此处存在冲突，见待决项 D-2。</div>
</section>

<section id="states">
  <h2>状态机</h2>
  <h3>文档会话</h3>
  <div class="shot">${readSvg('fig-18-state.svg')}<div class="cap"><span>fig-18-state.svg</span><a href="./svg/fig-18-state.svg" download>下载 SVG</a></div></div>
  <h3>索引</h3>
  <div class="shot">${readSvg('fig-19-state.svg')}<div class="cap"><span>fig-19-state.svg</span><a href="./svg/fig-19-state.svg" download>下载 SVG</a></div></div>
  <h3>引用</h3>
  <div class="shot">${readSvg('fig-20-state.svg')}<div class="cap"><span>fig-20-state.svg</span><a href="./svg/fig-20-state.svg" download>下载 SVG</a></div></div>
</section>

<section id="extract">
  <h2>索引与检索</h2>
  <p class="lead">抽取器负责产出纯文本与位置锚点；索引行内保存正文分段与锚点对应关系，命中后可反查「命中所在段 → anchor」，实现跳转原文精确位置。</p>
  <div class="shot">${readSvg('fig-21-flow.svg')}<div class="cap"><span>fig-21-flow.svg</span><a href="./svg/fig-21-flow.svg" download>下载 SVG</a></div></div>
  <table>
    <thead><tr><th>指标</th><th>目标（需求 §6）</th><th>设计手段</th></tr></thead>
    <tbody>
      <tr><td>1 万文档全文检索</td><td>&lt; 500ms</td><td>FTS5 前缀索引 + 过滤下推 + 分页；上下文用 snippet() 截取</td></tr>
      <tr><td>冷启动</td><td>&lt; 2s</td><td>首屏只加载文件树与最近打开；索引后台增量推进；引擎按需装载</td></tr>
      <tr><td>定时扫描磁盘</td><td>单独进程</td><td>低优先级独立扫描器 + 节流 + 哈希去重</td></tr>
      <tr><td>索引可重建</td><td>必须</td><td><code>.index/</code> 快照 + 一键重建，步骤可审计</td></tr>
    </tbody>
  </table>
</section>

<section id="links">
  <h2>双链与跨格式引用</h2>
  <p class="lead">双源触发：写入期（引擎保存上报，增量准确）+ 扫描期（外部工具改动兜底）；以 mtime + hash 幂等，冲突时以扫描期为准。</p>
  <div class="shot">${readSvg('fig-22-flow.svg')}<div class="cap"><span>fig-22-flow.svg</span><a href="./svg/fig-22-flow.svg" download>下载 SVG</a></div></div>
  <table>
    <thead><tr><th>语法</th><th>示例</th><th>解析结果</th></tr></thead>
    <tbody>
      <tr><td>Wiki 链接</td><td><code>[[方案.pptx#p:7]]</code></td><td>出链(wikilink) + 锚点 p:7</td></tr>
      <tr><td>标题双链</td><td><code>[[周报#结论]]</code></td><td>出链 + 锚点 heading:结论</td></tr>
      <tr><td>@ 引用</td><td><code>@台账.xlsx#Sheet1!B2</code></td><td>出链(at) + 单元格锚点</td></tr>
      <tr><td>嵌入引用</td><td>引擎生成的引用块</td><td>出链(embed) + 快照内容</td></tr>
    </tbody>
  </table>
</section>

<section id="git">
  <h2>Git 集成</h2>
  <table>
    <thead><tr><th>能力</th><th>实现</th><th>策略</th></tr></thead>
    <tbody>
      <tr><td>状态</td><td><code>git_status</code></td><td>仅工作区作用域，默认忽略 .index/</td></tr>
      <tr><td>提交</td><td><code>git_commit</code></td><td>文档区 Ctrl+S = 保存；工作区 = 提交面板（见 D-8）</td></tr>
      <tr><td>自动快照</td><td>调度器 + 空闲检测</td><td>默认 10 分钟或空闲 60s；跳过仅索引变动</td></tr>
      <tr><td>日志</td><td><code>git_log</code> + commits 缓存</td><td>首次全量，之后增量</td></tr>
      <tr><td>diff</td><td><code>git_diff</code></td><td>文本逐行；二进制给结构化摘要</td></tr>
      <tr><td>回滚</td><td><code>git_restore</code></td><td>生成新提交而非 reset，历史不丢</td></tr>
      <tr><td>远端</td><td>可选</td><td>默认无 remote，离线可用</td></tr>
    </tbody>
  </table>
</section>

<section id="security">
  <h2>插件运行时与安全</h2>
  <table>
    <thead><tr><th>项</th><th>要求</th></tr></thead>
    <tbody>
      <tr><td>路径</td><td>所有 fs 命令经 <code>normalize + starts_with(workspace)</code> 校验；拒绝 <code>..</code>、符号链接越界、UNC 越权，越界返回 <code>E_SCOPE</code></td></tr>
      <tr><td>静态服务</td><td>插件内置 web 包走 <code>127.0.0.1</code> 微型静态服务，必须带一次性 token，并做路径穿越防护</td></tr>
      <tr><td>权限声明</td><td>清单声明 <code>fs:read / fs:write / net / proc</code>，安装时展示并由用户确认，<code>net</code> 默认拒绝</td></tr>
      <tr><td>隔离</td><td>插件异常不扩散；崩溃写隔离日志；sidecar 崩溃自动重启</td></tr>
      <tr><td>数据外流</td><td>无出网路径；未来开启也仅限插件更新检查与可选 Git remote</td></tr>
      <tr><td>供应链</td><td>记录来源、版本、SHA-256；未知来源插件显著提示</td></tr>
    </tbody>
  </table>
  <h4>失败与降级矩阵</h4>
  <table>
    <thead><tr><th>故障</th><th>检测</th><th>降级</th><th>用户可见</th></tr></thead>
    <tbody>
      <tr><td>插件未安装</td><td>注册表无匹配</td><td>外部唤起 / 占位卡</td><td>占位卡 + 安装按钮</td></tr>
      <tr><td>插件装载异常</td><td>plugin/state-changed</td><td>隔离该引擎</td><td>占位卡 + 日志</td></tr>
      <tr><td>sidecar 崩溃</td><td>健康检查失败</td><td>重启 3 次后转外跳</td><td>状态栏警示</td></tr>
      <tr><td>抽取失败</td><td>extract 抛错</td><td>仅入库路径与标题</td><td>标注「正文未索引」</td></tr>
      <tr><td>索引损坏</td><td>校验失败</td><td>一键重建</td><td>搜索页错误态</td></tr>
      <tr><td>Git 冲突</td><td>git_status 报冲突</td><td>暂停自动快照</td><td>标题栏冲突徽标</td></tr>
    </tbody>
  </table>
</section>

<section id="nfr">
  <h2>性能预算与可观测性</h2>
  <table>
    <thead><tr><th>需求</th><th>设计位置</th><th>验收方式</th></tr></thead>
    <tbody>
      <tr><td>本地优先（离线可用、不降级）</td><td>系统上下文 / 安全</td><td>断网启动与全流程走查</td></tr>
      <tr><td>文件优先（原生文件可被外部工具打开）</td><td>格式数据流</td><td>外部 Office / 文本工具直接打开库内文件</td></tr>
      <tr><td>Git 优先（索引与本体分离、可重建）</td><td>索引表 / Git 集成</td><td>删除 .index/ 后一键重建成功</td></tr>
      <tr><td>P0 全文检索 + 命中跳转</td><td>抽取管线 / SearchService</td><td>1 万文档 &lt; 500ms；点击命中定位到锚点</td></tr>
      <tr><td>P0 双链 + 反链 + 图谱</td><td>core:link / LinkService</td><td>建链→反链出现→图谱呈现→失效标记</td></tr>
      <tr><td>P0 Git 版本</td><td>core:git / git_*</td><td>自动快照 / 手动提交 / diff / 回滚四步闭环</td></tr>
      <tr><td>可观测</td><td>事件总线 / 降级矩阵</td><td>索引重建与 Git 操作均有进度与失败提示</td></tr>
      <tr><td>安全</td><td>路径沙箱</td><td>路径穿越用例全部被 E_SCOPE 拒绝</td></tr>
    </tbody>
  </table>
</section>

<section id="risks">
  <h2>风险与待决项</h2>
  <p class="lead">以下 11 项为两份输入文档之间（或与工程现状之间）的不一致，需评审后落 ADR。</p>
  <table>
    <thead><tr><th>#</th><th>问题</th><th>影响</th><th>建议</th><th>建议 ADR</th></tr></thead>
    <tbody>
      <tr><td>D-1</td><td>智能文档扩展名不一致：需求 <code>.docxxml</code> vs 架构 <code>.fdoc</code></td><td>类型契约与引擎注册错位</td><td>统一为 .docxxml，.fdoc 作兼容别名</td><td>ADR-0004</td></tr>
      <tr><td>D-2</td><td>索引位置冲突：需求要求随库存于 .index/，架构要求存 app data</td><td>可移植性与性能不可兼得</td><td>双层策略：JSONL 快照 + SQLite 缓存</td><td>ADR-0005</td></tr>
      <tr><td>D-3</td><td>格式数量不一致：P0 写「八种」，矩阵仅 5 行</td><td>验收口径模糊</td><td>以本册能力矩阵为唯一口径</td><td>ADR-0004</td></tr>
      <tr><td>D-4</td><td>Git 能力未出现在架构插件清单与命令组</td><td>P0 无法验收</td><td>新增 core:git + git_* 命令组</td><td>ADR-0006</td></tr>
      <tr><td>D-5</td><td>双链 / 反链 / 图谱 / 文本抽取无对应插件与契约</td><td>P0 双链不可交付</td><td>新增 core:link 与 ExtractorEngine 契约</td><td>ADR-0006</td></tr>
      <tr><td>D-6</td><td>插件形态未定：monorepo 内置包 vs 运行时动态库</td><td>生态与安装包体积</td><td>官方内置 monorepo，第三方走本地动态库 + 安装包</td><td>ADR-0007</td></tr>
      <tr><td>D-7</td><td>OnlyOffice：AGPL v3 + 安装包 +1~2GB</td><td>分发与体验成本</td><td>默认用轻量 DTO 引擎覆盖主场景，OnlyOffice 设为可选</td><td>ADR-0008</td></tr>
      <tr><td>D-8</td><td>Ctrl+S 语义冲突：保存文档 vs 手动提交</td><td>心智冲突、易误提交</td><td>文档区 Ctrl+S = 保存；提交走标题栏 / Ctrl+Shift+S</td><td>ADR-0009</td></tr>
      <tr><td>D-9</td><td>CI 门禁仍指向 Rust workspace（packages/core），仓库已无 Rust 源码</td><td>CI 必红</td><td>重写为 Node 侧 lint + test + build 门禁</td><td>ADR-0010</td></tr>
      <tr><td>D-10</td><td>需求要求 Windows，CI 仅 ubuntu</td><td>平台专属问题无门禁</td><td>增加 windows-latest 构建 + 冒烟作业</td><td>ADR-0010</td></tr>
      <tr><td>D-11</td><td>P0「八种格式原地预览」与「宿主零内置解析」并存</td><td>未装插件即无法预览</td><td>由只读抽取适配器兜底出纯文本预览</td><td>ADR-0007</td></tr>
    </tbody>
  </table>
  <div class="note warn"><b>说明</b>：本册未创建 ADR 文件，仅给出建议编号；采纳后按项目 ADR 模板在 <code>docs/adr/</code> 逐条落盘。</div>
</section>
`

  const html = shell(
    'MKV 详细设计图册',
    'Migrant Knowledge Vault · 系统上下文、分层架构、插件与能力矩阵、关键时序、领域模型、状态机、接口契约与安全边界、待决项。',
    [['Detailed Design v1.0', 'acc'], ['2026-11-10'], ['Cordis + Tauri 2 + React', 'tl'], ['自包含 HTML · 离线可看'],
      ['源文档：需求描述 v1.0 + 架构设计 v1.0']],
    `<h4>目录</h4>${tocList(toc)}`,
    body,
    'MKV 详细设计图册 v1.0 · 产物：<code>docs/design/detailed-design.html</code> 与 <code>docs/design/svg/fig-*.svg</code> · 原型见 <a href="./prototype.html">prototype.html</a>'
  )
  fs.writeFileSync(path.join(OUT, 'detailed-design.html'), html)
  console.log('detailed-design.html written')
}

/* ============================ 索引页 ============================ */
{
  const all = [
    ...pages.map((p) => ({ file: p.file, title: p.title, kind: '原型线框' })),
    ...figs.map((f) => ({ file: f.file, title: f.heading, kind: '设计图' })),
  ]
  const body = `
<section>
  <h2>产物清单</h2>
  <p class="lead">共 ${all.length} 张图（${pages.length} 张原型线框 + ${figs.length} 张设计图），均提供独立 SVG 与内嵌于 HTML 图册的版本。</p>
  <table>
    <thead><tr><th>文件</th><th>标题</th><th>类别</th><th>预览</th></tr></thead>
    <tbody>
      ${all.map((a) => `<tr><td><a href="./svg/${a.file}">${a.file}</a></td><td>${a.title}</td><td>${a.kind}</td><td><a href="./svg/${a.file}" target="_blank">打开 SVG</a></td></tr>`).join('\n')}
    </tbody>
  </table>
</section>
<section>
  <h2>配套文档</h2>
  <ul>
    <li><a href="./prototype.html">prototype.html</a> —— 原型图册（信息架构 / 11 页线框 / 状态矩阵 / 交互流程 / 视觉规范）</li>
    <li><a href="./detailed-design.html">detailed-design.html</a> —— 详细设计图册（架构 / 时序 / 模型 / 状态机 / 契约 / 待决项）</li>
    <li><a href="./01-原型图.md">01-原型图.md</a> —— 原型图文字规范（含全部说明表）</li>
    <li><a href="./02-详细设计.md">02-详细设计.md</a> —— 详细设计文字规范（含接口契约与追踪矩阵）</li>
  </ul>
</section>`
  const html = shell(
    'MKV 设计图索引',
    'Migrant Knowledge Vault · 原型图与详细设计图的统一入口。全部图以独立 SVG 提供，并内嵌为两份自包含 HTML 图册。',
    [['v1.0', 'acc'], ['2026-11-10'], ['SVG × ' + all.length, 'tl']],
    '<h4>目录</h4><a href="#">产物清单</a>',
    body,
    'MKV 设计集 · 图索引'
  )
  fs.writeFileSync(path.join(OUT, 'index.html'), html)
  console.log('index.html written')
}
