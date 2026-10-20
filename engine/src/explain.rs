//! 설명 층(5단계). 두 국면(둔 수와 최선 수의 결과)의 특징값 차이 중 에퀴티 차이와 방향이 맞는 것을
//! 기여도 순으로 골라 템플릿 문장으로 만든다.
//!
//! 기여도는 **선형 대리 모델**로 추정한다: 같은 국면·주사위의 후보 수들 사이에서 특징값 차이로
//! 신경망 에퀴티 차이를 근사하도록 최소제곱으로 맞춘 가중치(`weights/surrogate.json`, `examples/fit_surrogate.rs`).
//! 특징 k의 기여 = w_k × (최선 수의 x_k − 둔 수의 x_k). 이 값은 근사라서 화면에 "설명(근사)"로 따로 표시한다.
//! 모든 문장의 숫자는 `Features`에서 그대로 가져온다.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::board::{Board, Side};
use crate::eval::is_race;
use crate::features::{Features, SideFeatures, compute_features};

/// 대리 모델 입력 개수.
pub const K: usize = 21;

/// 대리 모델 입력 이름. 순서가 가중치 파일과 맞아야 한다.
pub const NAMES: [&str; K] = [
    "pip_lead",
    "my_shots",
    "my_blots",
    "five_point",
    "bar_point",
    "home_points",
    "prime_len",
    "anchors",
    "back_checkers",
    "opp_on_bar",
    "opp_shots",
    "opp_home_points",
    "opp_prime_len",
    "my_off",
    "opp_off",
    "my_on_bar",
    "race_pip_lead",
    "my_hit_loss",
    "my_stacks",
    "my_trapped",
    "opp_trapped",
];

/// 수를 둔 쪽(mover) 기준 대리 모델 입력. 핍 차이는 10핍 단위, 샷은 36분율.
pub fn surrogate_inputs(f: &Features, mover: Side, race: bool) -> [f32; K] {
    let me = &f.sides[mover.idx()];
    let op = &f.sides[mover.opp().idx()];
    let lead = (op.pips as f32 - me.pips as f32) / 10.0;
    [
        lead,
        me.total_shots as f32 / 36.0,
        me.blots.len() as f32,
        me.five_point as u8 as f32,
        me.bar_point as u8 as f32,
        me.home_points as f32,
        me.prime_len as f32,
        me.anchors.len() as f32,
        me.back_checkers as f32,
        op.on_bar as f32,
        op.total_shots as f32 / 36.0,
        op.home_points as f32,
        op.prime_len as f32,
        me.off as f32,
        op.off as f32,
        me.on_bar as f32,
        if race { lead } else { 0.0 },
        hit_loss(me),
        stacks(me) as f32,
        trapped(me, op) as f32,
        trapped(op, me) as f32,
    ]
}

/// 프라임으로 보는 최소 길이.
pub const TRAP_PRIME: u8 = 4;

/// `me`의 말 중 `op`의 가장 긴 연속 포인트(4칸 이상) 뒤에 있는 말 수(바 포함).
/// 상대 포인트 lo..=hi는 내 기준 25 − hi..=25 − lo이고, 내 말은 내려가므로 25 − lo보다 위에 있으면 갇힌 것.
pub fn trapped(me: &SideFeatures, op: &SideFeatures) -> u32 {
    if op.prime_len < TRAP_PRIME {
        return 0;
    }
    let top = 25 - op.prime_range.0 as usize;
    me.on_bar as u32
        + me.point_counts[top..]
            .iter()
            .map(|&n| n as u32)
            .sum::<u32>()
}

/// 블롯이 맞았을 때 잃는 핍의 기댓값(10핍 단위): Σ (맞을 조합/36) × (25 − 포인트).
pub fn hit_loss(s: &SideFeatures) -> f32 {
    s.blots
        .iter()
        .map(|b| b.shots as f32 / 36.0 * (25 - b.point) as f32)
        .sum::<f32>()
        / 10.0
}

