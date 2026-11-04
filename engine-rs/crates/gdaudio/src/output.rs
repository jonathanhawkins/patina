//! Platform audio output backends (pat-p0s82.1).
//!
//! Provides:
//! - [`mix_and_write`] — pump [`AudioServer::mix`] into any [`AudioOutputStream`]
//! - [`CaptureAudioOutput`] — records mixed PCM for tests / offline capture
//! - [`PlatformAudioOutput`] — real device output via `cpal` (CoreAudio / WASAPI / ALSA)

// Only the cpal-backed device output shares state with a callback thread, and
// that whole half of the module is gated off on wasm32 — an ungated import here
// would warn on every web build.
#[cfg(not(target_arch = "wasm32"))]
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
    mix_and_write_gain(server, output, frame_count, 1.0)
}

/// [`mix_and_write`] with a final output gain. The mix itself — every voice's
/// playhead, every bus's peak meter — is unaffected; only what reaches the
/// device is scaled. `0.0` is the editor's Game-view mute: the game keeps
/// running exactly as unmuted, and its own `AudioServer` bus state is not
/// touched (Godot's editor mute never changes what the game can read back).
pub fn mix_and_write_gain(
    server: &mut AudioServer,
    output: &mut dyn AudioOutputStream,
    frame_count: usize,
    gain: f32,
) -> usize {
    let mut mixed = server.mix(frame_count);
    if gain != 1.0 {
        for s in &mut mixed {
            *s *= gain;
        }
    }
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

/// The queue between the mixer (pushing once per game frame) and the device
/// callback (pulling on its own clock).
///
/// ⚠️ Three things the old `Vec` + `remove(0)` got wrong, each audible as
/// crackle on continuous audio (music) while short effects sounded fine:
/// - `remove(0)` shifted the whole backlog for EVERY sample, inside the
///   real-time callback and under the lock — its cost grew with the backlog
///   until the callback missed its deadline. A `VecDeque` pops in O(1).
/// - An empty queue mid-callback wrote silence right there: game frames (60
///   Hz) and device callbacks (~93 Hz at 512 frames) do not line up, so the
///   queue routinely ran dry between pushes. Playback now waits for
///   [`PREBUFFER_SECS`] of audio before starting, and re-primes after any
///   underrun, so a jittery producer costs latency, not a gap.
/// - Nothing bounded the backlog, so a producer running ahead built latency
///   forever. [`MAX_BACKLOG_SECS`] drops the oldest excess.
#[derive(Debug, Default)]
pub struct SharedRing {
    /// Interleaved f32 samples waiting to be played.
    pub samples: std::collections::VecDeque<f32>,
    /// Total frames accepted from `write_samples`.
    pub frames_accepted: usize,
    /// Total frames consumed by the device callback.
    pub frames_consumed: usize,
    /// Device callbacks that ran out of audio part-way (each one is a gap).
    pub underruns: usize,
    /// Device callbacks served at all.
    pub callbacks: usize,
    /// Callbacks answered with priming silence (start-up, or after an underrun).
    pub priming_callbacks: usize,
    /// Deepest the queue has been, in frames — how close to the latency cap.
    pub max_queued_frames: usize,
    /// Samples dropped by the backlog cap (a producer running ahead).
    pub dropped_samples: usize,
    /// Samples the device was handed outside `[-1, 1]` (hard clipping).
    pub clipped_samples: usize,
    /// The last [`HISTORY_SECS`] of EXACTLY what the device was handed,
    /// interleaved, silence included — the ground truth for "does it crackle".
    pub history: std::collections::VecDeque<f32>,
    /// Whether enough audio is queued to play without starving.
    primed: bool,
}

/// How much device output [`SharedRing::history`] keeps.
pub const HISTORY_SECS: f32 = 10.0;

/// A snapshot of the device queue, for diagnostics (`/api/runtime/audio`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeviceAudioStats {
    pub sample_rate: u32,
    pub channels: u16,
    pub callbacks: usize,
    pub underruns: usize,
    pub priming_callbacks: usize,
    pub queued_frames: usize,
    pub max_queued_frames: usize,
    pub frames_accepted: usize,
    pub frames_consumed: usize,
    pub dropped_samples: usize,
    pub clipped_samples: usize,
}

