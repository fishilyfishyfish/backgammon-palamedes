//! 웹용 바인딩. 상태 없이 판(52칸 평면 배열)과 JSON 문자열만 주고받는다.
//! 진영: 0 = White, 1 = Black. 걸음 JSON: `[{"from":13,"to":7,"die":6}]` (hit은 엔진이 다시 계산).

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use crate::board::{Board, Side, Step};
use std::sync::OnceLock;

use crate::analysis::{analyze_move, hint};
use crate::cube::{CubeDecision, CubeState, cube_reasons, decide};
use crate::eval::{Evaluator, HandEval};
use crate::explain::Surrogate;
use crate::features::compute_features_with_eval;
use crate::matchplay::{MAX_AWAY, Phase, met_table, post_crawford_leader};
use crate::movegen::{legal_moves_with_boards, next_steps, turn_options};
use crate::nn::NnEval;
use crate::notation::format_move;
use crate::play::MatchScore;
use crate::rollout::rollout;
use crate::search::{MoveSummary, Objective, Search, rank_moves_ply};

#[derive(Deserialize)]
struct StepIn {
    from: u8,
    to: u8,
    die: u8,
}

fn board_arg(flat: &[u8]) -> Result<Board, JsError> {
    Board::from_flat(flat).ok_or_else(|| JsError::new("invalid board"))
}

fn side_arg(side: u8) -> Result<Side, JsError> {
    Side::from_index(side as usize).ok_or_else(|| JsError::new("invalid side"))
}

fn dice_arg(d1: u8, d2: u8) -> Result<(), JsError> {
    if (1..=6).contains(&d1) && (1..=6).contains(&d2) {
        Ok(())
    } else {
        Err(JsError::new("invalid dice"))
    }
}

fn steps_arg(json: &str) -> Result<Vec<Step>, JsError> {
    let v: Vec<StepIn> = serde_json::from_str(json).map_err(|e| JsError::new(&e.to_string()))?;
    Ok(v.into_iter()
        .map(|s| Step {
            from: s.from,
            to: s.to,
            die: s.die,
            hit: false,
        })
        .collect())
}

fn to_json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).expect("serializable")
}

/// 턴 시작 국면에서 played가 합법수의 앞부분인지 확인하고, 엔진이 다시 계산한 걸음(hit 포함)을 돌려준다.
fn validate_played(
    start: &Board,
    side: Side,
    d1: u8,
    d2: u8,
    played: &[Step],
) -> Result<Vec<Step>, JsError> {
    let mut out = Vec::with_capacity(played.len());
    for p in played {
        let next =
            next_steps(start, side, d1, d2, &out).ok_or_else(|| JsError::new("illegal prefix"))?;
        let s = next
            .into_iter()
            .find(|s| s.from == p.from && s.to == p.to && s.die == p.die)
            .ok_or_else(|| JsError::new("illegal step"))?;
        out.push(s);
    }
    Ok(out)
}

#[wasm_bindgen]
pub fn start_board() -> Vec<u8> {
    Board::start().to_flat()
}

/// 합법수 목록(결과 국면 기준 중복 제거). `[{"steps":[...],"board":[52],"notation":"..."}]`.
#[wasm_bindgen]
pub fn legal_moves_json(board: &[u8], side: u8, d1: u8, d2: u8) -> Result<String, JsError> {
    let (b, s) = (board_arg(board)?, side_arg(side)?);
    dice_arg(d1, d2)?;
    #[derive(Serialize)]
    struct Out<'a> {
        steps: &'a [Step],
        board: Vec<u8>,
        notation: String,
    }
    let moves = legal_moves_with_boards(&b, s, d1, d2);
    let out: Vec<Out> = moves
        .iter()
        .map(|(m, nb)| Out {
            steps: m.steps(),
            board: nb.to_flat(),
            notation: format_move(m.steps()),
        })
        .collect();
    Ok(to_json(&out))
}

