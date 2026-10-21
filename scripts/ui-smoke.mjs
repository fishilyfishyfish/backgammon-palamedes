// 웹 UI 자동 점검: web/을 정적 서버로 띄우고 Chromium으로 여러 판을 끝까지 둔다.
// 사용: node scripts/ui-smoke.mjs [판 수] [스크린샷 폴더]
// 먼저 scripts/build-wasm.sh로 web/pkg를 만들어야 한다.
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
let playwright;
try {
  playwright = require('playwright');
} catch {
  playwright = require(path.join(process.env.npm_config_prefix || '/opt/node22', 'lib/node_modules/playwright'));
}

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../web');
const games = Number(process.argv[2] || 3);
const shotDir = process.argv[3];
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.wasm': 'application/wasm' };

const server = http.createServer((req, res) => {
  const p = path.join(root, decodeURIComponent(new URL(req.url, 'http://x').pathname));
  const file = p.endsWith('/') ? path.join(p, 'index.html') : p;
  if (!file.startsWith(root) || !fs.existsSync(file)) {
    res.writeHead(404);
    res.end();
    return;
  }
  res.writeHead(200, { 'content-type': TYPES[path.extname(file)] || 'application/octet-stream' });
  fs.createReadStream(file).pipe(res);
});
await new Promise((r) => server.listen(0, r));
const url = `http://127.0.0.1:${server.address().port}/?fast`;

const browser = await playwright.chromium.launch();
const page = await browser.newPage({ viewport: { width: 1024, height: 768 }, hasTouch: true, acceptDownloads: true });
const errors = [];
page.on('pageerror', (e) => errors.push(String(e)));
page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));

const state = () => page.evaluate(() => {
  const S = window.__palamedes.state;
  return {
    phase: S.phase, busy: S.busy, selected: S.selected, result: S.result,
    options: S.ts ? S.ts.options.map((o) => [o.from, o.to]) : [], done: S.ts ? S.ts.done : false,
    groups: S.groups.length, history: S.history.length, stack: S.undoStack.length,
    focus: S.focus, warning: !!S.warning, hint: !!S.hint, cubeWarn: !!S.cubeWarn, cubeOffer: !!S.cubeOffer,
    canDouble: !document.getElementById('btn-double').disabled,
    match: S.match, matchOver: !!(S.match && S.match.length && S.match.score.some((x) => x >= S.match.length)),
  };
});
async function waitIdle() {
  await page.waitForFunction(() => {
    const S = window.__palamedes && window.__palamedes.state;
    return S && !S.busy && S.phase !== 'loading' && S.phase !== 'ai';
  }, null, { timeout: 20000 });
}
const tap = (p) => page.locator(`.hit[data-point="${p}"]`).tap();

