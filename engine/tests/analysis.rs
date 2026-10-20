mod common;

use common::{board, w2b};
use palamedes::analysis::{Grade, analyze_move, hint};
use palamedes::board::{Board, Side};
use palamedes::explain::{ANALYSIS, Surrogate, explain};
use palamedes::notation::format_move;
use palamedes::search::{Search, TwoPly, rank_moves_ply};
use palamedes::{NnEval, legal_moves};

const W: Side = Side::White;

fn after(b: &Board, side: Side, d1: u8, d2: u8, notation: &str) -> Board {
    let m = legal_moves(b, side, d1, d2)
        .into_iter()
        .find(|m| format_move(m.steps()) == notation)
        .unwrap_or_else(|| panic!("no move {notation}"));
    let mut nb = *b;
    nb.apply_steps(side, m.steps());
    nb
}

#[test]
fn grade_thresholds() {
    assert_eq!(Grade::from_loss(0.0, true), Grade::Best);
    assert_eq!(Grade::from_loss(0.039, false), Grade::Good);
    assert_eq!(Grade::from_loss(0.04, false), Grade::Doubtful);
    assert_eq!(Grade::from_loss(0.08, false), Grade::Bad);
    assert_eq!(Grade::from_loss(0.16, false), Grade::VeryBad);
    assert_eq!(Grade::VeryBad.mark(), "??");
    assert_eq!(Grade::Bad.mark(), "?");
    assert_eq!(Grade::Doubtful.mark(), "?!");
}

#[test]
fn surrogate_loaded() {
    let s = Surrogate::embedded();
    assert!(s.r2_heldout > 0.3, "{}", s.r2_heldout);
    assert!(s.weights.iter().all(|w| w.is_finite()));
    // 웹(분석·힌트·경고)은 2수로 평가하므로 대리 모델도 2수 에퀴티로 맞춘 것이어야 한다.
    assert_eq!(s.ply, 2);
}

#[test]
fn analyze_opening_mistake() {
    let e = NnEval::embedded();
    let b = Board::start();
    let played = after(&b, W, 3, 1, "24/20");
    let a = analyze_move(
        &e,
        Surrogate::embedded(),
        &b,
        W,
        (3, 1),
        &played,
        Search::ONE_PLY,
    )
    .unwrap();
    assert_eq!(a.best.notation, "8/5 6/5");
    assert!(a.loss > 0.04, "{}", a.loss);
    assert_ne!(a.grade, Grade::Best);
    assert!(a.rank > 1 && a.rank <= a.count);
    let ex = a.explanation.expect("explanation");
    assert!(!ex.reasons.is_empty());
    let home = ex
        .reasons
        .iter()
        .find(|r| r.topic == "home")
        .expect("home reason");
    assert_eq!(home.text, "최선 수는 5포인트를 만들어. 이 수는 못 만들어.");
    let blots = ex
        .reasons
        .iter()
        .find(|r| r.topic == "blots")
        .expect("blot reason");
    assert!(
        blots.text.contains("블롯 2개를") && blots.text.contains("맞을 블롯을 안 남겨"),
        "{}",
        blots.text
    );
    for w in ex.reasons.windows(2) {
        assert!(w[0].contribution >= w[1].contribution);
    }
    // 최선 수를 두면 등급은 최선, 설명 없음.
    let best = after(&b, W, 3, 1, "8/5 6/5");
    let a = analyze_move(
        &e,
        Surrogate::embedded(),
        &b,
        W,
        (3, 1),
        &best,
        Search::ONE_PLY,
    )
    .unwrap();
    assert_eq!((a.grade, a.loss, a.rank), (Grade::Best, 0.0, 1));
    assert!(a.explanation.is_none());
    // 합법수 결과가 아니면 None.
    assert!(
        analyze_move(
            &e,
            Surrogate::embedded(),
            &b,
            W,
            (3, 1),
            &Board::start(),
            Search::ONE_PLY
        )
        .is_none()
    );
}

