mod common;

use common::{board, notations, w2b};
use palamedes::board::{BAR, Board, Side};
use palamedes::legal_moves;
use palamedes::movegen::next_steps;

const W: Side = Side::White;

#[test]
fn start_position() {
    let b = Board::start();
    assert!(b.is_valid());
    assert_eq!(b.pips(Side::White), 167);
    assert_eq!(b.pips(Side::Black), 167);
    assert_eq!(Board::start().result(), None);
}

#[test]
fn opening_moves_notation() {
    let b = Board::start();
    let n31 = notations(&b, W, 3, 1);
    assert!(n31.contains(&"8/5 6/5".to_string()));
    let n65 = notations(&b, W, 6, 5);
    assert!(n65.contains(&"24/13".to_string()));
    let n66 = notations(&b, W, 6, 6);
    assert!(n66.contains(&"24/18(2) 13/7(2)".to_string()));
    // 6-6 시작: 24 두 개는 18로, 13은 7로, 8은 2로 갈 수 있다. 6에서 0은 홈 밖 말이 있어 불가.
    assert!(!n66.iter().any(|s| s.contains("off")));
}

#[test]
fn must_use_both_dice() {
    // White: 13, 10. Black이 White 기준 7, 2에 포인트.
    // 10/5를 먼저 두면 6을 쓸 수 없다(13/7 막힘, 5에서는 홈 밖 말이 있어 베어오프 불가).
    // 두 눈을 다 쓰는 수(13/8 10/4)가 있으므로 그것만 합법.
    let b = board(&[(13, 1), (10, 1)], &[(w2b(7), 2), (w2b(2), 2)]);
    assert_eq!(notations(&b, W, 6, 5), vec!["13/8 10/4".to_string()]);
    // UI용 걸음 단위 생성도 10/5를 막아야 한다.
    let first = next_steps(&b, W, 6, 5, &[]).unwrap();
    assert!(first.iter().all(|s| !(s.from == 10 && s.to == 5)));
    assert_eq!(first.len(), 2); // 13/8, 10/4
}

#[test]
fn larger_die_when_only_one_playable() {
    // White 말 하나가 12. 1이 막혀 있어 6과 5를 둘 다 쓸 수는 없다(12/6/1, 12/7/1 모두 막힘).
    let b = board(&[(12, 1)], &[(w2b(1), 2)]);
    assert_eq!(notations(&b, W, 6, 5), vec!["12/6".to_string()]);
    assert_eq!(notations(&b, W, 5, 6), vec!["12/6".to_string()]);
    // 큰 눈 자리가 막혀 있으면 작은 눈.
    let b = board(&[(12, 1)], &[(w2b(1), 2), (w2b(6), 2)]);
    assert_eq!(notations(&b, W, 6, 5), vec!["12/7".to_string()]);
    let first = next_steps(&b, W, 6, 5, &[]).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!((first[0].from, first[0].to), (12, 7));
}

#[test]
fn larger_die_rule_ui() {
    let b = board(&[(12, 1)], &[(w2b(1), 2)]);
    let first = next_steps(&b, W, 6, 5, &[]).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!((first[0].from, first[0].to, first[0].die), (12, 6, 6));
}

#[test]
fn bar_entry_first() {
    // 바에 하나. 6 진입(19)은 막혀 있고 1 진입(24)은 열림 → 반드시 bar/24 후 6을 쓴다.
    let b = board(
        &[(BAR, 1), (13, 5), (6, 5), (8, 4)],
        &[(w2b(19), 2), (w2b(1), 13)],
    );
    let moves = legal_moves(&b, W, 6, 1);
    assert!(!moves.is_empty());
    for m in &moves {
        assert_eq!(m.len(), 2);
        assert!(m.steps().iter().any(|s| s.from == BAR as u8 && s.to == 24));
    }
    let first = next_steps(&b, W, 6, 1, &[]).unwrap();
    assert!(first.iter().all(|s| s.from == BAR as u8));
}

#[test]
fn two_on_bar_one_entry() {
    // 바에 둘, 진입 가능한 칸은 하나 → 한 걸음만 두고 끝(두 번째 말은 못 들어와서 다른 말도 못 움직임).
    let b = board(&[(BAR, 2), (6, 13)], &[(w2b(19), 2), (w2b(1), 13)]);
    let n = notations(&b, W, 6, 1);
    assert_eq!(n, vec!["bar/24".to_string()]);
}

#[test]
fn closed_board_pass() {
    let b = board(
        &[(BAR, 1), (6, 14)],
        &[
            (w2b(19), 2),
            (w2b(20), 2),
            (w2b(21), 2),
            (w2b(22), 2),
            (w2b(23), 2),
            (w2b(24), 2),
            (w2b(1), 3),
        ],
    );
    let moves = legal_moves(&b, W, 3, 5);
    assert_eq!(moves.len(), 1);
    assert!(moves[0].is_empty());
}

#[test]
fn bearoff_exact_and_higher_die() {
    // 6에서 6으로 정확히 베어오프.
    let b = board(&[(6, 1), (1, 1)], &[(w2b(24), 2)]);
    assert!(notations(&b, W, 6, 1).contains(&"6/off 1/off".to_string()));
    // 3과 2만 있을 때 5: 가장 높은 3에서만 큰 눈 베어오프 가능.
    let b = board(&[(3, 1), (2, 1)], &[(w2b(24), 2)]);
    let s = b.step_for(W, 3, 5).unwrap();
    assert_eq!(s.to, 0);
    assert!(b.step_for(W, 2, 5).is_none());
    assert_eq!(notations(&b, W, 5, 4), vec!["3/off 2/off".to_string()]);
}

