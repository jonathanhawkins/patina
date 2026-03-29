//! Animation editor timeline with keyframe editing.
//!
//! Provides the editor-side state and interaction model for Godot 4's animation
//! editor panel, including:
//!
//! - **Timeline**: playhead, zoom, scroll, visible time range, snap-to-beat grid.
//! - **Track management**: add, remove, reorder, mute, solo tracks.
//! - **Keyframe editing**: select, add, remove, move, copy/paste keyframes.
//! - **Playback controls**: play, pause, stop, loop, speed.

use std::collections::HashSet;

use gdscene::animation::{AnimationTrack, KeyFrame, TrackType, TransitionType};
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// TrackDescriptor — editor-side track metadata
// ---------------------------------------------------------------------------

/// Describes a track to be created in the animation editor.
///
/// This is the editor-side complement to `gdscene::animation::AnimationTrack`.
/// It holds the configuration needed to create and validate a track.
#[derive(Debug, Clone)]
pub struct TrackDescriptor {
    /// The type of track (property, method, audio).
    pub track_type: TrackType,
    /// Node path this track targets (e.g. `"Player"`, `"Player/Sprite2D"`).
    pub node_path: String,
    /// Property or method path (e.g. `"position"`, `"position:x"`, `"play_sound"`).
    pub target_path: String,
}

impl TrackDescriptor {
    /// Creates a property track descriptor.
    pub fn property(node_path: impl Into<String>, property_path: impl Into<String>) -> Self {
        Self {
            track_type: TrackType::Property,
            node_path: node_path.into(),
            target_path: property_path.into(),
        }
    }

    /// Creates a method track descriptor.
    pub fn method(node_path: impl Into<String>, method_name: impl Into<String>) -> Self {
        Self {
            track_type: TrackType::Method,
            node_path: node_path.into(),
            target_path: method_name.into(),
        }
    }

    /// Creates an audio track descriptor.
    pub fn audio(node_path: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            track_type: TrackType::Audio,
            node_path: node_path.into(),
            target_path: label.into(),
        }
    }

    /// Validates the descriptor. Returns an error message if invalid.
    pub fn validate(&self) -> Result<(), TrackCreationError> {
        if self.node_path.is_empty() {
            return Err(TrackCreationError::EmptyNodePath);
        }
        if self.target_path.is_empty() {
            return Err(TrackCreationError::EmptyTargetPath);
        }
        Ok(())
    }

    /// Converts this descriptor to a runtime `AnimationTrack`.
    pub fn to_animation_track(&self) -> AnimationTrack {
        AnimationTrack::with_type(
            &self.node_path,
            &self.target_path,
            self.track_type,
        )
    }

    /// Returns a display label for the track (e.g. `"Player:position"` or `"[Method] Player:play"`).
    pub fn display_label(&self) -> String {
        let prefix = match self.track_type {
            TrackType::Property => "",
            TrackType::Method => "[Method] ",
            TrackType::Audio => "[Audio] ",
        };
        format!("{}{}:{}", prefix, self.node_path, self.target_path)
    }
}

/// Errors that can occur during track creation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackCreationError {
    /// Node path is empty.
    EmptyNodePath,
    /// Target path (property/method/label) is empty.
    EmptyTargetPath,
    /// A track with the same node+property already exists.
    DuplicateTrack {
        existing_index: usize,
    },
}

impl std::fmt::Display for TrackCreationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyNodePath => write!(f, "node path cannot be empty"),
            Self::EmptyTargetPath => write!(f, "target path cannot be empty"),
            Self::DuplicateTrack { existing_index } => {
                write!(f, "duplicate track already exists at index {}", existing_index)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Timeline
// ---------------------------------------------------------------------------

/// Timeline state for the animation editor.
#[derive(Debug, Clone)]
pub struct Timeline {
    /// Current playhead position in seconds.
    pub playhead: f64,
    /// Total animation length in seconds.
    pub length: f64,
    /// Horizontal zoom factor (pixels per second).
    pub zoom: f64,
    /// Horizontal scroll offset in seconds.
    pub scroll_offset: f64,
    /// Whether snap-to-grid is enabled.
    pub snap_enabled: bool,
    /// Snap interval in seconds (e.g. 0.1 for 10fps grid).
    pub snap_interval: f64,
    /// Visible width of the timeline in pixels.
    pub visible_width: f64,
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            playhead: 0.0,
            length: 1.0,
            zoom: 100.0,       // 100 px per second
            scroll_offset: 0.0,
            snap_enabled: true,
            snap_interval: 0.1, // 10 fps grid
            visible_width: 800.0,
        }
    }
}

impl Timeline {
    pub fn new(length: f64) -> Self {
        Self {
            length,
            ..Default::default()
        }
    }

    /// Returns the visible time range `(start, end)` in seconds.
    pub fn visible_range(&self) -> (f64, f64) {
        let duration = self.visible_width / self.zoom;
        (self.scroll_offset, self.scroll_offset + duration)
    }

    /// Returns the visible duration in seconds.
    pub fn visible_duration(&self) -> f64 {
        self.visible_width / self.zoom
    }

    /// Converts a time in seconds to a pixel X position in the timeline.
    pub fn time_to_x(&self, time: f64) -> f64 {
        (time - self.scroll_offset) * self.zoom
    }

