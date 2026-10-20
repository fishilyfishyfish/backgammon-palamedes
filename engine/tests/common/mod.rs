#![allow(dead_code)]

use palamedes::board::{Board, OFF, Side};

/// 각 진영의 (자기 기준 포인트, 개수) 목록으로 판을 만든다. 나머지는 베어오프로 채운다.
pub fn board(white: &[(usize, u8)], black: &[(usize, u8)]) -> Board {
    let mut b = Board::empty();
    for (s, list) in [(0, white), (1, black)] {
        for &(p, n) in list {
            b.pos[s][p] += n;
        }
        let used: u8 = b.pos[s].iter().sum();
        b.pos[s][OFF] += 15 - used;
    }
    assert!(b.is_valid(), "test board invalid: {:?}", b);
    b
}

/// White 기준 포인트 p를 Black 기준 포인트로.
pub fn w2b(p: usize) -> usize {
    25 - p
}

pub fn notations(b: &Board, side: Side, d1: u8, d2: u8) -> Vec<String> {
    let mut v: Vec<String> = palamedes::legal_moves(b, side, d1, d2)
        .iter()
        .map(|m| palamedes::notation::format_move(m.steps()))
        .collect();
    v.sort();
    v
}
