//! 대국 난이도(1..=5). 5는 엔진 그대로(신경망 2수), 낮을수록 사람처럼 실수한다.
//!
//! 사람다운 실수 = 두 가지를 섞는다.
//! 1. **잡음**: 후보마다 에퀴티에 정규분포 잡음을 더하고 가장 큰 수를 둔다(gnubg의 "noise" 방식).
//!    비슷한 수끼리는 자주 바뀌고 크게 나쁜 수는 드물다. 후보가 많고 엇비슷한 복잡한 국면일수록 실수가 잦다.
//! 2. **습관**: 초보가 흔히 보이는 경향을 특징값(`explain::surrogate_inputs`)에 준 가산점으로 넣는다.
//!    맞힐 수 있으면 맞히고, 맞을 위험을 덜 신경 쓰고, 한 포인트에 쌓고, 뒷말을 일찍 빼고, 바 포인트·프라임 가치를 낮게 본다.
//!
//! 큐브도 같은 식으로 판단 값에 잡음을 더한다. 잡음은 호출자가 준 시드로 만든다(웹은 브라우저 난수).

use serde::Serialize;

use crate::board::{Board, Side};
use crate::cube::CubeDecision;
use crate::eval::{Evaluator, is_race};
use crate::explain::{K, NAMES, surrogate_inputs};
use crate::features::compute_features;
use crate::matchplay::MatchCtx;
use crate::rng::Rng;
use crate::search::{Objective, ScoredMove, Search, rank_moves_ply};

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Level {
    pub level: u8,
    /// 탐색 깊이(1 또는 2).
    pub ply: u8,
    /// 후보 에퀴티에 더하는 잡음의 표준편차.
    pub noise: f32,
    /// 습관 가산점의 배율(0이면 없음).
    pub habit: f32,
    /// 큐브 판단 값에 더하는 잡음의 표준편차.
    pub cube_noise: f32,
}

pub const LEVELS: [Level; 5] = [
    Level {
        level: 1,
        ply: 1,
        noise: 0.08,
        habit: 0.9,
        cube_noise: 0.25,
    },
    Level {
        level: 2,
        ply: 1,
        noise: 0.06,
        habit: 0.6,
        cube_noise: 0.15,
    },
    Level {
        level: 3,
        ply: 1,
        noise: 0.04,
        habit: 0.4,
        cube_noise: 0.08,
    },
    Level {
        level: 4,
        ply: 1,
        noise: 0.02,
        habit: 0.15,
        cube_noise: 0.03,
    },
    Level {
        level: 5,
        ply: 2,
        noise: 0.0,
        habit: 0.0,
        cube_noise: 0.0,
    },
];

/// 1..=5 밖이면 가장 가까운 값.
pub fn level(n: u8) -> Level {
    LEVELS[(n.clamp(1, 5) - 1) as usize]
}

/// 초보 습관: (대리 모델 입력 이름, 단위당 에퀴티 가산점). 배율 1일 때 값.
const HABITS: [(&str, f32); 6] = [
    // 맞힐 수 있으면 맞힌다(상대를 바로 보낸 말 하나당).
    ("opp_on_bar", 0.05),
    // 맞으면 잃을 핍(10핍 단위)을 덜 무서워한다. 신경망 대리 모델 가중치(약 −0.19)의 절반쯤을 되돌린다.
    ("my_hit_loss", 0.09),
    // 한 포인트에 쌓는 걸 신경 쓰지 않는다.
    ("my_stacks", 0.02),
    // 뒷말을 일찍 뺀다.
    ("back_checkers", -0.02),
    // 바 포인트와 프라임의 가치를 낮게 본다.
    ("bar_point", -0.02),
    ("prime_len", -0.012),
];

fn habit_bonus(x: &[f32; K]) -> f32 {
    HABITS
        .iter()
        .map(|(name, w)| {
            let k = NAMES.iter().position(|n| n == name).expect("habit feature");
            w * x[k]
        })
        .sum()
}

/// 표준 정규분포(Box–Muller).
fn gauss(rng: &mut Rng) -> f32 {
    let u = |rng: &mut Rng| ((rng.next_u64() >> 11) as f64 + 0.5) / (1u64 << 53) as f64;
    let (a, b) = (u(rng), u(rng));
    ((-2.0 * a.ln()).sqrt() * (2.0 * std::f64::consts::PI * b).cos()) as f32
}

/// 습관을 따질 후보 수(에퀴티 상위). 그 아래 수는 잡음만 더한다(특징값 계산이 비싸서).
const HABIT_CANDIDATES: usize = 12;

/// 난이도대로 수를 고른다. 돌려주는 `equity`는 엔진 값(잡음·습관 전)이다.
#[allow(clippy::too_many_arguments)]
pub fn choose_move<E: Evaluator + Clone>(
    e: &E,
    b: &Board,
    side: Side,
    d1: u8,
    d2: u8,
    lv: &Level,
    ctx: Option<MatchCtx>,
    rng: &mut Rng,
) -> ScoredMove {
    let obj = ctx.map(Objective::Match).unwrap_or(Objective::Money);
    let search = Search::from_ply(lv.ply).with_objective(obj);
    let mut ranked = rank_moves_ply(e, b, side, d1, d2, search, None);
    if ranked.len() == 1 || (lv.noise == 0.0 && lv.habit == 0.0) {
        return ranked.swap_remove(0);
    }
    let mut best = (f32::NEG_INFINITY, 0);
    for (i, m) in ranked.iter().enumerate() {
        let mut v = m.equity + lv.noise * gauss(rng);
        if lv.habit > 0.0 && i < HABIT_CANDIDATES {
            let x = surrogate_inputs(&compute_features(&m.board), side, is_race(&m.board));
            v += lv.habit * habit_bonus(&x);
        }
        if v > best.0 {
            best = (v, i);
        }
    }
    ranked.swap_remove(best.1)
}

/// 난이도대로 큐브 판단. `d`는 더블하는 쪽 기준 판단(`cube::decide`). (더블할지, 상대가 받을지)를 돌려준다.
/// 더블: min(더블/테이크, 더블/패스) + 잡음 > 노 더블. 받기: 더블/테이크 + 잡음 ≤ 더블/패스(더블한 쪽 기준이라 작을수록 받는 쪽에 좋음).
pub fn cube_choice(d: &CubeDecision, lv: &Level, rng: &mut Rng) -> (bool, bool) {
    if lv.cube_noise == 0.0 {
        return (d.double, d.take);
    }
    let double = d.double_take.min(d.double_pass) + lv.cube_noise * gauss(rng) > d.no_double;
    let take = d.double_take + lv.cube_noise * gauss(rng) <= d.double_pass;
    (double, take)
}

/// 미러 대국용 플레이어.
pub struct LevelPlayer<E: Evaluator + Clone> {
    pub eval: E,
    pub level: Level,
    pub rng: Rng,
}

impl<E: Evaluator + Clone> crate::play::Player for LevelPlayer<E> {
    fn name(&self) -> String {
        format!("lv{}", self.level.level)
    }
    fn choose(&mut self, b: &Board, side: Side, d1: u8, d2: u8) -> crate::movegen::Move {
        choose_move(
            &self.eval,
            b,
            side,
            d1,
            d2,
            &self.level,
            None,
            &mut self.rng,
        )
        .mv
    }
}
