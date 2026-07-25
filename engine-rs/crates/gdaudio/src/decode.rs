//! Audio decoding for WAV and OGG Vorbis formats.
//!
//! Decodes audio files into a unified [`AudioBuffer`] of interleaved f32 samples.

use std::io::{Cursor, Read, Seek};

/// A decoded audio buffer holding interleaved f32 samples.
#[derive(Debug, Clone)]
pub struct AudioBuffer {
    /// Interleaved sample data, normalized to [-1.0, 1.0].
    pub samples: Vec<f32>,
    /// Sample rate in Hz (e.g. 44100, 48000).
    pub sample_rate: u32,
    /// Number of channels (1 = mono, 2 = stereo).
    pub channels: u16,
}

impl AudioBuffer {
    /// Returns the total number of frames (samples per channel).
    pub fn frame_count(&self) -> usize {
        if self.channels == 0 {
            return 0;
        }
        self.samples.len() / self.channels as usize
    }

    /// Returns the duration in seconds.
    pub fn duration_secs(&self) -> f32 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.frame_count() as f32 / self.sample_rate as f32
    }
}

/// Errors that can occur during audio decoding.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    /// WAV decoding error.
    #[error("WAV decode error: {0}")]
    Wav(String),
    /// OGG Vorbis decoding error.
    #[error("OGG decode error: {0}")]
    Ogg(String),
    /// Unsupported format.
    #[error("unsupported audio format: {0}")]
    UnsupportedFormat(String),
}

/// Decodes a WAV file from a byte slice into an [`AudioBuffer`].
pub fn decode_wav(data: &[u8]) -> Result<AudioBuffer, DecodeError> {
    decode_wav_reader(Cursor::new(data))
}

/// Decodes a WAV file from any reader into an [`AudioBuffer`].
pub fn decode_wav_reader<R: Read + Seek>(reader: R) -> Result<AudioBuffer, DecodeError> {
    let reader = hound::WavReader::new(reader).map_err(|e| DecodeError::Wav(e.to_string()))?;
    let spec = reader.spec();
    let channels = spec.channels;
    let sample_rate = spec.sample_rate;

    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample;
            let max_val = (1u32 << (bits - 1)) as f32;
            reader
                .into_samples::<i32>()
                .map(|s| s.map(|v| v as f32 / max_val))
                .collect::<Result<Vec<f32>, _>>()
                .map_err(|e| DecodeError::Wav(e.to_string()))?
        }
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .collect::<Result<Vec<f32>, _>>()
            .map_err(|e| DecodeError::Wav(e.to_string()))?,
    };

    Ok(AudioBuffer {
        samples,
        sample_rate,
        channels,
    })
}

/// Decodes an OGG Vorbis file from a byte slice into an [`AudioBuffer`].
pub fn decode_ogg(data: &[u8]) -> Result<AudioBuffer, DecodeError> {
    decode_ogg_reader(Cursor::new(data))
}

/// Decodes an OGG Vorbis file from any reader into an [`AudioBuffer`].
pub fn decode_ogg_reader<R: Read + Seek>(reader: R) -> Result<AudioBuffer, DecodeError> {
    let mut ogg_reader = lewton::inside_ogg::OggStreamReader::new(reader)
        .map_err(|e| DecodeError::Ogg(e.to_string()))?;

    let channels = ogg_reader.ident_hdr.audio_channels as u16;
    let sample_rate = ogg_reader.ident_hdr.audio_sample_rate;
    let mut samples = Vec::new();

    while let Some(packet) = ogg_reader
        .read_dec_packet_itl()
        .map_err(|e| DecodeError::Ogg(e.to_string()))?
    {
        // lewton returns interleaved i16 samples
        for s in packet {
            samples.push(s as f32 / 32768.0);
        }
    }

    Ok(AudioBuffer {
        samples,
        sample_rate,
        channels,
    })
}

