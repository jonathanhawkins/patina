//! Audio server that manages streams, mixing, and output buffer generation.
//!
//! The [`AudioServer`] is the central coordinator: it owns the [`AudioMixer`],
//! manages active [`AudioStreamPlayback`] instances, and produces mixed output
//! buffers each frame.

use crate::bus::AudioBus;
use crate::decode::AudioBuffer;
use crate::mixer::AudioMixer;
use crate::stream::{AudioStreamPlayback, LoopMode, PlaybackState};

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
    /// Playback handles that reached the end of their stream during the most
    /// recent `mix` calls and have not yet been drained. Scene glue drains this
    /// via [`take_finished`](Self::take_finished) to emit Godot's
    /// `AudioStreamPlayer::finished` signal.
    finished: Vec<PlaybackId>,
    /// Per-bus `(left, right)` LINEAR peak levels from the most recent mix,
    /// indexed by bus index — what drives Godot's editor VU meters via
    /// `get_bus_peak_volume_left_db` / `_right_db` (pat-0q2kx.159).
    ///
    /// Held linear rather than in dB so the decay below is a plain multiply;
    /// the getters convert.
    peaks: Vec<(f32, f32)>,
    /// Global multiplier on stream advancement (`AudioServer.playback_speed_scale`).
    /// Scales both the resample step and the time each voice advances, so it
    /// shifts pitch and tempo together the way Godot's does.
    playback_speed_scale: f32,
}

/// Per-mix decay applied to a bus peak that is no longer being driven.
///
/// Godot's meters fall back toward silence instead of latching, so an idle bus
/// visibly releases. This is a per-`mix()` factor, not per-second — the exact
/// slope is cosmetic, but it must be < 1.0 so an idle bus converges on the
/// floor rather than holding its last peak forever.
const PEAK_DECAY: f32 = 0.85;

/// Floor reported for silence, matching Godot's `-200 dB` sentinel.
const PEAK_FLOOR_DB: f32 = -200.0;

impl AudioServer {
    /// Creates a new audio server with default settings (44100 Hz, stereo).
    ///
    /// 44100 is Godot's `audio/driver/mix_rate` default (`audio_server.cpp`);
    /// a project that overrides it should build the server with
    /// [`with_sample_rate`](Self::with_sample_rate).
    pub fn new() -> Self {
        Self::with_sample_rate(44100)
    }

    /// Creates a server running at `sample_rate`, honouring a project's
    /// `audio/driver/mix_rate` (pat-0q2kx.159).
    ///
    /// A zero or absurd rate would make `dt` and the resample step degenerate
    /// (division by zero in `mix`), so it is clamped into a sane audio range
    /// rather than trusted from project text.
    pub fn with_sample_rate(sample_rate: u32) -> Self {
        Self {
            mixer: AudioMixer::new(),
            streams: Vec::new(),
            next_id: 1,
            output_sample_rate: sample_rate.clamp(8_000, 384_000),
            output_channels: 2,
            finished: Vec::new(),
            peaks: Vec::new(),
            playback_speed_scale: 1.0,
        }
    }

    /// Build a server honouring a project's `audio/driver/mix_rate`.
    ///
    /// Godot reads this at startup (`audio_server.cpp`) and defaults it to
    /// 44100. Patina hardcoded 44100 and never consulted the project at all, so
    /// a 48 kHz project mixed every stream at the wrong rate — audibly wrong
    /// pitch, since the resample step is `src_rate / output_rate`
    /// (pat-0q2kx.159).
    ///
    /// A non-numeric or absent value falls back to the default rather than
    /// failing: a malformed setting should not stop audio from starting.
    pub fn from_project_settings(settings: &gdcore::project_settings::ProjectSettings) -> Self {
        let rate = match settings.get("audio/driver/mix_rate") {
            Some(gdcore::project_settings::SettingsValue::Int(v)) if *v > 0 => *v as u32,
            Some(gdcore::project_settings::SettingsValue::Float(v)) if *v > 0.0 => *v as u32,
            _ => 44100,
        };
        Self::with_sample_rate(rate)
    }

    /// Global playback speed multiplier (`AudioServer.playback_speed_scale`).
    pub fn playback_speed_scale(&self) -> f32 {
        self.playback_speed_scale
    }

    /// Set the global playback speed multiplier. Negative values are clamped to
    /// zero — Godot does not play streams backwards through this knob.
    pub fn set_playback_speed_scale(&mut self, scale: f32) {
        self.playback_speed_scale = scale.max(0.0);
    }

