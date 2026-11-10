// 生成原型线框 SVG（临时构建工具，产物落盘到 docs/design/svg）
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const OUT = process.env.MKV_DESIGN_DIR || path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const SVG_DIR = path.join(OUT, 'svg')
fs.mkdirSync(SVG_DIR, { recursive: true })

const FONT = `'Roobert PRO Medium','Noto Sans','Segoe UI','Microsoft YaHei',sans-serif`
const MONO = `'Cascadia Code','Consolas','Courier New',monospace`

const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')

const STYLE = `
.bg{fill:#ffffff}
.panel{fill:#ffffff;stroke:#e9eaef;stroke-width:1}
.sunken{fill:#f7f8fb}
.bar{fill:#fbfbfd}
.divider{stroke:#e9eaef;stroke-width:1}
.ln{stroke:#c7cad5;stroke-width:1}
.ln-dash{stroke:#c7cad5;stroke-width:1;stroke-dasharray:3 3}
.hit{fill:#5b76fe;fill-opacity:.16}
.t{font-family:${FONT};font-size:11px;fill:#555a6a}
.th{font-family:${FONT};font-size:12px;font-weight:600;fill:#1c1c1e}
.t14{font-family:${FONT};font-size:14px;font-weight:600;fill:#1c1c1e}
.t18{font-family:${FONT};font-size:18px;font-weight:600;fill:#1c1c1e}
.tiny{font-family:${FONT};font-size:10px;fill:#a5a8b5}
.micro{font-family:${FONT};font-size:10px;letter-spacing:.6px;fill:#555a6a}
.mono{font-family:${MONO};font-size:11px;fill:#555a6a}
.mono-b{font-family:${MONO};font-size:11px;fill:#1c1c1e}
.btn{fill:#ffffff;stroke:#c7cad5;stroke-width:1}
.btn-p{fill:#5b76fe;stroke:#5b76fe;stroke-width:1}
.btn-t{font-family:${FONT};font-size:11px;font-weight:600;fill:#1c1c1e}
.btn-pt{font-family:${FONT};font-size:11px;font-weight:600;fill:#ffffff}
.chip{fill:#ffffff;stroke:#c7cad5;stroke-width:1}
.coral{fill:#ffc6c6}.coral-t{font-family:${FONT};font-size:11px;fill:#600000}
.teal{fill:#c3faf5}.teal-t{font-family:${FONT};font-size:11px;fill:#187574}
.rose{fill:#ffd8f4}
.orange{fill:#ffe6cd}
.pink{fill:#fde0f0}
.red{fill:#fbd4d4}.red-t{font-family:${FONT};font-size:11px;fill:#8a2020}
.blue{fill:#5b76fe}
.gray{fill:#c7cad5}
.ok{font-family:${FONT};font-size:11px;fill:#00b473}
.badge{font-family:${FONT};font-size:9.5px;letter-spacing:.5px;font-weight:600}
.anno{font-family:${FONT};font-size:11px;font-weight:700;fill:#ffffff}
.sel{fill:#eef1ff;stroke:#5b76fe;stroke-width:1}
`

const R = (x, y, w, h, cls = 'panel', extra = '') => {
  const m = /rx="(\d+)"/.exec(extra)
  const rx = m ? m[1] : 8
  const rest = extra.replace(/rx="\d+"\s*/, '')
  return `<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="${rx}" class="${cls}" ${rest}/>`
}
const RS = (x, y, w, h, cls = 'panel', extra = '') =>
  `<rect x="${x}" y="${y}" width="${w}" height="${h}" class="${cls}" ${extra}/>`
const T = (x, y, s, cls = 't', extra = '') => `<text x="${x}" y="${y}" class="${cls}" ${extra}>${esc(s)}</text>`
const L = (x1, y1, x2, y2, cls = 'divider') => `<line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" class="${cls}"/>`
const C = (cx, cy, r, cls, extra = '') => `<circle cx="${cx}" cy="${cy}" r="${r}" class="${cls}" ${extra}/>`
const DOT = (x, y, r, cls) => `<circle cx="${x}" cy="${y}" r="${r}" class="${cls}"/>`

// 标注序号徽标
const ANNO = (x, y, n) =>
  `<circle cx="${x}" cy="${y}" r="9" class="blue"/><text x="${x}" y="${y + 4}" text-anchor="middle" class="anno">${n}</text>`

// 小图标：文件/文件夹（几何图形，避免 emoji 字体差异）
const FILE_ICON = (x, y, color) =>
  `<path d="M${x} ${y} h7 l4 4 v13 h-11 z" fill="${color}" fill-opacity=".18" stroke="${color}" stroke-width="1"/><path d="M${x + 7} ${y} v4 h4" fill="none" stroke="${color}" stroke-width="1"/>`
const DIR_ICON = (x, y, color) =>
  `<path d="M${x} ${y} h5 l2 3 h8 v11 h-15 z" fill="${color}" fill-opacity=".18" stroke="${color}" stroke-width="1"/>`

const svg = (w, h, title, body) => `<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${w} ${h}" width="${w}" height="${h}" role="img" aria-label="${esc(title)}">
<title>${esc(title)}</title>
<style>${STYLE}</style>
${body}
</svg>
`

const pages = []
const add = (id, file, title, w, h, body, notes) => {
  fs.writeFileSync(path.join(SVG_DIR, file), svg(w, h, title, body))
  pages.push({ id, file, title, w, h, notes })
}

