//! Audio import pipeline for loading WAV and OGG files into playable buffers.
//!
//! Provides [`AudioStreamLoader`] which loads audio files from disk, decodes
//! them into [`AudioBuffer`]s, and optionally caches the results. This is the
//! primary entry point for the engine's audio asset pipeline.
//!
//! # Supported formats
//!
//! - **WAV** (`.wav`) — via the `hound` crate (PCM 8/16/24/32-bit, IEEE float)
//! - **OGG Vorbis** (`.ogg`) — via the `lewton` crate
//!
//! # Usage
//!
//! ```no_run
//! use gdaudio::import::AudioStreamLoader;
//!
//! let mut loader = AudioStreamLoader::new();
//! let import = loader.load_file("assets/music.ogg").unwrap();
//! println!("loaded {} frames at {} Hz", import.buffer.frame_count(), import.buffer.sample_rate);
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::decode::{self, AudioBuffer, DecodeError};

/// Errors that can occur during audio import.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    /// File I/O error.
    #[error("I/O error loading {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// Audio decoding failed.
    #[error("decode error for {path}: {source}")]
    Decode {
        path: PathBuf,
        source: DecodeError,
    },
    /// File extension is not a recognized audio format.
    #[error("unsupported audio format: {0}")]
    UnsupportedFormat(String),
}

/// Detected audio file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    /// WAV (RIFF/PCM or IEEE float).
    Wav,
    /// OGG Vorbis.
    OggVorbis,
}

impl AudioFormat {
    /// Detects the audio format from a file extension.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "wav" => Some(AudioFormat::Wav),
            "ogg" => Some(AudioFormat::OggVorbis),
            _ => None,
        }
    }

    /// Detects the audio format from the first bytes of file data.
    pub fn from_header(data: &[u8]) -> Option<Self> {
        if data.len() < 4 {
            return None;
        }
        if &data[..4] == b"RIFF" {
            Some(AudioFormat::Wav)
        } else if &data[..4] == b"OggS" {
            Some(AudioFormat::OggVorbis)
        } else {
            None
        }
    }
}

/// Metadata extracted from an audio file alongside the decoded buffer.
#[derive(Debug, Clone)]
pub struct AudioStreamInfo {
    /// The source file path.
    pub source_path: PathBuf,
    /// Detected format.
    pub format: AudioFormat,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Number of channels.
    pub channels: u16,
    /// Duration in seconds.
    pub duration_secs: f32,
    /// Total number of frames (samples per channel).
    pub frame_count: usize,
}

/// Result of loading an audio file: decoded buffer + metadata.
#[derive(Debug, Clone)]
pub struct AudioStreamImport {
    /// The decoded audio data, ready for playback.
    pub buffer: AudioBuffer,
    /// Metadata about the source file.
    pub info: AudioStreamInfo,
}

/// Loads and optionally caches audio files from disk.
///
/// The loader handles format detection, decoding, and caching. Audio files
/// are decoded into [`AudioBuffer`] instances suitable for
/// [`AudioServer::play`](crate::server::AudioServer::play).
pub struct AudioStreamLoader {
    /// Cached decoded buffers keyed by canonical path.
    cache: HashMap<PathBuf, AudioStreamImport>,
    /// Whether caching is enabled.
    cache_enabled: bool,
}

impl AudioStreamLoader {
    /// Creates a new loader with caching enabled.
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            cache_enabled: true,
        }
    }

    /// Creates a new loader with caching disabled.
    pub fn without_cache() -> Self {
        Self {
            cache: HashMap::new(),
            cache_enabled: false,
        }
    }

    /// Enables or disables the decode cache.
    pub fn set_cache_enabled(&mut self, enabled: bool) {
        self.cache_enabled = enabled;
        if !enabled {
            self.cache.clear();
        }
    }

    /// Returns the number of cached entries.
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }

    /// Clears the decode cache.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Loads an audio file from disk, decoding it into an [`AudioBuffer`].
    ///
    /// If caching is enabled and the file has been loaded before, returns
    /// a clone of the cached buffer.
    ///
    /// Format is detected first by file extension, then by header bytes.
    pub fn load_file<P: AsRef<Path>>(&mut self, path: P) -> Result<AudioStreamImport, ImportError> {
        let path = path.as_ref();

        // Check cache.
        if self.cache_enabled {
            if let Some(cached) = self.cache.get(path) {
                return Ok(cached.clone());
            }
        }

        let result = load_audio_file(path)?;

        if self.cache_enabled {
            self.cache.insert(path.to_path_buf(), result.clone());
        }

        Ok(result)
    }

    /// Loads an audio file and returns just the [`AudioBuffer`].
    pub fn load_buffer<P: AsRef<Path>>(&mut self, path: P) -> Result<AudioBuffer, ImportError> {
        self.load_file(path).map(|import| import.buffer)
    }
}

