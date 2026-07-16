//! IMA ADPCM (WAV format tag 0x11) decoder — pat-u3l8c.108 (IMA-ADPCM
//! sub-feature).
//!
//! Godot's WAV importer accepts `WAVE_FORMAT_IMA_ADPCM` (a 4:1 compressed PCM
//! format); Patina's `wav.rs` currently rejects audio format 2/17 as
//! unsupported. This module is the self-contained decode core: a pure-`std`
//! implementation of the Microsoft/IMA block ADPCM state machine that expands
//! compressed blocks back to signed 16-bit PCM.
//!
//! # Format
//!
//! ADPCM data is split into fixed-size blocks (`block_align` bytes). Each block
//! begins with a 4-byte header **per channel** — a signed 16-bit initial
//! predictor (the first decoded sample), a step-table index byte, and a
//! reserved byte — followed by 4-bit nibble codes. For multi-channel audio the
//! nibble data is interleaved in 4-byte words, one word per channel at a time;
//! the low nibble of each byte is decoded before the high nibble.
//!
//! # Scope / remaining work (pat-u3l8c.108, a 4-feature pack)
//!
//! This delivers only the **IMA-ADPCM decode** sub-feature. The pack's other
//! three sub-features are NOT implemented here and remain open: loop-mode
//! ping-pong / sub-buffer loop points (`stream.rs`), `AudioStreamRandomizer`,
//! and `get_playback_position` latency compensation (`stream.rs`). Wiring this
//! decoder into `wav.rs`'s format dispatch is also left to the owner of that
//! (contended) file — doing it here would break its existing
//! `reject_unsupported_format` test.

/// IMA ADPCM step-size table (89 entries), indexed by the adaptive step index.
const STEP_TABLE: &[i32] = &[
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, //
    19, 21, 23, 25, 28, 31, 34, 37, 41, 45, //
    50, 55, 60, 66, 73, 80, 88, 97, 107, 118, //
    130, 143, 157, 173, 190, 209, 230, 253, 279, 307, //
    337, 371, 408, 449, 494, 544, 598, 658, 724, 796, //
    876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, //
    2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, //
    5894, 6484, 7132, 7845, 8630, 9493, 10442, 11487, 12635, 13899, //
    15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];

/// Per-nibble step-index adjustment (16 entries).
const INDEX_TABLE: &[i32] = &[-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];

/// The largest valid step index (`STEP_TABLE.len() - 1`).
const MAX_STEP_INDEX: i32 = 88;

/// The adaptive decoder state for a single channel.
#[derive(Debug, Clone, Copy)]
struct AdpcmState {
    predictor: i32,
    step_index: i32,
}

impl AdpcmState {
    fn new(predictor: i32, step_index: i32) -> Self {
        Self {
            predictor,
            step_index: step_index.clamp(0, MAX_STEP_INDEX),
        }
    }

    /// Advances the state by one 4-bit `nibble` and returns the decoded sample.
    fn step(&mut self, nibble: u8) -> i16 {
        let nibble = (nibble & 0x0f) as usize;
        let step = STEP_TABLE[self.step_index as usize];

        // Reconstruct the magnitude of the difference from the low 3 bits.
        let mut diff = step >> 3;
        if nibble & 4 != 0 {
            diff += step;
        }
        if nibble & 2 != 0 {
            diff += step >> 1;
        }
        if nibble & 1 != 0 {
            diff += step >> 2;
        }

        // The high bit is the sign.
        if nibble & 8 != 0 {
            self.predictor -= diff;
        } else {
            self.predictor += diff;
        }
        self.predictor = self.predictor.clamp(i16::MIN as i32, i16::MAX as i32);

        self.step_index = (self.step_index + INDEX_TABLE[nibble]).clamp(0, MAX_STEP_INDEX);
        self.predictor as i16
    }
}

