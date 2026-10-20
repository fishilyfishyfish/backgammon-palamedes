mod common;

use common::{board, w2b};
use palamedes::board::{Board, Side};
use palamedes::compute_features;
use palamedes::features::shot_table;

#[test]
fn start_features() {
    let f = compute_features(&Board::start());
    for s in &f.sides {
        assert_eq!(s.pips, 167);
        assert!(s.blots.is_empty());
        assert_eq!(s.total_shots, 0);
        assert_eq!(s.made_points, vec![6, 8, 13, 24]);
        assert_eq!(s.anchors, vec![24]);
        assert!(!s.five_point && !s.bar_point);
        assert_eq!(s.prime_len, 1);
        assert_eq!(s.home_points, 1);
        assert_eq!(s.back_checkers, 2);
        assert_eq!(s.on_bar, 0);
    }
    assert!(f.outcome.is_none());
}

/// White 블롯 하나(10)와, 그보다 k칸 뒤에 있는 Black 말 하나. Black의 나머지 말은
/// 블롯을 지나친 곳(Black 홈)에 있어 주사위를 흡수만 한다.
fn single_shot_board(k: usize, white_extra: &[(usize, u8)]) -> palamedes::Board {
    let mut white = vec![(10, 1), (1, 2)];
    white.extend_from_slice(white_extra);
    board(&white, &[(w2b(10 - k), 1), (w2b(19), 14)])
}

#[test]
fn direct_shot_one_away_is_11() {
    let b = single_shot_board(1, &[]);
    let t = shot_table(&b, Side::White);
    assert_eq!(t.per_point[10], 11);
    assert_eq!(t.any, 11);
    let f = compute_features(&b);
    let blot = &f.sides[0].blots[0];
    assert_eq!(
        (blot.point, blot.nearest_attacker, blot.shots),
        (10, Some(1), 11)
    );
}

#[test]
fn six_away_is_17() {
    let b = single_shot_board(6, &[]);
    assert_eq!(shot_table(&b, Side::White).per_point[10], 17);
}

#[test]
fn all_distances_without_blocks() {
    // 막는 칸 없을 때의 잘 알려진 직접/간접 샷 수.
    let expected = [11, 12, 14, 15, 15, 17, 6, 6, 5, 3, 2, 3];
    for (i, &e) in expected.iter().enumerate() {
        let k = i + 1;
        let b = board(&[(15, 1), (1, 2)], &[(w2b(15 - k), 1), (w2b(19), 14)]);
        assert_eq!(shot_table(&b, Side::White).per_point[15], e, "distance {k}");
    }
}

#[test]
fn blocked_points_reduce_shots() {
    // 6칸 거리, White가 7과 6에 포인트: 3-3(4→7 막힘), 2-2(4→6 막힘) 사라지고 4-2는 8 경유로 가능.
    // 11(6 포함) + 5-1(2) + 4-2(2) = 15.
    let b = single_shot_board(6, &[(7, 2), (6, 2)]);
    assert_eq!(shot_table(&b, Side::White).per_point[10], 15);
}

#[test]
fn bar_checker_must_enter_first() {
    // Black 말 하나가 바에 있고, White 블롯은 White 기준 3(Black이 3으로 들어오는 칸).
    // 3이 포함된 11개 + 2-1(2) + 1-1(1로 들어와 1, 1) = 14.
    let b = board(&[(3, 1), (13, 2)], &[(palamedes::BAR, 1), (w2b(19), 14)]);
    assert_eq!(shot_table(&b, Side::White).per_point[3], 14);
    // White가 2에 포인트를 만들면 1-1(1로 들어와 2를 거쳐야 함)이 사라지고,
    // 2-1은 1로 들어와 2를 쓰는 길이 남아 그대로 → 13.
    let b = board(&[(3, 1), (2, 2)], &[(palamedes::BAR, 1), (w2b(19), 14)]);
    assert_eq!(shot_table(&b, Side::White).per_point[3], 13);
}

#[test]
fn made_points_prime_anchor() {
    let b = board(
        &[
            (4, 2),
            (5, 2),
            (6, 2),
            (7, 2),
            (8, 2),
            (20, 2),
            (13, 1),
            (BAR_IDX, 1),
        ],
        &[(w2b(12), 2)],
    );
    let f = compute_features(&b).sides[0].clone();
    assert!(f.five_point && f.bar_point);
    assert_eq!(f.prime_len, 5);
    assert_eq!(f.prime_range, (4, 8));
    assert_eq!(f.anchors, vec![20]);
    assert_eq!(f.home_points, 3);
    assert_eq!(f.back_checkers, 2);
    assert_eq!(f.on_bar, 1);
    assert_eq!(f.blots.len(), 1);
}

const BAR_IDX: usize = palamedes::BAR;
