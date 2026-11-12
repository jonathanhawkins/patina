//! Browser-decoded MP3 for the web runtime (pat-mgixk.4, item 4).
//!
//! # The gap
//!
//! Godot's web export has no MP3 codec of its own either — the browser has one,
//! and the export leans on it. Patina's `gdaudio` decodes ISO MP3 with
//! `minimp3`, which wraps C sources through `cc` and is therefore gated to
//! non-wasm targets (`gdaudio/Cargo.toml`). So on the web every
//! `AudioStreamMP3` in a scene came back `UnsupportedFormat` from
//! `decode_mp3_frames` and played silence, however correct the project was.
//!
//! # Why this is a cache and not a call
//!
//! The obvious fix — "call `decodeAudioData`" — does not fit where the decode
//! happens. `AudioServerBridge::process` loads a stream **synchronously**,
//! inside a frame, through `AudioStreamLoader`; `decodeAudioData` is
//! asynchronous and resolves on a later task. Nothing can await inside that
//! frame.
//!
//! So the direction is inverted: the page decodes its MP3 assets up front and
//! *registers* the PCM here, and [`gdaudio::decode::decode_mp3`]'s host-decoder
//! hook answers out of what has been registered. That is also how a web game
//! loads assets anyway — fetch and decode during a loading screen, play during
//! gameplay.
//!
//! ```text
//! fetch("music.mp3") -> ArrayBuffer
//!   -> AudioContext.decodeAudioData        the browser's own MP3 codec
//!     -> WebAudio.registerDecodedMp3(bytes, planar, rate, channels)
//!       -> BrowserMp3Decoder (installed as gdaudio's host decoder)
//!         -> AudioStreamLoader -> AudioServerBridge::process -> ring -> worklet
//! ```
//!
//! # Keying
//!
//! Entries are keyed by the **compressed bytes themselves**, not by a hash or a
//! `res://` path. A hash would need a collision story, and the path is not
//! available at the point the hook runs — `decode_mp3` receives bytes and
//! nothing else. The compressed bytes are an order of magnitude smaller than
//! the PCM they decode to, so keeping them costs little next to what is already
//! being stored.
//!
//! Deliberately portable Rust with no browser bindings, so the whole path is
//! driven by native tests; [`crate::web_audio`] is the thin `wasm_bindgen`
//! shell over it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use gdaudio::decode::{AudioBuffer, Mp3Decoder};

/// PCM the browser decoded, keyed by the MP3 bytes it came from.
///
/// Cloning shares one store, which is the whole point: the clone installed as
/// `gdaudio`'s host decoder and the handle the page registers through must see
/// the same entries.
#[derive(Clone, Default)]
pub struct BrowserMp3Decoder {
    decoded: Arc<Mutex<HashMap<Vec<u8>, AudioBuffer>>>,
}

impl BrowserMp3Decoder {
    /// An empty store, not yet installed. See [`Self::install`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Installs a clone of this decoder as `gdaudio`'s host MP3 decoder.
    ///
    /// A clone rather than a move: the installed decoder and this handle share
    /// one store, so registrations made afterwards are visible to the engine.
    /// Installing twice is harmless — the second replaces the first.
    pub fn install(&self) {
        gdaudio::decode::install_mp3_decoder(Arc::new(self.clone()));
    }

    /// Registers interleaved PCM for the MP3 in `mp3`, returning the number of
    /// entries now held.
    ///
    /// Rejects anything that cannot be played rather than storing it:
    ///
    /// * a zero `sample_rate` would make `AudioBuffer::duration_secs` divide by
    ///   zero and the resampler produce nothing;
    /// * a zero `channels` makes `frame_count` zero, so the stream is silent
    ///   while reporting success — the exact failure this bead exists to fix;
    /// * a sample count that is not a whole number of frames means the
    ///   interleaving is wrong, and the audio would come out with its channels
    ///   progressively swapped rather than obviously broken.
    pub fn register(
        &self,
        mp3: &[u8],
        interleaved: Vec<f32>,
        sample_rate: u32,
        channels: u16,
    ) -> Result<usize, String> {
        if mp3.is_empty() {
            return Err("cannot register PCM for an empty MP3".to_string());
        }
        if sample_rate == 0 {
            return Err("decoded PCM needs a non-zero sample rate".to_string());
        }
        if channels == 0 {
            return Err("decoded PCM needs at least one channel".to_string());
        }
        if interleaved.is_empty() {
            return Err("decoded PCM is empty".to_string());
        }
        if !interleaved.len().is_multiple_of(channels as usize) {
            return Err(format!(
                "{} interleaved samples is not a whole number of {channels}-channel frames",
                interleaved.len()
            ));
        }
        let buffer = AudioBuffer {
            samples: interleaved,
            sample_rate,
            channels,
        };
        let mut store = self.decoded.lock().unwrap_or_else(|e| e.into_inner());
        store.insert(mp3.to_vec(), buffer);
        Ok(store.len())
    }

