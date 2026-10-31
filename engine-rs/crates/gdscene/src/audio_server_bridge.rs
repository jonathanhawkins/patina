//! Bridges scene-tree `AudioStreamPlayer` nodes to the `gdaudio` runtime
//! (pat-u3l8c.2).
//!
//! Each frame [`AudioServerBridge::process`] scans the tree for
//! `AudioStreamPlayer` nodes, mirrors their properties into `gdaudio`
//! players, honors `autoplay` on first sight and the `playing` property as
//! the script-facing play/stop switch, then mixes `delta` seconds of audio
//! through the [`AudioServer`]. Mixed frames are retained for
//! inspection/tests; attach a platform output via
//! [`attach_output`](AudioServerBridge::attach_output) to hear them on a
//! real device.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use gdaudio::import::AudioStreamLoader;
use gdaudio::sample::AudioOutputStream;
use gdaudio::spatial::{
    doppler_pitch_scale, AttenuationModel, AudioListener3D, AudioStreamPlayer2D as Spatial2D,
    AudioStreamPlayer3D as Spatial3D, DopplerTracking,
};
use gdaudio::{mix_and_write, AudioBuffer, AudioServer, AudioStreamPlayer};
use gdcore::math::{Vector2, Vector3};
use gdvariant::Variant;

use crate::node::NodeId;

/// Prefix of the `__resref__stream` companion when the stream is an inline
/// `AudioStreamGenerator` sub-resource — `"<class>::<sub id>"`.
///
/// ⚠️ The trailing `::` is part of the needle. Without it this also matches
/// `AudioStreamGeneratorPlayback`, a different class.
const GENERATOR_RESREF_PREFIX: &str = "AudioStreamGenerator::";

/// The interpreter's one-shot channel for
/// `AudioStreamGeneratorPlayback.push_buffer()`, drained every frame.
///
/// Modelled on `seek_request`: `gdscene` depends on `gdscript-interop` and not
/// the reverse, so a script cannot call into this bridge directly. A node
/// property is the only direction the dependency graph allows.
const GENERATOR_PUSH_PROP: &str = "__generator_push__";

/// The one-shot channel for `AudioStreamPlayer.play(from)`, drained every
/// frame (pat-mgixk.5).
///
/// 🔑 Distinct from `seek_request` on purpose. A seek moves the playhead of the
/// voice that is *already* live and does nothing when there is none, so it
/// could not express Godot's `play()`, which stops whatever is playing and
/// starts again from `from` (`audio_stream_player_internal.cpp:137-145`,
/// `play_basic`). Writing `playing = true` could not express it either: on an
/// already-playing player that is a no-op edge, which is exactly why the
/// standard SFX retrigger was silent.
pub(crate) const PLAY_REQUEST_PROP: &str = "__play_request__";

/// Godot's `AudioStreamGenerator.buffer_length` default, in seconds
/// (`doc/classes/AudioStreamGenerator.xml`).
const GENERATOR_BUFFER_SECS: f32 = 0.5;
use crate::scene_tree::SceneTree;

/// Half of Godot's default viewport width (1152), used as the screen half-width
/// for 2D panning until the host tells us otherwise via
/// [`set_screen_half_width`](AudioServerBridge::set_screen_half_width). A 2D
/// source this far to the right of the listener pans fully right.
const DEFAULT_SCREEN_HALF_WIDTH: f32 = 576.0;

/// Speed of sound in air, m/s — the constant Godot's Doppler uses
/// (`AudioStreamPlayer3D`; the same 343.0 `gdaudio::spatial`'s own unit tests
/// pass in). Not a project setting in Godot, so it is not one here.
const SPEED_OF_SOUND_M_S: f32 = 343.0;

/// Which flavor of audio player a node is — positional players get a per-frame
/// pan/attenuation pass, plain ones do not (pat-0q2kx.57).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlayerKind {
    /// `AudioStreamPlayer` — non-positional, mixed at its own volume.
    NonPositional,
    /// `AudioStreamPlayer2D` — panned by canvas offset from the 2D listener.
    Positional2D,
    /// `AudioStreamPlayer3D` — panned/attenuated against the 3D listener.
    Positional3D,
}

impl PlayerKind {
    /// Classifies a node's class name, or `None` when it is not a player.
    fn from_class(class_name: &str) -> Option<Self> {
        match class_name {
            "AudioStreamPlayer" => Some(Self::NonPositional),
            "AudioStreamPlayer2D" => Some(Self::Positional2D),
            "AudioStreamPlayer3D" => Some(Self::Positional3D),
            _ => None,
        }
    }
}

/// Runtime audio driver owned by the main loop.
pub struct AudioServerBridge {
    server: AudioServer,
    importer: AudioStreamLoader,
    players: HashMap<NodeId, AudioStreamPlayer>,
    /// Nodes already visited once (autoplay fires on first sight).
    seen: HashSet<NodeId>,
    output: Option<Box<dyn AudioOutputStream>>,
    /// Base directory for resolving `res://` stream paths.
    base_dir: Option<PathBuf>,
    /// Fractional output frames carried between process calls.
    frame_accumulator: f64,
    /// The most recent mixed frames (interleaved f32), kept when no
    /// platform output is attached so tests and tools can inspect output.
    last_mix: Vec<f32>,
    /// Half the viewport width in canvas units, for 2D panning.
    screen_half_width: f32,
    /// pat-s3rrv.55: last frame's world position per 3D source, and the
    /// listener's, so Doppler has a velocity to work from.
    ///
    /// Godot derives Doppler velocity from the change in global transform
    /// between steps rather than from an authored velocity field
    /// (`AudioStreamPlayer3D::_notification` / `NOTIFICATION_INTERNAL_PROCESS`),
    /// so the bridge has to remember where everything was. A source seen for the
    /// first time has no previous position and therefore no velocity — its first
    /// frame is unshifted, which is correct rather than a special case.
    prev_source_pos: HashMap<NodeId, Vector3>,
    /// Last frame's listener position; `None` until the first frame.
    prev_listener_pos: Option<Vector3>,
    /// Streams that named a file the bridge could not load, in the order they
    /// failed. A silent player is indistinguishable from a correct one, so the
    /// failure has to survive past the log line for the runner and tests to
    /// see it (pat-s3rrv.5).
    stream_errors: Vec<String>,
    /// The loop each node's stream declares, resolved once at load and applied
    /// to every voice the node starts (pat-s3rrv.56).
    ///
    /// Kept here rather than on the voice because a node can stop and restart:
    /// the loop is a property of the STREAM, so a replayed clip must loop the
    /// same way without re-reading the file.
    stream_loops: HashMap<NodeId, gdaudio::import::StreamLoop>,
    /// Procedural stream state per generator-backed player (pat-s3rrv.159).
    ///
    /// Kept on the bridge rather than on the node because it holds *queued
    /// audio*, not scene data: it must survive a save/load and must never be
    /// written into a `.tscn`.
    generators: HashMap<NodeId, gdaudio::decode::AudioStreamGenerator>,
    /// Generator-backed players that are playing THIS frame.
    ///
    /// Rebuilt every `sync_player_node` rather than remembered, so a player
    /// that stops simply stops appearing — there is no "stopped" edge to miss.
    generators_active: HashSet<NodeId>,
    /// Which stream each node was last loaded FROM, as a
    /// [`StreamSpec::identity`] key (pat-mgixk.5).
    ///
    /// Loading used to happen only on first sight, so `player.stream =
    /// preload(...)` and an animation audio key switching clips both kept the
    /// buffer the node was born with. Comparing identities rather than the raw
    /// property matters because one stream has three spellings — a path, an
    /// `ExtResource` companion, a `Resource` variant — and a change of spelling
    /// is not a change of stream.
    stream_keys: HashMap<NodeId, String>,
}

impl AudioServerBridge {
    /// Creates a bridge with a fresh [`AudioServer`] and no device output.
    pub fn new() -> Self {
        Self {
            server: AudioServer::new(),
            importer: AudioStreamLoader::new(),
            players: HashMap::new(),
            seen: HashSet::new(),
            output: None,
            base_dir: None,
            frame_accumulator: 0.0,
            last_mix: Vec::new(),
            screen_half_width: DEFAULT_SCREEN_HALF_WIDTH,
            prev_source_pos: HashMap::new(),
            prev_listener_pos: None,
            stream_loops: HashMap::new(),
            stream_errors: Vec::new(),
            generators: HashMap::new(),
            generators_active: HashSet::new(),
            stream_keys: HashMap::new(),
        }
    }

