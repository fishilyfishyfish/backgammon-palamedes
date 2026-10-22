//! 더블링 큐브 판단(6단계).
//!
//! 큐브 없는(cubeless) 결과 확률에서 큐브가 있는(cubeful) 에퀴티를 Janowski 모델로 근사한다:
//! E = x·E_live + (1 − x)·E_dead. E_dead는 큐브 없는 에퀴티, E_live는 큐브가 완전히 살아 있을 때
//! (확률이 연속으로 움직여 테이크 포인트·캐시 포인트에서 정확히 더블·패스가 일어날 때)의 에퀴티,
//! x는 큐브 효율(접촉 국면 0.68, 레이스는 핍에 따라 0.6~0.75).
//!
//! 승패의 평균 크기 W, L(개먼·백개먼 포함)로 테이크 포인트와 캐시 포인트를 구한다:
//! TP = (L − ½)/(W + L + ½), CP = (L + 1)/(W + L + ½). W = L = 1이면 TP = 0.2, CP = 0.8.
//! 에퀴티는 모두 현재 큐브 값 1 기준(점수로 바꾸려면 큐브 값을 곱한다), 굴릴 쪽 관점.

use serde::Serialize;

use crate::board::{Board, Side};
use crate::eval::{OutcomeProbs, is_race};

/// 큐브 소유. 판단하는 쪽(굴릴 쪽) 기준.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Owner {
    Centered,
    Me,
    Opp,
}

/// 이길 때·질 때 평균 점수(큐브 값 1 기준).
pub fn win_values(p: &OutcomeProbs) -> (f32, f32) {
    let w = p.win;
    let l = 1.0 - w;
    let wv = if w > 1e-6 {
        1.0 + (p.win_gammon + p.win_backgammon) / w
    } else {
        1.0
    };
    let lv = if l > 1e-6 {
        1.0 + (p.lose_gammon + p.lose_backgammon) / l
    } else {
        1.0
    };
    (wv, lv)
}

/// 테이크 포인트(상대가 더블했을 때 내가 받을 수 있는 최소 승률)와 캐시 포인트(내가 더블하면 상대가 패스할 승률).
pub fn take_cash_points(w_val: f32, l_val: f32) -> (f32, f32) {
    let d = w_val + l_val + 0.5;
    ((l_val - 0.5) / d, (l_val + 1.0) / d)
}

/// 큐브가 완전히 살아 있을 때의 에퀴티.
pub fn live_equity(win: f32, w_val: f32, l_val: f32, owner: Owner) -> f32 {
    let (tp, cp) = take_cash_points(w_val, l_val);
    match owner {
        Owner::Centered => {
            if win <= tp {
                -1.0
            } else if win >= cp {
                1.0
            } else {
                -1.0 + 2.0 * (win - tp) / (cp - tp)
            }
        }
        Owner::Me => {
            if win >= cp {
                1.0
            } else {
                -l_val + (l_val + 1.0) * win / cp
            }
        }
        Owner::Opp => {
            if win <= tp {
                -1.0
            } else {
                -1.0 + (w_val + 1.0) * (win - tp) / (1.0 - tp)
            }
        }
    }
}

/// 큐브 효율. 레이스는 길수록 큐브가 효율적이다(근사값, 롤아웃으로 검증하지 않음).
pub fn cube_efficiency(b: &Board, on_roll: Side) -> f32 {
    if is_race(b) {
        (0.6 + 0.001 * b.pips(on_roll) as f32).min(0.75)
    } else {
        0.68
    }
}

