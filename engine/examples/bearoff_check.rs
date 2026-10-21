//! 베어오프 실수 측정: 홈보드 안 국면(진영당 최대 15개, 6칸)의 한쪽 베어오프 표(다 빼는 데 드는 굴림 수 분포,
//! 기대 굴림 수를 최소로 두는 수 기준)를 계산하고, 양쪽 다 하나 이상 뺀(개먼 없음) 베어오프 국면에서
//! 신경망의 수 선택이 표 기준 최선과 얼마나 다른지 잰다.
//! `cargo run --release --example bearoff_check -- [국면 수]`

use std::collections::HashMap;

use palamedes::board::{Board, Side};
use palamedes::movegen::legal_moves_with_boards;
use palamedes::nn::NnEval;
use palamedes::rng::Rng;
use palamedes::search::{Search, rank_moves_ply, rolls};

const MAXN: usize = 32;

struct Db {
    idx: HashMap<[u8; 6], usize>,
    /// 다 빼는 데 n번 굴림이 들 확률.
    dist: Vec<[f32; MAXN]>,
    mean: Vec<f32>,
}

fn key(b: &Board, s: Side) -> [u8; 6] {
    let p = &b.pos[s.idx()];
    [p[1], p[2], p[3], p[4], p[5], p[6]]
}

fn board_of(k: &[u8; 6]) -> Board {
    let mut b = Board::empty();
    let n: u8 = k.iter().sum();
    b.pos[0][1..=6].copy_from_slice(k);
    b.pos[0][0] = 15 - n;
    // 상대는 멀리(자기 기준 24 = 내 1 쪽이 아니라 반대편)에 두어 접촉이 없게 한다: 상대 기준 1..=6 = 내 19..=24.
    b.pos[1][1] = 15;
    b
}

fn build() -> Db {
    let mut keys = Vec::new();
    let mut k = [0u8; 6];
    fn rec(i: usize, left: u8, k: &mut [u8; 6], out: &mut Vec<[u8; 6]>) {
        if i == 6 {
            out.push(*k);
            return;
        }
        for c in 0..=left {
            k[i] = c;
            rec(i + 1, left - c, k, out);
        }
        k[i] = 0;
    }
    rec(0, 15, &mut k, &mut keys);
    let pip = |k: &[u8; 6]| {
        k.iter()
            .enumerate()
            .map(|(i, &c)| (i + 1) * c as usize)
            .sum::<usize>()
    };
    keys.sort_by_key(pip);
    let idx: HashMap<[u8; 6], usize> = keys.iter().enumerate().map(|(i, k)| (*k, i)).collect();
    let mut dist = vec![[0f32; MAXN]; keys.len()];
    let mut mean = vec![0f32; keys.len()];
    for (i, k) in keys.iter().enumerate() {
        if k.iter().all(|&c| c == 0) {
            dist[i][0] = 1.0;
            continue;
        }
        let b = board_of(k);
        let mut d = [0f32; MAXN];
        let mut m = 0f32;
        for (d1, d2, w) in rolls() {
            let best = legal_moves_with_boards(&b, Side::White, d1, d2)
                .into_iter()
                .map(|(_, nb)| idx[&key(&nb, Side::White)])
                .min_by(|&a, &c| mean[a].total_cmp(&mean[c]))
                .unwrap();
            for n in 1..MAXN {
                d[n] += w * dist[best][n - 1];
            }
            m += w * (1.0 + mean[best]);
        }
        dist[i] = d;
        mean[i] = m;
    }
    Db { idx, dist, mean }
}

/// 굴릴 쪽(a)이 이길 확률. a가 n번째 굴림에 끝나면 b는 n−1번 굴렸으므로 b가 n번 이상 걸려야 a 승.
fn win(db: &Db, a: &[u8; 6], b: &[u8; 6]) -> f32 {
    let (da, dbb) = (&db.dist[db.idx[a]], &db.dist[db.idx[b]]);
    let mut tail = [0f32; MAXN + 1];
    for n in (0..MAXN).rev() {
        tail[n] = tail[n + 1] + dbb[n];
    }
    (1..MAXN).map(|n| da[n] * tail[n]).sum()
}

fn in_home(b: &Board, s: Side) -> bool {
    let p = &b.pos[s.idx()];
    p[7..=25].iter().all(|&c| c == 0) && p[0] >= 1
}

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(2000);
    let t = std::time::Instant::now();
    let db = build();
    println!(
        "one-sided table: {} positions, {:.1}s, mean rolls (15 on 6pt) {:.2}",
        db.idx.len(),
        t.elapsed().as_secs_f64(),
        db.mean[db.idx[&[0, 0, 0, 0, 0, 15]]]
    );
    let e = NnEval::embedded();
    let mut rng = Rng::new(5);
    // 자기대국(신경망 1수)으로 양쪽이 홈에 들어오고 하나 이상 뺀 국면을 모은다.
    let mut samples: Vec<(Board, Side, u8, u8)> = Vec::new();
    while samples.len() < n {
        let mut b = Board::start();
        let mut side = Side::White;
        while b.result().is_none() {
            let (d1, d2) = rng.roll();
            if in_home(&b, Side::White) && in_home(&b, Side::Black) {
                samples.push((b, side, d1, d2));
            }
            b = palamedes::search::best_move(&e, &b, side, d1, d2).board;
            side = side.opp();
        }
    }
    samples.truncate(n);
    for ply in [1u8, 2] {
        let (mut loss, mut err, mut choices) = (0f64, 0u32, 0u32);
        for (b, side, d1, d2) in &samples {
            let opp = key(b, side.opp());
            // 이 수를 둔 뒤 상대 차례: 내 승률 = 1 − 상대(굴릴 쪽) 승률.
            let eq = |nb: &Board| 2.0 * (1.0 - win(&db, &opp, &key(nb, *side))) - 1.0;
            let all: Vec<f32> = legal_moves_with_boards(b, *side, *d1, *d2)
                .iter()
                .map(|(_, nb)| eq(nb))
                .collect();
            let best = all.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            if all.iter().filter(|&&x| x < best - 1e-6).count() == 0 {
                continue;
            }
            choices += 1;
            let pick =
                rank_moves_ply(&e, b, *side, *d1, *d2, Search::from_ply(ply), None).swap_remove(0);
            let l = (best - eq(&pick.board)) as f64;
            loss += l;
            err += (l > 0.01) as u32;
        }
        println!(
            "NN {ply}-ply: {choices} bear-off choices, mean loss {:.4}, errors > 0.01: {:.1}%",
            loss / choices.max(1) as f64,
            100.0 * err as f64 / choices.max(1) as f64
        );
    }
}
