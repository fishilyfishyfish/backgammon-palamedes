// 대국·분석 화면이 같이 쓰는 것: 엔진 RPC, 저장소, 판 배열 도우미, 기보 저장.

export const HUMAN = 0;
export const AI = 1;
export const BAR = 25;
// 엔진 선택(rank_moves_json 등의 engine 인자). 0 = 기준선(손 평가)은 웹에서 쓰지 않는다.
export const ENGINE_NN = 1;
// 탐색 깊이(ply 인자). 힌트·경고·분석은 2수. AI 수는 난이도대로(ai_move_json).
export const PLY_NN = 2;
// 실수 등급 임계값(GNU 백개먼 기본값). 엔진(analysis.rs)과 같아야 한다.
export const BAD = 0.08;

export const idx = (side, p) => side * 26 + p;

// 엔진이 준 걸음(hit 포함)을 판에 적용. 연출용.
export function applyStep(board, side, s) {
  const b = board.slice();
  b[idx(side, s.from)] -= 1;
  b[idx(side, s.to)] += 1;
  if (s.hit) {
    b[idx(1 - side, 25 - s.to)] -= 1;
    b[idx(1 - side, BAR)] += 1;
  }
  return b;
}

export const storage = {
  get(k, d) { try { const v = localStorage.getItem(k); return v === null ? d : JSON.parse(v); } catch { return d; } },
  set(k, v) { try { localStorage.setItem(k, JSON.stringify(v)); return true; } catch { return false; } },
};

// 화면 설정. color: 사람 말 색('white'|'black'), home: 사람 홈 위치('right'|'left'),
// ai: 상대('nn' | 'baseline' | 'random'), coach: 친절 모드('on' | 'off'), match: 매치 길이('0' = 머니, '3' | '5' | '7').
// ai: 상대 봇 id(engine/weights/bots.json). 예전 값(상대 nn | baseline | random, 난이도 '1'..'5')은 비슷한 봇으로 읽는다.
// 없는 id면 app.js가 가장 센 봇으로 바꾼다.
export const DEFAULT_BOT = 'palamedes';
const OLD_AI = { nn: 'palamedes', baseline: 'minji', random: 'saessak', 5: 'palamedes', 4: 'jiu', 3: 'minji', 2: 'dalsu', 1: 'saessak' };
export function loadView() {
  const v = { color: 'white', home: 'right', ai: DEFAULT_BOT, coach: 'on', match: '0', ...storage.get('palamedes.view', {}) };
  if (OLD_AI[v.ai]) v.ai = OLD_AI[v.ai];
  return v;
}

// ---------- 레이팅(FIBS 방식) ----------
// 레이팅이 diff만큼 높은 쪽이 n점 매치(머니 게임 한 판은 n = 1)를 이길 확률. 엔진 bots::win_prob와 같은 식.
export const fibsWinProb = (diff, n) => 1 / (1 + 10 ** ((-diff * Math.sqrt(n)) / 2000));
export const RATING_START = 1500;
// 이긴 쪽은 4·√n·(1 − 이길 확률)만큼 오른다. 경험(지금까지 둔 점수 합) 400 전에는 최대 5배로 빨리 움직인다.
export function rateResult(me, oppRating, won, n) {
  const p = fibsWinProb(me.rating - oppRating, n);
  const boost = Math.max(1, 5 - me.exp / 100);
  const delta = 4 * Math.sqrt(n) * boost * (won ? 1 - p : -p);
  return { rating: me.rating + delta, exp: me.exp + n, games: (me.games || 0) + 1, delta };
}

// 봇 아바타: 이름 첫 글자를 레이팅에 따라 색이 바뀌는 원 안에(외부 그림 없음).
export function avatarSvg(bot, size = 44) {
  const hue = Math.round(200 - ((bot.rating - 900) / 900) * 200);
  return `<svg class="avatar" width="${size}" height="${size}" viewBox="0 0 44 44" aria-hidden="true">
    <circle cx="22" cy="22" r="21" fill="hsl(${hue} 55% 45%)" stroke="hsl(${hue} 60% 30%)" stroke-width="2"/>
    <text x="22" y="23" text-anchor="middle" dominant-baseline="middle" font-size="18" font-weight="700" fill="#fff">${escapeHtml(bot.name[0])}</text>
  </svg>`;
}

