//! pat-mgixk.102 — `Geometry2D` / `Geometry3D`: the ClassDB method lists and
//! the polygon boolean, offset, triangulation and plane-clipping family.
//!
//! Every number asserted below was reproduced from the algorithm in a scratch
//! prototype before a line of Rust was written, against the fixtures the bead
//! names: two overlapping unit squares merging to **one 8-point ring of area
//! 1.75**, a square minus an inner square yielding an outer ring plus a
//! **clockwise hole**, a unit square offset by 1 expanding its bounding box by
//! exactly 1 on every side, `triangulate_polygon` of a square returning **6
//! indices**, `bresenham_line((0,0),(3,1))` returning
//! `[(0,0),(1,0),(2,1),(3,1)]`, and a plane halving a quad.
//!
//! # ⚠️ What was already true, and what the registration assertion is for
//!
//! `ClassDB.class_exists("Geometry2D")` was **already true before this
//! change** — measured through the prebuilt runner. `register_core_object_classes`
//! registers both singletons as a bare `ClassRegistration::new("Geometry2D")
//! .parent("Object")` with no methods at all (pat-mgixk.101), so the class
//! existed and its method list was Object's.
//!
//! 🔑 So `class_exists` cannot test the registration. `class_has_method` can,
//! and the discriminating probe is **`triangulate_delaunay`**: a name this
//! change registers in ClassDB but deliberately does NOT implement in the
//! dispatcher. It can only answer true via the new method list — no dispatcher
//! arm, and no amount of working geometry, can make it true. A bogus name is
//! asserted false alongside it, because a `class_has_method` that answered
//! true for everything would satisfy the rest on its own.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// `boot_project` and the `res://` roots are process-global, so serialize.
/// `nextest` isolates each test in its own process and does not need this.
static BOOT_LOCK: Mutex<()> = Mutex::new(());

const DT: f64 = 1.0 / 60.0;

