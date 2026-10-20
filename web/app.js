// 대국 화면: 사람 vs AI(신경망, 기준선 엔진, 랜덤 중 선택). 규칙 판단은 전부 엔진(Worker)에 맡긴다.
// 엔진 진영은 사람 = 0, AI = 1로 고정이고, 말 색과 판 방향은 화면 설정(S.view)일 뿐이다.
// 더블링 큐브: 굴리기 전에 더블할 수 있다. 머니 게임 또는 n점 매치(크로포드 규칙).
// 친절 모드(S.view.coach === 'on'): 수·큐브 힌트, 나쁨 이상의 수·큐브 판단 전 경고와 이유, 기보에 등급 표시.

import {
  AI, BAD, ENGINE_BASELINE, ENGINE_NN, BAR, HUMAN, PLY_BASELINE, PLY_NN,
  applyStep, createEngine, ctxJson, escapeHtml, gradeClass, gradeOf, loadView, saveGame, storage,
} from './common.js';
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
const clone = (x) => JSON.parse(JSON.stringify(x));

// ---------- 상태 ----------
const S = {
  phase: 'loading', // loading | opening | roll | move | ai | cube(AI가 더블, 내 응답 대기) | over
  board: null,
  turn: HUMAN,
  dice: null, // [d1, d2]
  openingDice: null, // [사람, AI]
  turnStart: null,
  groups: [], // 이번 턴에 둔 선택지별 걸음 묶음(무르기 단위)
  ts: null, // 엔진 turn_state
  selected: null,
  // 키보드로 고른 칸(화살표로 이동, Enter·Space로 누름). 지금 누를 수 있는 칸이 아니면 render에서 지운다.
  focus: null,
  history: [], // 수 {side, dice, notation, before, after, cube, grade?, mark?} | 큐브 {type: 'cube', side, action, board, cube, grade?, mark?}
  undoStack: [], // 사람 턴 시작 시점 스냅샷
  lastMove: null, // {side, steps}
  result: null, // {winner, points, passed}
  gameId: null,
  cube: { value: 1, owner: null },
  // 지금 매치. length 0 = 머니. score는 [사람, AI]. post = 크로포드 판이 지났는가.
  match: storage.get('palamedes.match', null),
  matchAtStart: null,
  hint: null, // hint_json 결과(이번 턴 시작 국면 기준)
  cubeHint: null, // {dec, forTaker}
  warning: null, // analyze_move_json 결과(턴 끝내기 전 경고)
  cubeWarn: null, // {kind: 'double'|'roll'|'take'|'pass', dec, loss}
  cubeOffer: null, // AI가 더블했을 때 AI 기준 cube_decision_json 결과
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
  double: $('btn-double'), newGame: $('btn-new'), coach: $('coach'), matchInfo: $('match-info'),
  pipH: $('pip-human'), pipA: $('pip-ai'), pipDiff: $('pip-diff'), moves: $('moves'),
  scoreH: $('score-human'), scoreA: $('score-ai'),
  chipH: $('chip-human'), chipA: $('chip-ai'), nameH: $('name-human'), nameA: $('name-ai'),
  viewButtons: document.querySelectorAll('[data-view]'),
};
function setStatus(t) { els.status.textContent = t; }
const coachOn = () => S.view.coach === 'on';
const other = (side) => 1 - side;

// ---------- 큐브 · 매치 ----------
const isMatch = () => S.match && S.match.length > 0;
const needs = (side) => S.match.length - S.match.score[side];
const isCrawford = () => isMatch() && !S.match.post && (needs(HUMAN) === 1 || needs(AI) === 1);
const matchOver = () => isMatch() && (needs(HUMAN) <= 0 || needs(AI) <= 0);
const ctx = () => ctxJson(S.cube, S.match);
function canDouble(side) {
  if (S.view.ai === 'random' && side === AI) return false;
  if (isCrawford() || S.cube.owner === other(side) || S.cube.value >= 64) return false;
  return !isMatch() || S.cube.value < needs(side);
}
function aiEngine() {
  return S.view.ai === 'baseline' ? [ENGINE_BASELINE, PLY_BASELINE] : [ENGINE_NN, PLY_NN];
}
const cubeDecision = (side, engine = ENGINE_NN) => call('cube_decision_json', S.board, side, engine, ctx());
const actionName = { no_double: '노 더블', double_take: '더블/테이크', double_pass: '더블/패스', too_good: '너무 좋아서 노 더블(개먼 노림)' };

