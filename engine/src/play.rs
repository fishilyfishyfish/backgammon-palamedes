//! 대국 진행과 강도 측정. 미러 대국: 같은 주사위 열로 자리를 바꿔 두 판을 둔다.

use crate::board::{Board, Side};
use crate::eval::Evaluator;
use crate::movegen::{Move, legal_moves};
use crate::rng::Rng;
use crate::search::{Search, best_move, rank_moves_ply};

pub trait Player {
    fn name(&self) -> String;
    fn choose(&mut self, b: &Board, side: Side, d1: u8, d2: u8) -> Move;
}

/// 합법수 중 무작위. 주사위와 다른 난수열을 쓴다(미러 대국에서 주사위가 같도록).
pub struct RandomPlayer {
    rng: Rng,
}

impl RandomPlayer {
    pub fn new(seed: u64) -> RandomPlayer {
        RandomPlayer {
            rng: Rng::new(seed),
        }
    }
}

impl Player for RandomPlayer {
    fn name(&self) -> String {
        "random".into()
    }
    fn choose(&mut self, b: &Board, side: Side, d1: u8, d2: u8) -> Move {
        let moves = legal_moves(b, side, d1, d2);
        moves[self.rng.below(moves.len() as u64) as usize]
    }
}

/// 평가함수 + 1수 탐색.
pub struct OnePly<E: Evaluator> {
    pub eval: E,
    pub label: String,
}

impl<E: Evaluator> OnePly<E> {
    pub fn new(eval: E, label: &str) -> Self {
        OnePly {
            eval,
            label: label.into(),
        }
    }
}

impl<E: Evaluator> Player for OnePly<E> {
    fn name(&self) -> String {
        self.label.clone()
    }
    fn choose(&mut self, b: &Board, side: Side, d1: u8, d2: u8) -> Move {
        best_move(&self.eval, b, side, d1, d2).mv
    }
}

/// 평가함수 + 설정한 깊이의 탐색(2수면 후보 거르기 포함).
pub struct Searcher<E: Evaluator + Clone> {
    pub eval: E,
    pub search: Search,
    pub label: String,
}

