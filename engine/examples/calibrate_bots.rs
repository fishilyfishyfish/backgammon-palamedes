//! 봇 레이팅 맞추기. 기준 상대 = 신경망 1수. 최강 봇(엔진 2수)을 1800으로 두고 기준 상대의 레이팅을 구한 뒤,
//! 나머지 봇은 기준 상대와 둔 승률(큐브 없는 단판)이 FIBS 공식의 목표 승률이 되도록 잡음 크기를 이분 탐색한다.
//! 잡음 0에서도 너무 약하면 습관 배율을 줄인다. 결과(잡음·습관·큐브 잡음·잰 레이팅)를 bots.json에 쓴다.
//! `cargo run --release --example calibrate_bots -- [탐색 쌍 수] [검증 쌍 수] [출력 경로]`

use palamedes::bots::{Bot, BotPlayer, Style, bots, rating_diff, win_prob};
use palamedes::nn::NnEval;
use palamedes::play::{OnePly, Player, parallel_mirror_match};
use palamedes::rng::Rng;

/// (기준 상대 대비 승률, 그 표준오차).
fn measure(style: Style, pairs: u32, seed: u64) -> (f32, f32) {
    let st = parallel_mirror_match(
        |t| {
            Box::new(BotPlayer {
                eval: NnEval::embedded(),
                style,
                label: "bot".into(),
                rng: Rng::new(seed ^ 0xB07 ^ t as u64),
            }) as Box<dyn Player>
        },
        |_| Box::new(OnePly::new(NnEval::embedded(), "ref")) as Box<dyn Player>,
        pairs,
        seed,
        4,
    );
    let p = st.win_rate() as f32;
    (p, (p * (1.0 - p) / st.games as f32).sqrt())
}

/// 승률 표준오차를 레이팅 표준오차로(공식의 기울기).
fn rating_se(p: f32, se: f32) -> f32 {
    se / (std::f32::consts::LN_10 / 2000.0 * p * (1.0 - p))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let pairs: u32 = args.first().and_then(|s| s.parse().ok()).unwrap_or(1000);
    let verify: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(3000);
    let out = args.get(2).cloned().unwrap_or("weights/bots.json".into());
    let mut list: Vec<Bot> = bots().to_vec();
    let top = list.last().unwrap().clone();
    assert_eq!(top.style, Style::ENGINE, "the strongest bot is the engine");
    let (p, se) = measure(top.style, verify, 7_000_000);
    let r_ref = top.rating as f32 - rating_diff(p, 1.0);
    println!(
        "{} (engine 2-ply) vs ref: win {:.1}% ± {:.1}% -> ref rating {:.0} ± {:.0}",
        top.name,
        100.0 * p,
        100.0 * se,
        r_ref,
        rating_se(p, se)
    );
    list.last_mut().unwrap().measured = Some([top.rating as f32, 0.0]);
    let n = list.len();
    for b in list.iter_mut().take(n - 1) {
        let target = win_prob(b.rating as f32 - r_ref, 1.0);
        let base = b.style;
        let seed = 8_000_000 + b.rating as u64;
        let at = |noise: f32, h: f32| Style {
            noise,
            habits: base.habits.map(|x| x * h),
            cube_noise: (3.0 * noise).min(0.3),
            ..base
        };
        // 잡음 0에서 목표보다 약하면 습관 배율을 줄이고, 아니면 잡음을 늘린다(둘 다 클수록 약해짐).
        let (p0, _) = measure(at(0.0, 1.0), pairs, seed);
        let (mut lo, mut hi) = (0.0f32, if p0 < target { 1.0 } else { 0.25 });
        let by_habit = p0 < target;
        for _ in 0..8 {
            let mid = 0.5 * (lo + hi);
            let st = if by_habit { at(0.0, mid) } else { at(mid, 1.0) };
            let (p, _) = measure(st, pairs, seed);
            if p > target {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let k = 0.5 * (lo + hi);
        b.style = if by_habit { at(0.0, k) } else { at(k, 1.0) };
        let (p, se) = measure(b.style, verify, seed + 1);
        let r = r_ref + rating_diff(p, 1.0);
        b.measured = Some([r.round(), rating_se(p, se).round()]);
        println!(
            "{} target {} (win {:.1}%): {} {:.3} -> win {:.1}% ± {:.1}%, rating {:.0} ± {:.0}",
            b.name,
            b.rating,
            100.0 * target,
            if by_habit { "habit scale" } else { "noise" },
            k,
            100.0 * p,
            100.0 * se,
            r,
            rating_se(p, se)
        );
    }
    std::fs::write(&out, serde_json::to_string_pretty(&list).unwrap() + "\n").expect("write");
    println!("wrote {out}");
}
