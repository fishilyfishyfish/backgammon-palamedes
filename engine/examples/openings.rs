//! 시작 국면 15가지 주사위에 대해 평가기가 고르는 수와 상위 후보를 보여 준다.
//! `cargo run --release --example openings`

use palamedes::board::{Board, Side};
use palamedes::notation::format_move;
use palamedes::{HandEval, rank_moves};

fn main() {
    let e = HandEval::default();
    let b = Board::start();
    for d1 in (1..=6u8).rev() {
        for d2 in (1..d1).rev() {
            let r = rank_moves(&e, &b, Side::White, d1, d2);
            let top: Vec<String> = r
                .iter()
                .take(3)
                .map(|s| format!("{} ({:+.3})", format_move(s.mv.steps()), s.equity))
                .collect();
            println!("{d1}{d2}: {}", top.join(", "));
        }
    }
    let p = palamedes::Evaluator::evaluate(&e, &b, Side::White);
    println!("start, on roll: {p:?} equity {:+.3}", p.equity());
}
