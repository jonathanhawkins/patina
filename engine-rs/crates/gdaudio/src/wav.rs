//! WAV file decoder.
//!
//! Parses WAV files (RIFF/fmt/data chunks) into PCM sample buffers.
//! Supports 8-bit unsigned, 16-bit signed, 24-bit signed, and 32-bit float
//! formats, converting all to normalized `f32` samples.

use crate::sample::{AudioSampleBuffer, ChannelLayout};

/// Errors that can occur during WAV decoding.
#[derive(Debug, Clone, PartialEq)]
pub enum WavError {
    /// File is too short to contain a valid header.
    TooShort,
    /// Missing or invalid RIFF header magic.
    InvalidRiffHeader,
    /// Missing or invalid WAVE format identifier.
    InvalidWaveFormat,
    /// The fmt chunk was not found.
    MissingFmtChunk,
    /// The data chunk was not found.
    MissingDataChunk,
    /// Unsupported audio format (only PCM=1 and IEEE float=3 are supported).
    UnsupportedFormat(u16),
    /// Unsupported bits per sample.
    UnsupportedBitsPerSample(u16),
    /// Data chunk is truncated or corrupt.
    TruncatedData,
}

impl std::fmt::Display for WavError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WavError::TooShort => write!(f, "WAV file too short"),
            WavError::InvalidRiffHeader => write!(f, "invalid RIFF header"),
            WavError::InvalidWaveFormat => write!(f, "invalid WAVE format"),
            WavError::MissingFmtChunk => write!(f, "missing fmt chunk"),
            WavError::MissingDataChunk => write!(f, "missing data chunk"),
            WavError::UnsupportedFormat(fmt) => write!(f, "unsupported audio format: {fmt}"),
            WavError::UnsupportedBitsPerSample(b) => {
                write!(f, "unsupported bits per sample: {b}")
            }
            WavError::TruncatedData => write!(f, "truncated data chunk"),
        }
    }
}

impl std::error::Error for WavError {}

/// Parsed WAV file header information.
#[derive(Debug, Clone)]
pub struct WavHeader {
    /// Audio format (1 = PCM, 3 = IEEE float).
    pub audio_format: u16,
    /// Number of channels.
    pub num_channels: u16,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Bits per sample (8, 16, 24, or 32).
    pub bits_per_sample: u16,
}

/// Reads a little-endian u16 from a byte slice.
fn read_u16_le(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([data[offset], data[offset + 1]])
}

