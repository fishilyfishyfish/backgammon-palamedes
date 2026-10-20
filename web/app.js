// 대국 화면: 사람 vs AI(신경망, 기준선 엔진, 랜덤 중 선택). 규칙 판단은 전부 엔진(Worker)에 맡긴다.
// 엔진 진영은 사람 = 0, AI = 1로 고정이고, 말 색과 판 방향은 화면 설정(S.view)일 뿐이다.
// 친절 모드(S.view.coach === 'on'): 힌트, 큰 실수(나쁨 이상) 전 경고와 이유, 수마다 등급 표시.

import { AI, BAD, ENGINE_BASELINE, ENGINE_NN, HUMAN, PLY_BASELINE, PLY_NN, applyStep, createEngine, escapeHtml, gradeClass, loadView, saveGame, storage } from './common.js';
import { diceList, renderBoard } from './board-view.js';

const call = createEngine((msg) => setStatus(`엔진을 불러오지 못했어: ${msg}`));

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
const flatSteps = (groups) => groups.flat().map(({ from, to, die }) => ({ from, to, die }));

// ---------- 상태 ----------
const S = {
  phase: 'loading', // loading | opening | roll | move | ai | over
  board: null,
  turn: HUMAN,
  dice: null, // [d1, d2]
  openingDice: null, // [사람, AI]
  turnStart: null,
  groups: [], // 이번 턴에 둔 선택지별 걸음 묶음(무르기 단위)
  ts: null, // 엔진 turn_state
  selected: null,
  history: [], // {side, dice, notation, before, after, grade?, mark?}
  undoStack: [], // 사람 턴 시작 시점 {board, dice, historyLen, lastMove}
  lastMove: null, // {side, steps}
  result: null, // {winner, points}
  gameId: null,
  hint: null, // hint_json 결과(이번 턴 시작 국면 기준)
  warning: null, // analyze_move_json 결과(턴 끝내기 전 경고)
  score: storage.get('palamedes.score', { human: 0, ai: 0 }),
  pips: [167, 167],
  busy: false,
  view: loadView(),
};

// ---------- DOM ----------
const $ = (id) => document.getElementById(id);
const svg = $('board');
const els = {
  status: $('status'), roll: $('btn-roll'), undo: $('btn-undo'), done: $('btn-done'), hint: $('btn-hint'),
  newGame: $('btn-new'), coach: $('coach'),
  pipH: $('pip-human'), pipA: $('pip-ai'), pipDiff: $('pip-diff'), moves: $('moves'),
  scoreH: $('score-human'), scoreA: $('score-ai'),
  chipH: $('chip-human'), chipA: $('chip-ai'), nameH: $('name-human'), nameA: $('name-ai'),
  viewButtons: document.querySelectorAll('[data-view]'),
};
function setStatus(t) { els.status.textContent = t; }
const coachOn = () => S.view.coach === 'on';

function render() {
  if (!S.board) return;
  const moving = S.phase === 'move' && S.ts;
  const selectable = new Set(moving ? S.ts.options.map((o) => o.from) : []);
  const dests = moving && S.selected !== null ? S.ts.options.filter((o) => o.from === S.selected).map((o) => o.to) : [];
  renderBoard(svg, {
    board: S.board,
    view: S.view,
    selectable,
    selected: S.selected,
    dests,
    opening: S.phase === 'opening' ? S.openingDice : null,
    dice: S.dice && S.phase !== 'opening' ? { side: S.turn, list: diceList(S.dice, S.phase === 'move' ? flatSteps(S.groups) : []) } : null,
    lastMove: S.phase !== 'move' ? S.lastMove : null,
    // 힌트는 아직 아무것도 안 뒀을 때만 판에 그린다(턴 시작 기준 수라서).
    hint: S.hint && S.phase === 'move' && S.groups.length === 0 ? { steps: S.hint.best.steps } : null,
  });
  renderPanel();
}

function reasonsHtml(ex) {
  if (!ex) return '';
  const items = ex.reasons.length
    ? ex.reasons.map((r) => `<li>${escapeHtml(r.text)}</li>`).join('')
    : '<li>특징값으로는 이 차이를 설명하지 못했어.</li>';
  return `<div class="approx"><span class="tag">설명(근사)</span><ul>${items}</ul></div>`;
}