/// Janowski 큐브 에퀴티.
pub fn cubeful_equity(p: &OutcomeProbs, owner: Owner, x: f32) -> f32 {
    let (wv, lv) = win_values(p);
    x * live_equity(p.win, wv, lv, owner) + (1.0 - x) * p.equity()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CubeAction {
    /// 더블하지 않는다.
    NoDouble,
    /// 더블, 상대는 받아야 한다.
    DoubleTake,
    /// 더블, 상대는 패스해야 한다.
    DoublePass,
    /// 너무 좋아서 더블하지 않고 개먼을 노린다(더블하면 상대가 패스하므로 1점밖에 못 받음).
    TooGood,
}

#[derive(Clone, Debug, Serialize)]
pub struct CubeDecision {
    pub action: CubeAction,
    /// 더블하는 게 맞는가.
    pub double: bool,
    /// 더블을 받으면 받아야 하는가(상대 입장).
    pub take: bool,
    /// 굴릴 쪽 기준 큐브 에퀴티(현재 큐브 값 1 기준): 노 더블, 더블/테이크, 더블/패스(= 1).
    pub no_double: f32,
    pub double_take: f32,
    pub double_pass: f32,
    /// 더블했을 때 최선(상대가 바르게 응답할 때) − 노 더블. 양수면 더블이 낫다.
    pub double_gain: f32,
    pub take_point: f32,
    pub cash_point: f32,
    pub efficiency: f32,
    pub probs: OutcomeProbs,
}

/// 머니 게임 큐브 판단. `probs`는 굴릴 쪽 기준(굴리기 전), `owner`는 Centered 또는 Me여야 더블할 수 있다.
pub fn money_decision(probs: &OutcomeProbs, owner: Owner, x: f32) -> CubeDecision {
    money_decision_adj(probs, owner, x, 0.0)
}

/// 접촉 국면 머니에서 노 더블 에퀴티에 더하는 보정. gnubg 2-ply 큐브 판단으로 채점해 보니(`cube_check`, 4000국면)
/// 더블 결정 근처 국면에서 우리 노 더블이 gnubg보다 평균 0.038 낮아 더블을 너무 일찍 했다(잘못된 더블 205 vs 놓침 34).
/// 0.03을 더하면 더블 쪽 손실이 판단당 0.0034 → 0.0021. 레이스는 오히려 나빠져서 넣지 않는다.
/// 다른 국면 4000개 검증: 0.0029 → 0.0018. 예전 정책과 자기대국 +0.009 ± 0.006 ppg(16000판).
pub const CONTACT_ND_ADJ: f32 = 0.03;

/// `money_decision`과 같되 노 더블 에퀴티에 `nd_adj`를 더한다.
pub fn money_decision_adj(probs: &OutcomeProbs, owner: Owner, x: f32, nd_adj: f32) -> CubeDecision {
    let nd = cubeful_equity(probs, owner, x) + nd_adj;
    let dt = 2.0 * cubeful_equity(probs, Owner::Opp, x);
    let dp = 1.0;
    let take = dt <= dp;
    let best_double = dt.min(dp);
    let (wv, lv) = win_values(probs);
    let (tp, cp) = take_cash_points(wv, lv);
    let double = owner != Owner::Opp && best_double > nd;
    let action = if owner == Owner::Opp {
        CubeAction::NoDouble
    } else if double {
        if take {
            CubeAction::DoubleTake
        } else {
            CubeAction::DoublePass
        }
    } else if !take {
        CubeAction::TooGood
    } else {
        CubeAction::NoDouble
    };
    CubeDecision {
        action,
        double,
        take,
        no_double: nd,
        double_take: dt,
        double_pass: dp,
        double_gain: best_double - nd,
        take_point: tp,
        cash_point: cp,
        efficiency: x,
        probs: *probs,
    }
}

/// 큐브 상태. `owner`가 None이면 가운데.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct CubeState {
    pub value: u8,
    pub owner: Option<Side>,
}

impl Default for CubeState {
    fn default() -> Self {
        CubeState {
            value: 1,
            owner: None,
        }
    }
}

impl CubeState {
    /// `side` 기준 소유.
    pub fn owner_for(&self, side: Side) -> Owner {
        match self.owner {
            None => Owner::Centered,
            Some(s) if s == side => Owner::Me,
            Some(_) => Owner::Opp,
        }
    }

    pub fn can_double(&self, side: Side) -> bool {
        self.owner != Some(side.opp())
    }
}

/// `side`가 굴리기 전에 하는 큐브 판단. 결과 확률은 2수(이번 굴림의 21가지마다 최선 수) 평가.
/// `ctx`가 있으면 점수 상황(MWC) 판단, 없으면 머니 판단.
pub fn decide<E: crate::eval::Evaluator + Clone>(
    e: &E,
    b: &Board,
    side: Side,
    cube: CubeState,
    ctx: Option<crate::matchplay::MatchCtx>,
) -> CubeDecision {
    decide_shifted(e, b, side, cube, ctx, 0.0)
}

/// `decide`와 같되 큐브 효율에 `x_shift`를 더한다(효율 값 비교 실험용).
pub fn decide_shifted<E: crate::eval::Evaluator + Clone>(
    e: &E,
    b: &Board,
    side: Side,
    cube: CubeState,
    ctx: Option<crate::matchplay::MatchCtx>,
    x_shift: f32,
) -> CubeDecision {
    decide_tuned(e, b, side, cube, ctx, x_shift, CONTACT_ND_ADJ)
}

