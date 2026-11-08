/**
 * 远端索引热替换 —— 真实浏览器实测。
 *
 * 前置：用编译期常量启动 dx serve，例如
 *   MKV_WEB_INDEX_URL=/remote-index.json dx serve --web --package mkv-web \
 *     --port 8081 --addr 127.0.0.1 --open false --watch false \
 *     --hot-reload false --interactive false
 *
 * 本脚本用 Playwright 的路由拦截伪造远端响应，无需真的部署索引服务：
 *   1. 远端返回有效产物 → 应热替换（文档数与知识库名改变、状态栏标「远端产物」）
 *   2. 远端返回 HTTP 500 → 应静默回落内嵌产物
 *   3. 远端返回空索引 → 应视为无效并回落内嵌产物
 *
 * 运行：
 *   MKV_URL=http://127.0.0.1:8081 node scripts/browser-remote-check.cjs
 * 产物：docs/browser-validation/remote-report.json
 */
'use strict';

const fs = require('fs');
const path = require('path');

const ROOT = path.resolve(__dirname, '..');
const URL = process.env.MKV_URL || 'http://127.0.0.1:8081';
const SHOT_DIR = path.join(ROOT, 'docs', 'browser-validation');
const INDEX_JSON = path.join(ROOT, 'packages', 'web', 'assets', 'mkv-index.json');
const REMOTE_PATH = '/remote-index.json';

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
      /* 尝试下一个 */
    }
  }
  throw new Error('未找到 playwright');
}

const { chromium } = loadPlaywright();
const index = JSON.parse(fs.readFileSync(INDEX_JSON, 'utf8'));
const embeddedDocs = index.docs.length;

// 伪造的「更新版」产物：只保留 2 篇，并改知识库名，便于确认热替换真的发生
const remoteIndex = {
  ...index,
  root: 'remote-updated-vault',
  docs: index.docs.slice(0, 2),
};
const remoteJson = JSON.stringify(remoteIndex);
const emptyJson = JSON.stringify({
  schema_version: 1,
  generated_at: 0,
  root: '',
  docs: [],
  links: [],
  postings: [],
});

const checks = [];
function check(name, ok, detail) {
  checks.push({ name, ok: !!ok, detail: String(detail) });
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}  ::  ${detail}`);
}

async function launch() {
  const opts = { headless: true, args: ['--no-sandbox', '--disable-dev-shm-usage'] };
  const channels = [
    process.env.MKV_BROWSER_CHANNEL,
    'msedge',
    'chromium',
  ].filter(Boolean);
  for (const channel of channels) {
    try {
      const b = await chromium.launch({ ...opts, channel });
      console.log(`浏览器：channel=${channel}`);
      return b;
    } catch (_) {
      /* 尝试下一个候选 */
    }
  }
  return chromium.launch({
    ...opts,
    executablePath: 'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
  });
}

/** 打开一个页面，按 mode 拦截远端请求，返回观测到的界面状态。 */
async function observe(browser, mode) {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  const pageErrors = [];
  page.on('pageerror', (e) => pageErrors.push(String(e)));

  await page.route(`**${REMOTE_PATH}`, async (route) => {
    if (mode === 'valid') {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        headers: { 'access-control-allow-origin': '*' },
        body: remoteJson,
      });
    } else if (mode === 'error') {
      await route.fulfill({ status: 500, body: 'index service down' });
    } else if (mode === 'empty') {
      await route.fulfill({ status: 200, contentType: 'application/json', body: emptyJson });
    } else {
      await route.abort('failed');
    }
  });

  await page.goto(URL, { waitUntil: 'domcontentloaded', timeout: 60000 });
  await page.waitForSelector('.mkv-app', { timeout: 90000 });
  await page.waitForFunction(
    (n) => document.querySelectorAll('.mkv-result').length >= 1,
    embeddedDocs,
    { timeout: 30000 }
  );

  // 等热替换的异步 future 完成（状态栏会带上「远端产物」）
  if (mode === 'valid') {
    await page.waitForFunction(
      () => {
        const s = document.querySelector('.mkv-topbar-status');
        return s && s.textContent.includes('远端产物');
      },
      null,
      { timeout: 15000 }
    );
  } else {
    await page.waitForTimeout(900);
  }

  const status = (await page.locator('.mkv-topbar-status').innerText()).trim();
  const root = (await page.locator('.mkv-topbar-root').innerText()).trim();
  const docCount = await page.locator('.mkv-result').count();
  const shot = path.join(SHOT_DIR, `remote-${mode}.png`);
  await page.screenshot({ path: shot, fullPage: true });
  await page.close();
  return { status, root, docCount, pageErrors, shot };
}

(async () => {
  fs.mkdirSync(SHOT_DIR, { recursive: true });
  const started = Date.now();
  let browser;
  let fatal = null;
  const observations = {};

  try {
    browser = await launch();

    const valid = await observe(browser, 'valid');
    observations.valid = valid;
    check(
      'R1 远端有效产物触发热替换',
      valid.status.includes('远端产物') && valid.docCount === remoteIndex.docs.length,
      `status="${valid.status}" 文档=${valid.docCount}（远端应 ${remoteIndex.docs.length}）`
    );
    check(
      'R2 热替换后知识库名同步更新',
      valid.root === 'remote-updated-vault',
      `root="${valid.root}"`
    );

    const error = await observe(browser, 'error');
    observations.error = error;
    check(
      'R3 远端 HTTP 500 静默回落内嵌产物',
      error.status.includes('内嵌产物') && error.docCount === embeddedDocs,
      `status="${error.status}" 文档=${error.docCount}/${embeddedDocs}`
    );

    const empty = await observe(browser, 'empty');
    observations.empty = empty;
    check(
      'R4 远端空索引视为无效并回落',
      empty.status.includes('内嵌产物') && empty.docCount === embeddedDocs,
      `status="${empty.status}" 文档=${empty.docCount}/${embeddedDocs}`
    );

    const errors = [
      ...valid.pageErrors,
      ...error.pageErrors,
      ...empty.pageErrors,
    ];
    check('R5 三个用例均无未捕获 JS 异常', errors.length === 0, errors.join(' | ') || '0 个');
  } catch (e) {
    fatal = String(e && e.stack ? e.stack : e);
    console.error('远端热替换验证中断：', fatal);
  } finally {
    if (browser) await browser.close();
  }

  const passed = !fatal && checks.length > 0 && checks.every((c) => c.ok);
  const report = {
    url: URL,
    remotePath: REMOTE_PATH,
    embeddedDocs,
    remoteDocs: remoteIndex.docs.length,
    passed,
    checks,
    observations,
    fatal,
    durationMs: Date.now() - started,
  };
  fs.writeFileSync(path.join(SHOT_DIR, 'remote-report.json'), JSON.stringify(report, null, 2), 'utf8');
  console.log(`\n===== 远端热替换验证：${checks.filter((c) => c.ok).length}/${checks.length} 项通过，${passed ? 'PASS' : 'FAIL'} =====`);
  console.log(`报告：${path.join(SHOT_DIR, 'remote-report.json')}`);
  process.exitCode = passed ? 0 : 1;
})();
