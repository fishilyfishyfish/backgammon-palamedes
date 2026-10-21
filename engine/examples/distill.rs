//! 2수 증류: 자기대국 국면을 지금 신경망의 2수(굴리기 전, 21가지 굴림마다 최선 수의 평균)로 평가한 값을
//! 정답으로 신경망을 이어서 학습한다. 1수 평가가 2수에 가까워지면 그 신경망으로 다시 정답을 만들어 반복한다.
//! 회차마다 시작 가중치와 1수 미러 대국으로 비교한다.
//! `cargo run --release --example distill -- key=value ...`
//! key: rounds(회차), positions(회차당 국면 수), epochs, lr, lr_end, explore(2위 수를 둘 확률), out, init, eval_pairs, seed

use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::time::Instant;

use palamedes::board::{Board, Side};
use palamedes::eval::{Evaluator, OutcomeProbs};
use palamedes::nn::{Net, NnEval, from_probs};
use palamedes::play::{OnePly, Player, parallel_mirror_match};
use palamedes::rng::Rng;
use palamedes::search::{TwoPly, rank_moves};

/// 자기대국(1수, `explore` 확률로 2위 수)에서 굴리기 전 국면을 모은다.
fn collect(e: &NnEval, n: usize, seed: u64, explore: f64) -> Vec<(Board, Side)> {
    let mut out = Vec::with_capacity(n);
    let mut g = 0u64;
    while out.len() < n {
        let mut rng = Rng::new(seed.wrapping_mul(1_000_003).wrapping_add(g));
        g += 1;
        let mut b = Board::start();
        let mut side = if g.is_multiple_of(2) {
            Side::White
        } else {
            Side::Black
        };
        let mut first = true;
        while b.result().is_none() && out.len() < n {
            let (d1, d2) = loop {
                let r = rng.roll();
                if !first || r.0 != r.1 {
                    break r;
                }
            };
            if !first {
                out.push((b, side));
            }
            first = false;
            let ranked = rank_moves(e, &b, side, d1, d2);
            let pick = if ranked.len() > 1 && (rng.below(1_000_000) as f64) < explore * 1e6 {
                1
            } else {
                0
            };
            b = ranked[pick].board;
            side = side.opp();
        }
    }
    out
}

fn main() {
    let mut rounds = 3usize;
    let mut positions = 400_000usize;
    let mut epochs = 2usize;
    let (mut lr0, mut lr1) = (0.004f32, 0.001f32);
    let mut explore = 0.1f64;
    let mut out = String::from("weights/td-distill.bin");
    let mut init: Option<String> = None;
    let mut eval_pairs = 2000u32;
    let mut seed = 11u64;
    for kv in std::env::args().skip(1) {
        let (k, v) = kv.split_once('=').expect("key=value");
        match k {
            "rounds" => rounds = v.parse().unwrap(),
            "positions" => positions = v.parse().unwrap(),
            "epochs" => epochs = v.parse().unwrap(),
            "lr" => lr0 = v.parse().unwrap(),
            "lr_end" => lr1 = v.parse().unwrap(),
            "explore" => explore = v.parse().unwrap(),
            "out" => out = v.to_string(),
            "init" => init = Some(v.to_string()),
            "eval_pairs" => eval_pairs = v.parse().unwrap(),
            "seed" => seed = v.parse().unwrap(),
            _ => panic!("unknown key {k}"),
        }
    }
    let start = match &init {
        Some(p) => NnEval::new(
            Net::read_from(&mut BufReader::new(File::open(p).expect("init"))).expect("read init"),
        ),
        None => NnEval::embedded(),
    };
    println!(
        "rounds={rounds} positions={positions} epochs={epochs} lr={lr0}->{lr1} explore={explore} out={out} init={init:?}"
    );
    let mut cur = start.clone();
    let t0 = Instant::now();
    for round in 0..rounds {
        // 1. 국면 모으기(4스레드)와 2수 정답.
        let per = positions / 4;
        let data: Vec<(Board, Side, [f32; 5])> = std::thread::scope(|sc| {
            let hs: Vec<_> = (0..4u64)
                .map(|t| {
                    let e = cur.clone();
                    sc.spawn(move || {
                        let two = TwoPly::new(e.clone());
                        collect(&e, per, seed + 100 * round as u64 + t, explore)
                            .into_iter()
                            .map(|(b, s)| {
                                let p: OutcomeProbs = two.evaluate(&b, s);
                                (b, s, from_probs(&p))
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
        });
        let t_data = t0.elapsed().as_secs_f64();
        // 2. 학습(섞어서 epochs번, 학습률 선형 감소).
        let mut net = (*cur.net).clone();
        let mut order: Vec<usize> = (0..data.len()).collect();
        let mut rng = Rng::new(seed ^ round as u64);
        let mut h = Vec::with_capacity(net.hidden);
        let total = (epochs * data.len()) as f32;
        let mut step = 0f32;
        let mut loss = 0f64;
        for _ in 0..epochs {
            for i in (1..order.len()).rev() {
                order.swap(i, rng.below(i as u64 + 1) as usize);
            }
            for &i in &order {
                let (b, s, t) = &data[i];
                let lr = lr0 + (lr1 - lr0) * step / total;
                loss += net.train_step(&net.encode(b, *s), t, lr, &mut h) as f64;
                step += 1.0;
            }
        }
        cur = NnEval::new(net);
        let mut f = BufWriter::new(File::create(&out).expect("out"));
        cur.net.write_to(&mut f).expect("write");
        f.flush().unwrap();
        // 3. 시작 가중치와 1수 미러 대국.
        let (a, b) = (cur.clone(), start.clone());
        let st = parallel_mirror_match(
            |_| Box::new(OnePly::new(a.clone(), "new")) as Box<dyn Player>,
            |_| Box::new(OnePly::new(b.clone(), "start")) as Box<dyn Player>,
            eval_pairs,
            123_000 + round as u64 * 10_000,
            4,
        );
        println!(
            "round {round}: {} positions ({t_data:.0}s), loss {:.4}, vs start 1-ply: ppg {:+.3} ± {:.3}, wins {:.1}%, {:.0}s",
            data.len(),
            loss / step as f64,
            st.ppg(),
            st.ppg_stderr(),
            100.0 * st.win_rate(),
            t0.elapsed().as_secs_f64()
        );
    }
}
