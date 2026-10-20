mod common;

use common::{board, w2b};
use palamedes::board::{Board, Side};
use palamedes::eval::{OutcomeProbs, is_race, norm_cdf};
use palamedes::notation::format_move;
use palamedes::play::{OnePly, RandomPlayer, mirror_match};
use palamedes::rng::Rng;
use palamedes::{Evaluator, HandEval, best_move, legal_moves};

const W: Side = Side::White;

fn valid(p: &OutcomeProbs) {
    assert!((0.0..=1.0).contains(&p.win), "{p:?}");
    assert!(
        p.win_backgammon <= p.win_gammon + 1e-6 && p.win_gammon <= p.win + 1e-6,
        "{p:?}"
    );
    assert!(
        p.lose_backgammon <= p.lose_gammon + 1e-6 && p.lose_gammon <= 1.0 - p.win + 1e-6,
        "{p:?}"
    );
    assert!((-3.0..=3.0).contains(&p.equity()));
}

#[test]
fn norm_cdf_values() {
    assert!((norm_cdf(0.0) - 0.5).abs() < 1e-6);
    assert!((norm_cdf(1.0) - 0.841_344_7).abs() < 1e-5);
    assert!((norm_cdf(-1.96) - 0.024_997_9).abs() < 1e-5);
}

#[test]
fn invert_and_equity() {
    let p = OutcomeProbs {
        win: 0.6,
        win_gammon: 0.2,
        win_backgammon: 0.05,
        lose_gammon: 0.1,
        lose_backgammon: 0.01,
    };
    assert!((p.equity() + p.invert().equity()).abs() < 1e-6);
    assert_eq!(p.invert().invert(), p);
    assert_eq!(OutcomeProbs::terminal(W, 3, W).equity(), 3.0);
    assert_eq!(OutcomeProbs::terminal(W, 2, Side::Black).equity(), -2.0);
}

#[test]
fn start_position_sane_and_symmetric() {
    let e = HandEval::default();
    let b = Board::start();
    let pw = e.evaluate(&b, W);
    let pb = e.evaluate(&b, Side::Black);
    valid(&pw);
    assert_eq!(pw, pb);
    // 시작 국면에서 둘 차례는 약간 유리.
    assert!(pw.win > 0.5 && pw.win < 0.6, "{pw:?}");
    assert!(pw.win_gammon > 0.08 && pw.win_gammon < 0.25, "{pw:?}");
}

#[test]
fn random_positions_valid_and_side_symmetric() {
    let e = HandEval::default();
    let mut rng = Rng::new(5);
    for _ in 0..30 {
        let mut b = Board::start();
        let mut side = W;
        while b.result().is_none() {
            let p = e.evaluate(&b, side);
            valid(&p);
            assert_eq!(p, e.evaluate(&b.swapped(), side.opp()));
            let (d1, d2) = rng.roll();
            let m = legal_moves(&b, side, d1, d2);
            b.apply_steps(side, m[rng.below(m.len() as u64) as usize].steps());
            side = side.opp();
        }
    }
}

#[test]
fn race_probabilities() {
    let e = HandEval::default();
    // 백 핍 30, 흑 핍 80인 레이스.
    let b = board(&[(6, 5)], &[(6, 5), (10, 5)]);
    assert!(is_race(&b));
    let p = e.evaluate(&b, W);
    assert!(p.win > 0.97, "{p:?}");
    // 같은 핍이면 둘 차례가 유리.
    let b = board(&[(6, 4), (5, 4)], &[(6, 4), (5, 4)]);
    let p = e.evaluate(&b, W);
    assert!(p.win > 0.5 && p.win < 0.85, "{p:?}");
    // 접촉이 남아 있으면 레이스 아님.
    assert!(!is_race(&Board::start()));
    // 흑이 하나도 못 뺐고 말이 멀리 남아 있으면 백의 개먼 가능성이 크다.
    let b = board(&[(1, 2)], &[(w2b(12), 13), (w2b(11), 2)]);
    assert!(is_race(&b));
    let p = e.evaluate(&b, W);
    assert!(p.win > 0.99 && p.win_gammon > 0.9, "{p:?}");
}

#[test]
fn opening_moves() {
    let e = HandEval::default();
    let b = Board::start();
    let pick = |d1, d2| format_move(best_move(&e, &b, W, d1, d2).mv.steps());
    assert_eq!(pick(3, 1), "8/5 6/5");
    assert_eq!(pick(4, 2), "8/4 6/4");
    assert_eq!(pick(6, 1), "13/7 8/7");
    assert_eq!(pick(6, 5), "24/13");
    assert_eq!(pick(5, 3), "8/3 6/3");
}

#[test]
fn takes_the_win() {
    let e = HandEval::default();
    let b = board(&[(2, 1), (1, 1)], &[(6, 10)]);
    let best = best_move(&e, &b, W, 2, 1);
    assert_eq!(format_move(best.mv.steps()), "2/off 1/off");
    assert_eq!(best.equity, 1.0);
}

#[test]
fn beats_random() {
    let pairs = if cfg!(debug_assertions) { 10 } else { 100 };
    let mut a = OnePly::new(HandEval::default(), "hand");
    let mut r = RandomPlayer::new(9);
    let st = mirror_match(&mut a, &mut r, pairs, 42);
    assert!(st.win_rate() > 0.9, "win rate {}", st.win_rate());
    assert!(st.ppg() > 1.5, "ppg {}", st.ppg());
}