    /// Converts a pixel X position to time in seconds.
    pub fn x_to_time(&self, x: f64) -> f64 {
        x / self.zoom + self.scroll_offset
    }

    /// Snaps a time value to the grid if snapping is enabled.
    pub fn snap_time(&self, time: f64) -> f64 {
        if !self.snap_enabled || self.snap_interval <= 0.0 {
            return time;
        }
        (time / self.snap_interval).round() * self.snap_interval
    }

    /// Sets the playhead position, clamped to `[0, length]`.
    pub fn set_playhead(&mut self, time: f64) {
        self.playhead = time.clamp(0.0, self.length);
    }

    /// Sets the playhead with optional snapping.
    pub fn set_playhead_snapped(&mut self, time: f64) {
        let t = self.snap_time(time);
        self.set_playhead(t);
    }

    /// Zooms in by a factor, keeping the given pixel position anchored.
    pub fn zoom_at(&mut self, factor: f64, anchor_x: f64) {
        let time_at_anchor = self.x_to_time(anchor_x);
        self.zoom = (self.zoom * factor).clamp(10.0, 10000.0);
        // Adjust scroll so the time at the anchor stays at the same pixel.
        self.scroll_offset = time_at_anchor - anchor_x / self.zoom;
        self.clamp_scroll();
    }

    /// Scrolls the timeline by a pixel delta.
    pub fn scroll_by(&mut self, dx_pixels: f64) {
        self.scroll_offset -= dx_pixels / self.zoom;
        self.clamp_scroll();
    }

    /// Sets the animation length.
    pub fn set_length(&mut self, length: f64) {
        self.length = length.max(0.01);
        if self.playhead > self.length {
            self.playhead = self.length;
        }
    }

    fn clamp_scroll(&mut self) {
        // Allow scrolling slightly past the end, but not before 0.
        let max_scroll = (self.length - self.visible_duration() * 0.5).max(0.0);
        self.scroll_offset = self.scroll_offset.clamp(-0.5, max_scroll + 0.5);
    }
}

// ---------------------------------------------------------------------------
// KeyframeRef — identifies a specific keyframe
// ---------------------------------------------------------------------------

/// Reference to a specific keyframe: (track index, keyframe index within that track).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyframeRef {
    pub track_index: usize,
    pub keyframe_index: usize,
}

impl KeyframeRef {
    pub fn new(track_index: usize, keyframe_index: usize) -> Self {
        Self {
            track_index,
            keyframe_index,
        }
    }
}

// ---------------------------------------------------------------------------
// KeyframeSelection
// ---------------------------------------------------------------------------

/// Selection state for keyframes in the animation editor.
#[derive(Debug, Clone, Default)]
pub struct KeyframeSelection {
    /// Set of selected keyframe references.
    selected: HashSet<KeyframeRef>,
}

impl KeyframeSelection {
    pub fn new() -> Self {
        Self::default()
    }

    /// Selects a single keyframe, clearing any previous selection.
    pub fn select(&mut self, kf: KeyframeRef) {
        self.selected.clear();
        self.selected.insert(kf);
    }

    /// Adds a keyframe to the selection (Shift+click).
    pub fn add(&mut self, kf: KeyframeRef) {
        self.selected.insert(kf);
    }

    /// Toggles a keyframe's selection state (Ctrl+click).
    pub fn toggle(&mut self, kf: KeyframeRef) {
        if !self.selected.remove(&kf) {
            self.selected.insert(kf);
        }
    }

    /// Selects all keyframes in a time range across all tracks.
    pub fn select_range(
        &mut self,
        track_count: usize,
        keyframe_counts: &[usize],
        keyframe_times: &[Vec<f64>],
        time_start: f64,
        time_end: f64,
    ) {
        self.selected.clear();
        let (t_min, t_max) = if time_start <= time_end {
            (time_start, time_end)
        } else {
            (time_end, time_start)
        };
        for track_idx in 0..track_count.min(keyframe_counts.len()) {
            if let Some(times) = keyframe_times.get(track_idx) {
                for (kf_idx, &time) in times.iter().enumerate() {
                    if time >= t_min && time <= t_max {
                        self.selected.insert(KeyframeRef::new(track_idx, kf_idx));
                    }
                }
            }
        }
    }

    /// Clears the selection.
    pub fn clear(&mut self) {
        self.selected.clear();
    }

    /// Returns the number of selected keyframes.
    pub fn count(&self) -> usize {
        self.selected.len()
    }

    /// Returns true if the given keyframe is selected.
    pub fn is_selected(&self, kf: KeyframeRef) -> bool {
        self.selected.contains(&kf)
    }

    /// Returns all selected keyframe references.
    pub fn selected(&self) -> &HashSet<KeyframeRef> {
        &self.selected
    }

    /// Returns true if nothing is selected.
    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }
}

// ---------------------------------------------------------------------------
// TrackState — per-track editor state
// ---------------------------------------------------------------------------

/// Editor state for a single animation track.
#[derive(Debug, Clone)]
pub struct TrackState {
    /// Track descriptor (type, node path, target path).
    pub descriptor: TrackDescriptor,
    /// Whether the track is muted (excluded from playback preview).
    pub muted: bool,
    /// Whether the track is soloed (only this track plays).
    pub solo: bool,
    /// Whether the track's keyframes are visible in the timeline.
    pub visible: bool,
    /// Whether the track is collapsed in the UI.
    pub collapsed: bool,
    /// Display height of the track row in pixels.
    pub row_height: f64,
}

