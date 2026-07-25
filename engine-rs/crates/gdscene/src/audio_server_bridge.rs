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
use std::path::PathBuf;

use gdaudio::import::AudioStreamLoader;
use gdaudio::sample::AudioOutputStream;
use gdaudio::{mix_and_write, AudioBuffer, AudioServer, AudioStreamPlayer};
use gdvariant::Variant;

use crate::node::NodeId;
use crate::scene_tree::SceneTree;

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
    pub fn set_stream_buffer(&mut self, node_id: NodeId, buffer: AudioBuffer) {
        self.players
            .entry(node_id)
            .or_insert_with(AudioStreamPlayer::new)
            .set_stream(buffer);
    }

    /// Number of currently audible streams.
    pub fn active_stream_count(&self) -> usize {
        self.server.active_stream_count()
    }

    fn resolve_stream_path(&self, raw: &str) -> PathBuf {
        let stripped = raw.strip_prefix("res://").unwrap_or(raw);
        match &self.base_dir {
            Some(base) => base.join(stripped),
            None => PathBuf::from(stripped),
        }
    }

    /// Syncs `AudioStreamPlayer` nodes and mixes `delta` seconds of audio.
    pub fn process(&mut self, tree: &mut SceneTree, delta: f64) {
        let player_ids: Vec<NodeId> = tree
            .all_nodes_in_tree_order()
            .into_iter()
            .filter(|&id| {
                tree.get_node(id)
                    .is_some_and(|n| n.class_name() == "AudioStreamPlayer")
            })
            .collect();

        for id in &player_ids {
            self.sync_player_node(tree, *id);
        }

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
        }

        // Mix this frame's worth of output.
        self.frame_accumulator += delta * f64::from(self.server.output_sample_rate());
        let frames = self.frame_accumulator as usize;
        self.frame_accumulator -= frames as f64;
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

    fn sync_player_node(&mut self, tree: &mut SceneTree, id: NodeId) {
        let (stream_path, volume_db, autoplay, playing_prop, bus, seek_request, paused_prop, pitch) = {
            let Some(node) = tree.get_node(id) else {
                return;
            };
            let as_bool = |key: &str| matches!(node.get_property(key), Variant::Bool(true));
            let as_f32 = |key: &str| match node.get_property(key) {
                Variant::Float(f) => Some(f as f32),
                Variant::Int(i) => Some(i as f32),
                _ => None,
            };
            // Godot's `stream` is a `Ref<AudioStream>`; in a `.tscn` it is
            // `ExtResource("id")` pointing at an `[ext_resource]` header, not a
            // path. `PackedScene` resolves that reference to the referenced file
            // under an `__extres__stream` companion, so prefer it and fall back
            // to a plain path string (the programmatic form) (pat-0q2kx.6).
            let stream_path = match node.get_property("__extres__stream") {
                Variant::String(s) if !s.is_empty() => s,
                _ => match node.get_property("stream") {
                    Variant::String(s) => s,
                    _ => String::new(),
                },
            };
            let volume_db = as_f32("volume_db").unwrap_or(0.0);
            let bus = match node.get_property("bus") {
                Variant::String(s) => s,
                _ => "Master".to_owned(),
            };
            (
                stream_path,
                volume_db,
                as_bool("autoplay"),
                as_bool("playing"),
                bus,
                // `seek_request` is the interpreter's one-shot channel for
                // `AudioStreamPlayer.seek(pos)` / `play(from)`; absent (Nil)
                // means "no seek this frame". Drained below.
                as_f32("seek_request"),
                as_bool("stream_paused"),
                // Godot's default pitch_scale is 1.0, not 0.
                as_f32("pitch_scale").unwrap_or(1.0),
            )
        };

        // Godot pauses a player's stream when the tree pauses and the node's
        // effective process mode cannot run (audio_stream_player_internal.cpp:97-131
        // `set_stream_paused(!node->can_process())` on NOTIFICATION_PAUSED).
        let can_process = tree.should_process_node(id);

        let first_sight = self.seen.insert(id);
        let resolved = if stream_path.is_empty() {
            None
        } else if is_unresolved_resource_ref(&stream_path) {
            // The scene named a resource but nothing resolved it to a file. Say
            // so — this is precisely the failure that used to present as a
            // player that simply never made a sound (pat-0q2kx.6).
            if first_sight {
                tracing::warn!(
                    node = %tree.get_node(id).map(|n| n.name().to_owned()).unwrap_or_default(),
                    stream = %stream_path,
                    "AudioStreamPlayer stream is an unresolved resource reference; \
                     the player will be silent"
                );
            }
            None
        } else {
            Some(self.resolve_stream_path(&stream_path))
        };

        let player = self
            .players
            .entry(id)
            .or_insert_with(AudioStreamPlayer::new);
        player.set_bus(bus);
        player.set_volume_db(volume_db);

        // Load the stream from disk on first sight if the node names one and
        // no buffer was injected directly.
        if first_sight && !player.has_stream() {
            if let Some(path) = resolved {
                // A failed import used to be discarded here, so a missing or
                // undecodable file was indistinguishable from success: the
                // player just stayed silent. Report it (pat-0q2kx.6).
                match self.importer.load_buffer(&path) {
                    Ok(buffer) => player.set_stream(buffer),
                    Err(err) => tracing::warn!(
                        path = %path.display(),
                        error = %err,
                        "AudioStreamPlayer stream failed to load; the player will be silent"
                    ),
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

        if first_sight && (autoplay || playing_prop) {
            if player.play(&mut self.server).is_some() {
                if let Some(node) = tree.get_node_mut(id) {
                    node.set_property("playing", Variant::Bool(true));
                }
            }
        } else if playing_prop && !voice_alive && !player.playing_flag() {
            // Script flipped `playing` on: start the stream.
            player.play(&mut self.server);
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

impl std::fmt::Debug for AudioServerBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioServerBridge")
            .field("player_count", &self.players.len())
            .field("active_streams", &self.server.active_stream_count())
            .field("has_output", &self.output.is_some())
            .finish()
    }
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
