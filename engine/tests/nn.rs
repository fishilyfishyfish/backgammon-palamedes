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

#[test]
fn extended_encoding() {
    use palamedes::nn::{INPUTS_EXT, encode_ext};
    for (b, s) in random_positions(300, 8) {
        let x = encode_ext(&b, s);
        assert_eq!(x, encode_ext(&b.swapped(), s.opp()));
        assert!(
            x.iter()
                .all(|&(i, v)| (i as usize) < INPUTS_EXT && v > 0.0 && v.is_finite())
        );
        assert_eq!(&x[..encode(&b, s).len()], &encode(&b, s)[..]);
    }
    let get = |x: &[(u16, f32)], k: usize| {
        x.iter()
            .find(|(i, _)| *i as usize == INPUTS + k)
            .map_or(0.0, |e| e.1)
    };
    // 시작 국면: 핍 167, 24의 말은 12·17·19의 상대 포인트를 못 지나감(탈출 0), 블롯 없음, 최장 연속 1칸, 홈 포인트 1개.
    let x = encode_ext(&Board::start(), Side::White);
    assert!((get(&x, 0) - 1.67).abs() < 1e-6);
    assert_eq!(get(&x, 1), 0.0);
    assert_eq!(get(&x, 2), 0.0);
    assert_eq!(get(&x, 3), 0.0);
    assert!((get(&x, 4) - 1.0 / 6.0).abs() < 1e-6);
    assert!((get(&x, 5) - 1.0 / 6.0).abs() < 1e-6);
    // 블롯 하나(백 20)와 6칸 뒤 상대 말(백 기준 14): 6을 만드는 굴림 17개. 백 말이 앞을 막는 상대 포인트가 없으면 탈출 36.
    let mut b = Board::empty();
    b.pos[0][20] = 1;
    b.pos[0][0] = 14;
    b.pos[1][25 - 14] = 1;
    b.pos[1][0] = 14;
    let x = encode_ext(&b, Side::White);
    assert!((get(&x, 2) - 17.0 / 36.0).abs() < 1e-6);
    assert_eq!(get(&x, 1), 1.0);
}

#[test]
fn extended_net_roundtrip() {
    use palamedes::nn::INPUTS_EXT;
    let net = Net::with_inputs(INPUTS_EXT, 8, 3);
    let mut buf = Vec::new();
    net.write_to(&mut buf).unwrap();
    let back = Net::from_bytes(&buf).unwrap();
    assert_eq!(back, net);
    let ev = NnEval::new(back);
    let p = ev.evaluate(&Board::start(), Side::White);
    assert!(p.win > 0.0 && p.win < 1.0);
}

#[test]
fn extended_encoding_2() {
    use palamedes::nn::{INPUTS_EXT2, encode_ext2};
    for (b, s) in random_positions(300, 9) {
        let x = encode_ext2(&b, s);
        assert_eq!(x, encode_ext2(&b.swapped(), s.opp()));
        assert!(
            x.iter()
                .all(|&(i, v)| (i as usize) < INPUTS_EXT2 && v > 0.0 && v.is_finite() && v < 5.0)
        );
    }
    let get = |x: &[(u16, f32)], k: usize| {
        x.iter()
            .find(|(i, _)| *i as usize == INPUTS + k)
            .map_or(0.0, |e| e.1)
    };
    // 블롯 하나(백 20)와 6칸 뒤 상대 말(백 기준 14): 막힌 칸이 없으면 6을 만드는 굴림 17개, 잃는 핍 5.
    let mut b = Board::empty();
    b.pos[0][20] = 1;
    b.pos[0][0] = 14;
    b.pos[1][25 - 14] = 1;
    b.pos[1][0] = 14;
    let x = encode_ext2(&b, Side::White);
    assert!((get(&x, 2) - 17.0 / 36.0).abs() < 1e-6);
    assert_eq!(get(&x, 3), 0.0);
    assert!((get(&x, 4) - 17.0 * 5.0 / 360.0).abs() < 1e-6);
    // 백 17·18에 포인트를 만들면 15(=14+1), 16 경유 조합 일부가 막힌다: 3-3(17 착지)·2-2(16, 18)·4-2/2-4 중 16 경유만 남음.
    b.pos[0][0] = 10;
    b.pos[0][17] = 2;
    b.pos[0][18] = 2;
    let x = encode_ext2(&b, Side::White);
    // 6이 하나라도(11) + 5-1(15 또는 19 경유, 19 안 막힘: 2) + 4-2(16 또는 18 경유, 16 안 막힘: 2) + 2-2(16·18 막힘: 0) + 3-3(17 막힘: 0) = 15
    assert!((get(&x, 2) - 15.0 / 36.0).abs() < 1e-6);
}
