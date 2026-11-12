//! pat-mgixk.92 — the `Camera3D` projection family, called from a script,
//! against the project's configured viewport.
//!
//! `call_camera3d_method` (`camera3d.rs:1182`) reads
//! `tree.root_window().visible_size()` — deliberately the *visible* rect rather
//! than the raw window, because that is what Godot's own
//! `get_viewport()->get_visible_rect().size` gives and the two differ once
//! content scaling is on. The bead was filed when only 4 of 19 methods were
//! served; the projection family is served now, and this is the acceptance it
//! named, which did not exist.
//!
//! # Why the window size is asserted FIRST
//!
//! 🔑 Every number below is a function of the viewport size, so a wrong size
//! produces wrong-but-plausible answers rather than an error. Measured through
//! `patina-runner`'s own boot path, `unproject_position(Vector3.ZERO)` comes
//! back as **(576, 324)** — exactly half of Godot's default 1152x648 — because
//! that path never applies `display/window/size/viewport_*` to the root window
//! (`project_boot.rs` sets no window size; only
//! `patina_runner::bootstrap` does, at `bootstrap.rs:461`). Reported separately.
//!
//! So this test asserts the viewport is 640x480 **before** asserting anything
//! derived from it. Without that, a future regression in the size plumbing
//! would surface here as a baffling off-centre projection instead of naming
//! itself.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// `boot_project` mutates the process-global `res://` roots, so serialize under
/// the single-process runner. `nextest` isolates per process.
static BOOT_LOCK: Mutex<()> = Mutex::new(());

const DT: f64 = 1.0 / 60.0;
const W: f32 = 640.0;
const H: f32 = 480.0;

/// Takes the boot lock, tolerating a previous test's panic, so one failure does
/// not turn every later test into an opaque `PoisonError`.
fn boot_lock() -> std::sync::MutexGuard<'static, ()> {
    BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk92_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A 640x480 project whose camera sits at `(0, 0, 5)` with an identity basis,
/// so it looks down `-Z` straight at the origin.
///
/// ⚠️ No `window/stretch/*`: with scaling disabled `visible_size()` equals the
/// window size, which keeps the expected numbers readable. A stretched project
/// would project against the base resolution instead, and that is a different
/// (also correct) answer.
fn write_project(dir: &Path, gd: &str) {
    std::fs::write(
        dir.join("project.godot"),
        "config_version=5\n\n\
         [application]\n\n\
         run/main_scene=\"res://main.tscn\"\n\n\
         [display]\n\n\
         window/size/viewport_width=640\n\
         window/size/viewport_height=480\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.tscn"),
        "[gd_scene load_steps=2 format=3]\n\n\
         [ext_resource type=\"Script\" path=\"res://main.gd\" id=\"1_m\"]\n\n\
         [node name=\"Main\" type=\"Node3D\"]\n\
         script = ExtResource(\"1_m\")\n\n\
         [node name=\"Cam\" type=\"Camera3D\" parent=\".\"]\n\
         transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 5)\n\
         current = true\n\n\
         [node name=\"Flag\" type=\"Node3D\" parent=\".\"]\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.gd"), gd).unwrap();
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

/// ⚠️ `done` is written LAST. An unserved registration that returns a VALUE
/// raises rather than answering `Nil`
/// (`gdscript-interop/src/native_method_gaps.rs`), which aborts `_process` — so
/// the flag separates "the projection methods answered" from "the frame died at
/// the first one".
///
/// ⚠️ Components are recorded individually rather than as whole vectors: the
/// comparisons below need a tolerance, and a `Vector2`/`Vector3` equality on a
/// float derived from a projection would be brittle for no gain.
const GD: &str = "\
extends Node3D
var phase = 0

