//! 탐색. 3단계: 1수(1-ply) 탐색 — 합법수마다 결과 국면을 평가해서 에퀴티가 가장 높은 수를 고른다.

use serde::Serialize;

use crate::board::{Board, Side};
use crate::eval::{Evaluator, OutcomeProbs};
use crate::movegen::{Move, legal_moves_with_boards};

#[derive(Clone, Debug)]
pub struct ScoredMove {
    pub mv: Move,
    pub board: Board,
    /// 수를 둔 쪽 기준 결과 확률.
    pub probs: OutcomeProbs,
    pub equity: f32,
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

/// 웹·분석용 요약.
#[derive(Clone, Debug, Serialize)]
pub struct MoveSummary {
    pub steps: Vec<crate::board::Step>,
    pub notation: String,
    pub board: Vec<u8>,
    pub probs: OutcomeProbs,
    pub equity: f32,
}

impl From<&ScoredMove> for MoveSummary {
    fn from(s: &ScoredMove) -> Self {
        MoveSummary {
            steps: s.mv.steps().to_vec(),
            notation: crate::notation::format_move(s.mv.steps()),
            board: s.board.to_flat(),
            probs: s.probs,
            equity: s.equity,
        }
    }
}
