//! pat-0q2kx.32 acceptance: a `RayCast2D`'s script getters report a real,
//! globally-transformed, filtered cast after `MainLoop::step`.
//!
//! Godot ground truth: `scene/2d/physics/ray_cast_2d.cpp::_update_raycast_state`
//! runs each physics frame from `global_position` to
//! `global_transform.xform(target_position)`, filtered by `collision_mask`, with
//! the parent `CollisionObject2D` excluded by default.
//!
//! Patina had both halves and no seam between them. `process_raycasts` cast from
//! the node's LOCAL position along the raw `target_position` with the unfiltered
//! `world.raycast`, then published `is_colliding`/`collider`. The GDScript bridge
//! reads `_hit_colliding`/`_hit_collider`/`_hit_point`/`_hit_normal`, which in 2D
//! nothing ever wrote. The failure was completely silent: `$Ray.is_colliding()`
//! returned `false` forever, exactly as it would for a ray genuinely pointing at
//! empty space.
//!
//! The ray below is rotated a quarter turn by its parent, so a cast that ignores
//! the global transform doesn't merely land in a slightly wrong place — it aims
//! along +X into an empty scene while the wall sits along +Y.

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
        "patina_0q2kx32_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `Player` sits at (100,0) rotated +90°, so its local +X is world +Y. The ray
/// aims at local (50,0) — i.e. world (100,0) → (100,50). The wall spans
/// y ∈ [35,45] across that path; nothing at all lies along world +X.
const MAIN_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://probe.gd" id="1_probe"]

[node name="Main" type="Node2D"]

[node name="Player" type="CharacterBody2D" parent="."]
position = Vector2(100, 0)
rotation = 1.5707963267948966
script = ExtResource("1_probe")

[node name="PlayerShape" type="CollisionShape2D" parent="Player"]
radius = 10.0

[node name="Ray" type="RayCast2D" parent="Player"]
position = Vector2(0, 0)
target_position = Vector2(50, 0)

[node name="Wall" type="StaticBody2D" parent="."]
position = Vector2(100, 40)

[node name="WallShape" type="CollisionShape2D" parent="Wall"]
size = Vector2(200, 10)
"#;

/// Script variables sync onto the node as properties after `_process`, so these
/// assignments are how the test observes what GDScript actually saw.
const PROBE_GD: &str = "\
extends CharacterBody2D
var ray_hit = false
var hit_x = 0.0
var hit_y = 0.0

func _process(_delta):
    var r = $Ray
    ray_hit = r.is_colliding()
    var p = r.get_collision_point()
    hit_x = p.x
    hit_y = p.y
";

/// Identical geometry, but the ray only accepts layer 2 while the wall is on
/// Godot's default layer 1.
const MASKED_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://probe.gd" id="1_probe"]

[node name="Main" type="Node2D"]

[node name="Player" type="CharacterBody2D" parent="."]
position = Vector2(100, 0)
rotation = 1.5707963267948966
script = ExtResource("1_probe")

[node name="PlayerShape" type="CollisionShape2D" parent="Player"]
radius = 10.0

[node name="Ray" type="RayCast2D" parent="Player"]
position = Vector2(0, 0)
target_position = Vector2(50, 0)
collision_mask = 2

[node name="Wall" type="StaticBody2D" parent="."]
position = Vector2(100, 40)

[node name="WallShape" type="CollisionShape2D" parent="Wall"]
size = Vector2(200, 10)
"#;

fn write_project(dir: &Path, tscn: &str) {
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"RayCast2DGetters\"\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.tscn"), tscn).unwrap();
    std::fs::write(dir.join("probe.gd"), PROBE_GD).unwrap();
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
    // The bootstrap does not populate the physics world; the runner does this
    // right after loading the scene (see `patina-runner/src/main.rs`).
    boot.main_loop_mut()
        .expect("main loop built")
        .register_physics_bodies();
    boot
}

/// **The acceptance test.**
#[test]
fn raycast2d_script_getters_global_transform() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("getters");
    write_project(&dir, MAIN_TSCN);

    let mut boot = boot(&dir);
    boot.main_loop_mut().unwrap().step(1.0 / 60.0);

    let tree = boot.main_loop().unwrap().tree();
    let player = tree
        .get_node_by_path("/root/Main/Player")
        .expect("Player instanced");
    let ray = tree
        .get_node_by_path("/root/Main/Player/Ray")
        .expect("Ray instanced");
    let wall = tree
        .get_node_by_path("/root/Main/Wall")
        .expect("Wall instanced");

    // ⚠️ The bead's core assertion, read back through GDScript rather than from
    // the node properties directly: `$Ray.is_colliding()` is true.
    let script_hit = tree.get_node(player).unwrap().get_property("ray_hit");
    assert!(
        matches!(script_hit, Variant::Bool(true)),
        "`$Ray.is_colliding()` must be true against the wall in the ray's \
         ROTATED global direction; got {script_hit:?}"
    );

    // `get_collision_point()` matches the wall's near face: the ray travels
    // world +Y from (100,0), and the wall spans y ∈ [35,45].
    let (hx, hy) = (
        tree.get_node(player).unwrap().get_property("hit_x"),
        tree.get_node(player).unwrap().get_property("hit_y"),
    );
    match (hx, hy) {
        (Variant::Float(x), Variant::Float(y)) => {
            assert!(
                (x - 100.0).abs() < 0.5 && (y - 35.0).abs() < 0.5,
                "`get_collision_point()` should be the wall's top face (100,35), got ({x},{y})"
            );
        }
        other => panic!("script should have read a Vector2 collision point, got {other:?}"),
    }

    // The parent CharacterBody2D is excluded by default, so the ray reports the
    // wall — not the body it is attached to and standing inside of.
    let collider = tree.get_node(ray).unwrap().get_property("_hit_collider");
    match collider {
        Variant::ObjectId(oid) => {
            assert_eq!(
                oid,
                wall.object_id(),
                "the collider must be the Wall, not the ray's own parent body"
            );
            assert_ne!(
                oid,
                player.object_id(),
                "`exclude_parent` must keep the parent body out of the result"
            );
        }
        other => panic!("`_hit_collider` should be an ObjectId handle, got {other:?}"),
    }
}

/// A mask sharing no bit with the wall's layer yields no hit — proving the cast
/// is filtered, not merely aimed correctly. Same geometry as above, so a failure
/// here is about the mask alone.
#[test]
fn raycast2d_script_getters_mask_mismatch_misses() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("masked");
    write_project(&dir, MASKED_TSCN);

    let mut boot = boot(&dir);
    boot.main_loop_mut().unwrap().step(1.0 / 60.0);

    let tree = boot.main_loop().unwrap().tree();
    let player = tree
        .get_node_by_path("/root/Main/Player")
        .expect("Player instanced");

    let script_hit = tree.get_node(player).unwrap().get_property("ray_hit");
    assert!(
        matches!(script_hit, Variant::Bool(false)),
        "a layer-2 ray must not see the layer-1 wall; got {script_hit:?}"
    );
}
