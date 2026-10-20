// 대국 화면: 사람(백) vs 랜덤 AI(흑). 규칙 판단은 전부 엔진(Worker)에 맡긴다.

const HUMAN = 0;
const AI = 1;
const BAR = 25;

// ---------- 엔진 호출 ----------
const worker = new Worker(new URL('./engine-worker.js', import.meta.url), { type: 'module' });
const pending = new Map();
let nextId = 1;
worker.onmessage = (e) => {
  const { id, ok, result, error } = e.data;
  const p = pending.get(id);
  if (!p) return;
  pending.delete(id);
  ok ? p.resolve(result) : p.reject(new Error(error));
};
worker.onerror = (e) => {
  setStatus(`엔진을 불러오지 못했어: ${e.message || '알 수 없는 오류'}`);
};
function call(fn, ...args) {
  const id = nextId++;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    worker.postMessage({ id, fn, args });
  });
}

// ---------- 유틸 ----------
// ?fast 를 붙이면 AI 연출 지연 없이 진행(자동 테스트용).
const FAST = new URLSearchParams(location.search).has('fast');
const sleep = (ms) => new Promise((r) => setTimeout(r, FAST ? 0 : ms));
function rollDie() {
  const buf = new Uint32Array(1);
  const limit = Math.floor(0x100000000 / 6) * 6;
  do crypto.getRandomValues(buf); while (buf[0] >= limit);
  return (buf[0] % 6) + 1;
}
const roll2 = () => [rollDie(), rollDie()];
const idx = (side, p) => side * 26 + p;
const flatSteps = (groups) => groups.flat().map(({ from, to, die }) => ({ from, to, die }));

// 엔진이 준 걸음(hit 포함)을 판에 적용. AI 수를 한 걸음씩 보여 줄 때만 쓴다.
function applyStep(board, side, s) {
  const b = board.slice();
  b[idx(side, s.from)] -= 1;
  b[idx(side, s.to)] += 1;
  if (s.hit) {
    b[idx(1 - side, 25 - s.to)] -= 1;
    b[idx(1 - side, BAR)] += 1;
  }
  return b;
}

const storage = {
  get(k, d) { try { const v = localStorage.getItem(k); return v === null ? d : JSON.parse(v); } catch { return d; } },
  set(k, v) { try { localStorage.setItem(k, JSON.stringify(v)); } catch { /* 저장 불가 환경 */ } },
};

// ---------- 상태 ----------
const S = {
  phase: 'loading', // loading | opening | roll | move | ai | over
  board: null,
  turn: HUMAN,
  dice: null, // [d1, d2]
  openingDice: null, // [백, 흑]
  turnStart: null,
  groups: [], // 이번 턴에 둔 선택지별 걸음 묶음(무르기 단위)
  ts: null, // 엔진 turn_state
  selected: null,
  history: [], // {side, dice, notation}
  undoStack: [], // 사람 턴 시작 시점 {board, dice, historyLen}
  lastMove: null, // {side, steps}
  result: null, // {winner, points}
  score: storage.get('palamedes.score', { human: 0, ai: 0 }),
  pips: [167, 167],
  busy: false,
};

// ---------- DOM ----------
const $ = (id) => document.getElementById(id);
const svg = $('board');
const els = {
  status: $('status'), roll: $('btn-roll'), undo: $('btn-undo'), done: $('btn-done'), newGame: $('btn-new'),
  pipH: $('pip-human'), pipA: $('pip-ai'), pipDiff: $('pip-diff'), moves: $('moves'),
  scoreH: $('score-human'), scoreA: $('score-ai'),
};
function setStatus(t) { els.status.textContent = t; }

// ---------- 판 기하 (viewBox 932 x 706) ----------
const G = {
  PW: 62, D: 56, R: 27,
  leftX: 22, barX: 394, barW: 56, rightX: 450,
  top: 42, bottom: 666, mid: 354,
  trayX: 846, trayW: 76,
};
const PT_LEN = 5 * G.D;

