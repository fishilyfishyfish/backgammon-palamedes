mod common;

use common::{board, w2b};
use palamedes::board::{Board, Side};
use palamedes::cube::{CubeAction, CubeState, decide};
use palamedes::matchplay::{MatchCtx, Phase};
use palamedes::rollout::{RolloutSums, rollout};
use palamedes::{NnEval, best_move};

const W: Side = Side::White;

#[test]
fn rollout_is_deterministic_and_splittable() {
    let e = NnEval::embedded();
    let after = best_move(&e, &Board::start(), W, 3, 1).board;
    let a = rollout(&e, &after, W, 0, 72, 7);
    assert_eq!(a, rollout(&e, &after, W, 0, 72, 7));
    let mut parts = RolloutSums::default();
    parts.add(&rollout(&e, &after, W, 0, 36, 7));
    parts.add(&rollout(&e, &after, W, 36, 36, 7));
    assert_eq!(parts, a);
    assert_eq!(a.trials, 72);
    let p = a.probs();
    assert!(p.win_backgammon <= p.win_gammon && p.win_gammon <= p.win);
}

#[test]
fn rollout_agrees_roughly_with_network() {
    let e = NnEval::embedded();
    let m = best_move(&e, &Board::start(), W, 3, 1);
    let r = rollout(&e, &m.board, W, 0, 360, 11);
    // 신경망 추정과 롤아웃이 크게 어긋나지 않는다(표준오차의 몇 배 안).
    let diff = (r.equity() - m.equity as f64).abs();
    assert!(
        diff < 4.0 * r.stderr() + 0.05,
        "nn {} rollout {} ± {}",
        m.equity,
        r.equity(),
        r.stderr()
    );
}

#[test]
fn rollout_of_finished_game() {
    let e = NnEval::embedded();
    let done = board(&[], &[(6, 15)]);
    let r = rollout(&e, &done, W, 0, 10, 1);
    assert_eq!(r.equity(), 2.0);
    assert_eq!(r.stderr(), 0.0);
}

#[test]
fn cube_decisions_with_network() {
    let e = NnEval::embedded();
    // 시작 국면에서는 더블하지 않는다.
    let d = decide(&e, &Board::start(), W, CubeState::default(), None);
    assert_eq!(d.action, CubeAction::NoDouble, "{d:?}");
    // 레이스에서 크게 앞서면(백 핍 20, 흑 핍 90) 더블/패스.
    let race = board(&[(2, 5), (1, 5)], &[(6, 5), (5, 5), (4, 5)]);
    let d = decide(&e, &race, W, CubeState::default(), None);
    assert_eq!(d.action, CubeAction::DoublePass, "{d:?}");
    // 큐브를 상대가 가지고 있으면 더블하지 않는다.
    let d = decide(
        &e,
        &race,
        W,
        CubeState {
            value: 2,
            owner: Some(Side::Black),
        },
        None,
    );
    assert!(!d.double);
    // 크로포드 판에서는 매치 판단도 더블하지 않는다.
    let ctx = MatchCtx {
        me: 3,
        opp: 1,
        cube: 1,
        phase: Phase::Pre,
    };
    assert!(!decide(&e, &race, W, CubeState::default(), Some(ctx)).double);
    // 흑이 모두 백 홈 안쪽에 갇혀 있으면(개먼 가능성 큼) 너무 좋아서 더블하지 않을 수 있다 — 적어도 더블/테이크는 아니다.
    let crush = board(
        &[(6, 3), (5, 3), (4, 3), (3, 3), (2, 2), (1, 1)],
        &[(palamedes::BAR, 3), (w2b(13), 12)],
    );
    let d = decide(&e, &crush, W, CubeState::default(), None);
    assert_ne!(d.action, CubeAction::DoubleTake, "{d:?}");
}