/// 한 포인트에 4개 이상 쌓인 말 중 3개를 넘는 수의 합.
pub fn stacks(s: &SideFeatures) -> u32 {
    s.point_counts
        .iter()
        .map(|&n| n.saturating_sub(3) as u32)
        .sum()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Surrogate {
    pub features: Vec<String>,
    pub weights: Vec<f32>,
    /// 학습에 안 쓴 국면에서 후보 수 간 에퀴티 차이를 얼마나 설명하는가(결정계수).
    pub r2_heldout: f32,
    /// 맞출 때 쓴 에퀴티의 탐색 깊이. 분석·힌트가 쓰는 깊이와 같아야 한다.
    #[serde(default)]
    pub ply: u8,
    pub positions: usize,
    pub samples: usize,
}

impl Surrogate {
    pub fn embedded() -> &'static Surrogate {
        static S: OnceLock<Surrogate> = OnceLock::new();
        S.get_or_init(|| {
            let s: Surrogate = serde_json::from_str(include_str!("../weights/surrogate.json"))
                .expect("surrogate.json");
            assert_eq!(s.features, NAMES, "surrogate feature order");
            s
        })
    }
}

/// 문장에서 두 수를 부르는 이름. 분석: ("이 수", "최선 수"), 힌트: ("2위 수", "추천 수").
#[derive(Clone, Copy, Debug)]
pub struct Labels {
    pub worse: &'static str,
    pub better: &'static str,
}

pub const ANALYSIS: Labels = Labels {
    worse: "이 수",
    better: "최선 수",
};
pub const HINT: Labels = Labels {
    worse: "2위 수",
    better: "추천 수",
};

#[derive(Clone, Debug, Serialize)]
pub struct Reason {
    /// 주제: blots, home, prime, anchor, hit, opp_blots, pips, off, stacks, trapped, bar.
    pub topic: &'static str,
    /// 대리 모델이 이 주제(묶인 특징들의 합)에 돌린 에퀴티 차이(better − worse).
    pub contribution: f32,
    pub text: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Explanation {
    /// 기여도 순 최대 3개. 에퀴티 차이와 방향이 맞는(기여도 > 0) 것만.
    pub reasons: Vec<Reason>,
    /// 대리 모델이 예측한 전체 에퀴티 차이(better − worse).
    pub approx_delta: f32,
    pub r2_heldout: f32,
}

/// 기여도가 이 값보다 작으면 이유로 쓰지 않는다.
const MIN_CONTRIBUTION: f32 = 0.004;

/// 같은 내용을 두 번 말하지 않도록 특징을 주제로 묶는다.
pub fn topic_of(name: &str) -> &'static str {
    match name {
        "my_shots" | "my_hit_loss" | "my_blots" => "blots",
        "five_point" | "home_points" => "home",
        "bar_point" | "prime_len" => "prime",
        "anchors" | "back_checkers" => "anchor",
        "opp_on_bar" | "opp_home_points" | "opp_prime_len" => "hit",
        "opp_shots" => "opp_blots",
        "pip_lead" | "race_pip_lead" => "pips",
        "my_off" | "opp_off" => "off",
        "my_stacks" => "stacks",
        "my_trapped" => "trapped",
        "opp_trapped" => "prime",
        _ => "bar",
    }
}

