//! 탐색.
//! - 1수(1-ply): 합법수마다 결과 국면을 평가해서 에퀴티가 가장 높은 수를 고른다(gnubg 용어로 0-ply).
//! - 2수(2-ply): 결과 국면을 `TwoPly`로 평가한다. 상대의 21가지 주사위마다 상대가 1수 기준 최선 응수를 둔다고 보고
//!   그 결과를 주사위 확률로 평균(gnubg 용어로 1-ply). 비용을 줄이려고 1수 순위 상위 몇 개만 2수로 다시 평가한다(`Filter`).

use serde::Serialize;

use crate::board::{Board, Side};
use crate::eval::{Evaluator, OutcomeProbs};
use crate::matchplay::MatchCtx;
use crate::movegen::{Move, legal_moves_with_boards};

#[derive(Clone, Debug)]
pub struct ScoredMove {
    pub mv: Move,
    pub board: Board,
    /// 수를 둔 쪽 기준 결과 확률.
    pub probs: OutcomeProbs,
    pub equity: f32,
    /// 이 값을 낸 탐색 깊이(1 또는 2).
    pub ply: u8,
}

/// 수를 둔 뒤 국면을 상대 차례로 평가하고 뒤집는다.
pub fn evaluate_after<E: Evaluator + ?Sized>(e: &E, after: &Board, mover: Side) -> OutcomeProbs {
    e.evaluate(after, mover.opp()).invert()
}

/// 모든 합법수를 에퀴티 내림차순으로. 같은 에퀴티면 생성 순서 유지.
pub fn rank_moves<E: Evaluator + ?Sized>(
    e: &E,
    b: &Board,
    side: Side,
    d1: u8,
    d2: u8,
) -> Vec<ScoredMove> {
    let mut v: Vec<ScoredMove> = legal_moves_with_boards(b, side, d1, d2)
        .into_iter()
        .map(|(mv, board)| {
            let probs = evaluate_after(e, &board, side);
            ScoredMove {
                mv,
                board,
                equity: probs.equity(),
                probs,
                ply: 1,
            }
        })
        .collect();
    v.sort_by(|a, b| b.equity.total_cmp(&a.equity));
    v
}

pub fn best_move<E: Evaluator + ?Sized>(
    e: &E,
    b: &Board,
    side: Side,
    d1: u8,
    d2: u8,
) -> ScoredMove {
    rank_moves(e, b, side, d1, d2).swap_remove(0)
}

/// 2수 평가기: `on_roll`의 21가지 주사위마다 `inner` 1수 기준 최선 수를 두고, 그 결과(`on_roll` 기준)를 평균한다.
#[derive(Clone, Debug)]
pub struct TwoPly<E> {
    pub inner: E,
    /// 진영별로 최선 응수를 고르는 기준([White, Black]). 머니면 둘 다 `Objective::Money`.
    pub objectives: [Objective; 2],
}

impl<E> TwoPly<E> {
    pub fn new(inner: E) -> TwoPly<E> {
        TwoPly {
            inner,
            objectives: [Objective::Money; 2],
        }
    }

    /// `side`의 목표가 `obj`일 때(상대 목표는 뒤집어서).
    pub fn with_objective(inner: E, side: Side, obj: Objective) -> TwoPly<E> {
        let mut objectives = [obj.swapped(); 2];
        objectives[side.idx()] = obj;
        TwoPly { inner, objectives }
    }
}

/// 수를 고르는 기준. 머니: 큐브 없는 에퀴티. 매치: 죽은 큐브 매치 승률을 정규화 에퀴티(EMG)로 바꾼 값.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Objective {
    Money,
    Match(MatchCtx),
}

impl Objective {
    pub fn value(&self, p: &OutcomeProbs) -> f32 {
        match self {
            Objective::Money => p.equity(),
            Objective::Match(c) => c.emg(c.mwc_dead(p)),
        }
    }

    /// 상대 관점.
    pub fn swapped(&self) -> Objective {
        match self {
            Objective::Money => Objective::Money,
            Objective::Match(c) => Objective::Match(c.swapped()),
        }
    }
}

/// 21가지 주사위와 36분의 가중치.
pub fn rolls() -> impl Iterator<Item = (u8, u8, f32)> {
    (1..=6u8)
        .flat_map(|a| (a..=6u8).map(move |b| (a, b, if a == b { 1.0 / 36.0 } else { 2.0 / 36.0 })))
}

