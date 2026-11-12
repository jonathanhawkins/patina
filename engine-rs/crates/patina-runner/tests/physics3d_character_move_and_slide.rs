//! pat-0q2kx.36 acceptance: a `CharacterBody3D` driven by GDScript actually
//! moves, lands, and is stopped by geometry — through real `MainLoop` ticks on a
//! scene loaded from `.tscn` text.
//!
//! Godot ground truth: `scene/3d/physics/character_body_3d.cpp` — a script calls
//! `move_and_slide()` each `_physics_process`; the engine consumes `velocity`,
//! slides along collisions, and publishes `is_on_floor` / `is_on_wall` /
//! `is_on_ceiling`.
//!
//! `gdphysics3d::character::move_and_slide` has been complete all along —
//! sub-stepping, slope snapping, platform carry — and nothing ever called it.
//! `MainLoop`'s 3D physics phase was sync → step → sync, and only the 2D server
//! had a character pass. The failure was completely silent: a CharacterBody3D
//! sat exactly where it was authored, `is_on_floor()` answered `false` forever,
//! and no error distinguished that from a character correctly standing still.
//!
//! The script below assigns `velocity` as a WHOLE Vector3 rather than mutating
//! `velocity.y` in place: node properties are flat, so a component write to a
//! property path silently no-ops and the test would pass or fail for reasons
//! unrelated to the bead.

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
        "patina_0q2kx36_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Floor top surface is y = 0.5 (centre 0, half-height 0.5). The player is a
/// 2m-tall capsule dropped from y = 3, so it falls 1.5m and rests at y = 1.5.
/// The wall's near face is x = 4.5; a 0.5m-radius capsule stops at x = 4.
/// NOTE ON SHAPE AUTHORING: the colliders below use the class-name + dimension
/// convention (`shape = "BoxShape3D"` with `size` on the node) rather than
/// `shape = SubResource("...")`, which keeps THIS test about character movement
/// rather than about shape loading.
///
/// This note used to add that `shape_from_node_3d` had no `SubResource:<id>` arm
/// and that an editor-authored 3D scene therefore "resolves to no shapes at
/// all". **That gap has since been closed** — `physics_server_3d.rs` resolves a
/// `SubResource:` reference through the `__shape__class` companion the packed
/// scene copies onto the node, mirroring the 2D path. Either convention works
/// now; this one is simply the more direct to author inline.
const MAIN_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://player.gd" id="1_player"]

[node name="Main" type="Node3D"]

[node name="Floor" type="StaticBody3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0)

[node name="FloorShape" type="CollisionShape3D" parent="Floor"]
shape = "BoxShape3D"
size = Vector3(40, 1, 40)

[node name="Wall" type="StaticBody3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 5, 2, 0)

[node name="WallShape" type="CollisionShape3D" parent="Wall"]
shape = "BoxShape3D"
size = Vector3(1, 4, 8)

[node name="Player" type="CharacterBody3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 3, 0)
script = ExtResource("1_player")

[node name="PlayerShape" type="CollisionShape3D" parent="Player"]
shape = "CapsuleShape3D"
radius = 0.5
height = 2.0
"#;

/// Accumulates gravity into a script-local `vy` and writes `velocity` whole.
/// Script variables sync onto the node as properties after `_physics_process`,
/// which is how the test observes what GDScript itself saw.
const PLAYER_GD: &str = "\
extends CharacterBody3D
var vy = 0.0
var on_floor_seen = false
var ticks = 0

func _physics_process(delta):
    ticks += 1
    if is_on_floor():
        on_floor_seen = true
        vy = 0.0
    else:
        vy -= 9.8 * delta
    velocity = Vector3(5.0, vy, 0.0)
    move_and_slide()
";

fn write_project(dir: &Path) {
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"Char3D\"\nrun/main_scene=\"res://main.tscn\"\n",
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
fn physics3d_character_move_and_slide_e2e() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("charslide");
    write_project(&dir);

    let mut boot = boot(&dir);

    // 2 seconds. The 1.5m drop lands in ~0.55s, and 5 m/s covers the 4m to the
    // wall in ~0.8s, so both events are comfortably inside the window.
    for _ in 0..120 {
        boot.main_loop_mut().unwrap().step(1.0 / 60.0);
    }

    let tree = boot.main_loop().unwrap().tree();
    let player = tree
        .get_node_by_path("/root/Main/Player")
        .expect("Player instanced");
    let node = tree.get_node(player).expect("player node");

    // ⚠️ The script's own view: `is_on_floor()` became true at some point. This
    // is the property the GDScript bridge reads, and nothing in 2D or 3D wrote
    // it for a CharacterBody3D before this bead.
    assert!(
        matches!(node.get_property("on_floor_seen"), Variant::Bool(true)),
        "the script's `is_on_floor()` must report true once the capsule lands; \
         got {:?}",
        node.get_property("on_floor_seen")
    );

    // The script really did run every tick, so a false pass from a script that
    // never executed is ruled out.
    match node.get_property("ticks") {
        Variant::Int(n) => assert!(
            n >= 100,
            "_physics_process should have run ~120 times, got {n}"
        ),
        other => panic!("script var `ticks` should be an Int, got {other:?}"),
    }

    let pos = gdscene::node3d::get_global_transform(tree, player).origin;

    // Landed on the floor rather than falling through it: the capsule's centre
    // rests at floor-top (0.5) + half-height (1.0) = 1.5.
    assert!(
        (pos.y - 1.5).abs() < 0.35,
        "the capsule must come to rest on the floor at y≈1.5, got {:?}",
        pos
    );

    // Travelled +X under its own velocity...
    assert!(
        pos.x > 3.0,
        "the character must travel +X across the floor, got x={} — x≈0 means \
         move_and_slide never ran",
        pos.x
    );
    // ...and was stopped by the wall instead of penetrating it. The wall's near
    // face is x=4.5 and the capsule radius is 0.5, so it rests at x≈4.
    assert!(
        pos.x < 4.6,
        "the character must be stopped by the wall at x=4.5, got x={} — it \
         penetrated the wall",
        pos.x
    );
}