/// Reads a little-endian u32 from a byte slice.
fn read_u32_le(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

/// Parses just the WAV header without decoding sample data.
pub fn parse_wav_header(data: &[u8]) -> Result<WavHeader, WavError> {
    if data.len() < 44 {
        return Err(WavError::TooShort);
    }

    // Validate RIFF header
    if &data[0..4] != b"RIFF" {
        return Err(WavError::InvalidRiffHeader);
    }

    // Validate WAVE format
    if &data[8..12] != b"WAVE" {
        return Err(WavError::InvalidWaveFormat);
    }

    // Find fmt chunk
    let (fmt_offset, _) = find_chunk(data, b"fmt ").ok_or(WavError::MissingFmtChunk)?;

    let audio_format = read_u16_le(data, fmt_offset);
    let num_channels = read_u16_le(data, fmt_offset + 2);
    let sample_rate = read_u32_le(data, fmt_offset + 4);
    let bits_per_sample = read_u16_le(data, fmt_offset + 14);

    Ok(WavHeader {
        audio_format,
        num_channels,
        sample_rate,
        bits_per_sample,
    })
}

/// Finds a chunk by its 4-byte ID and returns (data_offset, data_size).
fn find_chunk(data: &[u8], id: &[u8; 4]) -> Option<(usize, usize)> {
    let mut offset = 12; // Skip RIFF header (12 bytes)
    while offset + 8 <= data.len() {
        let chunk_id = &data[offset..offset + 4];
        let chunk_size = read_u32_le(data, offset + 4) as usize;
        if chunk_id == id {
            return Some((offset + 8, chunk_size));
        }
        // Advance to next chunk (pad to even boundary per WAV spec)
        offset += 8 + chunk_size;
        if chunk_size % 2 != 0 {
            offset += 1;
        }
    }
    None
}

/// Decodes a WAV file from raw bytes into an [`AudioSampleBuffer`].
///
/// Supports:
/// - PCM 8-bit unsigned
/// - PCM 16-bit signed
/// - PCM 24-bit signed
/// - IEEE 32-bit float
pub fn decode_wav(data: &[u8]) -> Result<AudioSampleBuffer, WavError> {
    let header = parse_wav_header(data)?;

    // Validate format
    match header.audio_format {
        1 => {} // PCM
        3 => {} // IEEE float
        f => return Err(WavError::UnsupportedFormat(f)),
    }

    match header.bits_per_sample {
        8 | 16 | 24 => {
            if header.audio_format != 1 {
                return Err(WavError::UnsupportedFormat(header.audio_format));
            }
        }
        32 => {} // PCM i32 or IEEE float
        b => return Err(WavError::UnsupportedBitsPerSample(b)),
    }

    // Find data chunk
    let (data_offset, data_size) = find_chunk(data, b"data").ok_or(WavError::MissingDataChunk)?;

    if data_offset + data_size > data.len() {
        return Err(WavError::TruncatedData);
    }

    let raw = &data[data_offset..data_offset + data_size];
    let bytes_per_sample = (header.bits_per_sample / 8) as usize;
    let num_samples = data_size / bytes_per_sample;

    let samples: Vec<f32> = match (header.audio_format, header.bits_per_sample) {
        (1, 8) => {
            // 8-bit unsigned PCM: 0..255, 128 = silence
            raw.iter().map(|&b| (b as f32 - 128.0) / 128.0).collect()
        }
        (1, 16) => {
            // 16-bit signed PCM
            (0..num_samples)
                .map(|i| {
                    let s = i16::from_le_bytes([raw[i * 2], raw[i * 2 + 1]]);
                    s as f32 / 32768.0
                })
                .collect()
        }
        (1, 24) => {
            // 24-bit signed PCM
            (0..num_samples)
                .map(|i| {
                    let b0 = raw[i * 3] as i32;
                    let b1 = raw[i * 3 + 1] as i32;
                    let b2 = raw[i * 3 + 2] as i32;
                    let s = b0 | (b1 << 8) | (b2 << 16);
                    // Sign-extend from 24 bits
                    let s = if s & 0x800000 != 0 { s | !0xFF_FFFF } else { s };
                    s as f32 / 8_388_608.0
                })
                .collect()
        }
        (3, 32) => {
            // 32-bit IEEE float
            (0..num_samples)
                .map(|i| {
                    f32::from_le_bytes([raw[i * 4], raw[i * 4 + 1], raw[i * 4 + 2], raw[i * 4 + 3]])
                })
                .collect()
        }
        (1, 32) => {
            // 32-bit signed PCM
            (0..num_samples)
                .map(|i| {
                    let s = i32::from_le_bytes([
                        raw[i * 4],
                        raw[i * 4 + 1],
                        raw[i * 4 + 2],
                        raw[i * 4 + 3],
                    ]);
                    s as f32 / 2_147_483_648.0
                })
                .collect()
        }
        _ => return Err(WavError::UnsupportedBitsPerSample(header.bits_per_sample)),
    };

    let layout = match header.num_channels {
        1 => ChannelLayout::Mono,
        2 => ChannelLayout::Stereo,
        n => ChannelLayout::Custom(n),
    };

    Ok(AudioSampleBuffer {
        sample_rate: header.sample_rate,
        channels: header.num_channels,
        channel_layout: layout,
        samples,
    })
}

/// Builds a minimal valid WAV file from raw parameters (for testing).
pub fn build_wav_bytes(
    sample_rate: u32,
    num_channels: u16,
    bits_per_sample: u16,
    audio_format: u16,
    sample_data: &[u8],
) -> Vec<u8> {
    let fmt_chunk_size: u32 = 16;
    let data_chunk_size = sample_data.len() as u32;
    let riff_size = 4 + (8 + fmt_chunk_size) + (8 + data_chunk_size);

    let byte_rate = sample_rate * num_channels as u32 * (bits_per_sample / 8) as u32;
    let block_align = num_channels * (bits_per_sample / 8);

    let mut buf = Vec::with_capacity(44 + sample_data.len());

    // RIFF header
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&riff_size.to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    // fmt chunk
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&fmt_chunk_size.to_le_bytes());
    buf.extend_from_slice(&audio_format.to_le_bytes());
    buf.extend_from_slice(&num_channels.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits_per_sample.to_le_bytes());

    // data chunk
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_chunk_size.to_le_bytes());
    buf.extend_from_slice(sample_data);

    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_wav_header_16bit_mono() {
        let wav = build_wav_bytes(44100, 1, 16, 1, &[0u8; 4]);
        let header = parse_wav_header(&wav).unwrap();
        assert_eq!(header.audio_format, 1);
        assert_eq!(header.num_channels, 1);
        assert_eq!(header.sample_rate, 44100);
        assert_eq!(header.bits_per_sample, 16);
    }

    #[test]
    fn decode_wav_header_stereo() {
        let wav = build_wav_bytes(48000, 2, 16, 1, &[0u8; 8]);
        let header = parse_wav_header(&wav).unwrap();
        assert_eq!(header.num_channels, 2);
        assert_eq!(header.sample_rate, 48000);
    }

    #[test]
    fn decode_16bit_pcm_samples() {
        // Two 16-bit samples: silence (0) and half-max (16384 = 0x4000)
        let sample_data: Vec<u8> = vec![
            0x00, 0x00, // sample 0: silence
            0x00, 0x40, // sample 1: 16384 (half positive range)
        ];
        let wav = build_wav_bytes(44100, 1, 16, 1, &sample_data);
        let buffer = decode_wav(&wav).unwrap();

        assert_eq!(buffer.sample_rate, 44100);
        assert_eq!(buffer.channels, 1);
        assert_eq!(buffer.samples.len(), 2);
        assert!((buffer.samples[0] - 0.0).abs() < 1e-5, "silence sample");
        assert!(
            (buffer.samples[1] - 0.5).abs() < 0.01,
            "half-max sample: {}",
            buffer.samples[1]
        );
    }

    #[test]
    fn decode_8bit_pcm_samples() {
        // 8-bit unsigned: 128 = silence, 255 ≈ +1.0, 0 ≈ -1.0
        let sample_data = vec![128u8, 255u8, 0u8];
        let wav = build_wav_bytes(22050, 1, 8, 1, &sample_data);
        let buffer = decode_wav(&wav).unwrap();

        assert_eq!(buffer.samples.len(), 3);
        assert!((buffer.samples[0]).abs() < 1e-5, "128 = silence");
        assert!(buffer.samples[1] > 0.9, "255 ≈ +1.0");
        assert!(buffer.samples[2] < -0.9, "0 ≈ -1.0");
    }

    #[test]
    fn decode_32bit_float_samples() {
        let sample_data: Vec<u8> = [0.5f32, -0.5f32, 1.0f32]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        let wav = build_wav_bytes(44100, 1, 32, 3, &sample_data);
        let buffer = decode_wav(&wav).unwrap();

        assert_eq!(buffer.samples.len(), 3);
        assert!((buffer.samples[0] - 0.5).abs() < 1e-6);
        assert!((buffer.samples[1] - (-0.5)).abs() < 1e-6);
        assert!((buffer.samples[2] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn reject_invalid_riff_header() {
        let data = b"NOT_RIFF_DATA_HERE_ENOUGH_BYTES_FOR_HEADER_VALIDATION_PASSES";
        assert_eq!(
            parse_wav_header(data).unwrap_err(),
            WavError::InvalidRiffHeader
        );
    }

    #[test]
    fn reject_too_short() {
        assert_eq!(
            parse_wav_header(&[0u8; 10]).unwrap_err(),
            WavError::TooShort
        );
    }

    #[test]
    fn reject_unsupported_format() {
        // Audio format 2 = ADPCM, unsupported
        let wav = build_wav_bytes(44100, 1, 16, 2, &[0u8; 4]);
        assert_eq!(
            decode_wav(&wav).unwrap_err(),
            WavError::UnsupportedFormat(2)
        );
    }

    #[test]
    fn stereo_buffer_channel_layout() {
        let sample_data = vec![0u8; 8]; // 2 stereo frames * 2 bytes each
        let wav = build_wav_bytes(44100, 2, 16, 1, &sample_data);
        let buffer = decode_wav(&wav).unwrap();

        assert_eq!(buffer.channels, 2);
        assert_eq!(buffer.channel_layout, ChannelLayout::Stereo);
    }
}