// 백 기준 포인트 p(1..24)의 중심 x와 위/아래.
function pointGeom(p) {
  if (p <= 6) return { x: G.rightX + (6 - p + 0.5) * G.PW, top: false };
  if (p <= 12) return { x: G.leftX + (12 - p + 0.5) * G.PW, top: false };
  if (p <= 18) return { x: G.leftX + (p - 13 + 0.5) * G.PW, top: true };
  return { x: G.rightX + (p - 19 + 0.5) * G.PW, top: true };
}
function slotY(top, i) {
  return top ? G.top + G.R + 1 + i * G.D : G.bottom - G.R - 1 - i * G.D;
}
function barSlotY(side, i) {
  // 백은 바 아래쪽, 흑은 위쪽에 쌓는다.
  return side === HUMAN ? G.mid + 34 + i * G.D : G.mid - 34 - i * G.D;
}

function countsAt(board, p) {
  return { w: board[idx(HUMAN, p)], b: board[idx(AI, 25 - p)] };
}

function checker(x, y, side, label) {
  const cls = side === HUMAN ? 'ck-w' : 'ck-b';
  let s = `<circle class="${cls}" cx="${x}" cy="${y}" r="${G.R}"/>`;
  if (label) s += `<text class="${side === HUMAN ? 'ck-n-w' : 'ck-n-b'}" x="${x}" y="${y}">${label}</text>`;
  return s;
}

function dieSvg(x, y, v, side, used) {
  const size = 46;
  const h = size / 2;
  const pos = {
    1: [[0, 0]], 2: [[-1, -1], [1, 1]], 3: [[-1, -1], [0, 0], [1, 1]],
    4: [[-1, -1], [1, -1], [-1, 1], [1, 1]], 5: [[-1, -1], [1, -1], [0, 0], [-1, 1], [1, 1]],
    6: [[-1, -1], [1, -1], [-1, 0], [1, 0], [-1, 1], [1, 1]],
  }[v];
  const cls = side === HUMAN ? 'die' : 'die-b';
  let s = `<g class="${used ? 'die used' : ''}" opacity="${used ? 0.35 : 1}">`;
  s += `<rect class="${cls}" x="${x - h}" y="${y - h}" width="${size}" height="${size}" rx="8"/>`;
  for (const [dx, dy] of pos) s += `<circle class="${side === HUMAN ? 'pip' : 'pip-b'}" cx="${x + dx * 12}" cy="${y + dy * 12}" r="4.6"/>`;
  return s + '</g>';
}

// 이번 턴에 이미 쓴 주사위 표시용.
function usedDiceFlags() {
  if (!S.dice) return [];
  const [a, b] = S.dice;
  const list = a === b ? [a, a, a, a] : [a, b];
  const used = list.map(() => false);
  if (S.phase === 'move') {
    for (const st of flatSteps(S.groups)) {
      const i = list.findIndex((d, k) => d === st.die && !used[k]);
      if (i >= 0) used[i] = true;
    }
  }
  return list.map((d, i) => ({ d, used: used[i] }));
}