    /// Sets the viewport half-width used for `AudioStreamPlayer2D` panning.
    ///
    /// Godot pans a 2D source by its horizontal offset from the listener
    /// measured against the screen, so the host viewport size is part of the
    /// calculation. Values `<= 0` are ignored rather than collapsing every 2D
    /// source to center.
    pub fn set_screen_half_width(&mut self, half_width: f32) {
        if half_width > 0.0 {
            self.screen_half_width = half_width;
        }
    }

    /// Returns the underlying audio server.
    pub fn server(&self) -> &AudioServer {
        &self.server
    }

    /// Returns the underlying audio server mutably.
    pub fn server_mut(&mut self) -> &mut AudioServer {
        &mut self.server
    }

    /// Attaches a device/output stream; mixed frames are written to it
    /// instead of being retained in [`last_mix`](Self::last_mix).
    pub fn attach_output(&mut self, output: Box<dyn AudioOutputStream>) {
        self.output = Some(output);
    }

    /// Sets the project base directory used to resolve `res://` paths.
    pub fn set_base_dir(&mut self, dir: PathBuf) {
        self.base_dir = Some(dir);
    }

    /// The most recently mixed interleaved frames (empty when a platform
    /// output is attached).
    pub fn last_mix(&self) -> &[f32] {
        &self.last_mix
    }

    /// Directly assigns a decoded stream buffer to the player for
    /// `node_id`, creating the player if needed. Used by tests and by
    /// callers that decode audio themselves.
    /// The live playback rate for a node's voice (pat-s3rrv.55).
    ///
    /// Doppler multiplies this every frame, and a pitch shift is inaudible in a
    /// peak/energy reading of the mix, so there is no way to observe the
    /// feature from `last_mix` alone. Takes `&mut self` because the server only
    /// hands out streams mutably.
    pub fn playback_pitch_scale(&mut self, id: NodeId) -> Option<f32> {
        let playback_id = self.players.get(&id).and_then(|p| p.playback_id())?;
        self.server.stream_mut(playback_id).map(|s| s.pitch_scale())
    }

    /// Reroutes a live voice to `bus` — Godot's `Area3D.audio_bus_override`
    /// (pat-s3rrv.102).
    ///
    /// `AudioServer::redirect_stream_bus` has existed and been tested since the
    /// audio server landed, documented as being "for Godot's Area3D/Area2D
    /// audio-bus override", and had NO CALLER: the effect was implemented and
    /// unreachable. This is the caller.
    ///
    /// Returns `false` when the node has no live voice, which is the ordinary
    /// case for a player that is not currently playing — the per-frame sweep
    /// calls this for every positional player and simply skips the silent ones.
    pub fn redirect_node_bus(&mut self, node_id: NodeId, bus: &str) -> bool {
        let Some(playback_id) = self.players.get(&node_id).and_then(|p| p.playback_id()) else {
            return false;
        };
        self.server.redirect_stream_bus(playback_id, bus)
    }

    /// The bus a node's live voice is routed to, for tests and tooling.
    ///
    /// Takes `&mut self` because the server only hands out streams mutably;
    /// nothing is mutated.
    pub fn node_bus(&mut self, node_id: NodeId) -> Option<String> {
        let playback_id = self.players.get(&node_id).and_then(|p| p.playback_id())?;
        self.server
            .stream_mut(playback_id)
            .map(|s| s.get_bus().to_string())
    }

    /// The nodes this bridge currently holds a player for, positional or not.
    ///
    /// The audio-bus sweep needs the list without borrowing the map, since it
    /// then calls back into `&mut self` to redirect.
    pub fn player_nodes(&self) -> Vec<NodeId> {
        self.players.keys().copied().collect()
    }

    pub fn set_stream_buffer(&mut self, node_id: NodeId, buffer: AudioBuffer) {
        self.players
            .entry(node_id)
            .or_insert_with(AudioStreamPlayer::new)
            .set_stream(buffer);
    }

    /// Per-bus `(left, right)` peak levels in dB, mixer bus order.
    ///
    /// Godot's audio bus panel drives every VU meter from
    /// `AudioServer.get_bus_peak_volume_left_db/right_db` on a timer. Patina
    /// computed the same peaks and exposed them nowhere, so the editor's meters
    /// were derived from the FADER position instead — they moved when you
    /// dragged the slider and did not move when the game got loud, which is the
    /// opposite of what a meter is for (pat-s3rrv.52).
    ///
    /// Returns plain `f32` pairs rather than any `gdaudio` type on purpose: the
    /// editor reads this through `gdscene` and must not need a `gdaudio`
    /// dependency of its own.
    ///
    /// Channel pair 0 (the stereo output) — silence and unknown buses report
    /// the dB floor rather than failing, since meters poll every frame.
    pub fn bus_peaks_db(&self) -> Vec<(f32, f32)> {
        (0..self.server.mixer().bus_count())
            .map(|i| {
                (
                    self.server.get_bus_peak_volume_left_db(i, 0),
                    self.server.get_bus_peak_volume_right_db(i, 0),
                )
            })
            .collect()
    }

    /// Number of currently audible streams.
    pub fn active_stream_count(&self) -> usize {
        self.server.active_stream_count()
    }

    /// Stream files this bridge named but failed to load, oldest first.
    ///
    /// Godot surfaces a load failure; a Patina player that cannot find its
    /// stream just stays silent, which reads exactly like a correct player with
    /// a quiet clip. Keeping the failures lets the runner report them and lets
    /// tests assert the difference (pat-s3rrv.5).
    pub fn stream_errors(&self) -> &[String] {
        &self.stream_errors
    }

    /// Resolves a `res://` (or bare) stream path against the project root.
    ///
    /// Godot resolves `res://` against the directory holding `project.godot`,
    /// never the process CWD (`ProjectSettings::globalize_path`). With no base
    /// dir set this fell back to a CWD-relative path, so a project launched
    /// from anywhere but its own directory loaded no audio at all. The ancestor
    /// walk mirrors the runner's own `resolve_res_path` so loose-file layouts —
    /// a scene in a subdirectory naming assets from the repo root — still
    /// resolve (pat-s3rrv.5).
    fn resolve_stream_path(&self, raw: &str) -> PathBuf {
        let stripped = raw.strip_prefix("res://").unwrap_or(raw);
        let Some(base) = &self.base_dir else {
            let direct = PathBuf::from(stripped);
            return import_artifact_for(&direct).unwrap_or(direct);
        };
        for ancestor in base.ancestors() {
            let candidate = ancestor.join(stripped);
            // ⚠️ The remap is checked BEFORE `candidate.exists()`, and that
            // order is the whole point (pat-s3rrv.47): after an editor import
            // the loose source is routinely gone, leaving only
            // `<source>.import` beside it and the artifact under
            // `.godot/imported/`. Testing the source first would fall straight
            // through to the next ancestor and the player would be silent for a
            // project that is perfectly well formed.
            if let Some(artifact) = import_artifact_for(&candidate) {
                return artifact;
            }
            if candidate.exists() {
                return candidate;
            }
        }
        // Nothing on disk: return the canonical location so the failure names
        // the path the project actually declared.
        base.join(stripped)
    }

