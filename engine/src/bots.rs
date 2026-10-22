//! 레이팅별 AI 상대(봇). 봇마다 이름·레이팅·성격(습관)이 있고, 낮은 레이팅일수록 사람처럼 실수한다.
//!
//! 사람다운 실수 = 두 가지를 섞는다.
//! 1. **잡음**: 후보마다 에퀴티에 정규분포 잡음을 더하고 가장 큰 수를 둔다(gnubg의 "noise" 방식).
//!    비슷한 수끼리는 자주 바뀌고 크게 나쁜 수는 드물다. 후보가 많고 엇비슷한 복잡한 국면일수록 실수가 잦다.
//! 2. **습관**: 사람이 흔히 보이는 경향을 특징값(`explain::surrogate_inputs`)에 준 가산점으로 넣는다(`HABITS`).
//!    봇마다 습관 배율이 달라서 같은 레이팅대라도 두는 스타일이 다르다(맞히기 좋아함, 겁 많음, 달리기 좋아함 등).
//!
//! 봇 목록과 잡음 크기는 `weights/bots.json`에 있다. 잡음은 `examples/calibrate_bots.rs`가 미러 대국으로 맞춘다:
//! 기준 상대(신경망 1수)와 둔 승률이 레이팅 차이에 맞는 FIBS 공식 승률이 되도록.
//! 레이팅 눈금은 엔진 2수를 1800으로 둔 상대값이다(사람 상대 기준점은 없다). 최강 봇(엔진 3수)은 2수와의 대국으로 1890.
//!
//! 큐브도 같은 식으로 판단 값에 잡음을 더한다. 잡음은 호출자가 준 시드로 만든다(웹은 브라우저 난수).

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::board::{Board, Side};
use crate::cube::CubeDecision;
use crate::eval::{Evaluator, is_race};
use crate::explain::{K, NAMES, surrogate_inputs};
use crate::features::compute_features;
use crate::matchplay::MatchCtx;
use crate::rng::Rng;
use crate::search::{Objective, ScoredMove, Search, rank_moves_ply};

/// 습관: (대리 모델 입력 이름, 배율 1일 때 단위당 에퀴티 가산점, 설명).
pub const HABITS: [(&str, f32, &str); 6] = [
    // 상대를 바로 보낸 말 하나당.
    ("opp_on_bar", 0.05, "맞힐 수 있으면 맞힌다"),
    // 맞으면 잃을 핍(10핍 단위). 신경망 대리 모델 가중치(약 −0.19)의 절반쯤. 음수 배율이면 더 무서워한다.
    ("my_hit_loss", 0.09, "맞을 위험을 덜 신경 쓴다"),
    ("my_stacks", 0.02, "한 포인트에 쌓는다"),
    ("back_checkers", -0.02, "뒷말을 일찍 뺀다"),
    ("bar_point", -0.02, "바 포인트를 가볍게 본다"),
    ("prime_len", -0.012, "프라임을 가볍게 본다"),
];

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Style {
    /// 탐색 깊이(1 또는 2).
    pub ply: u8,
    /// 후보 에퀴티에 더하는 잡음의 표준편차.
    pub noise: f32,
    /// `HABITS` 순서의 배율(0이면 그 습관 없음).
    pub habits: [f32; 6],
    /// 큐브 판단 값에 더하는 잡음의 표준편차.
    pub cube_noise: f32,
}

impl Style {
    /// 엔진 그대로(신경망 3수).
    pub const ENGINE: Style = Style {
        ply: 3,
        noise: 0.0,
        habits: [0.0; 6],
        cube_noise: 0.0,
    };

    pub fn has_habits(&self) -> bool {
        self.habits.iter().any(|&h| h != 0.0)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bot {
    pub id: String,
    pub name: String,
    /// 목표 레이팅(화면 표시).
    pub rating: u32,
    /// 한 줄 소개.
    pub blurb: String,
    pub style: Style,
    /// 보정 대국으로 잰 레이팅과 표준오차(`calibrate_bots`).
    #[serde(default)]
    pub measured: Option<[f32; 2]>,
}

/// 저장소에 포함된 봇 목록(레이팅 오름차순).
pub fn bots() -> &'static [Bot] {
    static B: OnceLock<Vec<Bot>> = OnceLock::new();
    B.get_or_init(|| serde_json::from_str(include_str!("../weights/bots.json")).expect("bots.json"))
}

/// 번호(0부터)로 봇. 범위 밖이면 가장 가까운 봇.
pub fn bot(i: usize) -> &'static Bot {
    let b = bots();
    &b[i.min(b.len() - 1)]
}