    /// Left-channel peak of `bus_idx` for channel pair `channel`, in dB.
    ///
    /// Returns the `-200 dB` floor for silence, an unknown bus, or a channel
    /// pair this output does not have — Godot reports the floor rather than
    /// failing, since the meters poll every frame.
    pub fn get_bus_peak_volume_left_db(&self, bus_idx: usize, channel: usize) -> f32 {
        Self::linear_to_db(self.peak_pair(bus_idx, channel).map(|p| p.0))
    }

    /// Right-channel peak of `bus_idx` for channel pair `channel`, in dB.
    pub fn get_bus_peak_volume_right_db(&self, bus_idx: usize, channel: usize) -> f32 {
        Self::linear_to_db(self.peak_pair(bus_idx, channel).map(|p| p.1))
    }

    /// Raw linear `(left, right)` peak, or `None` when out of range.
    ///
    /// Only channel pair 0 exists while output is stereo; a surround output
    /// would extend this to one pair per two channels.
    fn peak_pair(&self, bus_idx: usize, channel: usize) -> Option<(f32, f32)> {
        if channel >= (self.output_channels as usize).div_ceil(2).max(1) {
            return None;
        }
        self.peaks.get(bus_idx).copied()
    }

    fn linear_to_db(linear: Option<f32>) -> f32 {
        match linear {
            Some(v) if v > 0.0 => (20.0 * v.log10()).max(PEAK_FLOOR_DB),
            _ => PEAK_FLOOR_DB,
        }
    }