/// 사람이 한 걸음씩 둘 때: 턴 시작 국면 + 지금까지 둔 걸음 → 다음 선택지와 현재 국면.
#[wasm_bindgen]
pub fn turn_state_json(
    start: &[u8],
    side: u8,
    d1: u8,
    d2: u8,
    played_json: &str,
) -> Result<String, JsError> {
    let (b, s) = (board_arg(start)?, side_arg(side)?);
    dice_arg(d1, d2)?;
    let played = validate_played(&b, s, d1, d2, &steps_arg(played_json)?)?;
    let state =
        turn_options(&b, s, d1, d2, &played).ok_or_else(|| JsError::new("illegal prefix"))?;
    let mut now = b;
    now.apply_steps(s, &played);
    #[derive(Serialize)]
    struct Out {
        #[serde(flatten)]
        state: crate::movegen::TurnState,
        board: Vec<u8>,
        steps: Vec<Step>,
        notation: String,
    }
    Ok(to_json(&Out {
        state,
        board: now.to_flat(),
        notation: format_move(&played),
        steps: played,
    }))
}

/// 엔진 선택: 0 = 기준선(손 평가), 1 = 신경망(학습된 가중치, WASM에 포함).
fn engine_arg(engine: u8) -> Result<&'static dyn Evaluator, JsError> {
    static HAND: OnceLock<HandEval> = OnceLock::new();
    static NN: OnceLock<NnEval> = OnceLock::new();
    match engine {
        0 => Ok(HAND.get_or_init(HandEval::default)),
        1 => Ok(NN.get_or_init(NnEval::embedded)),
        _ => Err(JsError::new("invalid engine")),
    }
}

/// 양쪽 특징값 + 엔진의 결과 확률(White 기준). `on_roll`은 이제 굴릴 차례.
#[wasm_bindgen]
pub fn features_json(board: &[u8], on_roll: u8, engine: u8) -> Result<String, JsError> {
    let (b, s) = (board_arg(board)?, side_arg(on_roll)?);
    Ok(to_json(&compute_features_with_eval(
        &b,
        s,
        engine_arg(engine)?,
    )))
}

// ---------- 큐브·매치 상황 ----------
// ctx_json: {"cube": {"value": 1, "owner": null | 0 | 1}, "match": null | {"length": 5, "score": [0, 2], "post": false}}
// 빈 문자열이나 "{}"면 큐브 1·가운데·머니 게임.

#[derive(Deserialize, Default)]
struct CubeIn {
    value: Option<u8>,
    owner: Option<u8>,
}

#[derive(Deserialize)]
struct MatchIn {
    length: u8,
    score: [u8; 2],
    #[serde(default)]
    post: bool,
}

#[derive(Deserialize, Default)]
struct CtxIn {
    #[serde(default)]
    cube: CubeIn,
    #[serde(rename = "match", default)]
    m: Option<MatchIn>,
}

/// (큐브 상태, 매치 점수).
fn ctx_arg(json: &str) -> Result<(CubeState, Option<MatchScore>), JsError> {
    let c: CtxIn = if json.trim().is_empty() {
        CtxIn::default()
    } else {
        serde_json::from_str(json).map_err(|e| JsError::new(&format!("ctx: {e}")))?
    };
    let value = c.cube.value.unwrap_or(1);
    if !value.is_power_of_two() || value > 64 {
        return Err(JsError::new("ctx: cube value"));
    }
    let owner = match c.cube.owner {
        None => None,
        Some(i) => Some(side_arg(i)?),
    };
    let score = match c.m {
        None => None,
        Some(m) => {
            if m.length == 0
                || m.length as usize > MAX_AWAY
                || m.score.iter().any(|&x| x >= m.length)
            {
                return Err(JsError::new("ctx: match score"));
            }
            Some(MatchScore {
                length: m.length,
                score: m.score,
                phase: if m.post { Phase::Post } else { Phase::Pre },
            })
        }
    };
    Ok((CubeState { value, owner }, score))
}

fn objective_for(side: Side, cube: CubeState, score: Option<MatchScore>) -> Objective {
    score
        .and_then(|m| m.ctx(side, cube.value))
        .map(Objective::Match)
        .unwrap_or(Objective::Money)
}

