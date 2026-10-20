//! 매치 플레이(6단계): 매치 에퀴티 표(MET)와 점수 상황에서의 큐브·수 판단.
//!
//! ## MET를 직접 계산하는 이유
//! 공개된 표(Kazaross-XG2 등)는 이 개발 환경에서 받아 올 수 없었다(bkgm.com 접속 차단). 그래서 모델로 계산한다.
//! 모델: 한 판의 승률 p가 0.5에서 시작해 연속으로 움직인다(시장 손실 없음). 이기면 비율 `GAMMON_RATE`로 개먼.
//! 큐브는 살아 있고 양쪽이 최적으로 쓴다: 연속 모델에서는 상대의 테이크 포인트(=내 캐시 포인트)에서 더블하는 게
//! 최적이고, 그 사이 구간의 매치 승률(MWC)은 p에 대해 선형이다(마팅게일). 더블을 받은 뒤의 상황은 큐브 값 2v,
//! 소유권이 넘어간 같은 판으로 재귀. 판이 끝나면 다음 판의 MET로 이어지고, 크로포드 규칙(한쪽이 처음 1점 남으면 그 판은
//! 큐브 없음, 그 뒤로는 큐브 사용)을 따른다. 큐브 값 v가 내가 필요한 점수 이상이면 나에게 더블은 쓸모없다(죽은 큐브).
//! 개먼 비율 0.26은 신경망의 시작 국면 값(이길 때 개먼 약 25%)과 맞추고, 알려진 2-away/1-away 크로포드 트레일러
//! 승률(약 32%)과도 맞는 값이다.
//!
//! ## 점수 상황 판단
//! 한 판 안에서는 같은 모델에 실제 국면의 승률과 개먼·백개먼 비율을 넣는다. 큐브 있는 MWC는 Janowski 방식으로
//! x·(살아 있는 큐브) + (1 − x)·(죽은 큐브). 수 선택은 죽은 큐브 MWC를 gnubg식 정규화 에퀴티(EMG)로 바꿔 비교한다:
//! EMG = 2·(MWC − 현재 큐브로 한 판 질 때 MWC)/(이길 때 − 질 때) − 1. 그래서 머니 에퀴티와 같은 눈금으로 실수 등급을 매긴다.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Serialize;

use crate::cube::{CubeAction, CubeDecision, Owner};
use crate::eval::OutcomeProbs;

/// MET 모델의 개먼 비율(이긴 판 중 개먼). 백개먼은 MET 계산에서 무시.
pub const GAMMON_RATE: f64 = 0.26;
/// 계산하는 최대 남은 점수.
pub const MAX_AWAY: usize = 25;

/// 크로포드 단계. Pre: 아직 크로포드 판이 없었음. Post: 크로포드 판이 지났음.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Pre,
    Post,
}

/// 한 판의 결과 분포(이긴 판 기준 비율이 아니라 전체 확률): [단승, 개먼승, 백개먼승, 단패, 개먼패, 백개먼패]의 일부.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Gammons {
    /// 이겼을 때 개먼(백개먼 제외)·백개먼 비율.
    gw: f64,
    bgw: f64,
    gl: f64,
    bgl: f64,
}

impl Gammons {
    fn model() -> Gammons {
        Gammons {
            gw: GAMMON_RATE,
            bgw: 0.0,
            gl: GAMMON_RATE,
            bgl: 0.0,
        }
    }

    fn from_probs(p: &OutcomeProbs) -> Gammons {
        let w = (p.win as f64).max(1e-9);
        let l = (1.0 - p.win as f64).max(1e-9);
        Gammons {
            gw: ((p.win_gammon - p.win_backgammon) as f64 / w).clamp(0.0, 1.0),
            bgw: (p.win_backgammon as f64 / w).clamp(0.0, 1.0),
            gl: ((p.lose_gammon - p.lose_backgammon) as f64 / l).clamp(0.0, 1.0),
            bgl: (p.lose_backgammon as f64 / l).clamp(0.0, 1.0),
        }
    }

    fn key(&self) -> [u32; 4] {
        [self.gw, self.bgw, self.gl, self.bgl].map(|x| (x * 1e6).round() as u32)
    }
}

/// (A 남은 점수, B 남은 점수, 큐브, 소유, 단계, 결과 분포) → 판 진행 중 MWC 함수.
type Memo = HashMap<(usize, usize, usize, Owner, Phase, [u32; 4]), Pl>;

/// 구간별 선형 함수(p ∈ [0, 1] → A의 MWC). 점은 p 오름차순.
#[derive(Clone, Debug)]
struct Pl(Vec<(f64, f64)>);