impl Default for TrackState {
    fn default() -> Self {
        Self {
            descriptor: TrackDescriptor::property(".", "property"),
            muted: false,
            solo: false,
            visible: true,
            collapsed: false,
            row_height: 24.0,
        }
    }
}

impl TrackState {
    /// Creates a TrackState from a descriptor.
    pub fn from_descriptor(desc: TrackDescriptor) -> Self {
        Self {
            descriptor: desc,
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------------------
// PlaybackState
// ---------------------------------------------------------------------------

/// Playback mode for animation preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackMode {
    Stopped,
    Playing,
    Paused,
}

impl Default for PlaybackMode {
    fn default() -> Self {
        Self::Stopped
    }
}

/// Playback controls for the animation editor.
#[derive(Debug, Clone)]
pub struct PlaybackState {
    /// Current playback mode.
    pub mode: PlaybackMode,
    /// Playback speed multiplier (1.0 = normal).
    pub speed: f64,
    /// Whether looping is enabled.
    pub looping: bool,
    /// Whether to play the animation in reverse.
    pub reverse: bool,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            mode: PlaybackMode::Stopped,
            speed: 1.0,
            looping: false,
            reverse: false,
        }
    }
}

impl PlaybackState {
    /// Starts playback (or resumes from pause).
    pub fn play(&mut self) {
        self.mode = PlaybackMode::Playing;
    }

    /// Pauses playback.
    pub fn pause(&mut self) {
        if self.mode == PlaybackMode::Playing {
            self.mode = PlaybackMode::Paused;
        }
    }

    /// Stops playback and resets.
    pub fn stop(&mut self) {
        self.mode = PlaybackMode::Stopped;
    }

    /// Toggles between play and pause.
    pub fn toggle_play_pause(&mut self) {
        match self.mode {
            PlaybackMode::Playing => self.pause(),
            _ => self.play(),
        }
    }

    /// Returns true if currently playing.
    pub fn is_playing(&self) -> bool {
        self.mode == PlaybackMode::Playing
    }

    /// Returns the effective speed (negative if reversed).
    pub fn effective_speed(&self) -> f64 {
        if self.reverse {
            -self.speed
        } else {
            self.speed
        }
    }

    /// Advances the playhead by dt seconds according to playback settings.
    /// Returns the new playhead position and whether the animation wrapped/stopped.
    pub fn advance(&self, current_time: f64, dt: f64, length: f64) -> (f64, bool) {
        if self.mode != PlaybackMode::Playing || length <= 0.0 {
            return (current_time, false);
        }

        let delta = dt * self.effective_speed();
        let mut new_time = current_time + delta;
        let mut wrapped = false;

        if new_time >= length {
            if self.looping {
                new_time = new_time % length;
                wrapped = true;
            } else {
                new_time = length;
                wrapped = true;
            }
        } else if new_time < 0.0 {
            if self.looping {
                new_time = length + (new_time % length);
                if new_time >= length {
                    new_time = 0.0;
                }
                wrapped = true;
            } else {
                new_time = 0.0;
                wrapped = true;
            }
        }

        (new_time, wrapped)
    }
}

// ---------------------------------------------------------------------------
// AnimationEditor — top-level editor state
// ---------------------------------------------------------------------------

/// Top-level animation editor state combining timeline, selection, tracks, and playback.
#[derive(Debug, Clone)]
pub struct AnimationEditor {
    /// Timeline state (playhead, zoom, scroll).
    pub timeline: Timeline,
    /// Keyframe selection state.
    pub selection: KeyframeSelection,
    /// Per-track editor state.
    pub track_states: Vec<TrackState>,
    /// Playback controls.
    pub playback: PlaybackState,
    /// Whether onion-skinning is enabled.
    pub onion_skin: bool,
    /// Number of onion-skin frames before/after.
    pub onion_skin_count: usize,
}

impl AnimationEditor {
    pub fn new(length: f64, track_count: usize) -> Self {
        Self {
            timeline: Timeline::new(length),
            selection: KeyframeSelection::new(),
            track_states: (0..track_count).map(|_| TrackState::default()).collect(),
            playback: PlaybackState::default(),
            onion_skin: false,
            onion_skin_count: 2,
        }
    }

    /// Adds a track with default descriptor and returns its index.
    pub fn add_track(&mut self) -> usize {
        let idx = self.track_states.len();
        self.track_states.push(TrackState::default());
        idx
    }

    /// Creates a track from a descriptor with validation.
    ///
    /// Returns the new track index on success, or an error if the descriptor
    /// is invalid or a duplicate track already exists.
    pub fn create_track(&mut self, desc: TrackDescriptor) -> Result<usize, TrackCreationError> {
        desc.validate()?;

        // Check for duplicates (same node_path + target_path + track_type).
        for (i, state) in self.track_states.iter().enumerate() {
            if state.descriptor.node_path == desc.node_path
                && state.descriptor.target_path == desc.target_path
                && state.descriptor.track_type == desc.track_type
            {
                return Err(TrackCreationError::DuplicateTrack { existing_index: i });
            }
        }

        let idx = self.track_states.len();
        self.track_states.push(TrackState::from_descriptor(desc));
        Ok(idx)
    }

