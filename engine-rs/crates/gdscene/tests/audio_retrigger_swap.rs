//! pat-mgixk.5 acceptance — `AudioStreamPlayer.play()` restarts, assigning
//! `stream` reloads, and a stream can arrive as a `Resource` or as inline PCM.
//!
//! Four gaps, all of which presented as "the audio just doesn't change":
//!
//! * `AudioServerBridge::sync_player_node`'s branch table had no arm for
//!   `play()` on a live voice, and the interpreter wrote `seek_request` only
//!   when `from > 0.0` — so the ordinary SFX retrigger `$Sfx.play()` did
//!   nothing at all.
//! * the stream loaded only `if first_sight && !player.has_stream()`, so
//!   `$Sfx.stream = load("res://b.wav")` and an animation audio key switching
//!   clips both kept the buffer the node was born with.
//! * `stream` was read only as a `Variant::String`, while `load()`/`preload()`
//!   return a `Variant::Resource` — so a script-assigned stream was ignored.
//! * an inline `AudioStreamWAV` sub-resource (PCM in the `.tscn` itself) was
//!   classified as an unresolved reference and played silence.
//!
//! Every clause is asserted on the **mixed samples**, driven from a `.tscn`
//! and GDScript through `MainLoop::step`, so a regression to "writes a
//! property nobody reads" fails. The existing `anim_audio_track.rs` tests
//! cannot see any of this: they inject buffers with `set_stream_buffer`, which
//! bypasses loading entirely.
//!
//! NOTE ON THE FILE NAME: the acceptance calls its test
//! `audio_player_retrigger_and_stream_swap`, which is kept on the test
//! FUNCTION. The file name is shorter so the `-p gdscene --test <target>`
//! command that runs it is 56 characters: the sibling `anim_audio_track.rs`
//! records a verify lane that truncated a longer `--test` target and then ran
//! a binary that does not exist. Same trade, same reason.
//!
//! Fixtures are chosen so the mix *names* which stream is playing:
//! * a 1 s ramp whose every sample equals its own timestamp, so a mixed sample
//!   of ~0.9 means the playhead is 0.9 s in;
//! * two constant clips of OPPOSITE SIGN, so the sign of the mix says whether
//!   `plus.wav` or `minus.wav` reached the server. A magnitude test could pass
//!   on the wrong clip; a sign test cannot.

