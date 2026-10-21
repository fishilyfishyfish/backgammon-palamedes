//! 신경망 평가기(4단계). 은닉층 하나, 시그모이드. 출력 5개 = 굴릴 쪽 기준
//! [승리, 개먼승(백개먼 포함), 백개먼승, 개먼패, 백개먼패]. 각각 무조건부 확률.
//!
//! 입력(196개, Tesauro 방식): 굴릴 쪽, 상대 순서로 각 진영 자기 기준 좌표에서
//! 포인트 1..=24마다 4칸 [n≥1, n≥2, n≥3, (n−3)/2], 바 n/2, 베어오프 n/15.
//!
//! 확장 입력(209개 = 196 + 13): 위에 손 특징을 더한다. 진영마다(굴릴 쪽, 상대) 6개
//! [핍/100, 가장 뒤 말의 탈출 굴림/36, 블롯이 맞을 수 있는 굴림/36(막힌 칸 무시), 프라임 뒤에 갇힌 말/5,
//! 가장 긴 연속 포인트/6, 홈보드 포인트/6], 그리고 레이스면 1. 수 생성 없이 계산한다(학습 속도 때문).
//!
//! 확장 입력 2(221개 = 196 + 25, `encode_ext2`): 진영마다 12개
//! [핍/100, 가장 뒤 말의 탈출 굴림/36, 막힌 칸을 반영한 "블롯이 하나라도 맞는" 굴림/36, "블롯 둘 이상이 맞을 수 있는" 굴림/36,
//! 맞을 때 잃는 핍 기댓값/10, 프라임 뒤에 갇힌 말/5, 가장 긴 연속 포인트/6, 홈보드 포인트/6,
//! 갇힘 정도(내 15..=24 포인트에서 앞 12칸의 상대 포인트를 지나는 굴림의 최솟값/36), 가장 뒤 앵커((p − 18)/6),
//! 접촉이 끝날 때까지 필요한 핍/100, 바에 말이 있으면 한 말이 못 들어올 확률((상대 홈 포인트/6)²)] + 레이스면 1.
//! gnubg 입력 설명(공개 문서)의 개념을 참고해 직접 구현했다(코드는 쓰지 않음).
//! 가중치 파일의 입력 수로 어느 인코딩인지 정한다(`Net::encode`).

// 행렬 인덱스를 직접 쓰는 편이 순전파·역전파 식과 대응이 잘 보인다.
#![allow(clippy::needless_range_loop)]

use std::io::{self, Read, Write};

use crate::board::{BAR, Board, Side};
use crate::eval::{Evaluator, OutcomeProbs};

pub const INPUTS: usize = 196;
/// 손 특징을 더한 입력 수.
pub const INPUTS_EXT: usize = INPUTS + 2 * EXT_PER_SIDE + 1;
const EXT_PER_SIDE: usize = 6;
/// 손 특징 2를 더한 입력 수.
pub const INPUTS_EXT2: usize = INPUTS + 2 * EXT2_PER_SIDE + 1;
const EXT2_PER_SIDE: usize = 12;
pub const OUTPUTS: usize = 5;
const PER_SIDE: usize = 98;
const MAGIC: &[u8; 4] = b"PNN1";

/// 0이 아닌 입력만 (인덱스, 값). 대부분의 입력이 0이라 순전파·역전파를 희소하게 한다.
pub type Sparse = Vec<(u16, f32)>;

