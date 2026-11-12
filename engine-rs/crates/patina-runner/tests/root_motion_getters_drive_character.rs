//! pat-mgixk.78 acceptance: a GDScript reads root motion from the mixer and
//! applies it itself, and the body moves by the clip's displacement **once**.
//!
//! # What was wrong
//!
//! `SceneTree::process_animations` added each root-motion delta straight onto
//! the mixer root's `position` (`apply_root_motion_delta`). Godot's
//! `AnimationMixer` never moves the node: it cancels the root-motion track from
//! the visual pose, publishes the motion through
//! `get_root_motion_position/rotation/scale` and the `*_accumulator` getters, and
//! leaves the user to apply it — the canonical shape being
//! `velocity = mixer.get_root_motion_position() / delta` followed by
//! `move_and_slide()` (`AnimationMixer.xml:323-326`).
//!
//! So a ported controller doing exactly that moved **twice**: once because the
//! engine applied the delta, and once more because the script did. And the
//! getters did not exist at all — `grep get_root_motion_position` over
//! `gdscene/src` and `gdscript-interop/src` returned 0 hits — so there was no
//! supported way to write the controller Godot documents.
//!
//! # Why "exactly once" is the whole point, and how this test pins it
//!
//! 🔑 The clip travels 4.0 along +X over 2 seconds, i.e. **2 m/s**. The script
//! converts the per-frame delta back into a velocity and lets
//! `move_and_slide()` do the moving. After one second the body must be at
//! **x ≈ 2.0**.
//!
//! Under the old auto-apply the same script would land at **x ≈ 4.0** — the
//! engine's contribution plus the script's. The two outcomes differ by a factor
//! of two, so this is a one-bit answer and it fails loudly in the direction the
//! bead names. An absolute position is safe to assert here precisely because the
//! fixture pins the rate: nothing else in the scene moves the body.
//!
//! # Why the script writes `velocity` whole
//!
//! ⚠️ Node properties are flat, so `velocity.x = ...` writes a literal
//! `"velocity.x"` property that nothing reads, and the test would pass or fail
//! for reasons unrelated to this bead. The reference acceptance for
//! `move_and_slide` (`physics3d_character_move_and_slide.rs`) documents the same
//! trap.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

static BOOT_LOCK: Mutex<()> = Mutex::new(());

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk78_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A capsule on a floor, with an `AnimationPlayer` whose root-motion track
/// travels +X at a constant 2 m/s.
///
/// ⚠️ **Both path spellings below are load-bearing, and the obvious forms are
/// wrong in ways that fail SILENTLY.**
///
/// `root_motion_track` must be a `NodePath` **with a subname**: the loader keeps
/// only `np.get_concatenated_subnames()` and ignores the value entirely when
/// that is empty (`animation_tscn.rs:723-727`). So a bare
/// `NodePath("position")` has no subname, leaves `root_motion_track` at `None`,
/// and every getter reports zero. `NodePath(".:position")` yields the
/// `"position"` the matcher wants.
///
/// The TRACK path uses the **String** form `":position"`, which the loader
/// documents for hand-written scenes and splits on the colon into
/// `node_path = ""` + `property_path = "position"`
/// (`animation_tscn.rs:128-131`) — exactly the pair the proven Rust fixture in
/// `animation_root_motion_method_test.rs` builds with
/// `AnimationTrack::with_type("", "position", ..)`. A `NodePath("position")`
/// here would instead give `node_path = "position"` with an EMPTY property
/// path, which then falls to `implicit_property_path` and drops the track.
///
/// Both matter for the same reason: `consume_root_motion` and
/// `root_motion_frame` match `track.property_path == root_motion_track`, so a
/// mismatch is not an error — it is zero motion and a vacuously passing test.
///
/// The clip is `length = 2.0` with keys at 0 and 2, so one second of ticks lands
/// squarely mid-clip: no end-of-clip stop and no loop wrap to invert the delta.
const MAIN_TSCN: &str = r#"[gd_scene load_steps=4 format=3]

[ext_resource type="Script" path="res://player.gd" id="1_player"]

[sub_resource type="Animation" id="Animation_walk"]
resource_name = "walk"
length = 2.0
loop_mode = 0
tracks/0/type = "value"
tracks/0/imported = false
tracks/0/enabled = true
tracks/0/path = ":position"
tracks/0/interp = 1
tracks/0/loop_wrap = true
tracks/0/keys = {
"times": PackedFloat32Array(0, 2),
"transitions": PackedFloat32Array(1, 1),
"update": 0,
"values": [Vector3(0, 0, 0), Vector3(4, 0, 0)]
}

[sub_resource type="AnimationLibrary" id="AnimationLibrary_main"]
_data = {
"walk": SubResource("Animation_walk")
}

[node name="Main" type="Node3D"]