    /// Raise `peaks[bus_idx]` to this block's `max(|sample|)` per channel.
    ///
    /// Takes `&mut Vec` rather than `&mut self` so it can be called while a
    /// bus buffer is borrowed out of `bus_bufs` during `mix`.
    ///
    /// This MAXes against the decayed value instead of overwriting it, so a
    /// quiet block cannot yank the meter down faster than the release slope —
    /// which is what makes the reading a peak meter rather than a level meter.
    fn record_peak(peaks: &mut [(f32, f32)], bus_idx: usize, samples: &[f32], out_ch: usize) {
        let Some(slot) = peaks.get_mut(bus_idx) else {
            return;
        };
        let ch = out_ch.max(1);
        let mut left = 0.0f32;
        let mut right = 0.0f32;
        for (i, s) in samples.iter().enumerate() {
            let a = s.abs();
            // Channel 0 is left; channel 1 is right. A mono output mirrors its
            // single channel into both meters so the VU pair is never half-dead.
            if i % ch == 0 {
                left = left.max(a);
                if ch == 1 {
                    right = right.max(a);
                }
            } else if i % ch == 1 {
                right = right.max(a);
            }
        }
        slot.0 = slot.0.max(left);
        slot.1 = slot.1.max(right);
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
    pub fn stream_mut(
        &mut self,
        id: PlaybackId,
    ) -> Option<&mut crate::stream::AudioStreamPlayback> {
        self.streams
            .iter_mut()
            .find(|(pid, _)| *pid == id)
            .map(|(_, s)| &mut s.playback)
    }

    /// Suspends or resumes a live stream **without dropping the voice**
    /// (Godot's `AudioStreamPlayer.stream_paused`, pat-0q2kx.5).
    ///
    /// A paused stream keeps its position and stays in the stream table —
    /// [`mix`](Self::mix) skips non-`Playing` streams, so it contributes
    /// silence, and only `Stopped` streams are evicted. That is what makes
    /// resume-from-position work where `stop`/`play` would restart at zero.
    ///
    /// Returns `false` when `id` is not a live stream. Pausing an already-paused
    /// stream (or resuming a playing one) is a no-op; a `Stopped` stream is not
    /// resurrected by resuming, since its voice is already gone.
    pub fn set_stream_paused(&mut self, id: PlaybackId, paused: bool) -> bool {
        let Some(playback) = self.stream_mut(id) else {
            return false;
        };
        match (paused, playback.state()) {
            (true, PlaybackState::Playing) => playback.pause(),
            (false, PlaybackState::Paused) => playback.play(),
            _ => {}
        }
        true
    }

    /// The playback state of a live stream, or `None` when `id` names no stream
    /// (it finished and was evicted, or was explicitly stopped).
    ///
    /// Distinguishes "paused, still alive" from "gone", which
    /// [`is_playing`](Self::is_playing) cannot — it reports `false` for both.
    pub fn stream_state(&self, id: PlaybackId) -> Option<PlaybackState> {
        self.streams
            .iter()
            .find(|(pid, _)| *pid == id)
            .map(|(_, s)| s.playback.state())
    }

    /// The live playback position of a stream in seconds, or `None` when `id`
    /// names no stream.
    pub fn stream_position(&self, id: PlaybackId) -> Option<f32> {
        self.streams
            .iter()
            .find(|(pid, _)| *pid == id)
            .map(|(_, s)| s.playback.get_playback_position())
    }

    /// Redirects a live stream to a different mixer bus. Used for Godot's
    /// `Area3D`/`Area2D` audio-bus override: when a source node enters an area
    /// with `audio_bus_override` enabled, its voice is rerouted to the area's
    /// bus (e.g. a reverb bus) until it leaves. Returns `false` if `id` is
    /// unknown. The name resolves to `Master` at mix time if the bus is missing.
    pub fn redirect_stream_bus(&mut self, id: PlaybackId, bus: &str) -> bool {
        if let Some(playback) = self.stream_mut(id) {
            playback.set_bus(bus);
            true
        } else {
            false
        }
    }

    /// Drains and returns the playback handles that finished (reached the end of
    /// their stream) since the last drain. Scene glue calls this after each
    /// `mix` to emit `AudioStreamPlayer::finished` for the matching nodes.
    /// Streams stopped explicitly via [`stop`](Self::stop) are not reported.
    pub fn take_finished(&mut self) -> Vec<PlaybackId> {
        std::mem::take(&mut self.finished)
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
        let speed = self.playback_speed_scale;

        let bus_count = self.mixer.bus_count();
        // One accumulation buffer per bus, indexed by bus index.
        let mut bus_bufs: Vec<Vec<f32>> =
            (0..bus_count).map(|_| vec![0.0f32; num_samples]).collect();

        // Meters decay first, then each bus overwrites its entry if it is
        // louder this mix. A bus that goes quiet therefore releases toward the
        // floor instead of latching its last peak (pat-0q2kx.159).
        self.peaks.resize(bus_count, (0.0, 0.0));
        for p in self.peaks.iter_mut() {
            p.0 *= PEAK_DECAY;
            p.1 *= PEAK_DECAY;
        }

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

            // User volume (dB) times any spatial linear gain (distance attenuation).
            let stream_linear =
                AudioBus::db_to_linear(stream.playback.volume_db()) * stream.playback.linear_gain();

            let pos_secs = stream.playback.get_playback_position();
            let src_channels = stream.buffer.channels as usize;
            let src_rate = stream.buffer.sample_rate;

            if src_channels == 0 || src_rate == 0 {
                stream.playback.advance(dt * speed);
                continue;
            }

            // Resample the source into the output stream: each output frame
            // advances the source read cursor by `step` source frames, where
            // `step = (src_rate / output_rate) * pitch_scale`. This corrects the
            // pitch of streams whose sample rate differs from the output rate
            // (a 22050 Hz stream on 44100 Hz output steps 0.5 src frames/out
            // frame) and honors `pitch_scale` (2.0 doubles the step → +1 octave).
            // Fractional cursor positions are linearly interpolated.
            // `playback_speed_scale` multiplies the per-voice pitch, so it
            // shifts every stream's pitch AND tempo together — the same knob
            // Godot uses to fast-forward a whole game's audio.
            let pitch = stream.playback.pitch_scale().max(0.0) * speed;
            let step = (src_rate as f32 / rate as f32) * pitch;
            let src_pos_start = pos_secs * src_rate as f32;
            let frame_total = stream.buffer.frame_count();
            // Equal-power stereo pan gains (default 1.0/1.0 = center) supplied by
            // positional players (AudioStreamPlayer2D/3D) via `set_pan`.
            let (pan_left, pan_right) = stream.playback.pan();
            let buf = &mut bus_bufs[bus_idx];

            // Direction and loop region for the read cursor (pat-0q2kx.58): a
            // `Backward` loop — and the return leg of a `PingPong` — steps the
            // cursor DOWN through the sample, and a looping stream wraps inside
            // its `[loop_begin, loop_end]` window instead of running off the end.
            //
            // KNOWN LIMITATION: the direction is fixed for the whole block, so a
            // ping-pong that reaches its bound mid-block reflects at the next
            // block boundary rather than exactly on the sample. At typical block
            // sizes that is well under a millisecond; fixing it means splitting
            // the block at the reflection point.
            let direction = stream.playback.read_direction();
            let looping = stream.playback.loop_mode() != LoopMode::None;
            let (win_begin_secs, win_end_secs) = stream.playback.loop_window();
            let win0 = (win_begin_secs * src_rate as f32).max(0.0);
            let win1 = (win_end_secs * src_rate as f32).min(frame_total as f32);

            for frame in 0..frame_count {
                let mut src_pos = src_pos_start + frame as f32 * step * direction;
                if looping && win1 > win0 {
                    let span = win1 - win0;
                    // Only wrap once the cursor LEAVES the region. Wrapping
                    // unconditionally would fold the intro — the `[0, loop_begin)`
                    // lead-in Godot plays exactly once — into the loop window, so
                    // a stream with `loop_begin = 50` would jump to frame 50 on
                    // its very first block instead of playing 0..50 first.
                    if direction < 0.0 {
                        if src_pos < win0 {
                            src_pos = win1 - (win0 - src_pos).rem_euclid(span);
                        }
                        // A descending cursor sitting exactly ON `loop_end` must
                        // read the LAST frame of the region, not the frame at
                        // `loop_end` itself (which is one past it, and would
                        // `break` the block as out of range).
                        if src_pos >= win1 {
                            src_pos = win1 - 1.0;
                        }
                    } else if src_pos >= win1 {
                        src_pos = win0 + (src_pos - win0).rem_euclid(span);
                    }
                }
                if src_pos < 0.0 {
                    continue;
                }
                let i0 = src_pos.floor() as usize;
                if i0 >= frame_total {
                    break;
                }
                // Interpolate toward the next source frame (clamped at the end).
                let i1 = (i0 + 1).min(frame_total - 1);
                let frac = src_pos - i0 as f32;
                let dst_offset = frame * out_ch;

                for ch in 0..out_ch {
                    let src_ch = if ch < src_channels { ch } else { 0 };
                    let s0 = stream.buffer.samples[i0 * src_channels + src_ch];
                    let s1 = stream.buffer.samples[i1 * src_channels + src_ch];
                    let sample = s0 + (s1 - s0) * frac;
                    // Left/right pan applies to channels 0/1; extra channels pass through.
                    let pan_gain = match ch {
                        0 => pan_left,
                        1 => pan_right,
                        _ => 1.0,
                    };
                    buf[dst_offset + ch] += sample * stream_linear * pan_gain;
                }
            }

            // Advance source-stream time by the pitched amount so the reported
            // playback position, looping, and end-of-stream detection stay
            // consistent with the pitched read cursor.
            stream.playback.advance(dt * pitch);
        }

        // Any soloed non-master bus makes all other non-master buses inaudible.
        let any_solo =
            (1..bus_count).any(|i| self.mixer.get_bus(i).map(|b| b.is_solo()).unwrap_or(false));

        // 2 + 3. Process non-master buses from highest index down, routing each
        // into its send target so the target is processed with the send applied.
        for i in (1..bus_count).rev() {
            // Volume.
            let vol = self
                .mixer
                .get_bus(i)
                .map(|b| b.volume_linear())
                .unwrap_or(1.0);
            for s in bus_bufs[i].iter_mut() {
                *s *= vol;
            }

            // Effect chain (skipped when the bus bypasses its inserts).
            if let Some(bus) = self.mixer.get_bus_mut(i) {
                if !bus.is_bypass() && !bus.effects().is_empty() {
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
                let target_name = if bus.send().is_empty() {
                    "Master"
                } else {
                    bus.send()
                };
                (self.mixer.get_bus_by_name(target_name), bus.send_linear())
            };

            // Take this bus's processed samples out and route them into the target.
            let src = std::mem::take(&mut bus_bufs[i]);
            // Meter what this bus actually produced — post volume/effects, but
            // BEFORE the send level, since the meter shows the bus's own output
            // rather than its contribution to the target.
            Self::record_peak(&mut self.peaks, i, &src, out_ch);
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
        let master_vol = self
            .mixer
            .get_bus(0)
            .map(|b| b.volume_linear())
            .unwrap_or(1.0);
        for s in bus_bufs[0].iter_mut() {
            *s *= master_vol;
        }
        if let Some(master) = self.mixer.get_bus_mut(0) {
            if !master.is_bypass() && !master.effects().is_empty() {
                master.process_effects(&mut bus_bufs[0], rate, out_ch as u16);
            }
        }
        if self.mixer.get_bus(0).map(|b| b.is_mute()).unwrap_or(false) {
            for s in bus_bufs[0].iter_mut() {
                *s = 0.0;
            }
        }
        // Master is metered after its own gate, so a muted master reads silent.
        Self::record_peak(&mut self.peaks, 0, &bus_bufs[0], out_ch);

        // A stream reaches `Stopped` here only by running past its end (an
        // explicit `stop` removes it immediately). Record those handles as
        // finished so callers can emit the `finished` signal, then drop them.
        let just_finished: Vec<PlaybackId> = self
            .streams
            .iter()
            .filter(|(_, s)| s.playback.state() == PlaybackState::Stopped)
            .map(|(id, _)| *id)
            .collect();
        self.finished.extend(just_finished);
        self.streams
            .retain(|(_, s)| s.playback.state() != PlaybackState::Stopped);

        std::mem::take(&mut bus_bufs[0])
    }

    /// Apply the effect chain of a named bus to an interleaved buffer.
    ///
    /// Used by tests and by pipelines that mix per-bus then combine.
    /// The live effect in slot `effect_idx` of bus `bus_name`
    /// (`AudioServer.get_bus_effect_instance`, pat-0q2kx.160).
    ///
    /// Godot returns an `AudioEffectInstance` — the per-playback companion that
    /// carries the effect's running state. Patina keeps that state on the effect
    /// itself, so the live effect *is* the instance and this hands back a
    /// borrow of it. That is what makes a spectrum analyzer inserted on a bus
    /// queryable: without it the FFT in
    /// [`AudioEffectSpectrumAnalyzer`](crate::effects::AudioEffectSpectrumAnalyzer)
    /// ran on real mixed audio every frame and no caller could ever read it.
    ///
    /// Returns `None` for an unknown bus or an out-of-range slot.
    pub fn get_bus_effect_instance(
        &self,
        bus_name: &str,
        effect_idx: usize,
    ) -> Option<&dyn crate::effects::AudioEffect> {
        let idx = self.mixer.get_bus_by_name(bus_name)?;
        self.mixer.get_bus(idx)?.effects().effect_at(effect_idx)
    }

    /// Peak magnitude in `[from_hz, to_hz]` from the analyzer in slot
    /// `effect_idx` of `bus_name` (pat-0q2kx.160).
    ///
    /// Convenience over [`get_bus_effect_instance`](Self::get_bus_effect_instance)
    /// for callers that cannot hold a borrow across a language boundary — the
    /// script bridge being the motivating one. `None` when the bus or slot does
    /// not exist, or when that slot holds an effect that does not analyse
    /// spectrum.
    pub fn bus_effect_magnitude(
        &self,
        bus_name: &str,
        effect_idx: usize,
        from_hz: f32,
        to_hz: f32,
    ) -> Option<f32> {
        self.get_bus_effect_instance(bus_name, effect_idx)?
            .magnitude_for_frequency_range(from_hz, to_hz)
    }

    pub fn process_bus_effects(&mut self, bus_name: &str, samples: &mut [f32]) -> bool {
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

    /// Mono ramp where sample `i` == `i as f32`, at the given sample rate.
    /// Makes resampling behavior directly observable in the mixed output.
    fn make_ramp_buffer(frames: usize, sample_rate: u32) -> AudioBuffer {
        AudioBuffer {
            samples: (0..frames).map(|i| i as f32).collect(),
            sample_rate,
            channels: 1,
        }
    }

    // pat-nzgdu.26: a project bus layout loads named buses with their volumes,
    // and the runner mix honors per-bus volume, solo, a non-Master effect chain,
    // and the bypass flag. (The script-facing AudioServer singleton dispatch —
    // AudioServer.set_bus_volume_db etc. in the interpreter — is a separate,
    // interpreter-lane concern; this covers the mixer/runtime behavior.)
    #[test]
    fn audio_server_singleton_and_bus_layout() {
        use crate::effects::AudioEffectAmplify;

        // A project.godot [audio_bus_layout]: Master + Music(-10 dB) + SFX(0 dB).
        const LAYOUT: &str = r#"
[audio_bus_layout]
bus/0/name = "Master"
bus/0/volume_db = 0.0
bus/1/name = "Music"
bus/1/volume_db = -10.0
bus/1/send = "Master"
bus/2/name = "SFX"
bus/2/volume_db = 0.0
bus/2/send = "Master"
"#;
        // -10 dB in linear terms (10^(-10/20) ≈ 0.3162).
        let music_lin = 10f32.powf(-10.0 / 20.0);

        // (1) Per-bus volume: a voice on Music (-10 dB) mixes at the reduced
        //     level, NOT unity.
        let mut server = AudioServer::new();
        assert_eq!(server.mixer_mut().load_bus_layout_str(LAYOUT), 3);
        server.play_on_bus(make_dc_buffer(1.0, 44100), "Music");
        let out = server.mix(64);
        assert!(
            (out[0] - music_lin).abs() < 1e-3,
            "Music at -10 dB must mix at {music_lin}, got {}",
            out[0]
        );
        assert!(
            (out[0] - 1.0).abs() > 1e-2,
            "Music must NOT play at unity — -10 dB is a real reduction"
        );

        // (2) Solo: soloing SFX silences the non-soloed Music voice.
        let mut server = AudioServer::new();
        server.mixer_mut().load_bus_layout_str(LAYOUT);
        let sfx_idx = server.mixer().get_bus_by_name("SFX").unwrap();
        server
            .mixer_mut()
            .get_bus_mut(sfx_idx)
            .unwrap()
            .set_solo(true);
        server.play_on_bus(make_dc_buffer(1.0, 44100), "Music"); // not soloed
        let out = server.mix(64);
        assert!(
            out[0].abs() < 1e-4,
            "soloing SFX must silence the Music voice, got {}",
            out[0]
        );

        // (3) Non-Master effect chain: an Amplify(+6 dB ≈ x2) on Music boosts its
        //     voice above the plain -10 dB level (the effect runs after volume).
        let mut server = AudioServer::new();
        server.mixer_mut().load_bus_layout_str(LAYOUT);
        let music_idx = server.mixer().get_bus_by_name("Music").unwrap();
        server
            .mixer_mut()
            .get_bus_mut(music_idx)
            .unwrap()
            .effects_mut()
            .add(Box::new(AudioEffectAmplify::new(6.0)));
        server.play_on_bus(make_dc_buffer(1.0, 44100), "Music");
        let out_fx = server.mix(64)[0];
        assert!(
            out_fx > music_lin * 1.5,
            "non-Master Amplify must boost the Music voice: got {out_fx}, base {music_lin}"
        );

        // (4) Bypass: flagging bypass_fx on the Music bus skips its effect chain,
        //     so the voice falls back to the plain -10 dB level.
        let mut server = AudioServer::new();
        server.mixer_mut().load_bus_layout_str(LAYOUT);
        let music_idx = server.mixer().get_bus_by_name("Music").unwrap();
        {
            let bus = server.mixer_mut().get_bus_mut(music_idx).unwrap();
            bus.effects_mut()
                .add(Box::new(AudioEffectAmplify::new(6.0)));
            bus.set_bypass(true);
        }
        server.play_on_bus(make_dc_buffer(1.0, 44100), "Music");
        let out_bypass = server.mix(64)[0];
        assert!(
            (out_bypass - music_lin).abs() < 1e-3,
            "bypass must skip the effect: expected plain {music_lin}, got {out_bypass}"
        );
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
    fn mix_resamples_low_rate_stream_to_output_rate() {
        // 22050 Hz source on 44100 Hz output: the read cursor steps 0.5 source
        // frames per output frame, so a source ramp emerges at half the slope
        // (correct pitch). The old 1:1 read stepped 1.0 → played 2x too fast.
        let mut server = AudioServer::new();
        assert_eq!(server.output_sample_rate(), 44100);
        server.play(make_ramp_buffer(64, 22050));

        let out = server.mix(8); // stereo interleaved
        for frame in 0..8 {
            let got = out[frame * 2];
            let expected = 0.5 * frame as f32;
            assert!(
                (got - expected).abs() < 1e-4,
                "frame {frame}: got {got}, want {expected}"
            );
        }
    }

    #[test]
    fn mix_pitch_scale_doubles_read_rate() {
        // Same-rate stream, pitch_scale = 2.0 → step 2.0 source frames per
        // output frame → the ramp climbs twice as fast (one octave up).
        let mut server = AudioServer::new();
        let id = server.play(make_ramp_buffer(64, 44100));
        server.stream_mut(id).unwrap().set_pitch_scale(2.0);

        let out = server.mix(8);
        for frame in 0..8 {
            let got = out[frame * 2];
            let expected = 2.0 * frame as f32; // reads source frame 2*frame
            assert!(
                (got - expected).abs() < 1e-4,
                "frame {frame}: got {got}, want {expected}"
            );
        }
    }

    #[test]
    fn mix_resampler_linearly_interpolates() {
        // Two-sample source [0, 10] at half the output rate: output frame 1
        // lands at source position 0.5 and must interpolate to 5.0.
        // Nearest-neighbor resampling would snap it to 0.0 or 10.0.
        let mut server = AudioServer::new();
        server.play(AudioBuffer {
            samples: vec![0.0, 10.0],
            sample_rate: 22050,
            channels: 1,
        });

        let out = server.mix(2);
        assert!((out[0] - 0.0).abs() < 1e-4, "frame0 {}", out[0]);
        assert!((out[2] - 5.0).abs() < 1e-4, "frame1 midpoint {}", out[2]);
    }

    #[test]
    fn player_pushes_pitch_scale_on_play() {
        use crate::player::AudioStreamPlayer;
        let mut server = AudioServer::new();
        let mut player = AudioStreamPlayer::new();
        player.set_stream(make_ramp_buffer(64, 44100));
        player.set_pitch_scale(2.0);
        let id = player.play(&mut server).expect("stream starts");

        // Pitch propagated to the live playback...
        assert_eq!(server.stream_mut(id).unwrap().pitch_scale(), 2.0);
        // ...and the mixer honors it (output frame 1 reads source frame 2).
        let out = server.mix(4);
        assert!((out[2] - 2.0).abs() < 1e-4, "pitched read: {}", out[2]);
    }

    #[test]
    fn mix_applies_3d_attenuation_and_equal_power_pan() {
        use crate::spatial::{AudioListener3D, AudioStreamPlayer3D};
        use gdcore::math::Vector3;
        // Source 30 units to the listener's right (unit_size 10 → atten 0.25).
        let src = AudioStreamPlayer3D::new(Vector3::new(30.0, 0.0, 0.0));
        let res = src.compute_spatial(&AudioListener3D::default());
        assert!(
            res.pan_right > res.pan_left,
            "spatial calc pans right: {res:?}"
        );

        let mut server = AudioServer::new();
        let id = server.play(make_dc_buffer(1.0, 44100));
        {
            let pb = server.stream_mut(id).unwrap();
            pb.set_linear_gain(res.volume_linear);
            pb.set_pan(res.pan_left, res.pan_right);
        }
        let out = server.mix(1);
        // Right louder than left (panned right)...
        assert!(out[1] > out[0], "R {} should exceed L {}", out[1], out[0]);
        // ...distance-attenuated well below unity...
        assert!(out[1] < 0.5, "distance-attenuated: {}", out[1]);
        // ...and full-right pan silences the left channel.
        assert!(
            out[0].abs() < 1e-4,
            "left silent on full-right pan: {}",
            out[0]
        );
    }

    #[test]
    fn mix_applies_2d_pan_from_screen_position() {
        use crate::spatial::AudioStreamPlayer2D;
        use gdcore::math::Vector2;
        // Source at the right edge of the screen (half-width 400).
        let src = AudioStreamPlayer2D::new(Vector2::new(400.0, 0.0));
        let res = src.compute_spatial_2d(Vector2::ZERO, 400.0);

        let mut server = AudioServer::new();
        let id = server.play(make_dc_buffer(1.0, 44100));
        {
            let pb = server.stream_mut(id).unwrap();
            pb.set_linear_gain(res.volume_linear);
            pb.set_pan(res.pan_left, res.pan_right);
        }
        let out = server.mix(1);
        assert!(
            out[1] > out[0],
            "2D source on the right pans right: L{} R{}",
            out[0],
            out[1]
        );
    }

    #[test]
    fn area_bus_redirect_reroutes_live_stream() {
        let mut server = AudioServer::new();
        // A quiet "Reverb" bus to prove the redirect takes effect.
        let reverb_idx = server.mixer_mut().add_bus("Reverb");
        server
            .mixer_mut()
            .get_bus_mut(reverb_idx)
            .unwrap()
            .set_volume_db(-60.0); // ~0.001 linear

        let id = server.play(make_dc_buffer(1.0, 44100)); // starts on Master
        assert!(
            server.mix(1)[0].abs() > 0.5,
            "audible on Master before redirect"
        );

        // Area3D audio_bus_override: reroute the live voice to the Reverb bus.
        assert!(server.redirect_stream_bus(id, "Reverb"));
        let level = server.mix(1)[0].abs();
        assert!(
            level < 0.01,
            "redirected voice routes through attenuated Reverb bus: {level}"
        );

        // Unknown handle is a no-op.
        assert!(!server.redirect_stream_bus(PlaybackId(99999), "Reverb"));
    }

    #[test]
    fn mix_reports_finished_stream() {
        let mut server = AudioServer::new();
        // 100-frame stream (~2.27 ms) so one 200-frame mix runs past its end.
        let id = server.play(make_dc_buffer(1.0, 100));
        assert!(server.take_finished().is_empty(), "nothing finished yet");

        server.mix(200);
        let finished = server.take_finished();
        assert_eq!(finished, vec![id], "the ended stream is reported once");
        assert_eq!(server.active_stream_count(), 0, "ended stream is dropped");
        // Draining is one-shot.
        assert!(server.take_finished().is_empty(), "finished queue drained");
    }

    #[test]
    fn mix_does_not_report_still_playing_stream() {
        let mut server = AudioServer::new();
        // 1-second stream; a 256-frame mix does not reach the end.
        server.play(make_dc_buffer(1.0, 44100));
        server.mix(256);
        assert!(
            server.take_finished().is_empty(),
            "long stream has not finished"
        );
    }

    #[test]
    fn solo_bus_silences_non_solo_buses() {
        let mut server = AudioServer::new();
        server.mixer_mut().add_bus("Music");
        let voice_idx = server.mixer_mut().add_bus("Voice");

        server.play_on_bus(make_dc_buffer(1.0, 44100), "Music");
        // No solo yet: Music is audible.
        assert!(server.mix(1)[0].abs() > 0.5, "Music audible before solo");

        // Solo a *different* bus → the non-solo Music bus is silenced.
        server
            .mixer_mut()
            .get_bus_mut(voice_idx)
            .unwrap()
            .set_solo(true);
        let out = server.mix(1);
        assert!(
            out.iter().all(|&s| s.abs() < 1e-6),
            "non-solo bus must be silenced while another bus is soloed"
        );
    }

    #[test]
    fn non_master_bus_effect_chain_is_processed() {
        use crate::effects::AudioEffectAmplify;
        let mut server = AudioServer::new();
        let fx_idx = server.mixer_mut().add_bus("FX");
        // +6 dB (~2x) amplify on the *non-master* bus.
        server
            .mixer_mut()
            .get_bus_mut(fx_idx)
            .unwrap()
            .effects_mut()
            .add(Box::new(AudioEffectAmplify::new(6.0)));

        server.play_on_bus(make_dc_buffer(0.25, 44100), "FX");
        let out = server.mix(1)[0];
        // 0.25 amplified by ~2x → ~0.5 (proves the non-master effect chain ran).
        assert!((out - 0.499).abs() < 0.02, "amplified output: {out}");
        assert!(out > 0.25, "effect must have boosted above the dry input");
    }

    #[test]
    fn reverb_on_non_master_bus_is_audible() {
        use crate::effects::AudioEffectReverb;
        let mut server = AudioServer::new();
        let rev_idx = server.mixer_mut().add_bus("Reverb");
        server
            .mixer_mut()
            .get_bus_mut(rev_idx)
            .unwrap()
            .effects_mut()
            .add(Box::new(AudioEffectReverb::new()));

        server.play_on_bus(make_dc_buffer(0.5, 44100), "Reverb");
        let out = server.mix(1)[0];
        // Reverb keeps a dry component, so the source stays audible on the bus.
        assert!(
            out.abs() > 0.1,
            "reverb on a non-master bus stays audible: {out}"
        );
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
        server
            .mixer_mut()
            .get_bus_mut(a_idx)
            .unwrap()
            .set_solo(true);

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

    // -- paused-voice controls (pat-0q2kx.5) --------------------------------
    //
    // `stop`/`play` cannot express Godot's `stream_paused`: stopping evicts the
    // voice and resets position to 0, so resuming would restart the sound.
    // These cover the pause path that keeps the voice alive at its position.

    #[test]
    fn paused_voice_stays_alive_and_mixes_silence() {
        let mut server = AudioServer::new();
        let id = server.play(make_dc_buffer(0.5, 44100));
        assert!(
            (server.mix(1)[0] - 0.5).abs() < 1e-4,
            "audible before pause"
        );

        assert!(server.set_stream_paused(id, true), "stream is live");
        assert_eq!(
            server.stream_state(id),
            Some(PlaybackState::Paused),
            "pausing must not evict the voice — only Stopped streams are dropped"
        );
        assert_eq!(
            server.active_stream_count(),
            1,
            "the paused voice is still in the stream table"
        );
        assert_eq!(server.mix(1)[0], 0.0, "a paused voice mixes to silence");
    }

    #[test]
    fn paused_voice_resumes_from_the_same_position() {
        let mut server = AudioServer::new();
        let id = server.play(make_dc_buffer(0.5, 44100));
        server.mix(64);
        let at_pause = server.stream_position(id).expect("live stream");
        assert!(at_pause > 0.0, "position advanced before pausing");

        server.set_stream_paused(id, true);
        server.mix(64);
        assert_eq!(
            server.stream_position(id),
            Some(at_pause),
            "position must freeze while paused, not advance or reset"
        );

        server.set_stream_paused(id, false);
        assert_eq!(server.stream_state(id), Some(PlaybackState::Playing));
        server.mix(64);
        let after = server.stream_position(id).expect("still live");
        assert!(
            after > at_pause,
            "resuming continues from the frozen position ({at_pause}), got {after}"
        );
    }

    #[test]
    fn paused_voice_controls_reject_dead_handles() {
        let mut server = AudioServer::new();
        let id = server.play(make_dc_buffer(0.5, 44100));
        server.stop(id);

        assert!(
            !server.set_stream_paused(id, true),
            "a stopped stream has no voice to pause"
        );
        assert_eq!(server.stream_state(id), None);
        assert_eq!(server.stream_position(id), None);
    }

    #[test]
    fn paused_voice_resume_does_not_resurrect_a_stopped_stream() {
        let mut server = AudioServer::new();
        let id = server.play(make_dc_buffer(0.5, 44100));
        server.set_stream_paused(id, true);
        // Resuming a *paused* stream plays; there is no path that revives a
        // stream the server already evicted.
        server.set_stream_paused(id, false);
        assert_eq!(server.stream_state(id), Some(PlaybackState::Playing));

        server.stop(id);
        assert!(
            !server.set_stream_paused(id, false),
            "evicted stays evicted"
        );
    }
}
