//! 미러 대국으로 두 플레이어의 강도를 비교한다.
//! `cargo run --release --example match -- <A> <B> [쌍 수] [시드]`
//! 플레이어: random, hand(손 평가 1수 탐색)

use std::time::Instant;

use palamedes::HandEval;
use palamedes::play::{OnePly, Player, RandomPlayer, mirror_match};

fn player(name: &str, seed: u64) -> Box<dyn Player> {
    match name {
        "random" => Box::new(RandomPlayer::new(seed ^ 0xABCDEF)),
        "hand" => Box::new(OnePly::new(HandEval::default(), "hand")),
        _ => panic!("unknown player {name} (random | hand)"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a_name = args.first().map(String::as_str).unwrap_or("hand");
    let b_name = args.get(1).map(String::as_str).unwrap_or("random");
    let pairs: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(500);
    let seed: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1);
    let mut a = player(a_name, seed);
    let mut b = player(b_name, seed + 1);
    let t = Instant::now();
    let st = mirror_match(a.as_mut(), b.as_mut(), pairs, seed);
    let dt = t.elapsed().as_secs_f64();
    println!(
        "{a_name} vs {b_name}: {} games ({} mirror pairs), {a_name} wins {:.1}%, ppg {:+.3} ± {:.3} (1σ), gammons {}/{} , {:.1}s ({:.1} ms/game)",
        st.games,
        st.pairs,
        100.0 * st.win_rate(),
        st.ppg(),
        st.ppg_stderr(),
        st.a_gammons,
        st.b_gammons,
        dt,
        1000.0 * dt / st.games as f64
    );
}
