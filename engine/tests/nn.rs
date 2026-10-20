use palamedes::board::{Board, Side};
use palamedes::eval::Evaluator;
use palamedes::legal_moves;
use palamedes::nn::{INPUTS, Net, NnEval, OUTPUTS, encode};
use palamedes::play::{OnePly, RandomPlayer, mirror_match};
use palamedes::rng::Rng;
use palamedes::train::self_play_game;

fn random_positions(n: usize, seed: u64) -> Vec<(Board, Side)> {
    let mut rng = Rng::new(seed);
    let mut out = Vec::new();
    while out.len() < n {
        let mut b = Board::start();
        let mut side = Side::White;
        while b.result().is_none() && out.len() < n {
            out.push((b, side));
            let (d1, d2) = rng.roll();
            let m = legal_moves(&b, side, d1, d2);
            b.apply_steps(side, m[rng.below(m.len() as u64) as usize].steps());
            side = side.opp();
        }
    }
    out
}

#[test]
fn encoding_is_perspective_relative() {
    for (b, s) in random_positions(300, 3) {
        let x = encode(&b, s);
        assert_eq!(x, encode(&b.swapped(), s.opp()));
        assert!(x.iter().all(|&(i, v)| (i as usize) < INPUTS && v > 0.0));
    }
    // 시작 국면: 진영마다 포인트 4개(24:2, 13:5, 8:3, 6:5).
    let x = encode(&Board::start(), Side::White);
    let first: Vec<_> = x.iter().filter(|(i, _)| (*i as usize) < 98).collect();
    // 24: n≥1,n≥2 / 13: 4칸 / 8: 3칸 / 6: 4칸 = 13개
    assert_eq!(first.len(), 13);
}

fn loss(net: &Net, x: &[(u16, f32)], t: &[f32; OUTPUTS]) -> f64 {
    let y = net.predict(x);
    (0..OUTPUTS)
        .map(|o| {
            -(t[o] as f64 * (y[o] as f64).ln() + (1.0 - t[o] as f64) * (1.0 - y[o] as f64).ln())
        })
        .sum::<f64>()
        / OUTPUTS as f64
}

#[test]
fn gradient_matches_finite_difference() {
    let net = Net::new(6, 11);
    let x = encode(&random_positions(40, 4)[37].0, Side::White);
    let t = [0.7, 0.2, 0.05, 0.1, 0.01];
    let lr = 1e-3f32;
    let mut stepped = net.clone();
    let mut h = Vec::new();
    stepped.train_step(&x, &t, lr, &mut h);
    // train_step은 출력 평균이 아니라 합의 기울기를 쓴다(평균 손실 × OUTPUTS).
    let check = |get: &dyn Fn(&Net) -> f32, set: &dyn Fn(&mut Net, f32)| {
        let analytic = (get(&net) - get(&stepped)) as f64 / lr as f64;
        let eps = 1e-3f32;
        let (mut p, mut m) = (net.clone(), net.clone());
        set(&mut p, get(&net) + eps);
        set(&mut m, get(&net) - eps);
        let numeric = (loss(&p, &x, &t) - loss(&m, &x, &t)) / (2.0 * eps as f64) * OUTPUTS as f64;
        assert!(
            (analytic - numeric).abs() < 1e-3 + 0.02 * numeric.abs(),
            "analytic {analytic} numeric {numeric}"
        );
    };
    let i = x[2].0 as usize;
    for j in 0..3 {
        check(&|n: &Net| n.w1[i * 6 + j], &|n: &mut Net, v| {
            n.w1[i * 6 + j] = v
        });
        check(&|n: &Net| n.w2[j * OUTPUTS + 1], &|n: &mut Net, v| {
            n.w2[j * OUTPUTS + 1] = v
        });
        check(&|n: &Net| n.b1[j], &|n: &mut Net, v| n.b1[j] = v);
    }
    check(&|n: &Net| n.b2[0], &|n: &mut Net, v| n.b2[0] = v);
}

#[test]
fn save_load_roundtrip() {
    let net = Net::new(16, 5);
    let mut buf = Vec::new();
    net.write_to(&mut buf).unwrap();
    assert_eq!(Net::from_bytes(&buf).unwrap(), net);
    assert!(Net::from_bytes(&buf[..20]).is_err());
    assert!(Net::from_bytes(b"XXXX0000000000000").is_err());
}

#[test]
fn training_reduces_loss_on_fixed_set() {
    let mut net = Net::new(32, 6);
    let pos = random_positions(64, 8);
    let data: Vec<_> = pos
        .iter()
        .map(|(b, s)| {
            let p = b.pips(*s) as f32 / (b.pips(*s) + b.pips(s.opp())) as f32;
            (
                encode(b, *s),
                [1.0 - p, 0.3 * (1.0 - p), 0.02, 0.3 * p, 0.02],
            )
        })
        .collect();
    let total = |n: &Net| data.iter().map(|(x, t)| loss(n, x, t)).sum::<f64>();
    let before = total(&net);
    let mut h = Vec::new();
    for _ in 0..200 {
        for (x, t) in &data {
            net.train_step(x, t, 0.05, &mut h);
        }
    }
    assert!(total(&net) < before * 0.9, "{} -> {}", before, total(&net));
}

#[test]
fn lambda_return_targets() {
    let ev = NnEval::new(Net::new(16, 2));
    let samples = self_play_game(&ev, 77, 0.7, 0.0);
    assert!(samples.len() > 10);
    for s in &samples {
        assert!(
            s.target.iter().all(|v| (0.0..=1.0).contains(v)),
            "{:?}",
            s.target
        );
        assert!(s.target[2] <= s.target[1] + 1e-6 && s.target[1] <= s.target[0] + 1e-6);
    }
    // 마지막 수를 둔 쪽이 이겼다: 승리 1, 진 쪽 개먼 여부에 따라 0/1.
    let last = samples.last().unwrap().target;
    assert_eq!(last[0], 1.0);
    assert!(last[1] == 0.0 || last[1] == 1.0);
    assert_eq!(last[3], 0.0);
    // λ = 1이면 모든 표본의 목표가 실제 결과(굴릴 쪽 관점으로 번갈아 뒤집힘).
    let s1 = self_play_game(&ev, 77, 1.0, 0.0);
    for s in &s1 {
        assert!(
            s.target.iter().all(|&v| v == 0.0 || v == 1.0),
            "{:?}",
            s.target
        );
    }
    // 같은 시드는 같은 판.
    assert_eq!(self_play_game(&ev, 77, 0.7, 0.0).len(), samples.len());
}

#[test]
fn embedded_weights_play_well() {
    let ev = NnEval::embedded();
    for (b, s) in random_positions(200, 9) {
        let p = ev.evaluate(&b, s);
        assert!(p.win.is_finite() && (0.0..=1.0).contains(&p.win));
        assert_eq!(p, ev.evaluate(&b.swapped(), s.opp()));
    }
    let pairs = if cfg!(debug_assertions) { 10 } else { 100 };
    let mut a = OnePly::new(ev, "nn");
    let mut r = RandomPlayer::new(1);
    let st = mirror_match(&mut a, &mut r, pairs, 5);
    assert!(st.win_rate() > 0.9, "{}", st.win_rate());
}