[node name="Floor" type="StaticBody3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0)

[node name="FloorShape" type="CollisionShape3D" parent="Floor"]
shape = "BoxShape3D"
size = Vector3(40, 1, 40)

[node name="Player" type="CharacterBody3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1.5, 0)
script = ExtResource("1_player")

[node name="PlayerShape" type="CollisionShape3D" parent="Player"]
shape = "CapsuleShape3D"
radius = 0.5
height = 2.0

[node name="Anim" type="AnimationPlayer" parent="Player"]
libraries = {
"": SubResource("AnimationLibrary_main")
}
autoplay = "walk"
root_motion_track = NodePath(".:position")
"#;

/// The controller Godot's own documentation describes.
///
/// `travel` and `accum` are script variables, which sync onto the node as
/// properties after `_physics_process` — so the assertions below read what
/// GDScript itself saw, not what Rust could compute independently.
const PLAYER_GD: &str = "\
extends CharacterBody3D
var ticks = 0
var travel = 0.0
var accum = 0.0

func _physics_process(delta):
    ticks += 1
    var rm = $Anim.get_root_motion_position()
    travel += rm.x
    accum = $Anim.get_root_motion_position_accumulator().x
    if delta > 0.0:
        velocity = Vector3(rm.x / delta, 0.0, 0.0)
    move_and_slide()
";

fn write_project(dir: &Path) {
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"RootMotion\"\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.tscn"), MAIN_TSCN).unwrap();
    std::fs::write(dir.join("player.gd"), PLAYER_GD).unwrap();
}

fn boot(dir: &Path) -> EngineBootstrap {
    let config = BootConfig {
        main_scene: Some(dir.join("main.tscn")),
        project_dir: dir.to_path_buf(),
        headless: true,
        ..BootConfig::default()
    };
    let mut boot = EngineBootstrap::new(config);
    boot.run_all().expect("boot succeeds");
    boot
}

/// **The acceptance test.**
#[test]
fn root_motion_getters_drive_character() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("getters");
    write_project(&dir);

    let mut boot = boot(&dir);

    // One second at 60 Hz: mid-clip throughout, so the rate stays a constant
    // 2 m/s and the expected displacement is exactly 2.0.
    for _ in 0..60 {
        boot.main_loop_mut().unwrap().step(1.0 / 60.0);
    }

    let tree = boot.main_loop().unwrap().tree();
    let player = tree
        .get_node_by_path("/root/Main/Player")
        .expect("Player instanced");
    let node = tree.get_node(player).expect("player node");

    // -- precondition: the script ran, and the getters answered --------------
    //
    // 🔑 This clause exists because of how a MISSING getter fails. Declining a
    // method in `animation_player_method` does not return Nil — it falls
    // through to `UndefinedFunction`, which ABORTS the calling frame. So a
    // script whose `get_root_motion_position()` is unimplemented stops at that
    // line every tick: `ticks` still increments, but `travel` stays 0 and
    // nothing downstream runs. Asserting both separates "the getters work" from
    // "the script ran at all".
    match node.get_property("ticks") {
        Variant::Int(n) => assert!(n >= 50, "_physics_process should run ~60 times, got {n}"),
        other => panic!("script var `ticks` should be an Int, got {other:?}"),
    }
    match node.get_property("travel") {
        Variant::Float(t) => assert!(
            t > 1.5,
            "the script must have read a non-zero per-frame delta from \
             `get_root_motion_position()`; summed {t} over the second. A 0 here \
             means the getter declined and aborted the frame at that line"
        ),
        other => panic!("script var `travel` should be a Float, got {other:?}"),
    }

    // -- 1. 🔑 the body moved by the clip's displacement EXACTLY ONCE --------
    //
    // 2 m/s for one second = 2.0. Under the old auto-apply this same script
    // landed at ~4.0: the engine's delta plus the script's. A factor of two is
    // not a tolerance question.
    let pos = gdscene::node3d::get_global_transform(tree, player).origin;
    assert!(
        (pos.x - 2.0).abs() < 0.25,
        "the body must travel the clip's 2.0 — once, not twice. Got x={}. \
         About 4.0 means the engine is STILL applying root motion on top of the \
         script (the double-move this bead removes); about 0.0 means the script \
         never moved it at all",
        pos.x
    );

    // -- 2. the accumulator agrees with the summed deltas -------------------
    //
    // Read through `get_root_motion_position_accumulator()` by the script
    // itself, and compared against that same script's running sum of the
    // per-frame deltas — so this is the engine agreeing with itself across two
    // different getters rather than against a number I predicted.
    let (travel, accum) = match (node.get_property("travel"), node.get_property("accum")) {
        (Variant::Float(t), Variant::Float(a)) => (t, a),
        other => panic!("script vars `travel`/`accum` should be Floats, got {other:?}"),
    };
    assert!(
        (accum - travel).abs() < 1e-3,
        "`get_root_motion_position_accumulator()` must equal the summed \
         per-frame deltas: accumulator {accum} vs summed {travel}"
    );
}