use gdscene::animation::{Animation, AnimationPlayer, AnimationTrack, KeyFrame, TrackType};
use gdscene::node::{Node, NodeId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::{Dictionary, Variant};

const RATE: u32 = 44_100;
/// One `step(1.0 / 60.0)` mixes exactly this many frames (44100 / 60), so
/// positions in this file are exact rather than approximate.
const FRAMES_PER_STEP: usize = 735;

// -- fixtures ---------------------------------------------------------------

/// 16-bit mono PCM for `seconds` of a ramp whose sample value equals its own
/// timestamp in seconds. Quantisation error is one 16-bit LSB (~3.1e-5), well
/// inside every tolerance below.
fn ramp_pcm(seconds: f32) -> Vec<u8> {
    let frames = (RATE as f32 * seconds) as usize;
    let mut pcm = Vec::with_capacity(frames * 2);
    for n in 0..frames {
        let t = n as f32 / RATE as f32;
        pcm.extend_from_slice(&((t * 32767.0) as i16).to_le_bytes());
    }
    pcm
}

/// 16-bit mono PCM holding a constant `value` for `seconds`.
fn dc_pcm(value: f32, seconds: f32) -> Vec<u8> {
    let frames = (RATE as f32 * seconds) as usize;
    let sample = ((value * 32767.0) as i16).to_le_bytes();
    let mut pcm = Vec::with_capacity(frames * 2);
    for _ in 0..frames {
        pcm.extend_from_slice(&sample);
    }
    pcm
}

/// Wraps raw 16-bit mono PCM in a real RIFF/WAV container.
fn wav_file(pcm: &[u8]) -> Vec<u8> {
    // audio_format 1 = PCM.
    gdaudio::wav::build_wav_bytes(RATE, 1, 16, 1, pcm)
}

/// Standard base64 (RFC 4648, `+/` alphabet, `=` padding) — the form Godot 4's
/// text writer uses for a `PackedByteArray`.
///
/// Hand-rolled rather than pulled in as a dependency: `gdscene`'s dev-deps are
/// a shared file, and 15 lines here is cheaper than editing one.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b1 = u32::from(chunk[0]);
        let b2 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b3 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let n = (b1 << 16) | (b2 << 8) | b3;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

// -- harness ----------------------------------------------------------------

/// Loads a `.tscn`, attaches `script_src` to its root, and points the audio
/// bridge at `dir` for `res://` resolution.
fn scene_with_script(
    tscn: &str,
    root_name: &str,
    script_src: &str,
    dir: &std::path::Path,
) -> MainLoop {
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    let scene_root = tree
        .get_node_by_path(&format!("/root/{root_name}"))
        .expect("scene root");
    let script = GDScriptNodeInstance::from_source(script_src, scene_root).unwrap();
    tree.attach_script(scene_root, Box::new(script));
    let mut ml = MainLoop::new(tree);
    ml.audio_bridge_mut().set_base_dir(dir.to_path_buf());
    ml
}

/// First sample of the last mixed frame. With the ramp fixture this *is* the
/// playhead position in seconds; with a DC fixture its SIGN names the clip.
fn mix_first(ml: &MainLoop) -> f32 {
    ml.audio_bridge().last_mix().first().copied().unwrap_or(0.0)
}

/// Mean absolute amplitude of the last mixed frame — 0.0 means silence.
fn mix_level(ml: &MainLoop) -> f32 {
    let mix = ml.audio_bridge().last_mix();
    if mix.is_empty() {
        return 0.0;
    }
    mix.iter().map(|s| s.abs()).sum::<f32>() / mix.len() as f32
}

fn script_f64(ml: &MainLoop, node: NodeId, name: &str) -> f64 {
    let value = ml
        .tree()
        .get_script(node)
        .expect("script attached")
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value);
    match value {
        Some(Variant::Float(f)) => f,
        Some(Variant::Int(i)) => i as f64,
        other => panic!("script var `{name}` should be numeric, got {other:?}"),
    }
}

// -- (a) + (b): the acceptance ----------------------------------------------

/// **The acceptance test** — clauses (a) and (b) of pat-mgixk.5.
///
/// (a) `$Sfx.play()` half a second into a 1 s clip rewinds the playhead to ~0
/// and the voice keeps mixing for a further second — past the point the
/// original voice would have ended.
///
/// (b) `$Sfx.stream = load("res://minus.wav")` followed by `play()` mixes
/// `minus.wav`'s samples, not the `plus.wav` the scene was authored with.
#[test]
fn audio_player_retrigger_and_stream_swap() {
    // ---- (a) retrigger ----------------------------------------------------
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("ramp.wav"), wav_file(&ramp_pcm(1.0))).unwrap();

    let tscn = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="AudioStreamWAV" path="res://ramp.wav" id="1_ramp"]

[node name="World" type="Node"]