/// 모든 합법수를 에퀴티 내림차순으로. `engine`: 0 = 기준선, 1 = 신경망. `ply`: 1 또는 2. `ctx_json`: 위 형식.
/// 매치면 에퀴티 자리에 정규화 매치 에퀴티(EMG)가 들어간다. 2수면 걸러진 상위 후보만 2수 값(각 항목의 `ply`).
#[wasm_bindgen]
pub fn rank_moves_json(
    board: &[u8],
    side: u8,
    d1: u8,
    d2: u8,
    engine: u8,
    ply: u8,
    ctx_json: &str,
) -> Result<String, JsError> {
    let (b, s) = (board_arg(board)?, side_arg(side)?);
    dice_arg(d1, d2)?;
    let (cube, score) = ctx_arg(ctx_json)?;
    let search = Search::from_ply(ply).with_objective(objective_for(s, cube, score));
    let ranked = rank_moves_ply(&engine_arg(engine)?, &b, s, d1, d2, search, None);
    Ok(to_json(
        &ranked.iter().map(MoveSummary::from).collect::<Vec<_>>(),
    ))
}

/// 둔 수 분석: 실수 등급, 손실, 순위, 상위 후보, 설명(근사). `after`는 둔 수의 결과 국면.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn analyze_move_json(
    before: &[u8],
    side: u8,
    d1: u8,
    d2: u8,
    after: &[u8],
    engine: u8,
    ply: u8,
    ctx_json: &str,
) -> Result<String, JsError> {
    let (b, s, a) = (board_arg(before)?, side_arg(side)?, board_arg(after)?);
    dice_arg(d1, d2)?;
    let (cube, score) = ctx_arg(ctx_json)?;
    let search = Search::from_ply(ply).with_objective(objective_for(s, cube, score));
    let e = engine_arg(engine)?;
    let r = analyze_move(&e, Surrogate::embedded(), &b, s, (d1, d2), &a, search)
        .ok_or_else(|| JsError::new("after is not a legal result"))?;
    Ok(to_json(&r))
}

/// 힌트: 추천 수, 상위 후보, 2위보다 나은 이유(근사).
#[wasm_bindgen]
pub fn hint_json(
    before: &[u8],
    side: u8,
    d1: u8,
    d2: u8,
    engine: u8,
    ply: u8,
    ctx_json: &str,
) -> Result<String, JsError> {
    let (b, s) = (board_arg(before)?, side_arg(side)?);
    dice_arg(d1, d2)?;
    let (cube, score) = ctx_arg(ctx_json)?;
    let search = Search::from_ply(ply).with_objective(objective_for(s, cube, score));
    let e = engine_arg(engine)?;
    Ok(to_json(&hint(
        &e,
        Surrogate::embedded(),
        &b,
        s,
        (d1, d2),
        search,
    )))
}

/// `side`가 굴리기 전 큐브 판단. 매치면 값이 정규화 에퀴티(EMG), 머니면 큐브 1 기준 에퀴티.
/// 더블할 수 없는 상황(상대 소유, 크로포드 판, 이미 이길 만큼 큐브가 큼)도 판단 결과는 주고 `can_double`로 알려 준다.
#[wasm_bindgen]
pub fn cube_decision_json(
    board: &[u8],
    side: u8,
    engine: u8,
    ctx_json: &str,
) -> Result<String, JsError> {
    let (b, s) = (board_arg(board)?, side_arg(side)?);
    let (cube, score) = ctx_arg(ctx_json)?;
    let ctx = score.and_then(|m| m.ctx(s, cube.value));
    let e = engine_arg(engine)?;
    let d = decide(&e, &b, s, cube, ctx);
    let can_double = cube.can_double(s)
        && cube.value < 64
        && ctx.is_none_or(|c| !c.is_crawford() && (cube.value as u16) < c.me as u16);
    #[derive(Serialize)]
    struct Out {
        #[serde(flatten)]
        d: CubeDecision,
        can_double: bool,
        match_play: bool,
        /// 더블하는 쪽 입장, 더블을 받은 쪽 입장의 근거 문장.
        reasons: Vec<String>,
        taker_reasons: Vec<String>,
    }
    let mp = ctx.is_some();
    let (reasons, taker_reasons) = (cube_reasons(&d, false, mp), cube_reasons(&d, true, mp));
    Ok(to_json(&Out {
        d,
        can_double,
        match_play: mp,
        reasons,
        taker_reasons,
    }))
}