    /// Registers channel-major PCM, interleaving it on the way in.
    ///
    /// This is the shape `decodeAudioData` hands JS back: one `Float32Array`
    /// per channel, which a page concatenates. Interleaving here rather than in
    /// JS keeps the one piece of index arithmetic in the tested language.
    pub fn register_planar(
        &self,
        mp3: &[u8],
        planar: &[f32],
        sample_rate: u32,
        channels: u16,
    ) -> Result<usize, String> {
        if channels == 0 {
            return Err("decoded PCM needs at least one channel".to_string());
        }
        let channels_usize = channels as usize;
        if !planar.len().is_multiple_of(channels_usize) {
            return Err(format!(
                "{} planar samples do not divide into {channels} equal channels",
                planar.len()
            ));
        }
        let frames = planar.len() / channels_usize;
        let mut interleaved = vec![0.0f32; planar.len()];
        for channel in 0..channels_usize {
            let plane = &planar[channel * frames..(channel + 1) * frames];
            for (frame, &sample) in plane.iter().enumerate() {
                interleaved[frame * channels_usize + channel] = sample;
            }
        }
        self.register(mp3, interleaved, sample_rate, channels)
    }

    /// Whether PCM has been registered for exactly these bytes.
    pub fn has(&self, mp3: &[u8]) -> bool {
        let store = self.decoded.lock().unwrap_or_else(|e| e.into_inner());
        store.contains_key(mp3)
    }

    /// How many decoded streams are held.
    ///
    /// A page reporting 0 here is a page whose MP3s are all silent, which is
    /// otherwise indistinguishable from a scene that has no music in it.
    pub fn registered(&self) -> usize {
        let store = self.decoded.lock().unwrap_or_else(|e| e.into_inner());
        store.len()
    }

    /// Drops one entry, returning whether it was there. Frees the PCM for an
    /// asset the game is done with.
    pub fn forget(&self, mp3: &[u8]) -> bool {
        let mut store = self.decoded.lock().unwrap_or_else(|e| e.into_inner());
        store.remove(mp3).is_some()
    }

    /// Drops every entry.
    pub fn clear(&self) {
        let mut store = self.decoded.lock().unwrap_or_else(|e| e.into_inner());
        store.clear();
    }
}

impl Mp3Decoder for BrowserMp3Decoder {
    /// Answers out of the store, or `None` for bytes nothing has registered.
    ///
    /// `None` rather than an error on a miss: it lets `decode_mp3` fall through
    /// to whatever codec the target does have, so installing this on a native
    /// build never removes `minimp3`'s real decoding.
    fn decode(&self, data: &[u8]) -> Option<AudioBuffer> {
        let store = self.decoded.lock().unwrap_or_else(|e| e.into_inner());
        store.get(data).cloned()
    }
}

