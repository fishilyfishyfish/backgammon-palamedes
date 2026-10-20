//! TD(λ) 자기대국 학습.
//!
//! 한 판을 두는 동안 각 차례 t에서 (굴릴 쪽 기준 입력 x_t, 고른 수의 1수 평가 e_t)를 모은다.
//! e_t는 "수를 둔 뒤 국면을 상대 차례로 평가해 뒤집은 값"이라 V(s_{t+1})을 t 쪽에서 본 값과 같다.
//! 판이 끝나면 뒤에서부터 λ-리턴을 만든다: G_t = (1−λ)·e_t + λ·inv(G_{t+1}), 마지막 수의 e는 실제 결과.
//! 그 뒤 (x_t, G_t)로 교차 엔트로피 SGD를 한다(오프라인 λ-리턴 = 전방 관점 TD(λ)).

use crate::board::{Board, Side};
use crate::eval::{Evaluator, OutcomeProbs};
use crate::movegen::legal_moves_with_boards;
use crate::nn::{NnEval, OUTPUTS, Sparse, from_probs};
use crate::rng::Rng;
use crate::search::evaluate_after;

pub struct Sample {
    pub x: Sparse,
    pub target: [f32; OUTPUTS],
}

/// 자기대국 한 판. 양쪽 모두 `eval`로 1수 탐색. `explore` 확률로 상위 2수 중 2위를 둔다.
pub fn self_play_game(eval: &NnEval, seed: u64, lambda: f32, explore: f32) -> Vec<Sample> {
    let mut dice = Rng::new(seed);
    let mut coin = Rng::new(seed ^ 0x5DEE_CE66_D1CE_5EED);
    let (mut d1, mut d2) = loop {
        let (w, b) = dice.roll();
        if w != b {
            break (w, b);
        }
    };
    let mut side = if d1 > d2 { Side::White } else { Side::Black };
    let mut b = Board::start();
    let mut xs: Vec<Sparse> = Vec::with_capacity(80);
    let mut es: Vec<OutcomeProbs> = Vec::with_capacity(80);
    loop {
        xs.push(eval.net.encode(&b, side));
        let mut cands: Vec<(Board, OutcomeProbs, f32)> = legal_moves_with_boards(&b, side, d1, d2)
            .into_iter()
            .map(|(_, nb)| {
                let p = evaluate_after(eval, &nb, side);
                (nb, p, p.equity())
            })
            .collect();
        cands.sort_by(|a, c| c.2.total_cmp(&a.2));
        let pick =
            if cands.len() > 1 && explore > 0.0 && (coin.below(1_000_000) as f32) < explore * 1e6 {
                1
            } else {
                0
            };
        let (nb, p, _) = cands.swap_remove(pick);
        es.push(p);
        b = nb;
        if b.result().is_some() {
            break;
        }
        side = side.opp();
        (d1, d2) = dice.roll();
    }
    // λ-리턴. 마지막 e는 종국이라 evaluate_after가 이미 실제 결과를 준다.
    let mut targets = vec![OutcomeProbs::default(); es.len()];
    let last = es.len() - 1;
    targets[last] = es[last];
    for t in (0..last).rev() {
        targets[t] = mix(&es[t], &targets[t + 1].invert(), lambda);
    }
    xs.into_iter()
        .zip(targets)
        .map(|(x, g)| Sample {
            x,
            target: from_probs(&g),
        })
        .collect()
}

/// (1−λ)·a + λ·b (확률 벡터 성분별).
fn mix(a: &OutcomeProbs, b: &OutcomeProbs, lambda: f32) -> OutcomeProbs {
    let f = |x: f32, y: f32| (1.0 - lambda) * x + lambda * y;
    OutcomeProbs {
        win: f(a.win, b.win),
        win_gammon: f(a.win_gammon, b.win_gammon),
        win_backgammon: f(a.win_backgammon, b.win_backgammon),
        lose_gammon: f(a.lose_gammon, b.lose_gammon),
        lose_backgammon: f(a.lose_backgammon, b.lose_backgammon),
    }
}

#[derive(Clone, Debug)]
pub struct TrainConfig {
    /// 입력 수(`nn::INPUTS` 또는 손 특징을 더한 `nn::INPUTS_EXT`). 새로 만들 때만 쓴다.
    pub inputs: usize,
    pub hidden: usize,
    pub games: u64,
    pub lambda: f32,
    /// 학습률: 처음 lr_start에서 끝 lr_end까지 선형 감소.
    pub lr_start: f32,
    pub lr_end: f32,
    pub explore: f32,
    pub threads: usize,
    /// 한 라운드에 스레드당 두는 판 수. 라운드 동안 가중치는 고정.
    pub games_per_thread: u64,
    pub seed: u64,
}

impl Default for TrainConfig {
    fn default() -> Self {
        TrainConfig {
            inputs: crate::nn::INPUTS,
            hidden: 128,
            games: 100_000,
            lambda: 0.7,
            lr_start: 0.02,
            lr_end: 0.004,
            explore: 0.0,
            threads: 4,
            games_per_thread: 4,
            seed: 1,
        }
    }
}

/// 한 라운드: 현재 가중치로 여러 판을 병렬로 두고, 모은 표본으로 순서대로 SGD.
/// 반환: (둔 판 수, 표본 수, 평균 손실).
pub fn train_round(eval: &mut NnEval, cfg: &TrainConfig, game0: u64, lr: f32) -> (u64, usize, f32) {
    let per = cfg.games_per_thread;
    let samples: Vec<Vec<Sample>> = std::thread::scope(|sc| {
        let handles: Vec<_> = (0..cfg.threads as u64)
            .map(|t| {
                let ev = eval.clone();
                sc.spawn(move || {
                    let mut out = Vec::new();
                    for g in 0..per {
                        let id = game0 + t * per + g;
                        out.extend(self_play_game(
                            &ev,
                            cfg.seed.wrapping_mul(1_000_003).wrapping_add(id),
                            cfg.lambda,
                            cfg.explore,
                        ));
                    }
                    out
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("self-play thread"))
            .collect()
    });
    let net = std::sync::Arc::make_mut(&mut eval.net);
    let mut h = Vec::with_capacity(net.hidden);
    let (mut n, mut loss) = (0usize, 0.0f64);
    for s in samples.iter().flatten() {
        loss += net.train_step(&s.x, &s.target, lr, &mut h) as f64;
        n += 1;
    }
    (cfg.threads as u64 * per, n, (loss / n.max(1) as f64) as f32)
}

/// 학습률 일정.
pub fn lr_at(cfg: &TrainConfig, done: u64) -> f32 {
    let f = (done as f32 / cfg.games.max(1) as f32).min(1.0);
    cfg.lr_start + (cfg.lr_end - cfg.lr_start) * f
}

/// 평가기 하나로 바꾼 확률이 유효한지(학습 중 NaN 감시용).
pub fn probs_ok<E: Evaluator>(e: &E) -> bool {
    let p = e.evaluate(&Board::start(), Side::White);
    [
        p.win,
        p.win_gammon,
        p.win_backgammon,
        p.lose_gammon,
        p.lose_backgammon,
    ]
    .iter()
    .all(|v| v.is_finite())
}
