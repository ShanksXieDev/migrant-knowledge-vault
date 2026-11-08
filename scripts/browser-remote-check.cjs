/**
 * 远端索引热替换 + 站点功能回归 —— 真实浏览器实测。
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
 *   4. 远端不可达（连接中止）→ 同样回落内嵌产物，并在该页面继续跑功能回归
 *
 * 共 15 项断言，分三组：
 * - R1–R5：远端索引热替换（热替换生效 / 知识库名同步 / 500 回落 / 空索引回落 /
 *   各用例均无未捕获 JS 异常）。
 * - G1–G4：F-10 双链关系图谱 **V2**（渲染 / 缩放 / 格式图例 / 点选跳回预览）。
 *   V2 的类名是 `mkv-graph-v2-*`（`mkv-graph-v2-svg` / `-node` / `-edge` / `-tools`），
 *   旧的 `mkv-graph-svg` / `mkv-graph-tools` 已随产品下线，断言必须用 v2 选择器。
 * - D1–D6：F-12 正则检索（模式页签 / 正则命中 / 非法正则报错）与
 *   F-07 多维过滤（格式候选 / 过滤需回车重跑 / 清空过滤）。
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

// F-10 图谱的期望值（与 core 的 graph_topology 同语义）：
// - 节点：全部文档去重后的路径数（graph_snapshot 用 docs 建节点表）；
// - 边：剔除自环、悬空端点与重复 (from,to) 后的链接数。
const docPathSet = new Set(index.docs.map((d) => d.path));
const expectedGraphNodes = docPathSet.size;
const edgePairs = new Set();
for (const l of index.links) {
  if (!l.to_path || l.from_path === l.to_path) continue;
  if (!docPathSet.has(l.from_path) || !docPathSet.has(l.to_path)) continue;
  edgePairs.add(`${l.from_path}\u0000${l.to_path}`);
}
const expectedEdges = edgePairs.size;

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

/**
 * 打开一个页面并按 mode 拦截远端请求。
 *
 * mode：
 * - valid  → 远端返回伪造的有效产物（2 篇）→ 应触发热替换
 * - error  → 远端返回 HTTP 500 → 应回落内嵌产物
 * - empty  → 远端返回空索引 → 应回落内嵌产物
 * - offline→ 远端连接中止 → 应回落内嵌产物（功能回归就跑在这一页）
 */
async function openPage(browser, mode) {
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

  return { page, pageErrors };
}

