use palamedes::cube::{
    CubeAction, Owner, cubeful_equity, live_equity, money_decision, take_cash_points, win_values,
};
use palamedes::eval::OutcomeProbs;
use palamedes::matchplay::{MatchCtx, Phase, met_table, post_crawford_leader};

fn plain(win: f32) -> OutcomeProbs {
    OutcomeProbs {
        win,
        ..Default::default()
    }
}

fn gammonish(win: f32, wg: f32) -> OutcomeProbs {
    OutcomeProbs {
        win,
        win_gammon: wg,
        ..Default::default()
    }
}

#[test]
fn take_and_cash_points_without_gammons() {
    let (w, l) = win_values(&plain(0.6));
    assert_eq!((w, l), (1.0, 1.0));
    let (tp, cp) = take_cash_points(w, l);
    assert!((tp - 0.2).abs() < 1e-6 && (cp - 0.8).abs() < 1e-6);
    // 개먼을 많이 잃는 쪽은 테이크 포인트가 올라간다.
    let (w, l) = win_values(&OutcomeProbs {
        win: 0.6,
        lose_gammon: 0.2,
        ..Default::default()
    });
    assert!(take_cash_points(w, l).0 > 0.2);
}

#[test]
fn live_equity_endpoints() {
    for owner in [Owner::Centered, Owner::Me, Owner::Opp] {
        assert!((live_equity(1.0, 1.0, 1.0, owner) - 1.0).abs() < 1e-6);
        assert!((live_equity(0.0, 1.0, 1.0, owner) + 1.0).abs() < 1e-6);
    }
    // 가운데 큐브: 테이크 포인트 −1, 캐시 포인트 +1, 그 사이 선형.
    assert!((live_equity(0.5, 1.0, 1.0, Owner::Centered)).abs() < 1e-6);
    // 큐브를 가진 쪽이 더 유리.
    assert!(live_equity(0.5, 1.0, 1.0, Owner::Me) > live_equity(0.5, 1.0, 1.0, Owner::Opp));
}

#[test]
fn money_decisions() {
    let x = 0.68;
    assert_eq!(
        money_decision(&plain(0.5), Owner::Centered, x).action,
        CubeAction::NoDouble
    );
    let d = money_decision(&plain(0.72), Owner::Centered, x);
    assert_eq!(d.action, CubeAction::DoubleTake, "{d:?}");
    let d = money_decision(&plain(0.9), Owner::Centered, x);
    assert_eq!(d.action, CubeAction::DoublePass, "{d:?}");
    // 개먼 가능성이 크고 이미 패스 구간이면 더블하지 않고 개먼을 노린다.
    let d = money_decision(&gammonish(0.92, 0.6), Owner::Centered, x);
    assert_eq!(d.action, CubeAction::TooGood, "{d:?}");
    // 상대가 큐브를 가지고 있으면 더블할 수 없다.
    assert!(!money_decision(&plain(0.9), Owner::Opp, x).double);
    // 노 더블 에퀴티는 승률에 대해 증가.
    let mut last = -2.0;
    for i in 0..=20 {
        let e = cubeful_equity(&plain(i as f32 / 20.0), Owner::Centered, x);
        assert!(e >= last - 1e-6);
        last = e;
    }
}

#[test]
#[allow(clippy::needless_range_loop)]
fn met_is_consistent() {
    let t = met_table(15);
    for a in 0..15 {
        for b in 0..15 {
            assert!((t[a][b] + t[b][a] - 1.0).abs() < 1e-9, "symmetry {a} {b}");
            if a + 1 < 15 {
                assert!(t[a + 1][b] <= t[a][b] + 1e-9, "more to go is worse {a} {b}");
            }
        }
    }
    // 알려진 값(Kazaross-XG2)과 대략 맞는지: 크로포드 2-away/1-away 트레일러 약 32%, 2-away/3-away 약 60%.
    assert!((t[1][0] - 0.32).abs() < 0.02, "{}", t[1][0]);
    assert!((t[1][2] - 0.60).abs() < 0.02, "{}", t[1][2]);
    assert!((t[0][1] - 0.68).abs() < 0.02);
    // 크로포드 이후 2-away 트레일러는 약 50%.
    assert!((post_crawford_leader(2) - 0.5).abs() < 0.03);
}

#[test]
fn match_context_values() {
    let ctx = MatchCtx {
        me: 3,
        opp: 5,
        cube: 1,
        phase: Phase::Pre,
    };
    assert!(!ctx.is_crawford());
    // 확실히 이기는 판(단승)의 MWC = 1점 딴 뒤의 MET, EMG = +1.
    let sure = plain(1.0);
    let t = met_table(5);
    assert!((ctx.mwc_dead(&sure) - t[1][4]).abs() < 1e-6);
    assert!((ctx.emg(ctx.mwc_dead(&sure)) - 1.0).abs() < 1e-5);
    assert!((ctx.emg(ctx.mwc_dead(&plain(0.0))) + 1.0).abs() < 1e-5);
    // 크로포드 판에서는 더블할 수 없다.
    let craw = MatchCtx {
        me: 1,
        opp: 4,
        cube: 1,
        phase: Phase::Pre,
    };
    assert!(craw.is_crawford());
    let d = craw
        .swapped()
        .cube_decision(&plain(0.9), Owner::Centered, 0.68);
    assert!(!d.double && d.action == CubeAction::NoDouble);
    // 크로포드 이후 트레일러는 더블할 수 있고, 리더는 큐브가 죽어 있다.
    let post = MatchCtx {
        me: 4,
        opp: 1,
        cube: 1,
        phase: Phase::Post,
    };
    assert!(!post.is_crawford());
    let lead = post
        .swapped()
        .cube_decision(&plain(0.8), Owner::Centered, 0.68);
    assert!(!lead.double);
    // 점수 상황 판단도 승률이 충분히 높으면 더블/패스.
    let even = MatchCtx {
        me: 7,
        opp: 7,
        cube: 1,
        phase: Phase::Pre,
    };
    assert_eq!(
        even.cube_decision(&plain(0.92), Owner::Centered, 0.68)
            .action,
        CubeAction::DoublePass
    );
    assert_eq!(
        even.cube_decision(&plain(0.5), Owner::Centered, 0.68)
            .action,
        CubeAction::NoDouble
    );
}