impl<E: Evaluator + Clone> Player for Searcher<E> {
    fn name(&self) -> String {
        self.label.clone()
    }
    fn choose(&mut self, b: &Board, side: Side, d1: u8, d2: u8) -> Move {
        rank_moves_ply(&self.eval, b, side, d1, d2, self.search, None)
            .swap_remove(0)
            .mv
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GameResult {
    pub winner: Side,
    pub points: u8,
    pub turns: u32,
}

/// 한 판. 첫 수는 서로 하나씩 굴려 높은 쪽이 두 눈으로 시작(같으면 다시).
pub fn play_game(white: &mut dyn Player, black: &mut dyn Player, seed: u64) -> GameResult {
    let mut dice = Rng::new(seed);
    let (mut d1, mut d2) = loop {
        let (w, b) = dice.roll();
        if w != b {
            break (w, b);
        }
    };
    let mut side = if d1 > d2 { Side::White } else { Side::Black };
    let mut b = Board::start();
    let mut turns = 0;
    loop {
        let m = match side {
            Side::White => white.choose(&b, side, d1, d2),
            Side::Black => black.choose(&b, side, d1, d2),
        };
        b.apply_steps(side, m.steps());
        turns += 1;
        if let Some((winner, points)) = b.result() {
            return GameResult {
                winner,
                points,
                turns,
            };
        }
        side = side.opp();
        (d1, d2) = dice.roll();
    }
}

#[derive(Clone, Debug, Default)]
pub struct MatchStats {
    pub pairs: u32,
    /// A 기준 누적 점수(이기면 +, 지면 -).
    pub a_points: i64,
    pub a_wins: u32,
    pub games: u32,
    pub a_gammons: u32,
    pub b_gammons: u32,
    sum_sq_pair: f64,
}

impl MatchStats {
    pub fn merge(&mut self, o: &MatchStats) {
        self.pairs += o.pairs;
        self.a_points += o.a_points;
        self.a_wins += o.a_wins;
        self.games += o.games;
        self.a_gammons += o.a_gammons;
        self.b_gammons += o.b_gammons;
        self.sum_sq_pair += o.sum_sq_pair;
    }

    /// 한 판당 평균 점수(A 기준).
    pub fn ppg(&self) -> f64 {
        self.a_points as f64 / self.games as f64
    }
    /// ppg의 표준오차. 미러 쌍을 독립 표본으로 본다.
    pub fn ppg_stderr(&self) -> f64 {
        let n = self.pairs as f64;
        let mean_pair = self.a_points as f64 / n;
        let var = (self.sum_sq_pair / n - mean_pair * mean_pair).max(0.0) * n / (n - 1.0).max(1.0);
        (var / n).sqrt() / 2.0
    }
    pub fn win_rate(&self) -> f64 {
        self.a_wins as f64 / self.games as f64
    }
}

/// A와 B가 `pairs`쌍(2×pairs판)의 미러 대국을 둔다. 쌍 i는 시드 seed0 + i의 주사위를 공유.
pub fn mirror_match(a: &mut dyn Player, b: &mut dyn Player, pairs: u32, seed0: u64) -> MatchStats {
    let mut st = MatchStats::default();
    for i in 0..pairs {
        let seed = seed0.wrapping_add(i as u64);
        let mut pair = 0i64;
        for a_side in [Side::White, Side::Black] {
            let r = if a_side == Side::White {
                play_game(a, b, seed)
            } else {
                play_game(b, a, seed)
            };
            let a_won = r.winner == a_side;
            let pts = r.points as i64;
            pair += if a_won { pts } else { -pts };
            st.games += 1;
            if a_won {
                st.a_wins += 1;
                if pts >= 2 {
                    st.a_gammons += 1;
                }
            } else if pts >= 2 {
                st.b_gammons += 1;
            }
        }
        st.pairs += 1;
        st.a_points += pair;
        st.sum_sq_pair += (pair * pair) as f64;
    }
    st
}

/// `mirror_match`를 스레드로 나눠 돈다. 쌍 i는 어느 스레드에서 돌든 시드 seed0 + i를 쓴다.
/// 플레이어는 스레드마다 `make_a(t)`, `make_b(t)`로 새로 만든다.
pub fn parallel_mirror_match<A, B>(
    make_a: A,
    make_b: B,
    pairs: u32,
    seed0: u64,
    threads: usize,
) -> MatchStats
where
    A: Fn(usize) -> Box<dyn Player> + Sync,
    B: Fn(usize) -> Box<dyn Player> + Sync,
{
    let threads = threads.max(1);
    let chunk = pairs.div_ceil(threads as u32);
    let parts: Vec<MatchStats> = std::thread::scope(|sc| {
        let hs: Vec<_> = (0..threads)
            .map(|t| {
                let (make_a, make_b) = (&make_a, &make_b);
                sc.spawn(move || {
                    let start = t as u32 * chunk;
                    let n = chunk.min(pairs.saturating_sub(start));
                    let (mut a, mut b) = (make_a(t), make_b(t));
                    mirror_match(a.as_mut(), b.as_mut(), n, seed0.wrapping_add(start as u64))
                })
            })
            .collect();
        hs.into_iter()
            .map(|h| h.join().expect("match thread"))
            .collect()
    });
    let mut st = MatchStats::default();
    for p in &parts {
        st.merge(p);
    }
    st
}

// ---------- 큐브가 있는 판과 매치 ----------

use crate::cube::{CubeState, decide};
use crate::matchplay::{MatchCtx, Phase};
use crate::search::Objective;

/// 큐브 정책. Never: 더블 안 하고 항상 받음. Janowski: `cube::decide`대로.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CubePolicy {
    Never,
    Janowski,
    /// 점수를 무시하고 머니 판단을 그대로 쓴다(매치에서 비교용).
    MoneyOnly,
}

pub trait CubePlayer {
    fn name(&self) -> String;
    fn choose(&mut self, b: &Board, side: Side, d1: u8, d2: u8, ctx: Option<MatchCtx>) -> Move;
    /// 굴리기 전 더블할지.
    fn double(&mut self, b: &Board, side: Side, cube: CubeState, ctx: Option<MatchCtx>) -> bool;
    /// `side`가 더블을 받았을 때 받을지. `ctx`는 `side` 기준.
    fn take(&mut self, b: &Board, side: Side, cube: CubeState, ctx: Option<MatchCtx>) -> bool;
}

pub struct CubeAgent<E: Evaluator + Clone> {
    pub eval: E,
    pub search: Search,
    pub policy: CubePolicy,
    pub label: String,
}

impl<E: Evaluator + Clone> CubeAgent<E> {
    fn ctx_for_policy(&self, ctx: Option<MatchCtx>) -> Option<MatchCtx> {
        if self.policy == CubePolicy::MoneyOnly {
            None
        } else {
            ctx
        }
    }
}

impl<E: Evaluator + Clone> CubePlayer for CubeAgent<E> {
    fn name(&self) -> String {
        self.label.clone()
    }

    fn choose(&mut self, b: &Board, side: Side, d1: u8, d2: u8, ctx: Option<MatchCtx>) -> Move {
        let obj = self
            .ctx_for_policy(ctx)
            .map(Objective::Match)
            .unwrap_or(Objective::Money);
        rank_moves_ply(
            &self.eval,
            b,
            side,
            d1,
            d2,
            self.search.with_objective(obj),
            None,
        )
        .swap_remove(0)
        .mv
    }