    /// Creates a property track.
    pub fn create_property_track(
        &mut self,
        node_path: impl Into<String>,
        property_path: impl Into<String>,
    ) -> Result<usize, TrackCreationError> {
        self.create_track(TrackDescriptor::property(node_path, property_path))
    }

    /// Creates a method track.
    pub fn create_method_track(
        &mut self,
        node_path: impl Into<String>,
        method_name: impl Into<String>,
    ) -> Result<usize, TrackCreationError> {
        self.create_track(TrackDescriptor::method(node_path, method_name))
    }

    /// Creates an audio track.
    pub fn create_audio_track(
        &mut self,
        node_path: impl Into<String>,
        label: impl Into<String>,
    ) -> Result<usize, TrackCreationError> {
        self.create_track(TrackDescriptor::audio(node_path, label))
    }

    /// Returns the descriptor for a track at the given index.
    pub fn track_descriptor(&self, index: usize) -> Option<&TrackDescriptor> {
        self.track_states.get(index).map(|s| &s.descriptor)
    }

    /// Returns the track type for a track at the given index.
    pub fn track_type(&self, index: usize) -> Option<TrackType> {
        self.track_states.get(index).map(|s| s.descriptor.track_type)
    }

    /// Returns all tracks of a given type.
    pub fn tracks_of_type(&self, track_type: TrackType) -> Vec<usize> {
        self.track_states
            .iter()
            .enumerate()
            .filter(|(_, s)| s.descriptor.track_type == track_type)
            .map(|(i, _)| i)
            .collect()
    }

    /// Converts all tracks to runtime `AnimationTrack` objects.
    pub fn to_animation_tracks(&self) -> Vec<AnimationTrack> {
        self.track_states
            .iter()
            .map(|s| s.descriptor.to_animation_track())
            .collect()
    }

    /// Returns the display label for a track.
    pub fn track_label(&self, index: usize) -> Option<String> {
        self.track_states.get(index).map(|s| s.descriptor.display_label())
    }

    /// Finds a track by node path and target path.
    pub fn find_track(&self, node_path: &str, target_path: &str) -> Option<usize> {
        self.track_states.iter().position(|s| {
            s.descriptor.node_path == node_path && s.descriptor.target_path == target_path
        })
    }

    /// Finds all tracks targeting a specific node.
    pub fn tracks_for_node(&self, node_path: &str) -> Vec<usize> {
        self.track_states
            .iter()
            .enumerate()
            .filter(|(_, s)| s.descriptor.node_path == node_path)
            .map(|(i, _)| i)
            .collect()
    }

    /// Removes a track by index, adjusting selection.
    pub fn remove_track(&mut self, index: usize) -> bool {
        if index >= self.track_states.len() {
            return false;
        }
        self.track_states.remove(index);
        // Remove any selected keyframes on or after this track, re-index.
        let old_selection: Vec<KeyframeRef> = self.selection.selected().iter().copied().collect();
        self.selection.clear();
        for kf in old_selection {
            if kf.track_index < index {
                self.selection.add(kf);
            } else if kf.track_index > index {
                self.selection.add(KeyframeRef::new(kf.track_index - 1, kf.keyframe_index));
            }
            // kf.track_index == index: dropped
        }
        true
    }

    /// Swaps two tracks (for reordering), adjusting selection.
    pub fn swap_tracks(&mut self, a: usize, b: usize) -> bool {
        if a >= self.track_states.len() || b >= self.track_states.len() || a == b {
            return false;
        }
        self.track_states.swap(a, b);
        // Remap selected keyframes.
        let old_selection: Vec<KeyframeRef> = self.selection.selected().iter().copied().collect();
        self.selection.clear();
        for kf in old_selection {
            let new_track = if kf.track_index == a {
                b
            } else if kf.track_index == b {
                a
            } else {
                kf.track_index
            };
            self.selection.add(KeyframeRef::new(new_track, kf.keyframe_index));
        }
        true
    }

    /// Returns the number of tracks.
    pub fn track_count(&self) -> usize {
        self.track_states.len()
    }

    /// Mutes/unmutes a track.
    pub fn set_track_muted(&mut self, index: usize, muted: bool) {
        if let Some(state) = self.track_states.get_mut(index) {
            state.muted = muted;
        }
    }

    /// Toggles solo on a track (un-solos all others).
    pub fn toggle_solo(&mut self, index: usize) {
        if index >= self.track_states.len() {
            return;
        }
        let was_solo = self.track_states[index].solo;
        // Un-solo all first.
        for state in &mut self.track_states {
            state.solo = false;
        }
        if !was_solo {
            self.track_states[index].solo = true;
        }
    }

    /// Returns true if any track is soloed.
    pub fn has_solo(&self) -> bool {
        self.track_states.iter().any(|s| s.solo)
    }

    /// Returns true if a track should produce audio/output (considering mute/solo).
    pub fn is_track_audible(&self, index: usize) -> bool {
        if let Some(state) = self.track_states.get(index) {
            if state.muted {
                return false;
            }
            if self.has_solo() {
                return state.solo;
            }
            true
        } else {
            false
        }
    }

