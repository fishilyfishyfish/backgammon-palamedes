//! 합법수 생성.
//!
//! - `legal_moves`: 결과 국면이 같은 수는 하나로 합친 목록(AI·탐색용).
//! - `turn_options`: 사람이 한 걸음씩 두는 UI용. 지금까지 둔 걸음 뒤에 이어서 둘 수 있는
//!   걸음(같은 말로 여러 눈을 연달아 쓰는 복합 이동 포함)을 돌려준다.

use std::collections::HashSet;

use serde::Serialize;

pub use crate::board::Step;
use crate::board::{BAR, Board, Side};

/// 한 턴의 수. 최대 4걸음. 걸음이 0개면 패스(둘 수 있는 수가 없음).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Move {
    steps: [Step; 4],
    len: u8,
}

impl Move {
    pub fn new() -> Move {
        Move::default()
    }

    pub fn from_steps(steps: &[Step]) -> Move {
        let mut m = Move::new();
        for s in steps {
            m.push(*s);
        }
        m
    }

    #[inline]
    pub fn push(&mut self, s: Step) {
        self.steps[self.len as usize] = s;
        self.len += 1;
    }

    #[inline]
    pub fn pop(&mut self) {
        self.len -= 1;
    }

    #[inline]
    pub fn steps(&self) -> &[Step] {
        &self.steps[..self.len as usize]
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

fn dice_list(d1: u8, d2: u8) -> Vec<Vec<u8>> {
    if d1 == d2 {
        vec![vec![d1; 4]]
    } else {
        vec![vec![d1, d2], vec![d2, d1]]
    }
}

struct Gen {
    max_len: usize,
    leaves: Vec<(Move, Board)>,
}

/// 걸음을 DFS로 쌓는다. 더블이면 from이 감소하지 않는 순서만 본다(같은 걸음 집합의
/// 순서만 다른 경우를 잘라내기 위함; `tests/movegen.rs`에서 전수 탐색과 같은지 검사).
fn dfs(b: &mut Board, side: Side, dice: &[u8], cur: &mut Move, max_from: usize, g: &mut Gen) {
    let i = cur.len();
    let mut any = false;
    if i < dice.len() {
        for from in (1..=max_from).rev() {
            if let Some(s) = b.step_for(side, from, dice[i]) {
                any = true;
                b.apply_step(side, &s);
                cur.push(s);
                dfs(b, side, dice, cur, max_from_next(dice, from), g);
                cur.pop();
                b.undo_step(side, &s);
            }
        }
    }
    if !any {
        if i > g.max_len {
            g.max_len = i;
            g.leaves.clear();
        }
        if i == g.max_len {
            g.leaves.push((*cur, *b));
        }
    }
}

#[inline]
fn max_from_next(dice: &[u8], from: usize) -> usize {
    if dice.len() == 4 { from } else { BAR }
}

/// 규칙에 맞는 모든 수. 결과 국면이 같은 수는 하나만 남긴다.
/// 둘 수 있는 수가 없으면 빈 수 하나를 돌려준다.
pub fn legal_moves(board: &Board, side: Side, d1: u8, d2: u8) -> Vec<Move> {
    legal_moves_with_boards(board, side, d1, d2)
        .into_iter()
        .map(|(m, _)| m)
        .collect()
}

/// `legal_moves`와 같지만 결과 국면도 같이 준다.
pub fn legal_moves_with_boards(board: &Board, side: Side, d1: u8, d2: u8) -> Vec<(Move, Board)> {
    let mut g = Gen {
        max_len: 0,
        leaves: Vec::new(),
    };
    let mut b = *board;
    for dice in dice_list(d1, d2) {
        dfs(&mut b, side, &dice, &mut Move::new(), BAR, &mut g);
    }
    filter_and_dedupe(g, d1, d2)
}

fn filter_and_dedupe(g: Gen, d1: u8, d2: u8) -> Vec<(Move, Board)> {
    let mut leaves = g.leaves;
    // 한 눈만 쓸 수 있으면 큰 눈을 써야 한다.
    if d1 != d2 && g.max_len == 1 {
        let hi = d1.max(d2);
        if leaves.iter().any(|(m, _)| m.steps()[0].die == hi) {
            leaves.retain(|(m, _)| m.steps()[0].die == hi);
        }
    }
    let mut seen = HashSet::with_capacity(leaves.len());
    leaves.retain(|(_, b)| seen.insert(*b));
    leaves
}

/// 검증용: 더블 가지치기 없이 모든 순서를 다 본다.
pub fn legal_moves_bruteforce(board: &Board, side: Side, d1: u8, d2: u8) -> Vec<(Move, Board)> {
    fn dfs_all(b: &mut Board, side: Side, dice: &[u8], cur: &mut Move, g: &mut Gen) {
        let i = cur.len();
        let mut any = false;
        if i < dice.len() {
            for from in 1..=BAR {
                if let Some(s) = b.step_for(side, from, dice[i]) {
                    any = true;
                    b.apply_step(side, &s);
                    cur.push(s);
                    dfs_all(b, side, dice, cur, g);
                    cur.pop();
                    b.undo_step(side, &s);
                }
            }
        }
        if !any {
            if i > g.max_len {
                g.max_len = i;
                g.leaves.clear();
            }
            if i == g.max_len {
                g.leaves.push((*cur, *b));
            }
        }
    }
    let mut g = Gen {
        max_len: 0,
        leaves: Vec::new(),
    };
    let mut b = *board;
    for dice in dice_list(d1, d2) {
        dfs_all(&mut b, side, &dice, &mut Move::new(), &mut g);
    }
    filter_and_dedupe(g, d1, d2)
}

/// 이 주사위로 이번 턴에 반드시 써야 하는 걸음 수.
pub fn dice_to_use(board: &Board, side: Side, d1: u8, d2: u8) -> usize {
    legal_moves(board, side, d1, d2)[0].len()
}

fn remaining_dice(d1: u8, d2: u8, played: &[Step]) -> Option<Vec<u8>> {
    let mut rem = if d1 == d2 { vec![d1; 4] } else { vec![d1, d2] };
    for s in played {
        let i = rem.iter().position(|&d| d == s.die)?;
        rem.remove(i);
    }
    Some(rem)
}

/// 남은 눈으로 `need` 걸음을 더 둘 수 있는가.
fn can_complete(b: &mut Board, side: Side, rem: &[u8], need: usize) -> bool {
    if need == 0 {
        return true;
    }
    for (i, &d) in rem.iter().enumerate() {
        if rem[..i].contains(&d) {
            continue;
        }
        let mut rest = rem.to_vec();
        rest.remove(i);
        for from in 1..=BAR {
            if let Some(s) = b.step_for(side, from, d) {
                b.apply_step(side, &s);
                let ok = can_complete(b, side, &rest, need - 1);
                b.undo_step(side, &s);
                if ok {
                    return true;
                }
            }
        }
    }
    false
}

/// 턴 시작 국면 `start`에서 `played`를 둔 뒤 이어서 둘 수 있는 한 걸음 목록.
/// `played`가 합법수의 앞부분이 아니면 None.
pub fn next_steps(start: &Board, side: Side, d1: u8, d2: u8, played: &[Step]) -> Option<Vec<Step>> {
    let need = dice_to_use(start, side, d1, d2);
    let mut b = *start;
    for k in 0..played.len() {
        let ok = next_steps_unchecked(&b, side, d1, d2, &played[..k], need)?;
        let p = played[k];
        let s = ok
            .into_iter()
            .find(|s| s.from == p.from && s.to == p.to && s.die == p.die)?;
        b.apply_step(side, &s);
    }
    next_steps_unchecked(&b, side, d1, d2, played, need)
}

fn next_steps_unchecked(
    b: &Board,
    side: Side,
    d1: u8,
    d2: u8,
    played: &[Step],
    need: usize,
) -> Option<Vec<Step>> {
    let rem = remaining_dice(d1, d2, played)?;
    if played.len() >= need {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    let mut b = *b;
    if d1 != d2 && need == 1 {
        let hi = d1.max(d2);
        let lo = d1.min(d2);
        let die = if (1..=BAR).any(|f| b.step_for(side, f, hi).is_some()) {
            hi
        } else {
            lo
        };
        out.extend((1..=BAR).rev().filter_map(|f| b.step_for(side, f, die)));
        return Some(out);
    }
    for (i, &d) in rem.iter().enumerate() {
        if rem[..i].contains(&d) {
            continue;
        }
        let mut rest = rem.clone();
        rest.remove(i);
        for from in (1..=BAR).rev() {
            if let Some(s) = b.step_for(side, from, d) {
                b.apply_step(side, &s);
                if can_complete(&mut b, side, &rest, need - played.len() - 1) {
                    out.push(s);
                }
                b.undo_step(side, &s);
            }
        }
    }
    Some(out)
}

/// UI에서 고를 수 있는 이동 하나. 같은 말을 여러 눈으로 연달아 옮기는 복합 이동이면 steps가 여럿.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct TurnOption {
    pub from: u8,
    pub to: u8,
    pub steps: Vec<Step>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TurnState {
    /// 이번 턴에 써야 하는 걸음 수.
    pub need: usize,
    /// 지금까지 둔 걸음 수.
    pub played: usize,
    /// 수가 완성되었는가.
    pub done: bool,
    pub options: Vec<TurnOption>,
    /// 지금 둘 수 있는 한 걸음 전부(쓴 눈 `die`가 다르면 따로). 웹의 "누르면 바로 이동" 입력이 눈을 골라 쓴다.
    pub next: Vec<Step>,
}

/// 같은 (from, to)로 가는 경로가 여럿이면 중간에 맞히는 횟수가 적은 것, 그다음 걸음이 적은 것을 고른다.
pub fn turn_options(
    start: &Board,
    side: Side,
    d1: u8,
    d2: u8,
    played: &[Step],
) -> Option<TurnState> {
    let need = dice_to_use(start, side, d1, d2);
    let first = next_steps(start, side, d1, d2, played)?;
    let mut options: Vec<TurnOption> = Vec::new();
    let mut path: Vec<Step> = played.to_vec();
    for &s in &first {
        path.push(s);
        collect_chain(start, side, d1, d2, &mut path, played.len(), &mut options);
        path.pop();
    }
    Some(TurnState {
        need,
        played: played.len(),
        done: played.len() >= need,
        options,
        next: first,
    })
}

fn collect_chain(
    start: &Board,
    side: Side,
    d1: u8,
    d2: u8,
    path: &mut Vec<Step>,
    base: usize,
    options: &mut Vec<TurnOption>,
) {
    let chain: Vec<Step> = path[base..].to_vec();
    let last = *chain.last().unwrap();
    let cand = TurnOption {
        from: chain[0].from,
        to: last.to,
        steps: chain.clone(),
    };
    let cost = |o: &TurnOption| {
        let mid_hits = o.steps[..o.steps.len() - 1]
            .iter()
            .filter(|s| s.hit)
            .count();
        (mid_hits, o.steps.len())
    };
    match options
        .iter_mut()
        .find(|o| o.from == cand.from && o.to == cand.to)
    {
        Some(o) if cost(&cand) < cost(o) => *o = cand,
        Some(_) => {}
        None => options.push(cand),
    }
    if last.to == 0 {
        return;
    }
    let Some(next) = next_steps(start, side, d1, d2, path) else {
        return;
    };
    for s in next.into_iter().filter(|s| s.from == last.to) {
        path.push(s);
        collect_chain(start, side, d1, d2, path, base, options);
        path.pop();
    }
}
