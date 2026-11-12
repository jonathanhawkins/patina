//! pat-s3rrv.47 acceptance — MP3 reaches the mixer, and an imported project
//! plays through its `.import` remap when the loose source is gone.
//!
//! Godot ground truth: every imported resource resolves through its `.import`
//! sidecar to an artifact under `.godot/imported/`, never to the loose source —
//! which an exported or freshly-imported project routinely no longer has
//! (`editor/import/resource_importer_wav.cpp`, `modules/vorbis/`,
//! `modules/minimp3/`). `ResourceImporterWAV` also carries the source's `smpl`
//! loop points onto the imported `AudioStreamWAV`.
//!
//! Patina had two independent holes here. `gdaudio::decode::decode_mp3` has
//! existed since pat-p0s82.13, but `AudioFormat` had no MP3 variant and
//! `decode_auto` sniffed only RIFF/OggS, so **no `.mp3` in any scene could ever
//! play** — the decoder was unreachable rather than missing. Separately,
//! `AudioServerBridge::resolve_stream_path` joined `res://` to the base dir with
//! no sidecar lookup at all, so an imported project resolved to a path that is
//! not on disk and mixed silence.
//!
//! These drive the shipped path end to end — file on disk + `.tscn` text →
//! `PackedScene` → `SceneTree` → `MainLoop::step` → `AudioServerBridge` →
//! `AudioServer::mix` → PCM — and assert on mixed samples. No
//! `set_stream_buffer` injection anywhere: the buffer has to arrive by being
//! found and decoded from what the scene names.

use std::path::Path;

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;

/// A real MP3, shared with `gdaudio`'s own decoder tests so this cannot pass
/// against a fixture that is not actually MP3.
static TONE_MP3: &[u8] = include_bytes!("../../gdaudio/tests/fixtures/test_tone.mp3");
/// A real Ogg Vorbis stream, likewise shared with `gdaudio`'s tests.
static TONE_OGG: &[u8] = include_bytes!("../../gdaudio/tests/fixtures/test_tone.ogg");

/// A one-player scene naming `res://<stream>` with autoplay on.
fn scene_text(stream: &str) -> String {
    format!(
        r#"[gd_scene format=3]

[node name="World" type="Node"]

[node name="Speaker" type="AudioStreamPlayer" parent="."]
stream = "{stream}"
autoplay = true
"#
    )
}

/// Instances the scene, points the bridge at `project`, and mixes a few frames.
fn mix_scene(project: &Path, stream: &str) -> Vec<f32> {
    let scene = PackedScene::from_tscn(&scene_text(stream)).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");

    let mut ml = MainLoop::new(tree);
    ml.audio_bridge_mut().set_base_dir(project.to_path_buf());
    // Autoplay fires on first sight; the frames after it do the mixing.
    for _ in 0..4 {
        ml.step(1.0 / 60.0);
    }
    ml.audio_bridge().last_mix().to_vec()
}

fn peak(mix: &[f32]) -> f32 {
    mix.iter().fold(0.0f32, |a, s| a.max(s.abs()))
}

/// Marks `dir` as a Godot project so the `.import` resolver's project-root walk
/// terminates there. Both markers are written because `find_project_root`
/// accepts either, and a real project has both.
fn make_project(dir: &Path) {
    std::fs::write(dir.join("project.godot"), b"[application]\n").unwrap();
    std::fs::create_dir_all(dir.join(".godot/imported")).unwrap();
}

/// **The acceptance test.** An `.mp3` plays, and an OGG that exists ONLY as an
/// import artifact behind a `.import` sidecar plays too.
#[test]
fn audio_import_remap_and_mp3_stream_plays() {
    // -- (1) MP3 through the loader ----------------------------------------
    let dir = tempfile::TempDir::new().unwrap();
    make_project(dir.path());
    std::fs::write(dir.path().join("tone.mp3"), TONE_MP3).unwrap();

    let mix = mix_scene(dir.path(), "res://tone.mp3");
    assert!(!mix.is_empty(), "the bridge should have mixed some frames");
    assert!(
        peak(&mix) > 0.001,
        "an .mp3 stream mixed silence (peak {:.6}). Before pat-s3rrv.47 this was \
         unavoidable: `AudioFormat` had no MP3 variant, so the file was rejected \
         at format detection and `decode_mp3` was never called.",
        peak(&mix),
    );

    // -- (2) the `.import` remap, with the source deleted -------------------
    let dir = tempfile::TempDir::new().unwrap();
    make_project(dir.path());
    // The artifact, and ONLY the artifact. `tone.ogg` is deliberately never
    // written — that is exactly the state an imported/exported project is in,
    // and the state the old `candidate.exists()` walk could not resolve.
    let artifact_rel = ".godot/imported/tone.ogg-3f7a1c.oggvorbisstr";
    std::fs::write(dir.path().join(artifact_rel), TONE_OGG).unwrap();
    std::fs::write(
        dir.path().join("tone.ogg.import"),
        format!(
            "[remap]\n\nimporter=\"oggvorbisstr\"\ntype=\"AudioStreamOggVorbis\"\npath=\"res://{artifact_rel}\"\n"
        ),
    )
    .unwrap();
    assert!(
        !dir.path().join("tone.ogg").exists(),
        "fixture check: the loose source must NOT exist, or this proves nothing",
    );

    let mix = mix_scene(dir.path(), "res://tone.ogg");
    assert!(
        peak(&mix) > 0.001,
        "an artifact-only stream mixed silence (peak {:.6}). The player found no \
         file because `resolve_stream_path` never consulted `tone.ogg.import`.",
        peak(&mix),
    );
}

