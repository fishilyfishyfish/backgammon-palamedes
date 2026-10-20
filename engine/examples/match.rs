//! 미러 대국으로 두 플레이어의 강도를 비교한다(4스레드).
//! `cargo run --release --example match -- <A> <B> [쌍 수] [시드]`
//! 플레이어: random, hand(손 평가 1수 탐색), nn(포함된 가중치 1수 탐색), nn:<경로>(가중치 파일)

use std::time::Instant;

use palamedes::HandEval;
use palamedes::nn::{Net, NnEval};
use palamedes::play::{OnePly, Player, RandomPlayer, parallel_mirror_match};

fn load(name: &str) -> Option<NnEval> {
    match name {
        "nn" => Some(NnEval::embedded()),
        _ => name.strip_prefix("nn:").map(|p| {
            NnEval::new(Net::from_bytes(&std::fs::read(p).expect("weights file")).expect("weights"))
        }),
    }
}

fn maker(name: &str, seed: u64) -> impl Fn(usize) -> Box<dyn Player> + Sync {
    let nn = load(name);
    let name = name.to_string();
    move |t| -> Box<dyn Player> {
        match name.as_str() {
            "random" => Box::new(RandomPlayer::new(seed ^ 0xABCDEF ^ t as u64)),
            "hand" => Box::new(OnePly::new(HandEval::default(), "hand")),
            _ => Box::new(OnePly::new(
                nn.clone()
                    .unwrap_or_else(|| panic!("unknown player {name}")),
                &name,
            )),
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a_name = args.first().map(String::as_str).unwrap_or("nn");
    let b_name = args.get(1).map(String::as_str).unwrap_or("hand");
    let pairs: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(500);
    let seed: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1);
    let t = Instant::now();
    let st = parallel_mirror_match(maker(a_name, seed), maker(b_name, seed + 1), pairs, seed, 4);
    let dt = t.elapsed().as_secs_f64();
    println!(
        "{a_name} vs {b_name}: {} games ({} mirror pairs), {a_name} wins {:.1}%, ppg {:+.3} ± {:.3} (1σ), gammons {}/{}, {:.1}s",
        st.games,
        st.pairs,
        100.0 * st.win_rate(),
        st.ppg(),
        st.ppg_stderr(),
        st.a_gammons,
        st.b_gammons,
        dt,
    );
}