pub fn encode(b: &Board, on_roll: Side) -> Sparse {
    let mut x = Vec::with_capacity(64);
    for (k, s) in [on_roll, on_roll.opp()].into_iter().enumerate() {
        let me = &b.pos[s.idx()];
        let base = k * PER_SIDE;
        for p in 1..=24 {
            let n = me[p];
            if n == 0 {
                continue;
            }
            let i = base + (p - 1) * 4;
            x.push((i as u16, 1.0));
            if n >= 2 {
                x.push((i as u16 + 1, 1.0));
            }
            if n >= 3 {
                x.push((i as u16 + 2, 1.0));
            }
            if n >= 4 {
                x.push((i as u16 + 3, (n - 3) as f32 / 2.0));
            }
        }
        if me[BAR] > 0 {
            x.push(((base + 96) as u16, me[BAR] as f32 / 2.0));
        }
        if me[0] > 0 {
            x.push(((base + 97) as u16, me[0] as f32 / 15.0));
        }
    }
    x
}

/// 확장 입력: `encode` + 손 특징.
pub fn encode_ext(b: &Board, on_roll: Side) -> Sparse {
    let mut x = encode(b, on_roll);
    let sides = [on_roll, on_roll.opp()];
    for (k, s) in sides.into_iter().enumerate() {
        let me = &b.pos[s.idx()];
        let op = &b.pos[s.opp().idx()];
        let (prime, _) = longest_prime(me);
        let (op_prime, op_lo) = longest_prime(op);
        let trapped = if op_prime >= 4 {
            // 상대 포인트 lo..는 내 기준 25 − lo 이하. 그보다 위(와 바)에 있는 내 말.
            me[(25 - op_lo as usize + 1)..=BAR]
                .iter()
                .map(|&n| n as u32)
                .sum::<u32>()
        } else {
            0
        };
        let home = (1..=6).filter(|&p| me[p] >= 2).count();
        let vals = [
            b.pips(s) as f32 / 100.0,
            escapes(me, op) as f32 / 36.0,
            exposure(me, op) as f32 / 36.0,
            trapped as f32 / 5.0,
            prime as f32 / 6.0,
            home as f32 / 6.0,
        ];
        let base = INPUTS + k * EXT_PER_SIDE;
        for (j, v) in vals.into_iter().enumerate() {
            if v != 0.0 {
                x.push(((base + j) as u16, v));
            }
        }
    }
    if crate::eval::is_race(b) {
        x.push(((INPUTS_EXT - 1) as u16, 1.0));
    }
    x
}

/// 가장 긴 연속 포인트(2개 이상)의 길이와 그 낮은 쪽 끝(자기 기준).
fn longest_prime(me: &[u8; 26]) -> (u8, u8) {
    let (mut best, mut lo, mut run) = (0u8, 0u8, 0u8);
    for p in 1..=24 {
        if me[p] >= 2 {
            run += 1;
            if run > best {
                best = run;
                lo = (p as u8) + 1 - run;
            }
        } else {
            run = 0;
        }
    }
    (best, lo)
}

/// 21가지 굴림(36 가중)마다 거리 집합: 한 말이 한 번에 갈 수 있는 거리들(중간 칸 막힘 무시).
fn roll_distances(d1: u8, d2: u8) -> ([u8; 4], usize) {
    if d1 == d2 {
        ([d1, 2 * d1, 3 * d1, 4 * d1], 4)
    } else {
        ([d1, d2, d1 + d2, 0], 3)
    }
}

/// 가장 뒤(바 포함) 내 말이 앞쪽 12칸 안의 상대 포인트를 모두 지나갈 수 있는 굴림 수(36 중).
/// 한 말만 움직이고 중간·도착 칸이 상대 포인트면 안 된다. 앞에 막는 포인트가 없으면 36.
fn escapes(me: &[u8; 26], op: &[u8; 26]) -> u32 {
    match (1..=BAR).rev().find(|&p| me[p] > 0) {
        Some(back) => escapes_from(back, op),
        None => 36,
    }
}