await page.goto(url);
await waitIdle();
const tally = {};
let doubles = 0;
let answers = 0;
let cubeWarns = 0;
let matchGames = 0;
let undos = 0;
let warnings = 0;
let hints = 0;
let keyMoves = 0;
for (let g = 0; g < games; g++) {
  if (g > 0) {
    await page.locator('#btn-new').click();
    await page.waitForFunction(() => window.__palamedes.state.phase === 'opening');
  }
  // 판마다 설정(말 색, 홈 위치, 상대)을 바꿔 가며 둔다. 뒤집힌 판에서도 터치 영역이 맞는지 확인.
  await page.locator(`[data-view="color:${g % 2 ? 'black' : 'white'}"]`).click();
  await page.locator(`[data-view="home:${g % 4 >= 2 ? 'left' : 'right'}"]`).click();
  await page.locator(`[data-view="ai:${['5', '3', '1'][g % 3]}"]`).click();
  // 두 번째 판은 3점 매치를 끝까지(여러 판), 나머지는 머니 게임 한 판.
  const matchMode = g === 1 ? '3' : '0';
  await page.locator(`[data-view="match:${matchMode}"]`).click();
  await waitIdle();
  for (let guard = 0; guard < 6000; guard++) {
    await waitIdle();
    const s = await state();
    if (s.phase === 'over') {
      tally[s.result.points] = (tally[s.result.points] || 0) + 1;
      if (matchMode !== '0') matchGames++;
      if (matchMode === '0' || s.matchOver) break;
      await page.locator('#btn-new').click();
      continue;
    }
    if (s.cubeWarn) {
      cubeWarns++;
      const btns = page.locator('#coach [data-coach]');
      await btns.nth(Math.random() < 0.6 ? 1 : 0).click();
      continue;
    }
    if (s.phase === 'cube') {
      answers++;
      if (shotDir && answers === 1) {
        fs.mkdirSync(shotDir, { recursive: true });
        await page.locator('#btn-hint').click();
        await waitIdle();
        await page.screenshot({ path: path.join(shotDir, 'cube-offer.png'), fullPage: true });
      }
      await page.locator(`[data-coach="${Math.random() < 0.7 ? 'take' : 'pass'}"]`).click();
      continue;
    }
    if (s.phase === 'roll' && s.canDouble && Math.random() < 0.12) {
      doubles++;
      await page.locator('#btn-double').click();
      continue;
    }
    if (s.phase === 'opening' || s.phase === 'roll') {
      await page.locator('#btn-roll').click();
      continue;
    }
    if (s.phase === 'move') {
      if (s.warning) {
        // 큰 실수 경고: 대부분 그대로 두고, 가끔 다시 두기.
        warnings++;
        if (shotDir && warnings === 1) {
          fs.mkdirSync(shotDir, { recursive: true });
          await page.screenshot({ path: path.join(shotDir, 'coach-warning.png'), fullPage: true });
        }
        await page.locator(`[data-coach="${Math.random() < 0.3 ? 'redo' : 'commit'}"]`).click();
        continue;
      }
      if (!s.hint && s.groups === 0 && s.options.length && Math.random() < 0.15) {
        await page.locator('#btn-hint').click();
        await waitIdle();
        hints++;
        if (!(await state()).hint) throw new Error('hint not shown');
        continue;
      }
      if (s.done) {
        // 가끔 무르기 후 다시 두기.
        if (s.groups > 0 && Math.random() < 0.1) {
          await page.locator('#btn-undo').click();
          undos++;
          continue;
        }
        await page.locator('#btn-done').click();
        continue;
      }
      const [from, to] = s.options[Math.floor(Math.random() * s.options.length)];
      if (Math.random() < 0.25) {
        // 키보드로 두기: 화살표로 칸을 돌다가 Enter.
        keyMoves++;
        // 버튼에 포커스가 남아 있으면 Enter가 그 버튼을 누른다(기본 동작).
        await page.evaluate(() => document.activeElement && document.activeElement.blur());
        const pressTo = async (p) => {
          for (let i = 0; i < 30 && (await state()).focus !== p; i++) await page.keyboard.press('ArrowRight');
          if ((await state()).focus !== p) throw new Error(`keyboard focus never reached ${p}`);
          await page.keyboard.press('Enter');
          await waitIdle();
        };
        if (s.selected !== null && s.selected !== from) await page.keyboard.press('Escape');
        if ((await state()).selected !== from) await pressTo(from);
        await pressTo(to);
      } else {
        if (s.selected !== from) await tap(from);
        await tap(to);
      }
      await waitIdle();
      const after = await state();
      if (after.groups !== s.groups + 1) throw new Error(`move ${from}->${to} not applied: ${JSON.stringify(after)}`);
      continue;
    }
    throw new Error(`unexpected phase ${s.phase}`);
  }
}
console.log(`games: ${games}, results by points: ${JSON.stringify(tally)}, undos: ${undos}, keyboard moves: ${keyMoves}, hints: ${hints}, warnings: ${warnings}, doubles: ${doubles}, cube answers: ${answers}, cube warnings: ${cubeWarns}, match games: ${matchGames}`);

// 분석 화면: 저장된 마지막 판을 끝까지 분석하고 수마다 결과가 있는지 확인.
// 같은 페이지(localStorage 공유)에서 분석 화면으로 이동.
const ap = page;
await ap.setViewportSize({ width: 1180, height: 820 });
await ap.goto(url.replace('?fast', 'analysis.html'));
await ap.waitForFunction(() => {
  const A = window.__palamedesAnalysis && window.__palamedesAnalysis.state;
  return A && A.game && A.results.length === A.game.entries.length && A.results.every(Boolean);
}, null, { timeout: 60000 });
const summary = await ap.evaluate(() => {
  const A = window.__palamedesAnalysis.state;
  const errs = A.results.filter((r) => r.error).length;
  const grades = {};
  for (const r of A.results) grades[r.grade] = (grades[r.grade] || 0) + 1;
  const explained = A.results.filter((r) => r.explanation && r.explanation.reasons.length).length;
  return { moves: A.results.length, errs, grades, explained, games: A.games.length };
});
console.log(`analysis: ${JSON.stringify(summary)}`);
if (summary.errs) throw new Error('analysis errors');
// 내보내기 → 저장소 비우기 → 가져오기 왕복. 같은 판이 같은 결과로 분석되는지 확인.
const before = await ap.evaluate(() => {
  const A = window.__palamedesAnalysis.state;
  return { id: A.game.id, n: A.game.entries.length, losses: A.results.map((r) => r.loss) };
});
const [dl] = await Promise.all([ap.waitForEvent('download'), ap.locator('#btn-export').click()]);
const exported = await dl.path();
const file = JSON.parse(fs.readFileSync(exported, 'utf8'));
if (file.format !== 'palamedes-games' || file.games.length !== 1) throw new Error('bad export file');
await ap.evaluate(() => localStorage.removeItem('palamedes.games'));
await ap.reload();
await ap.waitForFunction(() => /저장된 기보가 없어/.test(document.getElementById('status').textContent));
// 깨진 파일은 거부.
await ap.locator('#file-import').setInputFiles({ name: 'bad.json', mimeType: 'application/json', buffer: Buffer.from('{"games":[{"entries":[{"side":3}]}]}') });
await ap.waitForFunction(() => /가져오지 못했어/.test(document.getElementById('status').textContent));
await ap.locator('#file-import').setInputFiles(exported);
await ap.waitForFunction((n) => {
  const A = window.__palamedesAnalysis.state;
  return A.game && A.game.entries.length === n && A.results.length === n && A.results.every(Boolean) && /가져왔어/.test(document.getElementById('status').textContent);
}, before.n, { timeout: 60000 });
const after = await ap.evaluate(() => {
  const A = window.__palamedesAnalysis.state;
  return { id: A.game.id, losses: A.results.map((r) => r.loss), games: A.games.length };
});
if (after.id !== before.id || JSON.stringify(after.losses) !== JSON.stringify(before.losses)) throw new Error('import round trip mismatch');
// 같은 파일을 다시 가져오면 건너뛴다.
await ap.locator('#file-import').setInputFiles(exported);
await ap.waitForFunction(() => /새로 가져올 판이 없어/.test(document.getElementById('status').textContent));
console.log(`export/import: ${before.n} moves round-tripped, games after import ${after.games}`);

