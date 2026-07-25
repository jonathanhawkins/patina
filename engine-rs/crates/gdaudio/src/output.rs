//! Platform audio output backends (pat-p0s82.1).
//!
//! Provides:
//! - [`mix_and_write`] — pump [`AudioServer::mix`] into any [`AudioOutputStream`]
//! - [`CaptureAudioOutput`] — records mixed PCM for tests / offline capture
//! - [`PlatformAudioOutput`] — real device output via `cpal` (CoreAudio / WASAPI / ALSA)

use std::sync::{Arc, Mutex};

use crate::sample::{AudioOutputStream, AudioSampleBuffer, ChannelLayout};
use crate::server::AudioServer;

/// Mix `frame_count` frames from `server` and write them to `output`.
///
/// Returns the number of frames the backend reports as written.
pub fn mix_and_write(
    server: &mut AudioServer,
    output: &mut dyn AudioOutputStream,
    frame_count: usize,
) -> usize {
    let mixed = server.mix(frame_count);
    let channels = server.output_channels();
    let sample_rate = server.output_sample_rate();
    let layout = match channels {
        1 => ChannelLayout::Mono,
        2 => ChannelLayout::Stereo,
        n => ChannelLayout::Custom(n),
    };
    let buffer = AudioSampleBuffer {
        sample_rate,
        channels,
        channel_layout: layout,
        samples: mixed,
    };
    output.write_samples(&buffer)
}

/// Records every sample written — used in tests and headless capture pipelines.
#[derive(Debug, Default)]
pub struct CaptureAudioOutput {
    sample_rate: u32,
    channels: u16,
    /// Total frames received.
    pub frames_written: usize,
    /// Concatenated interleaved PCM written so far.
    pub samples: Vec<f32>,
}

impl CaptureAudioOutput {
    /// Create a capture sink with the given format.
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            sample_rate,
            channels,
            frames_written: 0,
            samples: Vec::new(),
        }
    }
}

impl AudioOutputStream for CaptureAudioOutput {
    fn write_samples(&mut self, buffer: &AudioSampleBuffer) -> usize {
        let frames = buffer.frame_count();
        self.frames_written += frames;
        self.samples.extend_from_slice(&buffer.samples);
        frames
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn channels(&self) -> u16 {
        self.channels
    }
}

/// Shared ring used by the cpal callback and the producer side.
#[cfg(not(target_arch = "wasm32"))]
struct SharedRing {
    /// Interleaved f32 samples waiting to be played.
    samples: Vec<f32>,
    /// Total frames accepted from `write_samples`.
    frames_accepted: usize,
    /// Total frames consumed by the device callback.
    frames_consumed: usize,
}

/// Real platform audio output using the system default output device via cpal.
///
/// On macOS this is CoreAudio; on Windows WASAPI; on Linux ALSA/Pulse via cpal.
/// If no device is available, [`PlatformAudioOutput::try_open`] returns an error
/// so callers can fall back to [`crate::sample::NullAudioOutput`].
#[cfg(not(target_arch = "wasm32"))]
pub struct PlatformAudioOutput {
    sample_rate: u32,
    channels: u16,
    ring: Arc<Mutex<SharedRing>>,
    /// Kept alive to sustain the audio stream.
    _stream: cpal::Stream,
}

/// Errors opening a platform audio device.
#[derive(Debug, thiserror::Error)]
#[cfg(not(target_arch = "wasm32"))]
pub enum PlatformAudioError {
    /// No default output device.
    #[error("no default output device: {0}")]
    NoDevice(String),
    /// Stream configuration failed.
    #[error("stream config error: {0}")]
    Config(String),
    /// Stream build/play failed.
    #[error("stream error: {0}")]
    Stream(String),
}

#[cfg(not(target_arch = "wasm32"))]
impl PlatformAudioOutput {
    /// Open the default output device and start a pull stream.
    pub fn try_open() -> Result<Self, PlatformAudioError> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| PlatformAudioError::NoDevice("none reported by host".into()))?;

        let config = device
            .default_output_config()
            .map_err(|e| PlatformAudioError::Config(e.to_string()))?;

        let sample_rate = config.sample_rate().0;
        let channels = config.channels();
        let ring = Arc::new(Mutex::new(SharedRing {
            samples: Vec::new(),
            frames_accepted: 0,
            frames_consumed: 0,
        }));
        let ring_cb = Arc::clone(&ring);

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => build_stream::<f32>(&device, &config.into(), ring_cb)?,
            cpal::SampleFormat::I16 => build_stream::<i16>(&device, &config.into(), ring_cb)?,
            cpal::SampleFormat::U16 => build_stream::<u16>(&device, &config.into(), ring_cb)?,
            other => {
                return Err(PlatformAudioError::Config(format!(
                    "unsupported sample format {other:?}"
                )));
            }
        };

        stream
            .play()
            .map_err(|e| PlatformAudioError::Stream(e.to_string()))?;

        Ok(Self {
            sample_rate,
            channels,
            ring,
            _stream: stream,
        })
    }

    /// Frames accepted from the mixer side (not necessarily yet played).
    pub fn frames_accepted(&self) -> usize {
        self.ring.lock().map(|r| r.frames_accepted).unwrap_or(0)
    }

    /// Frames consumed by the audio device callback.
    pub fn frames_consumed(&self) -> usize {
        self.ring.lock().map(|r| r.frames_consumed).unwrap_or(0)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    ring: Arc<Mutex<SharedRing>>,
) -> Result<cpal::Stream, PlatformAudioError>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    use cpal::traits::DeviceTrait;

    let channels = config.channels as usize;
    let err_fn = |e| tracing::error!(error = %e, "cpal stream error");

    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                let mut guard = match ring.lock() {
                    Ok(g) => g,
                    Err(_) => return,
                };
                let mut out_i = 0;
                while out_i < data.len() {
                    if guard.samples.is_empty() {
                        // underrun: silence
                        data[out_i] = T::EQUILIBRIUM;
                        out_i += 1;
                        continue;
                    }
                    let s = guard.samples.remove(0);
                    data[out_i] = T::from_sample(s);
                    out_i += 1;
                    // count a frame when a full interleaved frame is written
                    if out_i % channels == 0 {
                        guard.frames_consumed += 1;
                    }
                }
            },
            err_fn,
            None,
        )
        .map_err(|e| PlatformAudioError::Stream(e.to_string()))
}