impl<E: Evaluator> Evaluator for TwoPly<E> {
    fn evaluate(&self, b: &Board, on_roll: Side) -> OutcomeProbs {
        if let Some((w, pts)) = b.result() {
            return OutcomeProbs::terminal(w, pts, on_roll);
        }
        let mut acc = OutcomeProbs::default();
        for (d1, d2, w) in rolls() {
            let best = legal_moves_with_boards(b, on_roll, d1, d2)
                .into_iter()
                .map(|(_, nb)| evaluate_after(&self.inner, &nb, on_roll))
                .max_by(|x, y| {
                    let o = &self.objectives[on_roll.idx()];
                    o.value(x).total_cmp(&o.value(y))
                })
                .expect("at least the empty move");
            acc.win += w * best.win;
            acc.win_gammon += w * best.win_gammon;
            acc.win_backgammon += w * best.win_backgammon;
            acc.lose_gammon += w * best.lose_gammon;
            acc.lose_backgammon += w * best.lose_backgammon;
        }
        acc.clamped()
    }
}

/// 3수 평가기: `on_roll`의 21가지 주사위마다 후보를 1수로 순위 매겨 상위 `keep`개만 남기고(가지치기),
/// 그것들을 2수로 평가해 최선을 고른 뒤 그 결과를 평균한다. 한 국면에 신경망 평가 약 2만 5천 번.
#[derive(Clone, Debug)]
pub struct ThreePly<E> {
    pub inner: E,
    pub objectives: [Objective; 2],
    pub keep: usize,
}

impl<E> ThreePly<E> {
    pub fn with_objective(inner: E, side: Side, obj: Objective, keep: usize) -> ThreePly<E> {
        let mut objectives = [obj.swapped(); 2];
        objectives[side.idx()] = obj;
        ThreePly {
            inner,
            objectives,
            keep,
        }
    }
}

impl<E: Evaluator + Clone> Evaluator for ThreePly<E> {
    fn evaluate(&self, b: &Board, on_roll: Side) -> OutcomeProbs {
        if let Some((w, pts)) = b.result() {
            return OutcomeProbs::terminal(w, pts, on_roll);
        }
        let o = &self.objectives[on_roll.idx()];
        let two = TwoPly {
            inner: self.inner.clone(),
            objectives: self.objectives,
        };
        let mut acc = OutcomeProbs::default();
        for (d1, d2, w) in rolls() {
            let mut cands: Vec<(Board, f32)> = legal_moves_with_boards(b, on_roll, d1, d2)
                .into_iter()
                .map(|(_, nb)| {
                    let v = o.value(&evaluate_after(&self.inner, &nb, on_roll));
                    (nb, v)
                })
                .collect();
            cands.sort_by(|x, y| y.1.total_cmp(&x.1));
            let best = cands
                .iter()
                .take(self.keep.max(1))
                .map(|(nb, _)| evaluate_after(&two, nb, on_roll))
                .max_by(|x, y| o.value(x).total_cmp(&o.value(y)))
                .expect("at least the empty move");
            acc.win += w * best.win;
            acc.win_gammon += w * best.win_gammon;
            acc.win_backgammon += w * best.win_backgammon;
            acc.lose_gammon += w * best.lose_gammon;
            acc.lose_backgammon += w * best.lose_backgammon;
        }
        acc.clamped()
    }
}

/// 3수: 2수 순위 상위 몇 개를 3수로 다시 보고, 상대 응수는 1수 상위 몇 개만 2수로 본다.
pub const THREE_PLY_KEEP: usize = 3;
pub const THREE_PLY_REPLIES: usize = 3;

/// 2수로 다시 평가할 후보: 1수 순위 상위 `keep`개 중 최선과의 차이가 `margin` 이하인 것.
#[derive(Clone, Copy, Debug)]
pub struct Filter {
    pub keep: usize,
    pub margin: f32,
}

impl Default for Filter {
    fn default() -> Self {
        Filter {
            keep: 8,
            margin: 0.16,
        }
    }
}

/// 탐색 설정: 깊이(1, 2, 3)와 2수 후보 거르기. 3수는 2수 결과 상위 `THREE_PLY_KEEP`개를 다시 본다.
#[derive(Clone, Copy, Debug)]
pub struct Search {
    pub ply: u8,
    pub filter: Filter,
    /// 수를 둔 쪽의 목표.
    pub objective: Objective,
}

