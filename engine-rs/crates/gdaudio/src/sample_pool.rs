//! The sample-playback fast path (pat-nzgdu.187).
//!
//! Godot has two ways to make sound. The **stream** path pulls frames from an
//! `AudioStreamPlayback` every mix tick and runs them through the bus graph —
//! flexible, and what [`crate::mixer`] implements. The **sample** path hands a
//! whole decoded buffer to the audio driver once, then just says "play id 7":
//! no per-tick mixing, no bus graph. It exists because on the web a JS round
//! trip per mix tick is ruinous, so the browser plays the buffer itself.
//!
//! **Honest scope — read this before extending.** In Godot the sample path is a
//! *driver* feature, and the base `AudioServer` does not implement it:
//!
//! ```cpp
//! // servers/audio/audio_server.h:143
//! virtual bool is_sample_playback_active(const Ref<AudioSamplePlayback> &) { return false; }
//! ```
//!
//! Every method has an empty/false default, and the **only** real implementation
//! in the whole engine is `platform/web/audio_driver_web.cpp`, which forwards to
//! JS (`godot_audio_sample_is_active`) keyed by the playback's instance id. So
//! "does a sample report playing?" is answered by the browser, not the engine.
//! This module is the engine-side registry that a driver would sit behind: it
//! tracks what is registered and what is playing, and deliberately does **not**
//! pretend to mix — the stream path already does that.
//!
//! Note what GDScript can actually reach: `AudioServer` binds exactly two sample
//! methods, `register_stream_as_sample` and `is_stream_registered_as_sample`
//! (`audio_server.cpp:2097-2098`). Everything else here is engine-internal, and
//! `AudioSample`/`AudioSamplePlayback` are opaque handles with no bound members
//! at all.

use std::collections::HashMap;

use crate::sample::AudioSampleBuffer;

/// How a registered sample repeats, mirroring `AudioSample::LoopMode`
/// (`servers/audio/audio_stream.h:60-65`).
///
/// ⚠️ This is a **C++-only** enum in Godot: it carries no `BIND_ENUM_CONSTANT`,
/// so GDScript cannot see it and it is absent from `extension_api.json`. The
/// discriminants still match Godot's declaration order, because the driver and
/// the engine agree on them across the FFI boundary.
/// ⚠️ Only `Disabled` and `Forward` have distinct behaviour in Godot's own
/// sample driver. Its JS routes `'forward'`/`'backward'` to the same `restart()`
/// and drops `'pingpong'` into `default: // do nothing`
/// (`platform/web/js/libs/library_godot_audio.js:783-793`). The modes below are
/// Godot's declared vocabulary; this pool loops all three non-`Disabled` modes
/// forward, which matches Godot for `Forward`/`Backward` and is a **deviation**
/// for `PingPong` (where Godot does nothing at all).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoopMode {
    /// Play once and stop. Godot's default.
    #[default]
    Disabled = 0,
    /// Repeat.
    Forward = 1,
    /// Declared by Godot as ping-pong; its driver does not implement it.
    PingPong = 2,
    /// Declared by Godot as backward; its driver treats it as `Forward`.
    Backward = 3,
}

/// Identifies a registered sample. Godot keys the driver's pool by the
/// `AudioSample`'s instance id; this is the same idea without the object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SampleId(pub u64);

/// The handle for one *playing* registered sample — Godot's
/// `AudioSamplePlayback`.
///
/// Opaque on purpose: in Godot this is a `RefCounted` with no bound members,
/// handed to the driver and never read by scripts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SamplePlaybackId(pub u64);

/// A decoded buffer registered for sample playback — Godot's `AudioSample`.
#[derive(Debug, Clone)]
pub struct RegisteredSample {
    /// The decoded PCM the driver will play.
    pub buffer: AudioSampleBuffer,
    /// How it repeats.
    pub loop_mode: LoopMode,
    /// First frame of the loop.
    pub loop_begin: usize,
    /// Last frame of the loop; `0` means "to the end".
    ///
    /// ⚠️ The default `0` is Godot's (`audio_stream.h:73`), but the *meaning* is
    /// this pool's. Godot's web driver stores `loopBegin`/`loopEnd` and **never
    /// applies them** — no `loopStart`/`loopEnd` is ever set on the
    /// `AudioBufferSourceNode`, and `forward` simply `restart()`s from 0. So the
    /// loop region is a deliberate deviation, not parity.
    pub loop_end: usize,
}

impl RegisteredSample {
    /// Registers `buffer` as a one-shot sample — Godot's default `LoopMode`.
    pub fn new(buffer: AudioSampleBuffer) -> Self {
        Self {
            buffer,
            loop_mode: LoopMode::Disabled,
            loop_begin: 0,
            loop_end: 0,
        }
    }

    /// Sets the loop mode, returning self so a sample can be built in one line.
    pub fn with_loop(mut self, mode: LoopMode) -> Self {
        self.loop_mode = mode;
        self
    }
}

/// One playing sample's state, as the driver would track it.
#[derive(Debug, Clone, Copy)]
struct ActivePlayback {
    sample: SampleId,
    /// Playhead in frames.
    position: usize,
    paused: bool,
    pitch_scale: f32,
}

/// The engine-side sample pool: register decoded buffers, start them, and ask
/// what is still playing.
///
/// This is the registry a driver sits behind. It answers `is_playing` from real
/// state rather than returning a constant — which is the whole point, since
/// Godot's *base* implementation returns a constant `false` and only the web
/// driver knows the truth.
#[derive(Debug, Default)]
pub struct SamplePool {
    samples: HashMap<SampleId, RegisteredSample>,
    active: HashMap<SamplePlaybackId, ActivePlayback>,
    next_sample: u64,
    next_playback: u64,
}

