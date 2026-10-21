use palamedes::board::{Board, Side};
use palamedes::level::{LEVELS, choose_move, level};
use palamedes::nn::NnEval;
use palamedes::rng::Rng;
use palamedes::search::{Search, rank_moves_ply};
use palamedes::{legal_moves, movegen::legal_moves_with_boards};

#[test]
fn levels_are_ordered() {
    assert_eq!(level(0).level, 1);
    assert_eq!(level(9).level, 5);
    for w in LEVELS.windows(2) {
        assert!(
            w[0].noise >= w[1].noise
                && w[0].habit >= w[1].habit
                && w[0].cube_noise >= w[1].cube_noise
        );
    }
}

#[test]
fn top_level_plays_engine_move_and_lower_levels_play_legal_moves() {
    let e = NnEval::embedded();
    let b = Board::start();
    for (d1, d2) in [(4, 2), (6, 5), (2, 1), (3, 3)] {
        let best =
            rank_moves_ply(&e, &b, Side::White, d1, d2, Search::TWO_PLY, None).swap_remove(0);
        let mut rng = Rng::new(1);
        assert_eq!(
            choose_move(&e, &b, Side::White, d1, d2, &level(5), None, &mut rng).board,
            best.board
        );
        let legal: Vec<Board> = legal_moves_with_boards(&b, Side::White, d1, d2)
            .into_iter()
            .map(|x| x.1)
            .collect();
        for lv in 1..=4 {
            for seed in 0..5 {
                let m = choose_move(
                    &e,
                    &b,
                    Side::White,
                    d1,
                    d2,
                    &level(lv),
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
                    &level(lv),
                    None,
                    &mut Rng::new(seed),
                );
                assert_eq!(m.board, again.board);
            }
        }
    }
    assert!(!legal_moves(&b, Side::White, 1, 2).is_empty());
}

#[test]
fn low_level_sometimes_deviates() {
    // 42 시작 수: 8/4 6/4가 분명한 최선이지만 1단계는 자주 다른 수를 둔다(측정: 400번 중 약 74%). 그래도 늘 다르진 않다.
    let e = NnEval::embedded();
    let b = Board::start();
    let best = rank_moves_ply(&e, &b, Side::White, 4, 2, Search::ONE_PLY, None).swap_remove(0);
    let other = (0..200)
        .filter(|&s| {
            choose_move(&e, &b, Side::White, 4, 2, &level(1), None, &mut Rng::new(s)).board
                != best.board
        })
        .count();
    assert!(other > 0 && other < 190, "deviations {other}");
}