/// 내 포인트 `back`(바 25 포함)의 말 하나가 앞 12칸 안의 상대 포인트를 모두 지나갈 수 있는 굴림 수(36 중).
fn escapes_from(back: usize, op: &[u8; 26]) -> u32 {
    // 내 포인트 q에 상대 포인트가 있으면 막힘. 상대 기준 25 − q.
    let blocked = |q: usize| (1..=24).contains(&q) && op[25 - q] >= 2;
    let lowest = (back.saturating_sub(12).max(1)..back).find(|&q| blocked(q));
    let Some(lowest) = lowest else {
        return 36;
    };
    // 한 말이 순서대로 d, e(더블이면 d 네 번)를 써서 lowest보다 아래로 갈 수 있는가.
    let run = |steps: &[u8]| {
        let mut at = back as i32;
        for &d in steps {
            at -= d as i32;
            if at < lowest as i32 {
                return true;
            }
            if blocked(at as usize) {
                return false;
            }
        }
        false
    };
    let mut n = 0;
    for d1 in 1..=6u8 {
        if run(&[d1; 4]) {
            n += 1;
        }
        for d2 in d1 + 1..=6u8 {
            if run(&[d1, d2]) || run(&[d2, d1]) {
                n += 2;
            }
        }
    }
    n
}

/// 내 블롯 중 하나라도 상대가 맞힐 수 있는 굴림 수(36 중). 상대 말에서 블롯까지 거리만 보고 막힌 칸은 무시.
fn exposure(me: &[u8; 26], op: &[u8; 26]) -> u32 {
    // 상대 말은 내 기준으로 낮은 번호에서 높은 번호로 온다. 상대 바는 내 기준 0.
    let mut dist = [false; 25];
    let mut any = false;
    for p in 1..=24 {
        if me[p] != 1 {
            continue;
        }
        for q in 0..p {
            let has = if q == 0 { op[BAR] > 0 } else { op[25 - q] > 0 };
            if has && p - q <= 24 {
                dist[p - q] = true;
                any = true;
            }
        }
    }
    if !any {
        return 0;
    }
    let mut n = 0;
    for d1 in 1..=6u8 {
        for d2 in d1..=6u8 {
            let (ds, len) = roll_distances(d1, d2);
            if ds[..len]
                .iter()
                .any(|&d| (d as usize) < 25 && dist[d as usize])
            {
                n += if d1 == d2 { 1 } else { 2 };
            }
        }
    }
    n
}

/// 막힌 칸(내 포인트, 말 2개 이상)을 반영한 상대의 히트 가능성: (블롯이 하나라도 맞는 굴림, 서로 다른 블롯
/// 둘 이상을 맞힐 수 있는 굴림, 맞을 때 잃는 핍(25 − p)의 굴림별 최댓값 합). 모두 36 기준.
/// 상대가 바에 말이 있으면 바에서 들어오며 맞히는 경우만 센다(들어온 뒤 다른 말로 맞히는 경우는 무시, 어림).
fn hits(me: &[u8; 26], op: &[u8; 26]) -> (u32, u32, u32) {
    let blots: Vec<usize> = (1..=24).filter(|&p| me[p] == 1).collect();
    if blots.is_empty() {
        return (0, 0, 0);
    }
    let blocked = |x: usize| (1..=24).contains(&x) && me[x] >= 2;
    // 공격하는 상대 말의 내 기준 위치(바 = 0).
    let attackers: Vec<usize> = if op[BAR] > 0 {
        vec![0]
    } else {
        (1..=24).filter(|&q| op[25 - q] > 0).collect()
    };
    // q에서 d1, d2(더블이면 d1 최대 4번)로 정확히 p에 닿을 수 있는가(중간 착지 칸이 막히면 안 됨).
    let reach = |q: usize, p: usize, d1: usize, d2: usize| -> bool {
        let d = p - q;
        if d1 == d2 {
            (1..=4).any(|k| k * d1 == d && (1..k).all(|j| !blocked(q + j * d1)))
        } else {
            d == d1 || d == d2 || (d == d1 + d2 && (!blocked(q + d1) || !blocked(q + d2)))
        }
    };
    let (mut one, mut two, mut loss) = (0, 0, 0);
    for d1 in 1..=6usize {
        for d2 in d1..=6usize {
            let w = if d1 == d2 { 1 } else { 2 };
            let hit: Vec<usize> = blots
                .iter()
                .copied()
                .filter(|&p| attackers.iter().any(|&q| q < p && reach(q, p, d1, d2)))
                .collect();
            if !hit.is_empty() {
                one += w;
                loss += w * hit.iter().map(|&p| 25 - p as u32).max().unwrap();
            }
            if hit.len() >= 2 {
                two += w;
            }
        }
    }
    (one, two, loss)
}

