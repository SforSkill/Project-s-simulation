//! Deterministic, platform independent pseudo random numbers.
//!
//! Used only for the movement spread and bot decisions. Damage and recoil never
//! consume randomness. The server derives each shot's stream from a secret match
//! seed, so clients cannot predict future spread (see `docs/weapons.md`).

/// SplitMix64: tiny, fast, statistically good enough for gameplay, and identical on
/// every platform because it only uses integer arithmetic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetRng {
    state: u64,
}

impl DetRng {
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Stream for a given key (e.g. match seed, shooter, shot index).
    pub fn for_key(seed: u64, a: u64, b: u64) -> Self {
        let mut r = Self::new(seed ^ a.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        r.state ^= b.wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
        r.next_u64();
        r
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Uniform in [0, 1).
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn range_f32(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next_f32()
    }

    /// Uniform integer in [0, n).
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        ((self.next_u32() as u64 * n as u64) >> 32) as u32
    }

    pub fn chance(&mut self, p: f32) -> bool {
        self.next_f32() < p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reproducible() {
        let mut a = DetRng::for_key(42, 3, 7);
        let mut b = DetRng::for_key(42, 3, 7);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        let mut c = DetRng::for_key(42, 3, 8);
        assert_ne!(DetRng::for_key(42, 3, 7).next_u64(), c.next_u64());
    }

    #[test]
    fn unit_interval() {
        let mut r = DetRng::new(1);
        let mut sum = 0.0;
        for _ in 0..10_000 {
            let v = r.next_f32();
            assert!((0.0..1.0).contains(&v));
            sum += v;
        }
        assert!((sum / 10_000.0 - 0.5).abs() < 0.02);
    }
}
