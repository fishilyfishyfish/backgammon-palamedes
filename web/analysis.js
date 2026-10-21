// 분석 화면: 저장된 기보를 수마다 엔진(신경망 2수 탐색)으로 평가하고, 실수 등급과 설명을 보여 준다.
// 큐브 동작(더블·받음·포기)과 더블 놓침도 엔진 큐브 판단으로 등급을 매긴다. 수마다 롤아웃으로 후보를 검증할 수 있다.

import {
  AI, ENGINE_NN, GAME_LIMIT, PLY_NN, HUMAN,
  createEngine, ctxJson, escapeHtml, exportText, gradeClass, gradeOf, loadGames, loadView, parseImport, saveGame,
} from './common.js';
import { diceList, renderBoard } from './board-view.js';

const $ = (id) => document.getElementById(id);
const svg = $('board');
const els = {
  select: $('game-select'), status: $('status'), summary: $('summary'), prev: $('btn-prev'), next: $('btn-next'),
  pos: $('pos'), detail: $('detail'), moves: $('moves'), show: document.querySelectorAll('[data-show]'),
  importBtn: $('btn-import'), exportBtn: $('btn-export'), exportAllBtn: $('btn-export-all'), file: $('file-import'),
};
const setStatus = (t) => { els.status.textContent = t; };
const call = createEngine((msg) => setStatus(`엔진을 불러오지 못했어: ${msg}`));

const A = {
  games: loadGames(),
  game: null,
  // entries와 같은 순서. 수: analyze_move_json 결과 + missed(더블 놓침), 큐브: {kind: 'cube', dec, loss, grade...}
  results: [],
  rollouts: {}, // 항목 번호 → {done, total, rows}
  cur: 0,
  show: 'played', // before | played | best
  view: loadView(),
  run: 0, // 분석 중에 기보를 바꾸면 이전 작업을 버리기 위한 번호
};

// 롤아웃: 후보마다 36 × 9 = 324회, 36회씩 나눠 부른다. 후보끼리 같은 시드(같은 주사위).
const ROLLOUT_TRIALS = 324;
const ROLLOUT_CHUNK = 36;

const sideName = (side) => (side === HUMAN ? '나' : 'AI');
const pct = (x) => `${(x * 100).toFixed(1)}%`;
const signed = (x) => `${x >= 0 ? '+' : ''}${x.toFixed(3)}`;
const cubeOf = (e) => e.cube || { value: 1, owner: null };
const isCube = (e) => e.type === 'cube';
const actionName = { no_double: '노 더블', double_take: '더블/테이크', double_pass: '더블/패스', too_good: '너무 좋아서 노 더블' };
const cubeWord = { double: '더블', take: '받음', pass: '포기' };

