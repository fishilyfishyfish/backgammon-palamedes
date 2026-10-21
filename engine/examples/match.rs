//! 미러 대국으로 두 플레이어의 강도를 비교한다(4스레드).
//! `cargo run --release --example match -- <A> <B> [쌍 수] [시드]`
//! 플레이어: random, hand(손 평가 1수 탐색), nn(포함된 가중치 1수 탐색), nn:<경로>(가중치 파일),
//!         bot:<id>(봇, weights/bots.json), lv:<잡음>:<습관 배율>(1수, 기본 습관에 배율, 조정용), 뒤에 @2를 붙이면 2수 탐색(예: nn@2, hand@2), @2f는 후보를 거르지 않는 2수

use std::time::Instant;

use palamedes::HandEval;
use palamedes::nn::{Net, NnEval};
use palamedes::play::{OnePly, Player, RandomPlayer, Searcher, parallel_mirror_match};
use palamedes::search::{Filter, Search};

fn load(name: &str) -> Option<NnEval> {
    match name {
        "nn" => Some(NnEval::embedded()),
        _ => name.strip_prefix("nn:").map(|p| {
            NnEval::new(Net::from_bytes(&std::fs::read(p).expect("weights file")).expect("weights"))
        }),
    }
}

fn maker(spec: &str, seed: u64) -> impl Fn(usize) -> Box<dyn Player> + Sync {
    let (name, ply) = match spec.split_once('@') {
        Some((n, p)) => (
            n.to_string(),
            p.trim_end_matches('f').parse::<u8>().expect("ply after @"),
        ),
        None => (spec.to_string(), 1),
    };
    let nn = load(&name);
    let mut search = Search::from_ply(ply);
    if spec.ends_with('f') {
        search.filter = Filter {
            keep: usize::MAX,
            margin: f32::INFINITY,
        };
    }
    let label = spec.to_string();
    move |t| -> Box<dyn Player> {
        match name.as_str() {
            "random" => Box::new(RandomPlayer::new(seed ^ 0xABCDEF ^ t as u64)),
            b if b.starts_with("bot:") || b.starts_with("lv:") => {
                Box::new(palamedes::bots::BotPlayer {
                    eval: NnEval::embedded(),
                    style: bot_spec(b),
                    label: label.clone(),
                    rng: palamedes::rng::Rng::new(seed ^ 0x1E7E1 ^ t as u64),
                })
            }
            "hand" if ply < 2 => Box::new(OnePly::new(HandEval::default(), &label)),
            "hand" => Box::new(Searcher {
                eval: HandEval::default(),
                search,
                label: label.clone(),
            }),
            _ => {
                let e = nn
                    .clone()
                    .unwrap_or_else(|| panic!("unknown player {name}"));
                if ply < 2 {
                    Box::new(OnePly::new(e, &label))
                } else {
                    Box::new(Searcher {
                        eval: e,
                        search,
                        label: label.clone(),
                    })
                }
            }
        }
    }
}

/// bot:<id> 또는 lv:<잡음>:<습관 배율>(1수, 모든 기본 습관에 같은 배율).
fn bot_spec(spec: &str) -> palamedes::bots::Style {
    if let Some(id) = spec.strip_prefix("bot:") {
        return palamedes::bots::bots()
            .iter()
            .find(|b| b.id == id)
            .unwrap_or_else(|| panic!("unknown bot {id}"))
            .style;
    }
    let (n, h) = spec[3..].split_once(':').expect("lv:<noise>:<habit>");
    let h: f32 = h.parse().unwrap();
    palamedes::bots::Style {
        ply: 1,
        noise: n.parse().unwrap(),
        habits: [h; 6],
        cube_noise: 0.0,
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