/// 확장 입력 2: `encode` + 손 특징 2(모듈 설명).
pub fn encode_ext2(b: &Board, on_roll: Side) -> Sparse {
    let mut x = encode(b, on_roll);
    for (k, s) in [on_roll, on_roll.opp()].into_iter().enumerate() {
        let me = &b.pos[s.idx()];
        let op = &b.pos[s.opp().idx()];
        let (prime, _) = longest_prime(me);
        let (op_prime, op_lo) = longest_prime(op);
        let trapped = if op_prime >= 4 {
            me[(25 - op_lo as usize + 1)..=BAR]
                .iter()
                .map(|&n| n as u32)
                .sum::<u32>()
        } else {
            0
        };
        let home = (1..=6).filter(|&p| me[p] >= 2).count();
        let (one, two, loss) = hits(me, op);
        let contain = (15..=24).map(|p| escapes_from(p, op)).min().unwrap();
        let back_anchor = (19..=24).rev().find(|&p| me[p] >= 2).map_or(0, |p| p - 18);
        // 상대의 가장 뒤 말(내 기준 가장 낮은 번호, 상대 바 = 0)보다 위에 있는 내 말이 그 아래로 내려가는 데 필요한 핍.
        let op_back = if op[BAR] > 0 {
            0
        } else {
            (1..=24).find(|&q| op[25 - q] > 0).unwrap_or(25)
        };
        let to_break: usize = (op_back + 1..=BAR)
            .map(|p| me[p] as usize * (p - op_back))
            .sum();
        let op_home = (1..=6).filter(|&p| op[p] >= 2).count() as f32;
        let enter = if me[BAR] > 0 {
            (op_home / 6.0).powi(2)
        } else {
            0.0
        };
        let vals = [
            b.pips(s) as f32 / 100.0,
            escapes(me, op) as f32 / 36.0,
            one as f32 / 36.0,
            two as f32 / 36.0,
            loss as f32 / 360.0,
            trapped as f32 / 5.0,
            prime as f32 / 6.0,
            home as f32 / 6.0,
            contain as f32 / 36.0,
            back_anchor as f32 / 6.0,
            to_break as f32 / 100.0,
            enter,
        ];
        let base = INPUTS + k * EXT2_PER_SIDE;
        for (j, v) in vals.into_iter().enumerate() {
            if v != 0.0 {
                x.push(((base + j) as u16, v));
            }
        }
    }
    if crate::eval::is_race(b) {
        x.push(((INPUTS_EXT2 - 1) as u16, 1.0));
    }
    x
}

