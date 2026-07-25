//! pat-0q2kx.29 acceptance: `position` / `rotation` / `scale` and `transform`
//! are views over ONE local transform, so a script write is visible downstream.
//!
//! Godot ground truth: `scene/3d/node_3d.cpp` — `set_position`/`set_rotation`/
//! `set_scale`/`set_transform` all mutate a single local-transform state with
//! dirty-flag conversion between the euler and matrix forms, so reading
//! `transform` after writing `position` always reflects the write.
//!
//! Patina stored two competing representations: the `.tscn` parser wrote
//! `transform`, the setters wrote separate keys, and `get_local_transform`
//! preferred `transform` when present. The consequence was silent — on any node
//! authored with a `transform =` line (which is how Godot saves a rotated node),
//! a script assigning `position` changed nothing that rendering or physics could
//! observe. Nothing errored; the node simply never moved.
//!
//! The scene below carries a **rotated** basis precisely so the two
//! representations disagree: a node with an identity basis would pass even with
//! the bug, because the composed and stored transforms would coincide.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

static BOOT_LOCK: Mutex<()> = Mutex::new(());

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_0q2kx29_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `Transform3D(xx,xy,xz, yx,yy,yz, zx,zy,zz, ox,oy,oz)` — three basis columns
/// then the origin. This basis is a 90° rotation about +Y:
/// x → (0,0,-1), y → (0,1,0), z → (1,0,0). Origin starts at (5,6,7).
const MAIN_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://mover.gd" id="1_mover"]

[node name="Main" type="Node3D"]

[node name="Mesh" type="MeshInstance3D" parent="."]
transform = Transform3D(0, 0, -1, 0, 1, 0, 1, 0, 0, 5, 6, 7)
script = ExtResource("1_mover")
"#;

/// Writes each component exactly once, on successive frames, so a later write
/// cannot mask an earlier one failing.
const MOVER_GD: &str = "\
extends MeshInstance3D
var frame = 0

func _process(_delta):
    frame += 1
    if frame == 1:
        self.position = Vector3(10, 20, 30)
";

fn write_project(dir: &Path) {
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"Node3DCoherence\"\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.tscn"), MAIN_TSCN).unwrap();
    std::fs::write(dir.join("mover.gd"), MOVER_GD).unwrap();
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
fn node3d_transform_state_coherence() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("coherence");
    write_project(&dir);

    let mut boot = boot(&dir);
    let mesh = boot
        .main_loop()
        .expect("main loop built")
        .tree()
        .get_node_by_path("/root/Main/Mesh")
        .expect("Mesh instanced");

    // The `.tscn` transform reached the node, and the component views report the
    // real decomposed origin rather than a default. Before this bead the
    // component keys were simply absent and `get_position` returned ZERO on a
    // node that is plainly at (5,6,7).
    {
        let tree = boot.main_loop().unwrap().tree();
        let t = gdscene::node3d::get_local_transform(tree, mesh);
        assert!(
            (t.origin.x - 5.0).abs() < 1e-4
                && (t.origin.y - 6.0).abs() < 1e-4
                && (t.origin.z - 7.0).abs() < 1e-4,
            "the .tscn transform origin must load, got {:?}",
            t.origin
        );
        let p = gdscene::node3d::get_position(tree, mesh);
        assert!(
            (p.x - 5.0).abs() < 1e-4 && (p.y - 6.0).abs() < 1e-4 && (p.z - 7.0).abs() < 1e-4,
            "get_position must report the transform's origin, got {p:?}"
        );
    }

    // One frame: the script assigns `position`.
    boot.main_loop_mut().unwrap().step(0.016);

    let tree = boot.main_loop().unwrap().tree();
    let g = gdscene::node3d::get_global_transform(tree, mesh);

    // ⚠️ The bead's core assertion: the scripted write reaches the canonical
    // transform that rendering and physics read. With the two-representation
    // bug this stayed at the .tscn origin (5,6,7).
    assert!(
        (g.origin.x - 10.0).abs() < 1e-4
            && (g.origin.y - 20.0).abs() < 1e-4
            && (g.origin.z - 30.0).abs() < 1e-4,
        "a scripted `position` write must move the node; global origin is {:?}",
        g.origin
    );

    // ...and the rotation authored in the .tscn survives the position write.
    // Writing `position` must not clobber the basis — which a naive
    // "delete transform and recompose from components" fix would do, since the
    // component keys never carried that rotation.
    assert!(
        (g.basis.x.z - -1.0).abs() < 1e-4
            && (g.basis.y.y - 1.0).abs() < 1e-4
            && (g.basis.z.x - 1.0).abs() < 1e-4,
        "the .tscn 90°-about-Y basis must be preserved, got x={:?} y={:?} z={:?}",
        g.basis.x,
        g.basis.y,
        g.basis.z
    );
}
