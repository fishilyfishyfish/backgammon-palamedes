//! GNU 백개먼과 미러 대국(큐브 없는 머니, 측정 전용). gnubg는 apt 패키지(/usr/games/gnubg)를 파이썬 스크립트
//! 모드로 띄우고(`scripts/gnubg-driver.py`), 국면·주사위를 보내 최선 수를 받는다. 스레드마다 gnubg 프로세스 하나.
//! `cargo run --release --example vs_gnubg -- <우리 플레이어> <gnubg 깊이(0|1|2)> [쌍 수] [시드]`
//! 우리 플레이어: nn | nn@2 | nn:<경로> | nn:<경로>@2 | gnubg:<깊이>(gnubg끼리, 측정 점검용)

use std::time::Instant;

use palamedes::nn::{Net, NnEval};
use palamedes::play::{OnePly, Player, Searcher, parallel_mirror_match};
use palamedes::search::Search;

#[path = "support/gnubg.rs"]
mod gnubg;
use gnubg::{Gnubg, GnubgPlayer};

fn ours(spec: &str) -> Box<dyn Fn(usize) -> Box<dyn Player> + Sync> {
    if let Some(p) = spec.strip_prefix("gnubg:") {
        let plies: u8 = p.parse().expect("gnubg:<plies>");
        let label = spec.to_string();
        return Box::new(move |_| {
            Box::new(GnubgPlayer {
                g: Gnubg::start(plies),
                label: label.clone(),
            }) as Box<dyn Player>
        });
    }
    let (name, ply) = match spec.split_once('@') {
        Some((n, p)) => (n.to_string(), p.parse::<u8>().unwrap()),
        None => (spec.to_string(), 1),
    };
    let e = match name.strip_prefix("nn:") {
        Some(p) => {
            NnEval::new(Net::from_bytes(&std::fs::read(p).expect("weights")).expect("weights"))
        }
        None => NnEval::embedded(),
    };
    let label = spec.to_string();
    Box::new(move |_| -> Box<dyn Player> {
        if ply < 2 {
            Box::new(OnePly::new(e.clone(), &label))
        } else {
            Box::new(Searcher {
                eval: e.clone(),
                search: Search::from_ply(ply),
                label: label.clone(),
            })
        }
    })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a = args.first().cloned().unwrap_or("nn@2".into());
    let plies: u8 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let pairs: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(200);
    let seed: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1);
    let t = Instant::now();
    let st = parallel_mirror_match(
        ours(&a),
        |_| {
            Box::new(GnubgPlayer {
                g: Gnubg::start(plies),
                label: format!("gnubg {plies}-ply"),
            }) as Box<dyn Player>
        },
        pairs,
        seed,
        4,
    );
    println!(
        "{a} vs gnubg {plies}-ply: {} games ({} pairs), {a} wins {:.1}%, ppg {:+.3} ± {:.3}, gammons {}/{}, {:.0}s",
        st.games,
        st.pairs,
        100.0 * st.win_rate(),
        st.ppg(),
        st.ppg_stderr(),
        st.a_gammons,
        st.b_gammons,
        t.elapsed().as_secs_f64()
    );
}
