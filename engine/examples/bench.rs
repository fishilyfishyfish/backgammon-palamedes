//! 성능 측정: `cargo run --release --example bench`.
//! 무작위 자기대국 국면에서 합법수 생성·특징값 계산 속도를 잰다.

use std::time::Instant;

use palamedes::board::{Board, Side};
use palamedes::rng::Rng;
use palamedes::{Evaluator, HandEval, best_move, compute_features, legal_moves};

fn main() {
    let mut rng = Rng::new(7);
    let mut positions = Vec::new();
    while positions.len() < 20_000 {
        let mut b = Board::start();
        let mut side = Side::White;
        while b.result().is_none() {
            let (d1, d2) = rng.roll();
            positions.push((b, side, d1, d2));
            let moves = legal_moves(&b, side, d1, d2);
            b.apply_steps(side, moves[rng.below(moves.len() as u64) as usize].steps());
            side = side.opp();
        }
    }

    let t = Instant::now();
    let mut total = 0usize;
    for (b, s, d1, d2) in &positions {
        total += legal_moves(b, *s, *d1, *d2).len();
    }
    let dt = t.elapsed().as_secs_f64();
    println!(
        "legal_moves: {} positions in {:.3}s = {:.0} pos/s (avg {:.1} moves)",
        positions.len(),
        dt,
        positions.len() as f64 / dt,
        total as f64 / positions.len() as f64
    );

    let n = 2_000;
    let t = Instant::now();
    for (b, ..) in positions.iter().take(n) {
        std::hint::black_box(compute_features(b));
    }
    let dt = t.elapsed().as_secs_f64();
    println!(
        "compute_features: {n} positions in {dt:.3}s = {:.0} pos/s",
        n as f64 / dt
    );

    let e = HandEval::default();
    let t = Instant::now();
    for (b, s, ..) in positions.iter().take(n) {
        std::hint::black_box(e.evaluate(b, *s));
    }
    let dt = t.elapsed().as_secs_f64();
    println!(
        "HandEval::evaluate: {n} positions in {dt:.3}s = {:.0} pos/s",
        n as f64 / dt
    );

    let n = 1_000;
    let t = Instant::now();
    for (b, s, d1, d2) in positions.iter().take(n) {
        std::hint::black_box(best_move(&e, b, *s, *d1, *d2));
    }
    let dt = t.elapsed().as_secs_f64();
    println!(
        "best_move (1-ply, HandEval): {n} decisions in {dt:.3}s = {:.2} ms/decision",
        1000.0 * dt / n as f64
    );
}
