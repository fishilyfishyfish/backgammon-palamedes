//! TD(λ) 자기대국 학습.
//! `cargo run --release --example train -- [key=value ...]`
//! key: games, hidden, ext(1이면 손 특징 입력), lambda, lr, lr_end, explore, threads, gpt(스레드당 라운드 판 수), seed,
//!      eval_every(판), eval_pairs, out(저장 경로), init(이어서 학습할 가중치)

use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::time::Instant;

use palamedes::HandEval;
use palamedes::nn::{Net, NnEval};
use palamedes::play::{OnePly, Player, parallel_mirror_match};
use palamedes::train::{TrainConfig, lr_at, probs_ok, train_round};

fn main() {
    let mut cfg = TrainConfig::default();
    let mut eval_every = 5_000u64;
    let mut eval_pairs = 300u32;
    let mut out = String::from("weights/td-new.bin");
    let mut init: Option<String> = None;
    for kv in std::env::args().skip(1) {
        let (k, v) = kv.split_once('=').expect("key=value");
        match k {
            "games" => cfg.games = v.parse().unwrap(),
            "hidden" => cfg.hidden = v.parse().unwrap(),
            "ext" => {
                cfg.inputs = if v == "1" {
                    palamedes::nn::INPUTS_EXT
                } else {
                    palamedes::nn::INPUTS
                }
            }
            "lambda" => cfg.lambda = v.parse().unwrap(),
            "lr" => cfg.lr_start = v.parse().unwrap(),
            "lr_end" => cfg.lr_end = v.parse().unwrap(),
            "explore" => cfg.explore = v.parse().unwrap(),
            "threads" => cfg.threads = v.parse().unwrap(),
            "gpt" => cfg.games_per_thread = v.parse().unwrap(),
            "seed" => cfg.seed = v.parse().unwrap(),
            "eval_every" => eval_every = v.parse().unwrap(),
            "eval_pairs" => eval_pairs = v.parse().unwrap(),
            "out" => out = v.to_string(),
            "init" => init = Some(v.to_string()),
            _ => panic!("unknown key {k}"),
        }
    }
    let net = match &init {
        Some(p) => Net::read_from(&mut BufReader::new(File::open(p).expect("init file")))
            .expect("read init"),
        None => Net::with_inputs(cfg.inputs, cfg.hidden, cfg.seed),
    };
    println!(
        "config: {cfg:?}, eval_every={eval_every}, eval_pairs={eval_pairs}, out={out}, init={init:?}"
    );
    let mut eval = NnEval::new(net);
    let t0 = Instant::now();
    let (mut done, mut next_eval) = (0u64, eval_every);
    let (mut loss_acc, mut loss_n) = (0.0f64, 0u32);
    while done < cfg.games {
        let lr = lr_at(&cfg, done);
        let (g, _, loss) = train_round(&mut eval, &cfg, done, lr);
        done += g;
        loss_acc += loss as f64;
        loss_n += 1;
        if done >= next_eval || done >= cfg.games {
            next_eval += eval_every;
            assert!(probs_ok(&eval), "NaN in network");
            let ev = eval.clone();
            let st = parallel_mirror_match(
                |_| Box::new(OnePly::new(ev.clone(), "nn")) as Box<dyn Player>,
                |_| Box::new(OnePly::new(HandEval::default(), "hand")) as Box<dyn Player>,
                eval_pairs,
                900_000,
                cfg.threads,
            );
            println!(
                "games {done:>7}  {:>6.0}s  lr {lr:.4}  loss {:.4}  vs hand: ppg {:+.3} ± {:.3}, wins {:.1}%",
                t0.elapsed().as_secs_f64(),
                loss_acc / loss_n as f64,
                st.ppg(),
                st.ppg_stderr(),
                100.0 * st.win_rate()
            );
            loss_acc = 0.0;
            loss_n = 0;
            let mut f = BufWriter::new(File::create(&out).expect("out file"));
            eval.net.write_to(&mut f).expect("write");
            f.flush().unwrap();
        }
    }
}