/* ============================ P-1 工作台 ============================ */
{
  const W = 1280, H = 800
  let b = `<rect class="bg" x="0" y="0" width="${W}" height="${H}"/>`
  // 标题栏
  b += `<rect class="bar" x="0" y="0" width="${W}" height="44"/><line class="divider" x1="0" y1="44" x2="${W}" y2="44"/>`
  b += R(16, 12, 20, 20, 'blue', 'rx="6"')
  b += T(41, 27, 'MKV', 'th')
  b += T(70, 27, '知识库: D:/Docs/MyVault', 't')
  b += R(240, 12, 76, 20, 'chip')
  b += T(252, 26, 'main ⌄', 't')
  b += R(1040, 10, 104, 24, 'btn-p')
  b += T(1092, 26, '⬆ 提交 3 处改动', 'btn-pt', 'text-anchor="middle"')
  b += R(1152, 10, 96, 24, 'btn')
  b += T(1200, 26, 'CtrlK 搜索命令', 'btn-t', 'text-anchor="middle"')
  b += C(1270, 22, 11, 'btn')
  b += T(1270, 26, '⚙', 't', 'text-anchor="middle"')
  // 左侧栏
  b += `<rect class="sunken" x="0" y="44" width="248" height="728"/><line class="divider" x1="248" y1="44" x2="248" y2="772"/>`
  b += ANNO(28, 70, '1')
  b += R(16, 56, 216, 28)
  b += T(28, 74, '🔍 过滤文件…', 'tiny')
  b += DIR_ICON(20, 100, '#555a6a'); b += T(36, 110, '需求', 'th')
  b += T(228, 110, '12', 'tiny', 'text-anchor="end"')
  b += FILE_ICON(36, 120, '#5b76fe'); b += T(52, 131, '需求.md', 't')
  b += FILE_ICON(36, 140, '#00b473'); b += T(52, 151, '台账.xlsx', 't')
  b += DIR_ICON(20, 166, '#555a6a'); b += T(36, 176, '方案', 'th')
  b += FILE_ICON(36, 186, '#e8853a'); b += T(52, 197, '方案.pptx', 't')
  b += FILE_ICON(36, 206, '#9aa0b0'); b += T(52, 217, '流程.vsdx', 't')
  b += DIR_ICON(20, 232, '#555a6a'); b += T(36, 242, '归档', 't')
  b += T(228, 242, '31', 'tiny', 'text-anchor="end"')
  b += FILE_ICON(36, 252, '#9aa0b0'); b += T(52, 263, '旧版.docx', 't')
  b += DOT(232, 176, 3, 'blue')
  b += `<line class="divider" x1="16" y1="284" x2="232" y2="284"/>`
  b += R(16, 296, 96, 26, 'btn'); b += T(64, 313, '＋ 新建文件', 'btn-t', 'text-anchor="middle"')
  b += R(120, 296, 84, 26, 'btn'); b += T(162, 313, '⟳ 刷新', 'btn-t', 'text-anchor="middle"')
  b += T(16, 762, '索引 .index/ · 仅本地', 'tiny')
  // 中间标签页
  b += `<rect class="bg" x="248" y="44" width="720" height="32"/>`
  b += R(260, 50, 112, 22, 'sel')
  b += T(272, 65, '方案.pptx', 'th'); b += T(360, 65, '×', 'tiny')
  b += T(384, 65, '台账.xlsx', 't'); b += T(440, 65, '×', 'tiny')
  b += T(460, 65, '＋', 'tiny')
  b += `<line class="divider" x1="248" y1="76" x2="968" y2="76"/>`
  // 引用条
  b += ANNO(276, 100, '2')
  b += R(264, 88, 688, 34, 'coral', 'rx="10"')
  b += T(280, 110, '⬡ 引用自  台账.xlsx › 2024Q1 › B2「营收 120 万」', 'coral-t')
  b += T(800, 110, '⟳ 源已变更', 'coral-t')
  b += T(936, 110, '查看差异 ▸', 'coral-t', 'text-anchor="end"')
  // 文档工作区
  b += ANNO(276, 142, '3')
  b += R(264, 128, 688, 560)
  b += T(284, 156, '幻灯片渲染 · 第 7 / 24 页', 'tiny')
  b += `<rect class="sunken" x="284" y="168" width="648" height="300" rx="12"/>`
  b += R(500, 216, 216, 132, 'panel', 'rx="6"')
  b += T(608, 288, '第 7 页缩略', 'tiny', 'text-anchor="middle"')
  b += R(540, 312, 136, 8, 'gray', 'rx="4"')
  b += T(608, 506, '（可点选元素 · 元素级锚点）', 'tiny', 'text-anchor="middle"')
  b += T(284, 500, 'Q1 营收概览', 'th')
  b += T(284, 520, '正文与图表由引擎渲染，能力不足时降级为只读。', 'tiny')
  // 能力条
  b += ANNO(276, 712, '4')
  b += R(264, 692, 688, 34, 'panel')
  b += T(282, 714, '模式：编辑 ▾', 't')
  b += T(360, 714, '引擎：deck 0.16', 't')
  b += T(800, 714, '⇩ 导出', 't')
  b += T(860, 714, '⇱ 另存', 't')
  b += T(936, 714, '⤢ 聚焦', 't', 'text-anchor="end"')
  b += T(264, 748, '◀◀  命中 3 / 12  ▶▶', 'tiny')
  b += T(600, 748, '已保存 ✓ 12:04', 'ok', 'text-anchor="middle"')
  b += T(952, 748, '预览 | 编辑 | 分栏', 'tiny', 'text-anchor="end"')
  // 右侧面板
  b += `<rect class="bar" x="968" y="44" width="312" height="728"/><line class="divider" x1="968" y1="44" x2="968" y2="772"/>`
  b += ANNO(996, 70, '5')
  const tabs = ['大纲', '反链', '引用', '修订']
  tabs.forEach((t, i) => {
    const x = 984 + i * 74
    b += T(x, 70, t, t === '反链' ? 'th' : 't')
    if (t === '反链') b += `<rect class="blue" x="${x}" y="78" width="24" height="2" rx="1"/>`
  })
  b += T(1264, 70, '3', 'tiny', 'text-anchor="end"')
  b += `<line class="divider" x1="968" y1="88" x2="1280" y2="88"/>`
  b += T(984, 110, '反链（谁引用了我）', 'th')
  const bl = [
    ['周报.md › §结论', '"营收 120 万" · 2 天前 · 未提交', 'blue'],
    ['复盘.md › L42', '"见 [[台账.xlsx#Sheet1!B2]]" · 5 天前', 'teal'],
    ['归档/2023.md › 第 4 段', '已失效（源文件已移动）', 'red'],
  ]
  bl.forEach((it, i) => {
    const y = 126 + i * 74
    b += DOT(992, y + 10, 4, it[2])
    b += T(1006, y + 14, it[0], 'th')
    b += T(1006, y + 32, it[1], 'tiny')
    b += `<line class="ln-dash" x1="984" y1="${y + 48}" x2="1264" y2="${y + 48}"/>`
  })
  b += R(984, 352, 280, 28, 'btn')
  b += T(1000, 370, '＋ 反向插入（拖拽文件到此处）', 't')
  b += T(984, 404, '引用（我引用了谁）', 'th')
  b += T(984, 424, '· 台账.xlsx › Sheet1!B2', 't')
  b += T(984, 442, '· 需求.md › §2 目标', 't')
  b += `<line class="divider" x1="968" y1="460" x2="1280" y2="460"/>`
  b += T(984, 486, '大纲', 'th')
  b += T(984, 508, '1. 背景', 't')
  b += T(984, 528, '2. 目标', 't')
  b += T(984, 548, '3. 结论', 't')
  // 状态栏
  b += `<rect class="bar" x="0" y="772" width="${W}" height="28"/><line class="divider" x1="0" y1="772" x2="${W}" y2="772"/>`
  b += DOT(16, 786, 4, 'teal')
  b += T(28, 790, '索引就绪 12,480 篇', 't')
  b += T(180, 790, 'Git 工作区干净', 't')
  b += T(300, 790, '插件 5/6 可用', 't')
  b += T(420, 790, '后台扫描中 34%', 't')
  b += T(1264, 790, '全部文件本地 · 无网络请求', 'tiny', 'text-anchor="end"')
  add('p01', 'p01-workbench.svg', 'P-1 工作台（主界面三栏线框）', W, H, b,
    ['① 资源侧栏：统一文件树，支持过滤、右键、拖拽插入引用',
      '② 跨格式引用条：源变更时提示「已变更」，可刷新/看差异',
      '③ 文档工作区：预览 = 编辑同页，渲染器按格式分发',
      '④ 能力条：模式、引擎、导出/另存；引擎缺失即禁用',
      '⑤ 上下文面板：大纲 / 反链 / 引用 / 修订 四页签联动'])
}