/// 난이도(1..=5)대로 AI가 둘 수(신경망). `seed`는 잡음용 난수 시드(호출마다 다르게). 결과는 `MoveSummary` 하나.
#[wasm_bindgen]
pub fn ai_move_json(
    board: &[u8],
    side: u8,
    d1: u8,
    d2: u8,
    level: u8,
    seed: u32,
    ctx_json: &str,
) -> Result<String, JsError> {
    let (b, s) = (board_arg(board)?, side_arg(side)?);
    dice_arg(d1, d2)?;
    let (cube, score) = ctx_arg(ctx_json)?;
    let ctx = score.and_then(|m| m.ctx(s, cube.value));
    let lv = crate::level::level(level);
    let mut rng = crate::rng::Rng::new(seed as u64 ^ 0x9E37_79B9);
    let m = crate::level::choose_move(&engine_arg(1)?, &b, s, d1, d2, &lv, ctx, &mut rng);
    Ok(to_json(&MoveSummary::from(&m)))
}

/// 난이도대로 AI 큐브 판단(신경망). `side`는 더블하는 쪽. `{double, take}`: 더블할지, 상대가 받을지.
#[wasm_bindgen]
pub fn ai_cube_json(
    board: &[u8],
    side: u8,
    level: u8,
    seed: u32,
    ctx_json: &str,
) -> Result<String, JsError> {
    let (b, s) = (board_arg(board)?, side_arg(side)?);
    let (cube, score) = ctx_arg(ctx_json)?;
    let ctx = score.and_then(|m| m.ctx(s, cube.value));
    let d = decide(&engine_arg(1)?, &b, s, cube, ctx);
    let mut rng = crate::rng::Rng::new(seed as u64 ^ 0x7F4A_7C15);
    let (double, take) = crate::level::cube_choice(&d, &crate::level::level(level), &mut rng);
    #[derive(Serialize)]
    struct Out {
        double: bool,
        take: bool,
    }
    Ok(to_json(&Out { double, take }))
}

/// 롤아웃: `after`(`mover`가 둔 뒤 국면)에서 시행 [start, start + trials). 후보끼리 같은 `seed`를 쓰면 같은 주사위.
/// 큐브 없는 머니 결과(`mover` 기준 합계)를 돌려준다.
#[wasm_bindgen]
pub fn rollout_json(
    after: &[u8],
    mover: u8,
    start: u32,
    trials: u32,
    seed: u32,
    engine: u8,
) -> Result<String, JsError> {
    let (b, s) = (board_arg(after)?, side_arg(mover)?);
    if trials > 10_000 {
        return Err(JsError::new("too many trials"));
    }
    Ok(to_json(&rollout(
        engine_arg(engine)?,
        &b,
        s,
        start,
        trials,
        seed as u64,
    )))
}

/// 계산한 매치 에퀴티 표(행 = 내가 남은 점수, 열 = 상대가 남은 점수, 1..=n)와 크로포드 이후 리더 값.
#[wasm_bindgen]
pub fn met_json(n: u8) -> String {
    let n = (n as usize).clamp(1, MAX_AWAY);
    #[derive(Serialize)]
    struct Out {
        pre: Vec<Vec<f64>>,
        post_leader: Vec<f64>,
    }
    to_json(&Out {
        pre: met_table(n),
        post_leader: (1..=n).map(post_crawford_leader).collect(),
    })
}

/// 종국 판정. 진행 중이면 `null`, 끝났으면 `{"winner":0,"points":2}`.
#[wasm_bindgen]
pub fn result_json(board: &[u8]) -> Result<String, JsError> {
    #[derive(Serialize)]
    struct Out {
        winner: usize,
        points: u8,
    }
    let r = board_arg(board)?.result().map(|(w, points)| Out {
        winner: w.idx(),
        points,
    });
    Ok(to_json(&r))
}

#[wasm_bindgen]
pub fn pip_counts(board: &[u8]) -> Result<Vec<u32>, JsError> {
    let b = board_arg(board)?;
    Ok(vec![b.pips(Side::White), b.pips(Side::Black)])
}