/// Decodes an IMA ADPCM byte stream into interleaved signed 16-bit PCM.
///
/// `channels` is the channel count (>= 1) and `block_align` is the encoder's
/// block size in bytes. Malformed trailing bytes that cannot form a full block
/// header are ignored.
///
/// ```
/// use gdaudio::ima_adpcm::decode_ima_adpcm;
///
/// // One mono block: predictor = 1000, step index 0, then zero nibbles.
/// let mut block = vec![0xE8, 0x03, 0x00, 0x00]; // header
/// block.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // 8 zero nibbles
/// let pcm = decode_ima_adpcm(&block, 1, block.len() as u16);
/// assert_eq!(pcm[0], 1000); // header sample
/// assert!(pcm.iter().all(|&s| s == 1000)); // zero nibbles hold the predictor
/// ```
pub fn decode_ima_adpcm(data: &[u8], channels: u16, block_align: u16) -> Vec<i16> {
    let channels = channels.max(1) as usize;
    let min_block = 4 * channels;
    let block_align = (block_align as usize).max(min_block);

    let mut out = Vec::new();
    let mut off = 0;
    while off < data.len() {
        let end = (off + block_align).min(data.len());
        decode_block(&data[off..end], channels, &mut out);
        off += block_align;
    }
    out
}

/// Convenience wrapper returning normalized `f32` samples in `[-1, 1]`, matching
/// `wav.rs`'s sample convention.
pub fn decode_ima_adpcm_f32(data: &[u8], channels: u16, block_align: u16) -> Vec<f32> {
    decode_ima_adpcm(data, channels, block_align)
        .into_iter()
        .map(|s| s as f32 / 32768.0)
        .collect()
}

