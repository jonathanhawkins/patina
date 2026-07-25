//! pat-0q2kx.6 — an `AudioStreamPlayer` in a real Godot `.tscn` actually plays
//! the stream its `ExtResource` names.
//!
//! Godot ground truth: `scene/audio/audio_stream_player.cpp` — `stream` is a
//! `Ref<AudioStream>`. In a `.tscn` that is written `stream = ExtResource("id")`
//! pointing at an `[ext_resource type="AudioStreamWAV" path="res://…"]` header,
//! never a bare path string.
//!
//! Patina resolved `ExtResource` refs for `script` and nothing else, so every
//! other property kept the literal text `ExtResource("id")`. The audio bridge
//! treated that as a filename, the import failed, and the `Err` was dropped —
//! so a genuine Godot scene's audio player was simply mute, with no diagnostic
//! anywhere. These tests drive the real path end to end:
//!
//! ```text
//! .wav on disk + .tscn text -> PackedScene -> SceneTree
//!   -> MainLoop::step -> AudioServerBridge::process -> AudioServer::mix -> PCM
//! ```
//!
//! No `set_stream_buffer` injection anywhere — the buffer must arrive by being
//! loaded from the file the scene names.

use std::io::Write;
use std::sync::{Arc, Mutex};

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// A quarter second of loud 440 Hz mono at 44.1 kHz, as a real RIFF/WAV file.
fn beep_wav() -> Vec<u8> {
    let sample_rate = 44_100u32;
    let frames = sample_rate / 4;
    let mut pcm = Vec::with_capacity(frames as usize * 2);
    for n in 0..frames {
        let t = n as f32 / sample_rate as f32;
        let s = (t * 440.0 * std::f32::consts::TAU).sin() * 0.8;
        pcm.extend_from_slice(&((s * i16::MAX as f32) as i16).to_le_bytes());
    }
    // format 1 = PCM
    gdaudio::wav::build_wav_bytes(sample_rate, 1, 16, 1, &pcm)
}

fn load(tscn: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

/// A scene exactly as Godot writes one: an `[ext_resource]` header plus a
/// player whose `stream` is a reference to it.
fn scene_text(res_path: &str) -> String {
    format!(
        r#"[gd_scene load_steps=2 format=3]

[ext_resource type="AudioStreamWAV" path="{res_path}" id="1_beep"]

[node name="World" type="Node"]

[node name="Beeper" type="AudioStreamPlayer" parent="."]
stream = ExtResource("1_beep")
autoplay = true
"#
    )
}

/// Captures `tracing` output emitted inside `f`, scoped to this thread so it
/// cannot race other tests in the binary.
fn capture_logs<T>(f: impl FnOnce() -> T) -> (T, String) {
    #[derive(Clone)]
    struct Sink(Arc<Mutex<Vec<u8>>>);
    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let buf = Arc::new(Mutex::new(Vec::new()));
    let sink = Sink(buf.clone());
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || sink.clone())
        .with_ansi(false)
        .finish();
    let out = tracing::subscriber::with_default(subscriber, f);
    let logged = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    (out, logged)
}

#[test]
fn audio_tscn_stream_resource_plays() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("beep.wav"), beep_wav()).unwrap();

    let tree = load(&scene_text("res://beep.wav"));

    // The ExtResource reference must have been resolved to the file it names,
    // while the raw ref survives for scene round-trip.
    let player_id = tree
        .get_node_by_path("/root/World/Beeper")
        .expect("AudioStreamPlayer node");
    let node = tree.get_node(player_id).unwrap();
    assert_eq!(
        node.get_property("stream"),
        Variant::String("ExtResource(\"1_beep\")".into()),
        "the raw resource reference must be preserved on the property itself"
    );
    assert_eq!(
        node.get_property("__extres__stream"),
        Variant::String("res://beep.wav".into()),
        "the ExtResource ref must resolve to the path from its [ext_resource] header"
    );

    let mut ml = MainLoop::new(tree);
    ml.audio_bridge_mut().set_base_dir(dir.path().to_path_buf());

    // Autoplay fires on first sight; a few frames of mixing follow.
    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }

    assert_eq!(
        ml.audio_bridge().active_stream_count(),
        1,
        "the scene's autoplay stream should be playing"
    );
    let mix = ml.audio_bridge().last_mix();
    assert!(!mix.is_empty(), "the bridge should have mixed some frames");
    assert!(
        mix.iter().any(|s| s.abs() > 0.01),
        "mixed output is silent — the .tscn stream never reached the audio server \
         (peak {:.5})",
        mix.iter().fold(0.0f32, |a, s| a.max(s.abs()))
    );
}

#[test]
fn audio_tscn_missing_stream_file_warns_and_stays_silent() {
    // Same scene, but the file the ext_resource names was never written.
    let dir = tempfile::TempDir::new().unwrap();
    let tree = load(&scene_text("res://absent.wav"));

    let (ml, logs) = capture_logs(|| {
        let mut ml = MainLoop::new(tree);
        ml.audio_bridge_mut().set_base_dir(dir.path().to_path_buf());
        for _ in 0..2 {
            ml.step(1.0 / 60.0);
        }
        ml
    });

    // The failure is reported rather than swallowed...
    assert!(
        logs.contains("WARN"),
        "a missing stream file must warn, not fail silently; logs were:\n{logs}"
    );
    assert!(
        logs.contains("absent.wav"),
        "the warning must name the file that failed to load; logs were:\n{logs}"
    );
    // ...and nothing pretends to be playing.
    assert_eq!(
        ml.audio_bridge().active_stream_count(),
        0,
        "a player whose stream failed to load must not report an active voice"
    );
    assert!(
        ml.audio_bridge().last_mix().iter().all(|s| s.abs() < 1e-6),
        "output should be silent when the stream could not be loaded"
    );
}

#[test]
fn audio_unresolved_stream_reference_warns_rather_than_probing_a_bogus_path() {
    // A player built without the scene parser keeps a raw `ExtResource(...)`
    // string. That is not a filename, and handing it to the importer is how
    // this bug used to hide.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let player = tree
        .add_child(root, gdscene::Node::new("Beeper", "AudioStreamPlayer"))
        .unwrap();
    {
        let n = tree.get_node_mut(player).unwrap();
        n.set_property("stream", Variant::String("ExtResource(\"1_beep\")".into()));
        n.set_property("autoplay", Variant::Bool(true));
    }

    let (ml, logs) = capture_logs(|| {
        let mut ml = MainLoop::new(tree);
        ml.step(1.0 / 60.0);
        ml
    });

    assert!(
        logs.contains("unresolved resource reference"),
        "an unresolved ExtResource must be reported as such; logs were:\n{logs}"
    );
    assert_eq!(ml.audio_bridge().active_stream_count(), 0);
}
