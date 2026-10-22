//! 큐브 판단 채점: 자기대국 국면에서 우리 큐브 판단(`cube::decide`, 머니)을 gnubg 큐브 판단(2-ply 큐브 있는 평가)으로 채점한다.
//! 큐브 두 상황(가운데 1, 내가 가진 2)마다, gnubg 기준으로 더블 쪽 손실(우리가 고른 행동의 에퀴티 − 최선)과
//! 받는 쪽 손실을 모으고, 우리와 gnubg의 "더블 이득(min(테이크, 패스) − 노 더블)" 차이의 치우침을 본다.
//! `cargo run --release --example cube_check -- [국면 수] [gnubg 깊이] [x 보정] [더블 여유들(쉼표)] [접촉 노 더블 보정] [시드]`
//! 더블 여유 δ: 우리 더블 이득이 δ를 넘을 때만 더블한 것으로 다시 채점한다(실험용).

#[path = "support/gnubg.rs"]
mod gnubg;

use gnubg::{Gnubg, GnubgCube};
use palamedes::board::{Board, Side};
use palamedes::cube::{CubeDecision, CubeState, decide_tuned};
use palamedes::eval::is_race;
use palamedes::nn::NnEval;
use palamedes::rng::Rng;
use palamedes::search::best_move;

/// 자기대국(1수)에서 굴리기 전 국면을 모은다. 접촉 국면 위주로 너무 한쪽으로 기운 국면은 뺀다(gnubg 판단이 뻔함).
fn positions(n: usize, seed: u64) -> Vec<(Board, Side)> {
    let e = NnEval::embedded();
    let mut out = Vec::new();
    let mut g = 0;
    while out.len() < n {
        let mut rng = Rng::new(seed + g);
        g += 1;
        let mut b = Board::start();
        let mut side = Side::White;
        let mut t = 0;
        while b.result().is_none() {
            if t > 2 && t % 3 == (g % 3) as usize {
                out.push((b, side));
            }
            let (d1, d2) = rng.roll();
            b = best_move(&e, &b, side, d1, d2).board;
            side = side.opp();
            t += 1;
        }
    }
    out.truncate(n);
    out
}

#[derive(Default)]
struct Tally {
    n: u32,
    doubler_loss: f64,
    taker_loss: f64,
    takes_judged: u32,
    missed: u32,
    wrong_double: u32,
    wrong_take: u32,
    wrong_pass: u32,
    big: u32,
    gain_bias: f64,
    // 더블 이득이 −0.3 이내인 국면에서 노 더블·더블/테이크 값의 치우침(우리 − gnubg).
    near: u32,
    nd_bias: f64,
    dt_bias: f64,
}

fn score(t: &mut Tally, ours: &CubeDecision, g: &GnubgCube) {
    let g_double = g.take.min(g.pass);
    let best = g.no_double.max(g_double);
    let chosen = if ours.double { g_double } else { g.no_double };
    let dl = (best - chosen) as f64;
    t.n += 1;
    t.doubler_loss += dl;
    if dl > 0.04 {
        t.big += 1;
    }
    if !ours.double && g_double > g.no_double + 1e-4 {
        t.missed += 1;
    }
    if ours.double && g_double + 1e-4 < g.no_double {
        t.wrong_double += 1;
    }
    // 받는 쪽: 더블 이득이 있거나 비슷한(−0.2 이내) 국면만 채점.
    if g_double > g.no_double - 0.2 {
        let chosen_t = if ours.take { g.take } else { g.pass };
        let tl = (chosen_t - g.take.min(g.pass)) as f64;
        t.taker_loss += tl;
        t.takes_judged += 1;
        if ours.take && g.take > g.pass + 1e-4 {
            t.wrong_take += 1;
        }
        if !ours.take && g.take + 1e-4 < g.pass {
            t.wrong_pass += 1;
        }
    }
    let our_gain = ours.double_take.min(ours.double_pass) - ours.no_double;
    t.gain_bias += (our_gain - (g_double - g.no_double)) as f64;
    if g_double > g.no_double - 0.3 {
        t.near += 1;
        t.nd_bias += (ours.no_double - g.no_double) as f64;
        t.dt_bias += (ours.double_take - g.take) as f64;
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let n: usize = args.first().and_then(|s| s.parse().ok()).unwrap_or(1000);
    let plies: u8 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(2);
    let shift: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let margins: Vec<f32> = args
        .get(3)
        .map(|s| s.split(',').map(|x| x.parse().unwrap()).collect())
        .unwrap_or(vec![0.0]);
    let nd_adj: f32 = args
        .get(4)
        .and_then(|s| s.parse().ok())
        .unwrap_or(palamedes::cube::CONTACT_ND_ADJ);
    let seed: u64 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(4242);
    let pos = positions(n, seed);
    let threads = 4;
    // (상황 이름, 큐브 값, 내가 가졌는가)
    let cases = [("centered", 1u8, false), ("owned 2", 2u8, true)];
    let results: Vec<Vec<(usize, bool, CubeDecision, GnubgCube)>> = std::thread::scope(|sc| {
        let hs: Vec<_> = (0..threads)
            .map(|t| {
                let pos = &pos;
                sc.spawn(move || {
                    let e = NnEval::embedded();
                    let mut g = Gnubg::start(plies);
                    let mut out = Vec::new();
                    for i in (t..pos.len()).step_by(threads) {
                        let (b, side) = pos[i];
                        for (k, (_, value, mine)) in cases.iter().enumerate() {
                            let cube = CubeState {
                                value: *value,
                                owner: mine.then_some(side),
                            };
                            let ours = decide_tuned(&e, &b, side, cube, None, shift, nd_adj);
                            let gc = g.cube(&b, side, *value, mine.then_some(true));
                            out.push((k, is_race(&b), ours, gc));
                        }
                    }
                    out
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for &margin in &margins {
        println!(
            "{n} positions, gnubg {plies}-ply cubeful judge, x shift {shift:+}, double margin {margin}"
        );
        for (k, (name, _, _)) in cases.iter().enumerate() {
            for race in [false, true] {
                let mut t = Tally::default();
                for (kk, r, ours, gc) in results.iter().flatten() {
                    if *kk == k && *r == race {
                        let mut o = ours.clone();
                        if o.double {
                            o.double = o.double_take.min(o.double_pass) > o.no_double + margin;
                        }
                        score(&mut t, &o, gc);
                    }
                }
                if t.n == 0 {
                    continue;
                }
                println!(
                    "{name:9} {:7} | n {:4} | doubler loss/decision {:.4} (>0.04: {}), missed {} wrong-double {} | taker loss/judged {:.4} (n {}), wrong-take {} wrong-pass {} | our double gain − gnubg {:+.4} | near {}: ND {:+.4}, DT {:+.4}",
                    if race { "race" } else { "contact" },
                    t.n,
                    t.doubler_loss / t.n as f64,
                    t.big,
                    t.missed,
                    t.wrong_double,
                    t.taker_loss / t.takes_judged.max(1) as f64,
                    t.takes_judged,
                    t.wrong_take,
                    t.wrong_pass,
                    t.gain_bias / t.n as f64,
                    t.near,
                    t.nd_bias / t.near.max(1) as f64,
                    t.dt_bias / t.near.max(1) as f64,
                );
            }
        }
    }
}
