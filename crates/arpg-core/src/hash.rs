use blake3::Hasher;

pub trait HasherExt {
    fn update_u64(&mut self, v: u64);
    fn update_i64(&mut self, v: i64);
    fn update_u32(&mut self, v: u32);
    fn update_u128(&mut self, v: u128);
}

impl HasherExt for blake3::Hasher {
    fn update_u64(&mut self, v: u64) {
        self.update(&v.to_le_bytes());
    }
    fn update_i64(&mut self, v: i64) {
        self.update(&v.to_le_bytes());
    }
    fn update_u32(&mut self, v: u32) {
        self.update(&v.to_le_bytes());
    }
    fn update_u128(&mut self, v: u128) {
        self.update(&v.to_le_bytes());
    }
}

pub fn state_hash(canonical_bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(canonical_bytes);
    *hasher.finalize().as_bytes()
}
