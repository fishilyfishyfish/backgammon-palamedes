//! 가중치 하나를 바꾼 손 평가를 기본값과 미러 대국시킨다.
//! `cargo run --release --example tune -- <쌍 수> key=value [key=value ...]`
//! key: roll_bonus, contact_sigma, race_wastage, point_value_floor, hit_tempo, hit_scale, hit_board_factor,
//!      roller_blot, bar_base, stack_penalty, point_scale(포인트 가치 전체 배율), prime_scale

use palamedes::HandEval;
use palamedes::eval::HandWeights;
use palamedes::play::{OnePly, mirror_match};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let pairs: u32 = args[0].parse().expect("pairs");
    let mut w = HandWeights::default();
    for kv in &args[1..] {
        let (k, v) = kv.split_once('=').expect("key=value");
        let v: f32 = v.parse().expect("number");
        match k {
            "roll_bonus" => w.roll_bonus = v,
            "contact_sigma" => w.contact_sigma = v,
            "race_wastage" => w.race_wastage = v,
            "point_value_floor" => w.point_value_floor = v,
            "hit_tempo" => w.hit_tempo = v,
            "hit_scale" => w.hit_scale = v,
            "hit_board_factor" => w.hit_board_factor = v,
            "roller_blot" => w.roller_blot = v,
            "bar_base" => w.bar_base = v,
            "stack_penalty" => w.stack_penalty = v,
            "point_scale" => w.point_value.iter_mut().for_each(|x| *x *= v),
            "prime_scale" => w.prime_bonus.iter_mut().for_each(|x| *x *= v),
            _ => panic!("unknown key {k}"),
        }
    }
    let mut a = OnePly::new(HandEval { w }, "variant");
    let mut b = OnePly::new(HandEval::default(), "default");
    // 검증용으로 다른 주사위를 쓰려면 TUNE_SEED.
    let seed = std::env::var("TUNE_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000);
    let st = mirror_match(&mut a, &mut b, pairs, seed);
    println!(
        "{:?}: ppg {:+.3} ± {:.3}, wins {:.1}%",
        &args[1..],
        st.ppg(),
        st.ppg_stderr(),
        100.0 * st.win_rate()
    );
}
