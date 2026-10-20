// SVG 판 그리기(viewBox 932 x 706). 대국·분석 화면 공용. 판 번호는 사람(진영 0) 기준.
//
// model: {
//   board: 52칸 배열, view: {color, home},
//   selectable?: Set(출발점), selected?: 출발점, dests?: [도착점],   // 사람 기준 포인트, 0 = 베어오프, 25 = 바
//   dice?: {side, list: [{d, used}]}, opening?: [사람 눈, AI 눈],
//   lastMove?: {side, steps}, hint?: {steps},   // steps는 그 진영 기준 좌표, hint는 사람 수
//   cube?: {value, owner: null | 0 | 1},           // 더블링 큐브(두 트레이 사이). 가운데면 64로 표시
//   focus?: 포인트                                  // 키보드로 고른 칸(사람 기준, 0 = 베어오프, 25 = 바)
// }

import { AI, BAR, HUMAN, idx } from './common.js';

export const VIEWBOX = '0 0 932 706';

const G = {
  PW: 62, D: 56, R: 27,
  leftX: 22, barX: 394, barW: 56, rightX: 450,
  top: 42, bottom: 666, mid: 354,
  trayX: 846, trayW: 76,
};
const PT_LEN = 5 * G.D;
const VB_W = 932;

const PIPS = {
  1: [[0, 0]], 2: [[-1, -1], [1, 1]], 3: [[-1, -1], [0, 0], [1, 1]],
  4: [[-1, -1], [1, -1], [-1, 1], [1, 1]], 5: [[-1, -1], [1, -1], [0, 0], [-1, 1], [1, 1]],
  6: [[-1, -1], [1, -1], [-1, 0], [1, 0], [-1, 1], [1, 1]],
};

