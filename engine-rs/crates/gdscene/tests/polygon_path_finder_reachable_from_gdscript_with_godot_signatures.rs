//! pat-s3rrv.240 — `PolygonPathFinder` is reachable from a game script.
//!
//! # The gap
//!
//! `gdscene::polygon_path_finder` has been a complete visibility-graph
//! pathfinder — setup, Dijkstra over a properly-crossing-segment visibility
//! test, point penalties, even-odd inside test — with its own unit tests and
//! **no route in**. It was not in ClassDB, not constructible from a script, not
//! loadable from a `.tres`; its only reference anywhere was the `pub mod` line.
//! A finished engine behind a green test suite that no game could call.
//!
//! # Why the bindings live in `gdscene` rather than the interpreter
//!
//! The type is a `gdscene` type, and `gdscene` depends on `gdscript-interop`,
//! not the reverse — so the interpreter cannot name it. It goes through the
//! same `SceneAccess` seam AStar and the mesh builders use: the scene layer owns
//! the instance and hands back a tagged handle.
//!
//! That seam is also where Godot's signatures are adapted, and the two
//! adaptations are the substance of this bead:
//!
//! * `setup(points, connections)` takes a **flat** `PackedInt32Array` of index
//!   pairs, while the Rust module takes `&[(usize, usize)]`.
//! * `get_bounds()` returns a `Rect2`, while the module reports a min/max pair.

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Reads a script `var` off the attached instance.
///
/// Script vars and node properties are separate stores, so `get_property` would
/// answer `Nil` here and fail for the wrong reason.
fn script_var(tree: &SceneTree, node: NodeId, name: &str) -> Variant {
    tree.get_script(node)
        .expect("script attached")
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("script has no var `{name}`"))
        .default_value
}

/// Attaches `src` to a fresh tree and runs `run()` against the real accessor.
fn run(src: &str) -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree
        .add_child(root, Node::new("Finder", "Node"))
        .expect("child adds");
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(src, node).expect("script parses")),
    );
    assert!(
        tree.call_script_method(node, "run", &[]).is_some(),
        "run() raised — PolygonPathFinder.new() or one of its methods did not \
         resolve. Before this bead the constructor raised an unknown-class \
         TypeError, which is exactly this failure."
    );
    (tree, node)
}

/// A unit square obstacle at (10,10)-(20,20), as four corners and four flat
/// boundary edges — the `PackedInt32Array` spelling Godot uses.
const SQUARE: &str = "\
var pts = PackedVector2Array([Vector2(10, 10), Vector2(20, 10), Vector2(20, 20), Vector2(10, 20)])
    var cons = PackedInt32Array([0, 1, 1, 2, 2, 3, 3, 0])
";

// ---------------------------------------------------------------------------
// The bead's acceptance
// ---------------------------------------------------------------------------

