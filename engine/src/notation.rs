//! 표준 기보 표기. 예: "8/5 6/5", "bar/22*", "24/18*/13", "6/off(2)".

use crate::board::{BAR, Step};

fn pt(p: u8) -> String {
    match p as usize {
        BAR => "bar".to_string(),
        0 => "off".to_string(),
        _ => p.to_string(),
    }
}

/// 걸음 목록을 표기 문자열로. 같은 말이 연달아 움직이면 하나로 잇고, 같은 이동은 (n)으로 묶는다.
pub fn format_move(steps: &[Step]) -> String {
    if steps.is_empty() {
        return "(no move)".to_string();
    }
    // 각 체인: [(위치, 맞힘)] 경로.
    let mut chains: Vec<Vec<(u8, bool)>> = Vec::new();
    for s in steps {
        let cont = chains
            .iter_mut()
            .rev()
            .find(|c| c.last().map(|&(p, _)| p) == Some(s.from) && s.from != 0);
        match cont {
            Some(c) => c.push((s.to, s.hit)),
            None => chains.push(vec![(s.from, false), (s.to, s.hit)]),
        }
    }
    let mut parts: Vec<(u8, String)> = chains
        .iter()
        .map(|c| {
            let last = c.len() - 1;
            let text = c
                .iter()
                .enumerate()
                .filter(|&(i, &(_, hit))| i == 0 || i == last || hit)
                .map(|(_, &(p, hit))| format!("{}{}", pt(p), if hit { "*" } else { "" }))
                .collect::<Vec<_>>()
                .join("/");
            (c[0].0, text)
        })
        .collect();
    parts.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < parts.len() {
        let mut n = 1;
        while i + n < parts.len() && parts[i + n].1 == parts[i].1 {
            n += 1;
        }
        out.push(if n > 1 {
            format!("{}({})", parts[i].1, n)
        } else {
            parts[i].1.clone()
        });
        i += n;
    }
    out.join(" ")
}
