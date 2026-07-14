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

    /// Mutable access to a stream's playback controls (volume, bus, seek).
    pub fn stream_mut(&mut self, id: PlaybackId) -> Option<&mut crate::stream::AudioStreamPlayback> {
        self.streams
            .iter_mut()
            .find(|(pid, _)| *pid == id)
            .map(|(_, s)| &mut s.playback)
    }

    /// Returns the number of active streams.
    pub fn active_stream_count(&self) -> usize {
        self.streams.len()
    }

    /// Advances all streams and produces a mixed output buffer.
    ///
    /// The output buffer contains `frame_count * output_channels` interleaved
    /// f32 samples, mixed from all active streams through the bus send graph
    /// (pat-p0s82.12).
    ///
    /// Mixing follows Godot's bus topology:
    /// 1. Each playing stream is summed (at its own volume) into an
    ///    accumulation buffer for the bus it targets.
    /// 2. Buses are processed from the highest index down to `Master` (index 0).
    ///    For each bus we apply its volume, run its effect chain, then apply the
    ///    mute/solo audibility gate.
    /// 3. Every non-master bus routes its processed output into its **send
    ///    target** (default `"Master"`) scaled by the send level. As in Godot,
    ///    a send target must sit *above* the source bus (a lower index, nearer
    ///    `Master`) so it is processed after receiving the send.
    /// 4. The `Master` bus buffer is the final output.
    ///
    /// When any bus is soloed, only soloed (non-master) buses remain audible;
    /// `Master` itself is gated only by its own mute since it is the output sink.
    pub fn mix(&mut self, frame_count: usize) -> Vec<f32> {
        let out_ch = self.output_channels as usize;
        let num_samples = frame_count * out_ch;
        let rate = self.output_sample_rate;
        let dt = frame_count as f32 / rate as f32;

        let bus_count = self.mixer.bus_count();
        // One accumulation buffer per bus, indexed by bus index.
        let mut bus_bufs: Vec<Vec<f32>> = (0..bus_count).map(|_| vec![0.0f32; num_samples]).collect();

        // 1. Sum each playing stream into its target bus buffer at stream volume.
        for (_id, stream) in &mut self.streams {
            if stream.playback.state() != PlaybackState::Playing {
                continue;
            }

            // Route to the named bus; unknown names fall back to Master (index 0).
            let bus_name = stream.playback.get_bus().to_string();
            let bus_idx = self
                .mixer
                .get_bus_by_name(&bus_name)
                .filter(|&idx| idx < bus_count)
                .unwrap_or(0);

            let stream_linear = AudioBus::db_to_linear(stream.playback.volume_db());

            let pos_secs = stream.playback.get_playback_position();
            let src_channels = stream.buffer.channels as usize;
            let src_rate = stream.buffer.sample_rate;

            if src_channels == 0 || src_rate == 0 {
                stream.playback.advance(dt);
                continue;
            }

            let src_frame_start = (pos_secs * src_rate as f32) as usize;
            let buf = &mut bus_bufs[bus_idx];

            for frame in 0..frame_count {
                let src_frame = src_frame_start + frame;
                if src_frame >= stream.buffer.frame_count() {
                    break;
                }

                let src_offset = src_frame * src_channels;
                let dst_offset = frame * out_ch;

                for ch in 0..out_ch {
                    let src_ch = if ch < src_channels { ch } else { 0 };
                    let sample = stream.buffer.samples[src_offset + src_ch];
                    buf[dst_offset + ch] += sample * stream_linear;
                }
            }

            stream.playback.advance(dt);
        }

        // Any soloed non-master bus makes all other non-master buses inaudible.
        let any_solo = (1..bus_count)
            .any(|i| self.mixer.get_bus(i).map(|b| b.is_solo()).unwrap_or(false));

        // 2 + 3. Process non-master buses from highest index down, routing each
        // into its send target so the target is processed with the send applied.
        for i in (1..bus_count).rev() {
            // Volume.
            let vol = self.mixer.get_bus(i).map(|b| b.volume_linear()).unwrap_or(1.0);
            for s in bus_bufs[i].iter_mut() {
                *s *= vol;
            }

            // Effect chain.
            if let Some(bus) = self.mixer.get_bus_mut(i) {
                if !bus.effects().is_empty() {
                    bus.process_effects(&mut bus_bufs[i], rate, out_ch as u16);
                }
            }

            // Mute / solo audibility gate.
            let (mute, solo) = self
                .mixer
                .get_bus(i)
                .map(|b| (b.is_mute(), b.is_solo()))
                .unwrap_or((false, false));
            let audible = !mute && (!any_solo || solo);

            // Resolve the send target (empty send => Master) and level.
            let (target_idx, send_lin) = {
                let bus = self.mixer.get_bus(i).unwrap();
                let target_name = if bus.send().is_empty() { "Master" } else { bus.send() };
                (self.mixer.get_bus_by_name(target_name), bus.send_linear())
            };

            // Take this bus's processed samples out and route them into the target.
            let src = std::mem::take(&mut bus_bufs[i]);
            if audible {
                if let Some(t) = target_idx {
                    if t != i && t < bus_count {
                        let target = &mut bus_bufs[t];
                        for (d, s) in target.iter_mut().zip(src.iter()) {
                            *d += s * send_lin;
                        }
                    }
                }
            }
        }

        // 4. Master bus (index 0): volume, effects, then its own mute gate.
        let master_vol = self.mixer.get_bus(0).map(|b| b.volume_linear()).unwrap_or(1.0);
        for s in bus_bufs[0].iter_mut() {
            *s *= master_vol;
        }
        if let Some(master) = self.mixer.get_bus_mut(0) {
            if !master.effects().is_empty() {
                master.process_effects(&mut bus_bufs[0], rate, out_ch as u16);
            }
        }
        if self.mixer.get_bus(0).map(|b| b.is_mute()).unwrap_or(false) {
            for s in bus_bufs[0].iter_mut() {
                *s = 0.0;
            }
        }

        // Remove stopped streams.
        self.streams
            .retain(|(_, s)| s.playback.state() != PlaybackState::Stopped);

        std::mem::take(&mut bus_bufs[0])
    }

    /// Apply the effect chain of a named bus to an interleaved buffer.
    ///
    /// Used by tests and by pipelines that mix per-bus then combine.
    pub fn process_bus_effects(
        &mut self,
        bus_name: &str,
        samples: &mut [f32],
    ) -> bool {
        let Some(idx) = self.mixer.get_bus_by_name(bus_name) else {
            return false;
        };
        if let Some(bus) = self.mixer.get_bus_mut(idx) {
            bus.process_effects(samples, self.output_sample_rate, self.output_channels);
            true
        } else {
            false
        }
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

    // ---- Bus send routing topology (pat-p0s82.12) ----

    #[test]
    fn server_send_a_to_b_then_master() {
        let mut server = AudioServer::new();
        // "B" must be *above* "A" (lower index) to receive A's send.
        server.mixer_mut().add_bus("B"); // idx 1 -> Master (default)
        let a_idx = server.mixer_mut().add_bus("A"); // idx 2
        {
            let a = server.mixer_mut().get_bus_mut(a_idx).unwrap();
            a.set_send("B");
            a.set_send_db(-6.0); // ~0.5012 linear
        }

        server.play_on_bus(make_dc_buffer(1.0, 44100), "A");
        let output = server.mix(1);

        // A(1.0) --send 0.5012--> B(unity) --> Master(unity) == ~0.5012
        assert!((output[0] - 0.5012).abs() < 0.01, "got {}", output[0]);
    }

    #[test]
    fn server_send_applies_target_bus_volume() {
        let mut server = AudioServer::new();
        let b_idx = server.mixer_mut().add_bus("B");
        server
            .mixer_mut()
            .get_bus_mut(b_idx)
            .unwrap()
            .set_volume_db(-6.0); // target bus attenuates ~0.5012
        let a_idx = server.mixer_mut().add_bus("A");
        server.mixer_mut().get_bus_mut(a_idx).unwrap().set_send("B"); // full send

        server.play_on_bus(make_dc_buffer(1.0, 44100), "A");
        let output = server.mix(1);

        // A(1.0) --> B(vol 0.5012) --> Master == ~0.5012
        assert!((output[0] - 0.5012).abs() < 0.01, "got {}", output[0]);
    }

    #[test]
    fn server_muted_bus_sends_silence() {
        let mut server = AudioServer::new();
        server.mixer_mut().add_bus("B");
        let a_idx = server.mixer_mut().add_bus("A");
        {
            let a = server.mixer_mut().get_bus_mut(a_idx).unwrap();
            a.set_send("B");
            a.set_mute(true);
        }

        server.play_on_bus(make_dc_buffer(1.0, 44100), "A");
        let output = server.mix(64);

        // A is muted -> nothing is routed to B -> silence at Master.
        assert!(
            output.iter().all(|&s| s.abs() < 1e-6),
            "muted send bus should be silent, got {}",
            output[0]
        );
    }

    #[test]
    fn server_solo_isolates_other_buses() {
        let mut server = AudioServer::new();
        let a_idx = server.mixer_mut().add_bus("A"); // idx 1
        server.mixer_mut().add_bus("B"); // idx 2
        server.mixer_mut().get_bus_mut(a_idx).unwrap().set_solo(true);

        // Both buses send to Master by default.
        server.play_on_bus(make_dc_buffer(0.5, 44100), "A"); // soloed -> heard
        server.play_on_bus(make_dc_buffer(0.4, 44100), "B"); // not soloed -> silenced

        let output = server.mix(1);
        // Only A's 0.5 survives the solo gate.
        assert!((output[0] - 0.5).abs() < 1e-4, "got {}", output[0]);
    }

    #[test]
    fn server_solo_survives_through_send_chain() {
        let mut server = AudioServer::new();
        // B (idx1) -> Master; A (idx2) -> B, soloed.
        server.mixer_mut().add_bus("B");
        let a_idx = server.mixer_mut().add_bus("A");
        {
            let a = server.mixer_mut().get_bus_mut(a_idx).unwrap();
            a.set_send("B");
            a.set_solo(true);
        }
        // A stream is soloed; a Master-direct stream must be silenced by the solo.
        server.play_on_bus(make_dc_buffer(0.5, 44100), "A");
        server.play_on_bus(make_dc_buffer(0.9, 44100), "B"); // B not soloed -> muted at B

        let output = server.mix(1);
        // Only the soloed A path (0.5) reaches Master through B.
        assert!((output[0] - 0.5).abs() < 1e-4, "got {}", output[0]);
    }

    #[test]
    fn server_routing_matrix_combines_paths() {
        let mut server = AudioServer::new();
        server.mixer_mut().add_bus("B"); // idx 1 -> Master
        let a_idx = server.mixer_mut().add_bus("A"); // idx 2 -> B
        server.mixer_mut().get_bus_mut(a_idx).unwrap().set_send("B");

        server.play_on_bus(make_dc_buffer(0.2, 44100), "A"); // A -> B -> Master
        server.play_on_bus(make_dc_buffer(0.3, 44100), "B"); // B -> Master (direct)
        server.play(make_dc_buffer(0.1, 44100)); // Master (direct)

        let output = server.mix(1);
        // B receives 0.3 direct + 0.2 from A = 0.5; Master receives 0.5 + 0.1 = 0.6.
        assert!((output[0] - 0.6).abs() < 1e-4, "got {}", output[0]);
    }
}