    /// Syncs audio player nodes and mixes `delta` seconds of audio.
    ///
    /// Covers all three player classes. `AudioStreamPlayer2D`/`3D` were skipped
    /// entirely before pat-0q2kx.57 — the scan matched the base class name
    /// exactly — so a positional player made no sound at all, however correct
    /// its properties.
    pub fn process(&mut self, tree: &mut SceneTree, delta: f64) {
        let players: Vec<(NodeId, PlayerKind)> = tree
            .all_nodes_in_tree_order()
            .into_iter()
            .filter_map(|id| {
                let kind = tree
                    .get_node(id)
                    .and_then(|n| PlayerKind::from_class(n.class_name()))?;
                Some((id, kind))
            })
            .collect();
        let player_ids: Vec<NodeId> = players.iter().map(|(id, _)| *id).collect();

        for id in &player_ids {
            self.sync_player_node(tree, *id);
        }

        // Positional players re-derive pan/volume from the live transforms every
        // frame, exactly as Godot's `_update_panning` does, and must run before
        // the mix below so this frame's audio carries this frame's position.
        self.apply_spatial(tree, &players, delta);

        // Drop players whose nodes left the tree.
        let live: HashSet<NodeId> = player_ids.iter().copied().collect();
        let stale: Vec<NodeId> = self
            .players
            .keys()
            .filter(|id| !live.contains(id))
            .copied()
            .collect();
        for id in stale {
            if let Some(mut p) = self.players.remove(&id) {
                p.stop(&mut self.server);
            }
            self.seen.remove(&id);
            // Queued audio for a node that left the tree would otherwise be
            // held forever, and replayed if a later node reused the id.
            self.generators.remove(&id);
            self.generators_active.remove(&id);
            // 🔑 The remembered stream identity MUST go with the player
            // (pat-mgixk.5). `players.remove` above leaves a node that re-enters
            // the tree — or any later node reusing the id — with a fresh,
            // empty `AudioStreamPlayer`; a surviving identity would then match
            // the stream it names, `needs_load` would be false, and the player
            // would be silent forever with nothing in the log.
            self.stream_keys.remove(&id);
            // Same reasoning for the loop the departed stream declared: a new
            // one-shot clip on a reused id would inherit it.
            self.stream_loops.remove(&id);
        }

        // Mix this frame's worth of output.
        self.frame_accumulator += delta * f64::from(self.server.output_sample_rate());
        let frames = self.frame_accumulator as usize;
        self.frame_accumulator -= frames as f64;
        // Generated audio has to reach the server BEFORE the mix below, or it
        // lands one frame late — audible as latency that grows with block size.
        for id in &player_ids {
            self.mix_generator_block(*id, frames);
        }

        if frames > 0 {
            if let Some(output) = self.output.as_mut() {
                mix_and_write(&mut self.server, output.as_mut(), frames);
            } else {
                self.last_mix = self.server.mix(frames);
            }
        }

        // Publish each player's live position AFTER mixing, so a script reading
        // `get_playback_position()` next frame sees the position this frame's
        // audio actually reached (Godot `AudioStreamPlayer.get_playback_position`,
        // pat-0q2kx.5). Mixing is what advances the playhead, so publishing
        // before it would report last frame's value.
        for id in &player_ids {
            self.publish_playback_position(tree, *id);
        }
    }

    /// Recomputes pan and distance attenuation for every positional player and
    /// pushes the result onto its live voice (pat-0q2kx.57).
    ///
    /// Godot recomputes this every frame from the node's *global* transform
    /// against the active listener (`AudioStreamPlayer3D::_update_panning`), so
    /// a moving source or a moving listener both take effect immediately. The
    /// math itself already lived in `gdaudio::spatial` and was tested there;
    /// what was missing was any call path from the scene tree to it.
    fn apply_spatial(&mut self, tree: &SceneTree, players: &[(NodeId, PlayerKind)], delta: f64) {
        if !players
            .iter()
            .any(|(_, kind)| *kind != PlayerKind::NonPositional)
        {
            return;
        }
        // Resolved once per frame: every source shares one listener.
        let listener_3d = self.resolve_listener_3d(tree);
        let listener_2d = self.resolve_listener_2d(tree);

        for (id, kind) in players {
            let Some(playback_id) = self.players.get(id).and_then(|p| p.playback_id()) else {
                continue;
            };
            let mut doppler: Option<f32> = None;
            let result = match kind {
                PlayerKind::NonPositional => continue,
                PlayerKind::Positional3D => {
                    let source = self.read_spatial_3d(tree, *id);
                    doppler = self.doppler_for(tree, *id, &source, &listener_3d, delta);
                    source.compute_spatial(&listener_3d)
                }
                PlayerKind::Positional2D => {
                    let source = self.read_spatial_2d(tree, *id);
                    source.compute_spatial_2d(listener_2d, self.screen_half_width)
                }
            };
            if let Some(playback) = self.server.stream_mut(playback_id) {
                playback.set_linear_gain(result.volume_linear);
                playback.set_pan(result.pan_left, result.pan_right);
                // pat-s3rrv.55: `compute_spatial` has always produced this and
                // nothing consumed it, so the emission cone and the distance
                // filter were computed and thrown away — a source outside its
                // cone sounded identical to one on-axis. The mixer now has a
                // per-voice shelf to put it in.
                playback.set_highshelf(result.highshelf_db, result.highshelf_cutoff_hz);
                if let Some(pitch) = doppler {
                    playback.set_pitch_scale(pitch);
                }
            }
        }
        // Remember where everything was, for next frame's velocities. Done after
        // the loop so every source in THIS frame compared against the same
        // listener sample.
        self.prev_listener_pos = Some(listener_3d.position);
    }

    /// The Doppler pitch multiplier for one source this frame, or `None` when
    /// the node has `doppler_tracking` off (pat-s3rrv.55).
    ///
    /// `gdaudio::spatial::doppler_pitch_scale` existed and was unit-tested with
    /// **no callers** — the maths was right and nothing in the scene path asked
    /// for it, so `doppler_tracking` was an authored property that did nothing.
    ///
    /// ⚠️ This MULTIPLIES the node's own `pitch_scale` rather than replacing it.
    /// `sync_player_node` has already pushed the authored pitch onto the voice
    /// by the time `apply_spatial` runs, and `set_pitch_scale` overwrites, so
    /// setting the bare Doppler factor here would silently discard whatever the
    /// game author set.
    fn doppler_for(
        &mut self,
        tree: &SceneTree,
        id: NodeId,
        source: &Spatial3D,
        listener: &AudioListener3D,
        delta: f64,
    ) -> Option<f32> {
        if source.doppler_tracking == DopplerTracking::Disabled {
            // Still record the position: enabling tracking mid-run must not
            // produce one bogus frame from a stale sample.
            self.prev_source_pos.insert(id, source.position);
            return None;
        }
        let prev_source = self.prev_source_pos.insert(id, source.position);
        let dt = delta as f32;
        if dt <= 1e-6 {
            return None;
        }
        // First sight of this source (or of the listener) has no velocity.
        let (Some(prev_source), Some(prev_listener)) = (prev_source, self.prev_listener_pos) else {
            return None;
        };
        let source_velocity = (source.position - prev_source) * (1.0 / dt);
        let listener_velocity = (listener.position - prev_listener) * (1.0 / dt);
        let doppler = doppler_pitch_scale(
            source.position,
            source_velocity,
            listener.position,
            listener_velocity,
            SPEED_OF_SOUND_M_S,
        );
        let authored = tree
            .get_node(id)
            .and_then(|n| number_property(n, "pitch_scale"))
            .unwrap_or(1.0);
        Some((authored * doppler).max(0.01))
    }

    /// The active 3D listener: an `AudioListener3D` with `current` set wins,
    /// then the current `Camera3D`, then the world origin.
    ///
    /// Godot resolves the listener the same way and in the same order
    /// (`AudioStreamPlayer3D::_update_panning` asks the viewport for its audio
    /// listener before falling back to the camera).
    fn resolve_listener_3d(&self, tree: &SceneTree) -> AudioListener3D {
        let listener_node = Self::current_node_of_class(tree, "AudioListener3D")
            .or_else(|| crate::camera3d::current_camera3d(tree));
        let Some(node_id) = listener_node else {
            return AudioListener3D::default();
        };
        let xform = crate::node3d::get_global_transform(tree, node_id);
        // Godot's convention: -Z is forward, +X is right.
        let forward = -xform.basis.z;
        let right = xform.basis.x;
        AudioListener3D {
            position: xform.origin,
            forward: normalized_or(forward, Vector3::new(0.0, 0.0, -1.0)),
            right: normalized_or(right, Vector3::new(1.0, 0.0, 0.0)),
        }
    }

    /// The active 2D listener position: an `AudioListener2D` with `current`
    /// set, then a `Camera2D`, then the canvas origin.
    fn resolve_listener_2d(&self, tree: &SceneTree) -> Vector2 {
        let listener_node = Self::current_node_of_class(tree, "AudioListener2D")
            .or_else(|| Self::current_node_of_class(tree, "Camera2D"))
            .or_else(|| Self::first_node_of_class(tree, "Camera2D"));
        match listener_node {
            Some(id) => crate::node2d::get_global_position(tree, id),
            None => Vector2::ZERO,
        }
    }

