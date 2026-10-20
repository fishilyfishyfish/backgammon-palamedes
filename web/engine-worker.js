// 엔진(WASM)을 돌리는 Web Worker. 메인 스레드와는 {id, fn, args} → {id, ok, result|error}로 주고받는다.
import init, * as engine from './pkg/palamedes.js';

const ready = init();

// JSON을 돌려주는 함수는 여기서 파싱해서 넘긴다.
const JSON_FNS = new Set(['legal_moves_json', 'turn_state_json', 'features_json', 'result_json', 'rank_moves_json']);

self.onmessage = async (e) => {
  const { id, fn, args } = e.data;
  try {
    await ready;
    if (typeof engine[fn] !== 'function') throw new Error(`unknown engine function: ${fn}`);
    const conv = args.map((a) => (Array.isArray(a) ? Uint8Array.from(a) : a));
    let result = engine[fn](...conv);
    if (JSON_FNS.has(fn)) result = JSON.parse(result);
    else if (ArrayBuffer.isView(result)) result = Array.from(result);
    self.postMessage({ id, ok: true, result });
  } catch (err) {
    self.postMessage({ id, ok: false, error: String(err && err.message ? err.message : err) });
  }
};
