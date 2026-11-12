//! Scene-facing [`AudioStreamPlayer`] that drives an [`AudioServer`].
//!
//! Mirrors the core of Godot's `AudioStreamPlayer` node: assign a stream
//! (decoded [`AudioBuffer`]), set volume/bus/autoplay, then `play()` / `stop()`
//! against the shared audio server. State is kept on this struct so unit tests
//! and scene glue can share one implementation.

use crate::decode::AudioBuffer;
use crate::server::{AudioServer, PlaybackId};
use crate::stream::PlaybackState;

/// Godot-compatible `AudioStreamPlayer` runtime state.
///
/// This is the logical node payload; scene trees store the same fields as
/// `Variant` properties and call into these methods via editor/runtime glue.
#[derive(Debug, Clone)]
pub struct AudioStreamPlayer {
    /// Decoded stream data. `None` means no stream is assigned (play is a no-op).
    stream: Option<AudioBuffer>,
    /// Optional resource path for tooling (`stream` property in scenes).
    stream_path: String,
    /// Volume in decibels (Godot default 0.0).
    volume_db: f32,
    /// Pitch scale multiplier (Godot default 1.0). Pushed to the active stream
    /// playback on [`play`](Self::play); the server's mixer resamples by this
    /// factor so 2.0 doubles the pitch.
    pitch_scale: f32,
    /// When `true`, callers should invoke [`play`](Self::play) on tree enter.
    autoplay: bool,
    /// Mixer bus name (Godot default `"Master"`).
    bus: String,
    /// Active server handle while playing.
    playback_id: Option<PlaybackId>,
    /// Local playing flag used when the server handle is not yet issued.
    wants_playing: bool,
}

impl Default for AudioStreamPlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioStreamPlayer {
    /// Creates a stopped player with Godot defaults.
    pub fn new() -> Self {
        Self {
            stream: None,
            stream_path: String::new(),
            volume_db: 0.0,
            pitch_scale: 1.0,
            autoplay: false,
            bus: "Master".to_string(),
            playback_id: None,
            wants_playing: false,
        }
    }

    /// Assigns decoded sample data as the stream.
    pub fn set_stream(&mut self, buffer: AudioBuffer) {
        self.stream = Some(buffer);
    }

    /// Clears the assigned stream and stops any active playback on `server`.
    pub fn clear_stream(&mut self, server: &mut AudioServer) {
        self.stop(server);
        self.stream = None;
        self.stream_path.clear();
    }

    /// Sets an optional stream resource path (for scene serialization).
    pub fn set_stream_path(&mut self, path: impl Into<String>) {
        self.stream_path = path.into();
    }

    /// Returns the stream resource path, if any.
    pub fn stream_path(&self) -> &str {
        &self.stream_path
    }

    /// Returns `true` when a stream buffer is assigned.
    pub fn has_stream(&self) -> bool {
        self.stream.is_some()
    }

    /// Stream duration in seconds, or `0.0` when no stream is set.
    pub fn stream_length(&self) -> f32 {
        self.stream
            .as_ref()
            .map(|b| b.duration_secs())
            .unwrap_or(0.0)
    }

    /// Sets volume in decibels.
    pub fn set_volume_db(&mut self, db: f32) {
        self.volume_db = db;
    }

    /// Current volume in decibels.
    pub fn volume_db(&self) -> f32 {
        self.volume_db
    }

    /// Sets pitch scale. Applied to the active playback on the next
    /// [`play`](Self::play); clamped to a small positive minimum.
    pub fn set_pitch_scale(&mut self, scale: f32) {
        self.pitch_scale = scale.max(0.01);
    }

    /// Current pitch scale.
    pub fn pitch_scale(&self) -> f32 {
        self.pitch_scale
    }

    /// Sets the autoplay flag.
    pub fn set_autoplay(&mut self, autoplay: bool) {
        self.autoplay = autoplay;
    }

    /// Whether autoplay is enabled.
    pub fn autoplay(&self) -> bool {
        self.autoplay
    }

    /// Sets the target bus name.
    pub fn set_bus(&mut self, bus: impl Into<String>) {
        self.bus = bus.into();
    }

    /// Current bus name.
    pub fn bus(&self) -> &str {
        &self.bus
    }

    /// Starts playback on `server`. No-op when no stream is assigned.
    ///
    /// Returns the new [`PlaybackId`] when a stream starts, or `None` if there
    /// is no stream or the stream is empty.
    pub fn play(&mut self, server: &mut AudioServer) -> Option<PlaybackId> {
        // Stop any previous voice so re-play restarts cleanly (Godot-like).
        if let Some(id) = self.playback_id.take() {
            server.stop(id);
        }

        let Some(buffer) = self.stream.clone() else {
            self.wants_playing = false;
            return None;
        };
        if buffer.frame_count() == 0 {
            self.wants_playing = false;
            return None;
        }

        let id = server.play_on_bus(buffer, &self.bus);
        // Apply volume and pitch to the active stream playback object so the
        // server's mixer resamples at the requested pitch.
        if let Some(stream) = server.stream_mut(id) {
            stream.set_volume_db(self.volume_db);
            stream.set_pitch_scale(self.pitch_scale);
        }
        self.playback_id = Some(id);
        self.wants_playing = true;
        Some(id)
    }

