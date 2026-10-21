//! GNU 백개먼과 미러 대국(큐브 없는 머니, 측정 전용). gnubg는 apt 패키지(/usr/games/gnubg)를 파이썬 스크립트
//! 모드로 띄우고(`scripts/gnubg-driver.py`), 국면·주사위를 보내 최선 수를 받는다. 스레드마다 gnubg 프로세스 하나.
//! `cargo run --release --example vs_gnubg -- <우리 플레이어> <gnubg 깊이(0|1|2)> [쌍 수] [시드]`
//! 우리 플레이어: nn | nn@2 | nn:<경로> | nn:<경로>@2 | gnubg:<깊이>(gnubg끼리, 측정 점검용)

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Instant;

use palamedes::board::{BAR, Board, Side};
use palamedes::movegen::{Move, legal_moves_with_boards};
use palamedes::nn::{Net, NnEval};
use palamedes::play::{OnePly, Player, Searcher, parallel_mirror_match};
use palamedes::search::Search;

struct Gnubg {
    _child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Gnubg {
    fn start(plies: u8) -> Gnubg {
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts/gnubg-driver.py");
        let mut child = Command::new("/usr/games/gnubg")
            .args(["-t", "-q", "-p", script])
            .current_dir(std::env::temp_dir())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start gnubg (apt install gnubg)");
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut g = Gnubg {
            _child: child,
            input,
            output,
        };
        let ready = g.wait_for("READY");
        // 제이코비가 꺼져 개먼 가치가 살아 있는지 확인(큐브 없는 단판 비교의 전제).
        assert!(
            !ready.contains("'jacoby': 1"),
            "gnubg jacoby must be off: {ready}"
        );
        writeln!(g.input, "ply {plies}").unwrap();
        g.wait_for("OK");
        g
    }

    /// 표시용 출력은 건너뛰고 `prefix`로 시작하는 줄을 돌려준다.
    fn wait_for(&mut self, prefix: &str) -> String {
        let mut line = String::new();
        loop {
            line.clear();
            if self.output.read_line(&mut line).unwrap() == 0 {
                panic!("gnubg exited");
            }
            if line.starts_with(prefix) {
                return line.trim_end().to_string();
            }
        }
    }
}

struct GnubgPlayer {
    g: Gnubg,
    label: String,
}

fn side_cells(b: &Board, s: Side) -> Vec<u8> {
    let p = &b.pos[s.idx()];
    let mut v: Vec<u8> = (1..=24).map(|i| p[i]).collect();
    v.push(p[BAR]);
    v
}

impl Player for GnubgPlayer {
    fn name(&self) -> String {
        self.label.clone()
    }

    fn choose(&mut self, b: &Board, side: Side, d1: u8, d2: u8) -> Move {
        let legal = legal_moves_with_boards(b, side, d1, d2);
        if legal.len() == 1 {
            return legal[0].0;
        }
        let mut msg: Vec<String> = side_cells(b, side).iter().map(|x| x.to_string()).collect();
        msg.extend(side_cells(b, side.opp()).iter().map(|x| x.to_string()));
        msg.push(d1.to_string());
        msg.push(d2.to_string());
        writeln!(self.g.input, "{}", msg.join(" ")).unwrap();
        let line = self.g.wait_for("MOVE");
        let v: Vec<i32> = line[4..]
            .split_whitespace()
            .map(|x| x.parse().unwrap())
            .collect();
        // gnubg의 (출발, 도착) 쌍을 직접 적용해 결과 국면을 만들고, 같은 결과의 합법수를 찾는다.
        let mut nb = *b;
        let (me, op) = (side.idx(), side.opp().idx());
        for pair in v.chunks(2) {
            let (from, to) = (pair[0] as usize, pair[1] as usize);
            if pair[0] < 0 {
                break;
            }
            nb.pos[me][from] -= 1;
            if (1..=24).contains(&to) && nb.pos[op][25 - to] == 1 {
                nb.pos[op][25 - to] = 0;
                nb.pos[op][BAR] += 1;
            }
            nb.pos[me][to] += 1;
        }
        legal
            .iter()
            .find(|(_, r)| *r == nb)
            .unwrap_or_else(|| panic!("gnubg move {v:?} not legal for {b:?} {d1}{d2}"))
            .0
    }
}

fn ours(spec: &str) -> Box<dyn Fn(usize) -> Box<dyn Player> + Sync> {
    if let Some(p) = spec.strip_prefix("gnubg:") {
        let plies: u8 = p.parse().expect("gnubg:<plies>");
        let label = spec.to_string();
        return Box::new(move |_| {
            Box::new(GnubgPlayer {
                g: Gnubg::start(plies),
                label: label.clone(),
            }) as Box<dyn Player>
        });
    }
    let (name, ply) = match spec.split_once('@') {
        Some((n, p)) => (n.to_string(), p.parse::<u8>().unwrap()),
        None => (spec.to_string(), 1),
    };
    let e = match name.strip_prefix("nn:") {
        Some(p) => {
            NnEval::new(Net::from_bytes(&std::fs::read(p).expect("weights")).expect("weights"))
        }
        None => NnEval::embedded(),
    };
    let label = spec.to_string();
    Box::new(move |_| -> Box<dyn Player> {
        if ply < 2 {
            Box::new(OnePly::new(e.clone(), &label))
        } else {
            Box::new(Searcher {
                eval: e.clone(),
                search: Search::from_ply(ply),
                label: label.clone(),
            })
        }
    })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a = args.first().cloned().unwrap_or("nn@2".into());
    let plies: u8 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let pairs: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(200);
    let seed: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1);
    let t = Instant::now();
    let st = parallel_mirror_match(
        ours(&a),
        |_| {
            Box::new(GnubgPlayer {
                g: Gnubg::start(plies),
                label: format!("gnubg {plies}-ply"),
            }) as Box<dyn Player>
        },
        pairs,
        seed,
        4,
    );
    println!(
        "{a} vs gnubg {plies}-ply: {} games ({} pairs), {a} wins {:.1}%, ppg {:+.3} ± {:.3}, gammons {}/{}, {:.0}s",
        st.games,
        st.pairs,
        100.0 * st.win_rate(),
        st.ppg(),
        st.ppg_stderr(),
        st.a_gammons,
        st.b_gammons,
        t.elapsed().as_secs_f64()
    );
}