    /// First node of `class_name` whose `current` property is true — Godot's
    /// `make_current()`/`clear_current()` write exactly that flag.
    fn current_node_of_class(tree: &SceneTree, class_name: &str) -> Option<NodeId> {
        tree.all_nodes_in_tree_order().into_iter().find(|&id| {
            tree.get_node(id).is_some_and(|n| {
                n.class_name() == class_name
                    && matches!(n.get_property("current"), Variant::Bool(true))
            })
        })
    }

    /// First node of `class_name` regardless of `current` — the fallback Godot
    /// uses when a scene has a camera but nobody called `make_current()`.
    fn first_node_of_class(tree: &SceneTree, class_name: &str) -> Option<NodeId> {
        tree.all_nodes_in_tree_order().into_iter().find(|&id| {
            tree.get_node(id)
                .is_some_and(|n| n.class_name() == class_name)
        })
    }

    /// Mirrors an `AudioStreamPlayer3D` node's spatial properties, using Godot's
    /// defaults for anything the scene did not set.
    fn read_spatial_3d(&self, tree: &SceneTree, id: NodeId) -> Spatial3D {
        let mut source = Spatial3D {
            position: crate::node3d::get_global_transform(tree, id).origin,
            ..Default::default()
        };
        let Some(node) = tree.get_node(id) else {
            return source;
        };
        if let Some(v) = number_property(node, "volume_db") {
            source.volume_db = v;
        }
        if let Some(v) = number_property(node, "unit_size") {
            source.unit_size = v;
        }
        if let Some(v) = number_property(node, "max_distance") {
            source.max_distance = v;
        }
        if let Some(v) = number_property(node, "max_db") {
            source.max_db = v;
        }
        if let Some(v) = number_property(node, "panning_strength") {
            source.panning_strength = v;
        }
        if let Variant::Int(model) = node.get_property("attenuation_model") {
            source.attenuation_model = AttenuationModel::from_godot_int(model);
        }
        // pat-s3rrv.55: the emission cone and attenuation filter. These were
        // struct fields nothing ever wrote, so authoring them in a scene had no
        // effect whatsoever — the classic registered-but-inert shape.
        if let Variant::Bool(on) = node.get_property("emission_angle_enabled") {
            source.emission_angle_enabled = on;
        }
        if let Some(v) = number_property(node, "emission_angle_degrees") {
            source.emission_angle_degrees = v;
        }
        if let Some(v) = number_property(node, "emission_angle_filter_attenuation_db") {
            source.emission_angle_filter_attenuation_db = v;
        }
        if let Some(v) = number_property(node, "attenuation_filter_cutoff_hz") {
            source.attenuation_filter_cutoff_hz = v;
        }
        if let Some(v) = number_property(node, "attenuation_filter_db") {
            source.attenuation_filter_db = v;
        }
        // 🔑 `doppler_tracking` was the LAST unread property, and its absence
        // made the whole Doppler path dead: `Spatial3D::doppler_tracking`
        // defaults to `Disabled`, so `doppler_for` returned on its very first
        // branch every frame and the pitch was never touched. The maths, the
        // previous-position bookkeeping and the `set_pitch_scale` call were all
        // correct and all unreachable — a source authored with
        // `doppler_tracking = 1` behaved exactly like one with it off
        // (pat-s3rrv.55).
        if let Variant::Int(mode) = node.get_property("doppler_tracking") {
            source.doppler_tracking = DopplerTracking::from_godot_int(mode);
        }
        // The cone is measured against the node's FORWARD, which Godot takes as
        // the -Z basis column of the global transform — so a rotated speaker
        // points where the scene says it points rather than always down -Z.
        // ⚠️ `Basis` stores COLUMNS as `x`/`y`/`z`, so the -Z column is
        // `-basis.z` directly. Reading it index-wise as `(-x.z, -y.z, -z.z)`
        // would transpose the matrix and point a rotated source the wrong way.
        let basis = crate::node3d::get_global_transform(tree, id).basis;
        let z = basis.z;
        let forward = gdcore::math::Vector3::new(-z.x, -z.y, -z.z);
        if forward.length() > 1e-6 {
            source.forward = forward;
        }
        source
    }

    /// Mirrors an `AudioStreamPlayer2D` node's spatial properties. Godot's 2D
    /// player spells its falloff exponent `attenuation` (the 3D one uses an
    /// enum model instead).
    fn read_spatial_2d(&self, tree: &SceneTree, id: NodeId) -> Spatial2D {
        let mut source = Spatial2D {
            position: crate::node2d::get_global_position(tree, id),
            ..Default::default()
        };
        let Some(node) = tree.get_node(id) else {
            return source;
        };
        if let Some(v) = number_property(node, "volume_db") {
            source.volume_db = v;
        }
        if let Some(v) = number_property(node, "max_distance") {
            source.max_distance = v;
        }
        if let Some(v) = number_property(node, "attenuation") {
            source.attenuation = v;
        }
        if let Some(v) = number_property(node, "panning_strength") {
            source.panning_strength = v;
        }
        source
    }

    /// Writes the live playback position of `id`'s voice onto its node as
    /// `playback_position` (seconds). A voice that has ended or was never
    /// started reports `0.0`, matching Godot.
    fn publish_playback_position(&mut self, tree: &mut SceneTree, id: NodeId) {
        let pos = self
            .players
            .get(&id)
            .and_then(|p| p.playback_id())
            .and_then(|pid| self.server.stream_position(pid))
            .unwrap_or(0.0);
        if let Some(node) = tree.get_node_mut(id) {
            node.set_property("playback_position", Variant::Float(f64::from(pos)));
        }
    }

    /// Creates this player's generator if it is new, then drains whatever the
    /// script pushed this frame into it.
    ///
    /// ⚠️ The push channel is drained even when the queue is full. Godot's
    /// `push_buffer()` is all-or-nothing and returns `false` when the block
    /// does not fit; leaving the rejected block on the node would replay it
    /// next frame and turn a dropped block into a stuck one.
    fn sync_generator(&mut self, tree: &mut SceneTree, id: NodeId, playing: bool) {
        // ⚠️ Created BEFORE the play-state gate below. Being generator-backed
        // is a property of the STREAM, not of whether the player happens to be
        // running, and `generator_frames_queued()` distinguishes the two by
        // `None` vs `Some(0)` — creating it only on the playing path would make
        // a stopped generator report exactly like an inline `AudioStreamWAV`.
        let rate = self.server.output_sample_rate();
        self.generators.entry(id).or_insert_with(|| {
            // ⚠️ The generator's own `mix_rate` / `buffer_length` live in the
            // sub-resource BODY, which is not on the node (only the
            // `__resref__` companion is), so they are not readable here.
            // Adopting the output rate is the assumption that a script pushes
            // at the rate it is mixed at — the same assumption Godot's default
            // 44100 encodes for a 44100 output — and it keeps the block out of
            // the resampler, where a wrong guess would be an inaudible pitch
            // shift rather than an error.
            gdaudio::decode::AudioStreamGenerator::new(
                rate,
                (GENERATOR_BUFFER_SECS * rate as f32) as usize,
            )
        });

        if playing {
            self.generators_active.insert(id);
        } else {
            self.generators_active.remove(&id);
            // Godot's `get_stream_playback()` yields nothing on a stopped
            // player, so a script cannot push and nothing already queued is
            // heard again on replay. Dropping the queue here is what makes
            // `stop()` silence generated audio at once rather than after the
            // backlog drains.
            if let Some(generator) = self.generators.get_mut(&id) {
                generator.clear_buffer();
            }
            if let Some(node) = tree.get_node_mut(id) {
                node.set_property(GENERATOR_PUSH_PROP, Variant::Nil);
            }
            return;
        }
        let pushed = match tree
            .get_node(id)
            .map(|n| n.get_property(GENERATOR_PUSH_PROP))
        {
            Some(Variant::PackedVector2Array(frames)) if !frames.is_empty() => frames,
            _ => return,
        };

        if let Some(generator) = self.generators.get_mut(&id) {
            // Godot's frames are `Vector2(left, right)`.
            let block: Vec<(f32, f32)> = pushed.iter().map(|v| (v.x, v.y)).collect();
            if !generator.push_buffer(&block) {
                tracing::warn!(
                    frames = block.len(),
                    available = generator.frames_available(),
                    "AudioStreamGeneratorPlayback.push_buffer() dropped a block that \
                     did not fit; the script should check can_push_buffer() first"
                );
            }
        }

        // Drain the one-shot channel so the same block is not pushed again next
        // frame, which would repeat the audio for as long as the script idled.
        if let Some(node) = tree.get_node_mut(id) {
            node.set_property(GENERATOR_PUSH_PROP, Variant::Nil);
        }
    }

