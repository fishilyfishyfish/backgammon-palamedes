//! 판 표현과 한 걸음(step) 단위의 수 적용/되돌리기.

use serde::Serialize;

/// 베어오프된 말이 저장되는 인덱스.
pub const OFF: usize = 0;
/// 바에 있는 말이 저장되는 인덱스.
pub const BAR: usize = 25;
/// 한 진영의 말 수.
pub const CHECKERS: u8 = 15;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize)]
pub enum Side {
    White = 0,
    Black = 1,
}

impl Side {
    #[inline]
    pub fn idx(self) -> usize {
        self as usize
    }

    #[inline]
    pub fn opp(self) -> Side {
        match self {
            Side::White => Side::Black,
            Side::Black => Side::White,
        }
    }

    pub fn from_index(i: usize) -> Option<Side> {
        match i {
            0 => Some(Side::White),
            1 => Some(Side::Black),
            _ => None,
        }
    }
}

/// 말 하나를 주사위 한 눈만큼 옮긴 기록. `to == 0`이면 베어오프.
/// `die`는 실제로 쓴 주사위 눈(큰 눈으로 베어오프하면 from보다 클 수 있다).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize)]
pub struct Step {
    pub from: u8,
    pub to: u8,
    pub die: u8,
    pub hit: bool,
}

/// `pos[side][i]`: side 기준 포인트 i에 있는 그 진영의 말 수.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Board {
    pub pos: [[u8; 26]; 2],
}

impl Default for Board {
    fn default() -> Self {
        Board::start()
    }
}

impl Board {
    pub fn empty() -> Board {
        Board { pos: [[0; 26]; 2] }
    }

    /// 표준 시작 배치: 24에 2, 13에 5, 8에 3, 6에 5.
    pub fn start() -> Board {
        let mut b = Board::empty();
        for s in 0..2 {
            b.pos[s][24] = 2;
            b.pos[s][13] = 5;
            b.pos[s][8] = 3;
            b.pos[s][6] = 5;
        }
        b
    }

    /// 52칸 평면 배열(White 0..26, Black 26..52)에서 읽는다.
    pub fn from_flat(flat: &[u8]) -> Option<Board> {
        if flat.len() != 52 {
            return None;
        }
        let mut b = Board::empty();
        b.pos[0].copy_from_slice(&flat[..26]);
        b.pos[1].copy_from_slice(&flat[26..]);
        b.is_valid().then_some(b)
    }

    pub fn to_flat(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(52);
        v.extend_from_slice(&self.pos[0]);
        v.extend_from_slice(&self.pos[1]);
        v
    }

    /// 각 진영 15개, 한 포인트에 두 진영이 같이 있지 않음.
    pub fn is_valid(&self) -> bool {
        for s in 0..2 {
            let total: u32 = self.pos[s].iter().map(|&c| c as u32).sum();
            if total != CHECKERS as u32 {
                return false;
            }
        }
        (1..=24).all(|p| self.pos[0][p] == 0 || self.pos[1][25 - p] == 0)
    }

    #[inline]
    pub fn get(&self, side: Side, p: usize) -> u8 {
        self.pos[side.idx()][p]
    }

    /// side 기준 포인트 p(1..=24)에 있는 상대 말 수.
    #[inline]
    pub fn opp_at(&self, side: Side, p: usize) -> u8 {
        self.pos[side.opp().idx()][25 - p]
    }

    #[inline]
    pub fn off(&self, side: Side) -> u8 {
        self.pos[side.idx()][OFF]
    }

    #[inline]
    pub fn on_bar(&self, side: Side) -> u8 {
        self.pos[side.idx()][BAR]
    }

    /// 핍 카운트. 바는 25핍.
    pub fn pips(&self, side: Side) -> u32 {
        self.pos[side.idx()]
            .iter()
            .enumerate()
            .map(|(p, &c)| p as u32 * c as u32)
            .sum()
    }

    /// 모든 말이 홈(1..=6) 또는 베어오프 상태인가.
    #[inline]
    pub fn all_home(&self, side: Side) -> bool {
        self.pos[side.idx()][7..=BAR].iter().all(|&c| c == 0)
    }

    /// side가 `from`에서 `die`만큼 움직일 수 있으면 그 걸음을 돌려준다.
    pub fn step_for(&self, side: Side, from: usize, die: u8) -> Option<Step> {
        let me = &self.pos[side.idx()];
        if from == OFF || from > BAR || me[from] == 0 || !(1..=6).contains(&die) {
            return None;
        }
        if me[BAR] > 0 && from != BAR {
            return None;
        }
        let to = from as i32 - die as i32;
        if to >= 1 {
            let o = self.opp_at(side, to as usize);
            if o >= 2 {
                return None;
            }
            return Some(Step {
                from: from as u8,
                to: to as u8,
                die,
                hit: o == 1,
            });
        }
        if !self.all_home(side) {
            return None;
        }
        // 정확한 눈이 아니면, 더 높은 칸에 말이 없을 때만 큰 눈으로 베어오프.
        if to < 0 && me[from + 1..=6].iter().any(|&c| c > 0) {
            return None;
        }
        Some(Step {
            from: from as u8,
            to: 0,
            die,
            hit: false,
        })
    }

    pub fn apply_step(&mut self, side: Side, s: &Step) {
        let (me, opp) = (side.idx(), side.opp().idx());
        self.pos[me][s.from as usize] -= 1;
        self.pos[me][s.to as usize] += 1;
        if s.hit {
            self.pos[opp][25 - s.to as usize] -= 1;
            self.pos[opp][BAR] += 1;
        }
    }

    pub fn undo_step(&mut self, side: Side, s: &Step) {
        let (me, opp) = (side.idx(), side.opp().idx());
        if s.hit {
            self.pos[opp][BAR] -= 1;
            self.pos[opp][25 - s.to as usize] += 1;
        }
        self.pos[me][s.to as usize] -= 1;
        self.pos[me][s.from as usize] += 1;
    }

    pub fn apply_steps(&mut self, side: Side, steps: &[Step]) {
        for s in steps {
            self.apply_step(side, s);
        }
    }

    pub fn undo_steps(&mut self, side: Side, steps: &[Step]) {
        for s in steps.iter().rev() {
            self.undo_step(side, s);
        }
    }

    /// 종국이면 (승자, 배수). 배수 1 = 싱글, 2 = 개먼, 3 = 백개먼.
    pub fn result(&self) -> Option<(Side, u8)> {
        for winner in [Side::White, Side::Black] {
            if self.off(winner) != CHECKERS {
                continue;
            }
            let loser = winner.opp();
            if self.off(loser) > 0 {
                return Some((winner, 1));
            }
            // 진 쪽 말이 바나 이긴 쪽 홈(진 쪽 기준 19..=24)에 남아 있으면 백개먼.
            let l = &self.pos[loser.idx()];
            if l[19..=BAR].iter().any(|&c| c > 0) {
                return Some((winner, 3));
            }
            return Some((winner, 2));
        }
        None
    }
}