// 지금 누를 수 있는 칸: 출발점을 안 골랐으면 출발점들, 골랐으면 그 도착점들. 바 → 높은 포인트 → 베어오프 순.
function pressable() {
  if (S.phase !== 'move' || !S.ts || S.warning) return [];
  const opts = S.ts.options;
  const list = S.selected === null ? opts.map((o) => o.from) : opts.filter((o) => o.from === S.selected).map((o) => o.to);
  return [...new Set(list)].sort((a, b) => b - a);
}
const pointName = (p) => (p === 0 ? '베어오프' : p === BAR ? '바' : `${p}포인트`);

function moveFocus(delta) {
  const list = pressable();
  if (!list.length) return;
  const i = list.indexOf(S.focus);
  S.focus = list[i < 0 ? (delta > 0 ? 0 : list.length - 1) : (i + delta + list.length) % list.length];
  const what = S.selected === null ? '움직일 말' : `${pointName(S.selected)}에서 갈 칸`;
  setStatus(`${what}: ${pointName(S.focus)} (${list.indexOf(S.focus) + 1}/${list.length}). Enter로 고르고, Esc로 취소.`);
  render();
}

function render() {
  if (!S.board) return;
  const moving = S.phase === 'move' && S.ts;
  const selectable = new Set(moving ? S.ts.options.map((o) => o.from) : []);
  const dests = moving && S.selected !== null ? S.ts.options.filter((o) => o.from === S.selected).map((o) => o.to) : [];
  if (S.focus !== null && !pressable().includes(S.focus)) S.focus = null;
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
    cube: S.cube,
    focus: S.focus,
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

const listHtml = (lines) => `<ul>${lines.map((t) => `<li>${escapeHtml(t)}</li>`).join('')}</ul>`;

function renderCoach() {
  const c = els.coach;
  const show = (cls, html) => {
    c.hidden = false;
    c.className = `coach ${cls}`;
    c.innerHTML = html;
  };
  if (S.cubeWarn) {
    const { kind, dec, loss } = S.cubeWarn;
    const g = gradeOf(loss, false);
    const what = {
      double: ['지금 더블하면', '그래도 더블', 'force-double', '취소', 'cancel'],
      roll: ['더블하지 않고 굴리면', '그냥 굴리기', 'force-roll', '더블하기', 'double'],
      take: ['받으면', '그래도 받기', 'force-take', '다시 고르기', 'cancel'],
      pass: ['포기하면', '그래도 포기', 'force-pass', '다시 고르기', 'cancel'],
    }[kind];
    const forTaker = kind === 'take' || kind === 'pass';
    show('warn', `
      <div class="judge"><span class="tag">엔진 판단</span>
        ${what[0]} 에퀴티를 ${loss.toFixed(3)} 잃어(${g.label} ${g.mark}). 맞는 판단: <b>${
          forTaker ? (dec.take ? '받기' : '포기') : actionName[dec.action]}</b>.</div>
      <div class="approx"><span class="tag">근거(큐브 모델 근사)</span>${listHtml(forTaker ? dec.taker_reasons : dec.reasons)}</div>
      <div class="coach-actions">
        <button type="button" class="btn" data-coach="${what[4]}">${what[3]}</button>
        <button type="button" class="btn primary" data-coach="${what[2]}">${what[1]}</button>
      </div>`);
    return;
  }
  if (S.phase === 'cube' && S.cubeOffer) {
    const d = S.cubeOffer;
    const hint = S.cubeHint
      ? `<div class="judge"><span class="tag">엔진 판단</span><b>${d.take ? '받는' : '포기하는'} 게 맞아.</b></div>
         <div class="approx"><span class="tag">근거(큐브 모델 근사)</span>${listHtml(d.taker_reasons)}</div>`
      : '';
    show('hint', `
      <div class="judge">AI가 더블했어. 받으면 큐브 ${S.cube.value * 2}로 계속하고 큐브는 내 것이 돼. 포기하면 ${S.cube.value}점을 내줘.</div>
      ${hint}
      <div class="coach-actions">
        <button type="button" class="btn" data-coach="pass">포기</button>
        <button type="button" class="btn primary" data-coach="take">받기</button>
      </div>`);
    return;
  }
  if (S.warning) {
    const a = S.warning;
    show('warn', `
      <div class="judge"><span class="tag">엔진 판단</span>
        이 수(${escapeHtml(a.played.notation)})는 최선 수(${escapeHtml(a.best.notation)})보다 에퀴티가 ${a.loss.toFixed(3)} 낮아.
        등급: <b>${a.label} ${a.mark}</b> · 후보 ${a.count}개 중 ${a.rank}위.</div>
      ${reasonsHtml(a.explanation)}
      <div class="coach-actions">
        <button type="button" class="btn" data-coach="redo">다시 두기</button>
        <button type="button" class="btn primary" data-coach="commit">그래도 두기</button>
      </div>`);
    return;
  }
  if (S.cubeHint && S.phase === 'roll') {
    const d = S.cubeHint.dec;
    show('hint', `
      <div class="judge"><span class="tag">엔진 판단</span>지금 큐브 판단: <b>${actionName[d.action]}</b>.</div>
      <div class="approx"><span class="tag">근거(큐브 모델 근사)</span>${listHtml(d.reasons)}</div>`);
    return;
  }
  if (S.hint && S.phase === 'move') {
    const h = S.hint;
    const second = h.top[1];
    const vs = second ? ` 2위 수(${escapeHtml(second.notation)})보다 에퀴티가 ${h.margin.toFixed(3)} 높아.` : ' 둘 수 있는 수가 이것뿐이야.';
    const note = S.groups.length ? ' (이번 턴 처음 국면 기준이라 무르기로 되돌린 뒤 두면 돼)' : '';
    show('hint', `
      <div class="judge"><span class="tag">엔진 판단</span>
        추천 수: <b>${escapeHtml(h.best.notation)}</b>.${vs} 승리 확률 ${(h.best.probs.win * 100).toFixed(1)}%.${note}</div>
      ${reasonsHtml(h.explanation)}`);
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
  if (isMatch()) {
    els.scoreH.textContent = S.match.score[HUMAN];
    els.scoreA.textContent = S.match.score[AI];
    els.matchInfo.textContent = `${S.match.length}점 매치${isCrawford() ? ' · 크로포드' : ''}`;
  } else {
    els.scoreH.textContent = S.score.human;
    els.scoreA.textContent = S.score.ai;
    els.matchInfo.textContent = '머니';
  }
  const tone = (side) => ((side === HUMAN) === (S.view.color === 'white') ? 'white' : 'black');
  els.chipH.className = `chip ${tone(HUMAN)}`;
  els.chipA.className = `chip ${tone(AI)}`;
  els.nameH.textContent = `나 (${tone(HUMAN) === 'white' ? '백' : '흑'})`;
  els.nameA.textContent = `AI (${tone(AI) === 'white' ? '백' : '흑'})`;
  for (const btn of els.viewButtons) {
    const [key, value] = btn.dataset.view.split(':');
    btn.setAttribute('aria-pressed', String(String(S.view[key]) === value));
  }

  const blocked = S.busy || !!S.warning || !!S.cubeWarn;
  els.roll.disabled = blocked || !(S.phase === 'opening' || S.phase === 'roll');
  els.double.disabled = blocked || S.phase !== 'roll' || !canDouble(HUMAN);
  els.done.disabled = blocked || !(S.phase === 'move' && S.ts && S.ts.done);
  els.undo.disabled = blocked || !canUndo();
  els.hint.hidden = !coachOn();
  const hintable = (S.phase === 'move' && S.ts && S.ts.need > 0) || (S.phase === 'roll' && canDouble(HUMAN)) || S.phase === 'cube';
  els.hint.disabled = blocked || !hintable;
  els.newGame.textContent = isMatch() && S.phase === 'over' && !matchOver() ? '다음 판' : '새 게임';

  // 기보: 한 줄에 (사람, AI) 한 쌍. 큐브 동작은 그 칸에 같이 적는다. 친절 모드면 내 판단에 등급 기호.
  const rows = [];
  let row = null;
  let pendingCube = ['', ''];
  const cubeText = { double: '더블', take: '받음', pass: '포기' };
  for (const h of S.history) {
    const mark = h.mark ? ` <b class="mark ${gradeClass(h.grade)}">${h.mark}</b>` : '';
    if (h.type === 'cube') {
      pendingCube[h.side] += `<span class="cube-act">${cubeText[h.action]} ${h.action === 'double' ? h.cube.value * 2 : ''}${mark}</span> `;
      if (h.action === 'pass') {
        row = [null, null];
        rows.push(row);
        row[h.side] = { html: pendingCube[h.side] };
        pendingCube = ['', ''];
      }
      continue;
    }
    if (!row || h.side === HUMAN || row[AI]) {
      row = [null, null];
      rows.push(row);
    }
    row[h.side] = { html: `${pendingCube[h.side]}<span class="dice">${h.dice[0]}${h.dice[1]}</span>${escapeHtml(h.notation)}${mark}` };
    pendingCube[h.side] = '';
  }
  const cell = (c) => (c ? `<span>${c.html}</span>` : '<span class="empty">—</span>');
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
    match: S.matchAtStart && S.matchAtStart.length ? S.matchAtStart : null,
    entries: S.history.map(({ grade, mark, ...rest }) => rest),
    result: S.result,
  });
  storage.set('palamedes.match', S.match);
}