    /// Hands this frame's worth of generated audio to the server.
    ///
    /// ⚠️ Each block is played as its own short one-shot rather than appended
    /// to a living voice: an `ActiveStream` owns a fixed `AudioBuffer` and a
    /// playhead, with no append. That is safe here only because the bridge
    /// never reads `server.take_finished()` — were it to start, these
    /// per-frame handles would fire `finished` on the node every frame.
    fn mix_generator_block(&mut self, id: NodeId, frames: usize) {
        if frames == 0 {
            return;
        }
        if !self.generators_active.contains(&id) {
            return;
        }
        let Some(generator) = self.generators.get_mut(&id) else {
            return;
        };
        // An empty queue is an underrun: silence, not an error. Returning early
        // keeps a starved generator from queueing zero-length playbacks.
        if generator.frames_queued() == 0 {
            return;
        }
        let block = generator.mix(frames);
        let bus = self
            .players
            .get(&id)
            .map(|p| p.bus().to_owned())
            .unwrap_or_else(|| "Master".to_owned());
        self.server.play_on_bus(block, &bus);
    }

    /// Frames a generator-backed player has queued but not yet mixed.
    ///
    /// `None` means the node is not backed by a generator at all, which is a
    /// different thing from a generator that has run dry (`Some(0)`).
    pub fn generator_frames_queued(&self, id: NodeId) -> Option<usize> {
        self.generators.get(&id).map(|g| g.frames_queued())
    }

    fn sync_player_node(&mut self, tree: &mut SceneTree, id: NodeId) {
        let (
            stream_spec,
            volume_db,
            autoplay,
            playing_prop,
            bus,
            seek_request,
            play_request,
            paused_prop,
            pitch,
        ) = {
            let Some(node) = tree.get_node(id) else {
                return;
            };
            let as_bool = |key: &str| matches!(node.get_property(key), Variant::Bool(true));
            let as_f32 = |key: &str| match node.get_property(key) {
                Variant::Float(f) => Some(f as f32),
                Variant::Int(i) => Some(i as f32),
                _ => None,
            };
            let volume_db = as_f32("volume_db").unwrap_or(0.0);
            let bus = match node.get_property("bus") {
                Variant::String(s) => s,
                _ => "Master".to_owned(),
            };
            (
                resolve_stream_spec(node),
                volume_db,
                as_bool("autoplay"),
                as_bool("playing"),
                bus,
                // `seek_request` is the interpreter's one-shot channel for
                // `AudioStreamPlayer.seek(pos)`; absent (Nil) means "no seek
                // this frame". Drained below.
                as_f32("seek_request"),
                // `__play_request__` is the separate channel for `play(from)`,
                // which RESTARTS rather than seeks. Drained immediately below.
                as_f32(PLAY_REQUEST_PROP),
                as_bool("stream_paused"),
                // Godot's default pitch_scale is 1.0, not 0.
                as_f32("pitch_scale").unwrap_or(1.0),
            )
        };

        // Drained HERE, before any of the early returns below. A request left
        // on the node is re-read every frame, so a player whose stream failed
        // to load — which returns early — would restart forever.
        //
        // ⚠️ `remove_property`, not `set_property(.., Nil)`. The scene saver's
        // internal-key list is an allowlist, and it formats a stored `Nil` as
        // `null`, so overwriting with Nil would put `__play_request__ = null`
        // into every saved scene holding an audio player. Removing the key
        // reads back as Nil either way.
        if play_request.is_some() {
            if let Some(node) = tree.get_node_mut(id) {
                node.remove_property(PLAY_REQUEST_PROP);
            }
        }

        // Reassigned when a stream swap stops the player below, so the branch
        // table sees the state Godot would leave behind rather than the value
        // the node carried into the frame.
        let mut playing_prop = playing_prop;

        // Godot pauses a player's stream when the tree pauses and the node's
        // effective process mode cannot run (audio_stream_player_internal.cpp:97-131
        // `set_stream_paused(!node->can_process())` on NOTIFICATION_PAUSED).
        let can_process = tree.should_process_node(id);

        let first_sight = self.seen.insert(id);

        // An `AudioStreamGenerator` has no file. The scene carries it as an
        // INLINE sub-resource, so `stream` holds `SubResource("id")` and the
        // class name survives only in the `__resref__stream` companion that
        // `packed_scene.rs` writes for every inline sub-resource.
        //
        // ⚠️ Read the companion, not `stream`. Every inline sub-resource looks
        // identical in `stream` — matching there would treat an inline
        // `AudioStreamWAV` as a generator and silently swallow its audio.
        let is_generator = matches!(
            tree.get_node(id).map(|n| n.get_property("__resref__stream")),
            Some(Variant::String(s)) if s.starts_with(GENERATOR_RESREF_PREFIX)
        );
        if is_generator {
            // `autoplay` counts as playing on first sight, matching the voice
            // path below — otherwise an autoplaying generator would stay mute
            // until a script also set `playing`.
            self.sync_generator(tree, id, playing_prop || (first_sight && autoplay));
        }

        if let StreamSpec::Unresolved(raw) = &stream_spec {
            // The scene named a resource but nothing resolved it to a file or
            // to inline data. Say so — this is precisely the failure that used
            // to present as a player that simply never made a sound
            // (pat-0q2kx.6).
            // ⚠️ Not for a generator: it is fileless BY DESIGN, so this
            // warning would fire every run on a correct scene and train the
            // reader to ignore the one case it exists to catch.
            if first_sight && !is_generator {
                tracing::warn!(
                    node = %tree.get_node(id).map(|n| n.name().to_owned()).unwrap_or_default(),
                    stream = %raw,
                    "AudioStreamPlayer stream is an unresolved resource reference; \
                     the player will be silent"
                );
            }
        }

        // Both computed BEFORE the `&mut self.players` borrow below:
        // `resolve_stream_path` takes `&self`, which cannot coexist with it.
        let stream_key = stream_spec.identity();
        let resolved_path = match &stream_spec {
            StreamSpec::Path(raw) => Some(self.resolve_stream_path(raw)),
            _ => None,
        };
        let prev_stream_key = self.stream_keys.get(&id).cloned();

        let player = self
            .players
            .entry(id)
            .or_insert_with(AudioStreamPlayer::new);
        player.set_bus(bus);
        player.set_volume_db(volume_db);

        // Load the stream whenever the node names a DIFFERENT one than the
        // buffer the player holds — not merely on first sight.
        //
        // 🔑 `player.stream = preload(...)` and an animation audio key
        // switching clips both arrive on a node the bridge has already seen, so
        // a first-sight-only load left the original buffer playing for the rest
        // of the run (pat-mgixk.5).
        let needs_load = !stream_key.is_empty()
            && match &prev_stream_key {
                // Never loaded for this node. A buffer injected straight onto
                // the player (`set_stream_buffer`) still wins over the path the
                // scene names, exactly as before.
                None => !player.has_stream(),
                // Loaded before: only a genuinely different stream reloads.
                Some(prev) => prev != &stream_key,
            };
        // Remembered even when the load is skipped or fails: otherwise an
        // injected buffer (or a bad path) would leave the node with no
        // identity, and the NEXT assignment would look like the first one.
        if !stream_key.is_empty() {
            self.stream_keys.insert(id, stream_key);
        }

        if needs_load {
            // Godot's `set_stream` stops the player before swapping
            // (`audio_stream_player_internal.cpp:254-265`, `stop_callable`), so
            // the old clip never outlives the assignment and the new one does
            // not start by itself. A `play()` in the same frame — including the
            // one an animation audio key writes — restarts it through the
            // play-request arm below.
            //
            // ⚠️ Only on a RELOAD, never on the first load: the first load is
            // what `autoplay` and a scene-authored `playing = true` start from,
            // and stopping there would mute every autoplaying scene.
            if prev_stream_key.is_some() {
                player.stop(&mut self.server);
                if play_request.is_none() {
                    playing_prop = false;
                    if let Some(node) = tree.get_node_mut(id) {
                        node.set_property("playing", Variant::Bool(false));
                    }
                }
            }
            // A failed import used to be discarded here, so a missing or
            // undecodable file was indistinguishable from success: the
            // player just stayed silent. Report it (pat-0q2kx.6).
            let loaded = match (&stream_spec, &resolved_path) {
                (StreamSpec::Path(_), Some(path)) => load_stream_buffer(&mut self.importer, path)
                    .map_err(|err| {
                        format!(
                            "AudioStreamPlayer stream failed to load: {} ({err})",
                            path.display()
                        )
                    }),
                (StreamSpec::Inline(wav), _) => decode_inline_wav(wav).map_err(|err| {
                    format!("AudioStreamPlayer inline stream failed to decode: {err}")
                }),
                // `stream_key` is empty for both other variants, so `needs_load`
                // is false and this is unreachable; reported rather than
                // panicked because an audio frame is the wrong place to abort.
                _ => Err("AudioStreamPlayer stream resolved to nothing".to_owned()),
            };
            match loaded {
                Ok((buffer, loop_points)) => {
                    // ⚠️ `remove` on the `None` arm, not "leave it alone": the
                    // loop belongs to the STREAM, so a stale entry would make a
                    // one-shot clip inherit the previous clip's loop region.
                    match loop_points {
                        Some(points) => {
                            self.stream_loops.insert(id, points);
                        }
                        None => {
                            self.stream_loops.remove(&id);
                        }
                    }
                    player.set_stream(buffer);
                }
                Err(message) => {
                    // `warn!` alone is invisible in a default runner log, so
                    // a missing asset presented as plain silence. Raise it
                    // to `error!` and retain it (pat-s3rrv.5).
                    tracing::error!(
                        error = %message,
                        "AudioStreamPlayer stream failed to load; the player will be silent"
                    );
                    self.stream_errors.push(message);
                }
            }
        }

        // A *paused* voice is still alive — it just mixes to silence. Branching
        // on `is_playing` alone (which is false for both "paused" and "gone")
        // would make a paused stream look like it ran out, stopping it and
        // firing `finished`. Distinguish the two by asking the server for the
        // voice's actual state (pat-0q2kx.5).
        let voice_state = player
            .playback_id()
            .and_then(|pid| self.server.stream_state(pid));
        let voice_alive = voice_state.is_some();

        // Godot's `play(from = 0)` ALWAYS (re)starts: on a player that is
        // already running it stops the live voice and begins again from `from`
        // (`play_basic`, monophonic branch). That is the ordinary SFX
        // retrigger, and the table below had no arm for it — `playing` was
        // already true, so no edge fired and the call did nothing at all
        // (pat-mgixk.5).
        if let Some(from) = play_request {
            if let Some(pid) = player.play(&mut self.server) {
                if let Some(points) = self.stream_loops.get(&id) {
                    apply_stream_loop(&mut self.server, pid, points);
                }
                // `play()` and `play(0)` restart from the top, which
                // `player.play` already did; only a positive `from` needs the
                // playhead moved.
                if from > 0.0 {
                    if let Some(stream) = self.server.stream_mut(pid) {
                        stream.seek(from);
                    }
                }
            }
            // ⚠️ `playing` is deliberately NOT written here. Both writers of
            // the request — the interpreter's `play()` arm and the animation
            // audio key — already set `playing = true`, and mirroring
            // `player.playing_flag()` back onto the node instead would write
            // FALSE for a generator-backed player, whose audio never goes
            // through `player.play()` at all (`mix_generator_block`). That
            // would have silenced `AudioStreamGenerator` the frame a script
            // called `play()` on it.
        } else if first_sight && (autoplay || playing_prop) {
            if let Some(pid) = player.play(&mut self.server) {
                if let Some(points) = self.stream_loops.get(&id) {
                    apply_stream_loop(&mut self.server, pid, points);
                }
                if let Some(node) = tree.get_node_mut(id) {
                    node.set_property("playing", Variant::Bool(true));
                }
            }
        } else if playing_prop && !voice_alive && !player.playing_flag() {
            // Script flipped `playing` on: start the stream. The loop is a
            // property of the STREAM, so a replay loops the same way.
            if let Some(pid) = player.play(&mut self.server) {
                if let Some(points) = self.stream_loops.get(&id) {
                    apply_stream_loop(&mut self.server, pid, points);
                }
            }
        } else if !playing_prop && voice_alive {
            // Script flipped `playing` off: stop the stream, paused or not.
            player.stop(&mut self.server);
        } else if player.playing_flag() && !voice_alive {
            // Stream ran out on its own: reflect that and notify scripts.
            player.stop(&mut self.server);
            if let Some(node) = tree.get_node_mut(id) {
                node.set_property("playing", Variant::Bool(false));
            }
            tree.emit_signal(id, "finished", &[]);
        }

        // -- live-voice sync -------------------------------------------------
        // Everything below drives the voice that is playing *right now*. Before
        // pat-0q2kx.5 these node properties were written by the interpreter and
        // read by nobody, so `seek`, `stream_paused` and `pitch_scale` silently
        // no-opped behind green tests.
        let Some(pid) = player.playback_id() else {
            return;
        };

        // Volume and pitch track the node every frame, not just at `play()`, so
        // assigning `pitch_scale`/`volume_db` mid-playback is audible.
        player.set_pitch_scale(pitch);
        if let Some(stream) = self.server.stream_mut(pid) {
            stream.set_volume_db(volume_db);
            stream.set_pitch_scale(pitch.max(0.01));
            if let Some(pos) = seek_request {
                stream.seek(pos);
            }
        }

        // Drain the one-shot seek channel so the same request is not re-applied
        // every frame (which would pin the playhead in place).
        if seek_request.is_some() {
            if let Some(node) = tree.get_node_mut(id) {
                node.set_property("seek_request", Variant::Nil);
            }
        }

        // Effective pause = the script's `stream_paused` OR the tree pausing a
        // node that cannot process. Pausing preserves position; `playing` stays
        // true so `is_playing()` still reports true, as Godot does.
        self.server
            .set_stream_paused(pid, paused_prop || !can_process);
    }
}

