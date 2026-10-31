//! pat-mgixk.88 acceptance: the project's audio settings reach the live mixer.
//!
//! # What was wrong
//!
//! `AudioServer::from_project_settings` (`gdaudio/src/server.rs:127`) read
//! `audio/driver/mix_rate` and **had no caller anywhere**, so
//! `AudioServerBridge::new` always built a 44100 Hz server. A 48 kHz project
//! mixed every stream at the wrong rate — audibly wrong pitch, since the
//! resample step is `src_rate / output_rate`.
//!
//! And the two global panning settings were read by nothing at all: a grep for
//! `2d_panning_strength|3d_panning_strength` across every crate returned zero
//! hits. `read_spatial_2d`/`read_spatial_3d` used only the NODE's own
//! `panning_strength`. ⚠️ Godot's global default is **0.5**, not 1.0, so every
//! positional player was panned twice as hard as Godot pans it.
//!
//! # The path driven
//!
//! ```text
//! project.godot -> EngineBootstrap (Core..Running)
//!   -> MainLoop::configure_from_project_settings
//!     -> AudioServerBridge::configure_from_project_settings
//!       -> AudioServer::set_output_sample_rate  +  the two panning multipliers
//!         -> MainLoop::step -> mix
//! ```
//!
//! ⚠️ The panning clause is DIFFERENTIAL — two boots of the same scene that
//! differ only in whether `3d_panning_strength = 0` is present. Asserting
//! "L == R" alone would pass against a silent mix, and asserting it against a
//! hard-coded expected ratio would encode this panner's exact curve. The
//! control boot (setting absent, so the 0.5 default applies) must show L != R
//! for the same source, and both are guarded as non-silent first.

use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// 48 kHz, so `get_mix_rate()` cannot accidentally match the 44100 default.
const MIX_RATE: u32 = 48_000;

/// A hard-LEFT 3D source. Panning is the only thing that can separate the
/// channels for it, which is what makes the clause below falsifiable.
const MAIN_TSCN: &str = r#"[gd_scene format=3]

[node name="Main" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
current = true

[node name="Sfx" type="AudioStreamPlayer3D" parent="."]
position = Vector3(-30, 0, 0)
autoplay = true
"#;

/// `panning` selects whether the project pins `3d_panning_strength = 0`.
/// Absent means Godot's 0.5 default applies, which is the control.
fn write_project(panning_zero: bool) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "pat_mgixk_88_{}_{}",
        if panning_zero { "nopan" } else { "pan" },
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create project dir");

    let panning_line = if panning_zero {
        // ⚠️ Authored as `0.0`. The reader accepts zero as a VALID setting
        // meaning "no panning" rather than treating it as malformed — if it
        // rejected zero, this fixture would silently keep the 0.5 default and
        // the assertion below would be vacuous.
        "general/3d_panning_strength=0.0\n"
    } else {
        ""
    };
    std::fs::write(
        dir.join("project.godot"),
        format!(
            "config_version=5\n\n\
             [application]\n\n\
             run/main_scene=\"res://main.tscn\"\n\n\
             [audio]\n\n\
             driver/mix_rate={MIX_RATE}\n\
             {panning_line}"
        ),
    )
    .expect("write project.godot");
    std::fs::write(dir.join("main.tscn"), MAIN_TSCN).expect("write main.tscn");
    dir
}

/// A two-second DC buffer at the PROJECT's rate, so the resample step is unity
/// and the mix carries the source's full amplitude into both channels before
/// panning scales them. Shape copied from `audio_spatial_players_wiring`.
fn dc_buffer() -> gdaudio::AudioBuffer {
    gdaudio::AudioBuffer {
        samples: vec![1.0; (MIX_RATE as usize) * 2],
        sample_rate: MIX_RATE,
        channels: 2,
    }
}

/// Sums |L| and |R| over the interleaved mix.
fn channel_energy(mix: &[f32]) -> (f32, f32) {
    let mut left = 0.0f32;
    let mut right = 0.0f32;
    for frame in mix.chunks_exact(2) {
        left += frame[0].abs();
        right += frame[1].abs();
    }
    (left, right)
}