/// Detects format from a byte header and decodes accordingly.
///
/// Recognizes WAV (RIFF header) and OGG (OggS header).
pub fn decode_auto(data: &[u8]) -> Result<AudioBuffer, DecodeError> {
    if data.len() < 4 {
        return Err(DecodeError::UnsupportedFormat("data too short".into()));
    }
    if &data[..4] == b"RIFF" {
        decode_wav(data)
    } else if &data[..4] == b"OggS" {
        decode_ogg(data)
    } else {
        Err(DecodeError::UnsupportedFormat(format!(
            "unrecognized header: {:02x} {:02x} {:02x} {:02x}",
            data[0], data[1], data[2], data[3]
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generate a minimal valid WAV file in memory.
    fn make_wav_bytes(sample_rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        let spec = hound::WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::new(&mut cursor, spec).unwrap();
        for &s in samples {
            writer.write_sample(s).unwrap();
        }
        writer.finalize().unwrap();
        cursor.into_inner()
    }

    #[test]
    fn decode_wav_mono_16bit() {
        let raw = [0i16, 16384, -16384, 0];
        let wav_data = make_wav_bytes(44100, 1, &raw);

        let buf = decode_wav(&wav_data).unwrap();
        assert_eq!(buf.sample_rate, 44100);
        assert_eq!(buf.channels, 1);
        assert_eq!(buf.frame_count(), 4);
        assert!((buf.duration_secs() - 4.0 / 44100.0).abs() < 1e-6);

        // 16384 / 32768 = 0.5
        assert!((buf.samples[1] - 0.5).abs() < 1e-3);
        // -16384 / 32768 = -0.5
        assert!((buf.samples[2] + 0.5).abs() < 1e-3);
    }

    #[test]
    fn decode_wav_stereo() {
        // L, R, L, R
        let raw = [1000i16, -1000, 2000, -2000];
        let wav_data = make_wav_bytes(48000, 2, &raw);

        let buf = decode_wav(&wav_data).unwrap();
        assert_eq!(buf.channels, 2);
        assert_eq!(buf.frame_count(), 2);
        assert_eq!(buf.samples.len(), 4);
    }

    #[test]
    fn decode_wav_float32() {
        let mut cursor = Cursor::new(Vec::new());
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 22050,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::new(&mut cursor, spec).unwrap();
        writer.write_sample(0.0f32).unwrap();
        writer.write_sample(0.75f32).unwrap();
        writer.write_sample(-0.5f32).unwrap();
        writer.finalize().unwrap();

        let wav_data = cursor.into_inner();
        let buf = decode_wav(&wav_data).unwrap();
        assert_eq!(buf.sample_rate, 22050);
        assert_eq!(buf.channels, 1);
        assert_eq!(buf.frame_count(), 3);
        assert!((buf.samples[1] - 0.75).abs() < 1e-6);
        assert!((buf.samples[2] + 0.5).abs() < 1e-6);
    }

    #[test]
    fn audio_buffer_empty() {
        let buf = AudioBuffer {
            samples: vec![],
            sample_rate: 44100,
            channels: 2,
        };
        assert_eq!(buf.frame_count(), 0);
        assert_eq!(buf.duration_secs(), 0.0);
    }

    #[test]
    fn audio_buffer_zero_channels() {
        let buf = AudioBuffer {
            samples: vec![1.0],
            sample_rate: 44100,
            channels: 0,
        };
        assert_eq!(buf.frame_count(), 0);
    }

    #[test]
    fn audio_buffer_zero_sample_rate() {
        let buf = AudioBuffer {
            samples: vec![1.0],
            sample_rate: 0,
            channels: 1,
        };
        assert_eq!(buf.duration_secs(), 0.0);
    }

    #[test]
    fn decode_auto_wav() {
        let wav_data = make_wav_bytes(44100, 1, &[0, 100, -100]);
        let buf = decode_auto(&wav_data).unwrap();
        assert_eq!(buf.sample_rate, 44100);
        assert_eq!(buf.channels, 1);
    }

    #[test]
    fn decode_auto_unknown_format() {
        let data = [0xFF, 0xFE, 0xFD, 0xFC, 0x00];
        let err = decode_auto(&data).unwrap_err();
        assert!(matches!(err, DecodeError::UnsupportedFormat(_)));
    }

    #[test]
    fn decode_auto_too_short() {
        let err = decode_auto(&[0, 1]).unwrap_err();
        assert!(matches!(err, DecodeError::UnsupportedFormat(_)));
    }
}

/// Detect MP3 frame sync / ID3.
pub fn is_mp3(data: &[u8]) -> bool {
    if data.len() >= 3 && &data[0..3] == b"ID3" {
        return true;
    }
    // Frame sync 0xFFEx
    data.len() >= 2 && data[0] == 0xFF && (data[1] & 0xE0) == 0xE0
}

/// MP3 decode (pat-p0s82.13). Real ISO MP3 streams are decoded frame-by-frame
/// with `minimp3` (the same decoder Godot ships) into interleaved f32 PCM.
/// A Patina test fixture starting with `PATMP3` carries raw f32 samples
/// directly, used by lightweight unit tests that must not depend on a real
/// encoder.
pub fn decode_mp3(data: &[u8]) -> Result<AudioBuffer, DecodeError> {
    if data.starts_with(b"PATMP3") {
        // Fixture: PATMP3 + u32 sample_rate + u16 channels + f32 samples...
        if data.len() < 6 + 4 + 2 {
            return Err(DecodeError::UnsupportedFormat("PATMP3 truncated".into()));
        }
        let sr = u32::from_le_bytes(data[6..10].try_into().unwrap());
        let ch = u16::from_le_bytes(data[10..12].try_into().unwrap());
        let mut samples = Vec::new();
        let mut i = 12;
        while i + 4 <= data.len() {
            samples.push(f32::from_le_bytes(data[i..i + 4].try_into().unwrap()));
            i += 4;
        }
        return Ok(AudioBuffer {
            samples,
            sample_rate: sr,
            channels: ch,
        });
    }
    if !is_mp3(data) {
        return Err(DecodeError::UnsupportedFormat("not mp3".into()));
    }
    // Real ISO MP3: decode every frame with minimp3, concatenating the
    // interleaved PCM (i16 -> f32 in [-1, 1]). Leading ID3 tags and stray
    // non-frame bytes surface as `SkippedData` and are ignored; decoding stops
    // at end-of-stream (`Eof`) or when the trailing bytes can't form a frame
    // (`InsufficientData`).
    let mut decoder = minimp3::Decoder::new(data);
    let mut samples: Vec<f32> = Vec::new();
    let mut sample_rate: u32 = 0;
    let mut channels: u16 = 0;
    loop {
        match decoder.next_frame() {
            Ok(frame) => {
                sample_rate = frame.sample_rate as u32;
                channels = frame.channels as u16;
                samples.extend(frame.data.iter().map(|&s| s as f32 / 32768.0));
            }
            Err(minimp3::Error::Eof) | Err(minimp3::Error::InsufficientData) => break,
            Err(minimp3::Error::SkippedData) => continue,
            Err(minimp3::Error::Io(e)) => {
                return Err(DecodeError::UnsupportedFormat(format!("mp3 io error: {e}")));
            }
        }
    }
    if samples.is_empty() || sample_rate == 0 {
        return Err(DecodeError::UnsupportedFormat(
            "mp3 contained no decodable frames".into(),
        ));
    }
    Ok(AudioBuffer {
        samples,
        sample_rate,
        channels: channels.max(1),
    })
}

/// Procedural audio stream generator (AudioStreamGenerator slice).
#[derive(Debug, Clone)]
pub struct AudioStreamGenerator {
    pub sample_rate: u32,
    pub buffer: Vec<f32>,
    pub max_frames: usize,
}

impl AudioStreamGenerator {
    pub fn new(sample_rate: u32, max_frames: usize) -> Self {
        Self {
            sample_rate,
            buffer: Vec::new(),
            max_frames,
        }
    }
    pub fn push_frame(&mut self, left: f32, right: f32) -> bool {
        if self.buffer.len() / 2 >= self.max_frames {
            return false;
        }
        self.buffer.push(left);
        self.buffer.push(right);
        true
    }
    pub fn mix_to_buffer(&self) -> AudioBuffer {
        AudioBuffer {
            samples: self.buffer.clone(),
            sample_rate: self.sample_rate,
            channels: 2,
        }
    }

    /// Frames still buffered, waiting to be mixed out (pat-nzgdu.153).
    pub fn frames_queued(&self) -> usize {
        self.buffer.len() / 2
    }

    /// Room left before the buffer is full — Godot's
    /// `AudioStreamGeneratorPlayback.get_frames_available()` (pat-nzgdu.153).
    pub fn frames_available(&self) -> usize {
        self.max_frames.saturating_sub(self.frames_queued())
    }

    /// Whether `frames` more frames would fit — Godot's `can_push_buffer()`.
    pub fn can_push_buffer(&self, frames: usize) -> bool {
        self.frames_available() >= frames
    }

    /// Pushes a whole block of stereo frames — Godot's
    /// `AudioStreamGeneratorPlayback.push_buffer()` (pat-nzgdu.153).
    ///
    /// **All or nothing:** if the block doesn't fit, nothing is pushed and this
    /// returns `false`, matching Godot — a partial push would tear the signal.
    pub fn push_buffer(&mut self, frames: &[(f32, f32)]) -> bool {
        if !self.can_push_buffer(frames.len()) {
            return false;
        }
        for &(left, right) in frames {
            self.buffer.push(left);
            self.buffer.push(right);
        }
        true
    }

    /// Drops every queued frame — Godot's `clear_buffer()`.
    pub fn clear_buffer(&mut self) {
        self.buffer.clear();
    }

    /// **Consumes** up to `frames` frames and returns them (pat-nzgdu.153).
    ///
    /// This is what an audio sink pulls each block, and it is the counterpart to
    /// [`push_buffer`](Self::push_buffer): what goes in comes back out, once.
    /// Unlike [`mix_to_buffer`](Self::mix_to_buffer) — which peeks at the whole
    /// queue and leaves it in place — mixed frames are removed, so the generator
    /// refills rather than replaying stale audio.
    ///
    /// Yields fewer frames than asked (possibly none) when the queue runs dry;
    /// an underrun is silence, not an error.
    pub fn mix(&mut self, frames: usize) -> AudioBuffer {
        let take = frames.min(self.frames_queued());
        let samples: Vec<f32> = self.buffer.drain(..take * 2).collect();
        AudioBuffer {
            samples,
            sample_rate: self.sample_rate,
            channels: 2,
        }
    }
}

/// One sounding sub-playback inside a [`PolyphonicPlayer`] (pat-nzgdu.153).
#[derive(Debug, Clone)]
struct Voice {
    /// The stereo frames this voice plays.
    buffer: AudioBuffer,
    /// Next frame to read.
    cursor: usize,
}

impl Voice {
    fn frames(&self) -> usize {
        self.buffer.samples.len() / 2
    }
    fn finished(&self) -> bool {
        self.cursor >= self.frames()
    }
}

/// Simple polyphonic voice pool.
///
/// Godot's `AudioStreamPlaybackPolyphonic` runs several streams at once on one
/// player and sums them. [`try_play`](Self::try_play) reserves a voice slot;
/// [`play_buffer`](Self::play_buffer) reserves one **and** gives it audio, which
/// [`mix`](Self::mix) then sums (pat-nzgdu.153).
#[derive(Debug, Default)]
pub struct PolyphonicPlayer {
    pub max_voices: usize,
    pub active: usize,
    /// Output rate of [`mix`](Self::mix). Voices are assumed to already be at
    /// this rate (they share a bus); resampling belongs to the Resampled base.
    pub sample_rate: u32,
    voices: Vec<Voice>,
}

impl PolyphonicPlayer {
    pub fn new(max_voices: usize) -> Self {
        Self {
            max_voices,
            active: 0,
            sample_rate: 44_100,
            voices: Vec::new(),
        }
    }
    pub fn try_play(&mut self) -> bool {
        if self.active >= self.max_voices {
            return false;
        }
        self.active += 1;
        true
    }
    pub fn stop_one(&mut self) {
        self.active = self.active.saturating_sub(1);
    }

    /// Starts a sub-playback from `buffer` (pat-nzgdu.153).
    ///
    /// Takes a voice slot via [`try_play`](Self::try_play), so the pool's limit
    /// is enforced identically however a voice was started. Returns `false` when
    /// the pool is full — Godot drops the request rather than stealing a voice.
    pub fn play_buffer(&mut self, buffer: AudioBuffer) -> bool {
        if !self.try_play() {
            return false;
        }
        self.voices.push(Voice { buffer, cursor: 0 });
        true
    }

    /// Number of sub-playbacks still sounding.
    pub fn voice_count(&self) -> usize {
        self.voices.len()
    }

    /// Sums `frames` frames from every sounding sub-playback (pat-nzgdu.153).
    ///
    /// Voices that run out contribute silence for the rest of the block and are
    /// then retired, freeing their slot — so a finished one-shot doesn't hold
    /// the pool at its limit forever. Output is **not** clamped: mixing is
    /// additive and headroom is the bus's job, matching Godot's mix stage.
    pub fn mix(&mut self, frames: usize) -> AudioBuffer {
        let mut samples = vec![0.0f32; frames * 2];
        for voice in &mut self.voices {
            let available = voice.frames().saturating_sub(voice.cursor);
            let take = frames.min(available);
            for i in 0..take {
                let src = (voice.cursor + i) * 2;
                samples[i * 2] += voice.buffer.samples[src];
                samples[i * 2 + 1] += voice.buffer.samples[src + 1];
            }
            voice.cursor += take;
        }
        // Retire exhausted voices and free their slots.
        let before = self.voices.len();
        self.voices.retain(|v| !v.finished());
        let retired = before - self.voices.len();
        self.active = self.active.saturating_sub(retired);

        AudioBuffer {
            samples,
            sample_rate: self.sample_rate,
            channels: 2,
        }
    }
}

#[cfg(test)]
mod mp3_gen_tests {
    use super::*;

    #[test]
    fn patmp3_fixture_decodes() {
        let mut data = b"PATMP3".to_vec();
        data.extend_from_slice(&44100u32.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&0.5f32.to_le_bytes());
        data.extend_from_slice(&(-0.5f32).to_le_bytes());
        let buf = decode_mp3(&data).unwrap();
        assert_eq!(buf.sample_rate, 44100);
        assert_eq!(buf.samples.len(), 2);
    }

    // -- pat-nzgdu.153: generator push_buffer/mix + polyphonic mixing --------

    fn buf(frames: &[(f32, f32)]) -> AudioBuffer {
        let mut samples = Vec::new();
        for &(l, r) in frames {
            samples.push(l);
            samples.push(r);
        }
        AudioBuffer {
            samples,
            sample_rate: 44_100,
            channels: 2,
        }
    }

    #[test]
    fn generator_push_buffer_then_mix_returns_the_pushed_frames() {
        let mut g = AudioStreamGenerator::new(48_000, 4);
        assert_eq!(g.frames_available(), 4);

        assert!(g.push_buffer(&[(0.1, 0.2), (0.3, 0.4)]));
        assert_eq!(g.frames_queued(), 2);
        assert_eq!(g.frames_available(), 2);

        // What went in comes back out, in order.
        let out = g.mix(2);
        assert_eq!(out.samples, vec![0.1, 0.2, 0.3, 0.4]);
        assert_eq!(out.sample_rate, 48_000);
        assert_eq!(out.channels, 2);

        // mix CONSUMES — the queue is empty and refillable, not replayed.
        assert_eq!(g.frames_queued(), 0);
        assert_eq!(g.frames_available(), 4);
        assert!(g.mix(2).samples.is_empty(), "an empty queue mixes silence");
    }

    #[test]
    fn generator_push_buffer_is_all_or_nothing() {
        let mut g = AudioStreamGenerator::new(48_000, 2);
        // Three frames into a two-frame buffer: Godot rejects rather than
        // partially filling (a torn push would tear the signal).
        assert!(!g.can_push_buffer(3));
        assert!(!g.push_buffer(&[(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)]));
        assert_eq!(g.frames_queued(), 0, "nothing was pushed");

        assert!(g.push_buffer(&[(1.0, 1.0), (2.0, 2.0)]));
        assert_eq!(g.frames_available(), 0);
        g.clear_buffer();
        assert_eq!(g.frames_queued(), 0);
    }

    #[test]
    fn generator_mix_yields_what_is_queued_when_asked_for_more() {
        let mut g = AudioStreamGenerator::new(48_000, 8);
        assert!(g.push_buffer(&[(0.5, -0.5)]));
        // Under-run: one frame available, two requested.
        let out = g.mix(2);
        assert_eq!(out.samples, vec![0.5, -0.5]);
    }

    #[test]
    fn polyphonic_mixes_concurrent_sub_playbacks() {
        let mut p = PolyphonicPlayer::new(3);
        assert!(p.play_buffer(buf(&[(0.1, 0.2), (0.3, 0.4)])));
        assert!(p.play_buffer(buf(&[(0.01, 0.02), (0.03, 0.04)])));
        assert_eq!(p.voice_count(), 2);
        assert_eq!(p.active, 2);

        // Both voices sum, frame for frame.
        let out = p.mix(2);
        assert_eq!(out.channels, 2);
        assert!((out.samples[0] - 0.11).abs() < 1e-6, "{:?}", out.samples);
        assert!((out.samples[1] - 0.22).abs() < 1e-6, "{:?}", out.samples);
        assert!((out.samples[2] - 0.33).abs() < 1e-6, "{:?}", out.samples);
        assert!((out.samples[3] - 0.44).abs() < 1e-6, "{:?}", out.samples);
    }

    #[test]
    fn polyphonic_retires_finished_voices_and_frees_slots() {
        let mut p = PolyphonicPlayer::new(1);
        assert!(p.play_buffer(buf(&[(1.0, 1.0)])));
        // Pool is full: a second voice is dropped, not stolen.
        assert!(!p.play_buffer(buf(&[(9.0, 9.0)])));

        // Mixing past the end drains the voice and frees its slot.
        let out = p.mix(2);
        assert_eq!(out.samples, vec![1.0, 1.0, 0.0, 0.0], "tail is silence");
        assert_eq!(p.voice_count(), 0);
        assert_eq!(p.active, 0, "a finished one-shot must not hold the pool");

        // The freed slot is reusable.
        assert!(p.play_buffer(buf(&[(0.5, 0.5)])));
        assert_eq!(p.voice_count(), 1);
    }

    #[test]
    fn polyphonic_with_no_voices_mixes_silence() {
        let mut p = PolyphonicPlayer::new(2);
        let out = p.mix(3);
        assert_eq!(out.samples, vec![0.0; 6]);
        assert_eq!(out.sample_rate, 44_100);
    }

    #[test]
    fn generator_and_poly() {
        let mut g = AudioStreamGenerator::new(48000, 8);
        assert!(g.push_frame(0.1, 0.2));
        let b = g.mix_to_buffer();
        assert_eq!(b.channels, 2);
        let mut p = PolyphonicPlayer::new(2);
        assert!(p.try_play());
        assert!(p.try_play());
        assert!(!p.try_play());
    }
}