    /// Stops playback and clears the active handle.
    pub fn stop(&mut self, server: &mut AudioServer) {
        if let Some(id) = self.playback_id.take() {
            server.stop(id);
        }
        self.wants_playing = false;
    }

    /// Releases this player's active voice **without stopping it**, handing the
    /// caller its handle.
    ///
    /// 🔑 This exists for `max_polyphony` (pat-mgixk.85). [`play`](Self::play)
    /// opens by stopping `self.playback_id`, which is exactly right for Godot's
    /// monophonic default but wrong the moment one node may hold several voices:
    /// the voice already sounding has to SURVIVE the retrigger. Taking the
    /// handle out first leaves `play()` with `None`, so it starts a second voice
    /// alongside the first instead of replacing it.
    ///
    /// ⚠️ Ownership transfers with the handle. Once detached, this player will
    /// never stop that voice — including from [`stop`](Self::stop), which only
    /// knows about `self.playback_id`. The caller must stop it or let it play to
    /// its natural end, or it leaks for the lifetime of the server.
    pub fn detach_playback(&mut self) -> Option<PlaybackId> {
        self.playback_id.take()
    }

    /// Returns `true` if this player currently has an active playing stream.
    pub fn is_playing(&self, server: &AudioServer) -> bool {
        match self.playback_id {
            Some(id) => server.is_playing(id),
            None => false,
        }
    }

    /// Local "playing" intent flag (also updated by play/stop).
    pub fn playing_flag(&self) -> bool {
        self.wants_playing
    }

    /// Handles autoplay: if `autoplay` is set and a stream exists, starts it.
    ///
    /// Call this when the node enters the scene tree (Godot `_ready` / enter-tree).
    pub fn on_enter_tree(&mut self, server: &mut AudioServer) -> Option<PlaybackId> {
        if self.autoplay && self.stream.is_some() {
            self.play(server)
        } else {
            None
        }
    }

    /// Active playback id, if any.
    pub fn playback_id(&self) -> Option<PlaybackId> {
        self.playback_id
    }

    /// Best-effort playback state for debugging.
    pub fn state(&self, server: &AudioServer) -> PlaybackState {
        if self.is_playing(server) {
            PlaybackState::Playing
        } else {
            PlaybackState::Stopped
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dc_buffer(frames: usize) -> AudioBuffer {
        AudioBuffer {
            samples: vec![0.25; frames],
            sample_rate: 44100,
            channels: 1,
        }
    }

    #[test]
    fn play_stop_is_playing() {
        let mut server = AudioServer::new();
        let mut player = AudioStreamPlayer::new();
        player.set_stream(dc_buffer(44100));
        assert!(!player.is_playing(&server));

        let id = player.play(&mut server);
        assert!(id.is_some());
        assert!(player.is_playing(&server));
        assert!(player.playing_flag());

        player.stop(&mut server);
        assert!(!player.is_playing(&server));
        assert!(!player.playing_flag());
    }

    #[test]
    fn play_without_stream_is_noop() {
        let mut server = AudioServer::new();
        let mut player = AudioStreamPlayer::new();
        assert!(player.play(&mut server).is_none());
        assert!(!player.is_playing(&server));
        assert_eq!(server.active_stream_count(), 0);
    }

    #[test]
    fn autoplay_on_enter_tree() {
        let mut server = AudioServer::new();
        let mut player = AudioStreamPlayer::new();
        player.set_stream(dc_buffer(22050));
        player.set_autoplay(true);
        assert!(player.on_enter_tree(&mut server).is_some());
        assert!(player.is_playing(&server));
    }

    #[test]
    fn autoplay_off_does_not_start() {
        let mut server = AudioServer::new();
        let mut player = AudioStreamPlayer::new();
        player.set_stream(dc_buffer(22050));
        player.set_autoplay(false);
        assert!(player.on_enter_tree(&mut server).is_none());
        assert!(!player.is_playing(&server));
    }

    #[test]
    fn volume_and_bus_defaults() {
        let p = AudioStreamPlayer::new();
        assert_eq!(p.volume_db(), 0.0);
        assert_eq!(p.bus(), "Master");
        assert_eq!(p.pitch_scale(), 1.0);
        assert!(!p.autoplay());
    }

    #[test]
    fn replay_replaces_previous_voice() {
        let mut server = AudioServer::new();
        let mut player = AudioStreamPlayer::new();
        player.set_stream(dc_buffer(44100));
        let first = player.play(&mut server).unwrap();
        let second = player.play(&mut server).unwrap();
        assert_ne!(first, second);
        assert!(!server.is_playing(first));
        assert!(server.is_playing(second));
        assert_eq!(server.active_stream_count(), 1);
    }
}
