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
const page = await browser.newPage({ viewport: { width: 1024, height: 768 }, hasTouch: true });
const errors = [];
page.on('pageerror', (e) => errors.push(String(e)));
page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));

const state = () => page.evaluate(() => {
  const S = window.__palamedes.state;
  return {
    phase: S.phase, busy: S.busy, selected: S.selected, result: S.result,
    options: S.ts ? S.ts.options.map((o) => [o.from, o.to]) : [], done: S.ts ? S.ts.done : false,
    groups: S.groups.length, history: S.history.length, stack: S.undoStack.length,
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
const tally = { 1: 0, 2: 0, 3: 0 };
let undos = 0;
for (let g = 0; g < games; g++) {
  if (g > 0) {
    await page.locator('#btn-new').click();
    await page.waitForFunction(() => window.__palamedes.state.phase === 'opening');
  }
  // 판마다 설정(말 색, 홈 위치, 상대)을 바꿔 가며 둔다. 뒤집힌 판에서도 터치 영역이 맞는지 확인.
  await page.locator(`[data-view="color:${g % 2 ? 'black' : 'white'}"]`).click();
  await page.locator(`[data-view="home:${g % 4 >= 2 ? 'left' : 'right'}"]`).click();
  await page.locator(`[data-view="ai:${g % 3 === 2 ? 'random' : 'baseline'}"]`).click();
  for (let guard = 0; guard < 3000; guard++) {
    await waitIdle();
    const s = await state();
    if (s.phase === 'over') {
      tally[s.result.points]++;
      break;
    }
    if (s.phase === 'opening' || s.phase === 'roll') {
      await page.locator('#btn-roll').click();
      continue;
    }
    if (s.phase === 'move') {
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
      if (s.selected !== from) await tap(from);
      await tap(to);
      await waitIdle();
      const after = await state();
      if (after.groups !== s.groups + 1) throw new Error(`move ${from}->${to} not applied: ${JSON.stringify(after)}`);
      continue;
    }
    throw new Error(`unexpected phase ${s.phase}`);
  }
}
console.log(`games: ${games}, results by points: ${JSON.stringify(tally)}, undos: ${undos}`);

if (shotDir) {
  fs.mkdirSync(shotDir, { recursive: true });
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