/// `worse`와 `better`는 같은 국면에서 `mover`가 둔 두 수의 결과 국면.
pub fn explain(
    mover: Side,
    worse: &Board,
    better: &Board,
    sur: &Surrogate,
    labels: Labels,
) -> Explanation {
    let fw = compute_features(worse);
    let fb = compute_features(better);
    let xw = surrogate_inputs(&fw, mover, is_race(worse));
    let xb = surrogate_inputs(&fb, mover, is_race(better));
    // (주제, 합계 기여, 주제 안에서 기여가 가장 큰 특징, 그 기여)
    let mut topics: Vec<(&'static str, f32, &'static str, f32)> = Vec::new();
    let mut approx_delta = 0.0;
    for k in 0..K {
        let c = sur.weights[k] * (xb[k] - xw[k]);
        approx_delta += c;
        let t = topic_of(NAMES[k]);
        match topics.iter_mut().find(|e| e.0 == t) {
            Some(e) => {
                e.1 += c;
                if c > e.3 {
                    (e.2, e.3) = (NAMES[k], c);
                }
            }
            None => topics.push((t, c, NAMES[k], c)),
        }
    }
    topics.sort_by(|a, b| b.1.total_cmp(&a.1));
    let reasons = topics
        .into_iter()
        .filter(|e| e.1 >= MIN_CONTRIBUTION)
        .take(3)
        .map(|(topic, contribution, lead, _)| Reason {
            topic,
            contribution,
            text: sentence(topic, lead, &fw, &fb, mover, labels),
        })
        .collect();
    Explanation {
        reasons,
        approx_delta,
        r2_heldout: sur.r2_heldout,
    }
}

fn blot_phrase(s: &SideFeatures) -> String {
    let exposed: Vec<_> = s.blots.iter().filter(|b| b.shots > 0).collect();
    match exposed.as_slice() {
        [] => "블롯을".to_string(),
        [one] => match one.nearest_attacker {
            Some(d) => format!("{d}칸 거리에 블롯을"),
            None => "블롯을".to_string(),
        },
        many => format!("블롯 {}개를", many.len()),
    }
}