function snapshot(dice) {
  return {
    board: S.board.slice(), dice, historyLen: S.history.length, lastMove: S.lastMove,
    cube: clone(S.cube), match: clone(S.match), result: S.result, scoreTally: clone(S.score),
  };
}

// ---------- 진행 ----------
async function newGame() {
  const board = await call('start_board');
  const want = Number(S.view.match) || 0;
  // 게임 방식이 바뀌었거나 매치가 끝났으면 새 매치. 같은 매치를 이어 가는데 직전 판이 크로포드 판이었으면 이제 크로포드 이후.
  if (!S.match || S.match.length !== want || matchOver()) S.match = { length: want, score: [0, 0], post: false };
  else if (S.result && S.result.crawford) S.match.post = true;
  Object.assign(S, {
    phase: 'opening', board, turn: HUMAN, dice: null, openingDice: null, turnStart: null,
    groups: [], ts: null, selected: null, history: [], undoStack: [], lastMove: null, result: null,
    gameId: Date.now(), hint: null, cubeHint: null, warning: null, cubeWarn: null, cubeOffer: null,
    cube: { value: 1, owner: null }, matchAtStart: clone(S.match),
  });
  storage.set('palamedes.match', S.match);
  await updatePips();
  const m = isMatch() ? `${S.match.length}점 매치, ${S.match.score[HUMAN]} : ${S.match.score[AI]}${isCrawford() ? ', 크로포드 판(큐브 금지)' : ''}. ` : '';
  setStatus(`${m}첫 수를 정하자. 굴리기를 누르면 서로 주사위를 하나씩 굴려서 높은 쪽이 먼저 둬.`);
  render();
}

