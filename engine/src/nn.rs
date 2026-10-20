//! 신경망 평가기(4단계). 은닉층 하나, 시그모이드. 출력 5개 = 굴릴 쪽 기준
//! [승리, 개먼승(백개먼 포함), 백개먼승, 개먼패, 백개먼패]. 각각 무조건부 확률.
//!
//! 입력(196개, Tesauro 방식): 굴릴 쪽, 상대 순서로 각 진영 자기 기준 좌표에서
//! 포인트 1..=24마다 4칸 [n≥1, n≥2, n≥3, (n−3)/2], 바 n/2, 베어오프 n/15.

// 행렬 인덱스를 직접 쓰는 편이 순전파·역전파 식과 대응이 잘 보인다.
#![allow(clippy::needless_range_loop)]

use std::io::{self, Read, Write};

use crate::board::{BAR, Board, Side};
use crate::eval::{Evaluator, OutcomeProbs};

pub const INPUTS: usize = 196;
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

#[inline]
fn sigmoid(z: f32) -> f32 {
    1.0 / (1.0 + (-z).exp())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Net {
    pub hidden: usize,
    /// [입력][은닉]
    pub w1: Vec<f32>,
    pub b1: Vec<f32>,
    /// [은닉][출력]
    pub w2: Vec<f32>,
    pub b2: [f32; OUTPUTS],
}

impl Net {
    /// 작은 균등 난수로 초기화.
    pub fn new(hidden: usize, seed: u64) -> Net {
        let mut rng = crate::rng::Rng::new(seed);
        let mut u =
            |scale: f32| ((rng.next_u64() >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0) * scale;
        let s1 = 1.0 / (INPUTS as f32).sqrt();
        let s2 = 1.0 / (hidden as f32).sqrt();
        Net {
            hidden,
            w1: (0..INPUTS * hidden).map(|_| u(s1)).collect(),
            b1: vec![0.0; hidden],
            w2: (0..hidden * OUTPUTS).map(|_| u(s2)).collect(),
            b2: [0.0; OUTPUTS],
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
        for n in [INPUTS, self.hidden, OUTPUTS] {
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
        if dims[0] != INPUTS || dims[2] != OUTPUTS || dims[1] == 0 || dims[1] > 4096 {
            return Err(bad("dimension mismatch"));
        }
        let hidden = dims[1];
        let mut read = |n: usize| -> io::Result<Vec<f32>> {
            let mut buf = vec![0u8; n * 4];
            r.read_exact(&mut buf)?;
            Ok(buf
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect())
        };
        let w1 = read(INPUTS * hidden)?;
        let b1 = read(hidden)?;
        let w2 = read(hidden * OUTPUTS)?;
        let b2v = read(OUTPUTS)?;
        let mut b2 = [0.0; OUTPUTS];
        b2.copy_from_slice(&b2v);
        Ok(Net {
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
        to_probs(&self.net.predict(&encode(b, on_roll)))
    }
}
