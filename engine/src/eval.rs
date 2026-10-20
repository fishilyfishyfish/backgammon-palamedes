//! 평가함수. 3단계 기준선: 손으로 짠 평가(`HandEval`).
//!
//! 모든 평가는 "둘 차례인 쪽(on_roll) 기준"의 결과 확률을 돌려준다. 확률은 GNU 백개먼과 같은 관례:
//! `win_gammon`은 백개먼을 포함하고, `win_backgammon`은 그 일부다.

use serde::Serialize;

use crate::board::{BAR, Board, Side};
use crate::features::shot_table;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Default)]
pub struct OutcomeProbs {
    pub win: f32,
    pub win_gammon: f32,
    pub win_backgammon: f32,
    pub lose_gammon: f32,
    pub lose_backgammon: f32,
}

impl OutcomeProbs {
    /// 큐브 없는 머니 게임 에퀴티(한 판 기준 기대 점수).
    pub fn equity(&self) -> f32 {
        2.0 * self.win - 1.0 + self.win_gammon - self.lose_gammon + self.win_backgammon
            - self.lose_backgammon
    }

    /// 상대 쪽에서 본 확률.
    pub fn invert(&self) -> OutcomeProbs {
        OutcomeProbs {
            win: 1.0 - self.win,
            win_gammon: self.lose_gammon,
            win_backgammon: self.lose_backgammon,
            lose_gammon: self.win_gammon,
            lose_backgammon: self.win_backgammon,
        }
    }

    /// 범위와 포함 관계(bg ≤ gammon ≤ win)를 맞춘다.
    pub fn clamped(self) -> OutcomeProbs {
        let win = self.win.clamp(0.0, 1.0);
        let lose = 1.0 - win;
        let wg = self.win_gammon.clamp(0.0, win);
        let lg = self.lose_gammon.clamp(0.0, lose);
        OutcomeProbs {
            win,
            win_gammon: wg,
            win_backgammon: self.win_backgammon.clamp(0.0, wg),
            lose_gammon: lg,
            lose_backgammon: self.lose_backgammon.clamp(0.0, lg),
        }
    }

    /// 종국 국면의 확정 결과. `perspective` 기준.
    pub fn terminal(winner: Side, points: u8, perspective: Side) -> OutcomeProbs {
        let p = OutcomeProbs {
            win: 1.0,
            win_gammon: if points >= 2 { 1.0 } else { 0.0 },
            win_backgammon: if points >= 3 { 1.0 } else { 0.0 },
            lose_gammon: 0.0,
            lose_backgammon: 0.0,
        };
        if winner == perspective { p } else { p.invert() }
    }
}

pub trait Evaluator {
    /// `on_roll`이 이제 주사위를 굴릴 차례일 때, `on_roll` 기준 결과 확률.
    fn evaluate(&self, board: &Board, on_roll: Side) -> OutcomeProbs;
}

/// 표준 정규 누적분포(Abramowitz–Stegun 7.1.26, 오차 1.5e-7).
pub fn norm_cdf(x: f32) -> f32 {
    let x = x as f64;
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs() / std::f64::consts::SQRT_2);
    let y = 1.0
        - (((((1.061_405_429 * t - 1.453_152_027) * t) + 1.421_413_741) * t - 0.284_496_736) * t
            + 0.254_829_592)
            * t
            * (-(x * x) / 2.0).exp();
    let erf = if x >= 0.0 { y } else { -y };
    (0.5 * (1.0 + erf)) as f32
}

/// 주사위 한 번의 평균 핍(더블 포함) = 49/6.
const MEAN_ROLL: f32 = 8.166_667;
/// 한 번 굴림의 핍 분산.
const ROLL_VAR: f32 = 18.6;
/// 한 번 굴림에 베어오프되는 평균 말 수(더블이면 4).
const CHECKERS_PER_ROLL: f32 = 2.333;