async function onRoll(force = false) {
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
  if (S.phase !== 'roll') return;
  // 친절 모드: 더블할 때인데 그냥 굴리면 알려 준다.
  if (coachOn() && !force && canDouble(HUMAN)) {
    const dec = await cubeDecision(HUMAN);
    if (dec.double && dec.double_gain >= BAD) {
      S.cubeWarn = { kind: 'roll', dec, loss: dec.double_gain };
      setStatus('잠깐, 지금은 더블할 때일 수 있어.');
      render();
      return;
    }
  }
  await beginHumanTurn(roll2());
}

async function beginHumanTurn(dice, keepStatus = false) {
  S.undoStack.push(snapshot(dice));
  Object.assign(S, { turn: HUMAN, dice, turnStart: S.board.slice(), groups: [], selected: null, phase: 'move', hint: null, cubeHint: null, warning: null });
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
  if (!coachOn()) return;
  if (S.phase === 'cube' && S.cubeOffer) {
    S.cubeHint = { dec: S.cubeOffer, forTaker: true };
    setStatus('받을지 포기할지 엔진 판단을 아래에 보여 줬어.');
  } else if (S.phase === 'roll' && canDouble(HUMAN)) {
    S.cubeHint = { dec: await cubeDecision(HUMAN), forTaker: false };
    setStatus('지금 더블할지에 대한 엔진 판단이야.');
  } else if (S.phase === 'move' && S.ts && S.ts.need > 0) {
    S.hint = await call('hint_json', S.turnStart, HUMAN, S.dice[0], S.dice[1], ENGINE_NN, PLY_NN, ctx());
    setStatus(S.groups.length ? '힌트는 이번 턴 처음 국면 기준이야.' : '번호가 붙은 칸이 추천 수야.');
  }
  render();
}