// 엔진 Worker와 {id, fn, args} RPC.
export function createEngine(onError) {
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
  worker.onerror = (e) => onError(e.message || '알 수 없는 오류');
  return (fn, ...args) => {
    const id = nextId++;
    return new Promise((resolve, reject) => {
      pending.set(id, { resolve, reject });
      worker.postMessage({ id, fn, args });
    });
  };
}

// ---------- 기보 저장 ----------
// 판마다 {id, date, ai, match, entries, result}. 최근 GAME_LIMIT판만 남긴다.
//   match: null(머니) | {length, score: [사람, AI](판 시작 때), post(크로포드 판이 지났는가)}
//   entries: 수 {side, dice, notation, before, after, cube} | 큐브 {type: 'cube', side, action: 'double'|'take'|'pass', board, cube}
//   cube: 그 시점(수를 두기 전·큐브 동작 전)의 {value, owner: null|0|1}
//   result: {winner, points, passed?}
const GAMES_KEY = 'palamedes.games';
export const GAME_LIMIT = 50;

export function loadGames() {
  const games = storage.get(GAMES_KEY, []);
  return Array.isArray(games) ? games : [];
}

export function saveGame(game) {
  const games = loadGames().filter((g) => g.id !== game.id);
  games.unshift(game);
  // 저장 공간이 모자라면 오래된 판부터 버린다.
  for (let n = Math.min(games.length, GAME_LIMIT); n > 0; n--) {
    if (storage.set(GAMES_KEY, games.slice(0, n))) return true;
  }
  return false;
}

// ---------- 내보내기 · 가져오기 ----------
// 파일 형식: {format: 'palamedes-games', version: 2, exported, games: [게임...]}. 게임 하나나 게임 배열만 있어도 받는다.
// version 1(큐브·매치 정보 없음)도 받는다. 없는 값은 큐브 1·가운데·머니로 본다.
export const EXPORT_FORMAT = 'palamedes-games';

export function exportText(games) {
  return JSON.stringify({ format: EXPORT_FORMAT, version: 2, exported: new Date().toISOString(), games }, null, 1);
}

const isInt = (v, lo, hi) => Number.isInteger(v) && v >= lo && v <= hi;

// 52칸 판: 각 칸 0..15, 진영마다 합 15. 같은 포인트에 두 진영이 같이 있으면 안 된다.
function validBoard(b) {
  if (!Array.isArray(b) || b.length !== 52 || !b.every((v) => isInt(v, 0, 15))) return false;
  for (const side of [0, 1]) {
    if (b.slice(side * 26, side * 26 + 26).reduce((x, y) => x + y, 0) !== 15) return false;
  }
  for (let p = 1; p <= 24; p++) if (b[p] > 0 && b[26 + 25 - p] > 0) return false;
  return true;
}

function cleanCube(c, where) {
  if (c === undefined || c === null) return { value: 1, owner: null };
  const okValue = isInt(c.value, 1, 64) && (c.value & (c.value - 1)) === 0;
  const okOwner = c.owner === null || c.owner === undefined || isInt(c.owner, 0, 1);
  if (!okValue || !okOwner) throw new Error(`${where}: 큐브 값이 이상해`);
  return { value: c.value, owner: c.owner ?? null };
}

function cleanMatch(m, where) {
  if (m === undefined || m === null) return null;
  const ok = isInt(m.length, 1, 25) && Array.isArray(m.score) && m.score.length === 2 && m.score.every((x) => isInt(x, 0, m.length - 1));
  if (!ok) throw new Error(`${where}: 매치 점수가 이상해`);
  return { length: m.length, score: m.score.slice(), post: !!m.post };
}

