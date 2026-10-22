//! gnubg 연결(측정 전용). `vs_gnubg`, `cube_check` 예제가 같이 쓴다.
//! gnubg는 apt 패키지(/usr/games/gnubg)를 파이썬 스크립트 모드로 띄운다(`scripts/gnubg-driver.py`).

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use palamedes::board::{BAR, Board, Side};
use palamedes::movegen::{Move, legal_moves_with_boards};
use palamedes::play::Player;

pub struct Gnubg {
    _child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Gnubg {
    pub fn start(plies: u8) -> Gnubg {
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

pub struct GnubgPlayer {
    pub g: Gnubg,
    pub label: String,
}

pub fn side_cells(b: &Board, s: Side) -> Vec<u8> {
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

/// gnubg의 큐브 판단(굴리기 전, 머니, `side` 기준, 큐브 1 정규화 에퀴티).
#[derive(Clone, Copy, Debug)]
pub struct GnubgCube {
    pub optimal: f32,
    pub no_double: f32,
    pub take: f32,
    pub pass: f32,
}

impl Gnubg {
    /// `owner`: None = 가운데, Some(true) = `side`가 가짐, Some(false) = 상대가 가짐.
    pub fn cube(&mut self, b: &Board, side: Side, value: u8, owner: Option<bool>) -> GnubgCube {
        let mut msg: Vec<String> = vec!["cube".into()];
        msg.extend(side_cells(b, side).iter().map(|x| x.to_string()));
        msg.extend(side_cells(b, side.opp()).iter().map(|x| x.to_string()));
        msg.push(value.to_string());
        msg.push(
            match owner {
                None => -1,
                Some(true) => 1,
                Some(false) => 0,
            }
            .to_string(),
        );
        writeln!(self.input, "{}", msg.join(" ")).unwrap();
        let line = self.wait_for("CUBE");
        let v: Vec<f32> = line[4..]
            .split_whitespace()
            .map(|x| x.parse().unwrap())
            .collect();
        GnubgCube {
            optimal: v[0],
            no_double: v[1],
            take: v[2],
            pass: v[3],
        }
    }
}
