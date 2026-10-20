//! Palamedes backgammon engine.
//!
//! 좌표계: 각 진영은 자기 기준 포인트 번호를 쓴다. 1 = 자기 홈의 가장 안쪽, 24 = 상대 홈의 가장 안쪽.
//! 인덱스 0 = 베어오프된 말, 25 = 바. 상대 말이 내 포인트 p에 있으면 상대 기준으로는 25 - p.

pub mod board;
pub mod features;
pub mod movegen;
pub mod notation;
pub mod rng;
pub mod wasm;

pub use board::{BAR, Board, OFF, Side};
pub use features::{BlotInfo, Features, SideFeatures, compute_features};
pub use movegen::{Move, Step, legal_moves, turn_options};