fn boot_lock() -> std::sync::MutexGuard<'static, ()> {
    BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk102_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_project(dir: &Path, gd: &str) {
    std::fs::write(
        dir.join("project.godot"),
        "config_version=5\n\n[application]\n\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.tscn"),
        "[gd_scene load_steps=2 format=3]\n\n\
         [ext_resource type=\"Script\" path=\"res://main.gd\" id=\"1_m\"]\n\n\
         [node name=\"Main\" type=\"Node3D\"]\n\
         script = ExtResource(\"1_m\")\n\n\
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

/// ⚠️ `done` is written LAST. An unserved `Geometry2D` name **raises** rather
/// than answering `Nil` — the dispatcher's fallthrough is
/// `UndefinedFunction("Geometry2D.{method}")` — which aborts `_process` and
/// leaves every later `set_meta` unrun. The flag separates "the methods
/// answered" from "the frame died at the first one".
///
/// 🔑 The rings themselves are recorded, not summary numbers computed in
/// GDScript. Point counts, areas and bounding boxes are then derived in Rust
/// from the engine's own output, so a wrong ring cannot be hidden behind an
/// arithmetic coincidence in the fixture.
///
/// ⚠️ `is_polygon_clockwise` is asked of the ENGINE rather than recomputed in
/// the test. That predicate is the acceptance's actual claim, and its
/// convention is the opposite of the shoelace sign — Godot sums
/// `(x2-x1)*(y2+y1)` and tests `> 0`, which is a NEGATIVE signed area. A test
/// that re-derived it from the points would be asserting my reading of the
/// convention rather than the engine's.
const GD: &str = "\
extends Node3D
var phase = 0

func _process(delta):
    if self.phase != 0:
        return
    self.phase = 1
    var f = get_node(\"Flag\")
    f.set_meta(\"done\", 0)

    f.set_meta(\"exists_2d\", ClassDB.class_exists(\"Geometry2D\"))
    f.set_meta(\"exists_3d\", ClassDB.class_exists(\"Geometry3D\"))
    f.set_meta(\"has_merge\", ClassDB.class_has_method(\"Geometry2D\", \"merge_polygons\"))
    f.set_meta(\"has_delaunay\", ClassDB.class_has_method(\"Geometry2D\", \"triangulate_delaunay\"))
    f.set_meta(\"has_clip3d\", ClassDB.class_has_method(\"Geometry3D\", \"clip_polygon\"))
    f.set_meta(\"has_bogus\", ClassDB.class_has_method(\"Geometry2D\", \"no_such_geometry_method\"))

    var sqa = PackedVector2Array([Vector2(0,0), Vector2(1,0), Vector2(1,1), Vector2(0,1)])
    var sqb = PackedVector2Array([Vector2(0.5,0.5), Vector2(1.5,0.5), Vector2(1.5,1.5), Vector2(0.5,1.5)])

    var merged = Geometry2D.merge_polygons(sqa, sqb)
    f.set_meta(\"merge_rings\", merged.size())
    f.set_meta(\"merge_ring\", merged[0])
    f.set_meta(\"merge_cw\", Geometry2D.is_polygon_clockwise(merged[0]))

    var inter = Geometry2D.intersect_polygons(sqa, sqb)
    f.set_meta(\"inter_rings\", inter.size())
    f.set_meta(\"inter_ring\", inter[0])

    var outer = PackedVector2Array([Vector2(0,0), Vector2(4,0), Vector2(4,4), Vector2(0,4)])
    var inner = PackedVector2Array([Vector2(1,1), Vector2(3,1), Vector2(3,3), Vector2(1,3)])
    var clipped = Geometry2D.clip_polygons(outer, inner)
    f.set_meta(\"clip_rings\", clipped.size())
    f.set_meta(\"clip_a\", clipped[0])
    f.set_meta(\"clip_a_cw\", Geometry2D.is_polygon_clockwise(clipped[0]))
    f.set_meta(\"clip_b\", clipped[1])
    f.set_meta(\"clip_b_cw\", Geometry2D.is_polygon_clockwise(clipped[1]))

    var grown = Geometry2D.offset_polygon(sqa, 1.0)
    f.set_meta(\"offset_rings\", grown.size())
    f.set_meta(\"offset_ring\", grown[0])

    f.set_meta(\"tri\", Geometry2D.triangulate_polygon(sqa).size())
    f.set_meta(\"line\", Geometry2D.bresenham_line(Vector2i(0,0), Vector2i(3,1)))

    var quad = PackedVector3Array([Vector3(0,0,0), Vector3(1,0,0), Vector3(1,1,0), Vector3(0,1,0)])
    f.set_meta(\"clip3d\", Geometry3D.clip_polygon(quad, Plane(Vector3(1,0,0), 0.5)))

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
fn num(boot: &EngineBootstrap, flag: gdscene::node::NodeId, key: &str) -> f64 {
    match meta(boot, flag, key) {
        Variant::Float(f) => f,
        Variant::Int(n) => n as f64,
        other => panic!(
            "`{key}` must be a number; `Nil` means the call that produced it \
             was never dispatched, got {other:?}"
        ),
    }
}

/// Reads a recorded ring.
///
/// ⚠️ Accepts a generic `Array` as well as `PackedVector2Array`: the value
/// travels through `set_meta`/`get_meta`, and pinning the container KIND would
/// fail on a correct ring boxed as an untyped array.
fn ring(boot: &EngineBootstrap, flag: gdscene::node::NodeId, key: &str) -> Vec<(f64, f64)> {
    let items: Vec<Variant> = match meta(boot, flag, key) {
        Variant::PackedVector2Array(v) => {
            return v.into_iter().map(|p| (p.x as f64, p.y as f64)).collect()
        }
        Variant::Array(a) => a.to_vec(),
        other => panic!("`{key}` must be an array of Vector2, got {other:?}"),
    };
    items
        .into_iter()
        .map(|v| match v {
            Variant::Vector2(p) => (p.x as f64, p.y as f64),
            Variant::Vector2i(p) => (p.x as f64, p.y as f64),
            other => panic!("`{key}` must hold Vector2, got {other:?}"),
        })
        .collect()
}

/// Reads a recorded `Vector3` list.
fn ring3(boot: &EngineBootstrap, flag: gdscene::node::NodeId, key: &str) -> Vec<(f64, f64, f64)> {
    let items: Vec<Variant> = match meta(boot, flag, key) {
        Variant::PackedVector3Array(v) => {
            return v
                .into_iter()
                .map(|p| (p.x as f64, p.y as f64, p.z as f64))
                .collect()
        }
        Variant::Array(a) => a.to_vec(),
        other => panic!("`{key}` must be an array of Vector3, got {other:?}"),
    };
    items
        .into_iter()
        .map(|v| match v {
            Variant::Vector3(p) => (p.x as f64, p.y as f64, p.z as f64),
            other => panic!("`{key}` must hold Vector3, got {other:?}"),
        })
        .collect()
}

/// Reads a recorded list of integer cells.
fn cells(boot: &EngineBootstrap, flag: gdscene::node::NodeId, key: &str) -> Vec<(i64, i64)> {
    let items: Vec<Variant> = match meta(boot, flag, key) {
        Variant::Array(a) => a.to_vec(),
        Variant::PackedVector2Array(v) => {
            return v
                .into_iter()
                .map(|p| (p.x.round() as i64, p.y.round() as i64))
                .collect()
        }
        other => panic!("`{key}` must be an array of Vector2i, got {other:?}"),
    };
    items
        .into_iter()
        .map(|v| match v {
            Variant::Vector2i(p) => (p.x as i64, p.y as i64),
            Variant::Vector2(p) => (p.x.round() as i64, p.y.round() as i64),
            other => panic!("`{key}` must hold Vector2i, got {other:?}"),
        })
        .collect()
}

/// Shoelace area of a ring, always positive.
///
/// ⚠️ The magnitude only. The winding is asserted separately, through the
/// engine's own `is_polygon_clockwise`, precisely so this helper does not have
/// to encode a convention.
fn area(ring: &[(f64, f64)]) -> f64 {
    let n = ring.len();
    let mut sum = 0.0;
    for i in 0..n {
        let (x1, y1) = ring[i];
        let (x2, y2) = ring[(i + 1) % n];
        sum += x1 * y2 - x2 * y1;
    }
    (sum * 0.5).abs()
}

fn bbox(ring: &[(f64, f64)]) -> (f64, f64, f64, f64) {
    let mut b = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &(x, y) in ring {
        b.0 = b.0.min(x);
        b.1 = b.1.min(y);
        b.2 = b.2.max(x);
        b.3 = b.3.max(y);
    }
    b
}

#[test]
fn geometry_singletons_full_api() {
    let _guard = boot_lock();
    let dir = unique_project_dir("geometry");
    write_project(&dir, GD);
    let mut boot = boot(&dir);

    let flag = boot
        .main_loop()
        .unwrap()
        .tree()
        .get_node_by_path("/root/Main/Flag")
        .expect("the fixture declares a Flag node");

    for _ in 0..3 {
        boot.main_loop_mut().unwrap().step(DT);
    }

    assert_eq!(
        meta(&boot, flag, "done"),
        Variant::Int(1),
        "`_process` must reach its last line; 0 or Nil means one of the \
         Geometry2D/Geometry3D calls raised `UndefinedFunction` and aborted \
         the frame"
    );

    // -- (0) the ClassDB method lists ---------------------------------------
    assert_eq!(
        meta(&boot, flag, "exists_2d"),
        Variant::Bool(true),
        "`ClassDB.class_exists(\"Geometry2D\")` must be true"
    );
    assert_eq!(
        meta(&boot, flag, "exists_3d"),
        Variant::Bool(true),
        "`ClassDB.class_exists(\"Geometry3D\")` must be true"
    );
    assert_eq!(
        meta(&boot, flag, "has_merge"),
        Variant::Bool(true),
        "`Geometry2D` must advertise `merge_polygons` in ClassDB"
    );
    // 🔑 The discriminator for the registration itself: `triangulate_delaunay`
    // is registered here and deliberately NOT implemented, so no dispatcher arm
    // and no working geometry can make this true — only the method list can.
    assert_eq!(
        meta(&boot, flag, "has_delaunay"),
        Variant::Bool(true),
        "`Geometry2D` must advertise the whole Godot method list, including \
         `triangulate_delaunay`, which this change registers but does not \
         implement — ClassDB is introspection metadata and must match Godot \
         whatever the dispatcher serves"
    );
    assert_eq!(
        meta(&boot, flag, "has_clip3d"),
        Variant::Bool(true),
        "`Geometry3D` must advertise `clip_polygon`"
    );
    // The control: a predicate that answered true for everything would pass
    // all three assertions above while meaning nothing.
    assert_eq!(
        meta(&boot, flag, "has_bogus"),
        Variant::Bool(false),
        "a name Godot does not define must stay absent"
    );

    // -- (1) merge_polygons: one 8-point ring of area 1.75 ------------------
    assert_eq!(
        num(&boot, flag, "merge_rings") as i64,
        1,
        "two overlapping squares merge into a SINGLE ring; two rings means the \
         pieces were never chained together"
    );
    let merged = ring(&boot, flag, "merge_ring");
    assert_eq!(
        merged.len(),
        8,
        "the union outline of two unit squares overlapping at a corner has 8 \
         vertices — 3 original corners from each square plus the 2 crossings; \
         got {merged:?}"
    );
    assert!(
        (area(&merged) - 1.75).abs() < 1e-5,
        "1 + 1 - 0.25 = 1.75: each square has area 1 and they share a 0.5x0.5 \
         corner. An area of 2 means the overlap was counted twice and 1.0 that \
         one square was dropped; got {}",
        area(&merged)
    );
    // Clipper emits outer rings counter-clockwise, which `is_polygon_clockwise`
    // reports as false.
    assert_eq!(
        meta(&boot, flag, "merge_cw"),
        Variant::Bool(false),
        "an outer ring comes back counter-clockwise"
    );

    // -- (2) intersect_polygons: the 0.25 overlap ---------------------------
    //
    // 🔑 Not in the bead's list, but it is the assertion that separates a real
    // boolean engine from one that only knows how to union: the same fixture
    // must yield 0.25 here and 1.75 above.
    assert_eq!(
        num(&boot, flag, "inter_rings") as i64,
        1,
        "the two squares overlap in exactly one region"
    );
    let inter = ring(&boot, flag, "inter_ring");
    assert!(
        (area(&inter) - 0.25).abs() < 1e-5,
        "the shared corner is 0.5 x 0.5 = 0.25; got {}",
        area(&inter)
    );

    // -- (3) clip_polygons: an outer ring plus a CLOCKWISE hole -------------
    assert_eq!(
        num(&boot, flag, "clip_rings") as i64,
        2,
        "a square minus a wholly-contained inner square is an outline AND a \
         hole — one ring means the hole was dropped, which would leave a solid \
         square"
    );
    let (a, b) = (ring(&boot, flag, "clip_a"), ring(&boot, flag, "clip_b"));
    let (a_cw, b_cw) = (
        meta(&boot, flag, "clip_a_cw"),
        meta(&boot, flag, "clip_b_cw"),
    );
    // ⚠️ The rings are identified by AREA, not by index. Which one comes first
    // is an artefact of the order the engine chains segments in, and pinning it
    // would make this test fail on a correct result that happened to be
    // emitted the other way round.
    let (outer_cw, hole, hole_cw) = if area(&a) >= area(&b) {
        (a_cw, b, b_cw)
    } else {
        (b_cw, a, a_cw)
    };
    assert!(
        (area(&hole) - 4.0).abs() < 1e-5,
        "the hole is the 2x2 inner square, area 4; got {}",
        area(&hole)
    );
    // 🔑 THE assertion the bead names. Asked of the engine's own predicate, not
    // recomputed here — Godot's `is_polygon_clockwise` is a NEGATIVE signed
    // area, the opposite sign to the shoelace convention, and this is where
    // getting that backwards would show up.
    assert_eq!(
        hole_cw,
        Variant::Bool(true),
        "a hole must be emitted CLOCKWISE, as Clipper2 does — that winding is \
         what tells a triangulator or a renderer the ring is a hole rather \
         than a second solid island"
    );
    assert_eq!(
        outer_cw,
        Variant::Bool(false),
        "and the ring containing it must wind the other way; two rings of the \
         same winding carry no containment information at all"
    );

    // -- (4) offset_polygon expands the bounding box by delta ---------------
    assert_eq!(
        num(&boot, flag, "offset_rings") as i64,
        1,
        "inflating one convex polygon yields one ring"
    );
    let grown = ring(&boot, flag, "offset_ring");
    let (minx, miny, maxx, maxy) = bbox(&grown);
    assert!(
        (minx + 1.0).abs() < 1e-5
            && (miny + 1.0).abs() < 1e-5
            && (maxx - 2.0).abs() < 1e-5
            && (maxy - 2.0).abs() < 1e-5,
        "a unit square at the origin offset by 1 must span (-1,-1)..(2,2); got \
         ({minx},{miny})..({maxx},{maxy}). An UNCHANGED (0,0)..(1,1) is the \
         signature of edge normals pointing inward — the exact bug the \
         prototype for this caught, from keying the normal on \
         `is_polygon_clockwise` instead of on the shoelace sign"
    );
    // ⚠️ The bounding box, not the point count: Godot's default join is
    // JOIN_SQUARE, which chamfers each corner into two vertices, while
    // JOIN_MITER emits one. The box is the same under either, so asserting it
    // tests the offset without pinning a join style the acceptance does not
    // name.

    // -- (5) triangulate_polygon returns INDICES ----------------------------
    assert_eq!(
        num(&boot, flag, "tri") as i64,
        6,
        "`triangulate_polygon` answers a PackedInt32Array of vertex INDICES, so \
         a square is 2 triangles x 3 indices = 6 entries. A result of 2 would \
         mean triangles were returned instead, and 0 that the ear clipper found \
         no ear"
    );

    // -- (6) bresenham_line, inclusive of both endpoints --------------------
    assert_eq!(
        cells(&boot, flag, "line"),
        vec![(0, 0), (1, 0), (2, 1), (3, 1)],
        "`bresenham_line((0,0),(3,1))` is exactly these four cells, both \
         endpoints included; a 3-cell result means an endpoint was dropped"
    );

    // -- (7) Geometry3D.clip_polygon keeps the half BEHIND the plane --------
    let clipped3 = ring3(&boot, flag, "clip3d");
    assert_eq!(
        clipped3.len(),
        4,
        "halving a quad with a plane through its middle leaves a quad; got \
         {clipped3:?}"
    );
    // 🔑 Which half, not just how many points. The plane is x = 0.5 with normal
    // +x, so the surviving half is x <= 0.5. Keeping the FRONT half would
    // return 4 points too — the same count, the complementary polygon — which
    // is why the x range is asserted and not the size alone.
    let xs: Vec<f64> = clipped3.iter().map(|p| p.0).collect();
    let (xmin, xmax) = (
        xs.iter().cloned().fold(f64::MAX, f64::min),
        xs.iter().cloned().fold(f64::MIN, f64::max),
    );
    assert!(
        xmin.abs() < 1e-5 && (xmax - 0.5).abs() < 1e-5,
        "the kept half must span x in [0, 0.5] — the side the plane's normal \
         points AWAY from, which is what Godot calls inside; got \
         [{xmin}, {xmax}], and [0.5, 1] would be the complement"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