function gameLabel(g) {
  const d = new Date(g.date);
  const when = `${d.getMonth() + 1}/${d.getDate()} ${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
  const res = g.result ? `${g.result.winner === HUMAN ? '내 승' : 'AI 승'} ${g.result.points}점${g.result.passed ? '(포기)' : ''}` : '진행 중';
  const m = g.match ? `${g.match.length}점 매치 ${g.match.score[HUMAN]}:${g.match.score[AI]} · ` : '';
  const vs = g.aiName ? `vs ${g.aiName}${g.aiRating ? `(${g.aiRating})` : ''} · ` : '';
  return `${when} · ${vs}${m}${res} · ${g.entries.length}수`;
}

function fillSelect() {
  els.select.innerHTML = A.games.map((g) => `<option value="${g.id}">${escapeHtml(gameLabel(g))}</option>`).join('');
}

// 그 시점에 side가 더블할 수 있었나(앱의 규칙과 같음).
function couldDouble(side, cube, match) {
  if (cube.owner === 1 - side || cube.value >= 64) return false;
  if (!match) return true;
  const need = (s) => match.length - match.score[s];
  if (!match.post && (need(0) === 1 || need(1) === 1)) return false;
  return cube.value < need(side);
}

async function analyzeEntry(g, i) {
  const e = g.entries[i];
  const ctx = ctxJson(cubeOf(e), g.match);
  if (isCube(e)) {
    // 더블은 더블한 쪽, 받음·포기는 더블한 쪽(상대) 입장의 판단으로 본다.
    const doubler = e.action === 'double' ? e.side : 1 - e.side;
    const dec = await call('cube_decision_json', e.board, doubler, ENGINE_NN, ctx);
    let loss;
    let right;
    if (e.action === 'double') {
      right = dec.double;
      loss = right ? 0 : Math.max(0, -dec.double_gain);
    } else {
      const take = e.action === 'take';
      right = take === dec.take;
      loss = right ? 0 : Math.abs(dec.double_take - dec.double_pass);
    }
    return { kind: 'cube', dec, loss, ...gradeOf(loss, right) };
  }
  const r = await call('analyze_move_json', e.before, e.side, e.dice[0], e.dice[1], e.after, ENGINE_NN, PLY_NN, ctx);
  // 더블 놓침: 첫 수가 아니고, 이번 차례에 더블하지 않았고(바로 앞이 상대의 받음이 아님), 더블할 수 있었을 때.
  const prev = g.entries[i - 1];
  const doubledNow = prev && isCube(prev) && prev.action === 'take' && prev.side !== e.side;
  if (i > 0 && !doubledNow && couldDouble(e.side, cubeOf(e), g.match)) {
    const dec = await call('cube_decision_json', e.before, e.side, ENGINE_NN, ctx);
    if (dec.double) r.missed = { dec, loss: dec.double_gain, ...gradeOf(dec.double_gain, false) };
  }
  return r;
}

async function openGame(id) {
  A.game = A.games.find((g) => String(g.id) === String(id)) || null;
  A.results = [];
  A.rollouts = {};
  A.cur = 0;
  const run = ++A.run;
  render();
  if (!A.game) return;
  const entries = A.game.entries;
  for (let i = 0; i < entries.length; i++) {
    if (run !== A.run) return;
    setStatus(`분석 중… ${i + 1}/${entries.length}`);
    try {
      A.results[i] = await analyzeEntry(A.game, i);
    } catch (err) {
      A.results[i] = { error: err.message };
    }
    if (i === A.cur || i % 8 === 0) render();
  }
  if (run !== A.run) return;
  setStatus(entries.length ? '분석 끝. 기보에서 수를 누르거나 ◀ ▶로 넘겨 봐.' : '이 기보에는 수가 없어.');
  render();
}

function summaryHtml() {
  const out = [];
  for (const side of [HUMAN, AI]) {
    const pairs = A.game.entries.map((e, i) => [e, A.results[i]]).filter(([e, r]) => e.side === side && r && !r.error);
    const moves = pairs.filter(([e, r]) => !isCube(e) && r.count > 1);
    const n = moves.length;
    const loss = moves.reduce((s, [, r]) => s + r.loss, 0);
    const count = (g) => moves.filter(([, r]) => r.grade === g).length;
    // 큐브 실수: 잘못된 더블·받음·포기 + 더블 놓침(의심스러움 이상).
    const cubeErr = pairs.filter(([e, r]) => (isCube(e) ? r.loss >= 0.04 : r.missed && r.missed.loss >= 0.04));
    const cubeLoss = cubeErr.reduce((s, [e, r]) => s + (isCube(e) ? r.loss : r.missed.loss), 0);
    out.push(`<div><dt>${sideName(side)} · 선택이 있던 수 ${n}개</dt>
      <dd class="small">평균 손실 ${n ? (loss / n).toFixed(3) : '—'}<br>
      <span class="grade-doubtful">?! ${count('doubtful')}</span> ·
      <span class="grade-bad">? ${count('bad')}</span> ·
      <span class="grade-very_bad">?? ${count('very_bad')}</span><br>
      큐브 실수 ${cubeErr.length}개${cubeErr.length ? `(손실 ${cubeLoss.toFixed(3)})` : ''}</dd></div>`);
  }
  return out.join('');
}

function reasonsHtml(ex) {
  if (!ex) return '';
  const items = ex.reasons.length
    ? ex.reasons.map((r) => `<li>${escapeHtml(r.text)} <span class="contrib">(${signed(r.contribution)})</span></li>`).join('')
    : '<li>특징값으로는 이 차이를 설명하지 못했어.</li>';
  return `<div class="approx"><span class="tag">설명(근사)</span>
    <ul>${items}</ul>
    <p class="hint">괄호는 대리 모델이 그 항목에 돌린 에퀴티 차이. 대리 모델 전체 예측 ${ex.approx_delta.toFixed(3)},
    검증 국면 설명력 R² ${ex.r2_heldout.toFixed(2)}.</p></div>`;
}

const listHtml = (lines) => `<ul>${lines.map((t) => `<li>${escapeHtml(t)}</li>`).join('')}</ul>`;

function cubeDetailHtml(e, r) {
  const who = sideName(e.side);
  const d = r.dec;
  const unit = d.match_play ? '정규화 매치 에퀴티' : '에퀴티(큐브 1 기준)';
  const values = `노 더블 ${signed(d.no_double)}, 더블/테이크 ${signed(d.double_take)}, 더블/패스 ${signed(d.double_pass)} (${unit}).`;
  const right = e.action === 'double' ? actionName[d.action] : d.take ? '받기' : '포기';
  const verdict = r.grade === 'best' ? '맞는 판단이야.' : `맞는 판단은 <b>${right}</b>이고, 에퀴티를 ${r.loss.toFixed(3)} 잃었어.`;
  const forTaker = e.action !== 'double';
  return `
    <div class="judge"><span class="tag">엔진 판단</span>
      ${who}: <b>${cubeWord[e.action]}</b>${e.action === 'double' ? `(큐브 ${e.cube.value} → ${e.cube.value * 2})` : ''}. ${verdict}
      등급 <b class="${gradeClass(r.grade)}">${r.label} ${r.mark}</b>.<br>${values}</div>
    <div class="approx"><span class="tag">근거(큐브 모델 근사)</span>${listHtml(forTaker ? d.taker_reasons : d.reasons)}</div>`;
}

function rolloutHtml(i, r) {
  const ro = A.rollouts[i];
  if (!ro) {
    return `<div class="rollout"><button type="button" class="btn" data-rollout="${i}">롤아웃으로 확인(상위 후보, 각 ${ROLLOUT_TRIALS}회)</button>
      <p class="hint">롤아웃은 끝까지 여러 번 두어 본 평균이야(양쪽 신경망 1수, 큐브 없는 머니 기준).</p></div>`;
  }
  const rows = ro.rows
    .map((x) => {
      const n = x.sums.trials;
      const eq = n ? x.sums.eq / n : NaN;
      const se = n > 1 ? Math.sqrt(Math.max(0, x.sums.eq_sq / n - eq * eq) / (n - 1)) : NaN;
      return `<tr class="${x.played ? 'played' : ''}"><td>${escapeHtml(x.notation)}</td><td>${n ? signed(eq) : '—'}</td><td>${n > 1 ? `± ${se.toFixed(3)}` : ''}</td><td>${signed(x.engine)}</td></tr>`;
    })
    .join('');
  const prog = ro.done < ro.total ? `진행 ${ro.done}/${ro.total}` : `끝(${ro.total}회 × ${ro.rows.length}수)`;
  return `<div class="rollout"><span class="tag">롤아웃</span> ${prog}
    <table class="cands"><thead><tr><th>수</th><th>롤아웃</th><th>표준오차</th><th>엔진 2수</th></tr></thead><tbody>${rows}</tbody></table>
    <p class="hint">큐브 없는 머니 기준이라${A.game.match ? ' 매치 점수와 큐브는 반영하지 않아. 엔진 2수 열은 매치 기준 값이야.' : ' 큐브는 반영하지 않아.'}</p></div>`;
}

function detailHtml(i, e, r) {
  if (isCube(e)) {
    if (!r) return `<div class="judge">${sideName(e.side)}: ${cubeWord[e.action]} — 분석 중…</div>`;
    if (r.error) return `<div class="judge">분석 실패: ${escapeHtml(r.error)}</div>`;
    return cubeDetailHtml(e, r);
  }
  const who = sideName(e.side);
  const dice = `${e.dice[0]}-${e.dice[1]}`;
  if (!r) return `<div class="judge">${who} ${dice}: ${escapeHtml(e.notation)} — 분석 중…</div>`;
  if (r.error) return `<div class="judge">분석 실패: ${escapeHtml(r.error)}</div>`;
  const missed = r.missed
    ? `<div class="judge"><span class="tag">엔진 판단</span>굴리기 전에 더블할 수 있었어(<b>${actionName[r.missed.dec.action]}</b>, 더블하면 ${r.missed.loss.toFixed(3)} 이득).
        등급 <b class="${gradeClass(r.missed.grade)}">${r.missed.label} ${r.missed.mark}</b>.</div>
       <div class="approx"><span class="tag">근거(큐브 모델 근사)</span>${listHtml(r.missed.dec.reasons)}</div>`
    : '';
  if (r.count === 1) {
    return `<div class="judge"><span class="tag">엔진 판단</span>${who} ${dice}: ${escapeHtml(r.played.notation)} — 둘 수 있는 수가 이것뿐이었어.</div>${missed}`;
  }
  const p = r.played.probs;
  const b = r.best.probs;
  const top = r.top
    .map((m, k) => `<tr class="${m.notation === r.played.notation ? 'played' : ''}"><td>${k + 1}</td><td>${escapeHtml(m.notation)}</td><td>${signed(m.equity)}</td><td>${k ? (m.equity - r.best.equity).toFixed(3) : ''}</td></tr>`)
    .join('');
  // 둔 수가 상위 5개 밖이면 따로 한 줄.
  const extra = r.rank > r.top.length
    ? `<tr class="played"><td>${r.rank}</td><td>${escapeHtml(r.played.notation)}</td><td>${signed(r.played.equity)}</td><td>${(r.played.equity - r.best.equity).toFixed(3)}</td></tr>`
    : '';
  const verdict = r.grade === 'best'
    ? '최선 수야.'
    : `최선 수(<b>${escapeHtml(r.best.notation)}</b>)보다 에퀴티가 ${r.loss.toFixed(3)} 낮아.`;
  const unit = A.game.match ? ' (매치라 에퀴티는 정규화 매치 에퀴티)' : '';
  return `
    ${missed}
    <div class="judge"><span class="tag">엔진 판단</span>
      ${who} ${dice}: <b>${escapeHtml(r.played.notation)}</b> ${verdict}
      등급 <b class="${gradeClass(r.grade)}">${r.label} ${r.mark}</b> · 후보 ${r.count}개 중 ${r.rank}위${unit}.
      <br>둔 수 뒤 ${who}의 승리 ${pct(p.win)}, 개먼승 ${pct(p.win_gammon)}, 개먼패 ${pct(p.lose_gammon)}${
        r.grade === 'best' ? '' : ` (최선 수면 ${pct(b.win)}, ${pct(b.win_gammon)}, ${pct(b.lose_gammon)})`}.</div>
    <table class="cands"><thead><tr><th>#</th><th>수</th><th>에퀴티</th><th>차이</th></tr></thead><tbody>${top}${extra}</tbody></table>
    ${reasonsHtml(r.explanation)}
    ${rolloutHtml(i, r)}`;
}

async function runRollout(i) {
  const e = A.game.entries[i];
  const r = A.results[i];
  if (!r || r.error || isCube(e) || A.rollouts[i]) return;
  const cands = r.top.slice(0, 3).map((m) => ({ notation: m.notation, board: m.board, engine: m.equity, played: m.notation === r.played.notation }));
  if (!cands.some((c) => c.played)) cands.push({ notation: r.played.notation, board: r.played.board, engine: r.played.equity, played: true });
  const ro = { done: 0, total: ROLLOUT_TRIALS, rows: cands.map((c) => ({ ...c, sums: { trials: 0, eq: 0, eq_sq: 0 } })) };
  A.rollouts[i] = ro;
  render();
  const seed = (A.game.id + i) % 1_000_000_007;
  for (let start = 0; start < ROLLOUT_TRIALS; start += ROLLOUT_CHUNK) {
    for (const row of ro.rows) {
      const s = await call('rollout_json', row.board, e.side, start, ROLLOUT_CHUNK, seed, ENGINE_NN);
      row.sums.trials += s.trials;
      row.sums.eq += s.eq;
      row.sums.eq_sq += s.eq_sq;
    }
    ro.done = start + ROLLOUT_CHUNK;
    if (A.cur === i) render();
  }
  render();
}

function render() {
  for (const btn of els.show) btn.setAttribute('aria-pressed', String(btn.dataset.show === A.show));
  els.exportBtn.disabled = !A.game;
  els.exportAllBtn.disabled = !A.games.length;
  if (!A.game) {
    els.summary.innerHTML = '';
    els.moves.innerHTML = '';
    els.detail.hidden = true;
    els.pos.textContent = '';
    els.prev.disabled = els.next.disabled = true;
    renderBoard(svg, { board: startBoard, view: A.view });
    return;
  }
  const entries = A.game.entries;
  const n = entries.length;
  els.prev.disabled = A.cur <= 0;
  els.next.disabled = A.cur >= n - 1;
  els.pos.textContent = n ? `${A.cur + 1} / ${n}` : '0 / 0';
  els.summary.innerHTML = summaryHtml();

  // 기보: 수는 (사람, AI) 한 쌍씩, 큐브 동작은 그 칸 앞에 붙는다.
  const rows = [];
  let row = null;
  let pending = [[], []];
  entries.forEach((e, i) => {
    if (isCube(e)) {
      pending[e.side].push(i);
      if (e.action === 'pass') {
        row = [null, null];
        rows.push(row);
        row[e.side] = { cube: pending[e.side], move: null };
        pending = [[], []];
      }
      return;
    }
    if (!row || e.side === HUMAN || row[AI]) {
      row = [null, null];
      rows.push(row);
    }
    row[e.side] = { cube: pending[e.side], move: i };
    pending[e.side] = [];
  });
  const markOf = (r) => (r && r.mark ? ` <b class="mark ${gradeClass(r.grade)}">${r.mark}</b>` : '');
  const btn = (i, html) => `<button type="button" class="mv-btn ${i === A.cur ? 'cur' : ''}" data-i="${i}">${html}</button>`;
  const cell = (c) => {
    if (!c) return '<span class="empty">—</span>';
    const cubes = c.cube.map((i) => {
      const e = entries[i];
      return btn(i, `<span class="cube-act">${cubeWord[e.action]}${e.action === 'double' ? ` ${e.cube.value * 2}` : ''}</span>${markOf(A.results[i])}`);
    }).join('');
    if (c.move === null) return `<span>${cubes}</span>`;
    const e = entries[c.move];
    const r = A.results[c.move];
    const missed = r && r.missed && r.missed.mark ? ` <b class="mark ${gradeClass(r.missed.grade)}" title="더블 놓침">D${r.missed.mark}</b>` : '';
    return `<span>${cubes}${btn(c.move, `<span class="dice">${e.dice[0]}${e.dice[1]}</span>${escapeHtml(e.notation)}${markOf(r)}${missed}`)}</span>`;
  };
  els.moves.innerHTML = rows.map((r) => `<li><div class="mv">${cell(r[HUMAN])}${cell(r[AI])}</div></li>`).join('');
  const curBtn = els.moves.querySelector('.cur');
  if (curBtn) curBtn.scrollIntoView({ block: 'nearest' });

  if (!n) {
    els.detail.hidden = true;
    renderBoard(svg, { board: startBoard, view: A.view });
    return;
  }
  const e = entries[A.cur];
  const r = A.results[A.cur];
  els.detail.hidden = false;
  const bad = (x) => x && (x.grade === 'bad' || x.grade === 'very_bad');
  els.detail.className = `coach ${bad(r) || (r && bad(r.missed)) ? 'warn' : ''}`;
  els.detail.innerHTML = detailHtml(A.cur, e, r);

  const model = { view: A.view, cube: cubeOf(e) };
  if (isCube(e)) {
    Object.assign(model, { board: e.board });
  } else if (A.show === 'before' || !r || r.error) {
    Object.assign(model, { board: e.before, dice: { side: e.side, list: diceList(e.dice) } });
  } else {
    const m = A.show === 'best' ? r.best : r.played;
    Object.assign(model, { board: m.board, lastMove: { side: e.side, steps: m.steps }, dice: { side: e.side, list: diceList(e.dice, m.steps) } });
  }
  renderBoard(svg, model);
}

function go(i) {
  if (!A.game) return;
  A.cur = Math.max(0, Math.min(A.game.entries.length - 1, i));
  render();
}

let startBoard = null;

// ---------- 내보내기 · 가져오기 ----------
function stamp(d = new Date()) {
  const p = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}`;
}