#[cfg(not(target_arch = "wasm32"))]
impl AudioOutputStream for PlatformAudioOutput {
    fn write_samples(&mut self, buffer: &AudioSampleBuffer) -> usize {
        let frames = buffer.frame_count();
        if let Ok(mut guard) = self.ring.lock() {
            guard.samples.extend_from_slice(&buffer.samples);
            guard.frames_accepted += frames;
        }
        frames
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn channels(&self) -> u16 {
        self.channels
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::AudioBuffer;
    use crate::sample::NullAudioOutput;
    use crate::server::AudioServer;

    fn dc_buffer(value: f32, frames: usize) -> AudioBuffer {
        AudioBuffer {
            samples: vec![value; frames],
            sample_rate: 44100,
            channels: 1,
        }
    }

    #[test]
    fn mix_and_write_to_null_counts_frames() {
        let mut server = AudioServer::new();
        server.play(dc_buffer(0.5, 44100));
        let mut out = NullAudioOutput::new(44100, 2);
        let written = mix_and_write(&mut server, &mut out, 512);
        assert_eq!(written, 512);
        assert_eq!(out.frames_written, 512);
    }

    #[test]
    fn mix_and_write_to_capture_records_pcm() {
        let mut server = AudioServer::new();
        // Mono DC 0.25 — mixed into stereo output (duplicate L/R by mix logic)
        server.play(dc_buffer(0.25, 2048));
        let mut cap = CaptureAudioOutput::new(44100, 2);
        let written = mix_and_write(&mut server, &mut cap, 256);
        assert_eq!(written, 256);
        assert_eq!(cap.frames_written, 256);
        // stereo interleaved: 256 frames * 2 channels
        assert_eq!(cap.samples.len(), 256 * 2);
        // First left sample should be ~0.25 (mono duplicated into ch0)
        assert!(
            (cap.samples[0] - 0.25).abs() < 1e-4,
            "got {}",
            cap.samples[0]
        );
    }

    #[test]
    fn mix_and_write_play_wav_path_to_capture() {
        // Minimal WAV: 16-bit mono 44100 Hz, 100 frames of 0x1000 samples
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        let data_size = 100 * 2;
        let riff_size = 36 + data_size;
        wav.extend_from_slice(&(riff_size as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav.extend_from_slice(&44100u32.to_le_bytes());
        wav.extend_from_slice(&(44100u32 * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data_size as u32).to_le_bytes());
        for _ in 0..100 {
            wav.extend_from_slice(&0x1000i16.to_le_bytes());
        }

        let buf = crate::decode::decode_wav(&wav).expect("decode wav");
        let mut server = AudioServer::new();
        server.play(buf);
        let mut cap = CaptureAudioOutput::new(44100, 2);
        let n = mix_and_write(&mut server, &mut cap, 50);
        assert_eq!(n, 50);
        assert_eq!(cap.frames_written, 50);
        assert!(!cap.samples.is_empty());
        // Sample should be non-zero (0x1000 / 32768 ≈ 0.125)
        assert!(
            cap.samples[0].abs() > 0.01,
            "expected audible sample, got {}",
            cap.samples[0]
        );
    }

    #[test]
    fn platform_output_try_open_is_real_backend() {
        // In CI without audio this may fail; that is OK. When it succeeds we
        // prove write_samples queues frames on the real cpal path.
        match PlatformAudioOutput::try_open() {
            Ok(mut platform) => {
                assert!(platform.sample_rate() > 0);
                assert!(platform.channels() >= 1);
                let mut server = AudioServer::new();
                server.play(dc_buffer(0.01, 4096));
                let written = mix_and_write(&mut server, &mut platform, 256);
                assert_eq!(written, 256);
                assert_eq!(platform.frames_accepted(), 256);
            }
            Err(e) => {
                // Still prove the type exists and error is structured.
                let msg = e.to_string();
                assert!(!msg.is_empty());
                // Capture path still covers mix→output for CI.
                let mut server = AudioServer::new();
                server.play(dc_buffer(0.5, 512));
                let mut cap = CaptureAudioOutput::new(44100, 2);
                assert_eq!(mix_and_write(&mut server, &mut cap, 128), 128);
            }
        }
    }
}
