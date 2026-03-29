//! AudioStream import pipeline for WAV and OGG Vorbis files.
//!
//! Provides full import of audio files into [`AudioImportResult`] structs
//! containing decoded sample data, format metadata, and duration. This
//! module bridges the gap between the header-only importers in
//! [`crate::importers`] and the audio pipeline's need for decoded PCM data.
//!
//! # Supported formats
//!
//! - **WAV** (`.wav`) — PCM 8/16/24/32-bit and IEEE float via `hound`
//! - **OGG Vorbis** (`.ogg`) — via `lewton`
//!
//! # Usage
//!
//! ```ignore
//! use gdresource::audio_import::{AudioImporter, AudioImportResult};
//!
//! let importer = AudioImporter::new();
//! let result = importer.import_file(path)?;
//! println!("{} Hz, {} ch, {:.2}s", result.sample_rate, result.channels, result.duration_secs);
//! ```

use std::path::Path;
use std::sync::Arc;

use gdaudio::import::AudioFormat;
use gdaudio::decode::AudioBuffer;
use gdcore::error::{EngineError, EngineResult};
use gdvariant::Variant;

use crate::resource::Resource;

/// Result of importing an audio file: decoded buffer + metadata.
#[derive(Debug, Clone)]
pub struct AudioImportResult {
    /// Sample rate in Hz (e.g. 44100, 48000).
    pub sample_rate: u32,
    /// Number of channels (1 = mono, 2 = stereo).
    pub channels: u16,
    /// Duration in seconds.
    pub duration_secs: f32,
    /// Total number of frames (samples per channel).
    pub frame_count: usize,
    /// Detected audio format.
    pub format: AudioFormat,
    /// Decoded interleaved f32 samples, normalized to [-1.0, 1.0].
    pub samples: Vec<f32>,
}

impl AudioImportResult {
    /// Converts this import result into a [`Resource`] with the appropriate
    /// AudioStream class (`AudioStreamWAV` or `AudioStreamOggVorbis`).
    pub fn to_resource(&self, path: &str) -> Arc<Resource> {
        let class_name = match self.format {
            AudioFormat::Wav => "AudioStreamWAV",
            AudioFormat::OggVorbis => "AudioStreamOggVorbis",
        };
        let mut res = Resource::new(class_name);
        res.path = path.to_string();
        res.set_property("sample_rate", Variant::Int(self.sample_rate as i64));
        res.set_property("channels", Variant::Int(self.channels as i64));
        res.set_property(
            "length_seconds",
            Variant::Float(self.duration_secs as f64),
        );
        res.set_property("frame_count", Variant::Int(self.frame_count as i64));
        res.set_property("has_samples", Variant::Bool(true));
        Arc::new(res)
    }
}

/// Audio importer that decodes WAV and OGG Vorbis files to PCM sample data.
#[derive(Debug, Default)]
pub struct AudioImporter;

impl AudioImporter {
    /// Creates a new audio importer.
    pub fn new() -> Self {
        Self
    }

    /// Imports an audio file from a file path, auto-detecting format by extension.
    pub fn import_file(&self, path: &Path) -> EngineResult<AudioImportResult> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        let data = std::fs::read(path).map_err(EngineError::Io)?;

        match ext.as_str() {
            "wav" => self.decode_wav(&data),
            "ogg" => self.decode_ogg(&data),
            _ => {
                // Fall back to header-byte sniffing.
                self.import_bytes(&data)
            }
        }
    }

    /// Imports audio from raw bytes, auto-detecting format by magic bytes.
    pub fn import_bytes(&self, data: &[u8]) -> EngineResult<AudioImportResult> {
        if data.len() >= 4 && &data[..4] == b"RIFF" {
            self.decode_wav(data)
        } else if data.len() >= 4 && &data[..4] == b"OggS" {
            self.decode_ogg(data)
        } else {
            Err(EngineError::Parse(
                "unrecognized audio format (bad magic bytes)".into(),
            ))
        }
    }

    /// Imports a file and returns a full [`Resource`] with decoded audio data attached.
    pub fn import_resource(&self, path: &Path) -> EngineResult<Arc<Resource>> {
        let result = self.import_file(path)?;
        let res_path = format!(
            "res://{}",
            path.file_name().unwrap_or_default().to_string_lossy()
        );
        Ok(result.to_resource(&res_path))
    }

    /// Decodes WAV audio data from raw bytes.
    pub fn decode_wav(&self, data: &[u8]) -> EngineResult<AudioImportResult> {
        let buf = gdaudio::decode::decode_wav(data)
            .map_err(|e| EngineError::Parse(format!("WAV decode failed: {e}")))?;
        Ok(buffer_to_result(buf, AudioFormat::Wav))
    }

    /// Decodes OGG Vorbis audio data from raw bytes.
    pub fn decode_ogg(&self, data: &[u8]) -> EngineResult<AudioImportResult> {
        let buf = gdaudio::decode::decode_ogg(data)
            .map_err(|e| EngineError::Parse(format!("OGG decode failed: {e}")))?;
        Ok(buffer_to_result(buf, AudioFormat::OggVorbis))
    }
}

