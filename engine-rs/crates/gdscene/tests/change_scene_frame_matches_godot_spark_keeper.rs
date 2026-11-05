//! pat-la27y — the frame on which `change_scene_to_file` becomes visible,
//! against real Godot 4.6.1, on the unmodified `games/spark_keeper`.
//!
//! The title screen calls `get_tree().change_scene_to_file("res://scenes/main.tscn")`
//! from `_process` when `ui_accept` is just pressed. The parity gate presses
//! Enter before frame 49 steps. Godot's reference capture records
//! `/root/Main/Player` from a `call_deferred` made by its last-priority
//! `_process`, and it reads `null` at f49 and (320, 180) from f50.
//!
//! What Godot does in that frame (measured with a probe project, Godot
//! 4.6.1, `--fixed-fps 60`; pf = `Engine.get_process_frames()`):
//!
//! ```text
//! T _process pf=2            <- calls change_scene_to_file
//! T _exit_tree pf=2          <- the old scene leaves at once
//! C capture pf=2 main=false  <- MessageQueue flush: neither scene in the tree
//! M _enter_tree pf=2
//! M _ready pf=2              <- `_flush_scene_change`, after the flush
//! M _physics pf=3            <- first physics tick: the NEXT frame
//! M _process pf=3
//! ```
//!
//! So the new scene is readied in the SAME frame (after the deferred-call
//! flush, `scene_tree.cpp` `SceneTree::process`), and first ticks on the next.
//! Patina already ran that order; what differed was where the frame was READ.
//! The MCP `play` probes read the tree after the whole step, i.e. after the
//! scene change, so Patina reported the Player one frame before Godot's
//! capture could see it. They now read at the deferred-call flush
//! (`MainLoop::set_deferred_flush_observer`), the point Godot's capture reads.

use std::sync::{Arc, Mutex};

use gdscene::main_loop::MainLoop;

const PLAYER: &str = "/root/Main/Player";

fn game_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/spark_keeper")
        .canonicalize()
        .expect("games/spark_keeper fixture")
}

/// Boots the project as Play does: autoloads, then the main scene (the title).
fn boot() -> MainLoop {
    let dir = game_dir();
    gdcore::file_access::set_path_roots(Some(dir.clone()), None);
    let booted = gdscene::res_boot::boot_from_res().expect("spark_keeper boots");
    // Scripts reach an autoload by its global name (`Game.high_score`).
    for (name, _) in &booted.autoload_roots {
        gdscript_interop::autoload_registry::register_autoload(name.clone());
    }
    gdscene::project_runtime::seed_script_settings_for_this_thread(&booted.settings, &dir);
    let mut ml = MainLoop::new(booted.tree);
    gdscene::project_runtime::apply_project_settings(&mut ml, &booted.settings);
    ml.set_input_map(booted.input_map);
    for (_, id) in &booted.autoload_roots {
        ml.enter_tree_with_scripts(*id);
    }
    ml.enter_tree_with_scripts(booted.main_scene_root.expect("run/main_scene"));
    ml.register_physics_bodies();
    ml
}

/// Per frame (index = frame): (Player present at the deferred-call flush,
/// Player present after the whole step, Player in group "player" after the
/// step — set by its `_ready`).
fn run(frames: u64) -> Vec<(bool, bool, bool)> {
    let mut ml = boot();
    let seen = Arc::new(Mutex::new(false));
    let out = seen.clone();
    ml.set_deferred_flush_observer(Some(Box::new(move |tree| {
        *out.lock().unwrap() = tree.get_node_by_path(PLAYER).is_some();
    })));
    let mut series = vec![(false, false, false)];
    for f in 1..=frames {
        // Pressed before frame 49 steps, as the gate's `wait:800,Enter:300`.
        if f == 49 {
            ml.input_state_mut().action_press("ui_accept", 1.0);
        }
        *seen.lock().unwrap() = false;
        ml.step(1.0 / 60.0);
        let at_flush = *seen.lock().unwrap();
        let tree = ml.tree();
        let after = tree.get_node_by_path(PLAYER);
        let readied = after.is_some_and(|id| tree.get_nodes_in_group("player").contains(&id));
        series.push((at_flush, after.is_some(), readied));
        if f == 66 {
            ml.input_state_mut().action_release("ui_accept");
        }
    }
    // ⚠️ Script errors are not asserted: this bare boot does not bind the
    // `Game` autoload's methods the way the editor's Play does
    // (`Game.reset()` in main.gd fails with "method not found"), and nothing
    // asserted here depends on main.gd's `_ready`.
    series
}

#[test]
fn the_new_scene_is_readied_in_the_change_frame_and_seen_from_the_next() {
    let s = run(52);
    for f in 1..=48 {
        assert_eq!(s[f], (false, false, false), "f{f}: still the title screen");
    }
    // f49: `_process` changes the scene. At the flush (Godot's capture point)
    // the Player is not there yet — Godot reads null — but by the end of the
    // frame the scene has entered and its `_ready` has run, as in Godot.
    assert_eq!(
        s[49],
        (false, true, true),
        "f49 (at flush, after step, readied): Godot reads null at its capture \
         point and readies the scene later in the same frame"
    );
    // f50: Godot's capture sees (320, 180).
    assert_eq!(
        s[50],
        (true, true, true),
        "f50: Godot's capture sees the Player"
    );
}