/// Audio queued before playback starts (and after an underrun): absorbs the
/// beat between the game's frame clock and the device's callback clock.
pub const PREBUFFER_SECS: f32 = 0.05;
/// The most audio kept queued; beyond it the oldest is dropped.
pub const MAX_BACKLOG_SECS: f32 = 0.25;

impl SharedRing {
    /// Queues interleaved samples, keeping at most [`MAX_BACKLOG_SECS`].
    pub fn push(&mut self, samples: &[f32], sample_rate: u32, channels: u16) {
        self.samples.extend(samples.iter().copied());
        self.frames_accepted += samples.len() / channels.max(1) as usize;
        self.max_queued_frames = self
            .max_queued_frames
            .max(self.samples.len() / channels.max(1) as usize);
        let cap = (MAX_BACKLOG_SECS * sample_rate as f32) as usize * channels.max(1) as usize;
        if self.samples.len() > cap {
            let excess = self.samples.len() - cap;
            // Whole frames only, so channels never swap.
            let excess = excess - excess % channels.max(1) as usize;
            self.dropped_samples += excess;
            self.samples.drain(..excess);
        }
    }

    /// Counters only (cheap; no history copy).
    pub fn stats(&self, sample_rate: u32, channels: u16) -> DeviceAudioStats {
        DeviceAudioStats {
            sample_rate,
            channels,
            callbacks: self.callbacks,
            underruns: self.underruns,
            priming_callbacks: self.priming_callbacks,
            queued_frames: self.samples.len() / channels.max(1) as usize,
            max_queued_frames: self.max_queued_frames,
            frames_accepted: self.frames_accepted,
            frames_consumed: self.frames_consumed,
            dropped_samples: self.dropped_samples,
            clipped_samples: self.clipped_samples,
        }
    }

    /// Fills one device callback. Silence while priming; an empty queue
    /// part-way counts an underrun and re-primes.
    pub fn pull(&mut self, out: &mut [f32], sample_rate: u32, channels: u16) {
        self.pull_inner(out, sample_rate, channels);
        self.callbacks += 1;
        let cap = (HISTORY_SECS * sample_rate as f32) as usize * channels.max(1) as usize;
        for &s in out.iter() {
            if !(-1.0..=1.0).contains(&s) {
                self.clipped_samples += 1;
            }
        }
        self.history.extend(out.iter().copied());
        if self.history.len() > cap {
            let excess = self.history.len() - cap;
            let excess = excess - excess % channels.max(1) as usize;
            self.history.drain(..excess);
        }
    }

    fn pull_inner(&mut self, out: &mut [f32], sample_rate: u32, channels: u16) {
        let ch = channels.max(1) as usize;
        let prebuffer = (PREBUFFER_SECS * sample_rate as f32) as usize * ch;
        if !self.primed && self.samples.len() >= prebuffer.max(out.len()) {
            self.primed = true;
        }
        if !self.primed {
            out.fill(0.0);
            self.priming_callbacks += 1;
            return;
        }
        let mut starved = false;
        for (i, slot) in out.iter_mut().enumerate() {
            match self.samples.pop_front() {
                Some(s) => {
                    *slot = s;
                    if (i + 1) % ch == 0 {
                        self.frames_consumed += 1;
                    }
                }
                None => {
                    *slot = 0.0;
                    starved = true;
                }
            }
        }
        if starved {
            self.underruns += 1;
            self.primed = false;
        }
    }
}

/// The live platform device's queue, published for diagnostics: the device
/// lives on the game thread, but the editor's API thread has to read it.
#[cfg(not(target_arch = "wasm32"))]
static LIVE_DEVICE: Mutex<Option<(Arc<Mutex<SharedRing>>, u32, u16)>> = Mutex::new(None);

