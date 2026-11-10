// XML 合法性校验 + 生成 PNG 预览（临时工具）
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { JSDOM } from 'jsdom'

const dir = process.env.MKV_DESIGN_DIR ? path.join(process.env.MKV_DESIGN_DIR, 'svg') : path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', 'svg')
const dom = new JSDOM('', { contentType: 'text/html' })
const parser = new dom.window.DOMParser()
let bad = 0
for (const f of fs.readdirSync(dir).filter(x => x.endsWith('.svg'))) {
  const txt = fs.readFileSync(path.join(dir, f), 'utf8')
  const doc = parser.parseFromString(txt, 'image/svg+xml')
  const err = doc.querySelector('parsererror')
  if (err) {
    bad++
    console.log(`XML-FAIL ${f}: ${err.textContent.slice(0, 200)}`)
  } else {
    const svg = doc.documentElement
    console.log(`OK ${f} viewBox=${svg.getAttribute('viewBox')} nodes=${doc.querySelectorAll('*').length}`)
  }
}
console.log(bad === 0 ? 'ALL XML OK' : `FAILED ${bad}`)
process.exit(bad ? 1 : 0)