/// 접촉이 끝났는가(서로 지나쳐서 다시 맞을 일이 없음).
pub fn is_race(b: &Board) -> bool {
    let rear = |s: Side| {
        (1..=BAR)
            .rev()
            .find(|&p| b.pos[s.idx()][p] > 0)
            .unwrap_or(0)
    };
    rear(Side::White) + rear(Side::Black) <= 25
}

/// 레이스에서 다 빼는 데 필요한 굴림 수 추정과 그 분산.
fn race_rolls(b: &Board, s: Side, wastage_per_checker: f32) -> (f32, f32) {
    let me = &b.pos[s.idx()];
    let n: u8 = me[1..=BAR].iter().sum();
    let pips = b.pips(s) as f32 + wastage_per_checker * n as f32;
    let t = (pips / MEAN_ROLL).max(n as f32 / CHECKERS_PER_ROLL);
    (t, pips * ROLL_VAR / MEAN_ROLL.powi(3))
}

/// 개먼을 피하려면(말 하나라도 빼려면) 필요한 굴림 수와 분산. 이미 뺐으면 None.
fn save_gammon_rolls(b: &Board, s: Side) -> Option<(f32, f32)> {
    let me = &b.pos[s.idx()];
    if me[0] > 0 {
        return None;
    }
    let outside: u32 = (7..=BAR).map(|p| (p as u32 - 6) * me[p] as u32).sum();
    let k: u8 = me[7..=BAR].iter().sum();
    // 다 들어온 뒤 하나를 빼려면 평균 3핍 정도 더 필요.
    let pips = outside as f32 + 3.0;
    let t = (pips / MEAN_ROLL).max((k as f32 + 1.0) / CHECKERS_PER_ROLL);
    Some((t, pips * ROLL_VAR / MEAN_ROLL.powi(3)))
}

/// 레이스 확률. 둘 차례가 먼저 굴리므로 T_me ≤ T_opp면 이긴다.
fn race_probs(b: &Board, me: Side, wastage: f32) -> OutcomeProbs {
    let opp = me.opp();
    let (tm, vm) = race_rolls(b, me, wastage);
    let (to, vo) = race_rolls(b, opp, wastage);
    let win = norm_cdf((to - tm + 0.5) / (vm + vo + 0.3).sqrt());
    // 내가 T_me번째에 끝낼 때 상대는 T_me - 1번 굴렸다.
    let win_gammon = save_gammon_rolls(b, opp)
        .map(|(ts, vs)| norm_cdf((ts - tm + 0.5) / (vm + vs + 0.3).sqrt()))
        .unwrap_or(0.0);
    // 상대가 T_opp번째에 끝낼 때 나는 T_opp번 굴렸다.
    let lose_gammon = save_gammon_rolls(b, me)
        .map(|(ts, vs)| norm_cdf((ts - to - 0.5) / (vo + vs + 0.3).sqrt()))
        .unwrap_or(0.0);
    OutcomeProbs {
        win,
        win_gammon,
        win_backgammon: 0.0,
        lose_gammon,
        lose_backgammon: 0.0,
    }
    .clamped()
}

/// 손으로 짠 평가함수의 가중치. 단위는 대부분 "핍 환산".
#[derive(Clone, Debug, Serialize)]
pub struct HandWeights {
    /// 접촉 국면에서 둘 차례의 이점(핍).
    pub roll_bonus: f32,
    /// 접촉 국면의 불확실성: σ = contact_sigma × √(양쪽 핍 합).
    pub contact_sigma: f32,
    /// 레이스 낭비 핍(말 하나당).
    pub race_wastage: f32,
    /// 자기 기준 포인트별로 만든 포인트의 가치(핍). 인덱스 1..=24.
    pub point_value: [f32; 25],
    /// 1..=9 포인트 가치 중 상대 뒷말이 없을 때 남는 비율.
    pub point_value_floor: f32,
    /// 프라임 길이별 보너스(상대 말이 뒤에 갇혀 있을 때).
    pub prime_bonus: [f32; 7],
    /// 맞을 때 잃는 것: (25 - p) 핍 + 이 템포 값.
    pub hit_tempo: f32,
    /// 맞을 위험 전체 배율.
    pub hit_scale: f32,
    /// 맞힌 쪽 홈보드 포인트 하나당 맞은 손실 증가 비율.
    pub hit_board_factor: f32,
    /// 둘 차례인 쪽 블롯 하나당 벌점(곧 고칠 수 있으므로 작음).
    pub roller_blot: f32,
    /// 바에 있는 말 하나당 벌점: 기본 + 상대 홈보드 포인트 수별 추가.
    pub bar_base: f32,
    pub bar_by_board: [f32; 7],
    /// 한 포인트에 3개를 넘는 말 하나당 벌점.
    pub stack_penalty: f32,
}

