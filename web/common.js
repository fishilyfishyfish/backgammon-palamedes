// 대국·분석 화면이 같이 쓰는 것: 엔진 RPC, 저장소, 판 배열 도우미, 기보 저장.

export const HUMAN = 0;
export const AI = 1;
export const BAR = 25;
// 엔진 선택(rank_moves_json 등의 engine 인자).
export const ENGINE_BASELINE = 0;
export const ENGINE_NN = 1;
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
// ai: 상대('nn' | 'baseline' | 'random'), coach: 친절 모드('on' | 'off').
export function loadView() {
  return { color: 'white', home: 'right', ai: 'nn', coach: 'on', ...storage.get('palamedes.view', {}) };
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
// 판마다 {id, date, ai, entries: [{side, dice, notation, before, after}], result}. 최근 GAME_LIMIT판만 남긴다.
const GAMES_KEY = 'palamedes.games';
export const GAME_LIMIT = 20;

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
// 파일 형식: {format: 'palamedes-games', version: 1, exported, games: [게임...]}. 게임 하나나 게임 배열만 있어도 받는다.
export const EXPORT_FORMAT = 'palamedes-games';

export function exportText(games) {
  return JSON.stringify({ format: EXPORT_FORMAT, version: 1, exported: new Date().toISOString(), games }, null, 1);
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

function cleanGame(g, n) {
  if (!g || typeof g !== 'object' || !Array.isArray(g.entries)) throw new Error(`${n}번째 판: entries가 없어`);
  const entries = g.entries.map((e, i) => {
    const where = `${n}번째 판 ${i + 1}번째 수`;
    if (!e || !isInt(e.side, 0, 1)) throw new Error(`${where}: side가 0이나 1이 아니야`);
    if (!Array.isArray(e.dice) || e.dice.length !== 2 || !e.dice.every((d) => isInt(d, 1, 6))) throw new Error(`${where}: 주사위가 이상해`);
    if (!validBoard(e.before) || !validBoard(e.after)) throw new Error(`${where}: 판 배열이 이상해`);
    return { side: e.side, dice: e.dice.slice(), notation: typeof e.notation === 'string' ? e.notation : '', before: e.before.slice(), after: e.after.slice() };
  });
  const r = g.result;
  const result = r && isInt(r.winner, 0, 1) && isInt(r.points, 1, 3) ? { winner: r.winner, points: r.points } : null;
  const date = typeof g.date === 'string' && !Number.isNaN(Date.parse(g.date)) ? g.date : new Date().toISOString();
  return { id: Number.isFinite(g.id) ? g.id : Date.parse(date), date, ai: typeof g.ai === 'string' ? g.ai : 'unknown', entries, result };
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
    if (data.version !== 1) throw new Error(`지원하지 않는 버전이야: ${data.version}`);
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

// 등급 → CSS 클래스.
export const gradeClass = (g) => `grade-${g}`;

export function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
}