function download(name, text) {
  const url = URL.createObjectURL(new Blob([text], { type: 'application/json' }));
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

function exportCurrent() {
  if (!A.game) return;
  download(`palamedes-${stamp(new Date(A.game.date))}.json`, exportText([A.game]));
  setStatus('이 판을 파일로 내보냈어.');
}

function exportAll() {
  if (!A.games.length) return;
  download(`palamedes-games-${stamp()}.json`, exportText(A.games));
  setStatus(`저장된 ${A.games.length}판을 파일로 내보냈어.`);
}

async function importFile(file) {
  let parsed;
  try {
    parsed = parseImport(await file.text());
  } catch (err) {
    setStatus(`가져오지 못했어: ${err.message}`);
    return;
  }
  const { games, skipped } = parsed;
  if (!games.length) {
    setStatus(`새로 가져올 판이 없어(이미 있는 판 ${skipped}개).`);
    return;
  }
  // 파일의 첫 판이 목록 맨 위에 오도록 뒤에서부터 저장.
  for (const g of [...games].reverse()) saveGame(g);
  A.games = loadGames();
  fillSelect();
  const kept = games.filter((g) => A.games.some((x) => x.id === g.id)).length;
  const dropped = games.length - kept;
  const notes = [skipped ? `이미 있는 판 ${skipped}개는 건너뜀` : '', dropped ? `저장 한도(${GAME_LIMIT}판)를 넘어 ${dropped}개는 못 담음` : '']
    .filter(Boolean)
    .join(', ');
  els.select.value = String(games[0].id);
  await openGame(games[0].id);
  setStatus(`${kept}판을 가져왔어${notes ? ` (${notes})` : ''}. 수가 규칙에 안 맞으면 그 수에 분석 실패로 표시돼.`);
}

els.importBtn.addEventListener('click', () => els.file.click());
els.file.addEventListener('change', () => {
  const f = els.file.files[0];
  els.file.value = '';
  if (f) importFile(f);
});
els.exportBtn.addEventListener('click', exportCurrent);
els.exportAllBtn.addEventListener('click', exportAll);

els.select.addEventListener('change', () => openGame(els.select.value));
els.prev.addEventListener('click', () => go(A.cur - 1));
els.next.addEventListener('click', () => go(A.cur + 1));
els.detail.addEventListener('click', (e) => {
  const t = e.target.closest('[data-rollout]');
  if (t) runRollout(Number(t.dataset.rollout)).catch((err) => setStatus(`롤아웃 실패: ${err.message}`));
});
els.moves.addEventListener('click', (e) => {
  const t = e.target.closest('[data-i]');
  if (t) go(Number(t.dataset.i));
});
for (const btn of els.show) {
  btn.addEventListener('click', () => {
    A.show = btn.dataset.show;
    render();
  });
}
document.addEventListener('keydown', (e) => {
  if (e.target.closest('select')) return;
  if (e.key === 'ArrowLeft') go(A.cur - 1);
  else if (e.key === 'ArrowRight') go(A.cur + 1);
});

// 자동 테스트용 읽기 핸들.
window.__palamedesAnalysis = { state: A };

(async () => {
  startBoard = await call('start_board');
  if (!A.games.length) {
    setStatus(`저장된 기보가 없어. 대국을 두면 자동으로 저장돼(최근 ${GAME_LIMIT}판).`);
    render();
    return;
  }
  fillSelect();
  const want = new URLSearchParams(location.search).get('game');
  const id = want && A.games.some((g) => String(g.id) === want) ? want : A.games[0].id;
  els.select.value = String(id);
  await openGame(id);
})();