#[inline]
fn sigmoid(z: f32) -> f32 {
    1.0 / (1.0 + (-z).exp())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Net {
    /// 입력 수: `INPUTS`(기본) 또는 `INPUTS_EXT`(손 특징 포함).
    pub inputs: usize,
    pub hidden: usize,
    /// [입력][은닉]
    pub w1: Vec<f32>,
    pub b1: Vec<f32>,
    /// [은닉][출력]
    pub w2: Vec<f32>,
    pub b2: [f32; OUTPUTS],
}

impl Net {
    /// 작은 균등 난수로 초기화(기본 입력).
    pub fn new(hidden: usize, seed: u64) -> Net {
        Net::with_inputs(INPUTS, hidden, seed)
    }

    pub fn with_inputs(inputs: usize, hidden: usize, seed: u64) -> Net {
        assert!(
            [INPUTS, INPUTS_EXT, INPUTS_EXT2].contains(&inputs),
            "inputs"
        );
        let mut rng = crate::rng::Rng::new(seed);
        let mut u =
            |scale: f32| ((rng.next_u64() >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0) * scale;
        let s1 = 1.0 / (inputs as f32).sqrt();
        let s2 = 1.0 / (hidden as f32).sqrt();
        Net {
            inputs,
            hidden,
            w1: (0..inputs * hidden).map(|_| u(s1)).collect(),
            b1: vec![0.0; hidden],
            w2: (0..hidden * OUTPUTS).map(|_| u(s2)).collect(),
            b2: [0.0; OUTPUTS],
        }
    }

    /// 이 망의 입력 형식으로 인코딩.
    pub fn encode(&self, b: &Board, on_roll: Side) -> Sparse {
        match self.inputs {
            INPUTS_EXT => encode_ext(b, on_roll),
            INPUTS_EXT2 => encode_ext2(b, on_roll),
            _ => encode(b, on_roll),
        }
    }

    /// 은닉 활성값과 출력.
    pub fn forward(&self, x: &[(u16, f32)], h: &mut Vec<f32>) -> [f32; OUTPUTS] {
        let nh = self.hidden;
        h.clear();
        h.extend_from_slice(&self.b1);
        for &(i, v) in x {
            let row = &self.w1[i as usize * nh..(i as usize + 1) * nh];
            if v == 1.0 {
                for (a, w) in h.iter_mut().zip(row) {
                    *a += w;
                }
            } else {
                for (a, w) in h.iter_mut().zip(row) {
                    *a += v * w;
                }
            }
        }
        let mut z = self.b2;
        for (j, a) in h.iter_mut().enumerate() {
            *a = sigmoid(*a);
            let row = &self.w2[j * OUTPUTS..(j + 1) * OUTPUTS];
            for o in 0..OUTPUTS {
                z[o] += *a * row[o];
            }
        }
        z.map(sigmoid)
    }

    pub fn predict(&self, x: &[(u16, f32)]) -> [f32; OUTPUTS] {
        let mut h = Vec::with_capacity(self.hidden);
        self.forward(x, &mut h)
    }

    /// 교차 엔트로피 손실로 SGD 한 번. 손실값(출력 평균)을 돌려준다.
    pub fn train_step(
        &mut self,
        x: &[(u16, f32)],
        target: &[f32; OUTPUTS],
        lr: f32,
        h: &mut Vec<f32>,
    ) -> f32 {
        let y = self.forward(x, h);
        let nh = self.hidden;
        let mut d_out = [0.0f32; OUTPUTS];
        let mut loss = 0.0;
        for o in 0..OUTPUTS {
            d_out[o] = y[o] - target[o];
            let yc = y[o].clamp(1e-6, 1.0 - 1e-6);
            loss -= target[o] * yc.ln() + (1.0 - target[o]) * (1.0 - yc).ln();
        }
        // 은닉 델타는 w2를 바꾸기 전에 계산.
        let mut d_hid = vec![0.0f32; nh];
        for j in 0..nh {
            let row = &self.w2[j * OUTPUTS..(j + 1) * OUTPUTS];
            let s: f32 = (0..OUTPUTS).map(|o| d_out[o] * row[o]).sum();
            d_hid[j] = s * h[j] * (1.0 - h[j]);
        }
        for j in 0..nh {
            let row = &mut self.w2[j * OUTPUTS..(j + 1) * OUTPUTS];
            for o in 0..OUTPUTS {
                row[o] -= lr * d_out[o] * h[j];
            }
        }
        for o in 0..OUTPUTS {
            self.b2[o] -= lr * d_out[o];
        }
        for &(i, v) in x {
            let row = &mut self.w1[i as usize * nh..(i as usize + 1) * nh];
            for (w, d) in row.iter_mut().zip(&d_hid) {
                *w -= lr * d * v;
            }
        }
        for (b, d) in self.b1.iter_mut().zip(&d_hid) {
            *b -= lr * d;
        }
        loss / OUTPUTS as f32
    }

    pub fn write_to(&self, w: &mut impl Write) -> io::Result<()> {
        w.write_all(MAGIC)?;
        for n in [self.inputs, self.hidden, OUTPUTS] {
            w.write_all(&(n as u32).to_le_bytes())?;
        }
        for v in self
            .w1
            .iter()
            .chain(&self.b1)
            .chain(&self.w2)
            .chain(&self.b2)
        {
            w.write_all(&v.to_le_bytes())?;
        }
        Ok(())
    }

    pub fn read_from(r: &mut impl Read) -> io::Result<Net> {
        let bad = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_string());
        let mut magic = [0u8; 4];
        r.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(bad("not a PNN1 file"));
        }
        let mut dims = [0usize; 3];
        for d in &mut dims {
            let mut b = [0u8; 4];
            r.read_exact(&mut b)?;
            *d = u32::from_le_bytes(b) as usize;
        }
        let inputs_ok = [INPUTS, INPUTS_EXT, INPUTS_EXT2].contains(&dims[0]);
        if !inputs_ok || dims[2] != OUTPUTS || dims[1] == 0 || dims[1] > 4096 {
            return Err(bad("dimension mismatch"));
        }
        let (inputs, hidden) = (dims[0], dims[1]);
        let mut read = |n: usize| -> io::Result<Vec<f32>> {
            let mut buf = vec![0u8; n * 4];
            r.read_exact(&mut buf)?;
            Ok(buf
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| f32::from_le_bytes(*c))
                .collect())
        };
        let w1 = read(inputs * hidden)?;
        let b1 = read(hidden)?;
        let w2 = read(hidden * OUTPUTS)?;
        let b2v = read(OUTPUTS)?;
        let mut b2 = [0.0; OUTPUTS];
        b2.copy_from_slice(&b2v);
        Ok(Net {
            inputs,
            hidden,
            w1,
            b1,
            w2,
            b2,
        })
    }

    pub fn from_bytes(bytes: &[u8]) -> io::Result<Net> {
        Net::read_from(&mut &bytes[..])
    }
}

