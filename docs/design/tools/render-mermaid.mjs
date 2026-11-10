// 将 Markdown 中的 mermaid 代码块批量渲染为独立 SVG（临时构建工具）
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import puppeteer from 'puppeteer'

const OUT = process.env.MKV_DESIGN_DIR || path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const SVG_DIR = path.join(OUT, 'svg')
const MERMAID_JS = path.resolve(path.dirname(fileURLToPath(import.meta.url)), 'node_modules/mermaid/dist/mermaid.min.js')
const SOURCES = [
  { md: path.join(OUT, '01-原型图.md'), tag: 'proto' },
  { md: path.join(OUT, '02-详细设计.md'), tag: 'dd' },
  { md: path.join(OUT, 'README.md'), tag: 'readme' },
]

function extract(mdPath) {
  const lines = fs.readFileSync(mdPath, 'utf8').split(/\r?\n/)
  let inB = false, buf = [], heading = '', start = 0
  const out = []
  lines.forEach((ln, i) => {
    const h = /^#{1,6}\s+(.*)$/.exec(ln)
    if (h) heading = h[1].trim()
    if (!inB && ln.trim() === '```mermaid') { inB = true; buf = []; start = i + 2; return }
    if (inB && ln.trim() === '```') { inB = false; out.push({ heading, start, code: buf.join('\n') }); return }
    if (inB) buf.push(ln)
  })
  return out
}

const themeVars = {
  fontFamily: "'Roobert PRO Medium','Noto Sans','Segoe UI','Microsoft YaHei',sans-serif",
  fontSize: '14px',
  primaryColor: '#eef1ff',
  primaryTextColor: '#1c1c1e',
  primaryBorderColor: '#5b76fe',
  secondaryColor: '#c3faf5',
  secondaryBorderColor: '#187574',
  tertiaryColor: '#ffe6cd',
  tertiaryBorderColor: '#e8853a',
  lineColor: '#8a90a0',
  background: '#ffffff',
  mainBkg: '#ffffff',
  secondBkg: '#f7f8fb',
  nodeBorder: '#c7cad5',
  nodeTextColor: '#1c1c1e',
  clusterBkg: '#fbfbfd',
  clusterBorder: '#e9eaef',
  edgeLabelBackground: '#ffffff',
  titleColor: '#1c1c1e',
  actorBkg: '#ffffff',
  actorBorder: '#c7cad5',
  actorTextColor: '#1c1c1e',
  actorLineColor: '#8a90a0',
  signalColor: '#555a6a',
  signalTextColor: '#1c1c1e',
  labelBoxBkgColor: '#ffffff',
  labelBoxBorderColor: '#c7cad5',
  labelTextColor: '#1c1c1e',
  noteBkgColor: '#fff6e5',
  noteBorderColor: '#e8c98a',
  noteTextColor: '#1c1c1e',
  stateBkg: '#ffffff',
  stateBorder: '#5b76fe',
  stateLabelColor: '#1c1c1e',
  stateTitleColor: '#5b76fe',
  dividerColor: '#e9eaef',
  attributeBackgroundColorOdd: '#f7f8fb',
  attributeBackgroundColorEven: '#ffffff',
}

const browser = await puppeteer.launch({
  executablePath: process.env.MKV_CHROME || 'C:\\\\Program Files\\\\Google\\\\Chrome\\\\Application\\\\chrome.exe',
  headless: 'new',
  args: ['--no-sandbox', '--disable-gpu', '--font-render-hinting=none'],
})
const page = await browser.newPage()
await page.setContent('<!DOCTYPE html><html><head><meta charset="utf-8"></head><body><div id="host"></div></body></html>')
await page.addScriptTag({ path: MERMAID_JS })

const manifest = []
let n = 0
for (const src of SOURCES) {
  const blocks = extract(src.md)
  for (const b of blocks) {
    n++
    const type = b.code.split('\n')[0].trim().split(' ')[0]
    const kind = type.startsWith('sequence') ? 'seq' : type.startsWith('state') ? 'state' : type.startsWith('er') ? 'er' : 'flow'
    const id = `fig-${String(n).padStart(2, '0')}-${kind}`
    const file = `${id}.svg`
    let svg = ''
    try {
      svg = await page.evaluate(async (code, vars) => {
        mermaid.initialize({ startOnLoad: false, securityLevel: 'loose', theme: 'base', themeVariables: vars })
        const { svg } = await mermaid.render('g' + Math.random().toString(36).slice(2), code)
        return svg
      }, b.code, themeVars)
    } catch (e) {
      console.log(`RENDER-FAIL ${file} (${src.md} :${b.start}) ${String(e.message || e).slice(0, 120)}`)
      continue
    }
    if (!/<svg[^>]*xmlns=/.test(svg)) svg = svg.replace('<svg', '<svg xmlns="http://www.w3.org/2000/svg"')
    fs.writeFileSync(path.join(SVG_DIR, file), `<?xml version="1.0" encoding="UTF-8"?>\n${svg}\n`)
    manifest.push({
      id, file, kind, heading: b.heading, source: path.basename(src.md), line: b.start,
      title: b.heading || id,
    })
    console.log(`OK ${file}  ${src.tag} :: ${b.heading}`)
  }
}
await browser.close()
fs.writeFileSync(path.join(OUT, '.figs.json'), JSON.stringify(manifest, null, 2))
console.log(`\nrendered ${manifest.length} diagrams`)
