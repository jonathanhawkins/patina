//! pat-p0s82.13 — real ISO MP3 decoding.
//!
//! Before this bead `decode_mp3` only understood the synthetic `PATMP3` unit
//! fixture and returned `UnsupportedFormat` for genuine MP3 streams. This
//! app-level test decodes a real LAME-encoded MP3 (ID3v2 tag + MP3 frames:
//! a 440 Hz sine, ~0.12 s, 44.1 kHz stereo) end-to-end through the public
//! `gdaudio::decode_mp3` API and asserts real PCM comes out.

use gdaudio::{decode_mp3, is_mp3, AudioBuffer};

/// A tiny real MP3 committed as a test fixture (see tests/fixtures/). Being a
/// genuine ISO stream — not the `PATMP3` shortcut — it exercises the actual
/// minimp3 decode path.
static TONE_MP3: &[u8] = include_bytes!("fixtures/test_tone.mp3");

#[test]
fn real_iso_mp3_decodes_to_pcm() {
    assert!(
        is_mp3(TONE_MP3),
        "fixture must be recognized as MP3 (ID3 tag / frame sync)"
    );

    let buf: AudioBuffer = decode_mp3(TONE_MP3).expect("real ISO MP3 should decode");

    assert_eq!(
        buf.sample_rate, 44_100,
        "sample rate read from the MP3 header"
    );
    assert_eq!(buf.channels, 2, "fixture is stereo");

    // ~0.12 s of stereo at 44.1 kHz is on the order of ten thousand interleaved
    // samples; the whole point of the bead is that real frames yield real PCM.
    assert!(
        buf.samples.len() > 1_000,
        "expected substantial decoded PCM, got {} samples",
        buf.samples.len()
    );
    // Interleaved stereo frames => an even number of samples.
    assert_eq!(
        buf.samples.len() % 2,
        0,
        "interleaved stereo => even sample count"
    );
    // minimp3 output is normalized to f32 in [-1, 1].
    assert!(
        buf.samples.iter().all(|s| s.is_finite() && s.abs() <= 1.0),
        "PCM must be finite and normalized to [-1, 1]"
    );
}

#[test]
fn non_mp3_bytes_are_rejected() {
    let junk = [0u8, 1, 2, 3, 4, 5, 6, 7];
    assert!(!is_mp3(&junk), "arbitrary bytes are not MP3");
    assert!(
        decode_mp3(&junk).is_err(),
        "non-MP3 input must not decode to audio"
    );
}