function render() {
  const b = S.board;
  if (!b) return;
  const parts = [];
  const W = 932;
  const H = 706;
  parts.push(`<rect class="frame" x="10" y="30" width="824" height="648" rx="10"/>`);
  parts.push(`<rect class="felt" x="${G.leftX}" y="${G.top}" width="${6 * G.PW}" height="${G.bottom - G.top}"/>`);
  parts.push(`<rect class="felt" x="${G.rightX}" y="${G.top}" width="${6 * G.PW}" height="${G.bottom - G.top}"/>`);
  parts.push(`<rect class="barzone" x="${G.barX}" y="${G.top}" width="${G.barW}" height="${G.bottom - G.top}"/>`);
  parts.push(`<rect class="frame" x="${G.trayX - 6}" y="30" width="${G.trayW + 12}" height="648" rx="10"/>`);
  parts.push(`<rect class="tray" x="${G.trayX}" y="${G.top}" width="${G.trayW}" height="${PT_LEN}"/>`);
  parts.push(`<rect class="tray" x="${G.trayX}" y="${G.bottom - PT_LEN}" width="${G.trayW}" height="${PT_LEN}"/>`);

  // 포인트 삼각형과 번호(백 기준).
  for (let p = 1; p <= 24; p++) {
    const { x, top } = pointGeom(p);
    const half = G.PW / 2 - 1;
    const baseY = top ? G.top : G.bottom;
    const tipY = top ? G.top + PT_LEN - 10 : G.bottom - PT_LEN + 10;
    const cls = p % 2 === 0 ? 'pt-a' : 'pt-b';
    parts.push(`<polygon class="${cls}" points="${x - half},${baseY} ${x + half},${baseY} ${x},${tipY}"/>`);
    parts.push(`<text class="lbl" x="${x}" y="${top ? 22 : 696}">${p}</text>`);
  }

  // 직전 수 표시(출발·도착 칸).
  if (S.lastMove && S.phase !== 'move') {
    const { side, steps } = S.lastMove;
    const marks = new Set();
    for (const s of steps) {
      for (const q of [s.from, s.to]) {
        if (q >= 1 && q <= 24) marks.add(side === HUMAN ? q : 25 - q);
      }
    }
    for (const p of marks) {
      const { x, top } = pointGeom(p);
      parts.push(`<rect class="last" x="${x - G.PW / 2 + 3}" y="${top ? G.top : G.bottom - 8}" width="${G.PW - 6}" height="8" rx="3"/>`);
    }
  }

  const fromSet = new Set();
  const dests = [];
  if (S.phase === 'move' && S.ts) {
    for (const o of S.ts.options) fromSet.add(o.from);
    if (S.selected !== null) for (const o of S.ts.options) if (o.from === S.selected) dests.push(o.to);
  }

  // 포인트 위 말.
  for (let p = 1; p <= 24; p++) {
    const { x, top } = pointGeom(p);
    const { w, b: bk } = countsAt(b, p);
    const n = w || bk;
    const side = w ? HUMAN : AI;
    for (let i = 0; i < Math.min(n, 5); i++) {
      const label = i === 4 && n > 5 ? String(n) : '';
      parts.push(checker(x, slotY(top, i), side, label));
    }
    if (n && side === HUMAN && fromSet.has(p)) {
      const y = slotY(top, Math.min(n, 5) - 1);
      parts.push(`<circle class="${S.selected === p ? 'sel-ring' : 'can-ring'}" cx="${x}" cy="${y}" r="${G.R + 4}"/>`);
    }
  }

  // 바.
  const bx = G.barX + G.barW / 2;
  for (const side of [HUMAN, AI]) {
    const n = b[idx(side, BAR)];
    for (let i = 0; i < Math.min(n, 3); i++) {
      parts.push(checker(bx, barSlotY(side, i), side, i === 2 && n > 3 ? String(n) : ''));
    }
  }
  if (b[idx(HUMAN, BAR)] && fromSet.has(BAR)) {
    parts.push(`<circle class="${S.selected === BAR ? 'sel-ring' : 'can-ring'}" cx="${bx}" cy="${barSlotY(HUMAN, 0)}" r="${G.R + 4}"/>`);
  }

  // 베어오프 트레이: 백은 아래, 흑은 위.
  const tx = G.trayX + 6;
  const tw = G.trayW - 12;
  for (let i = 0; i < b[idx(HUMAN, 0)]; i++) parts.push(`<rect class="off-w" x="${tx}" y="${G.bottom - 4 - (i + 1) * 17}" width="${tw}" height="14" rx="4"/>`);
  for (let i = 0; i < b[idx(AI, 0)]; i++) parts.push(`<rect class="off-b" x="${tx}" y="${G.top + 4 + i * 17}" width="${tw}" height="14" rx="4"/>`);

  // 갈 수 있는 칸.
  for (const to of dests) {
    if (to === 0) {
      parts.push(`<rect class="dest-tray" x="${G.trayX + 2}" y="${G.bottom - PT_LEN + 2}" width="${G.trayW - 4}" height="${PT_LEN - 4}" rx="6"/>`);
      continue;
    }
    const { x, top } = pointGeom(to);
    const { w, b: bk } = countsAt(b, to);
    const slot = bk ? 0 : Math.min(w, 4);
    parts.push(`<circle class="dest" cx="${x}" cy="${slotY(top, slot)}" r="${G.R}"/>`);
  }

  // 주사위: 사람은 오른쪽, AI는 왼쪽 판 가운데.
  if (S.phase === 'opening' && S.openingDice) {
    parts.push(dieSvg(G.rightX + 3 * G.PW, G.mid, S.openingDice[0], HUMAN, false));
    parts.push(dieSvg(G.leftX + 3 * G.PW, G.mid, S.openingDice[1], AI, false));
  } else if (S.dice) {
    const flags = usedDiceFlags();
    const cx = S.turn === HUMAN ? G.rightX + 3 * G.PW : G.leftX + 3 * G.PW;
    const gap = 54;
    const x0 = cx - ((flags.length - 1) * gap) / 2;
    flags.forEach((f, i) => parts.push(dieSvg(x0 + i * gap, G.mid, f.d, S.turn, f.used && S.turn === HUMAN)));
  }

  // 터치 영역(맨 위).
  for (let p = 1; p <= 24; p++) {
    const { x, top } = pointGeom(p);
    const y = top ? G.top : G.mid;
    parts.push(`<rect class="hit" data-point="${p}" x="${x - G.PW / 2}" y="${y}" width="${G.PW}" height="${G.mid - G.top}"/>`);
  }
  parts.push(`<rect class="hit" data-point="${BAR}" x="${G.barX}" y="${G.top}" width="${G.barW}" height="${G.bottom - G.top}"/>`);
  parts.push(`<rect class="hit" data-point="0" x="${G.trayX - 6}" y="${G.mid}" width="${G.trayW + 12}" height="${G.bottom - G.mid + 12}"/>`);

  svg.setAttribute('viewBox', `0 0 ${W} ${H}`);
  svg.innerHTML = parts.join('');
  renderPanel();
}

