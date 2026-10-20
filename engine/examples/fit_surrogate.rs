//! 설명용 선형 대리 모델을 맞춘다: 같은 국면·주사위의 후보 수들 사이에서
//! 특징값 차이(대리 입력) → 신경망 에퀴티 차이. 결과는 `weights/surrogate.json`.
//! `cargo run --release --example fit_surrogate -- [판 수] [출력 경로]`
//!
//! 국면은 신경망 자기대국에서 모은다. 각 차례마다 에퀴티 상위 10개 후보를 쓰고, 후보 평균을 빼서
//! (국면 고정 효과 제거) 릿지 최소제곱으로 푼다. 판 번호 % 5 == 0인 판은 검증용으로 남긴다.

// 작은 선형대수라 인덱스 루프가 식과 대응이 잘 보인다.
#![allow(clippy::needless_range_loop, clippy::type_complexity)]

use palamedes::board::{Board, Side};
use palamedes::eval::is_race;
use palamedes::explain::{K, NAMES, Surrogate, surrogate_inputs};
use palamedes::features::compute_features;
use palamedes::nn::NnEval;
use palamedes::rank_moves;
use palamedes::rng::Rng;

const TOP: usize = 10;

/// 한 판에서 (판 번호, 차례별 [(입력, 에퀴티)]) 를 모은다.
fn game_turns(e: &NnEval, seed: u64) -> Vec<Vec<([f32; K], f32)>> {
    let mut dice = Rng::new(seed);
    let mut b = Board::start();
    let mut side = Side::White;
    let mut out = Vec::new();
    while b.result().is_none() {
        let (d1, d2) = dice.roll();
        let ranked = rank_moves(e, &b, side, d1, d2);
        if ranked.len() >= 2 {
            let turn: Vec<_> = ranked
                .iter()
                .take(TOP)
                .map(|m| {
                    (
                        surrogate_inputs(&compute_features(&m.board), side, is_race(&m.board)),
                        m.equity,
                    )
                })
                .collect();
            out.push(turn);
        }
        b = ranked[0].board;
        side = side.opp();
    }
    out
}

fn demean(turn: &[([f32; K], f32)]) -> Vec<([f64; K], f64)> {
    let n = turn.len() as f64;
    let mut mx = [0.0f64; K];
    let mut my = 0.0;
    for (x, y) in turn {
        for k in 0..K {
            mx[k] += x[k] as f64 / n;
        }
        my += *y as f64 / n;
    }
    turn.iter()
        .map(|(x, y)| {
            let mut d = [0.0; K];
            for k in 0..K {
                d[k] = x[k] as f64 - mx[k];
            }
            (d, *y as f64 - my)
        })
        .collect()
}

/// 가우스 소거로 A w = b.
fn solve(mut a: Vec<[f64; K]>, mut b: [f64; K]) -> [f64; K] {
    for c in 0..K {
        let p = (c..K)
            .max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))
            .unwrap();
        a.swap(c, p);
        b.swap(c, p);
        for r in c + 1..K {
            let f = a[r][c] / a[c][c];
            for j in c..K {
                a[r][j] -= f * a[c][j];
            }
            b[r] -= f * b[c];
        }
    }
    let mut w = [0.0; K];
    for c in (0..K).rev() {
        let s: f64 = (c + 1..K).map(|j| a[c][j] * w[j]).sum();
        w[c] = (b[c] - s) / a[c][c];
    }
    w
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let games: u64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(2000);
    let out = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "weights/surrogate.json".into());
    let e = NnEval::embedded();
    let threads = 4u64;
    let per_thread: Vec<Vec<(u64, Vec<Vec<([f32; K], f32)>>)>> = std::thread::scope(|sc| {
        let hs: Vec<_> = (0..threads)
            .map(|t| {
                let e = e.clone();
                sc.spawn(move || {
                    (0..games)
                        .filter(|g| g % threads == t)
                        .map(|g| (g, game_turns(&e, 7_000_000 + g)))
                        .collect()
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });

    let mut ata = vec![[0.0f64; K]; K];
    let mut atb = [0.0f64; K];
    let (mut train_pos, mut train_n) = (0usize, 0usize);
    let mut held: Vec<Vec<([f64; K], f64)>> = Vec::new();
    for (g, turns) in per_thread.into_iter().flatten() {
        for turn in turns {
            let d = demean(&turn);
            if g % 5 == 0 {
                held.push(d);
                continue;
            }
            train_pos += 1;
            for (x, y) in &d {
                train_n += 1;
                for i in 0..K {
                    atb[i] += x[i] * y;
                    for j in 0..K {
                        ata[i][j] += x[i] * x[j];
                    }
                }
            }
        }
    }
    // 릿지: 거의 변하지 않는 입력(예: 바에 남은 내 말)의 가중치가 튀지 않도록.
    for (i, row) in ata.iter_mut().enumerate() {
        row[i] += 1e-3 * train_n as f64;
    }
    let w = solve(ata, atb);
    let (mut sse, mut sst) = (0.0, 0.0);
    for turn in &held {
        for (x, y) in turn {
            let p: f64 = (0..K).map(|k| w[k] * x[k]).sum();
            sse += (y - p) * (y - p);
            sst += y * y;
        }
    }
    let r2 = 1.0 - sse / sst;
    let s = Surrogate {
        features: NAMES.iter().map(|s| s.to_string()).collect(),
        weights: w.iter().map(|&v| v as f32).collect(),
        r2_heldout: r2 as f32,
        positions: train_pos,
        samples: train_n,
    };
    for (n, v) in NAMES.iter().zip(&s.weights) {
        println!("{n:>16} {v:+.4}");
    }
    println!(
        "train positions {train_pos}, samples {train_n}, held-out positions {}, R² {r2:.3}",
        held.len()
    );
    std::fs::write(&out, serde_json::to_string_pretty(&s).unwrap() + "\n").expect("write");
}
