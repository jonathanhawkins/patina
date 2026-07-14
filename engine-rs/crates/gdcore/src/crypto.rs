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
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    /// Round constants (first 32 bits of fractional parts of cube roots of first 64 primes).
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
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

/// AES-256-CBC encryption context (pat-z35r2.6).
///
/// Godot's `AESContext` provides AES-256-CBC encrypt/decrypt. Uses the `aes`
/// + `cbc` crates for a real, tested implementation.
pub struct AesContext {
    mode: AesMode,
    key: Option<[u8; 32]>,
    iv: Option<[u8; 16]>,
    started: bool,
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
    /// Creates a new AES context.
    pub fn new() -> Self {
        Self {
            mode: AesMode::Encrypt,
            key: None,
            iv: None,
            started: false,
        }
    }

    /// Starts the AES context with a 32-byte key and 16-byte IV.
    pub fn start(&mut self, mode: AesMode, key: &[u8], iv: &[u8]) -> Result<(), &'static str> {
        if key.len() != 32 {
            return Err("AES-256 requires a 32-byte key");
        }
        if iv.len() != 16 {
            return Err("AES-CBC requires a 16-byte IV");
        }
        let mut k = [0u8; 32];
        k.copy_from_slice(key);
        let mut v = [0u8; 16];
        v.copy_from_slice(iv);
        self.mode = mode;
        self.key = Some(k);
        self.iv = Some(v);
        self.started = true;
        Ok(())
    }

    /// Encrypt or decrypt a full buffer (PKCS7 padded on encrypt).
    pub fn update(&self, data: &[u8]) -> Result<Vec<u8>, &'static str> {
        use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
        type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;
        type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

        if !self.started {
            return Err("AES context not started");
        }
        let key = self.key.as_ref().ok_or("AES key missing")?;
        let iv = self.iv.as_ref().ok_or("AES IV missing")?;
        match self.mode {
            AesMode::Encrypt => {
                let encryptor = Aes256CbcEnc::new(key.into(), iv.into());
                // Buffer must have room for PKCS7 padding (up to +16).
                let mut buf = vec![0u8; data.len() + 16];
                buf[..data.len()].copy_from_slice(data);
                let out = encryptor
                    .encrypt_padded_mut::<Pkcs7>(&mut buf, data.len())
                    .map_err(|_| "AES encrypt failed")?;
                Ok(out.to_vec())
            }
            AesMode::Decrypt => {
                if data.is_empty() || data.len() % 16 != 0 {
                    return Err("AES ciphertext length must be multiple of 16");
                }
                let decryptor = Aes256CbcDec::new(key.into(), iv.into());
                let mut buf = data.to_vec();
                let out = decryptor
                    .decrypt_padded_mut::<Pkcs7>(&mut buf)
                    .map_err(|_| "AES decrypt failed")?;
                Ok(out.to_vec())
            }
        }
    }

    /// Finishes the AES operation (state remains usable for another update).
    pub fn finish(&self) -> Result<(), &'static str> {
        if !self.started {
            return Err("AES context not started");
        }
        Ok(())
    }
}

impl Default for AesContext {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Selectable hash types (MD5 / SHA-1 / SHA-256), HMAC, and Godot-parity
// incremental contexts (pat-u3l8c.79). Pure-Rust, no external dependencies.
// SHA-256 above is reused; MD5 and SHA-1 are implemented here in the same
// software style. Verified against canonical RFC 1321 / FIPS 180 / RFC 2202 /
// RFC 4231 test vectors in the tests below.
// ---------------------------------------------------------------------------

/// MD5 per-round left-rotation amounts (RFC 1321).
const MD5_S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

/// MD5 per-round additive constants: `floor(2^32 * abs(sin(i + 1)))`.
const MD5_K: [u32; 64] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
    0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
    0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
    0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
    0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
    0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
];

/// Computes the MD5 digest (RFC 1321) of `data`.
pub fn md5(data: &[u8]) -> [u8; 16] {
    let (mut a0, mut b0, mut c0, mut d0) =
        (0x67452301u32, 0xefcdab89u32, 0x98badcfeu32, 0x10325476u32);

    // Padding: append 0x80, zeros until length ≡ 56 (mod 64), then the 64-bit
    // little-endian bit length.
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0x00);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut m = [0u32; 16];
        for (i, word) in m.iter_mut().enumerate() {
            *word = u32::from_le_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | ((!b) & d), i),
                16..=31 => ((d & b) | ((!d) & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | (!d)), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(MD5_K[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(MD5_S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&a0.to_le_bytes());
    out[4..8].copy_from_slice(&b0.to_le_bytes());
    out[8..12].copy_from_slice(&c0.to_le_bytes());
    out[12..16].copy_from_slice(&d0.to_le_bytes());
    out
}

/// Computes the SHA-1 digest (RFC 3174) of `data`.
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let (mut h0, mut h1, mut h2, mut h3, mut h4) = (
        0x67452301u32,
        0xEFCDAB89u32,
        0x98BADCFEu32,
        0x10325476u32,
        0xC3D2E1F0u32,
    );

    // Padding matches SHA-256: 0x80, zeros to 56 (mod 64), 64-bit big-endian len.
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0x00);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            *word = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h0, h1, h2, h3, h4);
        for (i, &word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1u32),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDCu32),
                _ => (b ^ c ^ d, 0xCA62C1D6u32),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut out = [0u8; 20];
    out[0..4].copy_from_slice(&h0.to_be_bytes());
    out[4..8].copy_from_slice(&h1.to_be_bytes());
    out[8..12].copy_from_slice(&h2.to_be_bytes());
    out[12..16].copy_from_slice(&h3.to_be_bytes());
    out[16..20].copy_from_slice(&h4.to_be_bytes());
    out
}

