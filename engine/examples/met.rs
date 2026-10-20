//! 계산한 매치 에퀴티 표를 출력한다. `cargo run --release --example met -- [n]`

use palamedes::matchplay::{met_table, post_crawford_leader};

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(9);
    let t = met_table(n);
    print!("{:>6}", "");
    for b in 1..=n {
        print!("{b:>6}");
    }
    println!();
    for (a, row) in t.iter().enumerate() {
        print!("{:>6}", a + 1);
        for v in row {
            print!("{:>6.1}", v * 100.0);
        }
        println!();
    }
    println!("post-Crawford leader (1-away) vs b-away:");
    for b in 2..=n {
        print!("{b}:{:.1} ", post_crawford_leader(b) * 100.0);
    }
    println!();
}