/* ============================ P-2 文件树 ============================ */
{
  const W = 480, H = 620
  let b = `<rect class="bg" x="0" y="0" width="${W}" height="${H}"/>`
  b += RS(16, 16, 448, 588, 'panel')
  b += T(32, 46, '文件树', 't14')
  b += R(32, 60, 416, 30)
  b += T(44, 79, '🔍 过滤文件…', 'tiny')
  b += `<line class="divider" x1="32" y1="104" x2="448" y2="104"/>`
  const rows = [
    ['d', '需求', '12', '#555a6a', 0],
    ['f', '需求.md', 'md', '#5b76fe', 1],
    ['f', '台账.xlsx', 'xlsx', '#00b473', 1],
    ['f', '智能文档.docxxml', 'docxxml', '#187574', 1],
    ['d', '方案', '8', '#555a6a', 0],
    ['f', '方案.pptx', 'pptx', '#e8853a', 1],
    ['f', '架构.drawio', 'drawio', '#9aa0b0', 1],
    ['f', '流程.vsdx', 'vsdx', '#9aa0b0', 1],
    ['d', '归档', '31', '#555a6a', 0],
    ['f', '旧版.docx', 'docx', '#187574', 1],
  ]
  rows.forEach((r, i) => {
    const y = 124 + i * 30
    const x = 40 + r[4] * 20
    if (r[0] === 'd') {
      b += DIR_ICON(x, y - 9, r[3]); b += T(x + 16, y + 2, r[1], 'th')
    } else {
      b += FILE_ICON(x, y - 9, r[3]); b += T(x + 16, y + 2, r[1], 't')
    }
    b += T(432, y + 2, r[2], 'tiny', 'text-anchor="end"')
  })
  b += `<line class="divider" x1="32" y1="432" x2="448" y2="432"/>`
  b += R(32, 448, 112, 28, 'btn'); b += T(88, 466, '＋ 新建', 'btn-t', 'text-anchor="middle"')
  b += R(152, 448, 96, 28, 'btn'); b += T(200, 466, '⇅ 排序', 'btn-t', 'text-anchor="middle"')
  b += R(256, 448, 96, 28, 'btn'); b += T(304, 466, '⟳ 刷新', 'btn-t', 'text-anchor="middle"')
  b += R(32, 490, 416, 28, 'sunken', 'rx="8"')
  b += T(44, 508, '拖到文档区＝插入引用（动态，非复制）', 'tiny')
  b += T(32, 542, '· 单击＝预览　双击＝编辑　右键＝更多操作', 'tiny')
  b += T(32, 560, '· 文件夹前圆点＝该目录存在未提交改动', 'tiny')
  b += T(32, 578, '· 输入 [[ 触发文件 / 标题 / 块 三级补全', 'tiny')
  add('p02', 'p02-file-tree.svg', 'P-2 文件树侧栏', W, H, b,
    ['单一入口管理多格式文件：md / xlsx / pptx / docx / docxxml / base / pdf / drawio / vsdx',
      '拖拽文件到文档区生成「动态引用」而非复制内容',
      '输入 [[ 触发三级补全：文件 → 标题 → 块锚点'])
}