impl Pl {
    fn at(&self, p: f64) -> f64 {
        let v = &self.0;
        if p <= v[0].0 {
            return v[0].1;
        }
        for w in v.windows(2) {
            let ((x0, y0), (x1, y1)) = (w[0], w[1]);
            if p <= x1 {
                return if x1 > x0 {
                    y0 + (y1 - y0) * (p - x0) / (x1 - x0)
                } else {
                    y1
                };
            }
        }
        v[v.len() - 1].1
    }

    /// f(p) = y가 되는 p(f는 비감소). 범위 밖이면 끝값.
    fn solve(&self, y: f64) -> f64 {
        let v = &self.0;
        if y <= v[0].1 {
            return v[0].0;
        }
        for w in v.windows(2) {
            let ((x0, y0), (x1, y1)) = (w[0], w[1]);
            if y <= y1 {
                return if y1 > y0 {
                    x0 + (x1 - x0) * (y - y0) / (y1 - y0)
                } else {
                    x0
                };
            }
        }
        v[v.len() - 1].0
    }
}

/// 매치 에퀴티 표. `pre[a][b]`: A가 a점, B가 b점 남았을 때 판 시작 전 A의 MWC(a 또는 b가 1이면 크로포드 판).
/// `post[b]`: 크로포드 판이 지난 뒤 1점 남은 리더(A)의 MWC, 트레일러가 b점 남음.
pub struct Met {
    pre: Vec<Vec<f64>>,
    post: Vec<f64>,
}