/// **The acceptance test.** A script builds a `PolygonPathFinder`, sets it up
/// with Godot's flat connection array, and every query answers.
#[test]
fn polygon_path_finder_reachable_from_gdscript_with_godot_signatures() {
    let src = format!(
        "extends Node\n\
         var inside = false\n\
         var outside_left = false\n\
         var bounds_pos = Vector2(-1, -1)\n\
         var bounds_size = Vector2(-1, -1)\n\
         var hits = 0\n\
         var penalty_read = -1.0\n\
         func run():\n\
         \x20   var f = PolygonPathFinder.new()\n\
         \x20   {SQUARE}\
         \x20   f.setup(pts, cons)\n\
         \x20   self.inside = f.is_point_inside(Vector2(15, 15))\n\
         \x20   self.outside_left = f.is_point_inside(Vector2(0, 15))\n\
         \x20   var b = f.get_bounds()\n\
         \x20   self.bounds_pos = b.position\n\
         \x20   self.bounds_size = b.size\n\
         \x20   self.hits = f.get_intersections(Vector2(0, 15), Vector2(30, 15)).size()\n\
         \x20   f.set_point_penalty(0, 7.5)\n\
         \x20   self.penalty_read = f.get_point_penalty(0)\n"
    );
    let (tree, node) = run(&src);

    // -- is_point_inside, both ways ------------------------------------------
    // The centre is inside; a point to the left on the same row is not. The
    // pair matters: an even-odd test with an off-by-one in its crossing count
    // reports *everything* inside, which a single positive assertion passes.
    assert_eq!(
        script_var(&tree, node, "inside"),
        Variant::Bool(true),
        "the square's centre must be inside it"
    );
    assert_eq!(
        script_var(&tree, node, "outside_left"),
        Variant::Bool(false),
        "a point outside the square must not be reported inside"
    );

    // -- get_bounds returns a Rect2, not the module's min/max pair ------------
    // Position and size are asserted separately, and the square is deliberately
    // NOT at the origin: with a square at (0,0)-(10,10), a `Rect2` built from
    // (min, max) instead of (min, max - min) would be indistinguishable from a
    // correct one. At (10,10)-(20,20) the wrong conversion gives size (20,20).
    assert_eq!(
        script_var(&tree, node, "bounds_pos"),
        Variant::Vector2(gdcore::math::Vector2::new(10.0, 10.0)),
        "get_bounds().position is the minimum corner"
    );
    assert_eq!(
        script_var(&tree, node, "bounds_size"),
        Variant::Vector2(gdcore::math::Vector2::new(10.0, 10.0)),
        "get_bounds().size is the EXTENT, not the maximum corner — a Rect2 built \
         from (min, max) would read (20, 20) here"
    );

    // -- get_intersections ---------------------------------------------------
    // A horizontal line through the middle crosses the left and right edges,
    // and only those. An exact 2 rather than `> 0`: reusing the module's
    // exclusive visibility predicate instead of Godot's inclusive intersection
    // test would answer 2 here too for this line, but a count is what pins the
    // top and bottom edges as NOT crossed.
    assert_eq!(
        script_var(&tree, node, "hits"),
        Variant::Int(2),
        "a line through the square crosses exactly its left and right edges"
    );

    // -- penalties round-trip through the flat signatures ---------------------
    assert_eq!(
        script_var(&tree, node, "penalty_read"),
        Variant::Float(7.5),
        "set_point_penalty must be observable through get_point_penalty"
    );
}

