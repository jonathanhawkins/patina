//! Audio server that manages streams, mixing, and output buffer generation.
//!
//! The [`AudioServer`] is the central coordinator: it owns the [`AudioMixer`],
//! manages active [`AudioStreamPlayback`] instances, and produces mixed output
//! buffers each frame.

use crate::bus::AudioBus;
use crate::decode::AudioBuffer;
use crate::mixer::AudioMixer;
use crate::stream::{AudioStreamPlayback, PlaybackState};

/// Handle to a playing stream inside the [`AudioServer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlaybackId(pub u64);

/// An active stream with its decoded audio data.
struct ActiveStream {
    playback: AudioStreamPlayback,
    buffer: AudioBuffer,
}

/// The audio server manages playback, mixing, and output buffer generation.
pub struct AudioServer {
    mixer: AudioMixer,
    streams: Vec<(PlaybackId, ActiveStream)>,
    next_id: u64,
    /// Master sample rate for output.
    output_sample_rate: u32,
    /// Master channel count for output.
    output_channels: u16,
}

impl AudioServer {
    /// Creates a new audio server with default settings (44100 Hz, stereo).
    pub fn new() -> Self {
        Self {
            mixer: AudioMixer::new(),
            streams: Vec::new(),
            next_id: 1,
            output_sample_rate: 44100,
            output_channels: 2,
        }
    }

    /// Returns a reference to the mixer.
    pub fn mixer(&self) -> &AudioMixer {
        &self.mixer
    }

    /// Returns a mutable reference to the mixer.
    pub fn mixer_mut(&mut self) -> &mut AudioMixer {
        &mut self.mixer
    }

    /// Returns the output sample rate.
    pub fn output_sample_rate(&self) -> u32 {
        self.output_sample_rate
    }

    /// Returns the output channel count.
    pub fn output_channels(&self) -> u16 {
        self.output_channels
    }

    /// Starts playback of a decoded audio buffer. Returns a handle.
    pub fn play(&mut self, buffer: AudioBuffer) -> PlaybackId {
        let id = PlaybackId(self.next_id);
        self.next_id += 1;

        let mut playback = AudioStreamPlayback::new(buffer.duration_secs());
        playback.play();

        self.streams.push((id, ActiveStream { playback, buffer }));
        id
    }

    /// Starts playback on a specific bus.
    pub fn play_on_bus(&mut self, buffer: AudioBuffer, bus: &str) -> PlaybackId {
        let id = PlaybackId(self.next_id);
        self.next_id += 1;

        let mut playback = AudioStreamPlayback::new(buffer.duration_secs());
        playback.set_bus(bus);
        playback.play();

        self.streams.push((id, ActiveStream { playback, buffer }));
        id
    }

    /// Stops a playing stream by handle.
    pub fn stop(&mut self, id: PlaybackId) {
        if let Some(pos) = self.streams.iter().position(|(pid, _)| *pid == id) {
            self.streams.remove(pos);
        }
    }

    /// Returns whether a stream is currently playing.
    pub fn is_playing(&self, id: PlaybackId) -> bool {
        self.streams
            .iter()
            .any(|(pid, s)| *pid == id && s.playback.is_playing())
    }

    /// Returns the number of active streams.
    pub fn active_stream_count(&self) -> usize {
        self.streams.len()
    }

    /// Advances all streams and produces a mixed output buffer.
    ///
    /// The output buffer contains `frame_count * output_channels` interleaved
    /// f32 samples, mixed from all active streams through their respective buses.
    pub fn mix(&mut self, frame_count: usize) -> Vec<f32> {
        let num_samples = frame_count * self.output_channels as usize;
        let mut output = vec![0.0f32; num_samples];
        let dt = frame_count as f32 / self.output_sample_rate as f32;

        for (_id, stream) in &mut self.streams {
            let bus_name = stream.playback.get_bus().to_string();
            let bus_linear = self
                .mixer
                .get_bus_by_name(&bus_name)
                .and_then(|idx| self.mixer.get_bus(idx))
                .map(|bus| {
                    if bus.is_mute() {
                        0.0
                    } else {
                        bus.volume_linear()
                    }
                })
                .unwrap_or(1.0);

            let stream_linear = AudioBus::db_to_linear(stream.playback.volume_db());
            let gain = bus_linear * stream_linear;

            if stream.playback.state() != PlaybackState::Playing {
                continue;
            }

            // Read samples from the buffer at the current playback position
            let pos_secs = stream.playback.get_playback_position();
            let src_channels = stream.buffer.channels as usize;
            let src_rate = stream.buffer.sample_rate;

            if src_channels == 0 || src_rate == 0 {
                stream.playback.advance(dt);
                continue;
            }

            let src_frame_start = (pos_secs * src_rate as f32) as usize;

            for frame in 0..frame_count {
                let src_frame = src_frame_start + frame;
                if src_frame >= stream.buffer.frame_count() {
                    break;
                }

                let src_offset = src_frame * src_channels;
                let dst_offset = frame * self.output_channels as usize;

                for ch in 0..self.output_channels as usize {
                    let src_ch = if ch < src_channels { ch } else { 0 };
                    let sample = stream.buffer.samples[src_offset + src_ch];
                    output[dst_offset + ch] += sample * gain;
                }
            }

            stream.playback.advance(dt);
        }

        // Remove stopped streams
        self.streams
            .retain(|(_, s)| s.playback.state() != PlaybackState::Stopped);

        output
    }
}