#[test]
fn shot_sentence_uses_computed_numbers() {
    // 덜 좋은 쪽: 10에 블롯(흑 말이 6칸 뒤 4에 있음 → 17/36), 2의 블롯은 흑 말 뒤라 안 맞음.
    let worse = board(&[(10, 1), (2, 1)], &[(w2b(4), 1), (w2b(19), 14)]);
    let better = board(&[(10, 2)], &[(w2b(4), 1), (w2b(19), 14)]);
    let ex = explain(W, &worse, &better, Surrogate::embedded(), ANALYSIS);
    let r = ex
        .reasons
        .iter()
        .find(|r| r.topic == "blots")
        .expect("shot reason");
    assert!(r.text.contains("6칸 거리에 블롯을"), "{}", r.text);
    assert!(r.text.contains("36개 중 17개"), "{}", r.text);
    assert!(r.contribution > 0.0);
}

#[test]
fn hint_opening() {
    let e = NnEval::embedded();
    let h = hint(
        &e,
        Surrogate::embedded(),
        &Board::start(),
        W,
        (4, 2),
        Search::ONE_PLY,
    );
    assert_eq!(h.best.notation, "8/4 6/4");
    assert!(h.margin > 0.0);
    assert!(h.top.len() >= 2);
    assert!(h.explanation.is_some());
}

#[test]
fn two_ply_evaluator_sane() {
    let e = NnEval::embedded();
    let two = TwoPly::new(e.clone());
    let b = Board::start();
    let p = palamedes::Evaluator::evaluate(&two, &b, W);
    assert!(p.win > 0.45 && p.win < 0.6, "{p:?}");
    // 백·흑 대칭.
    assert_eq!(
        p,
        palamedes::Evaluator::evaluate(&two, &b.swapped(), Side::Black)
    );
    // 종국은 확정값.
    let done = board(&[], &[(6, 15)]);
    assert_eq!(palamedes::Evaluator::evaluate(&two, &done, W).equity(), 2.0);
}

#[test]
fn two_ply_ranking_and_analysis() {
    let e = NnEval::embedded();
    let b = Board::start();
    let r = rank_moves_ply(&e, &b, W, 3, 1, Search::TWO_PLY, None);
    assert_eq!(format_move(r[0].mv.steps()), "8/5 6/5");
    assert!(r[0].ply == 2 && r[1].ply == 2);
    // 2수 값끼리는 내림차순이고 1수 값은 뒤에 온다.
    let k = r.iter().take_while(|m| m.ply == 2).count();
    assert!(r[k..].iter().all(|m| m.ply == 1));
    for w in r[..k].windows(2) {
        assert!(w[0].equity >= w[1].equity);
    }
    // 걸러진 후보 밖의 수를 둬도 분석은 그 수를 2수로 평가한다.
    let worst = r.last().unwrap().board;
    let a = analyze_move(
        &e,
        Surrogate::embedded(),
        &b,
        W,
        (3, 1),
        &worst,
        Search::TWO_PLY,
    )
    .unwrap();
    assert_eq!(a.played.ply, 2);
    assert_eq!(a.best.ply, 2);
    assert!(a.loss > 0.0);
    let h = hint(&e, Surrogate::embedded(), &b, W, (6, 1), Search::TWO_PLY);
    assert_eq!(h.best.notation, "13/7 8/7");
    assert!(h.top[0].ply == 2 && h.top[1].ply == 2);
}

#[test]
fn trapped_counts_checkers_behind_prime() {
    use palamedes::explain::trapped;
    use palamedes::features::compute_features;
    // 흑이 자기 4..=8 포인트(백 기준 17..=21)에 5칸 프라임. 백 24에 2개, 22에 1개는 갇히고 13의 말은 아니다.
    let b = board(
        &[(24, 2), (22, 1), (13, 5), (6, 5)],
        &[(4, 2), (5, 2), (6, 2), (7, 2), (8, 2), (13, 5)],
    );
    let f = compute_features(&b);
    let (w, k) = (&f.sides[0], &f.sides[1]);
    assert_eq!(k.prime_len, 5);
    assert_eq!(trapped(w, k), 3);
    // 백은 프라임이 없다.
    assert_eq!(trapped(k, w), 0);
}