/// Counters for the most recently opened platform device, if any.
#[cfg(not(target_arch = "wasm32"))]
pub fn live_device_stats() -> Option<DeviceAudioStats> {
    let guard = LIVE_DEVICE.lock().ok()?;
    let (ring, rate, ch) = guard.as_ref()?;
    let r = ring.lock().ok()?;
    Some(r.stats(*rate, *ch))
}

/// The last `secs` of what the live device actually played, interleaved, with
/// its `(sample_rate, channels)`.
#[cfg(not(target_arch = "wasm32"))]
pub fn live_device_capture(secs: f32) -> Option<(Vec<f32>, u32, u16)> {
    let guard = LIVE_DEVICE.lock().ok()?;
    let (ring, rate, ch) = guard.as_ref()?;
    let r = ring.lock().ok()?;
    let want = (secs.max(0.0) * *rate as f32) as usize * *ch as usize;
    let skip = r.history.len().saturating_sub(want);
    Some((r.history.iter().skip(skip).copied().collect(), *rate, *ch))
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
        let ring = Arc::new(Mutex::new(SharedRing::default()));
        let ring_cb = Arc::clone(&ring);

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                build_stream::<f32>(&device, &config.into(), ring_cb, sample_rate)?
            }
            cpal::SampleFormat::I16 => {
                build_stream::<i16>(&device, &config.into(), ring_cb, sample_rate)?
            }
            cpal::SampleFormat::U16 => {
                build_stream::<u16>(&device, &config.into(), ring_cb, sample_rate)?
            }
            other => {
                return Err(PlatformAudioError::Config(format!(
                    "unsupported sample format {other:?}"
                )));
            }
        };

        stream
            .play()
            .map_err(|e| PlatformAudioError::Stream(e.to_string()))?;
        if let Ok(mut live) = LIVE_DEVICE.lock() {
            *live = Some((Arc::clone(&ring), sample_rate, channels));
        }

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

    /// Device callbacks that ran out of audio part-way — each is an audible gap.
    pub fn underruns(&self) -> usize {
        self.ring.lock().map(|r| r.underruns).unwrap_or(0)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    ring: Arc<Mutex<SharedRing>>,
    sample_rate: u32,
) -> Result<cpal::Stream, PlatformAudioError>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    use cpal::traits::DeviceTrait;

    let channels = config.channels;
    let err_fn = |e| tracing::error!(error = %e, "cpal stream error");
    // Reused across callbacks: no allocation on the real-time thread after the
    // first one of a given size.
    let mut scratch: Vec<f32> = Vec::new();

    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                scratch.resize(data.len(), 0.0);
                match ring.lock() {
                    Ok(mut guard) => guard.pull(&mut scratch, sample_rate, channels),
                    Err(_) => scratch.fill(0.0),
                }
                for (slot, s) in data.iter_mut().zip(scratch.iter()) {
                    *slot = T::from_sample(*s);
                }
            },
            err_fn,
            None,
        )
        .map_err(|e| PlatformAudioError::Stream(e.to_string()))
}

#[cfg(not(target_arch = "wasm32"))]
impl AudioOutputStream for PlatformAudioOutput {
    fn queued_frames(&self) -> Option<usize> {
        self.ring
            .lock()
            .ok()
            .map(|r| r.samples.len() / self.channels.max(1) as usize)
    }

    fn write_samples(&mut self, buffer: &AudioSampleBuffer) -> usize {
        let frames = buffer.frame_count();
        if let Ok(mut guard) = self.ring.lock() {
            guard.push(&buffer.samples, self.sample_rate, self.channels);
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

    /// Native only: `PlatformAudioOutput` is the cpal device layer and does not
    /// exist on wasm32, where the browser's AudioWorklet is the device instead.
    #[cfg(not(target_arch = "wasm32"))]
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
