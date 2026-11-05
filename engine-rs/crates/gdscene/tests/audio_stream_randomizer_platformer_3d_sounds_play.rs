//! pat-m7bqe — the 3D platformer's jump and shoot sounds are
//! `AudioStreamRandomizer` sub-resources, and they must PLAY.
//!
//! `games/platformer_3d/player/player.tscn` (unmodified upstream demo):
//!
//! ```text
//! [sub_resource type="AudioStreamRandomizer" id="AudioStreamRandomizer_axa7i"]
//! random_pitch = 1.03
//! streams_count = 1
//! stream_0/stream = ExtResource("8")          ; res://player/sound_jump.wav
//! [node name="SoundJump" type="AudioStreamPlayer" parent="."]
//! stream = SubResource("AudioStreamRandomizer_axa7i")
//! ```
//!
//! The audio bridge resolved `stream` only as a file (`ExtResource`), an inline
//! `AudioStreamWAV`, or a `Resource`. A randomizer is none of those, so it fell
//! through to `StreamSpec::Unresolved` and both players were silent: `play()`
//! started no voice at all.
//!
//! Godot ground truth (`servers/audio/audio_stream.cpp`): each `play()` draws a
//! slot (`instantiate_playback`), and `AudioStreamPlaybackRandomizer::start`
//! rolls a pitch uniform in LOG space over `[1/random_pitch, random_pitch]`.
//! So a voice must start, it must carry the clip's samples, and its pitch must
//! sit inside `[1/1.03, 1.03]` and actually vary between plays.

use std::path::PathBuf;

use gdscene::packed_scene::{
    add_packed_scene_to_tree_from_res, begin_subscene_session, load_scene_from_res,
};
use gdscene::{GDScriptNodeInstance, MainLoop, NodeId, SceneTree};

const DT: f64 = 1.0 / 60.0;

fn game_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/platformer_3d")
        .canonicalize()
        .expect("games/platformer_3d fixture")
}

/// The real `player.tscn`, plus a driver node whose script calls `play()` on
/// `sound` every `period` frames — the same call `player.gd` makes on a jump
/// (`$SoundJump.play()`) or a shot (`$SoundShoot.play()`).
fn boot(sound: &str, period: u32) -> (MainLoop, NodeId) {
    let dir = game_dir();
    gdcore::file_access::set_path_roots(Some(dir.clone()), None);
    begin_subscene_session();
    let packed = load_scene_from_res("res://player/player.tscn").expect("player.tscn loads");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let player = add_packed_scene_to_tree_from_res(&mut tree, root, &packed)
        .expect("player.tscn instances");
    let player_name = tree.get_node(player).unwrap().name().to_string();
    let sound_id = tree
        .get_node_relative(player, sound)
        .unwrap_or_else(|| panic!("player.tscn must contain {sound}"));

    let driver = tree
        .add_child(root, gdscene::node::Node::new("Driver", "Node"))
        .unwrap();
    let src = format!(
        "extends Node\nvar frame = 0\nfunc _process(delta):\n    self.frame = self.frame + 1\n    if self.frame % {period} == 1:\n        get_node(\"../{player_name}/{sound}\").play()\n"
    );
    tree.attach_script(
        driver,
        Box::new(GDScriptNodeInstance::from_source(&src, driver).expect("driver parses")),
    );
    let mut ml = MainLoop::new(tree);
    ml.audio_bridge_mut().set_base_dir(dir);
    (ml, sound_id)
}

fn peak(ml: &MainLoop) -> f32 {
    ml.audio_bridge()
        .last_mix()
        .iter()
        .fold(0.0f32, |a, s| a.max(s.abs()))
}

/// Both randomizer-backed players start a voice on `play()` and mix the clip's
/// samples. At HEAD neither started anything: zero voices, a silent mix.
#[test]
fn jump_and_shoot_randomizers_play_their_clip() {
    for sound in ["SoundJump", "SoundShoot"] {
        let (mut ml, _id) = boot(sound, 1000);
        // `play()` on frame 1, then a few frames of mixing.
        for _ in 0..4 {
            ml.step(DT);
        }
        assert_eq!(
            ml.audio_bridge().active_stream_count(),
            1,
            "{sound}.play() must start exactly one voice; 0 means the \
             AudioStreamRandomizer sub-resource never resolved to its clip. \
             stream errors: {:?}",
            ml.audio_bridge().stream_errors()
        );
        let p = peak(&ml);
        assert!(
            p > 0.01,
            "{sound} must mix the clip's samples, got peak {p:.5}"
        );
        assert!(
            ml.audio_bridge().stream_errors().is_empty(),
            "no slot may fail to load: {:?}",
            ml.audio_bridge().stream_errors()
        );
    }
}

/// `random_pitch = 1.03`: every play's pitch lies in `[1/1.03, 1.03]`, and a
/// run of plays does not all land on one value. A bridge that resolved the
/// clip but dropped the variation would read exactly 1.0 every time.
#[test]
fn each_play_draws_a_pitch_inside_random_pitch() {
    let (mut ml, id) = boot("SoundJump", 3);
    let mut pitches = Vec::new();
    for frame in 1..=60 {
        ml.step(DT);
        // The driver plays on frames 1, 4, 7, ...; read the voice right after.
        if frame % 3 == 1 {
            let pitch = ml
                .audio_bridge_mut()
                .playback_pitch_scale(id)
                .expect("a voice must be live right after play()");
            pitches.push(pitch);
        }
    }
    let (lo, hi) = (1.0 / 1.03, 1.03);
    for p in &pitches {
        assert!(
            *p >= lo - 1e-4 && *p <= hi + 1e-4,
            "pitch {p} is outside Godot's [1/1.03, 1.03] for random_pitch = 1.03 \
             (all: {pitches:?})"
        );
    }
    let distinct = pitches
        .iter()
        .map(|p| (p * 1e5).round() as i64)
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    assert!(
        distinct >= pitches.len() / 2,
        "each play() must roll a fresh pitch; {} plays gave only {distinct} \
         distinct values: {pitches:?}",
        pitches.len()
    );
    assert!(
        pitches.iter().any(|p| (p - 1.0).abs() > 1e-3),
        "a randomizer with random_pitch = 1.03 must not play every clip at 1.0: {pitches:?}"
    );
}
