//! 봇별 실수 분포: 봇이 신경망 1수와 둔 판에서, 봇 쪽 수마다 손실(1수 최선 − 둔 수)을 모아
//! 등급 비율과 "쉬운 국면(1·2위 차이 ≥ 0.1)" / "어려운 국면(< 0.02)"의 실수율을 본다.
//! `cargo run --release --example bot_mistakes -- [판 수]`

use palamedes::board::{Board, Side};
use palamedes::bots::{bots, choose_move};
use palamedes::nn::NnEval;
use palamedes::rng::Rng;
use palamedes::search::{best_move, rank_moves};

fn main() {
    let games: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    let e = NnEval::embedded();
    println!(
        "bot (rating) | moves | best | <0.04 | ?! | ? | ?? | mean loss | error rate easy / hard"
    );
    for (bi, bot) in bots().iter().enumerate() {
        let l = bot.style;
        let mut rng = Rng::new(bi as u64);
        let (mut n, mut grades, mut sum) = (0u32, [0u32; 5], 0.0f64);
        let (mut easy, mut easy_err, mut hard, mut hard_err) = (0u32, 0u32, 0u32, 0u32);
        for g in 0..games {
            let mut dice = Rng::new(70_000 + g);
            let mut b = Board::start();
            let mut side = if g % 2 == 0 { Side::White } else { Side::Black };
            for _ in 0..300 {
                let (d1, d2) = dice.roll();
                if side == Side::White {
                    let ranked = rank_moves(&e, &b, side, d1, d2);
                    let m = choose_move(&e, &b, side, d1, d2, &l, None, &mut rng);
                    if ranked.len() > 1 {
                        // 같은 결과 국면이면 손실 0, 아니면 1수 값 차이(2수로 고른 수도 1수 눈금으로 잰다).
                        let mine = ranked.iter().find(|r| r.board == m.board).unwrap();
                        let loss = (ranked[0].equity - mine.equity) as f64;
                        let gap = ranked[0].equity - ranked[1].equity;
                        n += 1;
                        sum += loss;
                        let k = match loss {
                            0.0 => 0,
                            x if x < 0.04 => 1,
                            x if x < 0.08 => 2,
                            x if x < 0.16 => 3,
                            _ => 4,
                        };
                        grades[k] += 1;
                        if gap >= 0.1 {
                            easy += 1;
                            easy_err += (loss >= 0.04) as u32;
                        } else if gap < 0.02 {
                            hard += 1;
                            hard_err += (loss >= 0.04) as u32;
                        }
                    }
                    b = m.board;
                } else {
                    b = best_move(&e, &b, side, d1, d2).board;
                }
                if b.result().is_some() {
                    break;
                }
                side = side.opp();
            }
        }
        let pct = |x: u32| 100.0 * x as f64 / n as f64;
        println!(
            "{} ({}) | {n} | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.4} | {:.1}% / {:.1}%",
            bot.name,
            bot.rating,
            pct(grades[0]),
            pct(grades[1]),
            pct(grades[2]),
            pct(grades[3]),
            pct(grades[4]),
            sum / n as f64,
            100.0 * easy_err as f64 / easy.max(1) as f64,
            100.0 * hard_err as f64 / hard.max(1) as f64,
        );
    }
}