/** 打开一个页面，按 mode 拦截远端请求，返回观测到的界面状态。 */
async function observe(browser, mode) {
  const { page, pageErrors } = await openPage(browser, mode);

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

/**
 * 在「远端不可达 → 回落内嵌产物」的页面上跑功能回归（F-10 图谱 V2 + F-12/F-07）。
 *
 * 这一页用的是**完整内嵌产物**（与本地回归同一份索引），因此图谱节点/边口径
 * 与格式候选都有实测值可对照，不受「伪造远端产物只有 2 篇」的影响。
 */
async function observeFeatures(browser) {
  const { page, pageErrors } = await openPage(browser, 'offline');

  await page.goto(URL, { waitUntil: 'domcontentloaded', timeout: 60000 });
  await page.waitForSelector('.mkv-app', { timeout: 90000 });
  await page.waitForFunction(
    (n) => document.querySelectorAll('.mkv-result').length >= n,
    embeddedDocs,
    { timeout: 30000 }
  );

  const topStatus = async () => (await page.locator('.mkv-topbar-status').innerText()).trim();
  const modeTab = (text) => page.locator('.mkv-mode-tab').filter({ hasText: text });
  const resultFormats = async () =>
    (await page.locator('.mkv-result-meta > span:first-child').allInnerTexts()).map((s) => s.trim());
  const input = page.locator('.mkv-search-input');
  const waitHit = () =>
    page.waitForFunction(
      () => {
        const s = document.querySelector('.mkv-topbar-status');
        return s && s.textContent.includes('命中');
      },
      null,
      { timeout: 10000 }
    );

  // ---------- F-12 正则检索 ----------
  await modeTab('正则').click();
  await page.waitForTimeout(200);
  const regexTabClass = (await modeTab('正则').getAttribute('class')) || '';
  const termsTabClass = (await modeTab('普通词条').getAttribute('class')) || '';

  // `\w+` 与语料无关：任何含词字符的文档都能命中，
  // 因此线上站点的语料变化不会让这条断言失效。
  await input.fill('\\w+');
  await input.press('Enter');
  await waitHit();
  const regexHits = await page.locator('.mkv-result').count();
  const regexStatus = await topStatus();
  const regexErrors = await page.locator('.mkv-search-error').count();

  await input.fill('[');
  await input.press('Enter');
  await page.waitForTimeout(300);
  const badErrorCount = await page.locator('.mkv-search-error').count();
  const badErrorText = badErrorCount
    ? (await page.locator('.mkv-search-error').first().innerText()).trim()
    : '';
  const badHits = await page.locator('.mkv-result').count();
  const badStatus = await topStatus();
  // 恢复初始状态：切回「普通词条」，避免污染后续用例
  await modeTab('普通词条').click();
  await page.waitForTimeout(200);
  const backTabClass = (await modeTab('普通词条').getAttribute('class')) || '';
  const backErrors = await page.locator('.mkv-search-error').count();

  // ---------- F-07 多维过滤 ----------
  const filterPanel = page.locator('details.mkv-filter-panel');
  if (!(await filterPanel.evaluate((el) => el.open))) {
    await page.locator('.mkv-filter-summary').click();
    await page.waitForTimeout(200);
  }
  const panelOpen = await filterPanel.evaluate((el) => el.open);
  const formatGroup = page
    .locator('.mkv-filter-group')
    .filter({ has: page.locator('.mkv-filter-label', { hasText: '格式' }) });
  const formatChips = await formatGroup.locator('.mkv-chips .mkv-chip').allInnerTexts();

  // 基线结果集：用正则 `\w+` 命中全库（与语料无关），再按格式过滤。
  await modeTab('正则').click();
  await input.fill('\\w+');
  await input.press('Enter');
  await waitHit();
  const baseHits = await page.locator('.mkv-result').count();
  const baseFormats = await resultFormats();
  const fmtCount = {};
  for (const f of baseFormats) fmtCount[f] = (fmtCount[f] || 0) + 1;
  // 选命中最多的格式：勾它必然让结果集变小，过滤是否生效才可观测
  const targetFmt = Object.entries(fmtCount).sort((a, b) => b[1] - a[1])[0][0];
  const expectFmtHits = fmtCount[targetFmt];
  await formatGroup.locator('.mkv-chips .mkv-chip').filter({ hasText: targetFmt }).first().click();
  await page.waitForTimeout(200);
  const hitsBeforeEnter = await page.locator('.mkv-result').count();
  const chipOn = await page.locator('.mkv-chip-on').count();
  const filterHint = await topStatus();
  await input.press('Enter');
  await waitHit();
  const filteredHits = await page.locator('.mkv-result').count();
  const filteredFormats = await resultFormats();

  await page.locator('.mkv-filter-actions button').filter({ hasText: '清空过滤' }).click();
  await page.waitForTimeout(600);
  const clearedChips = await page.locator('.mkv-chip-on').count();
  const restoredHits = await page.locator('.mkv-result').count();
  const restoredFormats = await resultFormats();

  // ---------- F-10 图谱 V2 ----------
  await page.locator('.mkv-tab').filter({ hasText: '关系图谱' }).click();
  await page.waitForSelector('.mkv-graph-v2-svg', { timeout: 10000 });
  const graphNodes = await page.locator('.mkv-graph-v2-node').count();
  const graphEdges = await page.locator('.mkv-graph-v2-edge').count();
  const graphStats = (await page.locator('.mkv-graph-v2-stats').innerText()).trim();
  const graphStatus = (await page.locator('.mkv-graph-v2-status').innerText()).trim();
  const legendCount = await page.locator('.mkv-graph-v2-legend').count();
  const legendItems = await page.locator('.mkv-graph-v2-legend .mkv-graph-v2-legend-item').count();
  const legendTitle = (await page.locator('.mkv-graph-v2-legend-title').innerText()).trim();

  const graphGroup = page.locator('.mkv-graph-v2-svg > g');
  const transformBefore = await graphGroup.getAttribute('transform');
  await page.locator('.mkv-graph-v2-tools button').filter({ hasText: '放大' }).click();
  await page.waitForTimeout(150);
  const transformAfter = await graphGroup.getAttribute('transform');

  // 点节点：命中标签 pointer-events:none，落到圆点上才是可点的图形本体
  await page.locator('.mkv-graph-v2-node').first().locator('circle').click();
  await page.waitForSelector('.mkv-preview-header h2', { timeout: 10000 });
  const activeTab = (await page.locator('.mkv-tab-active').innerText()).trim();
  const previewTitle = (await page.locator('.mkv-preview-header h2').innerText()).trim();

  const shot = path.join(SHOT_DIR, 'remote-features.png');
  await page.screenshot({ path: shot, fullPage: true });
  await page.close();

  return {
    pageErrors,
    shot,
    regex: {
      regexTabClass,
      termsTabClass,
      regexHits,
      regexStatus,
      regexErrors,
      badErrorCount,
      badErrorText,
      badHits,
      badStatus,
      backTabClass,
      backErrors,
    },
    filter: {
      panelOpen,
      formatChips,
      baseHits,
      fmtCount,
      targetFmt,
      expectFmtHits,
      hitsBeforeEnter,
      chipOn,
      filterHint,
      filteredHits,
      filteredFormats,
      clearedChips,
      restoredHits,
      restoredFormats,
    },
    graph: {
      graphNodes,
      graphEdges,
      graphStats,
      graphStatus,
      legendCount,
      legendItems,
      legendTitle,
      transformBefore,
      transformAfter,
      activeTab,
      previewTitle,
    },
  };
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

    // ---------- 回落页面上的功能回归：图谱 V2 + 正则检索 + 多维过滤 ----------
    const feat = await observeFeatures(browser);
    observations.features = feat;

    const g = feat.graph;
    const rendered = /已渲染\s*(\d+)\s*\/\s*共\s*(\d+)/.exec(g.graphStatus);
    const renderedCount = rendered ? Number(rendered[1]) : -1;
    const totalCount = rendered ? Number(rendered[2]) : -1;
    const statEdges = Number((/(\d+)\s*条边/.exec(g.graphStats) || [])[1]);
    // 节点数超过阈值时只渲染视口内节点，DOM 里的节点数可能少于文档总数
    const culled = renderedCount >= 0 && renderedCount < totalCount;
    check(
      'G1 关系图谱渲染节点与边',
      g.graphNodes > 0 &&
        g.graphNodes <= expectedGraphNodes &&
        renderedCount === g.graphNodes &&
        totalCount === expectedGraphNodes &&
        g.graphEdges > 0 &&
        (culled ? g.graphEdges <= statEdges : g.graphEdges === statEdges),
      `${g.graphNodes} 节点 / ${g.graphEdges} 边；状态「${g.graphStatus}」，统计「${g.graphStats}」` +
        `（内嵌产物 ${embeddedDocs} 篇 → 期望 ${expectedGraphNodes} 节点 / ${expectedEdges} 边）`
    );
    check('G2 图谱缩放生效', g.transformBefore !== g.transformAfter, `${g.transformBefore} → ${g.transformAfter}`);
    check(
      'G3 图谱格式图例存在',
      g.legendCount === 1 && g.legendItems > 0 && g.legendTitle.includes('图例'),
      `图例容器 ${g.legendCount} 个 / 条目 ${g.legendItems} 项，标题「${g.legendTitle}」`
    );
    check(
      'G4 点选节点跳回文档预览',
      g.activeTab.includes('全部文档') && g.previewTitle.length > 0,
      `当前页签=${g.activeTab}，预览="${g.previewTitle}"`
    );

    const r = feat.regex;
    check(
      'D1 检索模式页签切换到正则',
      r.regexTabClass.includes('mkv-mode-active') && !r.termsTabClass.includes('mkv-mode-active'),
      `正则="${r.regexTabClass}" 普通词条="${r.termsTabClass}"`
    );
    check(
      'D2 正则模式检索命中且无错误提示',
      r.regexHits > 0 &&
        r.regexErrors === 0 &&
        r.regexStatus.includes('命中') &&
        r.regexStatus.includes('\\w+') &&
        r.regexStatus.includes(String(r.regexHits)),
      `正则「\\w+」→ ${r.regexHits} 条，status="${r.regexStatus}"，错误提示 ${r.regexErrors} 处`
    );
    check(
      'D3 非法正则显示「正则无效」而非零命中，切回普通词条后错误消失',
      r.badErrorCount >= 1 &&
        r.badErrorText.startsWith('正则无效') &&
        r.badHits === 0 &&
        r.badStatus.includes('正则无效') &&
        !r.badStatus.includes('命中') &&
        r.backTabClass.includes('mkv-mode-active') &&
        r.backErrors === 0,
      `错误="${r.badErrorText}"，结果 ${r.badHits} 条，status="${r.badStatus}"；` +
        `切回后普通词条="${r.backTabClass}"，错误提示 ${r.backErrors} 处`
    );

    const f = feat.filter;
    check(
      'D4 过滤面板可展开且「格式」维度有候选',
      f.panelOpen && f.formatChips.length > 0,
      `面板展开=${f.panelOpen}，格式候选 ${f.formatChips.length} 个：${f.formatChips.join(', ')}`
    );
    check(
      'D5 格式过滤需回车重跑且结果只剩该格式',
      f.chipOn >= 1 &&
        f.hitsBeforeEnter === f.baseHits &&
        f.filterHint.includes('按回车') &&
        f.filteredHits > 0 &&
        f.filteredHits <= f.baseHits &&
        f.filteredHits === f.expectFmtHits &&
        f.filteredFormats.every((x) => x === f.targetFmt),
      `基线「\\w+」→ ${f.baseHits} 条（${JSON.stringify(f.fmtCount)}）；` +
        `勾选「${f.targetFmt}」后未回车仍 ${f.hitsBeforeEnter} 条（status="${f.filterHint}"）；` +
        `回车后 ${f.filteredHits} 条，格式=${JSON.stringify(f.filteredFormats)}（期望 ${f.expectFmtHits} 条）`
    );
    check(
      'D6 清空过滤后条件与结果均恢复',
      f.clearedChips === 0 && f.restoredHits === f.baseHits && f.restoredFormats.length === f.baseHits,
      `选中态 ${f.clearedChips} 处，结果 ${f.restoredHits} 条（期望 ${f.baseHits} 条），格式=${JSON.stringify(f.restoredFormats)}`
    );

    const errors = [
      ...valid.pageErrors,
      ...error.pageErrors,
      ...empty.pageErrors,
      ...feat.pageErrors,
    ];
    check('R5 四个用例均无未捕获 JS 异常', errors.length === 0, errors.join(' | ') || '0 个');
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