/// Boots the project, injects a buffer on the autoplaying source, steps, and
/// returns `(mix, script_mix_rate)`.
fn boot_and_mix(panning_zero: bool) -> (Vec<f32>, f32) {
    let dir = write_project(panning_zero);
    // ⚠️ `main_scene` must be set ON THE CONFIG. `EngineBootstrap::init_main_scene`
    // instantiates a scene only when `config.main_scene` is `Some` — it does NOT
    // read `application/run/main_scene` from project.godot, and
    // `bootstrap.rs:533` treats an unset one as valid "headless/test mode". The
    // first revision of this fixture set only the project setting and the whole
    // tree came up empty, so `/root/Main/Sfx` did not exist. Struct-literal shape
    // copied from the green `animation_player_tscn_autoplay`.
    let config = BootConfig {
        main_scene: Some(dir.join("main.tscn")),
        project_dir: dir.clone(),
        headless: true,
        ..BootConfig::default()
    };
    let mut boot = EngineBootstrap::new(config);
    boot.run_all().expect("boot flow should complete");
    let main_loop = boot
        .main_loop_mut()
        .expect("the Running phase creates a MainLoop");

    // The rate the SERVER is actually running at, which is what
    // `AudioServer.get_mix_rate()` answers through `audio_get_mix_rate`.
    let rate = main_loop.audio_bridge().server().output_sample_rate() as f32;

    let sfx = main_loop
        .tree()
        .get_node_by_path("/root/Main/Sfx")
        .expect("the main scene has /root/Main/Sfx");
    main_loop
        .audio_bridge_mut()
        .set_stream_buffer(sfx, dc_buffer());

    for _ in 0..3 {
        main_loop.step(1.0 / 60.0);
    }
    let mix = main_loop.audio_bridge().last_mix().to_vec();
    let _ = std::fs::remove_dir_all(&dir);
    (mix, rate)
}

/// **THE ACCEPTANCE TEST (pat-mgixk.88).**
#[test]
fn audio_project_settings_mix_rate_and_panning() {
    // -- 1. `audio/driver/mix_rate` reaches the live server ------------------
    let (nopan_mix, rate) = boot_and_mix(true);
    assert_eq!(
        rate, MIX_RATE as f32,
        "the booted server must run at the project's audio/driver/mix_rate \
         ({MIX_RATE}), got {rate}. 44100 means \
         AudioServer::from_project_settings still has no caller and the bridge \
         built a default server — which also makes every stream resample at \
         the wrong step"
    );

    // -- 2. with 3d_panning_strength = 0 a hard-left source is CENTRED ------
    assert!(
        !nopan_mix.is_empty(),
        "the autoplaying AudioStreamPlayer3D must reach the mixer at all"
    );
    let (nl, nr) = channel_energy(&nopan_mix);
    assert!(
        nl + nr > 0.01,
        "fixture precondition: the mix must be audible before comparing \
         channels, got L{nl} R{nr}. A silent mix would satisfy 'L == R' for \
         entirely the wrong reason"
    );
    assert!(
        (nl - nr).abs() <= (nl + nr) * 0.02,
        "3d_panning_strength = 0 means NO panning, so a hard-left source must \
         mix equally to both channels: L{nl} R{nr}"
    );

    // -- 3. the control: without the setting, the same source is NOT centred -
    //
    // 🔑 This half is what makes clause 2 falsifiable. Without it, "L == R"
    // would also pass if the panner were broken, or if the global multiplier
    // were being applied as zero regardless of the project.
    let (pan_mix, _) = boot_and_mix(false);
    let (pl, pr) = channel_energy(&pan_mix);
    assert!(
        pl + pr > 0.01,
        "control precondition: the control mix must be audible too, got L{pl} R{pr}"
    );
    assert!(
        pl > pr * 1.05,
        "with the setting ABSENT, Godot's 0.5 default applies and a hard-left \
         source must still be louder on the LEFT: L{pl} R{pr}. Equal channels \
         here mean the panner never pans, so clause 2 above proves nothing"
    );
}