function renderPanel() {
  els.pipH.textContent = S.pips[HUMAN];
  els.pipA.textContent = S.pips[AI];
  const d = S.pips[AI] - S.pips[HUMAN];
  els.pipDiff.textContent = d === 0 ? '핍 카운트가 같아.' : d > 0 ? `내가 ${d}핍 앞서.` : `AI가 ${-d}핍 앞서.`;
  els.scoreH.textContent = S.score.human;
  els.scoreA.textContent = S.score.ai;

  els.roll.disabled = S.busy || !(S.phase === 'opening' || S.phase === 'roll');
  els.done.disabled = S.busy || !(S.phase === 'move' && S.ts && S.ts.done);
  els.undo.disabled = S.busy || !canUndo();

  // 기보: 한 줄에 (백, 흑) 한 쌍.
  const rows = [];
  let row = null;
  for (const h of S.history) {
    if (!row || h.side === HUMAN || row[AI]) {
      row = [null, null];
      rows.push(row);
    }
    row[h.side] = h;
  }
  const cell = (h) => (h ? `<span><span class="dice">${h.dice[0]}${h.dice[1]}</span>${h.notation}</span>` : '<span class="empty">—</span>');
  els.moves.innerHTML = rows.map((r) => `<li><div class="mv">${cell(r[HUMAN])}${cell(r[AI])}</div></li>`).join('');
  els.moves.scrollTop = els.moves.scrollHeight;
}

async function updatePips() {
  S.pips = await call('pip_counts', S.board);
}