impl Default for AudioServer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(dead_code)]
    fn make_sine_buffer(frequency: f32, duration_secs: f32, sample_rate: u32) -> AudioBuffer {
        let frame_count = (duration_secs * sample_rate as f32) as usize;
        let mut samples = Vec::with_capacity(frame_count);
        for i in 0..frame_count {
            let t = i as f32 / sample_rate as f32;
            samples.push((2.0 * std::f32::consts::PI * frequency * t).sin());
        }
        AudioBuffer {
            samples,
            sample_rate,
            channels: 1,
        }
    }

    fn make_dc_buffer(value: f32, frames: usize) -> AudioBuffer {
        AudioBuffer {
            samples: vec![value; frames],
            sample_rate: 44100,
            channels: 1,
        }
    }

    #[test]
    fn server_play_and_stop() {
        let mut server = AudioServer::new();
        let buf = make_dc_buffer(0.5, 44100);
        let id = server.play(buf);

        assert!(server.is_playing(id));
        assert_eq!(server.active_stream_count(), 1);

        server.stop(id);
        assert!(!server.is_playing(id));
        assert_eq!(server.active_stream_count(), 0);
    }

    #[test]
    fn server_mix_produces_output() {
        let mut server = AudioServer::new();
        let buf = make_dc_buffer(1.0, 44100);
        server.play(buf);

        let output = server.mix(512);
        assert_eq!(output.len(), 512 * 2); // stereo

        // DC value 1.0, mono -> both channels should be ~1.0
        assert!((output[0] - 1.0).abs() < 1e-4);
        assert!((output[1] - 1.0).abs() < 1e-4);
    }

    #[test]
    fn server_mix_applies_bus_volume() {
        let mut server = AudioServer::new();
        // Set master bus to -6 dB (~0.5012)
        server
            .mixer_mut()
            .get_bus_mut(0)
            .unwrap()
            .set_volume_db(-6.0);

        let buf = make_dc_buffer(1.0, 44100);
        server.play(buf);

        let output = server.mix(1);
        // ~0.5012
        assert!((output[0] - 0.5012).abs() < 0.01, "got {}", output[0]);
    }

    #[test]
    fn server_mix_applies_mute() {
        let mut server = AudioServer::new();
        server.mixer_mut().get_bus_mut(0).unwrap().set_mute(true);

        let buf = make_dc_buffer(1.0, 44100);
        server.play(buf);

        let output = server.mix(256);
        assert!(
            output.iter().all(|&s| s.abs() < 1e-6),
            "Muted bus should produce silence"
        );
    }

    #[test]
    fn server_multiple_streams_mix_together() {
        let mut server = AudioServer::new();
        let buf1 = make_dc_buffer(0.3, 44100);
        let buf2 = make_dc_buffer(0.4, 44100);
        server.play(buf1);
        server.play(buf2);

        let output = server.mix(1);
        // 0.3 + 0.4 = 0.7
        assert!((output[0] - 0.7).abs() < 1e-4);
    }

    #[test]
    fn server_stream_auto_removes_when_done() {
        let mut server = AudioServer::new();
        // Very short buffer — 10 frames at 44100
        let buf = make_dc_buffer(1.0, 10);
        let id = server.play(buf);

        // Mix more frames than the buffer contains
        let _ = server.mix(44100);
        assert!(!server.is_playing(id));
        assert_eq!(server.active_stream_count(), 0);
    }

    #[test]
    fn server_play_on_bus() {
        let mut server = AudioServer::new();
        let sfx_idx = server.mixer_mut().add_bus("SFX");
        server
            .mixer_mut()
            .get_bus_mut(sfx_idx)
            .unwrap()
            .set_volume_db(-20.0); // 0.1 linear

        let buf = make_dc_buffer(1.0, 44100);
        server.play_on_bus(buf, "SFX");

        let output = server.mix(1);
        // Should be ~0.1
        assert!((output[0] - 0.1).abs() < 0.01, "got {}", output[0]);
    }

    #[test]
    fn server_empty_mix() {
        let mut server = AudioServer::new();
        let output = server.mix(256);
        assert_eq!(output.len(), 256 * 2);
        assert!(output.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn server_stereo_buffer_mixes_correctly() {
        let mut server = AudioServer::new();
        // Stereo buffer: L=0.5, R=0.25
        let buf = AudioBuffer {
            samples: vec![0.5, 0.25, 0.5, 0.25, 0.5, 0.25, 0.5, 0.25],
            sample_rate: 44100,
            channels: 2,
        };
        server.play(buf);

        let output = server.mix(2);
        assert!((output[0] - 0.5).abs() < 1e-4, "L channel: {}", output[0]);
        assert!((output[1] - 0.25).abs() < 1e-4, "R channel: {}", output[1]);
    }
}