[node name="Sfx" type="AudioStreamPlayer" parent="."]
stream = ExtResource("1_ramp")
"#;
    // `+=` / `elif` avoided for parser-independence, matching the sibling tests.
    let src = "\
extends Node
var frame = 0
var pos_before_retrigger = 0.0
var pos_after_retrigger = 0.0
func _process(delta):
    self.frame = self.frame + 1
    var p = get_node(\"Sfx\")
    if self.frame == 1:
        p.play()
    if self.frame == 31:
        self.pos_before_retrigger = p.get_playback_position()
        p.play()
    if self.frame == 32:
        self.pos_after_retrigger = p.get_playback_position()
";
    let mut ml = scene_with_script(tscn, "World", src, dir.path());
    let world = ml.tree().get_node_by_path("/root/World").unwrap();

    // 30 frames = exactly 0.5 s of the 1 s clip.
    for _ in 0..30 {
        ml.step(1.0 / 60.0);
    }
    assert!(
        mix_level(&ml) > 0.0,
        "the scene's stream must load and play from `play()` alone; mix was silent"
    );

    // Frames 31 and 32: the script reads the position, retriggers, reads again.
    ml.step(1.0 / 60.0);
    ml.step(1.0 / 60.0);

    let before = script_f64(&ml, world, "pos_before_retrigger");
    let after = script_f64(&ml, world, "pos_after_retrigger");
    assert!(
        (before - 0.5).abs() < 0.02,
        "(a) precondition: the playhead should be ~0.5 s in before the \
         retrigger, got {before}"
    );
    assert!(
        after < 0.1,
        "(a) play() on an already-playing player must rewind to the start of \
         the clip; get_playback_position() reported {after} after {before}"
    );

    // The retriggered voice must still be running at a wall-clock time the
    // ORIGINAL voice could never reach: it started at frame 1 and the clip is
    // 1 s = 60 frames long, so without the restart frame 85 is silence.
    for _ in 0..53 {
        ml.step(1.0 / 60.0);
    }
    assert_eq!(
        ml.audio_bridge().active_stream_count(),
        1,
        "(a) the retriggered voice must still be playing 0.9 s after the \
         retrigger — the first voice would have ended by now"
    );
    // The ramp encodes time, so the mixed value IS the retriggered playhead.
    let expected = (85 - 31) as f32 * FRAMES_PER_STEP as f32 / RATE as f32;
    let heard = mix_first(&ml);
    assert!(
        (heard - expected).abs() < 0.02,
        "(a) the mix must read the RETRIGGERED playhead ({expected:.3} s), not \
         the original one; got {heard}"
    );

    // ---- (b) stream swap --------------------------------------------------
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("plus.wav"), wav_file(&dc_pcm(0.5, 1.0))).unwrap();
    std::fs::write(dir.path().join("minus.wav"), wav_file(&dc_pcm(-0.5, 1.0))).unwrap();

    let tscn = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="AudioStreamWAV" path="res://plus.wav" id="1_plus"]

[node name="World" type="Node"]

[node name="Sfx" type="AudioStreamPlayer" parent="."]
stream = ExtResource("1_plus")
"#;
    let src = "\
extends Node
var frame = 0
func _process(delta):
    self.frame = self.frame + 1
    var p = get_node(\"Sfx\")
    if self.frame == 1:
        p.play()
    if self.frame == 5:
        p.stream = load(\"res://minus.wav\")
        p.play()
";
    let mut ml = scene_with_script(tscn, "World", src, dir.path());

    ml.step(1.0 / 60.0);
    ml.step(1.0 / 60.0);
    let plus = mix_first(&ml);
    assert!(
        plus > 0.3,
        "(b) precondition: the scene's `plus.wav` must mix POSITIVE, got {plus}"
    );

    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }
    let minus = mix_first(&ml);
    assert!(
        minus < -0.3,
        "(b) `stream = load(\"res://minus.wav\")` then `play()` must mix \
         minus.wav's samples (negative), not the plus.wav the scene named; \
         got {minus}. A positive value here means the assignment never \
         reached the bridge — either the `Variant::Resource` was ignored or \
         the stream was never reloaded."
    );
}

// -- (c): inline AudioStreamWAV sub-resource --------------------------------