#[test]
fn bearoff_blocked_by_higher_checker() {
    // 6과 2에 말. 5로는 2에서 베어오프 불가(더 높은 6에 말이 있음) → 6/1을 둬야 한다.
    let b = board(&[(6, 1), (2, 1)], &[(w2b(24), 2)]);
    assert!(b.step_for(W, 2, 5).is_none());
    assert_eq!(b.step_for(W, 6, 5).unwrap().to, 1);
}

#[test]
fn no_bearoff_with_checker_outside() {
    let b = board(&[(7, 1), (2, 5)], &[(w2b(24), 2)]);
    assert!(b.step_for(W, 2, 3).is_none());
    assert!(b.step_for(W, 2, 2).is_none());
    // 같은 턴에 먼저 홈으로 들어오면 그 다음 눈으로 베어오프 가능: 7/1 후 2/off.
    assert!(notations(&b, W, 6, 2).contains(&"7/1 2/off".to_string()));
}

#[test]
fn hitting_sends_to_bar() {
    let mut b = board(&[(13, 2)], &[(w2b(7), 1), (w2b(1), 2)]);
    let moves = legal_moves(&b, W, 6, 6);
    let hit = moves
        .iter()
        .find(|m| m.steps().iter().any(|s| s.hit))
        .expect("hit exists");
    let before = b;
    b.apply_steps(W, hit.steps());
    assert_eq!(b.on_bar(Side::Black), 1);
    assert!(b.is_valid());
    b.undo_steps(W, hit.steps());
    assert_eq!(b, before);
}

#[test]
fn game_results() {
    // 싱글
    let b = board(&[], &[(6, 3)]);
    let mut b1 = b;
    b1.pos[1][0] = 12;
    assert_eq!(b1.result(), Some((Side::White, 1)));
    // 개먼: 진 쪽이 하나도 못 뺐고, 자기 홈 안에만 있음.
    let mut b2 = Board::empty();
    b2.pos[0][0] = 15;
    b2.pos[1][6] = 15;
    assert_eq!(b2.result(), Some((Side::White, 2)));
    // 백개먼: 진 쪽 말이 이긴 쪽 홈(진 쪽 기준 19..24)에 남음.
    let mut b3 = b2;
    b3.pos[1][6] = 14;
    b3.pos[1][19] = 1;
    assert_eq!(b3.result(), Some((Side::White, 3)));
    // 백개먼: 바에 남음.
    let mut b4 = b2;
    b4.pos[1][6] = 14;
    b4.pos[1][BAR] = 1;
    assert_eq!(b4.result(), Some((Side::White, 3)));
    // 18에 있으면 개먼(이긴 쪽 홈 밖).
    let mut b5 = b2;
    b5.pos[1][6] = 14;
    b5.pos[1][18] = 1;
    assert_eq!(b5.result(), Some((Side::White, 2)));
    // Black 승리도 같은 규칙.
    let mut b6 = Board::empty();
    b6.pos[1][0] = 15;
    b6.pos[0][BAR] = 1;
    b6.pos[0][3] = 14;
    assert_eq!(b6.result(), Some((Side::Black, 3)));
    assert_eq!(Board::start().result(), None);
}

#[test]
fn notation_formats() {
    use palamedes::notation::format_move;
    let b = board(&[(13, 2)], &[(w2b(7), 1), (w2b(1), 2)]);
    assert!(notations(&b, W, 6, 3).contains(&"13/7*/4".to_string()));
    assert!(notations(&b, W, 6, 3).contains(&"13/10 13/7*".to_string()));
    assert_eq!(format_move(&[]), "(no move)");
}

#[test]
fn turn_options_compound_and_progress() {
    use palamedes::turn_options;
    let b = Board::start();
    let t = turn_options(&b, W, 6, 5, &[]).unwrap();
    assert_eq!((t.need, t.played, t.done), (2, 0, false));
    let run = t
        .options
        .iter()
        .find(|o| o.from == 24 && o.to == 13)
        .expect("24/13 compound");
    assert_eq!(run.steps.len(), 2);
    // 13/7을 둔 뒤에는 5만 남는다.
    let first = t
        .options
        .iter()
        .find(|o| o.from == 13 && o.to == 7)
        .unwrap()
        .steps
        .clone();
    let t2 = turn_options(&b, W, 6, 5, &first).unwrap();
    assert!(
        t2.options
            .iter()
            .all(|o| o.steps.iter().all(|s| s.die == 5))
    );
    let second = t2
        .options
        .iter()
        .find(|o| o.from == 13 && o.to == 8)
        .unwrap()
        .steps
        .clone();
    let played: Vec<_> = first.into_iter().chain(second).collect();
    let t3 = turn_options(&b, W, 6, 5, &played).unwrap();
    assert!(t3.done && t3.options.is_empty());
    // 합법수 앞부분이 아니면 None.
    let bogus = [palamedes::Step {
        from: 6,
        to: 3,
        die: 3,
        hit: false,
    }];
    assert!(turn_options(&b, W, 6, 5, &bogus).is_none());
}