/* ============================ P-3 文档工作区 ============================ */
{
  const W = 960, H = 640
  let b = `<rect class="bg" x="0" y="0" width="${W}" height="${H}"/>`
  b += RS(16, 16, 928, 608, 'panel')
  b += R(16, 16, 928, 40, 'bar', 'rx="8"')
  b += T(32, 41, '模式：预览 | 编辑 | 分栏', 't')
  b += T(300, 41, '锚点：2.3 / 目标设备', 't')
  b += T(928, 41, '引擎：md ▾', 't', 'text-anchor="end"')
  b += `<line class="divider" x1="16" y1="56" x2="944" y2="56"/>`
  b += T(48, 96, '# 目标与范围', 't18')
  b += T(48, 122, '本文档描述本地知识库的产品目标与范围边界。', 't')
  b += T(48, 152, '正文段落……（编辑态为块编辑，CtrlB 加粗、CtrlShiftX 插入引用）', 't')
  b += R(48, 168, 12, 12, 'blue', 'rx="3"')
  b += T(68, 178, '引用：', 't')
  b += `<rect class="hit" x="104" y="166" width="196" height="16" rx="4"/>`
  b += T(104, 178, '[[台账.xlsx#Sheet1!B2]]', 'mono-b')
  b += T(308, 178, '其值为 120 万', 't')
  b += R(48, 200, 400, 96, 'sunken', 'rx="8"')
  b += T(60, 222, '▸ 代码块 #anchor', 'mono')
  b += T(60, 244, 'vault = load_workspace(path)', 'mono')
  b += T(60, 264, 'index = build_index(scope)', 'mono')
  b += T(60, 280, '· 双击进入编辑；Esc 退出编辑', 'tiny')
  // 格式分发说明条
  b += `<line class="divider" x1="16" y1="316" x2="944" y2="316"/>`
  b += T(32, 340, '同一容器 · 按格式分发渲染器', 'th')
  const cells = [
    ['Markdown', '块编辑', 'teal'],
    ['DocxXML', 'TipTap 块', 'teal'],
    ['Base', '多维表格', 'teal'],
    ['Excel', '表格 / 单元格', 'orange'],
    ['PPT', '幻灯片 / 页', 'orange'],
    ['Word', '文档 / 章节', 'orange'],
    ['PDF', '只读查看', 'rose'],
    ['Drawio', '只读节点', 'pink'],
  ]
  cells.forEach((c, i) => {
    const x = 32 + (i % 4) * 226
    const y = 352 + Math.floor(i / 4) * 54
    b += R(x, y, 210, 44, 'panel')
    b += R(x + 10, y + 12, 10, 14, c[2], 'rx="2"')
    b += T(x + 28, y + 22, c[0], 'th')
    b += T(x + 28, y + 36, c[1], 'tiny')
  })
  b += `<line class="divider" x1="16" y1="466" x2="944" y2="466"/>`
  b += T(32, 490, '锚点模型（点击来源必须可定位）', 'th')
  const anchors = [
    'md      path#heading / #block:<id>',
    'base    path#record:<id>',
    'xlsx    path#Sheet1!B2 或 #Sheet1!B2:D8',
    'pptx    path#p:7 / #p:7@<element>',
    'docx    path#第二章 / #para:<id>',
  ]
  anchors.forEach((a, i) => b += T(32, 514 + i * 18, a, 'mono'))
  b += R(32, 614, 912, 1, 'divider', 'rx="0"')
  add('p03', 'p03-document-workspace.svg', 'P-3 文档工作区（预览 = 编辑同页）', W, H, b,
    ['一个容器承载全部格式，切换格式不改变布局（需求 §5.4）',
      '编辑块、代码块、标题均可作为双链锚点',
      '引擎缺失 → 占位卡，引用条与反链仍可用'])
}

/* ============================ P-4 跨格式引用条 ============================ */
{
  const W = 960, H = 300
  let b = `<rect class="bg" x="0" y="0" width="${W}" height="${H}"/>`
  const rows = [
    ['teal', 'teal-t', '最新', '⬡ 引用自  台账.xlsx › 2024Q1 › B2「营收 120 万」', '内容与源一致', '定位到源'],
    ['coral', 'coral-t', '已变更', '⬡ 引用自  台账.xlsx › 2024Q1 › B2「营收 120 万」', '⟳ 源已变更 v3 · 快照：营收 120 万（2024-11-02）', '刷新 / 查看差异'],
    ['red', 'red-t', '失效', '⬡ 引用自  台账-旧.xlsx › B2「营收 120 万」', '源文件已删除或移动', '重定位 / 转为纯文本'],
  ]
  rows.forEach((r, i) => {
    const y = 24 + i * 88
    b += T(16, y + 12, r[2], 'th')
    b += R(16, y + 20, 928, 56, r[0], 'rx="10"')
    b += T(34, y + 46, r[3], r[1])
    b += T(34, y + 64, r[4], r[1])
    b += T(928, y + 46, r[5], r[1], 'text-anchor="end"')
  })
  add('p04', 'p04-reference-bar.svg', 'P-4 跨格式引用条（三种状态）', W, H, b,
    ['引用是动态的，不是复制：源更新即标记「已变更」',
      '刷新 = 取源最新内容写回引用处，产生一次可 diff 的变更',
      '失效（源删除/改名）→ 提供「重定位」与「转为纯文本快照」两个出口'])
}