export function renderBoard(svg, m) {
  const b = m.board;
  if (!b) return;
  const view = m.view;
  // 홈을 왼쪽에 두면 판 전체를 좌우로 뒤집는다. X: 중심 좌표, RX: 왼쪽 끝 좌표 + 폭.
  const X = (x) => (view.home === 'left' ? VB_W - x : x);
  const RX = (x, w) => (view.home === 'left' ? VB_W - x - w : x);
  // 진영의 화면 색: 'w' | 'b'.
  const tone = (side) => ((side === HUMAN) === (view.color === 'white') ? 'w' : 'b');
  const pointGeom = (p) => {
    if (p <= 6) return { x: X(G.rightX + (6 - p + 0.5) * G.PW), top: false };
    if (p <= 12) return { x: X(G.leftX + (12 - p + 0.5) * G.PW), top: false };
    if (p <= 18) return { x: X(G.leftX + (p - 13 + 0.5) * G.PW), top: true };
    return { x: X(G.rightX + (p - 19 + 0.5) * G.PW), top: true };
  };
  const slotY = (top, i) => (top ? G.top + G.R + 1 + i * G.D : G.bottom - G.R - 1 - i * G.D);
  // 사람 말은 바 아래쪽, AI 말은 위쪽에 쌓는다.
  const barSlotY = (side, i) => (side === HUMAN ? G.mid + 34 + i * G.D : G.mid - 34 - i * G.D);
  const countsAt = (p) => ({ w: b[idx(HUMAN, p)], b: b[idx(AI, 25 - p)] });
  const checker = (x, y, side, label) => {
    const t = tone(side);
    let s = `<circle class="ck-${t}" cx="${x}" cy="${y}" r="${G.R}"/>`;
    if (label) s += `<text class="ck-n-${t}" x="${x}" y="${y}">${label}</text>`;
    return s;
  };
  const dieSvg = (x, y, v, side, used) => {
    const size = 46;
    const h = size / 2;
    const t = tone(side);
    let s = `<g opacity="${used ? 0.35 : 1}">`;
    s += `<rect class="${t === 'w' ? 'die' : 'die-b'}" x="${x - h}" y="${y - h}" width="${size}" height="${size}" rx="8"/>`;
    for (const [dx, dy] of PIPS[v]) s += `<circle class="${t === 'w' ? 'pip' : 'pip-b'}" cx="${x + dx * 12}" cy="${y + dy * 12}" r="4.6"/>`;
    return s + '</g>';
  };
  // 진영 기준 포인트 → 사람 기준.
  const toHuman = (side, q) => (q === 0 || q === BAR ? q : side === HUMAN ? q : 25 - q);

  const parts = [];
  parts.push(`<rect class="frame" x="${RX(10, 824)}" y="30" width="824" height="648" rx="10"/>`);
  parts.push(`<rect class="felt" x="${RX(G.leftX, 6 * G.PW)}" y="${G.top}" width="${6 * G.PW}" height="${G.bottom - G.top}"/>`);
  parts.push(`<rect class="felt" x="${RX(G.rightX, 6 * G.PW)}" y="${G.top}" width="${6 * G.PW}" height="${G.bottom - G.top}"/>`);
  parts.push(`<rect class="barzone" x="${RX(G.barX, G.barW)}" y="${G.top}" width="${G.barW}" height="${G.bottom - G.top}"/>`);
  parts.push(`<rect class="frame" x="${RX(G.trayX - 6, G.trayW + 12)}" y="30" width="${G.trayW + 12}" height="648" rx="10"/>`);
  parts.push(`<rect class="tray" x="${RX(G.trayX, G.trayW)}" y="${G.top}" width="${G.trayW}" height="${PT_LEN}"/>`);
  parts.push(`<rect class="tray" x="${RX(G.trayX, G.trayW)}" y="${G.bottom - PT_LEN}" width="${G.trayW}" height="${PT_LEN}"/>`);

  for (let p = 1; p <= 24; p++) {
    const { x, top } = pointGeom(p);
    const half = G.PW / 2 - 1;
    const baseY = top ? G.top : G.bottom;
    const tipY = top ? G.top + PT_LEN - 10 : G.bottom - PT_LEN + 10;
    parts.push(`<polygon class="${p % 2 === 0 ? 'pt-a' : 'pt-b'}" points="${x - half},${baseY} ${x + half},${baseY} ${x},${tipY}"/>`);
    parts.push(`<text class="lbl" x="${x}" y="${top ? 22 : 696}">${p}</text>`);
  }

  // 직전 수 표시(출발·도착 칸).
  if (m.lastMove) {
    const marks = new Set();
    for (const s of m.lastMove.steps) {
      for (const q of [s.from, s.to]) {
        const p = toHuman(m.lastMove.side, q);
        if (p >= 1 && p <= 24) marks.add(p);
      }
    }
    for (const p of marks) {
      const { x, top } = pointGeom(p);
      parts.push(`<rect class="last" x="${x - G.PW / 2 + 3}" y="${top ? G.top : G.bottom - 8}" width="${G.PW - 6}" height="8" rx="3"/>`);
    }
  }

  const selectable = m.selectable || new Set();
  for (let p = 1; p <= 24; p++) {
    const { x, top } = pointGeom(p);
    const { w, b: bk } = countsAt(p);
    const n = w || bk;
    const side = w ? HUMAN : AI;
    for (let i = 0; i < Math.min(n, 5); i++) {
      parts.push(checker(x, slotY(top, i), side, i === 4 && n > 5 ? String(n) : ''));
    }
    if (n && side === HUMAN && selectable.has(p)) {
      parts.push(`<circle class="${m.selected === p ? 'sel-ring' : 'can-ring'}" cx="${x}" cy="${slotY(top, Math.min(n, 5) - 1)}" r="${G.R + 4}"/>`);
    }
  }

  const bx = X(G.barX + G.barW / 2);
  for (const side of [HUMAN, AI]) {
    const n = b[idx(side, BAR)];
    for (let i = 0; i < Math.min(n, 3); i++) parts.push(checker(bx, barSlotY(side, i), side, i === 2 && n > 3 ? String(n) : ''));
  }
  if (b[idx(HUMAN, BAR)] && selectable.has(BAR)) {
    parts.push(`<circle class="${m.selected === BAR ? 'sel-ring' : 'can-ring'}" cx="${bx}" cy="${barSlotY(HUMAN, 0)}" r="${G.R + 4}"/>`);
  }

  const tw = G.trayW - 12;
  const tx = RX(G.trayX + 6, tw);
  for (let i = 0; i < b[idx(HUMAN, 0)]; i++) parts.push(`<rect class="off-${tone(HUMAN)}" x="${tx}" y="${G.bottom - 4 - (i + 1) * 17}" width="${tw}" height="14" rx="4"/>`);
  for (let i = 0; i < b[idx(AI, 0)]; i++) parts.push(`<rect class="off-${tone(AI)}" x="${tx}" y="${G.top + 4 + i * 17}" width="${tw}" height="14" rx="4"/>`);

  // 힌트: 추천 수의 출발·도착 칸에 번호 표시.
  if (m.hint) {
    m.hint.steps.forEach((s, i) => {
      for (const [q, kind] of [[s.from, 'from'], [s.to, 'to']]) {
        let x, y;
        if (q === 0) {
          x = X(G.trayX + G.trayW / 2);
          y = G.bottom - PT_LEN / 2;
        } else if (q === BAR) {
          x = bx;
          y = barSlotY(HUMAN, 0);
        } else {
          const g = pointGeom(q);
          const { w } = countsAt(q);
          x = g.x;
          y = slotY(g.top, kind === 'from' ? Math.max(0, Math.min(w, 5) - 1) : Math.min(w, 4));
        }
        parts.push(`<circle class="hint-${kind}" cx="${x}" cy="${y}" r="${G.R - 3}"/>`);
        if (kind === 'to') parts.push(`<text class="hint-n" x="${x}" y="${y}">${i + 1}</text>`);
      }
    });
  }

  for (const to of m.dests || []) {
    if (to === 0) {
      parts.push(`<rect class="dest-tray" x="${RX(G.trayX + 2, G.trayW - 4)}" y="${G.bottom - PT_LEN + 2}" width="${G.trayW - 4}" height="${PT_LEN - 4}" rx="6"/>`);
      continue;
    }
    const { x, top } = pointGeom(to);
    const { w, b: bk } = countsAt(to);
    parts.push(`<circle class="dest" cx="${x}" cy="${slotY(top, bk ? 0 : Math.min(w, 4))}" r="${G.R}"/>`);
  }

  // 키보드 포커스: 칸 전체(터치 영역과 같은 크기)에 테두리.
  if (m.focus != null) {
    let r;
    if (m.focus === 0) r = [RX(G.trayX - 2, G.trayW + 4), G.mid, G.trayW + 4, G.bottom - G.mid + 8];
    else if (m.focus === BAR) r = [RX(G.barX + 2, G.barW - 4), G.top, G.barW - 4, G.bottom - G.top];
    else {
      const { x, top } = pointGeom(m.focus);
      r = [x - G.PW / 2 + 2, top ? G.top : G.mid, G.PW - 4, G.mid - G.top];
    }
    parts.push(`<rect class="kbd-focus" x="${r[0]}" y="${r[1]}" width="${r[2]}" height="${r[3]}" rx="8"/>`);
  }

  // 주사위: 사람은 오른쪽(홈 쪽), AI는 반대쪽 판 가운데.
  if (m.opening) {
    parts.push(dieSvg(X(G.rightX + 3 * G.PW), G.mid, m.opening[0], HUMAN, false));
    parts.push(dieSvg(X(G.leftX + 3 * G.PW), G.mid, m.opening[1], AI, false));
  } else if (m.dice) {
    const { side, list } = m.dice;
    const cx = X(side === HUMAN ? G.rightX + 3 * G.PW : G.leftX + 3 * G.PW);
    const gap = 54;
    const x0 = cx - ((list.length - 1) * gap) / 2;
    list.forEach((f, i) => parts.push(dieSvg(x0 + i * gap, G.mid, f.d, side, f.used)));
  }

  // 더블링 큐브: 두 트레이 사이. 소유자 쪽(사람 아래, AI 위)으로 조금 치우치고 그쪽 색으로 테두리.
  if (m.cube) {
    const size = 44;
    const cx = X(G.trayX + G.trayW / 2);
    const shift = m.cube.owner === null || m.cube.owner === undefined ? 0 : m.cube.owner === HUMAN ? 8 : -8;
    const cy = G.mid + shift;
    const label = m.cube.owner === null || m.cube.owner === undefined ? 64 : m.cube.value;
    const ownerCls = m.cube.owner === null || m.cube.owner === undefined ? '' : ` owner-${tone(m.cube.owner)}`;
    parts.push(`<g class="cube${ownerCls}"><rect x="${cx - size / 2}" y="${cy - size / 2}" width="${size}" height="${size}" rx="7"/>`);
    parts.push(`<text x="${cx}" y="${cy}">${label}</text></g>`);
  }

  // 터치 영역(맨 위).
  for (let p = 1; p <= 24; p++) {
    const { x, top } = pointGeom(p);
    parts.push(`<rect class="hit" data-point="${p}" x="${x - G.PW / 2}" y="${top ? G.top : G.mid}" width="${G.PW}" height="${G.mid - G.top}"/>`);
  }
  parts.push(`<rect class="hit" data-point="${BAR}" x="${RX(G.barX, G.barW)}" y="${G.top}" width="${G.barW}" height="${G.bottom - G.top}"/>`);
  parts.push(`<rect class="hit" data-point="0" x="${RX(G.trayX - 6, G.trayW + 12)}" y="${G.mid}" width="${G.trayW + 12}" height="${G.bottom - G.mid + 12}"/>`);

  svg.setAttribute('viewBox', VIEWBOX);
  svg.innerHTML = parts.join('');
}

// 주사위 목록. used: 이번 턴에 이미 쓴 걸음들(die 필드).
export function diceList(dice, usedSteps = []) {
  const [a, c] = dice;
  const list = a === c ? [a, a, a, a] : [a, c];
  const used = list.map(() => false);
  for (const st of usedSteps) {
    const i = list.findIndex((d, k) => d === st.die && !used[k]);
    if (i >= 0) used[i] = true;
  }
  return list.map((d, i) => ({ d, used: used[i] }));
}
