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
const GAME_LIMIT = 20;

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

// 등급 → CSS 클래스.
export const gradeClass = (g) => `grade-${g}`;

export function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
}