impl Default for AudioServerBridge {
    fn default() -> Self {
        Self::new()
    }
}

/// What a player's `stream` property points at, once the companions a scene
/// load leaves beside it are taken into account (pat-mgixk.5).
#[derive(Debug, Clone, PartialEq)]
enum StreamSpec {
    /// No stream named at all.
    None,
    /// A file to decode, in `res://` or filesystem form.
    Path(String),
    /// PCM the resource carries itself: an inline `AudioStreamWAV`
    /// sub-resource, or a `load()`ed resource with no path of its own.
    Inline(InlineWav),
    /// The scene named a resource and nothing resolved it to either a file or
    /// inline data. Kept distinct from [`None`](Self::None) so the player can
    /// say so rather than being quietly mute.
    Unresolved(String),
}

impl StreamSpec {
    /// A value that changes exactly when the stream the node names changes.
    ///
    /// Reload is driven off this rather than off the raw property because one
    /// stream has several spellings, and a change of spelling is not a change
    /// of stream. Empty means "nothing loadable here", which is also what makes
    /// a reload impossible for the two no-stream variants.
    fn identity(&self) -> String {
        match self {
            Self::None | Self::Unresolved(_) => String::new(),
            Self::Path(p) => format!("path:{p}"),
            Self::Inline(wav) => format!("inline:{:016x}", wav.identity()),
        }
    }
}

/// The PCM payload of an `AudioStreamWAV`, in Godot's own property shape
/// (`doc/classes/AudioStreamWAV.xml`).
///
/// This is the shape `scripting.rs` already builds for
/// `AudioEffectRecord.get_recording()` and the shape `packed_scene.rs` copies
/// onto a node as `__subres__stream__<key>` companions, so one decoder serves
/// an inline sub-resource and a script-assigned resource alike.
#[derive(Debug, Clone, PartialEq)]
struct InlineWav {
    data: Vec<u8>,
    format: i64,
    mix_rate: u32,
    stereo: bool,
    loop_mode: i64,
    loop_begin: i64,
    loop_end: i64,
}