async function onDone() {
  if (S.phase !== 'move' || !S.ts || !S.ts.done || S.warning) return;
  let analysis = null;
  if (coachOn() && S.ts.need > 0) {
    analysis = await call('analyze_move_json', S.turnStart, HUMAN, S.dice[0], S.dice[1], S.ts.board, ENGINE_NN, PLY_NN, ctx());
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
  const entry = { side: HUMAN, dice: S.dice, notation: S.ts.notation, before: S.turnStart.slice(), after: S.ts.board.slice(), cube: clone(S.cube) };
  if (analysis) Object.assign(entry, { grade: analysis.grade, mark: analysis.mark });
  S.history.push(entry);
  S.lastMove = { side: HUMAN, steps: S.ts.steps };
  persist();
  if (await checkOver()) return;
  await aiTurn();
}

function pushCube(side, action, grade) {
  const e = { type: 'cube', side, action, board: S.board.slice(), cube: clone(S.cube) };
  if (grade) Object.assign(e, { grade: grade.grade, mark: grade.mark });
  S.history.push(e);
}

// 내가 더블.
async function onDouble(force = false) {
  if (S.phase !== 'roll' || !canDouble(HUMAN)) return;
  let grade = null;
  if (coachOn()) {
    const dec = await cubeDecision(HUMAN);
    const loss = dec.double ? 0 : Math.max(0, -dec.double_gain);
    grade = gradeOf(loss, dec.double);
    if (!force && loss >= BAD) {
      S.cubeWarn = { kind: 'double', dec, loss };
      setStatus('잠깐, 지금 더블하는 건 손해일 수 있어.');
      render();
      return;
    }
  }
  S.cubeWarn = null;
  S.cubeHint = null;
  pushCube(HUMAN, 'double', grade);
  // AI 응답: 내 입장(더블한 쪽)에서 계산한 "상대가 받아야 하는가".
  let take = true;
  if (S.view.ai !== 'random') {
    const [engine] = aiEngine();
    take = (await cubeDecision(HUMAN, engine)).take;
  }
  pushCube(AI, take ? 'take' : 'pass');
  if (take) {
    S.cube = { value: S.cube.value * 2, owner: AI };
    persist();
    setStatus(`AI가 받았어. 큐브 ${S.cube.value}. 굴리기를 눌러.`);
    render();
  } else {
    await endGame(HUMAN, S.cube.value, true);
  }
}

// AI가 더블했을 때 내 응답.
async function onAnswer(take, force = false) {
  if (S.phase !== 'cube' || !S.cubeOffer) return;
  const d = S.cubeOffer;
  // 받는 쪽(나)의 손실: 더블한 쪽 기준 받으면 dt, 포기하면 dp. 나는 그 반대.
  const loss = take ? (d.take ? 0 : d.double_take - d.double_pass) : (d.take ? d.double_pass - d.double_take : 0);
  const grade = coachOn() ? gradeOf(Math.max(0, loss), take === d.take) : null;
  if (coachOn() && !force && loss >= BAD) {
    S.cubeWarn = { kind: take ? 'take' : 'pass', dec: d, loss };
    setStatus(`잠깐, ${take ? '받는' : '포기하는'} 건 손해일 수 있어.`);
    render();
    return;
  }
  S.cubeWarn = null;
  S.cubeHint = null;
  S.cubeOffer = null;
  pushCube(HUMAN, take ? 'take' : 'pass', grade);
  if (!take) {
    await endGame(AI, S.cube.value, true);
    return;
  }
  S.cube = { value: S.cube.value * 2, owner: HUMAN };
  persist();
  setStatus(`받았어. 큐브 ${S.cube.value}, 큐브는 내 것이야.`);
  await aiRollAndMove();
}

async function onCoach(action) {
  if (S.cubeWarn) {
    const kind = S.cubeWarn.kind;
    S.cubeWarn = null;
    if (action === 'force-double') return onDouble(true);
    if (action === 'force-roll') return onRoll(true);
    if (action === 'double') return onDouble(true);
    if (action === 'force-take') return onAnswer(true, true);
    if (action === 'force-pass') return onAnswer(false, true);
    setStatus(kind === 'take' || kind === 'pass' ? '받을지 포기할지 다시 골라.' : '취소했어. 굴리기를 누르거나 더블해.');
    render();
    return;
  }
  if (S.phase === 'cube' && (action === 'take' || action === 'pass')) return onAnswer(action === 'take');
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

// AI 차례. 첫 수(dice가 주어짐)가 아니면 굴리기 전에 더블할지 정한다.
async function aiTurn(dice = null) {
  Object.assign(S, { phase: 'ai', turn: AI, selected: null, ts: null, hint: null, cubeHint: null, warning: null });
  if (!dice && canDouble(AI)) {
    const [engine] = aiEngine();
    const dec = await cubeDecision(AI, engine);
    if (dec.double) {
      pushCube(AI, 'double');
      S.cubeOffer = dec;
      S.phase = 'cube';
      S.dice = null;
      persist();
      setStatus(`AI가 더블했어(큐브 ${S.cube.value * 2}). 받을지 포기할지 골라.`);
      render();
      return;
    }
  }
  await aiRollAndMove(dice);
}

async function aiRollAndMove(dice = null) {
  dice = dice || roll2();
  Object.assign(S, { phase: 'ai', turn: AI, dice });
  render();
  await sleep(450);
  const before = S.board.slice();
  let m;
  if (S.view.ai === 'random') {
    const moves = await call('legal_moves_json', S.board, AI, dice[0], dice[1]);
    m = moves[Math.floor(Math.random() * moves.length)];
  } else {
    const [engine, ply] = aiEngine();
    m = (await call('rank_moves_json', S.board, AI, dice[0], dice[1], engine, ply, ctx()))[0];
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
  S.history.push({ side: AI, dice, notation: m.notation, before, after: m.board.slice(), cube: clone(S.cube) });
  S.lastMove = { side: AI, steps: m.steps };
  persist();
  await updatePips();
  if (await checkOver()) return;
  S.phase = 'roll';
  setStatus(`${els.status.textContent} — 이제 내 차례야. 굴리기를 눌러${canDouble(HUMAN) ? '(더블도 할 수 있어)' : ''}.`);
  render();
}

async function checkOver() {
  const r = await call('result_json', S.board);
  if (!r) return false;
  await endGame(r.winner, r.points * S.cube.value, false, r.points);
  return true;
}

async function endGame(winner, points, passed, mult = 1) {
  const crawford = isCrawford();
  S.result = { winner, points, passed, crawford };
  S.phase = 'over';
  S.cubeOffer = null;
  const kind = passed ? '포기' : mult === 3 ? '백개먼' : mult === 2 ? '개먼' : '싱글';
  const who = winner === HUMAN ? '내가 이겼어' : 'AI가 이겼어';
  if (isMatch()) {
    S.match.score[winner] = Math.min(S.match.length, S.match.score[winner] + points);
    const sc = `${S.match.score[HUMAN]} : ${S.match.score[AI]}`;
    if (matchOver()) {
      setStatus(`${who}(${kind}, ${points}점). ${S.match.length}점 매치 ${sc}로 ${winner === HUMAN ? '내가' : 'AI가'} 매치를 이겼어. 새 게임을 누르면 새 매치야.`);
    } else {
      setStatus(`${who}(${kind}, ${points}점). 매치 ${sc}. 다음 판을 눌러.`);
    }
  } else {
    if (winner === HUMAN) S.score.human += points;
    else S.score.ai += points;
    storage.set('palamedes.score', S.score);
    setStatus(`${who}. ${kind}${S.cube.value > 1 ? `, 큐브 ${S.cube.value}` : ''} → ${points}점. 분석 화면에서 복기할 수 있어.`);
  }
  persist();
  render();
}

function canUndo() {
  if (S.phase === 'move') return S.groups.length > 0 || S.undoStack.length >= 2;
  if (S.phase === 'roll' || S.phase === 'over' || S.phase === 'cube') return S.undoStack.length >= 1;
  return false;
}

async function onUndo() {
  if (!canUndo() || S.warning || S.cubeWarn) return;
  if (S.phase === 'move' && S.groups.length) {
    S.groups.pop();
    S.selected = null;
    await refreshTurn();
    return;
  }
  // 직전 내 턴의 시작으로 되돌린다(그 사이 AI 수·큐브 동작·점수도 취소). 같은 주사위로 다시 둔다.
  if (S.phase === 'move') S.undoStack.pop();
  const snap = S.undoStack.pop();
  S.board = snap.board.slice();
  S.history.length = snap.historyLen;
  S.lastMove = snap.lastMove;
  S.cube = clone(snap.cube);
  S.match = clone(snap.match);
  S.score = clone(snap.scoreTally);
  storage.set('palamedes.score', S.score);
  S.result = null;
  S.cubeOffer = null;
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
  point: guarded(onPoint), roll: guarded(() => onRoll()), done: guarded(onDone), hint: guarded(onHint),
  undo: guarded(onUndo), newGame: guarded(newGame), coach: guarded(onCoach), double: guarded(() => onDouble()),
};
// 화면 설정은 진행 중에도 바로 바꿀 수 있다(판 상태와 무관). 게임 방식(match)은 다음 새 게임부터.
for (const btn of els.viewButtons) {
  btn.addEventListener('click', () => {
    const [key, value] = btn.dataset.view.split(':');
    S.view[key] = value;
    if (key === 'coach' && value === 'off') {
      S.hint = null;
      S.cubeHint = null;
    }
    if (key === 'match') {
      const fresh = S.phase === 'opening' && S.history.length === 0;
      if (fresh) {
        S.match = null;
        storage.set('palamedes.view', S.view);
        act.newGame();
        return;
      }
      setStatus(`다음 새 게임부터 ${value === '0' ? '머니 게임' : `${value}점 매치`}로 시작해.`);
    }
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
els.double.addEventListener('click', act.double);
els.newGame.addEventListener('click', act.newGame);
// 판 키보드 조작: 화살표로 누를 수 있는 칸을 돌고, Enter·Space로 누른다(마우스로 그 칸을 누른 것과 같다).
// 칸을 누른 뒤에는 다음 고를 칸 목록의 첫 칸으로 포커스를 옮긴다.
async function pressFocus() {
  await act.point(S.focus);
  const list = pressable();
  if (list.length) moveFocus(0);
}
document.addEventListener('keydown', (e) => {
  if (e.target.closest('select, input, textarea')) return;
  // 버튼·링크에 포커스가 있으면 Enter·Space는 그 버튼을 누르는 기본 동작에 맡긴다.
  if ((e.key === 'Enter' || e.key === ' ') && e.target.closest('button, a')) return;
  const arrows = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 };
  if (e.key in arrows && pressable().length) { e.preventDefault(); moveFocus(arrows[e.key]); }
  else if ((e.key === 'Enter' || e.key === ' ') && S.focus !== null) { e.preventDefault(); pressFocus(); }
  else if (e.key === 'Escape' && (S.focus !== null || S.selected !== null)) {
    S.focus = null;
    S.selected = null;
    setStatus('움직일 말을 골라.');
    render();
  }
  else if (e.key === 'r' || e.key === ' ') { e.preventDefault(); act.roll(); }
  else if (e.key === 'Enter') act.done();
  else if (e.key === 'h') act.hint();
  else if (e.key === 'd') act.double();
  else if (e.key === 'u' || (e.key === 'z' && (e.ctrlKey || e.metaKey))) act.undo();
});

// 자동 테스트(scripts/ui-smoke.mjs)용 읽기 전용 핸들.
window.__palamedes = { state: S };

act.newGame();