/// Clause (c) — a player whose `stream` is an inline `AudioStreamWAV`
/// sub-resource with embedded `data` plays, instead of being written off as an
/// unresolved `SubResource` reference.
#[test]
fn audio_player_inline_wav_subresource_plays() {
    // No files on disk at all: everything this player needs is in the scene
    // text, which is the whole point of the clause.
    let dir = tempfile::TempDir::new().unwrap();
    let tscn = format!(
        r#"[gd_scene load_steps=2 format=3]

[sub_resource type="AudioStreamWAV" id="1_wav"]
format = 1
mix_rate = 44100
stereo = false
data = PackedByteArray("{}")

[node name="World" type="Node"]

[node name="Beeper" type="AudioStreamPlayer" parent="."]
stream = SubResource("1_wav")
autoplay = true
"#,
        base64(&dc_pcm(0.5, 0.5))
    );

    let scene = PackedScene::from_tscn(&tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    let beeper = tree.get_node_by_path("/root/World/Beeper").unwrap();

    // Assert the fixture actually reached the node before asserting on sound:
    // a sub-resource body the scene loader dropped would make the clause fail
    // for a reason that has nothing to do with the bridge.
    let node = tree.get_node(beeper).unwrap();
    assert_eq!(
        node.get_property("__resref__stream"),
        Variant::String("AudioStreamWAV::1_wav".into()),
        "the inline sub-resource's class must reach the node"
    );
    match node.get_property("__subres__stream__data") {
        Variant::PackedByteArray(bytes) => assert_eq!(
            bytes.len(),
            dc_pcm(0.5, 0.5).len(),
            "the whole PCM payload must survive the base64 round trip"
        ),
        other => panic!("`__subres__stream__data` should be a PackedByteArray, got {other:?}"),
    }

    let mut ml = MainLoop::new(tree);
    ml.audio_bridge_mut().set_base_dir(dir.path().to_path_buf());
    for _ in 0..3 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        ml.audio_bridge().active_stream_count(),
        1,
        "(c) an inline AudioStreamWAV must start a voice; errors were {:?}",
        ml.audio_bridge().stream_errors()
    );
    let heard = mix_first(&ml);
    assert!(
        heard > 0.3,
        "(c) an inline AudioStreamWAV with embedded `data` must play \
         non-silent; got {heard}"
    );
}

// -- (d): animation audio track switching streams ---------------------------

/// An audio-track keyframe: Godot keys audio as a
/// `{stream, start_offset, end_offset}` dict.
fn audio_key(at: f64, stream: &str) -> KeyFrame {
    let mut d = Dictionary::new();
    d.insert("stream", Variant::String(stream.to_string()));
    d.insert("start_offset", Variant::Float(0.0));
    d.insert("end_offset", Variant::Float(0.0));
    KeyFrame::linear(at, Variant::Dictionary(d.into()))
}

/// Clause (d) — an animation audio track whose second key names a DIFFERENT
/// stream plays that file, loaded from disk.
///
/// ⚠️ No `set_stream_buffer` anywhere. `anim_audio_track.rs` injects its
/// buffers that way, which is exactly why it could not see this gap: with a
/// buffer already on the player there is nothing for a reload to change.
#[test]
fn animation_audio_track_stream_swap_loads_keyed_file() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("plus.wav"), wav_file(&dc_pcm(0.5, 2.0))).unwrap();
    std::fs::write(dir.path().join("minus.wav"), wav_file(&dc_pcm(-0.5, 2.0))).unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let anim_node = tree
        .add_child(root, Node::new("Anim", "AnimationPlayer"))
        .unwrap();
    let sfx = tree
        .add_child(anim_node, Node::new("Sfx", "AudioStreamPlayer"))
        .unwrap();

    let mut track = AnimationTrack::with_type("Sfx", "audio", TrackType::Audio);
    track.add_keyframe(audio_key(0.05, "res://plus.wav"));
    track.add_keyframe(audio_key(0.60, "res://minus.wav"));

    let mut anim = Animation::new("fx", 2.0);
    anim.tracks.push(track);
    let mut player = AnimationPlayer::new();
    player.add_animation(anim);
    player.play("fx");
    tree.attach_animation_player(anim_node, player).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.audio_bridge_mut().set_base_dir(dir.path().to_path_buf());

    // Cross the first key (t = 0.05 s) and mix a frame of it.
    for _ in 0..5 {
        ml.step(1.0 / 60.0);
    }
    let first = mix_first(&ml);
    assert!(
        first > 0.3,
        "(d) precondition: the first audio key must load and play plus.wav \
         from disk (positive); got {first}, errors {:?}",
        ml.audio_bridge().stream_errors()
    );
    assert_eq!(
        ml.tree().get_node(sfx).unwrap().get_property("stream"),
        Variant::String("res://plus.wav".into()),
    );

    // Cross the second key (t = 0.60 s, crossed on step 36) and mix two
    // frames of it.
    for _ in 0..33 {
        ml.step(1.0 / 60.0);
    }
    assert_eq!(
        ml.tree().get_node(sfx).unwrap().get_property("stream"),
        Variant::String("res://minus.wav".into()),
        "(d) precondition: the second key must write its own stream path"
    );
    let second = mix_first(&ml);
    assert!(
        second < -0.3,
        "(d) a key that switches streams must play the NEW file (negative); \
         got {second}. A positive value means the bridge kept mixing the \
         buffer the first key loaded — plus.wav is 2 s long, so it is still \
         playing and the swap is invisible without a reload."
    );
}

