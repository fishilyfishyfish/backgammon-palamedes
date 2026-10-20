//! 롤아웃(6단계): 국면에서 끝까지 여러 번 두어 보고 결과를 평균한다(큐브 없는 머니).
//!
//! - 양쪽 모두 신경망 1수 탐색으로 둔다(빠름. 결정당 네이티브 0.02 ms).
//! - 분산 줄이기: (1) 처음 두 굴림을 36×36 층화(시행 i의 첫 굴림 = i mod 36, 둘째 = (i / 36) mod 36),
//!   (2) 후보 수끼리 같은 시드를 쓴다(공통 난수). 다만 수가 다르면 진행이 금방 갈라져서 차이의 오차는 조금만 준다(CLAUDE.md).
//! - 시행 단위로 쪼개 부를 수 있게 `start`(시행 번호 시작)를 받는다. 웹은 36개씩 나눠 불러 진행률을 보여 준다.

use serde::Serialize;

use crate::board::{Board, Side};
use crate::eval::{Evaluator, OutcomeProbs};
use crate::rng::Rng;
use crate::search::best_move;

/// 시행 i의 k번째 굴림(k = 0, 1은 층화, 그 뒤는 시드 난수).
fn roll(i: u32, k: u32, rng: &mut Rng) -> (u8, u8) {
    let idx = match k {
        0 => Some(i % 36),
        1 => Some((i / 36) % 36),
        _ => None,
    };
    match idx {
        Some(x) => ((x / 6) as u8 + 1, (x % 6) as u8 + 1),
        None => rng.roll(),
    }
}

/// 롤아웃 누적값. 확률은 `mover` 기준 합계, `eq_sq`는 에퀴티 제곱합.
#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq)]
pub struct RolloutSums {
    pub trials: u32,
    pub win: f64,
    pub win_gammon: f64,
    pub win_backgammon: f64,
    pub lose_gammon: f64,
    pub lose_backgammon: f64,
    pub eq: f64,
    pub eq_sq: f64,
}

impl RolloutSums {
    pub fn add(&mut self, o: &RolloutSums) {
        self.trials += o.trials;
        self.win += o.win;
        self.win_gammon += o.win_gammon;
        self.win_backgammon += o.win_backgammon;
        self.lose_gammon += o.lose_gammon;
        self.lose_backgammon += o.lose_backgammon;
        self.eq += o.eq;
        self.eq_sq += o.eq_sq;
    }

    pub fn probs(&self) -> OutcomeProbs {
        let n = self.trials.max(1) as f64;
        OutcomeProbs {
            win: (self.win / n) as f32,
            win_gammon: (self.win_gammon / n) as f32,
            win_backgammon: (self.win_backgammon / n) as f32,
            lose_gammon: (self.lose_gammon / n) as f32,
            lose_backgammon: (self.lose_backgammon / n) as f32,
        }
    }

    pub fn equity(&self) -> f64 {
        self.eq / self.trials.max(1) as f64
    }

    /// 평균 에퀴티의 표준오차(층화 효과는 반영하지 않은 보수적 값).
    pub fn stderr(&self) -> f64 {
        let n = self.trials as f64;
        if n < 2.0 {
            return f64::INFINITY;
        }
        let m = self.eq / n;
        ((self.eq_sq / n - m * m).max(0.0) / (n - 1.0)).sqrt()
    }
}

/// `after`는 `mover`가 수를 둔 뒤 국면(상대 차례). 시행 [start, start + trials)를 돌린다.
pub fn rollout<E: Evaluator + ?Sized>(
    e: &E,
    after: &Board,
    mover: Side,
    start: u32,
    trials: u32,
    seed: u64,
) -> RolloutSums {
    let mut s = RolloutSums::default();
    for i in start..start + trials {
        let mut rng = Rng::new(seed ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut b = *after;
        let mut side = mover.opp();
        let mut k = 0;
        let (winner, pts) = loop {
            if let Some(r) = b.result() {
                break r;
            }
            let (d1, d2) = roll(i, k, &mut rng);
            k += 1;
            let m = best_move(e, &b, side, d1, d2).mv;
            b.apply_steps(side, m.steps());
            side = side.opp();
        };
        let o = OutcomeProbs::terminal(winner, pts, mover);
        s.trials += 1;
        s.win += o.win as f64;
        s.win_gammon += o.win_gammon as f64;
        s.win_backgammon += o.win_backgammon as f64;
        s.lose_gammon += o.lose_gammon as f64;
        s.lose_backgammon += o.lose_backgammon as f64;
        let eq = o.equity() as f64;
        s.eq += eq;
        s.eq_sq += eq * eq;
    }
    s
}