function renderCoach() {
  const c = els.coach;
  if (S.warning) {
    const a = S.warning;
    c.hidden = false;
    c.className = 'coach warn';
    c.innerHTML = `
      <div class="judge"><span class="tag">엔진 판단</span>
        이 수(${escapeHtml(a.played.notation)})는 최선 수(${escapeHtml(a.best.notation)})보다 에퀴티가 ${a.loss.toFixed(3)} 낮아.
        등급: <b>${a.label} ${a.mark}</b> · 후보 ${a.count}개 중 ${a.rank}위.</div>
      ${reasonsHtml(a.explanation)}
      <div class="coach-actions">
        <button type="button" class="btn" data-coach="redo">다시 두기</button>
        <button type="button" class="btn primary" data-coach="commit">그래도 두기</button>
      </div>`;
    return;
  }
  if (S.hint && S.phase === 'move') {
    const h = S.hint;
    c.hidden = false;
    c.className = 'coach hint';
    const second = h.top[1];
    const vs = second ? ` 2위 수(${escapeHtml(second.notation)})보다 에퀴티가 ${h.margin.toFixed(3)} 높아.` : ' 둘 수 있는 수가 이것뿐이야.';
    const note = S.groups.length ? ' (이번 턴 처음 국면 기준이라 무르기로 되돌린 뒤 두면 돼)' : '';
    c.innerHTML = `
      <div class="judge"><span class="tag">엔진 판단</span>
        추천 수: <b>${escapeHtml(h.best.notation)}</b>.${vs} 승리 확률 ${(h.best.probs.win * 100).toFixed(1)}%.${note}</div>
      ${reasonsHtml(h.explanation)}`;
    return;
  }
  c.hidden = true;
  c.innerHTML = '';
}

function renderPanel() {
  els.pipH.textContent = S.pips[HUMAN];
  els.pipA.textContent = S.pips[AI];
  const d = S.pips[AI] - S.pips[HUMAN];
  els.pipDiff.textContent = d === 0 ? '핍 카운트가 같아.' : d > 0 ? `내가 ${d}핍 앞서.` : `AI가 ${-d}핍 앞서.`;
  els.scoreH.textContent = S.score.human;
  els.scoreA.textContent = S.score.ai;
  const tone = (side) => ((side === HUMAN) === (S.view.color === 'white') ? 'white' : 'black');
  els.chipH.className = `chip ${tone(HUMAN)}`;
  els.chipA.className = `chip ${tone(AI)}`;
  els.nameH.textContent = `나 (${tone(HUMAN) === 'white' ? '백' : '흑'})`;
  els.nameA.textContent = `AI (${tone(AI) === 'white' ? '백' : '흑'})`;
  for (const btn of els.viewButtons) {
    const [key, value] = btn.dataset.view.split(':');
    btn.setAttribute('aria-pressed', String(S.view[key] === value));
  }

  els.roll.disabled = S.busy || !(S.phase === 'opening' || S.phase === 'roll');
  els.done.disabled = S.busy || !!S.warning || !(S.phase === 'move' && S.ts && S.ts.done);
  els.undo.disabled = S.busy || !!S.warning || !canUndo();
  els.hint.hidden = !coachOn();
  els.hint.disabled = S.busy || !!S.warning || S.phase !== 'move' || !S.ts || S.ts.need === 0;

  // 기보: 한 줄에 (사람, AI) 한 쌍. 친절 모드면 내 수에 등급 기호.
  const rows = [];
  let row = null;
  for (const h of S.history) {
    if (!row || h.side === HUMAN || row[AI]) {
      row = [null, null];
      rows.push(row);
    }
    row[h.side] = h;
  }
  const cell = (h) => {
    if (!h) return '<span class="empty">—</span>';
    const mark = h.mark ? ` <b class="mark ${gradeClass(h.grade)}">${h.mark}</b>` : '';
    return `<span><span class="dice">${h.dice[0]}${h.dice[1]}</span>${escapeHtml(h.notation)}${mark}</span>`;
  };
  els.moves.innerHTML = rows.map((r) => `<li><div class="mv">${cell(r[HUMAN])}${cell(r[AI])}</div></li>`).join('');
  els.moves.scrollTop = els.moves.scrollHeight;
  renderCoach();
}

async function updatePips() {
  S.pips = await call('pip_counts', S.board);
}

function persist() {
  if (!S.gameId) return;
  saveGame({
    id: S.gameId,
    date: new Date(S.gameId).toISOString(),
    ai: S.view.ai,
    entries: S.history.map(({ side, dice, notation, before, after }) => ({ side, dice, notation, before, after })),
    result: S.result,
  });
}