/// FIBS 레이팅 공식: 레이팅이 `diff`만큼 높은 쪽이 `n`점 매치를 이길 확률.
pub fn win_prob(diff: f32, n: f32) -> f32 {
    1.0 / (1.0 + 10f32.powf(-diff * n.sqrt() / 2000.0))
}

/// `win_prob`의 역: 승률 p에서 레이팅 차이.
pub fn rating_diff(p: f32, n: f32) -> f32 {
    2000.0 * (p / (1.0 - p)).log10() / n.sqrt()
}

fn habit_bonus(x: &[f32; K], habits: &[f32; 6]) -> f32 {
    HABITS
        .iter()
        .zip(habits)
        .filter(|(_, m)| **m != 0.0)
        .map(|((name, w, _), m)| {
            let k = NAMES.iter().position(|n| n == name).expect("habit feature");
            w * m * x[k]
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

/// 스타일대로 수를 고른다. 돌려주는 `equity`는 엔진 값(잡음·습관 전)이다.
#[allow(clippy::too_many_arguments)]
pub fn choose_move<E: Evaluator + Clone>(
    e: &E,
    b: &Board,
    side: Side,
    d1: u8,
    d2: u8,
    st: &Style,
    ctx: Option<MatchCtx>,
    rng: &mut Rng,
) -> ScoredMove {
    let obj = ctx.map(Objective::Match).unwrap_or(Objective::Money);
    let search = Search::from_ply(st.ply).with_objective(obj);
    let mut ranked = rank_moves_ply(e, b, side, d1, d2, search, None);
    let habits = st.has_habits();
    if ranked.len() == 1 || (st.noise == 0.0 && !habits) {
        return ranked.swap_remove(0);
    }
    let mut best = (f32::NEG_INFINITY, 0);
    for (i, m) in ranked.iter().enumerate() {
        let mut v = m.equity + st.noise * gauss(rng);
        if habits && i < HABIT_CANDIDATES {
            let x = surrogate_inputs(&compute_features(&m.board), side, is_race(&m.board));
            v += habit_bonus(&x, &st.habits);
        }
        if v > best.0 {
            best = (v, i);
        }
    }
    ranked.swap_remove(best.1)
}

/// 스타일대로 큐브 판단. `d`는 더블하는 쪽 기준 판단(`cube::decide`). (더블할지, 상대가 받을지)를 돌려준다.
/// 더블: min(더블/테이크, 더블/패스) + 잡음 > 노 더블. 받기: 더블/테이크 + 잡음 ≤ 더블/패스(더블한 쪽 기준이라 작을수록 받는 쪽에 좋음).
pub fn cube_choice(d: &CubeDecision, st: &Style, rng: &mut Rng) -> (bool, bool) {
    if st.cube_noise == 0.0 {
        return (d.double, d.take);
    }
    let double = d.double_take.min(d.double_pass) + st.cube_noise * gauss(rng) > d.no_double;
    let take = d.double_take + st.cube_noise * gauss(rng) <= d.double_pass;
    (double, take)
}

/// 미러 대국용 플레이어.
pub struct BotPlayer<E: Evaluator + Clone> {
    pub eval: E,
    pub style: Style,
    pub label: String,
    pub rng: Rng,
}

impl<E: Evaluator + Clone> crate::play::Player for BotPlayer<E> {
    fn name(&self) -> String {
        self.label.clone()
    }
    fn choose(&mut self, b: &Board, side: Side, d1: u8, d2: u8) -> crate::movegen::Move {
        choose_move(
            &self.eval,
            b,
            side,
            d1,
            d2,
            &self.style,
            None,
            &mut self.rng,
        )
        .mv
    }
}