/* ============================ P-5 搜索中心 ============================ */
{
  const W = 1280, H = 760
  let b = `<rect class="bg" x="0" y="0" width="${W}" height="${H}"/>`
  b += T(24, 40, '搜索中心', 't18')
  b += T(160, 40, '跨格式全文检索 · 命中可跳转原文精确位置', 't')
  // 搜索框
  b += R(24, 56, 1232, 44)
  b += T(44, 84, '🔍', 't')
  b += T(66, 84, '营收 120', 'th')
  b += `<rect class="hit" x="126" y="70" width="34" height="18" rx="4"/>`
  b += T(1200, 84, '12 个结果 · 38ms', 'tiny', 'text-anchor="end"')
  // 过滤条
  const chips = [['格式：全部 ▾'], ['标签：财务 ▾'], ['路径：需求/ ▾'], ['时间：近 30 天 ▾'], ['正则 ⚡']]
  let cx = 24
  chips.forEach((c, i) => {
    const w = i === 0 ? 96 : i === 4 ? 72 : 104
    b += R(cx, 112, w, 28, i === 4 ? 'sel' : 'chip')
    b += T(cx + 14, 130, c[0], 't')
    cx += w + 12
  })
  // 左侧过滤
  b += RS(0, 152, 260, 608, 'sunken')
  b += `<line class="divider" x1="260" y1="152" x2="260" y2="760"/>`
  b += T(24, 182, '过滤', 'th')
  b += T(24, 210, '格式', 'micro')
  const fmts = [['☑ 全部', '12', true], ['☐ xlsx', '3'], ['☐ pptx', '2'], ['☐ docx', '5'], ['☐ md', '2']]
  fmts.forEach((f, i) => {
    const y = 232 + i * 24
    b += T(32, y, f[0], f[2] ? 'th' : 't')
    b += T(240, y, f[1], 'tiny', 'text-anchor="end"')
  })
  b += T(24, 368, '标签', 'micro')
  ;['☑ 财务', '☐ 产品', '☐ 归档'].forEach((t, i) => b += T(32, 392 + i * 24, t, i === 0 ? 'th' : 't'))
  b += T(24, 480, '时间范围', 'micro')
  b += R(24, 492, 212, 30, 'panel'); b += T(38, 512, '2026-10-11 → 2026-11-10', 'tiny')
  b += T(24, 556, '索引状态', 'micro')
  b += DOT(30, 574, 4, 'teal')
  b += T(42, 578, '已索引 12,480 篇 · 覆盖 9 类格式', 'tiny')
  // 结果列表
  const res = [
    ['台账.xlsx › 2024Q1 › B2', 'xlsx', '…预计营收 120 万，同比 +12%…', '营收 120'],
    ['方案.pptx › p.7', 'pptx', '…Q1 营收 120 万达成情况…', '营收 120'],
    ['周报.md › §结论', 'md', '…据台账，营收 120 万已确认…', '营收 120'],
  ]
  res.forEach((r, i) => {
    const y = 176 + i * 96
    b += R(284, y, 972, 84)
    b += T(304, y + 26, `${i + 1}. ${r[0]}`, 'th')
    b += R(1150, y + 12, 90, 20, 'sunken', 'rx="6"')
    b += T(1195, y + 26, r[1], 'micro', 'text-anchor="middle"')
    b += T(304, y + 52, r[2].split('营收 120')[0], 't')
    b += `<rect class="hit" x="${304 + 150}" y="${y + 38}" width="48" height="17" rx="4"/>`
    b += T(304 + 152, y + 52, '营收 120', 't')
    b += T(304 + 210, y + 52, r[2].split('营收 120')[1], 't')
    b += T(1236, y + 70, '定位 ⟩', 'tiny', 'text-anchor="end"')
  })
  b += T(284, 484, '…… 省略 9 条', 'tiny')
  b += R(284, 504, 972, 56, 'panel')
  b += T(304, 528, '未命中时的引导：检查索引状态 / 建立检索别名 / 用正则重试', 'tiny')
  b += T(284, 546, '正则开关开启时非法表达式会在输入框下方给出行内错误并禁用检索', 'tiny')
  add('p05', 'p05-search.svg', 'P-5 搜索中心（结果 + 上下文 + 过滤）', W, H, b,
    ['命中列表带上下文片段，命中词高亮（Blue 450）',
      '过滤（格式/标签/路径/时间）与检索同一表达式下推',
      '点击命中 → 打开文档定位 → 右栏自动切到「反链」形成闭环'])
}

/* ============================ P-6 双链与反链 ============================ */
{
  const W = 1280, H = 560
  let b = `<rect class="bg" x="0" y="0" width="${W}" height="${H}"/>`
  // 左：文档内联双链 + 悬停卡片
  b += RS(16, 16, 700, 528, 'panel')
  b += T(40, 52, '周报.md', 't14')
  b += T(40, 84, '▸ 见 ', 't')
  b += `<rect class="hit" x="72" y="70" width="118" height="18" rx="4"/>`
  b += T(74, 84, '[[台账.xlsx#Sheet1!B2]]', 'mono-b')
  b += T(196, 84, '，其值为 120 万', 't')
  // 悬停预览卡
  b += R(72, 104, 300, 128, 'panel', 'rx="12"')
  b += T(88, 126, '台账.xlsx › 2024Q1 › B2', 'th')
  b += T(88, 148, '营收 120 万', 't')
  b += T(88, 168, '被 3 处引用 · 最近更新 2 天前', 'tiny')
  b += `<line class="divider" x1="88" y1="180" x2="356" y2="180"/>`
  b += T(88, 200, '[ 点击打开并高亮定位 ]', 'tiny')
  // 解析管线说明
  b += `<line class="divider" x1="40" y1="268" x2="692" y2="268"/>`
  b += T(40, 294, '链接语法', 'th')
  ;['[[方案.pptx#p:7]]   → 出链(wikilink) + 锚点 p:7',
      '[[周报#结论]]        → 出链 + 锚点 heading:结论',
      '@台账.xlsx#Sheet1!B2 → 出链(at) + 单元格锚点',
      '引用块（引擎生成）    → 出链(embed) + 快照内容'].forEach((t, i) =>
    b += T(40, 318 + i * 22, t, 'mono'))
  b += T(40, 424, '解析时机：写入期（引擎保存上报）+ 扫描期（外部改动兜底）', 'tiny')
  b += T(40, 442, '幂等键：mtime + hash；冲突时以扫描期为准', 'tiny')
  // 右：反链面板
  b += RS(732, 16, 532, 528, 'panel')
  b += T(756, 52, '反链（谁引用了我）', 't14')
  b += R(1120, 32, 120, 26, 'chip'); b += T(1180, 49, '按时间排序 ▾', 't', 'text-anchor="middle"')
  b += `<line class="divider" x1="752" y1="66" x2="1244" y2="66"/>`
  const bl = [
    ['●', 'blue', '周报.md › §结论', '"营收 120 万" · 2 天前 · 未提交', ''],
    ['●', 'teal', '复盘.md › L42', '"见 [[台账.xlsx#Sheet1!B2]]" · 5 天前', ''],
    ['●', 'red', '归档/2023.md › 第 4 段', '已失效（源文件已移动）', '重定位'],
  ]
  bl.forEach((it, i) => {
    const y = 90 + i * 92
    b += DOT(764, y + 8, 4, it[1])
    b += T(780, y + 12, it[0] === '●' ? it[2] : it[2], 'th')
    b += T(780, y + 32, it[3], 'tiny')
    if (it[4]) b += R(1130, y + 2, 90, 24, 'btn'); b += T(1175, y + 18, it[4], 'btn-t', 'text-anchor="middle"')
    b += `<line class="ln-dash" x1="756" y1="${y + 52}" x2="1240" y2="${y + 52}"/>`
  })
  b += T(756, 396, '引用（我引用了谁）', 'th')
  b += T(756, 418, '· 台账.xlsx › Sheet1!B2', 't')
  b += T(756, 436, '· 需求.md › §2 目标', 't')
  b += `<line class="divider" x1="752" y1="460" x2="1244" y2="460"/>`
  b += R(756, 476, 464, 30, 'panel')
  b += T(772, 496, '搜到 → 跳转 → 看反链 → 顺藤摸瓜（需求 §5.3）', 'tiny')
  add('p06', 'p06-backlinks.svg', 'P-6 内联双链与反链面板', W, H, b,
    ['双链语法：`[[文档#锚点]]`、`@引用`、引擎生成的引用块',
      '反链条目携带上下文句与 Git 状态，便于判断新鲜度',
      '失效链接单独归组，提供「重定位」出口'])
}

