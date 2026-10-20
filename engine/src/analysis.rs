//! 수 분석(5단계): 둔 수의 실수 등급과 설명, 힌트.
//!
//! 등급은 엔진 에퀴티 손실(최선 수 − 둔 수)로 매긴다. 임계값은 GNU 백개먼 기본값:
//! 0.04 이상 의심스러움(?!), 0.08 이상 나쁨(?), 0.16 이상 매우 나쁨(??).
//! 탐색 깊이는 `Search`로 받는다(웹은 2수). 2수일 때 둔 수는 후보 거르기와 상관없이 항상 2수로 평가해서
//! 최선 수와 같은 깊이로 비교한다.

use serde::Serialize;

use crate::board::{Board, Side};
use crate::eval::Evaluator;
use crate::explain::{ANALYSIS, Explanation, HINT, Surrogate, explain};
use crate::search::{MoveSummary, Search, rank_moves_ply};

pub const DOUBTFUL: f32 = 0.04;
pub const BAD: f32 = 0.08;
pub const VERY_BAD: f32 = 0.16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Grade {
    /// 최선 수(같은 결과 국면).
    Best,
    /// 손실 < 0.04
    Good,
    Doubtful,
    Bad,
    VeryBad,
}

impl Grade {
    pub fn from_loss(loss: f32, is_best: bool) -> Grade {
        if is_best {
            Grade::Best
        } else if loss >= VERY_BAD {
            Grade::VeryBad
        } else if loss >= BAD {
            Grade::Bad
        } else if loss >= DOUBTFUL {
            Grade::Doubtful
        } else {
            Grade::Good
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Grade::Best => "최선",
            Grade::Good => "괜찮음",
            Grade::Doubtful => "의심스러움",
            Grade::Bad => "나쁨",
            Grade::VeryBad => "매우 나쁨",
        }
    }

    /// 기보 표기용 기호.
    pub fn mark(self) -> &'static str {
        match self {
            Grade::Best | Grade::Good => "",
            Grade::Doubtful => "?!",
            Grade::Bad => "?",
            Grade::VeryBad => "??",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct MoveAnalysis {
    pub grade: Grade,
    pub label: &'static str,
    pub mark: &'static str,
    /// 최선 수 대비 에퀴티 손실(≥ 0).
    pub loss: f32,
    /// 둔 수의 순위(1부터)와 전체 후보 수.
    pub rank: usize,
    pub count: usize,
    pub played: MoveSummary,
    pub best: MoveSummary,
    /// 상위 후보(최대 5개).
    pub top: Vec<MoveSummary>,
    /// 둔 수가 최선이 아니면 설명. 최선이면 None.
    pub explanation: Option<Explanation>,
}

/// `after`는 `before`에서 `side`가 둔 수의 결과 국면. 합법수 결과가 아니면 None.
pub fn analyze_move<E: Evaluator + ?Sized>(
    e: &E,
    sur: &Surrogate,
    before: &Board,
    side: Side,
    (d1, d2): (u8, u8),
    after: &Board,
    search: Search,
) -> Option<MoveAnalysis> {
    let ranked = rank_moves_ply(&e, before, side, d1, d2, search, Some(after));
    let i = ranked.iter().position(|m| m.board == *after)?;
    let best = &ranked[0];
    let played = &ranked[i];
    let loss = (best.equity - played.equity).max(0.0);
    let grade = Grade::from_loss(loss, i == 0);
    let explanation = (i != 0).then(|| explain(side, &played.board, &best.board, sur, ANALYSIS));
    Some(MoveAnalysis {
        grade,
        label: grade.label(),
        mark: grade.mark(),
        loss,
        rank: i + 1,
        count: ranked.len(),
        played: played.into(),
        best: best.into(),
        top: ranked.iter().take(5).map(MoveSummary::from).collect(),
        explanation,
    })
}

#[derive(Clone, Debug, Serialize)]
pub struct Hint {
    pub best: MoveSummary,
    pub top: Vec<MoveSummary>,
    /// 2위 수보다 얼마나 나은가. 후보가 하나면 0.
    pub margin: f32,
    /// 추천 수가 2위 수보다 나은 이유. 후보가 하나면 None.
    pub explanation: Option<Explanation>,
}

pub fn hint<E: Evaluator + ?Sized>(
    e: &E,
    sur: &Surrogate,
    before: &Board,
    side: Side,
    (d1, d2): (u8, u8),
    search: Search,
) -> Hint {
    let ranked = rank_moves_ply(&e, before, side, d1, d2, search, None);
    let explanation =
        (ranked.len() > 1).then(|| explain(side, &ranked[1].board, &ranked[0].board, sur, HINT));
    Hint {
        best: (&ranked[0]).into(),
        top: ranked.iter().take(5).map(MoveSummary::from).collect(),
        margin: if ranked.len() > 1 {
            ranked[0].equity - ranked[1].equity
        } else {
            0.0
        },
        explanation,
    }
}