// ---------- 진행 ----------
async function newGame() {
  const board = await call('start_board');
  Object.assign(S, {
    phase: 'opening', board, turn: HUMAN, dice: null, openingDice: null, turnStart: null,
    groups: [], ts: null, selected: null, history: [], undoStack: [], lastMove: null, result: null,
    gameId: Date.now(), hint: null, warning: null,
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
  Object.assign(S, { turn: HUMAN, dice, turnStart: S.board.slice(), groups: [], selected: null, phase: 'move', hint: null, warning: null });
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
  if (S.phase !== 'move' || !S.ts || S.warning) return;
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

async function onHint() {
  if (!coachOn() || S.phase !== 'move' || !S.ts || S.ts.need === 0) return;
  S.hint = await call('hint_json', S.turnStart, HUMAN, S.dice[0], S.dice[1], ENGINE_NN, PLY_NN);
  setStatus(S.groups.length ? '힌트는 이번 턴 처음 국면 기준이야.' : '번호가 붙은 칸이 추천 수야.');
  render();
}

async function onDone() {
  if (S.phase !== 'move' || !S.ts || !S.ts.done || S.warning) return;
  let analysis = null;
  if (coachOn() && S.ts.need > 0) {
    analysis = await call('analyze_move_json', S.turnStart, HUMAN, S.dice[0], S.dice[1], S.ts.board, ENGINE_NN, PLY_NN);
    if (analysis.loss >= BAD) {
      S.warning = analysis;
      setStatus('잠깐, 큰 실수일 수 있어. 아래 설명을 보고 정해.');
      render();
      return;
    }
  }
  await commitHumanMove(analysis);
}

async function commitHumanMove(analysis) {
  S.warning = null;
  S.hint = null;
  const entry = { side: HUMAN, dice: S.dice, notation: S.ts.notation, before: S.turnStart.slice(), after: S.ts.board.slice() };
  if (analysis) Object.assign(entry, { grade: analysis.grade, mark: analysis.mark });
  S.history.push(entry);
  S.lastMove = { side: HUMAN, steps: S.ts.steps };
  persist();
  if (await checkOver()) return;
  await aiTurn(roll2());
}

async function onCoach(action) {
  if (!S.warning) return;
  if (action === 'commit') {
    await commitHumanMove(S.warning);
  } else {
    // 다시 두기: 이번 턴에 둔 걸음을 모두 되돌린다.
    S.warning = null;
    S.groups = [];
    S.selected = null;
    await refreshTurn();
  }
}

async function aiTurn(dice) {
  Object.assign(S, { phase: 'ai', turn: AI, dice, selected: null, ts: null, hint: null, warning: null });
  render();
  await sleep(450);
  const before = S.board.slice();
  let m;
  if (S.view.ai === 'random') {
    const moves = await call('legal_moves_json', S.board, AI, dice[0], dice[1]);
    m = moves[Math.floor(Math.random() * moves.length)];
  } else {
    const [engine, ply] = S.view.ai === 'baseline' ? [ENGINE_BASELINE, PLY_BASELINE] : [ENGINE_NN, PLY_NN];
    m = (await call('rank_moves_json', S.board, AI, dice[0], dice[1], engine, ply))[0];
  }
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
  S.history.push({ side: AI, dice, notation: m.notation, before, after: m.board.slice() });
  S.lastMove = { side: AI, steps: m.steps };
  persist();
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
  persist();
  setStatus(`${r.winner === HUMAN ? '내가 이겼어' : 'AI가 이겼어'}. ${kind} ${r.points}점. 분석 화면에서 복기할 수 있어.`);
  render();
  return true;
}

function canUndo() {
  if (S.phase === 'move') return S.groups.length > 0 || S.undoStack.length >= 2;
  if (S.phase === 'roll' || S.phase === 'over') return S.undoStack.length >= 1;
  return false;
}

async function onUndo() {
  if (!canUndo() || S.warning) return;
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
  persist();
  await beginHumanTurn(snap.dice);
  setStatus(`되돌렸어. ${snap.dice[0]}-${snap.dice[1]}로 다시 둬.`);
}

// ---------- 이벤트 ----------
// 상태를 바꾸는 동작은 한 번에 하나만. 진행 중에는 다른 입력(연타 포함)을 무시한다.
function guarded(fn) {
  return async (...args) => {
    if (S.busy) return;
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
  point: guarded(onPoint), roll: guarded(onRoll), done: guarded(onDone), hint: guarded(onHint),
  undo: guarded(onUndo), newGame: guarded(newGame), coach: guarded(onCoach),
};
// 화면 설정은 진행 중에도 바로 바꿀 수 있다(판 상태와 무관).
for (const btn of els.viewButtons) {
  btn.addEventListener('click', () => {
    const [key, value] = btn.dataset.view.split(':');
    S.view[key] = value;
    if (key === 'coach' && value === 'off') S.hint = null;
    storage.set('palamedes.view', S.view);
    render();
  });
}
svg.addEventListener('click', (e) => {
  const t = e.target.closest('[data-point]');
  if (t) act.point(Number(t.dataset.point));
});
els.coach.addEventListener('click', (e) => {
  const t = e.target.closest('[data-coach]');
  if (t) act.coach(t.dataset.coach);
});
els.roll.addEventListener('click', act.roll);
els.done.addEventListener('click', act.done);
els.undo.addEventListener('click', act.undo);
els.hint.addEventListener('click', act.hint);
els.newGame.addEventListener('click', act.newGame);
document.addEventListener('keydown', (e) => {
  if (e.target.closest('button, select, a')) return;
  if (e.key === 'r' || e.key === ' ') { e.preventDefault(); act.roll(); }
  else if (e.key === 'Enter') act.done();
  else if (e.key === 'h') act.hint();
  else if (e.key === 'u' || (e.key === 'z' && (e.ctrlKey || e.metaKey))) act.undo();
});

// 자동 테스트(scripts/ui-smoke.mjs)용 읽기 전용 핸들.
window.__palamedes = { state: S };

act.newGame();
