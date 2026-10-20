// 분석 화면: 저장된 기보를 수마다 엔진(신경망 1수 탐색)으로 평가하고, 실수 등급과 설명을 보여 준다.

import { AI, ENGINE_NN, HUMAN, createEngine, escapeHtml, gradeClass, loadGames, loadView } from './common.js';
import { diceList, renderBoard } from './board-view.js';

const $ = (id) => document.getElementById(id);
const svg = $('board');
const els = {
  select: $('game-select'), status: $('status'), summary: $('summary'), prev: $('btn-prev'), next: $('btn-next'),
  pos: $('pos'), detail: $('detail'), moves: $('moves'), show: document.querySelectorAll('[data-show]'),
};
const setStatus = (t) => { els.status.textContent = t; };
const call = createEngine((msg) => setStatus(`엔진을 불러오지 못했어: ${msg}`));

const A = {
  games: loadGames(),
  game: null,
  results: [], // entries와 같은 순서의 analyze_move_json 결과(분석 전이면 undefined)
  cur: 0,
  show: 'played', // before | played | best
  view: loadView(),
  run: 0, // 분석 중에 기보를 바꾸면 이전 작업을 버리기 위한 번호
};

const sideName = (side) => (side === HUMAN ? '나' : 'AI');
const pct = (x) => `${(x * 100).toFixed(1)}%`;

