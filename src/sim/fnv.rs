//! FNV-1a, shared by the world, terrain and water digests.
//!
//! The replay checksum considers every simulation value, including the shape of
//! the cave and the position of every drop of water, so the digest has to be
//! cheap enough to take at the end of a run and stable across machines. Raw bit
//! patterns go in; nothing here is used for hashing collections.

#[derive(Clone, Copy, Debug)]
pub struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Self::new()
    }
}

impl Fnv {
    pub fn new() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }

    pub fn u8(&mut self, v: u8) {
        self.0 ^= v as u64;
        self.0 = self.0.wrapping_mul(0x100_0000_01b3);
    }

    pub fn u32(&mut self, v: u32) {
        self.u64(v as u64);
    }

    pub fn u64(&mut self, v: u64) {
        for i in 0..8 {
            self.u8((v >> (i * 8)) as u8);
        }
    }

    pub fn f32(&mut self, v: f32) {
        self.u64(v.to_bits() as u64);
    }

    pub fn finish(&self) -> u64 {
        self.0
    }
}

/// One-shot digest of a byte slice.
pub fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h = Fnv::new();
    for &b in bytes {
        h.u8(b);
    }
    h.finish()
}