// ---------- 진행 ----------
async function newGame() {
  const board = await call('start_board');
  Object.assign(S, {
    phase: 'opening', board, turn: HUMAN, dice: null, openingDice: null, turnStart: null,
    groups: [], ts: null, selected: null, history: [], undoStack: [], lastMove: null, result: null,
  });
  await updatePips();
  setStatus('첫 수를 정하자. 굴리기를 누르면 서로 주사위를 하나씩 굴려서 높은 쪽이 먼저 둬.');
  render();
}

async function onRoll() {
  if (S.phase === 'opening') {
    const w = rollDie();
    const b = rollDie();
    S.openingDice = [w, b];
    if (w === b) {
      setStatus(`둘 다 ${w}이 나와서 다시 굴려야 해.`);
      render();
      return;
    }
    const dice = [Math.max(w, b), Math.min(w, b)];
    S.openingDice = null;
    if (w > b) {
      setStatus(`나 ${w}, AI ${b}. 내가 먼저 ${dice[0]}-${dice[1]}로 둬.`);
      await beginHumanTurn(dice, true);
    } else {
      setStatus(`나 ${w}, AI ${b}. AI가 먼저 둬.`);
      await aiTurn(dice);
    }
    return;
  }
  if (S.phase === 'roll') await beginHumanTurn(roll2());
}

async function beginHumanTurn(dice, keepStatus = false) {
  S.undoStack.push({ board: S.board.slice(), dice, historyLen: S.history.length, lastMove: S.lastMove });
  Object.assign(S, { turn: HUMAN, dice, turnStart: S.board.slice(), groups: [], selected: null, phase: 'move' });
  await refreshTurn(keepStatus);
}

async function refreshTurn(keepStatus = false) {
  S.ts = await call('turn_state_json', S.turnStart, HUMAN, S.dice[0], S.dice[1], JSON.stringify(flatSteps(S.groups)));
  S.board = S.ts.board;
  await updatePips();
  if (!keepStatus) {
    if (S.ts.need === 0) setStatus(`${S.dice[0]}-${S.dice[1]}: 둘 수 있는 수가 없어. 턴 끝내기를 눌러.`);
    else if (S.ts.done) setStatus(`수 완성: ${S.ts.notation}. 턴 끝내기를 누르거나 무르기로 바꿔.`);
    else if (S.selected !== null) setStatus('초록색 칸 중 하나를 눌러.');
    else setStatus(`${S.dice[0]}-${S.dice[1]}: 움직일 말을 눌러. (${S.ts.played}/${S.ts.need})`);
  }
  // 고를 수 있는 출발점이 하나뿐이면 미리 선택해 준다.
  const froms = new Set(S.ts.options.map((o) => o.from));
  if (froms.size === 1 && S.selected === null) S.selected = [...froms][0];
  render();
}

async function onPoint(p) {
  if (S.phase !== 'move' || !S.ts) return;
  const opts = S.ts.options;
  if (S.selected !== null) {
    const o = opts.find((x) => x.from === S.selected && x.to === p);
    if (o) {
      S.groups.push(o.steps);
      S.selected = null;
      await refreshTurn();
      return;
    }
  }
  if (opts.some((x) => x.from === p)) {
    S.selected = S.selected === p ? null : p;
    setStatus(S.selected === null ? '움직일 말을 눌러.' : '초록색 칸 중 하나를 눌러.');
  } else {
    S.selected = null;
  }
  render();
}

async function onDone() {
  if (S.phase !== 'move' || !S.ts || !S.ts.done) return;
  S.history.push({ side: HUMAN, dice: S.dice, notation: S.ts.notation });
  S.lastMove = { side: HUMAN, steps: S.ts.steps };
  if (await checkOver()) return;
  await aiTurn(roll2());
}