impl std::fmt::Debug for BrowserMp3Decoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrowserMp3Decoder")
            .field("registered", &self.registered())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes the tests that install into the process-global decoder slot,
    /// and empties it again on the way out. Bound once per test rather than
    /// reset at the top of each, so two threads cannot interleave between a
    /// clear and an install. Poison-tolerant.
    static DECODER_SLOT: Mutex<()> = Mutex::new(());

    #[must_use]
    struct DecoderSlot(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

    impl DecoderSlot {
        fn acquire() -> Self {
            Self(DECODER_SLOT.lock().unwrap_or_else(|e| e.into_inner()))
        }
    }

    impl Drop for DecoderSlot {
        fn drop(&mut self) {
            gdaudio::decode::clear_mp3_decoder();
        }
    }

    /// Bytes that pass `gdaudio::is_mp3` (the `ID3` prefix) but hold no
    /// decodable frame, so no linked codec can turn them into audio on any
    /// target — which is the browser's situation for *every* ISO MP3.
    fn undecodable_mp3() -> Vec<u8> {
        let mut data = b"ID3\x04\x00\x00\x00\x00\x00\x0a".to_vec();
        data.extend_from_slice(&[0u8; 10]);
        data
    }

    #[test]
    fn a_registered_stream_decodes_through_gdaudios_hook() {
        let _slot = DecoderSlot::acquire();
        let mp3 = undecodable_mp3();
        assert!(
            gdaudio::decode::decode_mp3(&mp3).is_err(),
            "the control is wrong: these bytes must be undecodable without the \
             browser's PCM, or this test proves nothing"
        );

        let decoder = BrowserMp3Decoder::new();
        decoder.install();
        assert_eq!(
            decoder
                .register(&mp3, vec![0.1, -0.1, 0.2, -0.2], 48_000, 2)
                .expect("register the browser's PCM"),
            1
        );

        let buffer = gdaudio::decode::decode_mp3(&mp3).expect("the hook must answer");
        assert_eq!(buffer.sample_rate, 48_000);
        assert_eq!(buffer.channels, 2);
        assert_eq!(buffer.frame_count(), 2);
        assert_eq!(buffer.samples, vec![0.1, -0.1, 0.2, -0.2]);
    }

    /// Different bytes are different assets. Keying on the compressed bytes is
    /// only correct if a near-miss does not collide.
    #[test]
    fn a_different_stream_is_a_miss_not_the_wrong_audio() {
        let _slot = DecoderSlot::acquire();
        let decoder = BrowserMp3Decoder::new();
        decoder.install();
        let mut registered = undecodable_mp3();
        registered.push(1);
        let mut other = undecodable_mp3();
        other.push(2);
        decoder.register(&registered, vec![0.5], 44_100, 1).unwrap();

        assert!(decoder.has(&registered));
        assert!(!decoder.has(&other));
        assert!(
            gdaudio::decode::decode_mp3(&other).is_err(),
            "an unregistered stream must miss rather than return a neighbour's PCM"
        );
    }

    /// A clone shares the store — the property that lets `install` hand a clone
    /// to `gdaudio` while the page keeps registering through this handle.
    #[test]
    fn a_clone_sees_registrations_made_through_the_original() {
        let decoder = BrowserMp3Decoder::new();
        let clone = decoder.clone();
        decoder.register(b"ID3fake", vec![0.25], 44_100, 1).unwrap();
        assert_eq!(clone.registered(), 1);
        assert!(clone.has(b"ID3fake"));
        assert!(clone.decode(b"ID3fake").is_some());
    }

    /// `decodeAudioData` hands JS one array per channel; the interleave has to
    /// land each plane in the right slot, not concatenate them.
    #[test]
    fn planar_pcm_is_interleaved_on_the_way_in() {
        let decoder = BrowserMp3Decoder::new();
        // Left = [1, 2, 3], right = [-1, -2, -3].
        decoder
            .register_planar(b"ID3planar", &[1.0, 2.0, 3.0, -1.0, -2.0, -3.0], 44_100, 2)
            .expect("register planar PCM");
        let buffer = decoder.decode(b"ID3planar").expect("registered");
        assert_eq!(buffer.frame_count(), 3);
        assert_eq!(
            buffer.samples,
            vec![1.0, -1.0, 2.0, -2.0, 3.0, -3.0],
            "frames must be (L,R) pairs; a concatenation would leave the right \
             channel playing three frames late"
        );
    }

    #[test]
    fn mono_planar_pcm_is_unchanged() {
        let decoder = BrowserMp3Decoder::new();
        decoder
            .register_planar(b"ID3mono", &[0.1, 0.2, 0.3], 22_050, 1)
            .unwrap();
        let buffer = decoder.decode(b"ID3mono").unwrap();
        assert_eq!(buffer.samples, vec![0.1, 0.2, 0.3]);
        assert_eq!(buffer.channels, 1);
    }

    /// Unplayable PCM is refused rather than stored. A zero channel count in
    /// particular would store happily and then play silence, which is the exact
    /// symptom this bead is about — so it must not be reachable by accident.
    #[test]
    fn unplayable_pcm_is_refused() {
        let decoder = BrowserMp3Decoder::new();
        assert!(decoder.register(b"", vec![0.1], 44_100, 1).is_err());
        assert!(decoder.register(b"ID3x", vec![0.1], 0, 1).is_err());
        assert!(decoder.register(b"ID3x", vec![0.1], 44_100, 0).is_err());
        assert!(decoder.register(b"ID3x", Vec::new(), 44_100, 1).is_err());
        assert!(
            decoder
                .register(b"ID3x", vec![0.1, 0.2, 0.3], 44_100, 2)
                .is_err(),
            "3 samples is not a whole number of stereo frames"
        );
        assert!(decoder.register_planar(b"ID3x", &[0.1], 44_100, 0).is_err());
        assert!(
            decoder
                .register_planar(b"ID3x", &[0.1, 0.2, 0.3], 44_100, 2)
                .is_err(),
            "3 planar samples do not split into two equal channels"
        );
        assert_eq!(decoder.registered(), 0, "nothing was stored");
    }

    #[test]
    fn forget_and_clear_release_entries() {
        let decoder = BrowserMp3Decoder::new();
        decoder.register(b"ID3a", vec![0.1], 44_100, 1).unwrap();
        decoder.register(b"ID3b", vec![0.2], 44_100, 1).unwrap();
        assert_eq!(decoder.registered(), 2);
        assert!(decoder.forget(b"ID3a"));
        assert!(!decoder.forget(b"ID3a"), "forgetting twice is not an error");
        assert_eq!(decoder.registered(), 1);
        decoder.clear();
        assert_eq!(decoder.registered(), 0);
    }

    /// Installing must not take MP3 support away from a target that has a real
    /// codec: a miss falls through to it.
    #[test]
    fn an_empty_decoder_leaves_the_built_in_codec_in_charge() {
        let _slot = DecoderSlot::acquire();
        BrowserMp3Decoder::new().install();
        assert!(
            gdaudio::decode::installed_mp3_decoder().is_some(),
            "install must reach gdaudio's slot"
        );
        // Still the built-in codec's answer, not a hook error.
        assert!(gdaudio::decode::decode_mp3(&undecodable_mp3()).is_err());
    }
}