impl Met {
    pub fn get() -> &'static Met {
        static M: OnceLock<Met> = OnceLock::new();
        M.get_or_init(Met::compute)
    }

    fn compute() -> Met {
        let n = MAX_AWAY;
        let mut m = Met {
            pre: vec![vec![0.5; n + 1]; n + 1],
            post: vec![0.5; n + 1],
        };
        // 크로포드 이후(리더 1점): 트레일러 남은 점수가 작은 쪽부터.
        for b in 2..=n {
            let mut memo = HashMap::new();
            let v = m
                .game(
                    1,
                    b,
                    1,
                    Owner::Centered,
                    Phase::Post,
                    Gammons::model(),
                    &mut memo,
                )
                .at(0.5);
            m.post[b] = v;
        }
        // 크로포드 이전: a + b가 작은 쪽부터(다음 판 상태는 항상 합이 더 작다).
        for s in 2..=2 * n {
            for a in 1..=n {
                let b = s as i64 - a as i64;
                if b < 1 || b as usize > n {
                    continue;
                }
                let b = b as usize;
                let v = if a == 1 && b == 1 {
                    0.5
                } else if a == 1 {
                    m.crawford_leader(b)
                } else if b == 1 {
                    1.0 - m.crawford_leader(a)
                } else {
                    let mut memo = HashMap::new();
                    m.game(
                        a,
                        b,
                        1,
                        Owner::Centered,
                        Phase::Pre,
                        Gammons::model(),
                        &mut memo,
                    )
                    .at(0.5)
                };
                m.pre[a][b] = v;
            }
        }
        m
    }

    /// 크로포드 판(큐브 없음): 리더(1점 남음)의 MWC, 트레일러 b점 남음.
    fn crawford_leader(&self, b: usize) -> f64 {
        let g = GAMMON_RATE;
        0.5 + 0.5
            * ((1.0 - g) * self.next(1, b as i64 - 1, Phase::Post)
                + g * self.next(1, b as i64 - 2, Phase::Post))
    }

    /// 판이 끝난 뒤(A가 a, B가 b점 남음) 다음 판 시작 시점 A의 MWC.
    pub fn next(&self, a: i64, b: i64, phase: Phase) -> f64 {
        if a <= 0 {
            return 1.0;
        }
        if b <= 0 {
            return 0.0;
        }
        let (a, b) = (
            a.min(MAX_AWAY as i64) as usize,
            b.min(MAX_AWAY as i64) as usize,
        );
        if a == 1 && b == 1 {
            return 0.5;
        }
        match phase {
            Phase::Post if a == 1 => self.post[b],
            Phase::Post if b == 1 => 1.0 - self.post[a],
            // 크로포드 이후인데 둘 다 1점이 아니면(트레일러가 따라온 경우 포함) 일반 표를 쓴다.
            _ => self.pre[a][b],
        }
    }

    /// 이번 판이 끝난 뒤의 단계. 크로포드 판이 끝나면 그 뒤로는 Post.
    /// (Pre에서 누가 처음 1점 남게 되면 다음 판이 크로포드 판이고, 그 값은 `pre[1][b]`에 들어 있다.)
    fn phase_after(phase: Phase, crawford_game: bool) -> Phase {
        if crawford_game { Phase::Post } else { phase }
    }

    /// 판 진행 중 A의 MWC를 p(A의 승률)의 함수로. v = 큐브 값, owner = A 기준 소유.
    #[allow(clippy::too_many_arguments)]
    fn game(
        &self,
        a: usize,
        b: usize,
        v: usize,
        owner: Owner,
        phase: Phase,
        g: Gammons,
        memo: &mut Memo,
    ) -> Pl {
        let key = (a, b, v, owner, phase, g.key());
        if let Some(f) = memo.get(&key) {
            return f.clone();
        }
        let (ai, bi, vi) = (a as i64, b as i64, v as i64);
        // 크로포드 판이 아니고, 큐브 값이 그쪽 필요 점수보다 작을 때만 더블이 의미 있다.
        let crawford_game = phase == Phase::Pre && (a == 1 || b == 1);
        let after = Met::phase_after(phase, crawford_game);
        let next = |da: i64, db: i64| self.next(ai - da, bi - db, after);
        let end_win =
            (1.0 - g.gw - g.bgw) * next(vi, 0) + g.gw * next(2 * vi, 0) + g.bgw * next(3 * vi, 0);
        let end_loss =
            (1.0 - g.gl - g.bgl) * next(0, vi) + g.gl * next(0, 2 * vi) + g.bgl * next(0, 3 * vi);
        let a_can = !crawford_game && owner != Owner::Opp && v < a;
        let b_can = !crawford_game && owner != Owner::Me && v < b;

        let mut right = (1.0, end_win);
        let mut left = (0.0, end_loss);
        let mut a_cash: Option<(f64, f64)> = None;
        let mut b_cash: Option<(f64, f64)> = None;
        if a_can {
            let taken = self.game(a, b, 2 * v, Owner::Opp, phase, g, memo);
            let pass = next(vi, 0);
            a_cash = Some((taken.solve(pass), pass));
        }
        if b_can {
            let taken = self.game(a, b, 2 * v, Owner::Me, phase, g, memo);
            let pass = next(0, vi);
            b_cash = Some((taken.solve(pass), pass));
        }
        // 캐시할지(상대의 테이크 포인트에서 더블) 개먼을 노리고 계속 둘지를 번갈아 정한다.
        let line = |l: (f64, f64), r: (f64, f64), p: f64| {
            if r.0 > l.0 {
                l.1 + (r.1 - l.1) * (p - l.0) / (r.0 - l.0)
            } else {
                r.1
            }
        };
        for _ in 0..3 {
            if let Some(c) = a_cash {
                let play_on = line(left, (1.0, end_win), c.0);
                right = if c.1 >= play_on { c } else { (1.0, end_win) };
            }
            if let Some(c) = b_cash {
                let play_on = line((0.0, end_loss), right, c.0);
                left = if c.1 <= play_on { c } else { (0.0, end_loss) };
            }
        }
        let f = if left.0 < right.0 {
            let mut pts = vec![];
            if left.0 > 0.0 {
                pts.push((0.0, left.1));
            }
            pts.push(left);
            pts.push(right);
            if right.0 < 1.0 {
                pts.push((1.0, right.1));
            }
            Pl(pts)
        } else {
            // 두 쪽 캐시 구간이 겹치는 퇴화 경우: 죽은 큐브로 둔다.
            Pl(vec![(0.0, end_loss), (1.0, end_win)])
        };
        memo.insert(key, f.clone());
        f
    }
}

/// 매치 상황(수를 판단하는 쪽 기준): 남은 점수, 큐브, 크로포드 단계.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct MatchCtx {
    /// 내가 이기려면 필요한 점수, 상대가 필요한 점수.
    pub me: u8,
    pub opp: u8,
    pub cube: u8,
    /// Pre: 크로포드 판 전(지금이 크로포드 판일 수도 있음), Post: 크로포드 판이 지남.
    pub phase: Phase,
}

impl MatchCtx {
    /// 이 판이 크로포드 판인가(큐브 사용 금지): 크로포드 전인데 한쪽이 1점 남음.
    pub fn is_crawford(&self) -> bool {
        self.phase == Phase::Pre && (self.me == 1 || self.opp == 1)
    }

    pub fn swapped(&self) -> MatchCtx {
        MatchCtx {
            me: self.opp,
            opp: self.me,
            ..*self
        }
    }

