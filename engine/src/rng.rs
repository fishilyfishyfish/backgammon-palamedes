//! 시드 고정 난수(SplitMix64). 자기대국·미러 대국에서 같은 주사위 열을 재현하기 위해 쓴다.

#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// [0, n) 균등 정수.
    pub fn below(&mut self, n: u64) -> u64 {
        // 거부 표본으로 편향 제거.
        let zone = u64::MAX - u64::MAX % n;
        loop {
            let x = self.next_u64();
            if x < zone {
                return x % n;
            }
        }
    }

    pub fn die(&mut self) -> u8 {
        self.below(6) as u8 + 1
    }

    pub fn roll(&mut self) -> (u8, u8) {
        (self.die(), self.die())
    }
}