/// Decodes one block, appending interleaved samples to `out`.
fn decode_block(block: &[u8], channels: usize, out: &mut Vec<i16>) {
    let header_len = 4 * channels;
    if channels == 0 || block.len() < header_len {
        return;
    }

    let mut states: Vec<AdpcmState> = Vec::with_capacity(channels);
    let mut chan: Vec<Vec<i16>> = Vec::with_capacity(channels);
    for ch in 0..channels {
        let base = ch * 4;
        let predictor = i16::from_le_bytes([block[base], block[base + 1]]) as i32;
        let index = block[base + 2] as i32;
        states.push(AdpcmState::new(predictor, index));
        // The header predictor is the channel's first output sample.
        chan.push(vec![predictor as i16]);
    }

    // Nibble data: 4-byte words, one channel at a time per group.
    let data = &block[header_len..];
    const WORD: usize = 4;
    let group = WORD * channels;
    let mut off = 0;
    while off + group <= data.len() {
        for (ch, state) in states.iter_mut().enumerate() {
            let word = &data[off + ch * WORD..off + ch * WORD + WORD];
            for &byte in word {
                let low = byte & 0x0f;
                let high = (byte >> 4) & 0x0f;
                let s0 = state.step(low);
                chan[ch].push(s0);
                let s1 = state.step(high);
                chan[ch].push(s1);
            }
        }
        off += group;
    }

    // Interleave channel sample streams into frames.
    let frames = chan[0].len();
    for f in 0..frames {
        for c in chan.iter() {
            out.push(c[f]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a single mono block: 4-byte header + `data` nibble bytes.
    fn mono_block(predictor: i16, index: u8, data: &[u8]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&predictor.to_le_bytes());
        b.push(index);
        b.push(0); // reserved
        b.extend_from_slice(data);
        b
    }

    #[test]
    fn step_table_has_89_entries() {
        assert_eq!(STEP_TABLE.len(), 89, "IMA step table must have 89 entries");
        assert_eq!(STEP_TABLE[0], 7);
        assert_eq!(STEP_TABLE[88], 32767);
        assert_eq!(MAX_STEP_INDEX as usize, STEP_TABLE.len() - 1);
    }

    #[test]
    fn index_table_has_16_entries() {
        assert_eq!(INDEX_TABLE.len(), 16);
        assert_eq!(INDEX_TABLE[0], -1);
        assert_eq!(INDEX_TABLE[7], 8);
    }

    #[test]
    fn header_predictor_is_first_sample() {
        let block = mono_block(1234, 0, &[0x00, 0x00, 0x00, 0x00]);
        let pcm = decode_ima_adpcm(&block, 1, block.len() as u16);
        assert_eq!(pcm[0], 1234);
    }

    #[test]
    fn zero_nibbles_hold_the_predictor() {
        // nibble 0 at index 0: diff = step(7) >> 3 = 0, so the predictor never
        // moves; the step index decays toward 0 and stays there.
        let block = mono_block(1000, 0, &[0x00, 0x00, 0x00, 0x00]);
        let pcm = decode_ima_adpcm(&block, 1, block.len() as u16);
        assert_eq!(pcm.len(), 1 + 8, "1 header + 8 nibble samples");
        assert!(pcm.iter().all(|&s| s == 1000), "got {pcm:?}");
    }

    #[test]
    fn nibbles_reconstruct_known_deltas() {
        // predictor 0, index 0. First byte 0x04 => low nibble 4, high nibble 0.
        // nibble 4 @ step 7: diff = 0 + 7 = 7 -> predictor 7, index -> 2.
        // nibble 0 @ step 9 (index 2): diff = 9>>3 = 1 -> predictor 8, index -> 1.
        let block = mono_block(0, 0, &[0x04, 0x00, 0x00, 0x00]);
        let pcm = decode_ima_adpcm(&block, 1, block.len() as u16);
        assert_eq!(pcm[0], 0, "header");
        assert_eq!(pcm[1], 7, "low nibble 4");
        assert_eq!(pcm[2], 8, "high nibble 0");
    }

    #[test]
    fn sign_bit_decreases_predictor() {
        // nibble 0xC = 1100: sign bit set (subtract), magnitude bit 4 set.
        // step 7: diff = 0 + 7 = 7 -> predictor 0 - 7 = -7.
        let block = mono_block(0, 0, &[0x0C, 0x00, 0x00, 0x00]);
        let pcm = decode_ima_adpcm(&block, 1, block.len() as u16);
        assert_eq!(pcm[1], -7);
    }

    #[test]
    fn output_length_matches_block_geometry() {
        // 1 mono block, block_align 12 => 4 header + 8 data bytes = 16 nibbles.
        let block = mono_block(0, 0, &[0x11; 8]);
        let pcm = decode_ima_adpcm(&block, 1, block.len() as u16);
        assert_eq!(pcm.len(), 1 + 16);
    }

    #[test]
    fn extreme_nibbles_clamp_without_overflow() {
        // Repeated max-magnitude positive nibbles (7) must saturate at i16::MAX
        // and never panic.
        let block = mono_block(0, 88, &[0x77; 64]);
        let pcm = decode_ima_adpcm(&block, 1, block.len() as u16);
        assert!(pcm.iter().all(|&s| (i16::MIN..=i16::MAX).contains(&s)));
        assert_eq!(*pcm.iter().max().unwrap(), i16::MAX, "should saturate high");
    }

    #[test]
    fn stereo_interleaves_channels_with_header_predictors() {
        // Two channels: L predictor 100, R predictor -100, then all-zero data
        // so both channels hold their predictor.
        let mut block = Vec::new();
        block.extend_from_slice(&100i16.to_le_bytes()); // L predictor
        block.push(0); // L index
        block.push(0); // reserved
        block.extend_from_slice(&(-100i16).to_le_bytes()); // R predictor
        block.push(0); // R index
        block.push(0); // reserved
        block.extend_from_slice(&[0x00; 8]); // one 4-byte word per channel

        let pcm = decode_ima_adpcm(&block, 2, block.len() as u16);
        // 1 header frame + 8 nibble frames = 9 frames * 2 channels.
        assert_eq!(pcm.len(), 9 * 2);
        assert_eq!(pcm[0], 100, "frame 0 left = L predictor");
        assert_eq!(pcm[1], -100, "frame 0 right = R predictor");
        for frame in pcm.chunks(2) {
            assert_eq!(frame[0], 100);
            assert_eq!(frame[1], -100);
        }
    }

    #[test]
    fn f32_conversion_is_in_range() {
        let block = mono_block(0, 40, &[0xA5; 16]);
        let pcm = decode_ima_adpcm_f32(&block, 1, block.len() as u16);
        assert!(!pcm.is_empty());
        assert!(
            pcm.iter().all(|&s| (-1.0..=1.0).contains(&s)),
            "out of range"
        );
    }

    #[test]
    fn empty_and_truncated_input_do_not_panic() {
        assert!(decode_ima_adpcm(&[], 1, 4).is_empty());
        // Fewer bytes than a header: no full block, nothing decoded.
        assert!(decode_ima_adpcm(&[0x00, 0x01], 1, 4).is_empty());
    }
}