/* ============================ P-7 关系图谱 ============================ */
{
  const W = 1000, H = 620
  let b = `<rect class="bg" x="0" y="0" width="${W}" height="${H}"/>`
  b += R(16, 16, 968, 44, 'panel')
  b += T(36, 44, '布局：力导向 ▾', 't')
  b += T(180, 44, '深度：2 ▾', 't')
  b += T(300, 44, '聚焦：当前文档', 't')
  b += T(460, 44, '过滤：仅 md ☐  含失效 ☑', 't')
  b += T(964, 44, '⟳ 重排', 't', 'text-anchor="end"')
  b += RS(16, 72, 968, 480, 'panel')
  const nodes = [
    [500, 300, '周报.md', 'md', 'blue', 1],
    [300, 190, '台账.xlsx', 'xlsx', 'teal', 0],
    [700, 190, '方案.pptx', 'pptx', 'orange', 0],
    [300, 420, '复盘.md', 'md', 'blue', 0],
    [700, 420, '归档/2023.md', 'md', 'gray', 0],
    [500, 130, '需求.md', 'md', 'blue', 0],
  ]
  const edges = [
    [500, 300, 300, 190, '引用', ''],
    [500, 300, 700, 190, '引用', ''],
    [500, 300, 300, 420, '引用', ''],
    [500, 300, 500, 130, '引用', ''],
    [300, 420, 700, 420, '失效', 'ln-dash'],
  ]
  edges.forEach(e => {
    b += L(e[0], e[1], e[2], e[3], e[4] || 'ln')
    const mx = (e[0] + e[2]) / 2, my = (e[1] + e[3]) / 2
    b += T(mx + 6, my - 4, e[4] === '失效' ? '失效' : '', 'tiny')
  })
  nodes.forEach(n => {
    const w = 96, h = 34
    const isCur = n[5]
    b += R(n[0] - w / 2, n[1] - h / 2, w, h, isCur ? 'sel' : 'panel', 'rx="10"')
    b += R(n[0] - w / 2 + 10, n[1] - 8, 10, 14, n[4], 'rx="2"')
    b += T(n[0] - w / 2 + 26, n[1] + 2, n[2], 'th')
    b += T(n[0] - w / 2 + 26, n[1] + 14, n[3], 'tiny')
    if (isCur) b += T(n[0] + w / 2 - 12, n[1] - h / 2 + 12, '●', 'blue')
  })
  b += T(36, 100, '单击节点→右侧预览　双击→打开文档　框选→批量关系视图', 'tiny')
  b += R(16, 564, 968, 40, 'panel')
  b += DOT(36, 584, 5, 'blue'); b += T(48, 588, '当前文档', 'tiny')
  b += DOT(130, 584, 5, 'teal'); b += T(142, 588, '被引用', 'tiny')
  b += DOT(220, 584, 5, 'panel'); b += T(232, 588, '邻域节点', 'tiny')
  b += DOT(340, 584, 5, 'gray'); b += T(352, 588, '失效链接（虚线）', 'tiny')
  b += T(964, 588, '规模保护：默认 2 度邻域，节点数 > 500 时按度数裁剪', 'tiny', 'text-anchor="end"')
  add('p07', 'p07-graph.svg', 'P-7 关系图谱视图', W, H, b,
    ['节点 = 文档，边 = 引用关系（区分单向 / 互引 / 失效）',
      '默认只展开 2 度邻域，避免万级节点一次性渲染',
      '双链关系图与反链面板共享同一关系数据'])
}