impl InlineWav {
    /// Hash of everything that decides what this stream sounds like.
    ///
    /// Inline PCM has no path to compare, so identity has to come from the
    /// bytes. Hashed rather than cloned because the comparison runs once per
    /// player per frame and the payload is the whole clip.
    fn identity(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.data.hash(&mut hasher);
        self.format.hash(&mut hasher);
        self.mix_rate.hash(&mut hasher);
        self.stereo.hash(&mut hasher);
        self.loop_mode.hash(&mut hasher);
        self.loop_begin.hash(&mut hasher);
        self.loop_end.hash(&mut hasher);
        hasher.finish()
    }
}

/// Reads a player node's `stream` property, plus the companions a scene load
/// writes beside it, into a single [`StreamSpec`].
///
/// ⚠️ `stream` is consulted FIRST, and `__extres__stream` only while `stream`
/// still holds an unresolved reference. The companion is the *resolution of
/// that reference*, so once a script or an animation key has replaced `stream`
/// with a real path or a `Resource` the companion is stale — preferring it
/// unconditionally is what made `player.stream = load(...)` keep playing the
/// clip the scene was authored with (pat-mgixk.5).
fn resolve_stream_spec(node: &crate::node::Node) -> StreamSpec {
    let companion_path = match node.get_property("__extres__stream") {
        Variant::String(s) if !s.is_empty() => Some(s),
        _ => None,
    };
    match node.get_property("stream") {
        // `load()` / `preload()` hand a script a `Resource`, never a string, so
        // this is the shape EVERY script-assigned stream arrives in. Matching
        // only `Variant::String` here is why such an assignment was ignored
        // outright.
        Variant::Resource(res) => {
            if !res.path.is_empty() {
                StreamSpec::Path(res.path.clone())
            } else if let Some(wav) = inline_wav_from_props(|key: &str| {
                res.properties.get(key).cloned().unwrap_or(Variant::Nil)
            }) {
                StreamSpec::Inline(wav)
            } else {
                StreamSpec::Unresolved(format!("{} with neither path nor data", res.class_name))
            }
        }
        // Godot's `stream` in a `.tscn` is `ExtResource("id")` (a file) or
        // `SubResource("id")` (inline data), never a path.
        Variant::String(s) if is_unresolved_resource_ref(&s) => {
            if let Some(path) = companion_path {
                StreamSpec::Path(path)
            } else if let Some(wav) = inline_wav_from_props(|key: &str| {
                node.get_property(&format!("__subres__stream__{key}"))
            }) {
                StreamSpec::Inline(wav)
            } else {
                StreamSpec::Unresolved(s)
            }
        }
        Variant::String(s) if !s.is_empty() => StreamSpec::Path(s),
        // No usable `stream` of its own: a companion is all there is.
        _ => match companion_path {
            Some(path) => StreamSpec::Path(path),
            None => StreamSpec::None,
        },
    }
}

/// Builds an [`InlineWav`] from a property bag in `AudioStreamWAV` shape.
/// `get` answers `Variant::Nil` for a property the bag does not hold.
///
/// `data` is the only required field — without PCM there is nothing to play —
/// and every other default is Godot's own
/// (`doc/classes/AudioStreamWAV.xml`: `format` 0 = 8-bit, `mix_rate` 44100,
/// `stereo` false, the loop fields 0).
///
/// ⚠️ `format` defaults to 0, not 1. `AudioEffectRecord.format` defaults to
/// 16-bit and `AudioStreamWAV.format` to 8-bit, and reading 16-bit data as
/// 8-bit halves the clip's length and turns it into noise.
fn inline_wav_from_props(get: impl Fn(&str) -> Variant) -> Option<InlineWav> {
    let data = match get("data") {
        Variant::PackedByteArray(bytes) if !bytes.is_empty() => bytes,
        _ => return None,
    };
    let as_int = |value: Variant, default: i64| match value {
        Variant::Int(i) => i,
        Variant::Float(f) => f as i64,
        _ => default,
    };
    Some(InlineWav {
        data,
        format: as_int(get("format"), 0),
        mix_rate: as_int(get("mix_rate"), 44_100).clamp(1, i64::from(u32::MAX)) as u32,
        stereo: matches!(get("stereo"), Variant::Bool(true)),
        loop_mode: as_int(get("loop_mode"), 0),
        loop_begin: as_int(get("loop_begin"), 0),
        loop_end: as_int(get("loop_end"), 0),
    })
}

/// Decodes an inline `AudioStreamWAV`'s PCM into a playable buffer.
///
/// 🔑 8-bit `AudioStreamWAV` is **signed** — the raw two's-complement byte, not
/// the unsigned 0..255 a RIFF/WAV file stores. This is the exact inverse of
/// `AudioEffectRecord`'s encoder (`gdaudio::effects::AudioEffectRecord::get_recording`),
/// and reading it as unsigned offsets the whole clip by a half-scale DC step.
fn decode_inline_wav(wav: &InlineWav) -> Result<LoadedStream, String> {
    let samples: Vec<f32> = match wav.format {
        // FORMAT_8_BITS
        0 => wav
            .data
            .iter()
            .map(|b| f32::from(*b as i8) / 128.0)
            .collect(),
        // FORMAT_16_BITS, little-endian
        1 => wav
            .data
            .chunks_exact(2)
            .map(|c| f32::from(i16::from_le_bytes([c[0], c[1]])) / 32768.0)
            .collect(),
        // FORMAT_IMA_ADPCM / FORMAT_QOA. Reported rather than decoded as PCM:
        // a lossily compressed payload read as raw samples is loud noise, which
        // is worse than silence with a line in the log.
        other => {
            return Err(format!(
                "format {other} is not decodable yet (only 8-bit and 16-bit PCM are)"
            ))
        }
    };
    if samples.is_empty() {
        return Err("the resource carries no samples".to_owned());
    }
    let buffer = AudioBuffer {
        samples,
        sample_rate: wav.mix_rate,
        channels: if wav.stereo { 2 } else { 1 },
    };
    // ⚠️ `loop_begin`/`loop_end` are FRAMES on the resource and SECONDS on the
    // playback API — the same conversion `gdaudio::import` does for a file's
    // `smpl` chunk. Handing frames straight through would put the loop region
    // tens of thousands of seconds out.
    let loop_points = match wav.loop_mode {
        1 => Some(gdaudio::stream::LoopMode::Forward),
        2 => Some(gdaudio::stream::LoopMode::PingPong),
        3 => Some(gdaudio::stream::LoopMode::Backward),
        _ => None,
    }
    .map(|mode| {
        let rate = wav.mix_rate.max(1) as f32;
        gdaudio::import::StreamLoop {
            mode,
            begin_secs: wav.loop_begin as f32 / rate,
            // Godot treats an unset `loop_end` (0, or anything at or before
            // `loop_begin`) as "to the end of the sample".
            end_secs: if wav.loop_end > wav.loop_begin {
                wav.loop_end as f32 / rate
            } else {
                buffer.duration_secs()
            },
        }
    });
    Ok((buffer, loop_points))
}

/// Whether `raw` is still a `.tscn` resource *reference* rather than a usable
/// path (pat-0q2kx.6).
///
/// Both spellings occur: the raw call form this project's `.tscn` parser stores
/// (`ExtResource("1_abc")`) and the `ExtResource:1_abc` form `gdresource`'s
/// value parser produces; `SubResource` appears the same way for streams with
/// embedded data. Treating any of them as a filename would hand the importer a
/// bogus path, which is how this failure used to hide.
fn is_unresolved_resource_ref(raw: &str) -> bool {
    let t = raw.trim();
    t.starts_with("ExtResource(")
        || t.starts_with("ExtResource:")
        || t.starts_with("SubResource(")
        || t.starts_with("SubResource:")
}

/// Reads a numeric node property as `f32`, accepting either Variant number
/// form. `None` means the scene never set it, so the caller keeps Godot's
/// default rather than collapsing the value to zero.
fn number_property(node: &crate::node::Node, key: &str) -> Option<f32> {
    match node.get_property(key) {
        Variant::Float(f) => Some(f as f32),
        Variant::Int(i) => Some(i as f32),
        _ => None,
    }
}

/// Normalizes `v`, falling back to `fallback` for a degenerate (zero-length)
/// basis column — a listener with a collapsed basis should still hear centered
/// audio rather than produce NaNs.
fn normalized_or(v: Vector3, fallback: Vector3) -> Vector3 {
    if v.length() < 1e-6 {
        fallback
    } else {
        v.normalized()
    }
}

