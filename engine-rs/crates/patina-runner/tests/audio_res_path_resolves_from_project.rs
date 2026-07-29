//! Acceptance test for pat-s3rrv.5: `res://` audio stream paths resolve against
//! the project directory, not the process working directory.
//!
//! Godot resolves every `res://` path against the folder holding
//! `project.godot` (`core/io/resource_loader.cpp`, `ProjectSettings::globalize_path`),
//! so a game launched from any working directory loads the same assets.
//!
//! Patina's [`AudioServerBridge::resolve_stream_path`] joined `self.base_dir`
//! when set and otherwise returned a bare *CWD-relative* path — and
//! `set_base_dir` had ZERO production callers: `EngineBootstrap::init_running`
//! built the `MainLoop`, loaded the bus layout from `project_dir` and attached
//! the output sink, but never told the bridge where `res://` lived. A project
//! launched from anywhere but its own directory was therefore silently mute,
//! and the load failure was a `tracing::warn!` the runner never surfaced.
//!
//! These tests boot a project written to a temp directory — which is never the
//! test process's working directory — and drive the real path end to end:
//!
//! ```text
//! BootConfig{project_dir} -> init_running -> bridge.set_base_dir
//!   -> sync_player_node -> AudioStreamLoader::load_buffer(project_dir/sfx/beep.wav)
//!   -> AudioServer::mix -> non-silent last_mix()
//! ```
//!
//! No `set_stream_buffer` injection: the buffer must arrive by being loaded
//! from the file the scene names, through the project root.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

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
    // audio_format 1 = PCM
    gdaudio::wav::build_wav_bytes(sample_rate, 1, 16, 1, &pcm)
}

/// A scene with an autoplaying player naming its stream by `res://` path.
const MAIN_TSCN: &str = r#"[gd_scene format=3]

[node name="World" type="Node"]

[node name="Beeper" type="AudioStreamPlayer" parent="."]
stream = "res://sfx/beep.wav"
autoplay = true
"#;

/// Writes a minimal project to a fresh temp directory. `with_asset` controls
/// whether the `.wav` the scene names actually exists.
fn write_project(tag: &str, with_asset: bool) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "patina_audio_res_{tag}_{}_{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(dir.join("sfx")).expect("create project dirs");
    fs::write(dir.join("project.godot"), "[application]\n").expect("write project.godot");
    fs::write(dir.join("main.tscn"), MAIN_TSCN).expect("write main.tscn");
    if with_asset {
        fs::write(dir.join("sfx").join("beep.wav"), beep_wav()).expect("write beep.wav");
    }
    dir
}

/// The premise of both tests: the project lives somewhere the process working
/// directory cannot reach it by a relative join. Without this, a CWD-relative
/// resolution would pass by accident.
fn assert_project_is_outside_cwd(project_dir: &Path) {
    let cwd = std::env::current_dir().expect("cwd");
    assert!(
        !project_dir.starts_with(&cwd) && !cwd.starts_with(project_dir),
        "test premise: the project dir {} must be outside the working directory {}",
        project_dir.display(),
        cwd.display(),
    );
    assert!(
        !cwd.join("sfx").join("beep.wav").exists(),
        "test premise: the stream must not be resolvable relative to the CWD",
    );
}

/// Booting from outside the project directory still loads the project's audio:
/// the autoplaying player's stream is found under the project root and mixes to
/// non-silent PCM within the first few frames.
#[test]
fn audio_res_path_resolves_from_project() {
    let project_dir = write_project("ok", true);
    assert_project_is_outside_cwd(&project_dir);

    // headless => no hardware sink is attached, so mixed frames stay in
    // `last_mix()` where the test can read them.
    let config = BootConfig::with_scene(project_dir.join("main.tscn"));
    assert_eq!(
        config.project_dir, project_dir,
        "with_scene should derive the project dir from the scene path",
    );
    let mut boot = EngineBootstrap::new(config);
    boot.run_all().expect("boot flow should complete");

    let main_loop = boot
        .main_loop_mut()
        .expect("Running phase creates a MainLoop");

    // Step a few frames: MainLoop::step drives AudioServerBridge::process,
    // which imports the stream on first sight and mixes it.
    let mut nonsilent = 0usize;
    let mut peak = 0.0f32;
    for _ in 0..4 {
        main_loop.step(1.0 / 60.0);
        let mix = main_loop.audio_bridge().last_mix();
        nonsilent += mix.iter().filter(|s| s.abs() > 0.01).count();
        peak = mix.iter().fold(peak, |acc, s| acc.max(s.abs()));
    }

    assert!(
        main_loop.audio_bridge().stream_errors().is_empty(),
        "the stream should load cleanly from the project root, got: {:?}",
        main_loop.audio_bridge().stream_errors(),
    );
    assert!(
        nonsilent > 0,
        "an autoplaying res:// stream should mix to non-silent PCM when booted \
         from outside the project dir (peak amplitude was {peak})",
    );

    fs::remove_dir_all(&project_dir).ok();
}

/// The same boot with the named file missing reports a visible error instead of
/// presenting as plain silence — the failure mode that made the CWD bug so hard
/// to see in the first place.
#[test]
fn audio_missing_res_stream_reports_a_visible_error() {
    let project_dir = write_project("missing", false);
    assert_project_is_outside_cwd(&project_dir);

    let mut boot = EngineBootstrap::new(BootConfig::with_scene(project_dir.join("main.tscn")));
    boot.run_all().expect("boot flow should complete");

    let main_loop = boot
        .main_loop_mut()
        .expect("Running phase creates a MainLoop");
    for _ in 0..4 {
        main_loop.step(1.0 / 60.0);
    }

    let errors = main_loop.audio_bridge().stream_errors();
    assert!(
        !errors.is_empty(),
        "a missing stream file must be reported, not silently ignored",
    );
    assert!(
        errors.iter().any(|e| e.contains("beep.wav")),
        "the reported error should name the missing stream, got: {errors:?}",
    );
    // And it names the project root, not a CWD-relative guess.
    assert!(
        errors
            .iter()
            .any(|e| e.contains(&project_dir.display().to_string())),
        "the reported error should name the path under the project root {}, got: {errors:?}",
        project_dir.display(),
    );

    fs::remove_dir_all(&project_dir).ok();
}