impl Default for AudioStreamLoader {
    fn default() -> Self {
        Self::new()
    }
}

/// Loads and decodes an audio file from disk without caching.
///
/// This is the core pipeline function. Format detection uses the file
/// extension first, falling back to header-byte sniffing.
pub fn load_audio_file<P: AsRef<Path>>(path: P) -> Result<AudioStreamImport, ImportError> {
    let path = path.as_ref();

    let data = std::fs::read(path).map_err(|e| ImportError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;

    // Detect format: extension first, then header.
    let ext_format = path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(AudioFormat::from_extension);
    let header_format = AudioFormat::from_header(&data);
    let format = ext_format.or(header_format).ok_or_else(|| {
        ImportError::UnsupportedFormat(
            path.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("(none)")
                .to_string(),
        )
    })?;

    let buffer = decode_audio_data(&data, format).map_err(|e| ImportError::Decode {
        path: path.to_path_buf(),
        source: e,
    })?;

    let info = AudioStreamInfo {
        source_path: path.to_path_buf(),
        format,
        sample_rate: buffer.sample_rate,
        channels: buffer.channels,
        duration_secs: buffer.duration_secs(),
        frame_count: buffer.frame_count(),
    };

    Ok(AudioStreamImport { buffer, info })
}

/// Decodes raw audio bytes given a known format.
pub fn decode_audio_data(data: &[u8], format: AudioFormat) -> Result<AudioBuffer, DecodeError> {
    match format {
        AudioFormat::Wav => decode::decode_wav(data),
        AudioFormat::OggVorbis => decode::decode_ogg(data),
    }
}

/// Converts an [`AudioSampleBuffer`](crate::sample::AudioSampleBuffer) to an [`AudioBuffer`].
///
/// This bridges the two buffer types used in the audio crate.
pub fn sample_buffer_to_audio_buffer(sb: &crate::sample::AudioSampleBuffer) -> AudioBuffer {
    AudioBuffer {
        samples: sb.samples.clone(),
        sample_rate: sb.sample_rate,
        channels: sb.channels,
    }
}

/// Converts an [`AudioBuffer`] to an [`AudioSampleBuffer`](crate::sample::AudioSampleBuffer).
pub fn audio_buffer_to_sample_buffer(ab: &AudioBuffer) -> crate::sample::AudioSampleBuffer {
    let layout = match ab.channels {
        1 => crate::sample::ChannelLayout::Mono,
        2 => crate::sample::ChannelLayout::Stereo,
        n => crate::sample::ChannelLayout::Custom(n),
    };
    crate::sample::AudioSampleBuffer {
        sample_rate: ab.sample_rate,
        channels: ab.channels,
        channel_layout: layout,
        samples: ab.samples.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Build a minimal WAV file using hound for testing.
    fn make_test_wav(sample_rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
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

    // -- AudioFormat detection ------------------------------------------------

    #[test]
    fn format_from_extension() {
        assert_eq!(AudioFormat::from_extension("wav"), Some(AudioFormat::Wav));
        assert_eq!(AudioFormat::from_extension("WAV"), Some(AudioFormat::Wav));
        assert_eq!(
            AudioFormat::from_extension("ogg"),
            Some(AudioFormat::OggVorbis)
        );
        assert_eq!(
            AudioFormat::from_extension("OGG"),
            Some(AudioFormat::OggVorbis)
        );
        assert_eq!(AudioFormat::from_extension("mp3"), None);
        assert_eq!(AudioFormat::from_extension(""), None);
    }

    #[test]
    fn format_from_header() {
        assert_eq!(
            AudioFormat::from_header(b"RIFF...."),
            Some(AudioFormat::Wav)
        );
        assert_eq!(
            AudioFormat::from_header(b"OggS...."),
            Some(AudioFormat::OggVorbis)
        );
        assert_eq!(AudioFormat::from_header(b"NOPE"), None);
        assert_eq!(AudioFormat::from_header(b"Og"), None);
    }

    // -- decode_audio_data (in-memory) ----------------------------------------

    #[test]
    fn decode_wav_data_produces_correct_buffer() {
        let samples = [0i16, 16384, -16384, 0];
        let wav_data = make_test_wav(44100, 1, &samples);

        let buf = decode_audio_data(&wav_data, AudioFormat::Wav).unwrap();
        assert_eq!(buf.sample_rate, 44100);
        assert_eq!(buf.channels, 1);
        assert_eq!(buf.frame_count(), 4);
        assert!((buf.samples[1] - 0.5).abs() < 0.01);
        assert!((buf.samples[2] + 0.5).abs() < 0.01);
    }

    #[test]
    fn decode_wav_stereo() {
        // L, R, L, R
        let samples = [1000i16, -1000, 2000, -2000];
        let wav_data = make_test_wav(48000, 2, &samples);

        let buf = decode_audio_data(&wav_data, AudioFormat::Wav).unwrap();
        assert_eq!(buf.channels, 2);
        assert_eq!(buf.frame_count(), 2);
    }

    #[test]
    fn decode_invalid_data_returns_error() {
        let result = decode_audio_data(b"not a wav file at all", AudioFormat::Wav);
        assert!(result.is_err());
    }

    // -- Buffer conversion ----------------------------------------------------

    #[test]
    fn sample_buffer_roundtrip_conversion() {
        let sb = crate::sample::AudioSampleBuffer {
            sample_rate: 44100,
            channels: 2,
            channel_layout: crate::sample::ChannelLayout::Stereo,
            samples: vec![0.5, -0.5, 0.3, -0.3],
        };

        let ab = sample_buffer_to_audio_buffer(&sb);
        assert_eq!(ab.sample_rate, 44100);
        assert_eq!(ab.channels, 2);
        assert_eq!(ab.samples, sb.samples);

        let roundtrip = audio_buffer_to_sample_buffer(&ab);
        assert_eq!(roundtrip.sample_rate, sb.sample_rate);
        assert_eq!(roundtrip.channels, sb.channels);
        assert_eq!(roundtrip.channel_layout, sb.channel_layout);
        assert_eq!(roundtrip.samples, sb.samples);
    }

    // -- AudioStreamLoader ----------------------------------------------------

    #[test]
    fn loader_load_wav_from_tempfile() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("test.wav");
        let wav_data = make_test_wav(22050, 1, &[0, 1000, -1000, 0, 500]);
        std::fs::write(&path, &wav_data).unwrap();

        let mut loader = AudioStreamLoader::new();
        let import = loader.load_file(&path).unwrap();

        assert_eq!(import.info.format, AudioFormat::Wav);
        assert_eq!(import.info.sample_rate, 22050);
        assert_eq!(import.info.channels, 1);
        assert_eq!(import.info.frame_count, 5);
        assert!(import.info.duration_secs > 0.0);
        assert_eq!(import.buffer.frame_count(), 5);
    }

    #[test]
    fn loader_caches_second_load() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("cached.wav");
        let wav_data = make_test_wav(44100, 2, &[100, -100, 200, -200]);
        std::fs::write(&path, &wav_data).unwrap();

        let mut loader = AudioStreamLoader::new();
        assert_eq!(loader.cache_size(), 0);

        let _import1 = loader.load_file(&path).unwrap();
        assert_eq!(loader.cache_size(), 1);

        let import2 = loader.load_file(&path).unwrap();
        assert_eq!(loader.cache_size(), 1); // still 1, served from cache
        assert_eq!(import2.info.sample_rate, 44100);
    }

    #[test]
    fn loader_no_cache_mode() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("nocache.wav");
        let wav_data = make_test_wav(44100, 1, &[0, 0]);
        std::fs::write(&path, &wav_data).unwrap();

        let mut loader = AudioStreamLoader::without_cache();
        let _import = loader.load_file(&path).unwrap();
        assert_eq!(loader.cache_size(), 0);
    }

    #[test]
    fn loader_clear_cache() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("clear.wav");
        std::fs::write(&path, make_test_wav(44100, 1, &[0])).unwrap();

        let mut loader = AudioStreamLoader::new();
        let _ = loader.load_file(&path).unwrap();
        assert_eq!(loader.cache_size(), 1);

        loader.clear_cache();
        assert_eq!(loader.cache_size(), 0);
    }

    #[test]
    fn loader_nonexistent_file_returns_io_error() {
        let mut loader = AudioStreamLoader::new();
        let err = loader.load_file("/nonexistent/path/audio.wav").unwrap_err();
        assert!(matches!(err, ImportError::Io { .. }));
    }

    #[test]
    fn loader_unsupported_extension_returns_error() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("music.mp3");
        // Write RIFF-less data so header detection also fails.
        std::fs::write(&path, b"NOT_AUDIO_DATA_HERE").unwrap();

        let mut loader = AudioStreamLoader::new();
        let err = loader.load_file(&path).unwrap_err();
        assert!(matches!(err, ImportError::UnsupportedFormat(_)));
    }

    #[test]
    fn loader_header_detection_overrides_bad_extension() {
        let dir = tempfile::TempDir::new().unwrap();
        // File has .dat extension but WAV content.
        let path = dir.path().join("audio.dat");
        std::fs::write(&path, make_test_wav(44100, 1, &[0, 100])).unwrap();

        let mut loader = AudioStreamLoader::new();
        // .dat extension is unknown, but header detection should find RIFF.
        let import = loader.load_file(&path).unwrap();
        assert_eq!(import.info.format, AudioFormat::Wav);
    }

    // -- load_audio_file (standalone function) --------------------------------

    #[test]
    fn load_audio_file_wav() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("standalone.wav");
        let wav_data = make_test_wav(48000, 2, &[0, 0, 1000, -1000]);
        std::fs::write(&path, &wav_data).unwrap();

        let import = load_audio_file(&path).unwrap();
        assert_eq!(import.info.format, AudioFormat::Wav);
        assert_eq!(import.buffer.sample_rate, 48000);
        assert_eq!(import.buffer.channels, 2);
        assert_eq!(import.buffer.frame_count(), 2);
    }

    // -- Full pipeline: load → AudioServer.play() → mix ---------------------

    #[test]
    fn full_pipeline_wav_to_server_playback() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("pipeline.wav");

        // Generate a 16-bit mono WAV with a known DC value.
        // 16384 / 32768 = 0.5
        let samples: Vec<i16> = vec![16384; 4410]; // ~0.1 sec at 44100
        let wav_data = make_test_wav(44100, 1, &samples);
        std::fs::write(&path, &wav_data).unwrap();

        // Step 1: Load via the import pipeline.
        let mut loader = AudioStreamLoader::new();
        let import = loader.load_file(&path).unwrap();
        assert_eq!(import.info.format, AudioFormat::Wav);
        assert_eq!(import.buffer.sample_rate, 44100);

        // Step 2: Play on AudioServer.
        let mut server = crate::server::AudioServer::new();
        let id = server.play(import.buffer);
        assert!(server.is_playing(id));

        // Step 3: Mix and verify output.
        let output = server.mix(256);
        assert_eq!(output.len(), 256 * 2); // stereo output

        // Each sample should be ~0.5 (mono 0.5 spread to both L and R).
        assert!(
            (output[0] - 0.5).abs() < 0.02,
            "expected ~0.5, got {}",
            output[0]
        );
        assert!(
            (output[1] - 0.5).abs() < 0.02,
            "R channel: expected ~0.5, got {}",
            output[1]
        );
    }

    #[test]
    fn full_pipeline_wav_on_named_bus() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("bus_test.wav");

        let samples: Vec<i16> = vec![32767; 4410]; // ~1.0 max
        let wav_data = make_test_wav(44100, 1, &samples);
        std::fs::write(&path, &wav_data).unwrap();

        let mut loader = AudioStreamLoader::new();
        let import = loader.load_file(&path).unwrap();

        let mut server = crate::server::AudioServer::new();
        let sfx_idx = server.mixer_mut().add_bus("SFX");
        server
            .mixer_mut()
            .get_bus_mut(sfx_idx)
            .unwrap()
            .set_volume_db(-20.0); // 0.1 linear

        let id = server.play_on_bus(import.buffer, "SFX");
        assert!(server.is_playing(id));

        let output = server.mix(1);
        // ~1.0 * 0.1 = 0.1
        assert!(
            (output[0] - 0.1).abs() < 0.02,
            "expected ~0.1, got {}",
            output[0]
        );
    }
}
