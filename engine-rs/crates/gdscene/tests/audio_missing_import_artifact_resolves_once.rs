//! A project cloned without Godot's `.godot/` import cache keeps every
//! `<asset>.import` sidecar, each pointing at an artifact that is not there —
//! the official "Dodge the Creeps" demo straight from git is exactly this. The
//! audio bridge falls back to the loose source, correctly, but it resolved the
//! stream path on EVERY frame for EVERY player: an ancestor walk of filesystem
//! probes plus a sidecar parse, and a warning each time. The runner's output
//! was two warnings per frame for the whole run.
//!
//! The path is now resolved only when a stream actually has to load.

use std::sync::{Arc, Mutex};

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;

fn beep_wav() -> Vec<u8> {
    let sample_rate = 44_100u32;
    let frames = sample_rate / 4;
    let mut pcm = Vec::with_capacity(frames as usize * 2);
    for n in 0..frames {
        let t = n as f32 / sample_rate as f32;
        let s = (t * 440.0 * std::f32::consts::TAU).sin() * 0.8;
        pcm.extend_from_slice(&((s * i16::MAX as f32) as i16).to_le_bytes());
    }
    gdaudio::wav::build_wav_bytes(sample_rate, 1, 16, 1, &pcm)
}

/// Godot's own sidecar shape, naming an artifact the clone does not have.
const SIDECAR: &str = r#"[remap]

importer="wav"
type="AudioStreamWAV"
uid="uid://td2mgko63p61"
path="res://.godot/imported/beep.wav-98c95c744b35280048c2bd093cf8a356.sample"

[deps]

source_file="res://beep.wav"
dest_files=["res://.godot/imported/beep.wav-98c95c744b35280048c2bd093cf8a356.sample"]
"#;

const SCENE: &str = r#"[gd_scene format=3]

[ext_resource type="AudioStream" path="res://beep.wav" id="1_beep"]

[node name="World" type="Node"]

[node name="Beeper" type="AudioStreamPlayer" parent="."]
stream = ExtResource("1_beep")
autoplay = true
"#;

fn capture_logs<T>(f: impl FnOnce() -> T) -> (T, String) {
    #[derive(Clone)]
    struct Sink(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Sink {
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
fn a_missing_import_artifact_is_resolved_and_reported_once_not_every_frame() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("beep.wav"), beep_wav()).unwrap();
    std::fs::write(dir.path().join("beep.wav.import"), SIDECAR).unwrap();

    let ((playing, mix_is_silent), logs) = capture_logs(|| {
        let scene = PackedScene::from_tscn(SCENE).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
        let mut ml = MainLoop::new(tree);
        ml.audio_bridge_mut().set_base_dir(dir.path().to_path_buf());
        // 12 frames = 0.2 s: inside the quarter-second beep, so it is still
        // playing at the end, and plenty of frames for a per-frame warning.
        for _ in 0..12 {
            ml.step(1.0 / 60.0);
        }
        (
            ml.audio_bridge().active_stream_count(),
            ml.audio_bridge().last_mix().iter().all(|s| s.abs() < 1e-6),
        )
    });

    // The fallback still works: the loose source plays.
    assert!(playing > 0, "the loose source is the stream that plays");
    assert!(!mix_is_silent, "and it is audible");

    let warnings = logs.matches("could not be resolved to an artifact").count();
    assert_eq!(
        warnings, 1,
        "resolved once, when the stream loaded — not on every one of 12 frames:\n{logs}"
    );
}