/// 주제별 템플릿 문장. `lead`(주제 안에서 기여가 가장 큰 특징)로 무엇을 말할지 고른다.
/// 값은 모두 특징값에서 가져온다. w = 덜 좋은 수, b = 더 좋은 수.
fn sentence(
    topic: &str,
    lead: &str,
    fw: &Features,
    fb: &Features,
    mover: Side,
    l: Labels,
) -> String {
    let (me_w, me_b) = (&fw.sides[mover.idx()], &fb.sides[mover.idx()]);
    let (op_w, op_b) = (&fw.sides[mover.opp().idx()], &fb.sides[mover.opp().idx()]);
    let (worse, better) = (l.worse, l.better);
    let cmp = |what: &str, vw: u32, vb: u32, unit: &str| {
        format!("{what}: {better} {vb}{unit}, {worse} {vw}{unit}.")
    };
    match topic {
        "blots" => {
            let (sw, sb) = (me_w.total_shots, me_b.total_shots);
            let (lw, lb) = (
                (hit_loss(me_w) * 10.0).max(0.0),
                (hit_loss(me_b) * 10.0).max(0.0),
            );
            if sw > sb {
                let what = blot_phrase(me_w);
                let many = me_w.blots.iter().filter(|b| b.shots > 0).count() > 1;
                let any = if many { "하나라도 " } else { "" };
                let tail = if sb == 0 {
                    format!("{better}는 맞을 블롯을 안 남겨.")
                } else {
                    format!("{better}는 36개 중 {sb}개야.")
                };
                format!(
                    "{worse}는 {what} 남겨서 상대가 {any}맞힐 수 있는 조합이 36개 중 {sw}개야. {tail}"
                )
            } else if lw > lb {
                format!(
                    "맞으면 잃을 핍의 기댓값이 {worse}는 {lw:.1}핍, {better}는 {lb:.1}핍이야(맞힐 수 있는 조합은 {worse} {sw}개, {better} {sb}개)."
                )
            } else {
                cmp(
                    "블롯 수",
                    me_w.blots.len() as u32,
                    me_b.blots.len() as u32,
                    "개",
                )
            }
        }
        "home" => match (me_w.five_point, me_b.five_point) {
            (false, true) => format!("{better}는 5포인트를 만들어. {worse}는 못 만들어."),
            (true, false) => format!("{worse}는 5포인트를 만들지만 {better}는 안 만들어."),
            _ => cmp(
                "홈보드 포인트",
                me_w.home_points as u32,
                me_b.home_points as u32,
                "개",
            ),
        },
        "prime" if lead == "opp_trapped" && trapped(op_w, me_w) != trapped(op_b, me_b) => {
            let (vw, vb) = (trapped(op_w, me_w), trapped(op_b, me_b));
            let len = |f: &SideFeatures, n: u32| {
                if n > 0 {
                    format!("{}칸 프라임 뒤에 {n}개", f.prime_len)
                } else {
                    "0개".to_string()
                }
            };
            format!(
                "내 프라임 뒤에 갇힌 상대 말: {better} {}, {worse} {}.",
                len(me_b, vb),
                len(me_w, vw)
            )
        }
        "prime" if lead == "prime_len" && me_w.prime_len != me_b.prime_len => {
            let (vw, vb) = (me_w.prime_len as u32, me_b.prime_len as u32);
            if vb > vw {
                format!("{better}는 {vb}칸 연속 포인트(프라임)를 만들어. {worse}는 {vw}칸이야.")
            } else {
                cmp("가장 긴 연속 포인트", vw, vb, "칸")
            }
        }
        "prime" => match (me_w.bar_point, me_b.bar_point) {
            (false, true) => {
                format!("{better}는 바 포인트(7포인트)를 만들어. {worse}는 못 만들어.")
            }
            (true, false) => {
                format!("{worse}는 바 포인트(7포인트)를 만들지만 {better}는 안 만들어.")
            }
            _ => {
                let (vw, vb) = (me_w.prime_len as u32, me_b.prime_len as u32);
                if vb > vw {
                    format!("{better}는 {vb}칸 연속 포인트(프라임)를 만들어. {worse}는 {vw}칸이야.")
                } else {
                    cmp("가장 긴 연속 포인트", vw, vb, "칸")
                }
            }
        },
        "anchor" => {
            let (aw, ab) = (me_w.anchors.len() as u32, me_b.anchors.len() as u32);
            if aw != ab && (lead == "anchors" || me_w.back_checkers == me_b.back_checkers) {
                cmp("상대 홈보드에 만든 앵커", aw, ab, "개")
            } else {
                cmp(
                    "상대 홈보드에 남은 내 말",
                    me_w.back_checkers as u32,
                    me_b.back_checkers as u32,
                    "개",
                )
            }
        }
        "hit" => {
            let (vw, vb) = (op_w.on_bar as u32, op_b.on_bar as u32);
            if vb > vw {
                format!("{better}는 상대 말을 {vb}개 바로 보내. {worse}는 {vw}개야.")
            } else if vw != vb {
                cmp("바에 간 상대 말", vw, vb, "개")
            } else {
                cmp(
                    "상대 홈보드 포인트",
                    op_w.home_points as u32,
                    op_b.home_points as u32,
                    "개",
                )
            }
        }
        "opp_blots" => cmp(
            "지금 판에서 상대 블롯을 맞힐 수 있는 조합(36개 중)",
            op_w.total_shots as u32,
            op_b.total_shots as u32,
            "개",
        ),
        "pips" => {
            let lw = op_w.pips as i32 - me_w.pips as i32;
            let lb = op_b.pips as i32 - me_b.pips as i32;
            format!("핍 차이(상대 − 나): {better} {lb:+}, {worse} {lw:+}.")
        }
        "off" => cmp("뺀 말", me_w.off as u32, me_b.off as u32, "개"),
        "trapped" => cmp(
            "상대 프라임(4칸 이상) 뒤에 갇힌 내 말",
            trapped(me_w, op_w),
            trapped(me_b, op_b),
            "개",
        ),
        "stacks" => cmp(
            "한 포인트에 3개를 넘게 쌓인 말",
            stacks(me_w),
            stacks(me_b),
            "개",
        ),
        _ => cmp(
            "바에 남은 내 말",
            me_w.on_bar as u32,
            me_b.on_bar as u32,
            "개",
        ),
    }
}