    /// Advances playback by dt seconds and updates the playhead.
    pub fn advance(&mut self, dt: f64) -> bool {
        let (new_time, wrapped) = self.playback.advance(
            self.timeline.playhead,
            dt,
            self.timeline.length,
        );
        self.timeline.playhead = new_time;
        wrapped
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Timeline -----------------------------------------------------------

    #[test]
    fn timeline_default_values() {
        let tl = Timeline::default();
        assert!((tl.playhead - 0.0).abs() < 0.001);
        assert!((tl.length - 1.0).abs() < 0.001);
        assert!(tl.snap_enabled);
    }

    #[test]
    fn timeline_time_to_x_roundtrip() {
        let tl = Timeline::new(2.0);
        let time = 0.75;
        let x = tl.time_to_x(time);
        let back = tl.x_to_time(x);
        assert!((back - time).abs() < 0.001);
    }

    #[test]
    fn timeline_visible_range() {
        let tl = Timeline {
            zoom: 200.0,
            scroll_offset: 0.5,
            visible_width: 400.0,
            ..Default::default()
        };
        let (start, end) = tl.visible_range();
        assert!((start - 0.5).abs() < 0.001);
        assert!((end - 2.5).abs() < 0.001); // 0.5 + 400/200
    }

    #[test]
    fn timeline_snap_time() {
        let tl = Timeline {
            snap_enabled: true,
            snap_interval: 0.1,
            ..Default::default()
        };
        let snapped = tl.snap_time(0.37);
        assert!((snapped - 0.4).abs() < 0.001);
    }

    #[test]
    fn timeline_snap_disabled_passthrough() {
        let tl = Timeline {
            snap_enabled: false,
            ..Default::default()
        };
        let time = 0.37;
        assert!((tl.snap_time(time) - time).abs() < 0.001);
    }

    #[test]
    fn timeline_set_playhead_clamps() {
        let mut tl = Timeline::new(2.0);
        tl.set_playhead(5.0);
        assert!((tl.playhead - 2.0).abs() < 0.001);
        tl.set_playhead(-1.0);
        assert!((tl.playhead - 0.0).abs() < 0.001);
    }

    #[test]
    fn timeline_set_playhead_snapped() {
        let mut tl = Timeline {
            length: 2.0,
            snap_enabled: true,
            snap_interval: 0.25,
            ..Default::default()
        };
        tl.set_playhead_snapped(0.37);
        assert!((tl.playhead - 0.25).abs() < 0.001);
    }

    #[test]
    fn timeline_zoom_at_anchors_point() {
        let mut tl = Timeline {
            zoom: 100.0,
            scroll_offset: 0.0,
            visible_width: 800.0,
            length: 10.0,
            ..Default::default()
        };
        let anchor_x = 400.0;
        let time_before = tl.x_to_time(anchor_x);
        tl.zoom_at(2.0, anchor_x);
        let time_after = tl.x_to_time(anchor_x);
        assert!(
            (time_before - time_after).abs() < 0.01,
            "anchor drift: before={}, after={}",
            time_before,
            time_after
        );
    }

    #[test]
    fn timeline_scroll_by() {
        let mut tl = Timeline {
            zoom: 100.0,
            scroll_offset: 2.0,
            visible_width: 800.0,
            length: 10.0,
            ..Default::default()
        };
        tl.scroll_by(100.0); // scroll left by 100px = 1 second
        assert!((tl.scroll_offset - 1.0).abs() < 0.01);
    }

    #[test]
    fn timeline_set_length_clamps_playhead() {
        let mut tl = Timeline::new(5.0);
        tl.set_playhead(4.0);
        tl.set_length(2.0);
        assert!((tl.playhead - 2.0).abs() < 0.001);
        assert!((tl.length - 2.0).abs() < 0.001);
    }

    // -- KeyframeRef --------------------------------------------------------

    #[test]
    fn keyframe_ref_equality() {
        let a = KeyframeRef::new(0, 1);
        let b = KeyframeRef::new(0, 1);
        let c = KeyframeRef::new(0, 2);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    // -- KeyframeSelection --------------------------------------------------

    #[test]
    fn keyframe_selection_single() {
        let mut sel = KeyframeSelection::new();
        sel.select(KeyframeRef::new(0, 0));
        assert_eq!(sel.count(), 1);
        assert!(sel.is_selected(KeyframeRef::new(0, 0)));
    }

    #[test]
    fn keyframe_selection_replaces() {
        let mut sel = KeyframeSelection::new();
        sel.select(KeyframeRef::new(0, 0));
        sel.select(KeyframeRef::new(1, 0));
        assert_eq!(sel.count(), 1);
        assert!(!sel.is_selected(KeyframeRef::new(0, 0)));
        assert!(sel.is_selected(KeyframeRef::new(1, 0)));
    }

    #[test]
    fn keyframe_selection_add() {
        let mut sel = KeyframeSelection::new();
        sel.select(KeyframeRef::new(0, 0));
        sel.add(KeyframeRef::new(0, 1));
        sel.add(KeyframeRef::new(1, 0));
        assert_eq!(sel.count(), 3);
    }

    #[test]
    fn keyframe_selection_toggle() {
        let mut sel = KeyframeSelection::new();
        sel.select(KeyframeRef::new(0, 0));
        sel.toggle(KeyframeRef::new(0, 0)); // deselect
        assert!(sel.is_empty());
        sel.toggle(KeyframeRef::new(0, 0)); // reselect
        assert_eq!(sel.count(), 1);
    }

    #[test]
    fn keyframe_selection_range() {
        let mut sel = KeyframeSelection::new();
        let times = vec![
            vec![0.0, 0.5, 1.0, 1.5],
            vec![0.3, 0.7, 1.2],
        ];
        let counts = vec![4, 3];
        sel.select_range(2, &counts, &times, 0.4, 1.1);
        // Track 0: kf 1 (0.5) and kf 2 (1.0) are in range
        // Track 1: kf 1 (0.7) is in range
        assert_eq!(sel.count(), 3);
        assert!(sel.is_selected(KeyframeRef::new(0, 1)));
        assert!(sel.is_selected(KeyframeRef::new(0, 2)));
        assert!(sel.is_selected(KeyframeRef::new(1, 1)));
    }

    #[test]
    fn keyframe_selection_clear() {
        let mut sel = KeyframeSelection::new();
        sel.select(KeyframeRef::new(0, 0));
        sel.add(KeyframeRef::new(1, 0));
        sel.clear();
        assert!(sel.is_empty());
    }

    // -- PlaybackState ------------------------------------------------------

    #[test]
    fn playback_default_is_stopped() {
        let ps = PlaybackState::default();
        assert_eq!(ps.mode, PlaybackMode::Stopped);
        assert!(!ps.is_playing());
    }

    #[test]
    fn playback_play_pause_toggle() {
        let mut ps = PlaybackState::default();
        ps.play();
        assert!(ps.is_playing());
        ps.pause();
        assert_eq!(ps.mode, PlaybackMode::Paused);
        ps.toggle_play_pause();
        assert!(ps.is_playing());
        ps.toggle_play_pause();
        assert_eq!(ps.mode, PlaybackMode::Paused);
    }

    #[test]
    fn playback_stop_resets() {
        let mut ps = PlaybackState::default();
        ps.play();
        ps.stop();
        assert_eq!(ps.mode, PlaybackMode::Stopped);
    }

    #[test]
    fn playback_effective_speed_reverse() {
        let ps = PlaybackState {
            speed: 2.0,
            reverse: true,
            ..Default::default()
        };
        assert!((ps.effective_speed() - (-2.0)).abs() < 0.001);
    }

    #[test]
    fn playback_advance_forward() {
        let mut ps = PlaybackState::default();
        ps.play();
        let (new_time, wrapped) = ps.advance(0.0, 0.5, 2.0);
        assert!((new_time - 0.5).abs() < 0.001);
        assert!(!wrapped);
    }

    #[test]
    fn playback_advance_wraps_with_loop() {
        let mut ps = PlaybackState::default();
        ps.play();
        ps.looping = true;
        let (new_time, wrapped) = ps.advance(1.8, 0.5, 2.0);
        assert!((new_time - 0.3).abs() < 0.001);
        assert!(wrapped);
    }

    #[test]
    fn playback_advance_stops_at_end_without_loop() {
        let mut ps = PlaybackState::default();
        ps.play();
        ps.looping = false;
        let (new_time, wrapped) = ps.advance(1.8, 0.5, 2.0);
        assert!((new_time - 2.0).abs() < 0.001);
        assert!(wrapped);
    }

    #[test]
    fn playback_advance_reverse_wraps() {
        let mut ps = PlaybackState::default();
        ps.play();
        ps.reverse = true;
        ps.looping = true;
        let (new_time, wrapped) = ps.advance(0.2, 0.5, 2.0);
        // 0.2 - 0.5 = -0.3 -> wraps to 2.0 + (-0.3 % 2.0) = 1.7
        assert!((new_time - 1.7).abs() < 0.01);
        assert!(wrapped);
    }

    #[test]
    fn playback_advance_does_nothing_when_stopped() {
        let ps = PlaybackState::default();
        let (new_time, wrapped) = ps.advance(0.5, 0.5, 2.0);
        assert!((new_time - 0.5).abs() < 0.001);
        assert!(!wrapped);
    }

    // -- TrackState ---------------------------------------------------------

    #[test]
    fn track_state_defaults() {
        let ts = TrackState::default();
        assert!(!ts.muted);
        assert!(!ts.solo);
        assert!(ts.visible);
        assert!(!ts.collapsed);
        assert!(ts.row_height > 0.0);
    }

    // -- AnimationEditor ----------------------------------------------------

    #[test]
    fn editor_creation() {
        let editor = AnimationEditor::new(2.0, 3);
        assert_eq!(editor.track_count(), 3);
        assert!((editor.timeline.length - 2.0).abs() < 0.001);
        assert!(!editor.playback.is_playing());
    }

    #[test]
    fn editor_add_remove_track() {
        let mut editor = AnimationEditor::new(1.0, 2);
        let idx = editor.add_track();
        assert_eq!(idx, 2);
        assert_eq!(editor.track_count(), 3);
        assert!(editor.remove_track(1));
        assert_eq!(editor.track_count(), 2);
    }

    #[test]
    fn editor_remove_track_adjusts_selection() {
        let mut editor = AnimationEditor::new(1.0, 3);
        editor.selection.select(KeyframeRef::new(2, 0));
        editor.remove_track(1);
        // Track 2 became track 1.
        assert!(!editor.selection.is_selected(KeyframeRef::new(2, 0)));
        assert!(editor.selection.is_selected(KeyframeRef::new(1, 0)));
    }

    #[test]
    fn editor_remove_track_drops_selection_on_removed() {
        let mut editor = AnimationEditor::new(1.0, 3);
        editor.selection.select(KeyframeRef::new(1, 0));
        editor.remove_track(1);
        assert!(editor.selection.is_empty());
    }

    #[test]
    fn editor_swap_tracks() {
        let mut editor = AnimationEditor::new(1.0, 3);
        editor.track_states[0].muted = true;
        editor.selection.select(KeyframeRef::new(0, 0));
        assert!(editor.swap_tracks(0, 2));
        assert!(editor.track_states[2].muted); // mute moved to index 2
        assert!(editor.selection.is_selected(KeyframeRef::new(2, 0))); // selection remapped
    }

    #[test]
    fn editor_mute_solo() {
        let mut editor = AnimationEditor::new(1.0, 3);
        assert!(editor.is_track_audible(0));

        editor.set_track_muted(0, true);
        assert!(!editor.is_track_audible(0));
        assert!(editor.is_track_audible(1));

        editor.set_track_muted(0, false);
        editor.toggle_solo(1);
        assert!(!editor.is_track_audible(0)); // not soloed
        assert!(editor.is_track_audible(1));   // soloed
        assert!(!editor.is_track_audible(2)); // not soloed
    }

    #[test]
    fn editor_toggle_solo_off() {
        let mut editor = AnimationEditor::new(1.0, 2);
        editor.toggle_solo(0);
        assert!(editor.has_solo());
        editor.toggle_solo(0); // un-solo
        assert!(!editor.has_solo());
        assert!(editor.is_track_audible(0));
        assert!(editor.is_track_audible(1));
    }

    #[test]
    fn editor_advance_playback() {
        let mut editor = AnimationEditor::new(2.0, 1);
        editor.playback.play();
        let wrapped = editor.advance(0.5);
        assert!(!wrapped);
        assert!((editor.timeline.playhead - 0.5).abs() < 0.001);
    }

    #[test]
    fn editor_advance_wraps_and_updates_playhead() {
        let mut editor = AnimationEditor::new(2.0, 1);
        editor.playback.play();
        editor.playback.looping = true;
        editor.timeline.set_playhead(1.8);
        let wrapped = editor.advance(0.5);
        assert!(wrapped);
        assert!((editor.timeline.playhead - 0.3).abs() < 0.01);
    }

    #[test]
    fn editor_remove_track_out_of_bounds() {
        let mut editor = AnimationEditor::new(1.0, 2);
        assert!(!editor.remove_track(5));
        assert_eq!(editor.track_count(), 2);
    }

    #[test]
    fn editor_swap_tracks_out_of_bounds() {
        let mut editor = AnimationEditor::new(1.0, 2);
        assert!(!editor.swap_tracks(0, 5));
        assert!(!editor.swap_tracks(0, 0)); // same index
    }

    // -- TrackDescriptor ----------------------------------------------------

    #[test]
    fn track_descriptor_property() {
        let desc = TrackDescriptor::property("Player", "position");
        assert_eq!(desc.track_type, TrackType::Property);
        assert_eq!(desc.node_path, "Player");
        assert_eq!(desc.target_path, "position");
    }

    #[test]
    fn track_descriptor_method() {
        let desc = TrackDescriptor::method("Player", "play_sound");
        assert_eq!(desc.track_type, TrackType::Method);
    }

    #[test]
    fn track_descriptor_audio() {
        let desc = TrackDescriptor::audio("AudioPlayer", "bgm");
        assert_eq!(desc.track_type, TrackType::Audio);
    }

    #[test]
    fn track_descriptor_validate_ok() {
        let desc = TrackDescriptor::property("Player", "position");
        assert!(desc.validate().is_ok());
    }

    #[test]
    fn track_descriptor_validate_empty_node_path() {
        let desc = TrackDescriptor::property("", "position");
        assert_eq!(desc.validate(), Err(TrackCreationError::EmptyNodePath));
    }

    #[test]
    fn track_descriptor_validate_empty_target_path() {
        let desc = TrackDescriptor::property("Player", "");
        assert_eq!(desc.validate(), Err(TrackCreationError::EmptyTargetPath));
    }

    #[test]
    fn track_descriptor_display_label() {
        assert_eq!(
            TrackDescriptor::property("Player", "position").display_label(),
            "Player:position"
        );
        assert_eq!(
            TrackDescriptor::method("Player", "play").display_label(),
            "[Method] Player:play"
        );
        assert_eq!(
            TrackDescriptor::audio("BGM", "track1").display_label(),
            "[Audio] BGM:track1"
        );
    }

    #[test]
    fn track_descriptor_to_animation_track() {
        let desc = TrackDescriptor::property("Player", "position:x");
        let track = desc.to_animation_track();
        assert_eq!(track.node_path, "Player");
        assert_eq!(track.property_path, "position:x");
        assert_eq!(track.track_type(), TrackType::Property);
    }

    // -- Track creation via AnimationEditor ---------------------------------

    #[test]
    fn editor_create_property_track() {
        let mut editor = AnimationEditor::new(2.0, 0);
        let idx = editor.create_property_track("Player", "position").unwrap();
        assert_eq!(idx, 0);
        assert_eq!(editor.track_count(), 1);
        assert_eq!(editor.track_type(0), Some(TrackType::Property));
    }

    #[test]
    fn editor_create_method_track() {
        let mut editor = AnimationEditor::new(2.0, 0);
        let idx = editor.create_method_track("Player", "play_sound").unwrap();
        assert_eq!(idx, 0);
        assert_eq!(editor.track_type(0), Some(TrackType::Method));
    }

    #[test]
    fn editor_create_audio_track() {
        let mut editor = AnimationEditor::new(2.0, 0);
        let idx = editor.create_audio_track("AudioPlayer", "bgm").unwrap();
        assert_eq!(idx, 0);
        assert_eq!(editor.track_type(0), Some(TrackType::Audio));
    }

    #[test]
    fn editor_create_track_rejects_duplicate() {
        let mut editor = AnimationEditor::new(2.0, 0);
        editor.create_property_track("Player", "position").unwrap();
        let result = editor.create_property_track("Player", "position");
        assert_eq!(result, Err(TrackCreationError::DuplicateTrack { existing_index: 0 }));
    }

    #[test]
    fn editor_create_track_allows_same_node_different_property() {
        let mut editor = AnimationEditor::new(2.0, 0);
        editor.create_property_track("Player", "position").unwrap();
        let idx = editor.create_property_track("Player", "rotation").unwrap();
        assert_eq!(idx, 1);
        assert_eq!(editor.track_count(), 2);
    }

    #[test]
    fn editor_create_track_allows_same_path_different_type() {
        let mut editor = AnimationEditor::new(2.0, 0);
        editor.create_property_track("Player", "position").unwrap();
        // Method track with same path is allowed (different type).
        let idx = editor.create_method_track("Player", "position").unwrap();
        assert_eq!(idx, 1);
    }

    #[test]
    fn editor_create_track_validates() {
        let mut editor = AnimationEditor::new(2.0, 0);
        let result = editor.create_property_track("", "position");
        assert_eq!(result, Err(TrackCreationError::EmptyNodePath));
    }

    #[test]
    fn editor_track_label() {
        let mut editor = AnimationEditor::new(2.0, 0);
        editor.create_property_track("Player", "position").unwrap();
        editor.create_method_track("Player", "play").unwrap();
        assert_eq!(editor.track_label(0), Some("Player:position".to_string()));
        assert_eq!(editor.track_label(1), Some("[Method] Player:play".to_string()));
        assert_eq!(editor.track_label(99), None);
    }

    #[test]
    fn editor_find_track() {
        let mut editor = AnimationEditor::new(2.0, 0);
        editor.create_property_track("Player", "position").unwrap();
        editor.create_property_track("Player", "rotation").unwrap();
        assert_eq!(editor.find_track("Player", "rotation"), Some(1));
        assert_eq!(editor.find_track("Player", "scale"), None);
    }

    #[test]
    fn editor_tracks_for_node() {
        let mut editor = AnimationEditor::new(2.0, 0);
        editor.create_property_track("Player", "position").unwrap();
        editor.create_property_track("Enemy", "position").unwrap();
        editor.create_method_track("Player", "play").unwrap();
        let player_tracks = editor.tracks_for_node("Player");
        assert_eq!(player_tracks, vec![0, 2]);
    }

    #[test]
    fn editor_tracks_of_type() {
        let mut editor = AnimationEditor::new(2.0, 0);
        editor.create_property_track("Player", "position").unwrap();
        editor.create_method_track("Player", "play").unwrap();
        editor.create_property_track("Player", "rotation").unwrap();
        editor.create_audio_track("BGM", "music").unwrap();
        assert_eq!(editor.tracks_of_type(TrackType::Property), vec![0, 2]);
        assert_eq!(editor.tracks_of_type(TrackType::Method), vec![1]);
        assert_eq!(editor.tracks_of_type(TrackType::Audio), vec![3]);
    }

    #[test]
    fn editor_to_animation_tracks() {
        let mut editor = AnimationEditor::new(2.0, 0);
        editor.create_property_track("Player", "position").unwrap();
        editor.create_method_track("Player", "play").unwrap();
        let tracks = editor.to_animation_tracks();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].track_type(), TrackType::Property);
        assert_eq!(tracks[0].node_path, "Player");
        assert_eq!(tracks[1].track_type(), TrackType::Method);
    }

    #[test]
    fn editor_remove_typed_track_adjusts_correctly() {
        let mut editor = AnimationEditor::new(2.0, 0);
        editor.create_property_track("Player", "position").unwrap();
        editor.create_method_track("Player", "play").unwrap();
        editor.create_audio_track("BGM", "music").unwrap();
        editor.remove_track(1); // remove method track
        assert_eq!(editor.track_count(), 2);
        assert_eq!(editor.track_type(0), Some(TrackType::Property));
        assert_eq!(editor.track_type(1), Some(TrackType::Audio));
    }

    #[test]
    fn editor_swap_typed_tracks() {
        let mut editor = AnimationEditor::new(2.0, 0);
        editor.create_property_track("Player", "position").unwrap();
        editor.create_audio_track("BGM", "music").unwrap();
        editor.swap_tracks(0, 1);
        assert_eq!(editor.track_type(0), Some(TrackType::Audio));
        assert_eq!(editor.track_type(1), Some(TrackType::Property));
    }
}