// 롤아웃: 선택이 있던 첫 수에서 돌리고 끝까지 기다린다.
const k = await ap.evaluate(() => window.__palamedesAnalysis.state.results.findIndex((r) => r && r.count > 1));
await ap.locator(`[data-i="${k}"]`).click();
const t0 = Date.now();
await ap.locator(`[data-rollout="${k}"]`).click();
await ap.waitForFunction((i) => {
  const ro = window.__palamedesAnalysis.state.rollouts[i];
  return ro && ro.done >= ro.total;
}, k, { timeout: 120000 });
const ro = await ap.evaluate((i) => {
  const r = window.__palamedesAnalysis.state.rollouts[i];
  return { rows: r.rows.length, trials: r.rows.map((x) => x.sums.trials), played: r.rows.some((x) => x.played) };
}, k);
if (!ro.played || ro.trials.some((n) => n !== 324)) throw new Error(`rollout incomplete ${JSON.stringify(ro)}`);
console.log(`rollout: ${ro.rows} candidates × 324 trials in ${((Date.now() - t0) / 1000).toFixed(1)}s`);

for (let i = 0; i < 5; i++) await ap.locator('#btn-next').click();
await ap.locator('[data-show="best"]').click();

if (shotDir) {
  fs.mkdirSync(shotDir, { recursive: true });
  // 설명이 있는 수로 이동해서 찍는다.
  const k = await ap.evaluate(() => window.__palamedesAnalysis.state.results.findIndex((r) => r.explanation && r.explanation.reasons.length));
  if (k >= 0) await ap.locator(`[data-i="${k}"]`).click();
  await ap.locator('[data-show="played"]').click();
  await ap.screenshot({ path: path.join(shotDir, 'analysis-tablet.png'), fullPage: true });
  await ap.setViewportSize({ width: 390, height: 844 });
  await ap.screenshot({ path: path.join(shotDir, 'analysis-phone.png'), fullPage: true });
  await page.goto(url);
  await waitIdle();
  await page.locator('#btn-new').click();
  // 내가 둘 차례(선택지 표시 상태)가 될 때까지 진행.
  for (;;) {
    await waitIdle();
    const s = await state();
    if (s.phase === 'move' && s.options.length) break;
    if (s.phase === 'move') await page.locator('#btn-done').click();
    else await page.locator('#btn-roll').click();
  }
  for (const [name, w, h] of [['tablet-landscape', 1180, 820], ['tablet-portrait', 820, 1180], ['phone', 390, 844]]) {
    await page.setViewportSize({ width: w, height: h });
    await page.waitForTimeout(150);
    const s = await state();
    if (s.phase === 'move' && s.options.length && s.selected === null) await tap(s.options[0][0]);
    await page.screenshot({ path: path.join(shotDir, `${name}.png`), fullPage: true });
  }
  await page.locator('[data-view="color:black"]').click();
  await page.locator('[data-view="home:left"]').click();
  await page.setViewportSize({ width: 1180, height: 820 });
  await page.screenshot({ path: path.join(shotDir, 'tablet-landscape-black-left.png'), fullPage: true });
  await page.locator('[data-view="color:white"]').click();
  await page.locator('[data-view="home:right"]').click();
}

await browser.close();
server.close();
if (errors.length) {
  console.error('page errors:\n' + errors.join('\n'));
  process.exit(1);
}
