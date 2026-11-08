/**
 * Migrant Knowledge Vault —— Web 端真实浏览器交互回归
 *
 * 驱动真实 Microsoft Edge（headless）打开 dx serve 的页面，执行点击 / 输入 / 页签切换，
 * 验证「构建期内嵌的预构建只读索引」在真实浏览器里可检索、可预览、可看双链。
 *
 * 运行：
 *   NODE_PATH=<playwright 所在 node_modules> node scripts/browser-check.cjs
 *   MKV_URL=http://127.0.0.1:8080 node scripts/browser-check.cjs
 *
 * 依赖：npm i playwright（任一可解析路径，脚本会自动探测）
 * 产物：docs/browser-validation/*.png + report.json
 */
'use strict';

const fs = require('fs');
const path = require('path');

const ROOT = path.resolve(__dirname, '..');
const URL = process.env.MKV_URL || 'http://127.0.0.1:8080';
const SHOT_DIR = process.env.MKV_SHOT_DIR || path.join(ROOT, 'docs', 'browser-validation');
const INDEX_JSON = path.join(ROOT, 'packages', 'web', 'assets', 'mkv-index.json');

function loadPlaywright() {
  const temp = process.env.TEMP || process.env.TMP || '';
  const candidates = [
    process.env.MKV_PLAYWRIGHT_DIR,
    temp && path.join(temp, 'mkv-browser-check', 'node_modules', 'playwright'),
    'playwright',
  ].filter(Boolean);
  for (const p of candidates) {
    try {
      return require(p);
    } catch (_) {
      /* 尝试下一个候选 */
    }
  }
  throw new Error('未找到 playwright：请先执行 npm i playwright，或用 MKV_PLAYWRIGHT_DIR 指定');
}

const { chromium } = loadPlaywright();
const index = JSON.parse(fs.readFileSync(INDEX_JSON, 'utf8'));

// 检索词优先用领域词典词（df 高、跨文档），产物变化时自动回退到任意真实词
let keyword = '模流分析';
if (!index.postings.some((p) => p.token === keyword)) {
  const alt = index.postings.find(
    (p) => p.docs.length >= 2 && /[\u4e00-\u9fff]/.test(p.token) && p.token.length >= 2
  );
  keyword = alt ? alt.token : index.postings[0].token;
}

// 第二个真实检索词，用于「再次回车检索」
const alt = index.postings.find(
  (p) => p.token !== keyword && p.docs.length >= 2 && /[\u4e00-\u9fff]/.test(p.token) && p.token.length >= 2
);
const keyword2 = alt ? alt.token : keyword;
const noMatch = '___mkv_nonexistent_term___';

// 反链目标：产物中入链最多的 to_path
const inbound = {};
for (const l of index.links) inbound[l.to_path] = (inbound[l.to_path] || 0) + 1;
const backlinkTarget = Object.entries(inbound).sort((a, b) => b[1] - a[1])[0] || [null, 0];
const expectedDocs = index.docs.length;

