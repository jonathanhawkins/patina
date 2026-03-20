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