/// `find_path` routes AROUND the obstacle rather than through it.
///
/// This is the assertion the algorithm exists for, and it is separated because
/// it is the one that fails if `setup` mis-pairs the flat connection array: a
/// finder with no edges happily returns the straight two-point line, which
/// looks like a working pathfinder until you notice it ignores the obstacle.
#[test]
fn polygon_path_finder_reachable_from_gdscript_with_godot_signatures_routes_around() {
    let src = format!(
        "extends Node\n\
         var count = 0\n\
         var crosses = 0\n\
         var touches = 0\n\
         func run():\n\
         \x20   var f = PolygonPathFinder.new()\n\
         \x20   {SQUARE}\
         \x20   f.setup(pts, cons)\n\
         \x20   var path = f.find_path(Vector2(0, 15), Vector2(30, 15))\n\
         \x20   self.count = path.size()\n\
         \x20   for i in range(path.size() - 1):\n\
         \x20       var hits = f.get_intersections(path[i], path[i + 1])\n\
         \x20       self.touches += hits.size()\n\
         \x20       for j in range(hits.size()):\n\
         \x20           var hit = hits[j]\n\
         \x20           if hit.distance_to(path[i]) > 0.001:\n\
         \x20               if hit.distance_to(path[i + 1]) > 0.001:\n\
         \x20                   self.crosses += 1\n"
    );
    let (tree, node) = run(&src);

    let count = match script_var(&tree, node, "count") {
        Variant::Int(n) => n,
        other => panic!("find_path returned no usable path: {other:?}"),
    };
    assert!(
        count > 2,
        "a path from (0,15) to (30,15) must detour around the square — {count} \
         points means it went straight through, which is what an empty edge set \
         produces"
    );
    // Stronger than the point count: no segment of the returned path may cross
    // a boundary edge. A detour that clips a corner still has > 2 points.
    //
    // ⚠️ **Interior intersections only — endpoint touches are not crossings.**
    // `get_intersections` is endpoint-INCLUSIVE, faithfully to Godot: it is a
    // direct port of `Geometry2D::segment_intersects_segment`
    // (`geometry_2d.h:188`), and upstream's own `PolygonPathFinder::get_intersections`
    // (`polygon_path_finder.cpp:506-520`) loops the edges calling exactly that.
    // A visibility-graph path routes THROUGH the obstacle's corners, so every
    // segment necessarily shares an endpoint with two edges.
    //
    // Counting raw hits therefore measured something no correct path can ever
    // achieve: this fixture's shortest detour is
    // `(0,15) -> (10,10) -> (20,10) -> (30,15)`, whose three segments touch two
    // edges apiece — the exact 6 this assertion used to report. The segment
    // along the bottom edge is collinear and is already rejected upstream, so it
    // contributes nothing either way.
    //
    // Filtering to hits strictly inside the segment keeps the assertion's real
    // intent — a path that CLIPS a corner or cuts the interior produces an
    // intersection away from both endpoints and still fails here — while no
    // longer failing a path for hugging the geometry it is supposed to hug.
    // `polygon_path_finder.rs` says the same thing in its own words: "a path is
    // allowed to travel along an obstacle edge or through a shared vertex".
    // 🔑 The anti-vacuous guard. `crosses` is a count of things that must NOT
    // happen, so it reads 0 both when the path is clean and when the loop never
    // ran at all — a `get_intersections` that returned nothing, or an iteration
    // form the interpreter silently ignored, would look like success. This
    // fixture's corner-hugging path MUST touch edges at its shared vertices, so
    // a non-zero raw count is the proof that the measurement happened.
    let touches = script_var(&tree, node, "touches");
    assert!(
        matches!(touches, Variant::Int(n) if n > 0),
        "the intersection query must have run and reported the endpoint touches \
         a corner-routing path necessarily makes; got {touches:?}, which means \
         the `crosses` assertion below would pass without measuring anything"
    );
    assert_eq!(
        script_var(&tree, node, "crosses"),
        Variant::Int(0),
        "no segment of the path may cross an obstacle edge"
    );
}

/// ClassDB must agree, or a `.tscn`/`.tres` and the inspector deny a class the
/// interpreter can construct.
#[test]
fn polygon_path_finder_reachable_from_gdscript_with_godot_signatures_registered() {
    use gdobject::class_db::{class_exists, get_class_info, is_parent_class, register_all_classes};

    register_all_classes();

    assert!(
        class_exists("PolygonPathFinder"),
        "PolygonPathFinder must be registered"
    );
    // The chain must WALK, not merely store the name — `Resource` is what a
    // `.tres` loader and the inspector both key on.
    assert!(
        is_parent_class("PolygonPathFinder", "Resource"),
        "PolygonPathFinder must resolve Resource through its inheritance chain"
    );

    let info = get_class_info("PolygonPathFinder").expect("registered");
    for name in [
        "setup",
        "find_path",
        "get_intersections",
        "get_closest_point",
        "is_point_inside",
        "set_point_penalty",
        "get_point_penalty",
        "get_bounds",
    ] {
        assert!(
            info.methods.iter().any(|m| m.name == name),
            "PolygonPathFinder must declare {name}"
        );
    }
    // `data` is Godot's serialization surface — without it a `.tres` carries
    // nothing and the round-trip silently produces an empty finder.
    assert!(
        info.properties.iter().any(|p| p.name == "data"),
        "PolygonPathFinder must declare its `data` property"
    );
    // The control: the lookup reports what was declared, not anything asked of it.
    assert!(
        !info.methods.iter().any(|m| m.name == "no_such_method"),
        "a method lookup that says yes to anything would satisfy every \
         assertion above"
    );
}
