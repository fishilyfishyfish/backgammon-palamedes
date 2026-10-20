//! 시작 국면 15가지 주사위에 대해 평가기가 고르는 수와 상위 후보를 보여 준다.
//! `cargo run --release --example openings -- [hand | nn | nn:<경로>]`

use palamedes::board::{Board, Side};
use palamedes::nn::{Net, NnEval};
use palamedes::notation::format_move;
use palamedes::{Evaluator, HandEval, rank_moves};

fn main() {
    let which = std::env::args().nth(1).unwrap_or_else(|| "hand".into());
    let e: Box<dyn Evaluator> = match which.as_str() {
        "hand" => Box::new(HandEval::default()),
        "nn" => Box::new(NnEval::embedded()),
        p => Box::new(NnEval::new(
            Net::from_bytes(
                &std::fs::read(p.strip_prefix("nn:").expect("hand | nn | nn:<path>"))
                    .expect("file"),
            )
            .expect("weights"),
        )),
    };
    let b = Board::start();
    for d1 in (1..=6u8).rev() {
        for d2 in (1..d1).rev() {
            let r = rank_moves(e.as_ref(), &b, Side::White, d1, d2);
            let top: Vec<String> = r
                .iter()
                .take(3)
                .map(|s| format!("{} ({:+.3})", format_move(s.mv.steps()), s.equity))
                .collect();
            println!("{d1}{d2}: {}", top.join(", "));
        }
    }
    let p = e.evaluate(&b, Side::White);
    println!("start, on roll: {p:?} equity {:+.3}", p.equity());
}
