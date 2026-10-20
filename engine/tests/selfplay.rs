//! 무작위 자기대국으로 불변식을 검사한다.

use std::collections::HashSet;

use palamedes::board::{Board, Side};
use palamedes::movegen::{legal_moves_bruteforce, legal_moves_with_boards, next_steps};
use palamedes::rng::Rng;

/// next_steps로 한 걸음씩 끝까지 가 본 모든 결과 국면.
fn ui_reachable(start: &Board, side: Side, d1: u8, d2: u8) -> HashSet<Board> {
    fn rec(
        start: &Board,
        side: Side,
        d1: u8,
        d2: u8,
        played: &mut Vec<palamedes::Step>,
        out: &mut HashSet<Board>,
    ) {
        let next = next_steps(start, side, d1, d2, played).expect("valid prefix");
        if next.is_empty() {
            let mut b = *start;
            b.apply_steps(side, played);
            out.insert(b);
            return;
        }
        for s in next {
            played.push(s);
            rec(start, side, d1, d2, played, out);
            played.pop();
        }
    }
    let mut out = HashSet::new();
    rec(start, side, d1, d2, &mut Vec::new(), &mut out);
    out
}

#[test]
fn random_games_invariants() {
    let mut rng = Rng::new(12345);
    let games = if cfg!(debug_assertions) { 25 } else { 400 };
    let mut gammons = 0;
    for g in 0..games {
        let mut b = Board::start();
        let mut side = Side::White;
        let mut turns = 0;
        loop {
            let (d1, d2) = rng.roll();
            let moves = legal_moves_with_boards(&b, side, d1, d2);
            assert!(!moves.is_empty());
            let n = moves[0].0.len();
            assert!(
                moves.iter().all(|(m, _)| m.len() == n),
                "all moves use the same number of dice"
            );

            // 더블 가지치기 결과 == 전수 탐색 결과.
            let fast: HashSet<Board> = moves.iter().map(|(_, nb)| *nb).collect();
            assert_eq!(fast.len(), moves.len(), "deduped");
            if g < 20 {
                let brute: HashSet<Board> = legal_moves_bruteforce(&b, side, d1, d2)
                    .iter()
                    .map(|(_, nb)| *nb)
                    .collect();
                assert_eq!(fast, brute, "board {:?} side {:?} dice {d1}{d2}", b, side);
                // UI 걸음 생성으로 도달 가능한 국면 == 합법수 결과 국면.
                assert_eq!(
                    ui_reachable(&b, side, d1, d2),
                    fast,
                    "ui board {:?} side {:?} dice {d1}{d2}",
                    b,
                    side
                );
            }

            // 적용 후 되돌리면 원래 국면.
            for (m, nb) in &moves {
                let mut t = b;
                t.apply_steps(side, m.steps());
                assert_eq!(t, *nb);
                assert!(t.is_valid());
                t.undo_steps(side, m.steps());
                assert_eq!(t, b);
            }

            b = moves[rng.below(moves.len() as u64) as usize].1;
            if let Some((w, pts)) = b.result() {
                assert_eq!(w, side, "only the mover can win");
                if pts >= 2 {
                    gammons += 1;
                }
                break;
            }
            side = side.opp();
            turns += 1;
            assert!(turns < 5000, "game too long");
        }
    }
    assert!(gammons > 0, "random play should produce some gammons");
}
