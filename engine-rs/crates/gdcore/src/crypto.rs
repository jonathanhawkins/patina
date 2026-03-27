//! Crypto and hashing singletons (SHA-256, AES stubs).
//!
//! Provides minimal Godot-compatible `HashingContext` and `Crypto` APIs.
//! SHA-256 is implemented in software (no external deps). AES is stubbed
//! with a placeholder that returns an error.

/// SHA-256 hash computation.
///
/// A pure-Rust software implementation matching FIPS 180-4.
pub struct HashingContext {
    state: [u32; 8],
    buffer: Vec<u8>,
    total_len: u64,
}

impl HashingContext {
    /// Initial hash values (first 32 bits of fractional parts of square roots of first 8 primes).
    const H0: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];

    /// Round constants (first 32 bits of fractional parts of cube roots of first 64 primes).
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
        0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
        0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];

    /// Creates a new SHA-256 hashing context.
    pub fn new() -> Self {
        Self {
            state: Self::H0,
            buffer: Vec::new(),
            total_len: 0,
        }
    }

    /// Starts (or restarts) the hashing context for SHA-256.
    pub fn start(&mut self) {
        self.state = Self::H0;
        self.buffer.clear();
        self.total_len = 0;
    }

    /// Updates the hash with additional data.
    pub fn update(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
        self.total_len += data.len() as u64;

        while self.buffer.len() >= 64 {
            let block: [u8; 64] = self.buffer[..64].try_into().unwrap();
            self.process_block(&block);
            self.buffer.drain(..64);
        }
    }

    /// Finishes the hash and returns the 32-byte SHA-256 digest.
    pub fn finish(&mut self) -> [u8; 32] {
        let bit_len = self.total_len * 8;

        // Padding: append 1 bit, then zeros, then 64-bit length.
        self.buffer.push(0x80);
        while self.buffer.len() % 64 != 56 {
            self.buffer.push(0x00);
        }
        self.buffer.extend_from_slice(&bit_len.to_be_bytes());

        // Process remaining blocks.
        while self.buffer.len() >= 64 {
            let block: [u8; 64] = self.buffer[..64].try_into().unwrap();
            self.process_block(&block);
            self.buffer.drain(..64);
        }

        let mut result = [0u8; 32];
        for (i, &word) in self.state.iter().enumerate() {
            result[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        result
    }

    fn process_block(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(Self::K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }
}

impl Default for HashingContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience function: compute SHA-256 of a byte slice in one call.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut ctx = HashingContext::new();
    ctx.update(data);
    ctx.finish()
}

/// Converts a SHA-256 digest to a hex string.
pub fn sha256_hex(data: &[u8]) -> String {
    let hash = sha256(data);
    hash.iter().map(|b| format!("{:02x}", b)).collect()
}

/// AES encryption context (stub).
///
/// Godot's `AESContext` provides AES-256-CBC encryption/decryption.
/// This is a placeholder that returns errors — real AES requires either
/// a crypto library dependency or a from-scratch implementation.
pub struct AesContext {
    _mode: AesMode,
}

/// AES operation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AesMode {
    /// Encrypt mode.
    Encrypt,
    /// Decrypt mode.
    Decrypt,
}

impl AesContext {
    /// Creates a new AES context (stub).
    pub fn new() -> Self {
        Self {
            _mode: AesMode::Encrypt,
        }
    }

    /// Starts the AES context with a key and IV.
    ///
    /// Returns `Err` because AES is not yet implemented.
    pub fn start(&mut self, mode: AesMode, key: &[u8], iv: &[u8]) -> Result<(), &'static str> {
        if key.len() != 32 {
            return Err("AES-256 requires a 32-byte key");
        }
        if iv.len() != 16 {
            return Err("AES-CBC requires a 16-byte IV");
        }
        self._mode = mode;
        Err("AES not yet implemented (stub)")
    }

    /// Updates the AES context with data (stub — returns error).
    pub fn update(&self, _data: &[u8]) -> Result<Vec<u8>, &'static str> {
        Err("AES not yet implemented (stub)")
    }

    /// Finishes the AES operation (stub — returns error).
    pub fn finish(&self) -> Result<(), &'static str> {
        Err("AES not yet implemented (stub)")
    }
}

impl Default for AesContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_empty_string() {
        let hash = sha256(b"");
        let hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
        assert_eq!(
            hex,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_hello_world() {
        let hash = sha256(b"hello world");
        let hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
        assert_eq!(
            hex,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn sha256_abc() {
        let hash = sha256(b"abc");
        let hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
        assert_eq!(
            hex,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha256_incremental_matches_oneshot() {
        let mut ctx = HashingContext::new();
        ctx.update(b"hello ");
        ctx.update(b"world");
        let incremental = ctx.finish();
        let oneshot = sha256(b"hello world");
        assert_eq!(incremental, oneshot);
    }

    #[test]
    fn sha256_restart_produces_fresh_hash() {
        let mut ctx = HashingContext::new();
        ctx.update(b"garbage data");
        ctx.start(); // restart
        ctx.update(b"abc");
        let hash = ctx.finish();
        let expected = sha256(b"abc");
        assert_eq!(hash, expected);
    }

    #[test]
    fn sha256_hex_convenience() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha256_long_input() {
        // 1000 bytes of 'a'
        let data = vec![b'a'; 1000];
        let hash = sha256(&data);
        // Known SHA-256 for 1000 'a's
        let hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
        assert_eq!(
            hex,
            "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3"
        );
    }

    #[test]
    fn aes_stub_returns_error() {
        let mut ctx = AesContext::new();
        let key = [0u8; 32];
        let iv = [0u8; 16];
        assert!(ctx.start(AesMode::Encrypt, &key, &iv).is_err());
        assert!(ctx.update(b"data").is_err());
        assert!(ctx.finish().is_err());
    }

    #[test]
    fn aes_rejects_wrong_key_size() {
        let mut ctx = AesContext::new();
        let short_key = [0u8; 16]; // Should be 32
        let iv = [0u8; 16];
        let err = ctx.start(AesMode::Encrypt, &short_key, &iv).unwrap_err();
        assert!(err.contains("32-byte key"));
    }

    #[test]
    fn aes_rejects_wrong_iv_size() {
        let mut ctx = AesContext::new();
        let key = [0u8; 32];
        let short_iv = [0u8; 8]; // Should be 16
        let err = ctx.start(AesMode::Encrypt, &key, &short_iv).unwrap_err();
        assert!(err.contains("16-byte IV"));
    }
}