/// The control that makes the remap assertion mean something: with the sidecar
/// removed and the source still absent, the same scene IS silent. Without this,
/// a bridge that somehow found audio elsewhere would look like a pass.
#[test]
fn without_the_sidecar_an_artifact_only_project_is_silent() {
    let dir = tempfile::TempDir::new().unwrap();
    make_project(dir.path());
    std::fs::write(
        dir.path()
            .join(".godot/imported/tone.ogg-3f7a1c.oggvorbisstr"),
        TONE_OGG,
    )
    .unwrap();
    // No `tone.ogg.import`, so nothing points at the artifact.

    let mix = mix_scene(dir.path(), "res://tone.ogg");
    assert!(
        peak(&mix) <= 0.001,
        "with no sidecar there is nothing to resolve; mixing audio here would \
         mean the artifact was found by guesswork rather than by the remap",
    );
}

/// A WAV carrying a `smpl` loop chunk imports with Godot's `AudioStreamWAV`
/// loop fields set, and the loop `type` is translated rather than copied.
#[test]
fn wav_smpl_chunk_sets_the_loop_fields() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("looped.wav");
    std::fs::write(&path, wav_with_smpl_loop(0, 128, 4096)).unwrap();

    let res = gdresource::importers::import_wav(&path).expect("wav imports");

    // WAV loop type 0 (forward) maps to Godot LOOP_FORWARD = 1, NOT 0.
    // Copying the raw value across would read as LOOP_DISABLED and the sample
    // would play once — silently, and looking exactly like the old behaviour.
    assert_eq!(
        res.get_property("loop_mode"),
        Some(&gdvariant::Variant::Int(1)),
        "WAV forward loop must become Godot LOOP_FORWARD (1)",
    );
    assert_eq!(
        res.get_property("loop_begin"),
        Some(&gdvariant::Variant::Int(128))
    );
    assert_eq!(
        res.get_property("loop_end"),
        Some(&gdvariant::Variant::Int(4096))
    );

    // A ping-pong loop is type 1 in the file and mode 2 in Godot — the pair
    // that proves this is a translation table and not an accidental identity.
    let pp = dir.path().join("pingpong.wav");
    std::fs::write(&pp, wav_with_smpl_loop(1, 0, 64)).unwrap();
    let res = gdresource::importers::import_wav(&pp).expect("wav imports");
    assert_eq!(
        res.get_property("loop_mode"),
        Some(&gdvariant::Variant::Int(2)),
        "WAV alternating loop must become Godot LOOP_PINGPONG (2)",
    );
}

/// A WAV with no `smpl` chunk keeps `LOOP_DISABLED`, so every existing
/// unlooped import is unaffected.
#[test]
fn wav_without_smpl_stays_unlooped() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("plain.wav");
    std::fs::write(
        &path,
        gdaudio::wav::build_wav_bytes(44100, 1, 16, 1, &[0u8; 256]),
    )
    .unwrap();

    let res = gdresource::importers::import_wav(&path).expect("wav imports");
    assert_eq!(
        res.get_property("loop_mode"),
        Some(&gdvariant::Variant::Int(0)),
        "no smpl chunk means LOOP_DISABLED",
    );
}

/// Builds a mono 16-bit WAV carrying one `smpl` loop.
///
/// `build_wav_bytes` emits only `fmt `+`data`, so the `smpl` chunk is appended
/// and the RIFF size field patched to cover it — a reader that trusts the RIFF
/// size would otherwise stop before the loop data.
fn wav_with_smpl_loop(loop_type: u32, begin: u32, end: u32) -> Vec<u8> {
    let mut wav = gdaudio::wav::build_wav_bytes(44100, 1, 16, 1, &[0u8; 512]);

    let mut smpl = Vec::new();
    smpl.extend_from_slice(b"smpl");
    smpl.extend_from_slice(&60u32.to_le_bytes()); // chunk size: 36 header + 24 loop
    for _ in 0..7 {
        smpl.extend_from_slice(&0u32.to_le_bytes()); // manufacturer..SMPTE offset
    }
    smpl.extend_from_slice(&1u32.to_le_bytes()); // num sample loops
    smpl.extend_from_slice(&0u32.to_le_bytes()); // sampler data
    smpl.extend_from_slice(&0u32.to_le_bytes()); // cue point id
    smpl.extend_from_slice(&loop_type.to_le_bytes());
    smpl.extend_from_slice(&begin.to_le_bytes());
    smpl.extend_from_slice(&end.to_le_bytes());
    smpl.extend_from_slice(&0u32.to_le_bytes()); // fraction
    smpl.extend_from_slice(&0u32.to_le_bytes()); // play count

    wav.extend_from_slice(&smpl);
    let riff_size = (wav.len() - 8) as u32;
    wav[4..8].copy_from_slice(&riff_size.to_le_bytes());
    wav
}
