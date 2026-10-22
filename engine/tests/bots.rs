use palamedes::board::{Board, Side};
use palamedes::bots::{Style, bot, bots, choose_move, rating_diff, win_prob};
use palamedes::movegen::legal_moves_with_boards;
use palamedes::nn::NnEval;
use palamedes::rng::Rng;
use palamedes::search::{Search, rank_moves_ply};

#[test]
fn roster_is_sorted_and_top_is_engine() {
    let b = bots();
    assert!(b.len() >= 5);
    assert!(b.windows(2).all(|w| w[0].rating < w[1].rating));
    assert_eq!(b.last().unwrap().style, Style::ENGINE);
    assert_eq!(bot(999).id, b.last().unwrap().id);
    let mut ids: Vec<_> = b.iter().map(|x| &x.id).collect();
    ids.dedup();
    assert_eq!(ids.len(), b.len());
}

#[test]
fn fibs_formula() {
    assert!((win_prob(0.0, 1.0) - 0.5).abs() < 1e-6);
    // 200점 높으면 단판 승률 약 55.7%.
    assert!((win_prob(200.0, 1.0) - 0.5573).abs() < 1e-3);
    assert!((rating_diff(win_prob(321.0, 5.0), 5.0) - 321.0).abs() < 0.1);
}

#[test]
fn engine_bot_plays_engine_move_and_others_play_legal_moves() {
    let e = NnEval::embedded();
    let b = Board::start();
    for (d1, d2) in [(4, 2), (6, 5), (2, 1), (3, 3)] {
        let best =
            rank_moves_ply(&e, &b, Side::White, d1, d2, Search::THREE_PLY, None).swap_remove(0);
        let top = &bots().last().unwrap().style;
        assert_eq!(
            choose_move(&e, &b, Side::White, d1, d2, top, None, &mut Rng::new(1)).board,
            best.board
        );
        let legal: Vec<Board> = legal_moves_with_boards(&b, Side::White, d1, d2)
            .into_iter()
            .map(|x| x.1)
            .collect();
        for bt in &bots()[..bots().len() - 1] {
            for seed in 0..3 {
                let m = choose_move(
                    &e,
                    &b,
                    Side::White,
                    d1,
                    d2,
                    &bt.style,
                    None,
                    &mut Rng::new(seed),
                );
                assert!(legal.contains(&m.board));
                // 같은 시드면 같은 수.
                let again = choose_move(
                    &e,
                    &b,
                    Side::White,
                    d1,
                    d2,
                    &bt.style,
                    None,
                    &mut Rng::new(seed),
                );
                assert_eq!(m.board, again.board);
            }
        }
    }
}

#[test]
fn weakest_bot_sometimes_deviates() {
    // 42 시작 수: 8/4 6/4가 분명한 최선이지만 가장 약한 봇은 자주 다른 수를 둔다. 그래도 늘 다르진 않다.
    let e = NnEval::embedded();
    let b = Board::start();
    let best = rank_moves_ply(&e, &b, Side::White, 4, 2, Search::ONE_PLY, None).swap_remove(0);
    let weak = &bots()[0].style;
    let other = (0..200)
        .filter(|&s| {
            choose_move(&e, &b, Side::White, 4, 2, weak, None, &mut Rng::new(s)).board != best.board
        })
        .count();
    assert!(other > 0 && other < 195, "deviations {other}");
}