impl SamplePool {
    /// An empty pool.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a decoded buffer, returning its id — Godot's driver virtual
    /// `AudioServer::register_sample` (`audio_server.h:138`), which is unbound.
    ///
    /// NOT `register_stream_as_sample`: that one takes a `Ref<AudioStream>` and
    /// calls `stream->generate_sample()` to *produce* the sample first
    /// (`audio_server.cpp:1954-1959`). This takes the produced sample.
    pub fn register(&mut self, sample: RegisteredSample) -> SampleId {
        self.next_sample += 1;
        let id = SampleId(self.next_sample);
        self.samples.insert(id, sample);
        id
    }

    /// Whether `id` is registered.
    ///
    /// This has **no direct Godot counterpart**: the scriptable
    /// `is_stream_registered_as_sample` is keyed by the *stream* that produced
    /// the sample (`audio_server.h:135`), and this pool has no stream concept
    /// yet. Wiring streams to samples is follow-up; see the bead notes.
    pub fn is_registered(&self, id: SampleId) -> bool {
        self.samples.contains_key(&id)
    }

    /// Drops a registration. Any playback of it is stopped: a driver cannot keep
    /// playing a buffer that has been freed.
    pub fn unregister(&mut self, id: SampleId) -> bool {
        let existed = self.samples.remove(&id).is_some();
        self.active.retain(|_, p| p.sample != id);
        existed
    }

    /// The registered sample, if any.
    pub fn get(&self, id: SampleId) -> Option<&RegisteredSample> {
        self.samples.get(&id)
    }

    /// Starts `id` playing, returning its playback handle — Godot's
    /// `start_sample_playback`.
    ///
    /// Returns `None` for an unregistered sample rather than minting a handle to
    /// nothing: a handle that reports "playing" for a sample the driver never
    /// received is worse than no handle.
    pub fn start(&mut self, id: SampleId) -> Option<SamplePlaybackId> {
        if !self.samples.contains_key(&id) {
            return None;
        }
        self.next_playback += 1;
        let pb = SamplePlaybackId(self.next_playback);
        self.active.insert(
            pb,
            ActivePlayback {
                sample: id,
                position: 0,
                paused: false,
                pitch_scale: 1.0,
            },
        );
        Some(pb)
    }

    /// Whether `pb` is still playing — Godot's `is_sample_playback_active`.
    ///
    /// A paused playback is still *active*: it has not finished and can resume.
    /// Godot's web driver reports the browser's node state, where a paused
    /// source still exists.
    pub fn is_playing(&self, pb: SamplePlaybackId) -> bool {
        self.active.contains_key(&pb)
    }

    /// Stops and forgets `pb` — Godot's `stop_sample_playback`.
    pub fn stop(&mut self, pb: SamplePlaybackId) -> bool {
        self.active.remove(&pb).is_some()
    }

    /// Pauses/resumes — Godot's `set_sample_playback_pause`.
    pub fn set_paused(&mut self, pb: SamplePlaybackId, paused: bool) -> bool {
        match self.active.get_mut(&pb) {
            Some(p) => {
                p.paused = paused;
                true
            }
            None => false,
        }
    }

    /// Whether `pb` is paused. False for an unknown handle.
    pub fn is_paused(&self, pb: SamplePlaybackId) -> bool {
        self.active.get(&pb).map(|p| p.paused).unwrap_or(false)
    }

    /// Playhead in seconds — Godot's `get_sample_playback_position`.
    pub fn playback_position(&self, pb: SamplePlaybackId) -> f64 {
        let Some(p) = self.active.get(&pb) else {
            return 0.0;
        };
        let Some(s) = self.samples.get(&p.sample) else {
            return 0.0;
        };
        if s.buffer.sample_rate == 0 {
            return 0.0;
        }
        p.position as f64 / s.buffer.sample_rate as f64
    }

    /// Sets playback speed — Godot's `update_sample_playback_pitch_scale`.
    pub fn set_pitch_scale(&mut self, pb: SamplePlaybackId, scale: f32) -> bool {
        match self.active.get_mut(&pb) {
            Some(p) => {
                p.pitch_scale = scale.max(0.0);
                true
            }
            None => false,
        }
    }

    /// How many samples are registered.
    pub fn registered_count(&self) -> usize {
        self.samples.len()
    }

    /// How many playbacks are active.
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    /// Advances every active playback by `frames`, retiring the ones that ran
    /// out. This is what makes `is_playing` mean something over time: a one-shot
    /// must stop reporting itself as playing once it has drained.
    ///
    /// Paused playbacks do not advance — that is the difference between paused
    /// and stopped.
    pub fn advance(&mut self, frames: usize) {
        let mut finished = Vec::new();
        for (&pb, p) in self.active.iter_mut() {
            if p.paused {
                continue;
            }
            let Some(s) = self.samples.get(&p.sample) else {
                finished.push(pb);
                continue;
            };
            let total = s.buffer.frame_count();
            let step = (frames as f32 * p.pitch_scale) as usize;
            p.position += step;

            if p.position < total {
                continue;
            }
            match s.loop_mode {
                // A one-shot that has drained is finished — the driver drops it.
                LoopMode::Disabled => finished.push(pb),
                // Everything else repeats forever; wrap into the loop region.
                _ => {
                    let begin = s.loop_begin.min(total.saturating_sub(1));
                    let end = if s.loop_end == 0 {
                        total
                    } else {
                        s.loop_end.min(total)
                    };
                    let span = end.saturating_sub(begin);
                    p.position = if span == 0 {
                        begin
                    } else {
                        begin + ((p.position - begin) % span)
                    };
                }
            }
        }
        for pb in finished {
            self.active.remove(&pb);
        }
    }
}