/// Hash algorithm selector, mirroring Godot's `HashingContext.HashType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashType {
    /// MD5 (128-bit digest).
    Md5,
    /// SHA-1 (160-bit digest).
    Sha1,
    /// SHA-256 (256-bit digest).
    Sha256,
}

impl HashType {
    /// Digest length in bytes for this hash type.
    pub fn digest_len(self) -> usize {
        match self {
            HashType::Md5 => 16,
            HashType::Sha1 => 20,
            HashType::Sha256 => 32,
        }
    }

    /// Internal compression block size in bytes (used for HMAC key processing).
    /// MD5, SHA-1, and SHA-256 all use a 64-byte block.
    fn block_size(self) -> usize {
        64
    }
}

/// Computes the digest of `data` under the selected [`HashType`].
pub fn hash(kind: HashType, data: &[u8]) -> Vec<u8> {
    match kind {
        HashType::Md5 => md5(data).to_vec(),
        HashType::Sha1 => sha1(data).to_vec(),
        HashType::Sha256 => sha256(data).to_vec(),
    }
}

/// Computes HMAC (RFC 2104) of `msg` under `key` using the selected hash.
pub fn hmac(kind: HashType, key: &[u8], msg: &[u8]) -> Vec<u8> {
    let block = kind.block_size();
    // Keys longer than the block size are hashed down first, then all keys are
    // zero-padded to the block size.
    let mut k = if key.len() > block {
        hash(kind, key)
    } else {
        key.to_vec()
    };
    k.resize(block, 0x00);

    let ipad: Vec<u8> = k.iter().map(|&b| b ^ 0x36).collect();
    let opad: Vec<u8> = k.iter().map(|&b| b ^ 0x5c).collect();

    // inner = H(ipad || msg)
    let mut inner = ipad;
    inner.extend_from_slice(msg);
    let inner_hash = hash(kind, &inner);

    // outer = H(opad || inner_hash)
    let mut outer = opad;
    outer.extend_from_slice(&inner_hash);
    hash(kind, &outer)
}

/// Incremental multi-algorithm hashing context mirroring Godot's
/// `HashingContext` (selectable MD5/SHA-1/SHA-256). Buffers input and hashes
/// on `finish`. The SHA-256-only [`HashingContext`] above is retained for
/// existing callers that want streaming SHA-256.
#[derive(Debug, Clone, Default)]
pub struct MultiHashContext {
    kind: Option<HashType>,
    buffer: Vec<u8>,
}

impl MultiHashContext {
    /// Creates an unstarted context.
    pub fn new() -> Self {
        Self {
            kind: None,
            buffer: Vec::new(),
        }
    }

    /// Starts (or restarts) hashing under `kind`.
    pub fn start(&mut self, kind: HashType) {
        self.kind = Some(kind);
        self.buffer.clear();
    }

    /// Appends `data` to the running hash. Errors if not started.
    pub fn update(&mut self, data: &[u8]) -> Result<(), &'static str> {
        if self.kind.is_none() {
            return Err("HashingContext not started");
        }
        self.buffer.extend_from_slice(data);
        Ok(())
    }

    /// Finishes and returns the digest, resetting the context. Errors if not
    /// started.
    pub fn finish(&mut self) -> Result<Vec<u8>, &'static str> {
        let kind = self.kind.ok_or("HashingContext not started")?;
        let out = hash(kind, &self.buffer);
        self.kind = None;
        self.buffer.clear();
        Ok(out)
    }
}

/// Incremental HMAC context mirroring Godot's `HMACContext`
/// (`start(type, key)` / `update(data)` / `finish()`).
#[derive(Debug, Clone, Default)]
pub struct HmacContext {
    kind: Option<HashType>,
    key: Vec<u8>,
    buffer: Vec<u8>,
}

impl HmacContext {
    /// Creates an unstarted HMAC context.
    pub fn new() -> Self {
        Self {
            kind: None,
            key: Vec::new(),
            buffer: Vec::new(),
        }
    }

    /// Starts HMAC under `kind` with `key`.
    pub fn start(&mut self, kind: HashType, key: &[u8]) {
        self.kind = Some(kind);
        self.key = key.to_vec();
        self.buffer.clear();
    }