function gameLabel(g) {
  const d = new Date(g.date);
  const when = `${d.getMonth() + 1}/${d.getDate()} ${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
  const res = g.result ? `${g.result.winner === HUMAN ? '내 승' : 'AI 승'} ${g.result.points}점` : '진행 중';
  return `${when} · ${res} · ${g.entries.length}수`;
}

function fillSelect() {
  els.select.innerHTML = A.games.map((g) => `<option value="${g.id}">${escapeHtml(gameLabel(g))}</option>`).join('');
}

async function openGame(id) {
  A.game = A.games.find((g) => String(g.id) === String(id)) || null;
  A.results = [];
  A.cur = 0;
  const run = ++A.run;
  render();
  if (!A.game) return;
  const entries = A.game.entries;
  for (let i = 0; i < entries.length; i++) {
    if (run !== A.run) return;
    const e = entries[i];
    setStatus(`분석 중… ${i + 1}/${entries.length}`);
    try {
      A.results[i] = await call('analyze_move_json', e.before, e.side, e.dice[0], e.dice[1], e.after, ENGINE_NN);
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
    const rs = A.game.entries.map((e, i) => [e, A.results[i]]).filter(([e, r]) => e.side === side && r && !r.error && r.count > 1);
    const n = rs.length;
    const loss = rs.reduce((s, [, r]) => s + r.loss, 0);
    const count = (g) => rs.filter(([, r]) => r.grade === g).length;
    out.push(`<div><dt>${sideName(side)} · 선택이 있던 수 ${n}개</dt>
      <dd class="small">평균 손실 ${n ? (loss / n).toFixed(3) : '—'}<br>
      <span class="grade-doubtful">?! ${count('doubtful')}</span> ·
      <span class="grade-bad">? ${count('bad')}</span> ·
      <span class="grade-very_bad">?? ${count('very_bad')}</span></dd></div>`);
  }
  return out.join('');
}

function reasonsHtml(ex) {
  if (!ex) return '';
  const items = ex.reasons.length
    ? ex.reasons.map((r) => `<li>${escapeHtml(r.text)} <span class="contrib">(${r.contribution >= 0 ? '+' : ''}${r.contribution.toFixed(3)})</span></li>`).join('')
    : '<li>특징값으로는 이 차이를 설명하지 못했어.</li>';
  return `<div class="approx"><span class="tag">설명(근사)</span>
    <ul>${items}</ul>
    <p class="hint">괄호는 대리 모델이 그 항목에 돌린 에퀴티 차이. 대리 모델 전체 예측 ${ex.approx_delta.toFixed(3)},
    검증 국면 설명력 R² ${ex.r2_heldout.toFixed(2)}.</p></div>`;
}

function detailHtml(e, r) {
  const who = sideName(e.side);
  const dice = `${e.dice[0]}-${e.dice[1]}`;
  if (!r) return `<div class="judge">${who} ${dice}: ${escapeHtml(e.notation)} — 분석 중…</div>`;
  if (r.error) return `<div class="judge">분석 실패: ${escapeHtml(r.error)}</div>`;
  if (r.count === 1) {
    return `<div class="judge"><span class="tag">엔진 판단</span>${who} ${dice}: ${escapeHtml(r.played.notation)} — 둘 수 있는 수가 이것뿐이었어.</div>`;
  }
  const p = r.played.probs;
  const b = r.best.probs;
  const top = r.top
    .map((m, i) => `<tr class="${m.notation === r.played.notation ? 'played' : ''}"><td>${i + 1}</td><td>${escapeHtml(m.notation)}</td><td>${m.equity >= 0 ? '+' : ''}${m.equity.toFixed(3)}</td><td>${i ? (m.equity - r.best.equity).toFixed(3) : ''}</td></tr>`)
    .join('');
  // 둔 수가 상위 5개 밖이면 따로 한 줄.
  const extra = r.rank > r.top.length
    ? `<tr class="played"><td>${r.rank}</td><td>${escapeHtml(r.played.notation)}</td><td>${r.played.equity >= 0 ? '+' : ''}${r.played.equity.toFixed(3)}</td><td>${(r.played.equity - r.best.equity).toFixed(3)}</td></tr>`
    : '';
  const verdict = r.grade === 'best'
    ? '최선 수야.'
    : `최선 수(<b>${escapeHtml(r.best.notation)}</b>)보다 에퀴티가 ${r.loss.toFixed(3)} 낮아.`;
  return `
    <div class="judge"><span class="tag">엔진 판단</span>
      ${who} ${dice}: <b>${escapeHtml(r.played.notation)}</b> ${verdict}
      등급 <b class="${gradeClass(r.grade)}">${r.label} ${r.mark}</b> · 후보 ${r.count}개 중 ${r.rank}위.
      <br>둔 수 뒤 ${who}의 승리 ${pct(p.win)}, 개먼승 ${pct(p.win_gammon)}, 개먼패 ${pct(p.lose_gammon)}${
        r.grade === 'best' ? '' : ` (최선 수면 ${pct(b.win)}, ${pct(b.win_gammon)}, ${pct(b.lose_gammon)})`}.</div>
    <table class="cands"><thead><tr><th>#</th><th>수</th><th>에퀴티</th><th>차이</th></tr></thead><tbody>${top}${extra}</tbody></table>
    ${reasonsHtml(r.explanation)}`;
}

function render() {
  for (const btn of els.show) btn.setAttribute('aria-pressed', String(btn.dataset.show === A.show));
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

  const rows = [];
  let row = null;
  entries.forEach((e, i) => {
    if (!row || e.side === HUMAN || row[AI]) {
      row = [null, null];
      rows.push(row);
    }
    row[e.side] = i;
  });
  const cell = (i) => {
    if (i === null) return '<span class="empty">—</span>';
    const e = entries[i];
    const r = A.results[i];
    const mark = r && r.mark ? ` <b class="mark ${gradeClass(r.grade)}">${r.mark}</b>` : '';
    return `<button type="button" class="mv-btn ${i === A.cur ? 'cur' : ''}" data-i="${i}"><span class="dice">${e.dice[0]}${e.dice[1]}</span>${escapeHtml(e.notation)}${mark}</button>`;
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
  els.detail.className = `coach ${r && (r.grade === 'bad' || r.grade === 'very_bad') ? 'warn' : ''}`;
  els.detail.innerHTML = detailHtml(e, r);

  const model = { view: A.view };
  if (A.show === 'before' || !r || r.error) {
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

els.select.addEventListener('change', () => openGame(els.select.value));
els.prev.addEventListener('click', () => go(A.cur - 1));
els.next.addEventListener('click', () => go(A.cur + 1));
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
    setStatus('저장된 기보가 없어. 대국을 두면 자동으로 저장돼(최근 20판).');
    render();
    return;
  }
  fillSelect();
  const want = new URLSearchParams(location.search).get('game');
  const id = want && A.games.some((g) => String(g.id) === want) ? want : A.games[0].id;
  els.select.value = String(id);
  await openGame(id);
})();