/* ============================ P-8 版本时间线 ============================ */
{
  const W = 1100, H = 560
  let b = `<rect class="bg" x="0" y="0" width="${W}" height="${H}"/>`
  b += RS(16, 16, 520, 528, 'panel')
  b += T(40, 50, '时间线', 't14')
  b += R(420, 32, 100, 26, 'chip'); b += T(470, 49, '仅当前文件 ▾', 't', 'text-anchor="middle"')
  b += `<line class="divider" x1="36" y1="66" x2="516" y2="66"/>`
  const commits = [
    ['c3f91a', '2026-11-10 11:02', '自动快照', '修订单价计算口径', 'blue'],
    ['8a2b10', '2026-11-09 18:41', '手动提交', '更新 Q1 台账', 'teal'],
    ['5d0e44', '2026-11-08 09:15', '自动快照', '补充架构图引用', 'teal'],
    ['2b7c31', '2026-11-07 22:03', '自动快照', '周报自动快照', 'teal'],
    ['初始导入', '2026-11-01 10:00', '初始', '导入知识库', 'gray'],
  ]
  b += L(60, 96, 60, 440, 'ln')
  commits.forEach((c, i) => {
    const y = 100 + i * 84
    b += DOT(60, y, 5, c[4])
    b += T(84, y - 4, c[0], 'mono-b')
    b += T(190, y - 4, c[2], 'tiny')
    b += T(84, y + 18, c[3], 'th')
    b += T(84, y + 36, c[1], 'tiny')
  })
  b += R(40, 470, 472, 32, 'panel')
  b += T(56, 490, '自动快照：默认 10 分钟或空闲 60s；跳过仅索引变动', 'tiny')
  // diff 区
  b += RS(552, 16, 532, 528, 'panel')
  b += T(576, 50, 'diff：方案.pptx', 't14')
  b += T(1064, 50, '还原到此版本', 't', 'text-anchor="end"')
  b += `<line class="divider" x1="572" y1="66" x2="1064" y2="66"/>`
  b += T(576, 92, 'c3f91a', 'mono-b')
  b += T(576, 116, '+  营收 120 万 → 138 万', 't')
  b += T(576, 138, '-  营收 120 万', 't')
  b += T(576, 176, '8a2b10', 'mono-b')
  b += T(576, 200, '(无差异)', 'tiny')
  b += `<line class="divider" x1="572" y1="220" x2="1064" y2="220"/>`
  b += T(576, 248, '二进制文件（xlsx / pptx）的 diff', 'th')
  ;['· 工作表：2024Q1 → 2024Q1',
      '· 单元格：B2「营收 120 万」→「营收 138 万」',
      '· 页数：24 → 24（幻灯片 7 版式变更）',
      '· 结构化摘要，不逐字节对比'].forEach((t, i) => b += T(576, 274 + i * 22, t, 'tiny'))
  b += R(576, 372, 488, 60, 'panel')
  b += T(592, 396, '回滚策略：生成新提交而非 reset，历史不丢', 'tiny')
  b += T(592, 416, '（Git 安全约束：禁止 force / hard reset）', 'tiny')
  b += R(576, 448, 488, 30, 'panel')
  b += T(592, 468, 'Ctrl+S：文档区＝保存；工作区＝打开提交面板', 'tiny')
  add('p08', 'p08-git-timeline.svg', 'P-8 Git 时间线与 diff', W, H, b,
    ['自动快照 + 手动提交共用一条时间线，标注提交类型',
      '二进制文件给结构化摘要（工作表/单元格/页/版式）',
      '回滚生成新提交，保留完整历史'])
}

/* ============================ P-9 命令面板 ============================ */
{
  const W = 900, H = 560
  let b = `<rect x="0" y="0" width="${W}" height="${H}" fill="#1c1c1e" fill-opacity=".38"/>`
  b += R(120, 80, 660, 400, 'panel', 'rx="16"')
  b += R(120, 80, 660, 56, 'bar', 'rx="16"')
  b += RS(120, 120, 660, 16, 'bar')
  b += T(148, 114, '🔍', 't')
  b += T(172, 114, '输入命令或文件名…', 't')
  b += `<line class="divider" x1="120" y1="136" x2="780" y2="136"/>`
  const rows = [
    ['提交：方案.pptx 的改动', 'CtrlEnter', '文件', true],
    ['在台账中查找「营收 120」', 'CtrlShiftF', '搜索', false],
    ['打开 台账.xlsx', 'CtrlO', '文件', false],
    ['新建 Markdown 文档', 'CtrlN', '文件', false],
    ['插入引用…', 'CtrlShiftR', '编辑', false],
    ['重建索引', 'AltR', '索引', false],
    ['安装插件：Deck（PPTX 引擎）', 'CtrlP', '插件', false],
  ]
  rows.forEach((r, i) => {
    const y = 148 + i * 46
    if (r[3]) b += R(132, y, 636, 40, 'sel', 'rx="8"')
    b += T(148, y + 25, r[0], r[3] ? 'th' : 't')
    b += R(600, y + 10, 56, 20, 'sunken', 'rx="6"')
    b += T(628, y + 24, r[3] ? '2' : '', 'tiny', 'text-anchor="middle"')
    b += T(676, y + 25, r[1], 'tiny', 'text-anchor="end"')
    b += R(700, y + 10, 60, 20, 'sunken', 'rx="6"')
    b += T(730, y + 24, r[2], 'micro', 'text-anchor="middle"')
  })
  b += `<line class="divider" x1="120" y1="440" x2="780" y2="440"/>`
  b += T(148, 462, '↑↓ 选择　Enter 执行　Esc 关闭　Ctrl+1..9 直选', 'tiny')
  b += T(772, 462, '聚合：文件 / 命令 / 插件 / 设置项', 'tiny', 'text-anchor="end"')
  add('p09', 'p09-command-palette.svg', 'P-9 命令面板', W, H, b,
    ['统一入口：文件、命令、插件安装、设置项四类候选',
      '插件与设置项为 P1 动态注册来源',
      '快捷键直选（Ctrl+1..9）用于高频动作'])
}