    /// Appends message bytes. Errors if not started.
    pub fn update(&mut self, data: &[u8]) -> Result<(), &'static str> {
        if self.kind.is_none() {
            return Err("HMACContext not started");
        }
        self.buffer.extend_from_slice(data);
        Ok(())
    }

    /// Finishes and returns the HMAC tag, resetting the context. Errors if not
    /// started.
    pub fn finish(&mut self) -> Result<Vec<u8>, &'static str> {
        let kind = self.kind.ok_or("HMACContext not started")?;
        let out = hmac(kind, &self.key, &self.buffer);
        self.kind = None;
        self.key.clear();
        self.buffer.clear();
        Ok(out)
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
    fn aes_encrypt_decrypt_roundtrip() {
        let key = [0x11u8; 32];
        let iv = [0x22u8; 16];
        let plain = b"Hello Patina AES-256-CBC!!";
        let mut enc = AesContext::new();
        enc.start(AesMode::Encrypt, &key, &iv).unwrap();
        let cipher = enc.update(plain).unwrap();
        assert!(!cipher.is_empty());
        let mut dec = AesContext::new();
        dec.start(AesMode::Decrypt, &key, &iv).unwrap();
        let out = dec.update(&cipher).unwrap();
        assert_eq!(&out, plain.as_slice());
        enc.finish().unwrap();
    }

    #[test]
    fn aes_update_before_start_errors() {
        let ctx = AesContext::new();
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

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }

    #[test]
    fn md5_known_vectors() {
        // RFC 1321 / canonical vectors.
        assert_eq!(hex(&md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex(&md5(b"abc")), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            hex(&md5(b"The quick brown fox jumps over the lazy dog")),
            "9e107d9d372bb6826bd81d3542a419d6"
        );
    }

    #[test]
    fn sha1_known_vectors() {
        // FIPS 180 / canonical vectors.
        assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(
            hex(&sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            hex(&sha1(b"The quick brown fox jumps over the lazy dog")),
            "2fd4e1c67a2d28fced849ee1bb76e7391b93eb12"
        );
    }

    #[test]
    fn hash_dispatch_matches_direct_and_reports_len() {
        assert_eq!(hash(HashType::Md5, b"abc"), md5(b"abc").to_vec());
        assert_eq!(hash(HashType::Sha1, b"abc"), sha1(b"abc").to_vec());
        assert_eq!(hash(HashType::Sha256, b"abc"), sha256(b"abc").to_vec());
        assert_eq!(HashType::Md5.digest_len(), 16);
        assert_eq!(HashType::Sha1.digest_len(), 20);
        assert_eq!(HashType::Sha256.digest_len(), 32);
    }

    #[test]
    fn hmac_md5_rfc2202_case1() {
        // RFC 2202 test case 1: key = 0x0b × 16, data = "Hi There".
        let key = [0x0bu8; 16];
        assert_eq!(
            hex(&hmac(HashType::Md5, &key, b"Hi There")),
            "9294727a3638bb1c13f48ef8158bfc9d"
        );
    }

    #[test]
    fn hmac_sha1_rfc2202_case1() {
        // RFC 2202 test case 1: key = 0x0b × 20, data = "Hi There".
        let key = [0x0bu8; 20];
        assert_eq!(
            hex(&hmac(HashType::Sha1, &key, b"Hi There")),
            "b617318655057264e28bc0b6fb378c8ef146be00"
        );
    }

    #[test]
    fn hmac_sha256_rfc4231_case1() {
        // RFC 4231 test case 1: key = 0x0b × 20, data = "Hi There".
        let key = [0x0bu8; 20];
        assert_eq!(
            hex(&hmac(HashType::Sha256, &key, b"Hi There")),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn hmac_long_key_is_hashed_first() {
        // Over-block-size key (131 bytes) must be hashed down first; assert the
        // tag is correct-length and deterministic.
        let key = [0xaau8; 131];
        let tag1 = hmac(
            HashType::Sha256,
            &key,
            b"Test Using Larger Than Block-Size Key",
        );
        let tag2 = hmac(
            HashType::Sha256,
            &key,
            b"Test Using Larger Than Block-Size Key",
        );
        assert_eq!(tag1.len(), 32);
        assert_eq!(tag1, tag2);
    }

    #[test]
    fn multi_hash_context_incremental_matches_oneshot() {
        let mut ctx = MultiHashContext::new();
        assert!(ctx.update(b"x").is_err()); // not started
        ctx.start(HashType::Sha1);
        ctx.update(b"a").unwrap();
        ctx.update(b"bc").unwrap();
        assert_eq!(ctx.finish().unwrap(), sha1(b"abc").to_vec());
    }

    #[test]
    fn hmac_context_matches_oneshot() {
        let key = [0x0bu8; 20];
        let mut ctx = HmacContext::new();
        assert!(ctx.update(b"x").is_err()); // not started
        ctx.start(HashType::Sha256, &key);
        ctx.update(b"Hi ").unwrap();
        ctx.update(b"There").unwrap();
        assert_eq!(
            ctx.finish().unwrap(),
            hmac(HashType::Sha256, &key, b"Hi There")
        );
    }
}