const checks = [];
function check(name, ok, detail) {
  checks.push({ name, ok: !!ok, detail: String(detail) });
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}  ::  ${detail}`);
}

async function shot(page, name) {
  const p = path.join(SHOT_DIR, name);
  await page.screenshot({ path: p, fullPage: true });
  console.log(`  -> 截图 ${p}`);
  return p;
}

(async () => {
  fs.mkdirSync(SHOT_DIR, { recursive: true });
  const pageErrors = [];
  const consoleErrors = [];
  const screenshots = [];
  const started = Date.now();
  let browser;
  let fatal = null;

  try {
    const launchOpts = { headless: true, args: ['--no-sandbox', '--disable-dev-shm-usage'] };
    // 本机优先用系统 Edge（免下载）；CI 用 MKV_BROWSER_CHANNEL=chromium
    // 配合 `npx playwright install chromium`；最后回退 Windows Edge 绝对路径。
    const channels = [
      process.env.MKV_BROWSER_CHANNEL,
      'msedge',
      'chromium',
    ].filter(Boolean);
    for (const channel of channels) {
      try {
        browser = await chromium.launch({ ...launchOpts, channel });
        console.log(`浏览器：channel=${channel}`);
        break;
      } catch (e) {
        console.log(`channel=${channel} 不可用：${String(e.message).split('\n')[0]}`);
      }
    }
    if (!browser) {
      browser = await chromium.launch({
        ...launchOpts,
        executablePath: 'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
      });
      console.log('浏览器：系统 Edge 绝对路径');
    }

    const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
    page.on('pageerror', (e) => pageErrors.push(String(e)));
    page.on('console', (m) => {
      if (m.type() === 'error') consoleErrors.push(m.text());
    });

    await page.goto(URL, { waitUntil: 'domcontentloaded', timeout: 60000 });
    await page.waitForSelector('.mkv-app', { timeout: 90000 });
    await page.waitForFunction(
      (n) => document.querySelectorAll('.mkv-result').length >= n,
      expectedDocs,
      { timeout: 30000 }
    );

    // A1 首屏：内嵌索引是否随 wasm 一起生效
    const docCount = await page.locator('.mkv-result').count();
    check('A1 首屏渲染全部文档（内嵌索引生效）', docCount === expectedDocs, `${docCount} / ${expectedDocs} 篇`);

    // A2 顶栏：索引来源与知识库名
    const status0 = (await page.locator('.mkv-topbar-status').innerText()).trim();
    const root0 = (await page.locator('.mkv-topbar-root').innerText()).trim();
    check('A2 顶栏显示内嵌索引与知识库名', status0.includes('内嵌产物') && root0.length > 0, `status="${status0}" root="${root0}"`);

    // A3 六格式文档都在列表里
    const formats = await page.locator('.mkv-result-meta > span:first-child').allInnerTexts();
    const uniq = [...new Set(formats.map((s) => s.trim()))].filter(Boolean);
    check('A3 多格式文档均在列表', uniq.length >= 6, uniq.join(', '));
    screenshots.push(await shot(page, '01-first-paint.png'));

    // A4/A5 点击文档 → 预览区切换 + 正文非空
    await page.locator('.mkv-result').nth(2).click();
    await page.waitForSelector('.mkv-preview-header h2', { timeout: 10000 });
    await page.waitForFunction(
      () => {
        const el = document.querySelector('.mkv-doc-content');
        return el && el.textContent.trim().length > 0;
      },
      null,
      { timeout: 10000 }
    );
    const previewTitle = (await page.locator('.mkv-preview-header h2').innerText()).trim();
    const previewLen = (await page.locator('.mkv-doc-content').innerText()).trim().length;
    check('A4 点击文档切换预览', previewTitle.length > 0 && /\./.test(previewTitle), `title="${previewTitle}"`);
    check('A5 预览正文非空', previewLen > 0, `${previewLen} 字符`);

    // A6/A7 Web 端只读语义：不得出现保存不了的「内置编辑」按钮
    const actionText = (await page.locator('.mkv-preview-actions').innerText()).trim();
    const editButtons = await page.locator('.mkv-preview-actions button').count();
    check('A6 Web 端无内置编辑按钮', editButtons === 0, `按钮数=${editButtons}`);
    check('A7 显示只读提示', actionText.includes('Web 端为只读索引'), `提示="${actionText}"`);

    // A8 双链反链：点击产物中入链最多的文档
    if (backlinkTarget[0]) {
      const targetItem = page.locator('.mkv-result').filter({ hasText: backlinkTarget[0] });
      await targetItem.first().click();
      await page.waitForFunction(
        () => document.querySelectorAll('.mkv-backlink').length > 0,
        null,
        { timeout: 10000 }
      );
      const bl = await page.locator('.mkv-backlink').count();
      check('A8 双链反链面板', bl >= 1, `目标=${backlinkTarget[0]}，界面 ${bl} 条 / 产物 ${backlinkTarget[1]} 条入链`);
      screenshots.push(await shot(page, '02-document-backlinks.png'));
    }

    // A9~A11 中文检索：输入后按回车触发（搜索按钮已取消）
    const input = page.locator('.mkv-search-input');
    await input.fill(keyword);
    await input.press('Enter');
    await page.waitForFunction(
      () => {
        const t = document.querySelector('.mkv-tab-active');
        return t && t.textContent.includes('检索结果');
      },
      null,
      { timeout: 10000 }
    );
    const hits = await page.locator('.mkv-result').count();
    const searchStatus = (await page.locator('.mkv-topbar-status').innerText()).trim();
    check('A9 回车触发中文检索并命中', hits > 0, `"${keyword}" → ${hits} 条`);
    check('A10 检索后自动切结果页签', true, '已切到「检索结果」');
    check('A11 状态栏命中反馈', searchStatus.includes('命中'), searchStatus);
    screenshots.push(await shot(page, '03-search-results.png'));

    // A12 点击检索结果打开文档
    const firstHitTitle = (await page.locator('.mkv-result-title').first().innerText()).trim();
    await page.locator('.mkv-result').first().click();
    await page.waitForFunction(
      () => {
        const el = document.querySelector('.mkv-preview-header h2');
        return el && el.textContent.trim().length > 0;
      },
      null,
      { timeout: 10000 }
    );
    const hitPreview = (await page.locator('.mkv-preview-header h2').innerText()).trim();
    check('A12 点击检索结果打开文档', hitPreview.length > 0, `命中="${firstHitTitle}" → 预览="${hitPreview}"`);

    // A13 搜索按钮已取消，输入框有回车引导
    const searchButtons = await page.locator('.mkv-searchbar button').count();
    const placeholder = (await input.getAttribute('placeholder')) || '';
    check(
      'A13 检索区无搜索按钮且提示回车',
      searchButtons === 0 && placeholder.includes('回车'),
      `按钮数=${searchButtons}，placeholder="${placeholder}"`
    );

    // A14 仅输入、不按回车 → 不应触发检索（负向断言）
    await input.fill(noMatch);
    await page.waitForTimeout(350);
    const passiveHits = await page.locator('.mkv-result').count();
    const passiveStatus = (await page.locator('.mkv-topbar-status').innerText()).trim();
    check(
      'A14 仅输入不触发检索',
      passiveHits === hits && passiveStatus === searchStatus,
      `未回车：结果 ${passiveHits} 条（应保持 ${hits}），status="${passiveStatus}"`
    );

    // A15 再次回车检索（换真实关键词）
    await input.fill(keyword2);
    await input.press('Enter');
    await page.waitForFunction(
      (kw) => {
        const s = document.querySelector('.mkv-topbar-status');
        return s && s.textContent.includes(kw);
      },
      keyword2,
      { timeout: 10000 }
    );
    await page.waitForTimeout(150);
    const secondHits = await page.locator('.mkv-result').count();
    const secondStatus = (await page.locator('.mkv-topbar-status').innerText()).trim();
    check(
      'A15 二次回车检索生效',
      secondHits > 0 && secondStatus.includes(keyword2),
      `"${keyword2}" → ${secondHits} 条，status="${secondStatus}"`
    );

    // A16 清空条件后回车 → 回到「全部文档」
    await input.fill('');
    await input.press('Enter');
    await page.waitForFunction(
      (n) => document.querySelectorAll('.mkv-result').length === n,
      expectedDocs,
      { timeout: 10000 }
    );
    const resetActive = (await page.locator('.mkv-tab-active').innerText()).trim();
    check('A16 清空回车恢复全部文档', resetActive.includes('全部文档'), `当前页签="${resetActive}"`);

    // A17 Tab 切回全部文档（显式点击路径）
    await page.locator('.mkv-tab').filter({ hasText: '检索结果' }).click();
    await page.locator('.mkv-tab').filter({ hasText: '全部文档' }).click();
    await page.waitForFunction(
      (n) => document.querySelectorAll('.mkv-result').length === n,
      expectedDocs,
      { timeout: 10000 }
    );
    const back = await page.locator('.mkv-result').count();
    check('A17 Tab 切换正常', back === expectedDocs, `${back} 篇`);

    // A18 未捕获的 JS 异常（console error 单列，外网字体等网络失败不算失败）
    check('A18 无未捕获 JS 异常', pageErrors.length === 0, pageErrors.length ? pageErrors.join(' | ') : '0 个');
  } catch (e) {
    fatal = String(e && e.stack ? e.stack : e);
    console.error('浏览器验证中断：', fatal);
  } finally {
    if (browser) await browser.close();
  }

  const passed = !fatal && checks.length > 0 && checks.every((c) => c.ok);
  const report = {
    url: URL,
    browser: 'Microsoft Edge (headless, Playwright)',
    keyword,
    keyword2,
    searchTrigger: 'Enter（无搜索按钮）',
    expectedDocs,
    backlinkTarget: { path: backlinkTarget[0], inbound: backlinkTarget[1] },
    passed,
    checks,
    pageErrors,
    consoleErrors,
    fatal,
    durationMs: Date.now() - started,
    screenshots,
  };
  fs.writeFileSync(path.join(SHOT_DIR, 'report.json'), JSON.stringify(report, null, 2), 'utf8');

  const okCount = checks.filter((c) => c.ok).length;
  console.log(`\n===== 浏览器验证：${okCount}/${checks.length} 项通过，结果 ${passed ? 'PASS' : 'FAIL'} =====`);
  console.log(`报告：${path.join(SHOT_DIR, 'report.json')}`);
  if (consoleErrors.length) console.log(`console error（非致命）：${consoleErrors.length} 条`);
  process.exitCode = passed ? 0 : 1;
})();