/* ============================ P-10 设置 ============================ */
{
  const W = 1100, H = 620
  let b = `<rect class="bg" x="0" y="0" width="${W}" height="${H}"/>`
  b += RS(16, 16, 220, 588, 'sunken')
  b += `<line class="divider" x1="236" y1="16" x2="236" y2="604"/>`
  const nav = [['通用', false], ['外观与主题', false], ['Git', false], ['索引与扫描', false], ['插件与格式', true], ['快捷键', false]]
  nav.forEach((n, i) => {
    const y = 48 + i * 44
    if (n[1]) { b += R(24, y - 20, 204, 36, 'sel', 'rx="8"'); b += R(24, y - 20, 3, 36, 'blue', 'rx="2"') }
    b += T(44, y, n[0], n[1] ? 'th' : 't')
  })
  b += T(24, 420, '本地与安全', 'micro')
  b += T(44, 444, '全量本地加密（可选）', 't')
  b += T(44, 468, '不联网上报（已开启）', 't')
  // 右侧内容
  b += T(268, 52, '插件与格式能力', 't18')
  b += T(268, 76, '引擎按格式与能力注册；缺失即降级（外跳 / 占位卡），不阻断主流程', 'tiny')
  b += `<line class="divider" x1="268" y1="92" x2="1068" y2="92"/>`
  const cols = [268, 460, 600, 760, 900]
  b += T(cols[0], 118, '插件', 'micro')
  b += T(cols[1], 118, '格式', 'micro')
  b += T(cols[2], 118, '能力', 'micro')
  b += T(cols[3], 118, '状态', 'micro')
  b += T(cols[4], 118, '操作', 'micro')
  b += `<line class="divider" x1="268" y1="128" x2="1068" y2="128"/>`
  const plugins = [
    ['deck (PPTX)', '.pptx', '编辑 · 渲染', '✔ 已启用 v0.16', 'teal', '禁用'],
    ['word (DOCX)', '.docx', '编辑 · 渲染', '✔ 已启用 v0.16', 'teal', '禁用'],
    ['grid (XLSX)', '.xlsx', '编辑 · 渲染', '✔ 已启用 v0.16', 'teal', '禁用'],
    ['block (DocxXML)', '.docxxml', '编辑 · 渲染', '✔ 已启用', 'teal', '禁用'],
    ['base (多维表格)', '.base', '编辑 · 渲染', '⚠ 实验性', 'orange', '设置'],
    ['pdf', '.pdf', '渲染（只读）', '✔ 已启用', 'teal', '禁用'],
    ['extract-drawio', '.drawio', '抽取（只读）', '✔ 已启用', 'teal', '诊断'],
    ['office (sidecar)', 'docx/xlsx/pptx', '重编辑（可选）', '✖ 未安装 · +1~2GB', 'red', '安装'],
  ]
  plugins.forEach((p, i) => {
    const y = 150 + i * 46
    if (i % 2 === 0) b += RS(260, y - 18, 816, 40, 'sunken')
    b += T(cols[0], y + 6, p[0], 'th')
    b += T(cols[1], y + 6, p[1], 'mono')
    b += T(cols[2], y + 6, p[2], 't')
    b += T(cols[3], y + 6, p[3], p[4] === 'teal' ? 'teal-t' : p[4] === 'orange' ? 'coral-t' : 'red-t')
    b += R(cols[4], y - 10, 72, 24, 'btn')
    b += T(cols[4] + 36, y + 6, p[5], 'btn-t', 'text-anchor="middle"')
  })
  b += `<line class="divider" x1="268" y1="524" x2="1068" y2="524"/>`
  b += R(268, 540, 180, 28, 'btn'); b += T(358, 558, '从本地安装插件包', 'btn-t', 'text-anchor="middle"')
  b += R(460, 540, 120, 28, 'btn'); b += T(520, 558, '检查更新', 'btn-t', 'text-anchor="middle"')
  b += R(592, 540, 140, 28, 'btn'); b += T(662, 558, '打开插件目录', 'btn-t', 'text-anchor="middle"')
  add('p10', 'p10-settings.svg', 'P-10 设置（插件与格式能力）', W, H, b,
    ['插件清单即能力清单：格式 → 能力 → 状态 → 操作',
      '未安装/实验性插件在能力表中显式标注',
      '安装时展示权限声明（fs / net / proc），net 默认拒绝'])
}

/* ============================ P-11 降级与异常态 ============================ */
{
  const W = 1160, H = 520
  let b = `<rect class="bg" x="0" y="0" width="${W}" height="${H}"/>`
  const cards = [
    ['coral', 'coral-t', '引擎插件未安装', '需要 Deck 插件才能预览 .pptx', '安装 Deck 插件', '渲染器降级：占位卡 + 引导安装\n引用条与右栏反链仍然可用'],
    ['orange', 'coral-t', '文档为只读', '文件被其他程序占用', '复制为副本', '顶栏黄条提示只读原因\n保存动作降级为「另存副本」'],
    ['teal', 'teal-t', '索引重建中', '已处理 4,120 / 12,480 篇（34%）', '后台继续', '状态栏进度条\n搜索结果标注「结果可能不完整」'],
  ]
  cards.forEach((c, i) => {
    const x = 16 + i * 376
    b += RS(x, 16, 356, 488, 'panel')
    b += RS(x, 16, 356, 8, c[0])
    b += T(x + 24, 56, c[1], 'th')
    b += T(x + 24, 92, c[2], 't14')
    b += T(x + 24, 116, c[3], 't')
    b += R(x + 24, 140, 308, 190, 'sunken', 'rx="12"')
    b += R(x + 128, 178, 100, 100, 'panel', 'rx="12"')
    b += T(x + 178, 234, '⬡', 'tiny', 'text-anchor="middle"')
    b += T(x + 178, 302, c[2], 'tiny', 'text-anchor="middle"')
    b += R(x + 24, 352, 140, 30, 'btn-p')
    b += T(x + 94, 372, c[4], 'btn-pt', 'text-anchor="middle"')
    b += T(x + 24, 410, '降级策略', 'micro')
    c[5].split('\n').forEach((t, k) => b += T(x + 24, 432 + k * 22, t, 'tiny'))
  })
  add('p11', 'p11-degraded-states.svg', 'P-11 降级与异常态', W, H, b,
    ['降级不阻断：占位卡保留引用与反链链路',
      '只读与占用场景提供「另存副本」出口',
      '索引重建期间搜索结果标注覆盖范围'])
}

fs.writeFileSync(path.join(OUT, '.pages.json'), JSON.stringify(pages, null, 2))
console.log('generated', pages.length, 'wireframes ->', SVG_DIR)
