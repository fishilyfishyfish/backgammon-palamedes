//! 대국 진행과 강도 측정. 미러 대국: 같은 주사위 열로 자리를 바꿔 두 판을 둔다.

use crate::board::{Board, Side};
use crate::eval::Evaluator;
use crate::movegen::{Move, legal_moves};
use crate::rng::Rng;
use crate::search::best_move;

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
