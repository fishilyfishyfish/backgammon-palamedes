//! 웹용 바인딩. 상태 없이 판(52칸 평면 배열)과 JSON 문자열만 주고받는다.
//! 진영: 0 = White, 1 = Black. 걸음 JSON: `[{"from":13,"to":7,"die":6}]` (hit은 엔진이 다시 계산).

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use crate::board::{Board, Side, Step};
use std::sync::OnceLock;

use crate::eval::{Evaluator, HandEval};
use crate::features::compute_features_with_eval;
use crate::movegen::{legal_moves_with_boards, next_steps, turn_options};
use crate::nn::NnEval;
use crate::notation::format_move;
use crate::search::{MoveSummary, rank_moves};

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

/// 1수 탐색으로 모든 합법수를 에퀴티 내림차순으로. `engine`: 0 = 기준선, 1 = 신경망.
/// `[{"steps":[...],"notation":"...","board":[52],"probs":{...},"equity":0.12}]` (수를 둔 쪽 기준).
#[wasm_bindgen]
pub fn rank_moves_json(
    board: &[u8],
    side: u8,
    d1: u8,
    d2: u8,
    engine: u8,
) -> Result<String, JsError> {
    let (b, s) = (board_arg(board)?, side_arg(side)?);
    dice_arg(d1, d2)?;
    let ranked = rank_moves(engine_arg(engine)?, &b, s, d1, d2);
    Ok(to_json(
        &ranked.iter().map(MoveSummary::from).collect::<Vec<_>>(),
    ))
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