    fn double(&mut self, b: &Board, side: Side, cube: CubeState, ctx: Option<MatchCtx>) -> bool {
        if self.policy == CubePolicy::Never {
            return false;
        }
        decide(&self.eval, b, side, cube, self.ctx_for_policy(ctx)).double
    }

    fn take(&mut self, b: &Board, side: Side, cube: CubeState, ctx: Option<MatchCtx>) -> bool {
        if self.policy == CubePolicy::Never {
            return true;
        }
        // 더블한 쪽 입장에서 판단하고, 그쪽 기준 "상대가 받아야 하는가"를 쓴다.
        let doubler = side.opp();
        let dctx = self.ctx_for_policy(ctx).map(|c| c.swapped());
        decide(&self.eval, b, doubler, cube, dctx).take
    }
}

/// 큐브가 있는 한 판의 결과.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CubeGameResult {
    pub winner: Side,
    /// 이긴 쪽이 얻은 점수(큐브 × 1/2/3). 패스면 큐브 값.
    pub points: u32,
    pub cube: u8,
    pub passed: bool,
}

/// 매치 진행 상황. length = 0이면 머니 게임.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MatchScore {
    pub length: u8,
    pub score: [u8; 2],
    pub phase: Phase,
}

impl MatchScore {
    pub fn ctx(&self, side: Side, cube: u8) -> Option<MatchCtx> {
        (self.length > 0).then(|| MatchCtx {
            me: self.length - self.score[side.idx()],
            opp: self.length - self.score[side.opp().idx()],
            cube,
            phase: self.phase,
        })
    }
}

/// 큐브가 있는 한 판. 첫 수는 서로 하나씩 굴려 정하고, 그 뒤 매 차례 굴리기 전에 더블할 수 있다.
pub fn play_cube_game(
    players: [&mut dyn CubePlayer; 2],
    seed: u64,
    score: Option<MatchScore>,
) -> CubeGameResult {
    let [p0, p1] = players;
    let ps: [&mut dyn CubePlayer; 2] = [p0, p1];
    let mut dice = Rng::new(seed);
    let (mut d1, mut d2) = loop {
        let (w, b) = dice.roll();
        if w != b {
            break (w, b);
        }
    };
    let mut side = if d1 > d2 { Side::White } else { Side::Black };
    let mut b = Board::start();
    let mut cube = CubeState::default();
    let crawford = score.is_some_and(|s| s.ctx(Side::White, 1).is_some_and(|c| c.is_crawford()));
    let ctx = |s: Side, c: &CubeState| score.and_then(|m| m.ctx(s, c.value));
    let mut first = true;
    loop {
        if !first {
            // 크로포드 판에서는 큐브를 못 쓴다. 매치에서 이미 이길 만큼 큐브가 커졌으면 더 올리지 않는다.
            let useful = ctx(side, &cube).is_none_or(|c| (cube.value as u16) < c.me as u16);
            if !crawford && cube.can_double(side) && cube.value < 64 && useful {
                let i = side.idx();
                if ps[i].double(&b, side, cube, ctx(side, &cube)) {
                    let o = side.opp();
                    if ps[o.idx()].take(&b, o, cube, ctx(o, &cube)) {
                        cube = CubeState {
                            value: cube.value * 2,
                            owner: Some(o),
                        };
                    } else {
                        return CubeGameResult {
                            winner: side,
                            points: cube.value as u32,
                            cube: cube.value,
                            passed: true,
                        };
                    }
                }
            }
            (d1, d2) = dice.roll();
        }
        first = false;
        let m = ps[side.idx()].choose(&b, side, d1, d2, ctx(side, &cube));
        b.apply_steps(side, m.steps());
        if let Some((w, mult)) = b.result() {
            return CubeGameResult {
                winner: w,
                points: cube.value as u32 * mult as u32,
                cube: cube.value,
                passed: false,
            };
        }
        side = side.opp();
    }
}

/// 한 매치. 판마다 시드 seed·1000 + 판 번호. 이긴 쪽과 판 수.
pub fn play_match(players: [&mut dyn CubePlayer; 2], length: u8, seed: u64) -> (Side, u32) {
    let [p0, p1] = players;
    let mut ps: [&mut dyn CubePlayer; 2] = [p0, p1];
    let mut sc = MatchScore {
        length,
        score: [0, 0],
        phase: Phase::Pre,
    };
    for g in 0.. {
        let crawford = sc.ctx(Side::White, 1).unwrap().is_crawford();
        let [a, b] = &mut ps;
        let r = play_cube_game(
            [&mut **a, &mut **b],
            seed.wrapping_mul(1000).wrapping_add(g as u64),
            Some(sc),
        );
        let w = r.winner.idx();
        sc.score[w] = (sc.score[w] as u32 + r.points).min(length as u32) as u8;
        if crawford {
            sc.phase = Phase::Post;
        }
        if sc.score[w] >= length {
            return (r.winner, g + 1);
        }
    }
    unreachable!()
}