/// `decide_shifted`와 같되 접촉 국면 머니의 노 더블 보정(`CONTACT_ND_ADJ`)을 직접 준다(비교 실험용).
#[allow(clippy::too_many_arguments)]
pub fn decide_tuned<E: crate::eval::Evaluator + Clone>(
    e: &E,
    b: &Board,
    side: Side,
    cube: CubeState,
    ctx: Option<crate::matchplay::MatchCtx>,
    x_shift: f32,
    contact_nd_adj: f32,
) -> CubeDecision {
    use crate::search::{Objective, TwoPly};
    let obj = ctx.map(Objective::Match).unwrap_or(Objective::Money);
    let probs =
        crate::eval::Evaluator::evaluate(&TwoPly::with_objective(e.clone(), side, obj), b, side);
    let x = (cube_efficiency(b, side) + x_shift).clamp(0.0, 1.0);
    let owner = cube.owner_for(side);
    match ctx {
        None => {
            let adj = if crate::eval::is_race(b) {
                0.0
            } else {
                contact_nd_adj
            };
            money_decision_adj(&probs, owner, x, adj)
        }
        Some(c) => crate::matchplay::MatchCtx {
            cube: cube.value,
            ..c
        }
        .cube_decision(&probs, owner, x),
    }
}

/// 큐브 판단 근거 문장. 값은 모두 `d`에서 가져온다. `for_taker`면 더블을 받은 쪽 입장(받을지 말지)으로 쓴다.
/// `match_play`면 에퀴티 단위가 정규화 매치 에퀴티라고 밝힌다.
pub fn cube_reasons(d: &CubeDecision, for_taker: bool, match_play: bool) -> Vec<String> {
    let pct = |x: f32| format!("{:.1}%", 100.0 * x);
    let unit = if match_play {
        "정규화 매치 에퀴티"
    } else {
        "큐브 1 기준 에퀴티"
    };
    let p = &d.probs;
    let mut out = Vec::new();
    // 상대가 받을 수 있는 최소 승률 = 1 − 더블한 쪽의 캐시 포인트.
    let taker_needs = 1.0 - d.cash_point;
    let taker_has = 1.0 - p.win;
    if for_taker {
        out.push(format!(
            "더블한 쪽의 이길 확률은 {}(개먼승 {}), 내 이길 확률은 {}야.",
            pct(p.win),
            pct(p.win_gammon),
            pct(taker_has)
        ));
        out.push(format!(
            "받으려면 이길 확률이 대략 {} 이상이어야 해(테이크 포인트, 큐브 효율 {:.2} 근사). 그래서 {}.",
            pct(taker_needs),
            d.efficiency,
            if d.take { "받는 게 맞아" } else { "포기하는 게 맞아" }
        ));
        out.push(format!(
            "받으면 더블한 쪽 {unit}가 {:+.3}, 포기하면 {:+.3}.",
            d.double_take, d.double_pass
        ));
        return out;
    }
    out.push(format!(
        "이길 확률 {}, 개먼승 {}, 개먼패 {}.",
        pct(p.win),
        pct(p.win_gammon),
        pct(p.lose_gammon)
    ));
    let best = d.double_take.min(d.double_pass);
    match d.action {
        CubeAction::NoDouble => out.push(format!(
            "지금 더블하면 손해야: 노 더블 {:+.3}, 더블했을 때 {:+.3}({unit}).",
            d.no_double, best
        )),
        CubeAction::DoubleTake | CubeAction::DoublePass => out.push(format!(
            "더블하면 {unit}가 {:+.3}에서 {:+.3}로 올라가.",
            d.no_double, best
        )),
        CubeAction::TooGood => out.push(format!(
            "더블하면 상대가 포기해서 {:+.3}이지만, 계속 두면 {:+.3}라서 개먼을 노리는 게 나아.",
            d.double_pass, d.no_double
        )),
    }
    if d.double {
        out.push(format!(
            "상대 이길 확률 {}는 상대가 받을 수 있는 최소 {}보다 {} 상대는 {}.",
            pct(taker_has),
            pct(taker_needs),
            if d.take { "높아서" } else { "낮아서" },
            if d.take {
                "받아야 해"
            } else {
                "포기해야 해"
            }
        ));
    }
    out
}