func _process(delta):
    if self.phase != 0:
        return
    self.phase = 1
    var f = get_node(\"Flag\")
    var cam = get_node(\"Cam\")
    f.set_meta(\"done\", 0)
    var sp = cam.unproject_position(Vector3(0, 0, 0))
    f.set_meta(\"sx\", sp.x)
    f.set_meta(\"sy\", sp.y)
    var n = cam.project_ray_normal(Vector2(320, 240))
    f.set_meta(\"nx\", n.x)
    f.set_meta(\"ny\", n.y)
    f.set_meta(\"nz\", n.z)
    var o = cam.project_ray_origin(Vector2(320, 240))
    f.set_meta(\"oz\", o.z)
    f.set_meta(\"behind\", cam.is_position_behind(Vector3(0, 0, 10)))
    f.set_meta(\"infront\", cam.is_position_behind(Vector3(0, 0, -10)))
    cam.set_orthogonal(10, 0.1, 100)
    f.set_meta(\"projection\", cam.projection)
    f.set_meta(\"done\", 1)
";

fn meta(boot: &EngineBootstrap, flag: gdscene::node::NodeId, key: &str) -> Variant {
    boot.main_loop()
        .unwrap()
        .tree()
        .get_node(flag)
        .expect("the Flag node is still in the tree")
        .get_meta(key)
}

/// Reads a recorded number, accepting either numeric Variant.
///
/// ⚠️ Not pinned to `Float`: a whole-numbered component can box as `Int`, and
/// asserting the Variant KIND would fail on a correct value of 320.
fn num(boot: &EngineBootstrap, flag: gdscene::node::NodeId, key: &str) -> f64 {
    match meta(boot, flag, key) {
        Variant::Float(f) => f,
        Variant::Int(n) => n as f64,
        other => panic!(
            "`{key}` must be recorded as a number; `Nil` means the call that \
             produced it was never dispatched, got {other:?}"
        ),
    }
}

#[test]
fn gdscript_camera3d_projection_methods() {
    let _guard = boot_lock();
    let dir = unique_project_dir("camera3d");
    write_project(&dir, GD);
    let mut boot = boot(&dir);

    let flag = boot
        .main_loop()
        .unwrap()
        .tree()
        .get_node_by_path("/root/Main/Flag")
        .expect("the fixture declares a Flag node");

    // -- (0) the viewport the projections are computed against ---------------
    //
    // 🔑 Asserted before anything derived from it. See the module note: the
    // runner's own boot path leaves this at Godot's 1152x648 default, which
    // turns every number below into a plausible wrong answer.
    let visible = boot
        .main_loop()
        .unwrap()
        .tree()
        .root_window()
        .visible_size();
    assert!(
        (visible.x - W).abs() < 0.5 && (visible.y - H).abs() < 0.5,
        "the root window's visible size must be the project's 640x480 before \
         any projection is meaningful; got {visible:?}. 1152x648 means \
         `display/window/size/viewport_*` never reached the root window"
    );

    for _ in 0..3 {
        boot.main_loop_mut().unwrap().step(DT);
    }

    // -- the frame survived every call ---------------------------------------
    assert_eq!(
        meta(&boot, flag, "done"),
        Variant::Int(1),
        "`_process` must reach its last line; 0 or Nil means one of the \
         projection methods raised and aborted the frame, which is what an \
         unserved value-returning registration does"
    );

    // -- (1) unproject_position puts the origin at the screen centre ---------
    let (sx, sy) = (num(&boot, flag, "sx"), num(&boot, flag, "sy"));
    assert!(
        (sx - 320.0).abs() < 1e-3 && (sy - 240.0).abs() < 1e-3,
        "a camera at (0,0,5) with an identity basis looks straight at the \
         origin, so `unproject_position(Vector3.ZERO)` is the centre of a \
         640x480 viewport — (320, 240); got ({sx}, {sy})"
    );

    // -- (2) the centre ray points down -Z -----------------------------------
    let (nx, ny, nz) = (
        num(&boot, flag, "nx"),
        num(&boot, flag, "ny"),
        num(&boot, flag, "nz"),
    );
    assert!(
        nx.abs() < 1e-3 && ny.abs() < 1e-3 && (nz + 1.0).abs() < 1e-3,
        "`project_ray_normal` at the viewport centre must be (0, 0, -1); got \
         ({nx}, {ny}, {nz}). An off-axis result means the screen point was \
         divided by a viewport of some other size"
    );
    assert!(
        (num(&boot, flag, "oz") - 5.0).abs() < 1e-3,
        "`project_ray_origin` at the centre starts at the camera, z = 5"
    );

    // -- (3) is_position_behind, both directions -----------------------------
    //
    // 🔑 Both, because a predicate stuck at `true` passes the first on its own.
    assert_eq!(
        meta(&boot, flag, "behind"),
        Variant::Bool(true),
        "the camera sits at z = 5 looking -Z, so (0,0,10) is BEHIND it"
    );
    assert_eq!(
        meta(&boot, flag, "infront"),
        Variant::Bool(false),
        "(0,0,-10) is in front, and a constant-`true` predicate would pass the \
         assertion above while meaning nothing"
    );

    // -- (4) set_orthogonal switches the projection mode ---------------------
    assert_eq!(
        meta(&boot, flag, "projection"),
        Variant::Int(1),
        "`set_orthogonal(10, 0.1, 100)` must leave `projection` at \
         PROJECTION_ORTHOGONAL (1)"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
