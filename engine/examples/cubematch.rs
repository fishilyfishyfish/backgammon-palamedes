//! 큐브 정책 비교: 머니 게임(큐브 있음)과 매치. 같은 주사위로 자리를 바꿔 두 번씩 둔다(미러).
//! `cargo run --release --example cubematch -- money <A> <B> [쌍 수]`
//! `cargo run --release --example cubematch -- match <A> <B> <매치 길이> [쌍 수]`
//! 정책: janowski(점수 반영), money(점수 무시, 머니 판단), never(더블 안 함, 항상 받음). 수는 모두 신경망 2수.
//! 뒤에 `:<보정>`을 붙이면 큐브 효율 x에 보정을 더한다(예: janowski:+0.1).

use std::time::Instant;

use palamedes::board::Side;
use palamedes::nn::NnEval;
use palamedes::play::{CubeAgent, CubePolicy, play_cube_game, play_match};
use palamedes::search::Search;

fn agent(spec: &str) -> CubeAgent<NnEval> {
    // "janowski" 또는 "janowski:+0.1"(큐브 효율 보정)
    let (name, x_shift) = match spec.split_once(':') {
        Some((n, x)) => (n, x.parse::<f32>().expect("x shift")),
        None => (spec, 0.0),
    };
    let policy = match name {
        "janowski" => CubePolicy::Janowski,
        "money" => CubePolicy::MoneyOnly,
        "never" => CubePolicy::Never,
        _ => panic!("unknown policy {name}"),
    };
    CubeAgent {
        eval: NnEval::embedded(),
        search: Search::TWO_PLY,
        policy,
        label: spec.into(),
        x_shift,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args[0].as_str();
    let (a, b) = (args[1].clone(), args[2].clone());
    let (length, pairs) = if mode == "match" {
        (
            args[3].parse::<u8>().unwrap(),
            args.get(4).and_then(|s| s.parse().ok()).unwrap_or(200u32),
        )
    } else {
        (
            0,
            args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1000u32),
        )
    };
    let threads = 4u32;
    let t = Instant::now();
    // (A 점수 합, A 점수 제곱합(쌍 단위), A 승 수, 판 수)
    let parts: Vec<(f64, f64, u32, u32)> = std::thread::scope(|sc| {
        let hs: Vec<_> = (0..threads)
            .map(|th| {
                let (a, b) = (a.clone(), b.clone());
                sc.spawn(move || {
                    let (mut pa, mut pb) = (agent(&a), agent(&b));
                    let (mut sum, mut sq, mut wins, mut n) = (0.0, 0.0, 0u32, 0u32);
                    for i in (th..pairs).step_by(threads as usize) {
                        let seed = 50_000 + i as u64;
                        let mut pair = 0.0;
                        for a_side in [Side::White, Side::Black] {
                            let (w, pts) = if mode == "match" {
                                let (w, _) = if a_side == Side::White {
                                    play_match([&mut pa, &mut pb], length, seed)
                                } else {
                                    play_match([&mut pb, &mut pa], length, seed)
                                };
                                (w, 1.0)
                            } else {
                                let r = if a_side == Side::White {
                                    play_cube_game([&mut pa, &mut pb], seed, None)
                                } else {
                                    play_cube_game([&mut pb, &mut pa], seed, None)
                                };
                                (r.winner, r.points as f64)
                            };
                            let won = w == a_side;
                            pair += if won { pts } else { -pts };
                            wins += won as u32;
                            n += 1;
                        }
                        sum += pair;
                        sq += pair * pair;
                    }
                    (sum, sq, wins, n)
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let (sum, sq, wins, n) = parts.iter().fold((0.0, 0.0, 0, 0), |x, p| {
        (x.0 + p.0, x.1 + p.1, x.2 + p.2, x.3 + p.3)
    });
    let np = (n / 2) as f64;
    let mean_pair = sum / np;
    let se_pair = ((sq / np - mean_pair * mean_pair).max(0.0) / (np - 1.0)).sqrt();
    if mode == "match" {
        println!(
            "{length}-point matches, {a} vs {b}: {n} matches, {a} wins {:.1}% ± {:.1}%, {:.0}s",
            100.0 * wins as f64 / n as f64,
            // 쌍 점수(±1씩 두 판)의 평균/4 + 0.5 = 승률, 표준오차도 /4.
            100.0 * se_pair / 4.0,
            t.elapsed().as_secs_f64()
        );
    } else {
        println!(
            "money with cube, {a} vs {b}: {n} games, {a} wins {:.1}%, ppg {:+.3} ± {:.3}, {:.0}s",
            100.0 * wins as f64 / n as f64,
            mean_pair / 2.0,
            se_pair / 2.0,
            t.elapsed().as_secs_f64()
        );
    }
}