impl Search {
    pub const ONE_PLY: Search = Search {
        ply: 1,
        filter: Filter {
            keep: 0,
            margin: 0.0,
        },
        objective: Objective::Money,
    };
    pub const TWO_PLY: Search = Search {
        ply: 2,
        filter: Filter {
            keep: 8,
            margin: 0.16,
        },
        objective: Objective::Money,
    };

    pub fn with_objective(self, objective: Objective) -> Search {
        Search { objective, ..self }
    }

    pub const THREE_PLY: Search = Search {
        ply: 3,
        ..Search::TWO_PLY
    };

    pub fn from_ply(ply: u8) -> Search {
        match ply {
            0 | 1 => Search::ONE_PLY,
            2 => Search::TWO_PLY,
            _ => Search::THREE_PLY,
        }
    }
}

/// `ply` 깊이로 순위를 매긴다. 2수면 걸러진 후보와 `include`(분석할 때 둔 수의 결과 국면)를 2수로 다시 평가해서
/// 앞에 두고(2수 에퀴티 순), 나머지는 1수 값으로 뒤에 둔다. 각 항목의 `ply`가 어느 깊이 값인지 알려 준다.
pub fn rank_moves_ply<E: Evaluator + Clone>(
    e: &E,
    b: &Board,
    side: Side,
    d1: u8,
    d2: u8,
    search: Search,
    include: Option<&Board>,
) -> Vec<ScoredMove> {
    let filter = search.filter;
    let obj = search.objective;
    let mut ranked = rank_moves(e, b, side, d1, d2);
    if obj != Objective::Money {
        for m in &mut ranked {
            m.equity = obj.value(&m.probs);
        }
        ranked.sort_by(|a, b| b.equity.total_cmp(&a.equity));
    }
    if search.ply < 2 || ranked.len() < 2 {
        return ranked;
    }
    let top = ranked[0].equity;
    let deep = TwoPly::with_objective(e.clone(), side.opp(), obj.swapped());
    let (mut hi, mut lo): (Vec<ScoredMove>, Vec<ScoredMove>) = (Vec::new(), Vec::new());
    for (i, m) in ranked.into_iter().enumerate() {
        // 1·2위는 항상 2수로 본다(힌트의 "2위보다 얼마나 나은가"가 같은 깊이끼리 비교되도록).
        let selected = i < 2
            || (i < filter.keep && top - m.equity <= filter.margin)
            || include == Some(&m.board);
        if selected {
            let probs = evaluate_after(&deep, &m.board, side);
            hi.push(ScoredMove {
                equity: obj.value(&probs),
                probs,
                ply: 2,
                ..m
            });
        } else {
            lo.push(m);
        }
    }
    hi.sort_by(|a, b| b.equity.total_cmp(&a.equity));
    if search.ply >= 3 && hi.len() >= 2 {
        let deep3 =
            ThreePly::with_objective(e.clone(), side.opp(), obj.swapped(), THREE_PLY_REPLIES);
        let rest = hi.split_off(THREE_PLY_KEEP.min(hi.len()));
        let mut top3: Vec<ScoredMove> = hi
            .into_iter()
            .map(|m| {
                let probs = evaluate_after(&deep3, &m.board, side);
                ScoredMove {
                    equity: obj.value(&probs),
                    probs,
                    ply: 3,
                    ..m
                }
            })
            .collect();
        top3.sort_by(|a, b| b.equity.total_cmp(&a.equity));
        // 분석할 때 둔 수가 3수 후보 밖이면 2수 값 그대로 뒤에 둔다.
        top3.extend(rest);
        top3.extend(lo);
        return top3;
    }
    hi.extend(lo);
    hi
}

/// 웹·분석용 요약.
#[derive(Clone, Debug, Serialize)]
pub struct MoveSummary {
    pub steps: Vec<crate::board::Step>,
    pub notation: String,
    pub board: Vec<u8>,
    pub probs: OutcomeProbs,
    pub equity: f32,
    pub ply: u8,
}

impl From<&ScoredMove> for MoveSummary {
    fn from(s: &ScoredMove) -> Self {
        MoveSummary {
            steps: s.mv.steps().to_vec(),
            notation: crate::notation::format_move(s.mv.steps()),
            board: s.board.to_flat(),
            probs: s.probs,
            equity: s.equity,
            ply: s.ply,
        }
    }
}