async function aiTurn(dice) {
  Object.assign(S, { phase: 'ai', turn: AI, dice, selected: null, ts: null });
  render();
  await sleep(450);
  const moves = await call('legal_moves_json', S.board, AI, dice[0], dice[1]);
  const m = moves[Math.floor(Math.random() * moves.length)];
  if (m.steps.length === 0) {
    setStatus(`AI ${dice[0]}-${dice[1]}: 둘 수 있는 수가 없어.`);
    await sleep(700);
  } else {
    setStatus(`AI ${dice[0]}-${dice[1]}: ${m.notation}`);
    for (const s of m.steps) {
      S.board = applyStep(S.board, AI, s);
      render();
      await sleep(330);
    }
    S.board = m.board;
  }
  S.history.push({ side: AI, dice, notation: m.notation });
  S.lastMove = { side: AI, steps: m.steps };
  await updatePips();
  if (await checkOver()) return;
  S.phase = 'roll';
  setStatus(`${els.status.textContent} — 이제 내 차례야. 굴리기를 눌러.`);
  render();
}

async function checkOver() {
  const r = await call('result_json', S.board);
  if (!r) return false;
  S.result = r;
  S.phase = 'over';
  const kind = r.points === 3 ? '백개먼' : r.points === 2 ? '개먼' : '싱글';
  if (r.winner === HUMAN) S.score.human += r.points;
  else S.score.ai += r.points;
  storage.set('palamedes.score', S.score);
  setStatus(`${r.winner === HUMAN ? '내가 이겼어' : 'AI가 이겼어'}. ${kind} ${r.points}점. 새 게임을 눌러.`);
  render();
  return true;
}

function canUndo() {
  if (S.phase === 'move') return S.groups.length > 0 || S.undoStack.length >= 2;
  if (S.phase === 'roll' || S.phase === 'over') return S.undoStack.length >= 1;
  return false;
}

async function onUndo() {
  if (!canUndo()) return;
  if (S.phase === 'move' && S.groups.length) {
    S.groups.pop();
    S.selected = null;
    await refreshTurn();
    return;
  }
  if (S.phase === 'over' && S.result) {
    if (S.result.winner === HUMAN) S.score.human -= S.result.points;
    else S.score.ai -= S.result.points;
    storage.set('palamedes.score', S.score);
    S.result = null;
  }
  // 직전 내 턴의 시작으로 되돌린다(그 사이 AI 수도 취소). 같은 주사위로 다시 둔다.
  if (S.phase === 'move') S.undoStack.pop();
  const snap = S.undoStack.pop();
  S.board = snap.board.slice();
  S.history.length = snap.historyLen;
  S.lastMove = snap.lastMove;
  await beginHumanTurn(snap.dice);
  setStatus(`되돌렸어. ${snap.dice[0]}-${snap.dice[1]}로 다시 둬.`);
}

// ---------- 이벤트 ----------
// 상태를 바꾸는 동작은 한 번에 하나만. 진행 중에는 다른 입력(연타 포함)을 무시한다.
function guarded(fn) {
  return async (...args) => {
      S.busy = true;
    renderPanel();
    try {
      await fn(...args);
    } catch (err) {
      setStatus(`오류: ${err.message}`);
      console.error(err);
    } finally {
      S.busy = false;
      render();
    }
  };
}
const act = {
  point: guarded(onPoint), roll: guarded(onRoll), done: guarded(onDone),
  undo: guarded(onUndo), newGame: guarded(newGame),
};
svg.addEventListener('click', (e) => {
  const t = e.target.closest('[data-point]');
  if (t) act.point(Number(t.dataset.point));
});
els.roll.addEventListener('click', act.roll);
els.done.addEventListener('click', act.done);
els.undo.addEventListener('click', act.undo);
els.newGame.addEventListener('click', act.newGame);
document.addEventListener('keydown', (e) => {
  if (e.target.closest('button')) return;
  if (e.key === 'r' || e.key === ' ') { e.preventDefault(); act.roll(); }
  else if (e.key === 'Enter') act.done();
  else if (e.key === 'u' || (e.key === 'z' && (e.ctrlKey || e.metaKey))) act.undo();
});

// 자동 테스트(scripts/ui-smoke.mjs)용 읽기 전용 핸들.
window.__palamedes = { state: S };

act.newGame();