// -- the one-shot channel ---------------------------------------------------

/// `play()` requests a restart through a channel that is DRAINED, and never
/// through the seek channel.
///
/// Two things could each have made the retrigger in the acceptance pass for the
/// wrong reason, and both are pinned here:
/// * a request left on the node re-fires every frame, which would pin the
///   playhead at 0 instead of restarting once — so the mix is checked *after*
///   ten further frames, where a stuck request reads 0.0 rather than 0.167;
/// * routing the restart back through `seek_request` would move the OLD voice's
///   playhead instead of starting a new one, and is a no-op on a stopped
///   player, so that channel must stay clear.
#[test]
fn play_drains_its_request_without_touching_the_seek_channel() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("ramp.wav"), wav_file(&ramp_pcm(1.0))).unwrap();
    let tscn = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="AudioStreamWAV" path="res://ramp.wav" id="1_ramp"]

[node name="World" type="Node"]

[node name="Sfx" type="AudioStreamPlayer" parent="."]
stream = ExtResource("1_ramp")
"#;
    // Two plays: the second lands on a LIVE voice, which is the case the branch
    // table had no arm for.
    let src = "\
extends Node
var frame = 0
func _process(delta):
    self.frame = self.frame + 1
    if self.frame == 1:
        get_node(\"Sfx\").play()
    if self.frame == 11:
        get_node(\"Sfx\").play()
";
    let mut ml = scene_with_script(tscn, "World", src, dir.path());
    let sfx = ml.tree().get_node_by_path("/root/World/Sfx").unwrap();

    ml.step(1.0 / 60.0);
    {
        let node = ml.tree().get_node(sfx).unwrap();
        assert_eq!(
            node.get_property("__play_request__"),
            Variant::Nil,
            "the one-shot play request must be drained the frame it is consumed, \
             or the clip restarts on every frame forever"
        );
        assert_eq!(
            node.get_property("seek_request"),
            Variant::Nil,
            "play() must not write a seek: a seek cannot start a stopped voice"
        );
    }

    // Ten frames of free advance: a request that re-fired would hold the
    // playhead at the ramp's origin.
    for _ in 0..9 {
        ml.step(1.0 / 60.0);
    }
    let before = mix_first(&ml);
    let expected_before = 9.0 * FRAMES_PER_STEP as f32 / RATE as f32;
    assert!(
        (before - expected_before).abs() < 0.02,
        "the playhead must advance freely after a play(); expected \
         ~{expected_before:.3}, got {before}"
    );

    // Frame 11 retriggers the LIVE voice: the ramp must read the top again.
    ml.step(1.0 / 60.0);
    let after = mix_first(&ml);
    assert!(
        after < 0.01,
        "play() on a live voice must restart the clip, so the ramp reads ~0 \
         again; got {after} (it was {before} the frame before)"
    );
    assert_eq!(
        ml.tree()
            .get_node(sfx)
            .unwrap()
            .get_property("__play_request__"),
        Variant::Nil,
        "the second request must be drained too"
    );
}
