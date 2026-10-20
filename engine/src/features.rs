//! 설명 층이 쓰는 특징값. 모든 값은 판에서 직접 계산한다.
//!
//! 샷(맞을 조합) 계산은 근사가 아니라 상대의 21가지 주사위(36 조합) 각각에 대해 실제 합법수를
//! 만들어 보고 그 중 블롯을 맞히는 수가 있는지 확인한다. 그래서 막힌 칸, 상대 바 진입 의무,
//! 두 눈 다 쓰기 강제 같은 규칙이 모두 반영된다.

use serde::Serialize;

use crate::board::{BAR, Board, Side};
use crate::movegen::legal_moves;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct BlotInfo {
    /// 자기 기준 포인트 번호.
    pub point: u8,
    /// 가장 가까운 상대 말(바 포함)까지의 거리. 맞을 수 있는 상대 말이 뒤에 없으면 None.
    pub nearest_attacker: Option<u8>,
    /// 상대가 이 블롯을 맞힐 수 있는 주사위 조합 수(36개 중).
    pub shots: u8,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SideFeatures {
    pub pips: u32,
    pub blots: Vec<BlotInfo>,
    /// 블롯 중 하나라도 맞을 수 있는 조합 수(36개 중).
    pub total_shots: u8,
    /// 말이 2개 이상 있는 포인트(자기 기준, 1..=24).
    pub made_points: Vec<u8>,
    pub five_point: bool,
    pub bar_point: bool,
    /// 상대 홈보드(자기 기준 19..=24)에 만든 포인트.
    pub anchors: Vec<u8>,
    /// 가장 긴 연속 포인트 길이와 그 범위(낮은 쪽, 높은 쪽). 없으면 0, (0, 0).
    pub prime_len: u8,
    pub prime_range: (u8, u8),
    /// 홈보드(1..=6)에 만든 포인트 수.
    pub home_points: u8,
    /// 상대 홈보드(19..=24)에 남은 말 수(바 제외).
    pub back_checkers: u8,
    pub on_bar: u8,
    pub off: u8,
    /// 포인트 1..=24의 말 수(인덱스 0 = 포인트 1).
    pub point_counts: Vec<u8>,
}

pub use crate::eval::OutcomeProbs;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Features {
    /// [White, Black].
    pub sides: [SideFeatures; 2],
    /// White 쪽에서 본 결과 확률. 평가기를 거치지 않았으면 None(`compute_features_with_eval`).
    pub outcome: Option<OutcomeProbs>,
}

/// 특징값 + 평가기의 결과 확률(White 기준). `on_roll`은 이제 굴릴 차례인 쪽.
pub fn compute_features_with_eval<E: crate::eval::Evaluator + ?Sized>(
    board: &Board,
    on_roll: Side,
    eval: &E,
) -> Features {
    let mut f = compute_features(board);
    let p = eval.evaluate(board, on_roll);
    f.outcome = Some(if on_roll == Side::White {
        p
    } else {
        p.invert()
    });
    f
}

pub fn compute_features(board: &Board) -> Features {
    let shots = [
        shot_table(board, Side::White),
        shot_table(board, Side::Black),
    ];
    Features {
        sides: [
            side_features(board, Side::White, &shots[0]),
            side_features(board, Side::Black, &shots[1]),
        ],
        outcome: None,
    }
}

/// side의 블롯에 대해: 포인트별 맞는 조합 수와, 하나라도 맞는 조합 수.
pub struct ShotTable {
    pub per_point: [u8; 26],
    pub any: u8,
}

pub fn shot_table(board: &Board, side: Side) -> ShotTable {
    let mut per_point = [0u8; 26];
    let mut any = 0u8;
    let attacker = side.opp();
    let me = &board.pos[side.idx()];
    let blot_mask: u32 = (1..=24)
        .filter(|&p| me[p] == 1)
        .fold(0, |m, p| m | (1 << p));
    if blot_mask == 0 {
        return ShotTable { per_point, any };
    }
    for d1 in 1..=6u8 {
        for d2 in d1..=6u8 {
            let weight = if d1 == d2 { 1 } else { 2 };
            let mut hit_mask = 0u32;
            for m in legal_moves(board, attacker, d1, d2) {
                for s in m.steps().iter().filter(|s| s.hit) {
                    // 공격자 기준 s.to = 내 기준 25 - s.to.
                    hit_mask |= 1 << (25 - s.to as u32);
                }
            }
            hit_mask &= blot_mask;
            if hit_mask != 0 {
                any += weight;
            }
            for (p, n) in per_point.iter_mut().enumerate().take(25).skip(1) {
                if hit_mask & (1 << p) != 0 {
                    *n += weight;
                }
            }
        }
    }
    ShotTable { per_point, any }
}

fn side_features(board: &Board, side: Side, shots: &ShotTable) -> SideFeatures {
    let me = &board.pos[side.idx()];
    let opp = &board.pos[side.opp().idx()];
    let made_points: Vec<u8> = (1..=24u8).filter(|&p| me[p as usize] >= 2).collect();

    // 상대 말의 내 기준 위치: 상대 포인트 q -> 25 - q, 상대 바 -> 0.
    let attacker_positions: Vec<u8> = (1..=BAR)
        .filter(|&q| opp[q] > 0)
        .map(|q| if q == BAR { 0 } else { (25 - q) as u8 })
        .collect();
    let blots = (1..=24u8)
        .filter(|&p| me[p as usize] == 1)
        .map(|p| BlotInfo {
            point: p,
            nearest_attacker: attacker_positions
                .iter()
                .filter(|&&a| a < p)
                .map(|&a| p - a)
                .min(),
            shots: shots.per_point[p as usize],
        })
        .collect();

    let (mut best_len, mut best_range, mut run) = (0u8, (0u8, 0u8), 0u8);
    for p in 1..=24u8 {
        if me[p as usize] >= 2 {
            run += 1;
            if run > best_len {
                best_len = run;
                best_range = (p + 1 - run, p);
            }
        } else {
            run = 0;
        }
    }

    SideFeatures {
        pips: board.pips(side),
        blots,
        total_shots: shots.any,
        five_point: me[5] >= 2,
        bar_point: me[7] >= 2,
        anchors: made_points.iter().copied().filter(|&p| p >= 19).collect(),
        home_points: made_points.iter().filter(|&&p| p <= 6).count() as u8,
        made_points,
        prime_len: best_len,
        prime_range: best_range,
        back_checkers: me[19..=24].iter().sum(),
        on_bar: me[BAR],
        off: me[0],
        point_counts: me[1..=24].to_vec(),
    }
}
