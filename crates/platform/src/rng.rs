//! Randomness: `crypto.getRandomValues` through `getrandom` on wasm32 (Rust workspace § 5 rule 3).

pub trait Rng {
    fn fill(&self, buf: &mut [u8]);

    fn u64(&self) -> u64 {
        let mut b = [0u8; 8];
        self.fill(&mut b);
        u64::from_le_bytes(b)
    }

    fn bytes<const N: usize>(&self) -> [u8; N] {
        let mut b = [0u8; N];
        self.fill(&mut b);
        b
    }
}