impl std::fmt::Debug for AudioServerBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioServerBridge")
            .field("player_count", &self.players.len())
            .field("active_streams", &self.server.active_stream_count())
            .field("has_output", &self.output.is_some())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Editor import artifacts (pat-s3rrv.47)
// ---------------------------------------------------------------------------

/// The compiled artifact an `.import` sidecar redirects `source` to, if any.
///
/// Godot resolves EVERY imported resource through its sidecar; the audio bridge
/// had no such lookup, so an imported project — where the loose `music.ogg` is
/// replaced by `.godot/imported/music-<md5>.oggvorbisstr` — resolved to a path
/// that is not on disk and played silence.
///
/// A sidecar that names a missing artifact is reported and treated as absent
/// rather than propagated: the caller's next step is to try the loose source,
/// which is a better outcome than refusing to resolve at all, and the warning
/// keeps the broken import from being invisible.
fn import_artifact_for(source: &Path) -> Option<PathBuf> {
    match gdresource::importers::resolve_import_artifact(source) {
        Ok(artifact) => artifact,
        Err(err) => {
            tracing::warn!(
                source = %source.display(),
                error = %err,
                "audio .import sidecar could not be resolved to an artifact; \
                 falling back to the loose source"
            );
            None
        }
    }
}

/// A decoded stream plus the loop it declares, if any (pat-s3rrv.56).
type LoadedStream = (AudioBuffer, Option<gdaudio::import::StreamLoop>);

/// Decodes the stream at `path`, unwrapping an editor import artifact when the
/// remap resolved to one (pat-s3rrv.47).
///
/// Plain sources keep going through [`AudioStreamLoader`] so they keep its
/// cache; artifacts are read straight through, because each one is named by
/// exactly one source and re-reading it is not the hot path.
fn load_stream_buffer(
    importer: &mut AudioStreamLoader,
    path: &Path,
) -> Result<LoadedStream, String> {
    let is_artifact = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .is_some_and(|e| matches!(e.as_str(), "oggvorbisstr" | "mp3str" | "sample"));
    if !is_artifact {
        // `load_file` rather than `load_buffer`: the loop the file declares
        // lives on the info half, and discarding it here is exactly why every
        // looping track played once and stopped (pat-s3rrv.56).
        return importer
            .load_file(path)
            .map(|import| (import.buffer, import.info.loop_points))
            .map_err(|e| e.to_string());
    }
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let payload = artifact_audio_payload(&data, &path.to_string_lossy())?;
    // An import artifact's container carries `loop`/`loop_offset` metadata that
    // nothing parses yet, so an artifact stream reports no loop and plays once
    // — the previous behaviour, rather than a guessed one.
    gdaudio::decode::decode_auto(&payload)
        .map(|buffer| (buffer, None))
        .map_err(|e| e.to_string())
}

/// Applies a stream's declared loop to a freshly started voice.
///
/// A free function on purpose: the call sites already hold a `&mut` borrow of
/// `self.players`, and a `&mut self` method would collide with it.
///
/// ⚠️ `begin_secs`/`end_secs` are SECONDS. The WAV `smpl` chunk and Godot's
/// `AudioStreamWAV.loop_begin`/`loop_end` are FRAMES; the conversion happens in
/// `gdaudio::import` where the sample rate is known, so anything reaching here
/// is already in the playback API's units.
fn apply_stream_loop(
    server: &mut AudioServer,
    pid: gdaudio::server::PlaybackId,
    points: &gdaudio::import::StreamLoop,
) {
    if let Some(voice) = server.stream_mut(pid) {
        voice.set_loop_mode(points.mode);
        voice.set_loop_begin(points.begin_secs);
        voice.set_loop_end(points.end_secs);
    }
}

/// Extracts the codec payload (ogg / mp3 / wav bytes) from an audio import
/// artifact.
///
/// Godot's `.oggvorbisstr` / `.mp3str` / `.sample` files are binary `Resource`s
/// whose `data` property is a `PackedByteArray` holding the untouched codec
/// stream, so the payload is decoded by the ordinary decoders once unwrapped.
///
/// An artifact that is NOT an `RSRC` container is returned as-is: some tooling
/// writes the bare codec bytes under the artifact extension, and failing on
/// that would be a regression for no gain.
///
/// ⚠️ Only the byte payload is recovered here. The container also carries
/// `loop`/`loop_offset` and `bpm`/`beat_count` metadata which nothing consumes
/// yet — see the bead's remaining scope rather than assuming it round-trips.
fn artifact_audio_payload(data: &[u8], path: &str) -> Result<Vec<u8>, String> {
    if !gdresource::res_loader::is_res_binary(data) {
        return Ok(data.to_vec());
    }
    let res = gdresource::res_loader::load_res_binary(data, path).map_err(|e| e.to_string())?;
    for key in ["data", "packed_data"] {
        if let Some(Variant::PackedByteArray(bytes)) = res.get_property(key) {
            if !bytes.is_empty() {
                return Ok(bytes.clone());
            }
        }
    }
    Err(format!(
        "audio import artifact {path} carries no non-empty `data` byte payload"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;

    fn sine_buffer(frames: usize) -> AudioBuffer {
        let samples: Vec<f32> = (0..frames)
            .flat_map(|i| {
                let s = (i as f32 * 2.0 * std::f32::consts::PI * 440.0 / 44100.0).sin() * 0.5;
                [s, s]
            })
            .collect();
        AudioBuffer {
            samples,
            sample_rate: 44100,
            channels: 2,
        }
    }

    fn tree_with_player(autoplay: bool) -> (SceneTree, NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut player = Node::new("Sfx", "AudioStreamPlayer");
        player.set_property("autoplay", Variant::Bool(autoplay));
        let id = tree.add_child(root, player).unwrap();
        (tree, id)
    }

    #[test]
    fn autoplay_produces_nonsilent_mix() {
        let (mut tree, id) = tree_with_player(true);
        let mut bridge = AudioServerBridge::new();
        bridge.set_stream_buffer(id, sine_buffer(44100));

        bridge.process(&mut tree, 1.0 / 60.0);
        bridge.process(&mut tree, 1.0 / 60.0);

        assert!(
            bridge.last_mix().iter().any(|s| s.abs() > 0.01),
            "autoplayed sine should produce non-silent samples",
        );
        assert_eq!(
            tree.get_node(id).unwrap().get_property("playing"),
            Variant::Bool(true),
        );
    }

    #[test]
    fn playing_property_starts_and_stops_stream() {
        let (mut tree, id) = tree_with_player(false);
        let mut bridge = AudioServerBridge::new();
        bridge.set_stream_buffer(id, sine_buffer(44100));

        bridge.process(&mut tree, 1.0 / 60.0);
        assert!(bridge.last_mix().iter().all(|s| s.abs() < 1e-6));

        tree.get_node_mut(id)
            .unwrap()
            .set_property("playing", Variant::Bool(true));
        bridge.process(&mut tree, 1.0 / 60.0);
        bridge.process(&mut tree, 1.0 / 60.0);
        assert!(bridge.last_mix().iter().any(|s| s.abs() > 0.01));

        tree.get_node_mut(id)
            .unwrap()
            .set_property("playing", Variant::Bool(false));
        bridge.process(&mut tree, 1.0 / 60.0);
        bridge.process(&mut tree, 1.0 / 60.0);
        assert!(bridge.last_mix().iter().all(|s| s.abs() < 1e-6));
    }

    #[test]
    fn finished_stream_resets_playing_and_emits_signal() {
        let (mut tree, id) = tree_with_player(true);
        let mut bridge = AudioServerBridge::new();
        // ~2.3ms of audio: finishes within the first mixed frame batch.
        bridge.set_stream_buffer(id, sine_buffer(100));

        for _ in 0..10 {
            bridge.process(&mut tree, 1.0 / 60.0);
        }

        assert_eq!(
            tree.get_node(id).unwrap().get_property("playing"),
            Variant::Bool(false),
        );
        assert_eq!(bridge.active_stream_count(), 0);
    }

    #[test]
    fn removed_node_drops_player() {
        let (mut tree, id) = tree_with_player(true);
        let mut bridge = AudioServerBridge::new();
        bridge.set_stream_buffer(id, sine_buffer(44100));
        bridge.process(&mut tree, 1.0 / 60.0);

        tree.remove_node(id).unwrap();
        bridge.process(&mut tree, 1.0 / 60.0);
        assert_eq!(bridge.active_stream_count(), 0);
    }
}