pub fn to_probs(y: &[f32; OUTPUTS]) -> OutcomeProbs {
    OutcomeProbs {
        win: y[0],
        win_gammon: y[1],
        win_backgammon: y[2],
        lose_gammon: y[3],
        lose_backgammon: y[4],
    }
    .clamped()
}

pub fn from_probs(p: &OutcomeProbs) -> [f32; OUTPUTS] {
    [
        p.win,
        p.win_gammon,
        p.win_backgammon,
        p.lose_gammon,
        p.lose_backgammon,
    ]
}

/// 저장소에 포함된 학습된 가중치(`engine/weights/td.bin`).
pub static EMBEDDED_WEIGHTS: &[u8] = include_bytes!("../weights/td.bin");

#[derive(Clone, Debug)]
pub struct NnEval {
    pub net: std::sync::Arc<Net>,
}

impl NnEval {
    pub fn new(net: Net) -> NnEval {
        NnEval {
            net: std::sync::Arc::new(net),
        }
    }

    pub fn embedded() -> NnEval {
        NnEval::new(Net::from_bytes(EMBEDDED_WEIGHTS).expect("embedded weights"))
    }
}

impl Evaluator for NnEval {
    fn evaluate(&self, b: &Board, on_roll: Side) -> OutcomeProbs {
        if let Some((w, pts)) = b.result() {
            return OutcomeProbs::terminal(w, pts, on_roll);
        }
        to_probs(&self.net.predict(&self.net.encode(b, on_roll)))
    }
}