impl Default for HandWeights {
    fn default() -> Self {
        let mut point_value = [0.0; 25];
        let pv: [(usize, f32); 24] = [
            (1, 0.5),
            (2, 1.5),
            (3, 2.5),
            (4, 4.5),
            (5, 7.0),
            (6, 5.5),
            (7, 5.0),
            (8, 3.0),
            (9, 2.0),
            (10, 1.5),
            (11, 1.0),
            (12, 0.5),
            (13, 0.5),
            (14, 0.3),
            (15, 0.3),
            (16, 0.3),
            (17, 0.5),
            (18, 2.0),
            (19, 2.0),
            (20, 4.0),
            (21, 3.5),
            (22, 2.0),
            (23, 1.5),
            (24, 1.0),
        ];
        for (p, v) in pv {
            point_value[p] = v;
        }
        HandWeights {
            roll_bonus: 4.0,
            contact_sigma: 3.0,
            race_wastage: 0.6,
            point_value,
            point_value_floor: 0.35,
            prime_bonus: [0.0, 0.0, 0.0, 2.0, 5.0, 9.0, 15.0],
            hit_tempo: 0.0,
            hit_scale: 1.0,
            hit_board_factor: 0.15,
            roller_blot: 0.8,
            bar_base: 2.0,
            bar_by_board: [0.0, 0.5, 1.5, 3.0, 6.0, 11.0, 25.0],
            stack_penalty: 0.6,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct HandEval {
    pub w: HandWeights,
}

/// 평가 항목별 내역(핍 환산). 설명 층과 디버깅에서 쓴다.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct HandTerms {
    pub points: f32,
    pub prime: f32,
    pub blots: f32,
    pub bar: f32,
    pub stacks: f32,
}

impl HandTerms {
    pub fn total(&self) -> f32 {
        self.points + self.prime + self.blots + self.bar + self.stacks
    }
}

impl HandEval {
    /// side의 위치 점수 내역(높을수록 side에게 좋음). `exposed`면 블롯이 지금 상대 주사위에 노출됨.
    #[allow(clippy::needless_range_loop)] // 포인트 번호로 두 배열을 같이 본다.
    pub fn terms(&self, b: &Board, side: Side, exposed: bool) -> HandTerms {
        let w = &self.w;
        let me = &b.pos[side.idx()];
        let opp = &b.pos[side.opp().idx()];
        // 상대 말의 내 기준 위치 q: 상대 포인트 r → 25 - r, 상대 바 → 0.
        let opp_at = |q: usize| if q == 0 { opp[BAR] } else { opp[25 - q] };
        let opp_home_points = (1..=6).filter(|&r| opp[r] >= 2).count();

        // 내 앞쪽 포인트(1..=9)는 상대가 지나가야 할 말이 있을 때 가치가 있다.
        let threat: u8 = (0..=9).map(opp_at).sum();
        let scale =
            w.point_value_floor + (1.0 - w.point_value_floor) * (threat as f32 / 3.0).min(1.0);
        let mut t = HandTerms::default();
        for p in 1..=24 {
            if me[p] >= 2 {
                let v = w.point_value[p];
                t.points += if p <= 9 { v * scale } else { v };
            }
            if me[p] > 3 {
                t.stacks -= w.stack_penalty * (me[p] - 3) as f32;
            }
        }

        // 프라임: 연속 포인트 중 그 아래(뒤)에 상대 말이 갇혀 있는 가장 긴 것.
        let mut run = 0usize;
        let mut best = 0usize;
        for p in 1..=24 {
            if me[p] >= 2 {
                run += 1;
                let low = p + 1 - run;
                if run >= 3 && (0..low).any(|q| opp_at(q) > 0) {
                    best = best.max(run);
                }
            } else {
                run = 0;
            }
        }
        t.prime = w.prime_bonus[best.min(6)];

        // 블롯.
        if exposed {
            let shots = shot_table(b, side);
            let (mut sum_w, mut sum_wv) = (0.0f32, 0.0f32);
            for p in 1..=24 {
                let n = shots.per_point[p] as f32;
                if n > 0.0 {
                    sum_w += n;
                    sum_wv += n * ((25 - p) as f32 + w.hit_tempo);
                }
            }
            if sum_w > 0.0 {
                let p_hit = shots.any as f32 / 36.0;
                let danger = 1.0 + w.hit_board_factor * opp_home_points as f32;
                t.blots = -w.hit_scale * p_hit * (sum_wv / sum_w) * danger;
            }
        } else {
            let blots = (1..=24).filter(|&p| me[p] == 1).count();
            t.blots = -w.roller_blot * blots as f32;
        }

        // 바.
        let on_bar = me[BAR] as f32;
        t.bar = -on_bar * (w.bar_base + w.bar_by_board[opp_home_points]);
        t
    }

    fn gammon_rate(b: &Board, winner: Side) -> (f32, f32) {
        let loser = b.pos[winner.opp().idx()];
        let wpos = b.pos[winner.idx()];
        if loser[0] > 0 {
            return (0.0, 0.0);
        }
        let outside: u32 = (7..=BAR).map(|p| (p as u32 - 6) * loser[p] as u32).sum();
        let home_points = (1..=6).filter(|&p| wpos[p] >= 2).count() as f32;
        let g = ((outside as f32 - 30.0) / 180.0 + 0.03 * home_points + 0.04 * loser[BAR] as f32)
            .clamp(0.0, 0.75);
        // 진 쪽 말이 이긴 쪽 홈(진 쪽 기준 19..=24)이나 바에 있으면 백개먼 가능성.
        let deep: u8 = loser[19..=BAR].iter().sum();
        let bg = (0.04 * deep as f32 + 0.03 * loser[BAR] as f32).clamp(0.0, 0.5);
        (g, bg)
    }
}

impl Evaluator for HandEval {
    fn evaluate(&self, b: &Board, on_roll: Side) -> OutcomeProbs {
        if let Some((w, pts)) = b.result() {
            return OutcomeProbs::terminal(w, pts, on_roll);
        }
        if is_race(b) {
            return race_probs(b, on_roll, self.w.race_wastage);
        }
        let opp = on_roll.opp();
        let mine = self.terms(b, on_roll, false).total();
        let theirs = self.terms(b, opp, true).total();
        let pm = b.pips(on_roll) as f32;
        let po = b.pips(opp) as f32;
        // 위치 점수는 그만큼 유효 핍을 줄인다: 유효 핍 = 핍 - 점수.
        let delta = (po - theirs) - (pm - mine) + self.w.roll_bonus;
        let sigma = self.w.contact_sigma * (pm + po).sqrt();
        let win = norm_cdf(delta / sigma);
        let (g_me, bg_me) = Self::gammon_rate(b, on_roll);
        let (g_op, bg_op) = Self::gammon_rate(b, opp);
        OutcomeProbs {
            win,
            win_gammon: win * g_me,
            win_backgammon: win * g_me * bg_me,
            lose_gammon: (1.0 - win) * g_op,
            lose_backgammon: (1.0 - win) * g_op * bg_op,
        }
        .clamped()
    }
}