    fn after(&self, da: i64, db: i64) -> f64 {
        let (na, nb) = (self.me as i64 - da, self.opp as i64 - db);
        Met::get().next(na, nb, Met::phase_after(self.phase, self.is_crawford()))
    }

    /// 큐브 없이(죽은 큐브) 이 판의 결과 확률에서 얻는 MWC.
    pub fn mwc_dead(&self, p: &OutcomeProbs) -> f64 {
        let v = self.cube as i64;
        let (w, wg, wbg) = (p.win as f64, p.win_gammon as f64, p.win_backgammon as f64);
        let (l, lg, lbg) = (1.0 - w, p.lose_gammon as f64, p.lose_backgammon as f64);
        (w - wg) * self.after(v, 0)
            + (wg - wbg) * self.after(2 * v, 0)
            + wbg * self.after(3 * v, 0)
            + (l - lg) * self.after(0, v)
            + (lg - lbg) * self.after(0, 2 * v)
            + lbg * self.after(0, 3 * v)
    }

    /// MWC → 정규화 에퀴티(EMG): 현재 큐브로 한 판 이기면 +1, 지면 −1.
    pub fn emg(&self, mwc: f64) -> f32 {
        let v = self.cube as i64;
        let (win, lose) = (self.after(v, 0), self.after(0, v));
        if win - lose < 1e-12 {
            return 0.0;
        }
        (2.0 * (mwc - lose) / (win - lose) - 1.0) as f32
    }

    /// 판 진행 중 큐브 있는 MWC(Janowski 방식). owner는 이쪽 기준.
    pub fn mwc_cubeful(&self, p: &OutcomeProbs, cube: u8, owner: Owner, x: f32) -> f64 {
        let ctx = MatchCtx { cube, ..*self };
        let mut memo = HashMap::new();
        let live = Met::get()
            .game(
                self.me as usize,
                self.opp as usize,
                cube as usize,
                owner,
                self.phase,
                Gammons::from_probs(p),
                &mut memo,
            )
            .at(p.win as f64);
        let x = x as f64;
        x * live + (1.0 - x) * ctx.mwc_dead(p)
    }

    /// 점수 상황 큐브 판단. 값은 EMG(현재 큐브 기준)로 돌려줘서 머니 판단과 같은 눈금으로 비교한다.
    pub fn cube_decision(&self, p: &OutcomeProbs, owner: Owner, x: f32) -> CubeDecision {
        let v = self.cube;
        let can = !self.is_crawford() && owner != Owner::Opp && (v as u16) < self.me as u16;
        let nd = self.mwc_cubeful(p, v, owner, x);
        let dt = self.mwc_cubeful(p, v.saturating_mul(2), Owner::Opp, x);
        let dp = self.after(v as i64, 0);
        let take = dt <= dp;
        let best = dt.min(dp);
        let double = can && best > nd;
        let action = if !can {
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
        // 테이크 포인트: 상대가 테이크와 패스가 같아지는 내 승률(살아 있는 큐브 모델, 결과 분포는 이 국면 것).
        let mut memo = HashMap::new();
        let taken = Met::get().game(
            self.me as usize,
            self.opp as usize,
            (v as usize) * 2,
            Owner::Opp,
            self.phase,
            Gammons::from_probs(p),
            &mut memo,
        );
        let cash_point = taken.solve(dp) as f32;
        let flipped = Met::get().game(
            self.opp as usize,
            self.me as usize,
            (v as usize) * 2,
            Owner::Opp,
            self.phase,
            Gammons::from_probs(&p.invert()),
            &mut memo,
        );
        let take_point = 1.0 - flipped.solve(self.swapped().after(v as i64, 0)) as f32;
        CubeDecision {
            action,
            double,
            take,
            no_double: self.emg(nd),
            double_take: self.emg(dt),
            double_pass: self.emg(dp),
            double_gain: self.emg(best) - self.emg(nd),
            take_point,
            cash_point,
            efficiency: x,
            probs: *p,
        }
    }
}

/// 표 일부를 사람이 읽을 수 있게(디버깅·문서용). 행 = 내가 남은 점수, 열 = 상대가 남은 점수.
pub fn met_table(n: usize) -> Vec<Vec<f64>> {
    let m = Met::get();
    (1..=n)
        .map(|a| {
            (1..=n)
                .map(|b| m.next(a as i64, b as i64, Phase::Pre))
                .collect()
        })
        .collect()
}

/// 크로포드 이후 리더(1점 남음)의 MWC, 트레일러가 b점 남음.
pub fn post_crawford_leader(b: usize) -> f64 {
    Met::get().next(1, b as i64, Phase::Post)
}