/// Converts a decoded [`AudioBuffer`] into an [`AudioImportResult`].
fn buffer_to_result(buf: AudioBuffer, format: AudioFormat) -> AudioImportResult {
    AudioImportResult {
        sample_rate: buf.sample_rate,
        channels: buf.channels,
        duration_secs: buf.duration_secs(),
        frame_count: buf.frame_count(),
        format,
        samples: buf.samples,
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

    // -- AudioImporter: WAV decoding ------------------------------------------

    #[test]
    fn decode_wav_mono_produces_correct_result() {
        let samples = [0i16, 16384, -16384, 0];
        let wav_data = make_test_wav(44100, 1, &samples);

        let importer = AudioImporter::new();
        let result = importer.decode_wav(&wav_data).unwrap();

        assert_eq!(result.sample_rate, 44100);
        assert_eq!(result.channels, 1);
        assert_eq!(result.frame_count, 4);
        assert_eq!(result.format, AudioFormat::Wav);
        assert!(result.duration_secs > 0.0);
        // 16384 / 32768 ≈ 0.5
        assert!((result.samples[1] - 0.5).abs() < 0.01);
        assert!((result.samples[2] + 0.5).abs() < 0.01);
    }

    #[test]
    fn decode_wav_stereo() {
        let samples = [1000i16, -1000, 2000, -2000];
        let wav_data = make_test_wav(48000, 2, &samples);

        let importer = AudioImporter::new();
        let result = importer.decode_wav(&wav_data).unwrap();

        assert_eq!(result.channels, 2);
        assert_eq!(result.frame_count, 2);
        assert_eq!(result.samples.len(), 4);
    }

    #[test]
    fn decode_wav_invalid_data_returns_error() {
        let importer = AudioImporter::new();
        let result = importer.decode_wav(b"not a wav file at all");
        assert!(result.is_err());
    }

    // -- AudioImporter: OGG decoding ------------------------------------------

    #[test]
    fn decode_ogg_invalid_data_returns_error() {
        let importer = AudioImporter::new();
        let result = importer.decode_ogg(b"not an ogg file");
        assert!(result.is_err());
    }

    // -- AudioImporter: magic-based detection ---------------------------------

    #[test]
    fn import_bytes_detects_wav_by_magic() {
        let wav_data = make_test_wav(22050, 1, &[0, 100, -100]);

        let importer = AudioImporter::new();
        let result = importer.import_bytes(&wav_data).unwrap();

        assert_eq!(result.format, AudioFormat::Wav);
        assert_eq!(result.sample_rate, 22050);
        assert_eq!(result.channels, 1);
    }

    #[test]
    fn import_bytes_rejects_unknown_magic() {
        let importer = AudioImporter::new();
        let result = importer.import_bytes(&[0xFF, 0xFE, 0xFD, 0xFC, 0x00]);
        assert!(result.is_err());
    }

    #[test]
    fn import_bytes_rejects_too_short() {
        let importer = AudioImporter::new();
        let result = importer.import_bytes(&[0, 1]);
        assert!(result.is_err());
    }

    // -- AudioImporter: file-based import ------------------------------------

    #[test]
    fn import_wav_file_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let wav_path = dir.path().join("test.wav");
        let wav_data = make_test_wav(44100, 2, &[100, -100, 200, -200]);
        std::fs::write(&wav_path, &wav_data).unwrap();

        let importer = AudioImporter::new();
        let result = importer.import_file(&wav_path).unwrap();

        assert_eq!(result.sample_rate, 44100);
        assert_eq!(result.channels, 2);
        assert_eq!(result.frame_count, 2);
        assert_eq!(result.format, AudioFormat::Wav);
    }

    #[test]
    fn import_file_header_fallback_for_unknown_extension() {
        let dir = tempfile::tempdir().unwrap();
        // File has .dat extension but WAV content.
        let path = dir.path().join("audio.dat");
        std::fs::write(&path, make_test_wav(44100, 1, &[0, 100])).unwrap();

        let importer = AudioImporter::new();
        let result = importer.import_file(&path).unwrap();
        assert_eq!(result.format, AudioFormat::Wav);
    }

    #[test]
    fn import_file_not_found() {
        let importer = AudioImporter::new();
        let result = importer.import_file(Path::new("/nonexistent/audio.wav"));
        assert!(result.is_err());
    }

    #[test]
    fn import_file_unsupported_extension_and_bad_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("music.mp3");
        std::fs::write(&path, b"NOT_AUDIO_DATA_HERE").unwrap();

        let importer = AudioImporter::new();
        let result = importer.import_file(&path);
        assert!(result.is_err());
    }

    // -- AudioImportResult → Resource conversion ------------------------------

    #[test]
    fn wav_result_to_resource() {
        let result = AudioImportResult {
            sample_rate: 44100,
            channels: 2,
            duration_secs: 1.5,
            frame_count: 66150,
            format: AudioFormat::Wav,
            samples: vec![0.0; 132300],
        };
        let res = result.to_resource("res://sfx/explosion.wav");

        assert_eq!(res.class_name, "AudioStreamWAV");
        assert_eq!(res.path, "res://sfx/explosion.wav");
        assert_eq!(res.get_property("sample_rate"), Some(&Variant::Int(44100)));
        assert_eq!(res.get_property("channels"), Some(&Variant::Int(2)));
        assert_eq!(
            res.get_property("frame_count"),
            Some(&Variant::Int(66150))
        );
        assert_eq!(
            res.get_property("has_samples"),
            Some(&Variant::Bool(true))
        );
        // duration
        if let Some(Variant::Float(d)) = res.get_property("length_seconds") {
            assert!((*d - 1.5).abs() < 0.01);
        } else {
            panic!("expected length_seconds float property");
        }
    }

    #[test]
    fn ogg_result_to_resource() {
        let result = AudioImportResult {
            sample_rate: 48000,
            channels: 1,
            duration_secs: 2.0,
            frame_count: 96000,
            format: AudioFormat::OggVorbis,
            samples: vec![0.0; 96000],
        };
        let res = result.to_resource("res://music/theme.ogg");

        assert_eq!(res.class_name, "AudioStreamOggVorbis");
        assert_eq!(res.path, "res://music/theme.ogg");
        assert_eq!(res.get_property("sample_rate"), Some(&Variant::Int(48000)));
        assert_eq!(res.get_property("channels"), Some(&Variant::Int(1)));
    }

    // -- import_resource (file → Resource) ------------------------------------

    #[test]
    fn import_resource_creates_wav_resource() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sound.wav");
        let wav_data = make_test_wav(22050, 1, &[0, 500, -500, 0, 250]);
        std::fs::write(&path, &wav_data).unwrap();

        let importer = AudioImporter::new();
        let res = importer.import_resource(&path).unwrap();

        assert_eq!(res.class_name, "AudioStreamWAV");
        assert_eq!(res.path, "res://sound.wav");
        assert_eq!(res.get_property("sample_rate"), Some(&Variant::Int(22050)));
        assert_eq!(res.get_property("channels"), Some(&Variant::Int(1)));
        assert_eq!(
            res.get_property("has_samples"),
            Some(&Variant::Bool(true))
        );
    }

    // -- Full pipeline: import → decode → verify samples ----------------------

    #[test]
    fn full_pipeline_wav_dc_value() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dc.wav");

        // Generate a known DC value: 16384/32768 = 0.5
        let samples: Vec<i16> = vec![16384; 4410]; // ~0.1s at 44100
        let wav_data = make_test_wav(44100, 1, &samples);
        std::fs::write(&path, &wav_data).unwrap();

        let importer = AudioImporter::new();
        let result = importer.import_file(&path).unwrap();

        assert_eq!(result.sample_rate, 44100);
        assert_eq!(result.channels, 1);
        assert_eq!(result.frame_count, 4410);
        assert!(result.duration_secs > 0.09 && result.duration_secs < 0.11);

        // All samples should be ~0.5.
        for &s in &result.samples {
            assert!(
                (s - 0.5).abs() < 0.01,
                "expected ~0.5, got {s}"
            );
        }
    }
}