function cleanGame(g, n) {
  if (!g || typeof g !== 'object' || !Array.isArray(g.entries)) throw new Error(`${n}번째 판: entries가 없어`);
  const entries = g.entries.map((e, i) => {
    const where = `${n}번째 판 ${i + 1}번째 항목`;
    if (!e || !isInt(e.side, 0, 1)) throw new Error(`${where}: side가 0이나 1이 아니야`);
    const cube = cleanCube(e.cube, where);
    if (e.type === 'cube') {
      if (!['double', 'take', 'pass'].includes(e.action)) throw new Error(`${where}: 큐브 동작이 이상해`);
      if (!validBoard(e.board)) throw new Error(`${where}: 판 배열이 이상해`);
      return { type: 'cube', side: e.side, action: e.action, board: e.board.slice(), cube };
    }
    if (!Array.isArray(e.dice) || e.dice.length !== 2 || !e.dice.every((d) => isInt(d, 1, 6))) throw new Error(`${where}: 주사위가 이상해`);
    if (!validBoard(e.before) || !validBoard(e.after)) throw new Error(`${where}: 판 배열이 이상해`);
    return { side: e.side, dice: e.dice.slice(), notation: typeof e.notation === 'string' ? e.notation : '', before: e.before.slice(), after: e.after.slice(), cube };
  });
  const r = g.result;
  const result = r && isInt(r.winner, 0, 1) && isInt(r.points, 1, 192) ? { winner: r.winner, points: r.points, passed: !!r.passed } : null;
  const match = cleanMatch(g.match, `${n}번째 판`);
  const date = typeof g.date === 'string' && !Number.isNaN(Date.parse(g.date)) ? g.date : new Date().toISOString();
  const aiName = typeof g.aiName === 'string' ? g.aiName.slice(0, 40) : null;
  const aiRating = Number.isFinite(g.aiRating) ? g.aiRating : null;
  return { id: Number.isFinite(g.id) ? g.id : Date.parse(date), date, ai: typeof g.ai === 'string' ? g.ai : 'unknown', aiName, aiRating, match, entries, result };
}

// 파일 내용 → 게임 목록. 형식이 틀리면 이유를 담은 Error.
// 이미 있는 판과 id가 같고 내용도 같으면 건너뛰고, 내용이 다르면 새 id를 준다.
export function parseImport(text) {
  let data;
  try {
    data = JSON.parse(text);
  } catch {
    throw new Error('JSON 파일이 아니야');
  }
  let list;
  if (data && data.format === EXPORT_FORMAT) {
    if (data.version !== 1 && data.version !== 2) throw new Error(`지원하지 않는 버전이야: ${data.version}`);
    list = data.games;
  } else {
    list = Array.isArray(data) ? data : [data];
  }
  if (!Array.isArray(list) || !list.length) throw new Error('판이 하나도 없어');
  const games = list.map((g, i) => cleanGame(g, i + 1));
  const existing = new Map(loadGames().map((g) => [g.id, JSON.stringify(g.entries)]));
  const out = [];
  let skipped = 0;
  let bump = 0;
  for (const g of games) {
    const prev = existing.get(g.id);
    if (prev === JSON.stringify(g.entries)) {
      skipped++;
      continue;
    }
    if (prev !== undefined) g.id = Date.now() + ++bump;
    existing.set(g.id, JSON.stringify(g.entries));
    out.push(g);
  }
  return { games: out, skipped };
}

// 엔진 ctx_json: 큐브 상태와 매치 점수(점수 배열은 진영 순서 [사람(0), AI(1)]).
export function ctxJson(cube, match) {
  return JSON.stringify({
    cube: { value: cube ? cube.value : 1, owner: cube ? cube.owner : null },
    match: match && match.length ? { length: match.length, score: match.score, post: !!match.post } : null,
  });
}

// 손실 → 등급(엔진 analysis.rs와 같은 임계값). 큐브 판단처럼 엔진이 등급을 안 주는 경우에 쓴다.
export function gradeOf(loss, isBest) {
  if (isBest) return { grade: 'best', label: '최선', mark: '' };
  if (loss >= 0.16) return { grade: 'very_bad', label: '매우 나쁨', mark: '??' };
  if (loss >= 0.08) return { grade: 'bad', label: '나쁨', mark: '?' };
  if (loss >= 0.04) return { grade: 'doubtful', label: '의심스러움', mark: '?!' };
  return { grade: 'good', label: '괜찮음', mark: '' };
}

// 등급 → CSS 클래스.
export const gradeClass = (g) => `grade-${g}`;

export function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
}
