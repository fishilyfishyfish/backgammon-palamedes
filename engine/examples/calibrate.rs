//! 신경망 보정 검사: 자기대국 국면에서 신경망 1수 평가(수를 둔 쪽 기준)와 롤아웃 결과를 비교한다.
//! 롤아웃도 같은 신경망 1수로 두므로 "자기 플레이 아래에서 예측이 실제 결과와 맞는가"를 본다.
//! `cargo run --release --example calibrate -- [국면 수] [국면당 시행 수]`

use std::time::Instant;

use palamedes::board::{Board, Side};
use palamedes::eval::{OutcomeProbs, is_race};
use palamedes::nn::NnEval;
use palamedes::rng::Rng;
use palamedes::rollout::rollout;
use palamedes::search::{best_move, evaluate_after};

struct Sample {
    race: bool,
    pred: OutcomeProbs,
    roll: OutcomeProbs,
    pred_eq: f64,
    roll_eq: f64,
}

/// 시드 하나로 한 판을 두고, 차례 번호가 `pick`(7로 나눈 나머지)인 수를 둔 직후 국면을 모은다.
fn positions(e: &NnEval, seed: u64) -> Vec<(Board, Side)> {
    let mut rng = Rng::new(seed);
    let (mut d1, mut d2) = loop {
        let (a, b) = rng.roll();
        if a != b {
            break (a, b);
        }
    };
    let mut side = if d1 > d2 { Side::White } else { Side::Black };
    let mut b = Board::start();
    let mut out = Vec::new();
    let pick = (seed % 7) as usize;
    for turn in 0.. {
        b = best_move(e, &b, side, d1, d2).board;
        if b.result().is_some() {
            break;
        }
        if turn % 7 == pick {
            out.push((b, side));
        }
        side = side.opp();
        (d1, d2) = rng.roll();
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let n: usize = args.first().and_then(|s| s.parse().ok()).unwrap_or(400);
    let trials: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1296);
    let e = NnEval::embedded();
    let mut pos = Vec::new();
    let mut seed = 900_000u64;
    while pos.len() < n {
        pos.extend(positions(&e, seed));
        seed += 1;
    }
    pos.truncate(n);
    let t = Instant::now();
    let threads = 4;
    let samples: Vec<Sample> = std::thread::scope(|sc| {
        let hs: Vec<_> = (0..threads)
            .map(|th| {
                let pos = &pos;
                sc.spawn(move || {
                    let e = NnEval::embedded();
                    (th..pos.len())
                        .step_by(threads)
                        .map(|i| {
                            let (b, mover) = pos[i];
                            let pred = evaluate_after(&e, &b, mover);
                            let r = rollout(&e, &b, mover, 0, trials, 7_000 + i as u64);
                            Sample {
                                race: is_race(&b),
                                pred,
                                roll: r.probs(),
                                pred_eq: pred.equity() as f64,
                                roll_eq: r.equity(),
                            }
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });
    println!(
        "{} positions × {trials} trials, {:.0}s",
        samples.len(),
        t.elapsed().as_secs_f64()
    );
    println!("predicted win bucket | n | NN win | rollout win | NN gammon (w+l) | rollout gammon");
    for k in 0..10 {
        let lo = k as f32 / 10.0;
        let s: Vec<&Sample> = samples
            .iter()
            .filter(|s| s.pred.win >= lo && (s.pred.win < lo + 0.1 || k == 9))
            .collect();
        if s.is_empty() {
            continue;
        }
        let m = |f: &dyn Fn(&Sample) -> f32| {
            s.iter().map(|x| f(x) as f64).sum::<f64>() / s.len() as f64
        };
        println!(
            "{:>3.0}–{:<3.0}% | {:>4} | {:5.1}% | {:5.1}% | {:5.1}% | {:5.1}%",
            100.0 * lo,
            100.0 * (lo + 0.1),
            s.len(),
            100.0 * m(&|x| x.pred.win),
            100.0 * m(&|x| x.roll.win),
            100.0 * m(&|x| x.pred.win_gammon + x.pred.lose_gammon),
            100.0 * m(&|x| x.roll.win_gammon + x.roll.lose_gammon),
        );
    }
    for (label, race) in [("contact", false), ("race", true)] {
        let s: Vec<&Sample> = samples.iter().filter(|x| x.race == race).collect();
        if s.is_empty() {
            continue;
        }
        let k = s.len() as f64;
        let bias = s.iter().map(|x| x.pred_eq - x.roll_eq).sum::<f64>() / k;
        let mae = s.iter().map(|x| (x.pred_eq - x.roll_eq).abs()).sum::<f64>() / k;
        let wbias = s
            .iter()
            .map(|x| (x.pred.win - x.roll.win) as f64)
            .sum::<f64>()
            / k;
        let gbias = s
            .iter()
            .map(|x| {
                (x.pred.win_gammon + x.pred.lose_gammon - x.roll.win_gammon - x.roll.lose_gammon)
                    as f64
            })
            .sum::<f64>()
            / k;
        println!(
            "{label}: n {} | equity bias {bias:+.3}, mean |error| {mae:.3} | win bias {:+.1}%p | gammon bias {:+.1}%p",
            s.len(),
            100.0 * wbias,
            100.0 * gbias
        );
    }
}
