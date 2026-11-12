//! pat-sazee: a scene file's `Transform3D(...)` / `Basis(...)` literal lists
//! the basis ROW-major, and Patina loaded (and saved) it as columns, which
//! transposed — i.e. INVERTED — every authored 3D rotation.
//!
//! Godot's writer walks `m3.rows[i][j]` (core/variant/variant_parser.cpp:2110)
//! and its parser feeds the nine numbers to `Basis(xx, xy, xz, ...)`, which
//! fills `rows` (core/math/basis.h:211). `gdcore::math3d::Basis` stores
//! COLUMNS. The real symptom: platformer_3d's follow camera
//! (`player/player.tscn:106`) tilted UP at the sky where Godot's looks DOWN at
//! the player.
//!
//! Every expected number here was printed by real Godot 4.6.1 headless with
//! `fixtures/real_godot/basis_row_major_probe.gd` (run inside a copy of
//! games/platformer_3d). `basis_row_major_probe.tscn` / `.scn` are the files
//! that script made Godot save — byte-for-byte Godot output.
//!
//! The camera's rotation is a pure X rotation, so its matrix is the transpose
//! of the inverse rotation: the bug flips the tilt sign. The probe scene uses a
//! NON-symmetric rotation (Euler 0.3, 0.7, -0.4) with an origin, so every
//! off-diagonal pair differs and any transposition shows.

use std::path::PathBuf;
use std::sync::Mutex;

use gdcore::math::Vector3;
use gdcore::math3d::Transform3D;
use gdscene::packed_scene::{
    add_packed_scene_to_tree, add_packed_scene_to_tree_from_res, begin_subscene_session,
    load_scene_from_res, PackedScene,
};
use gdscene::scene_saver::TscnSaver;
use gdscene::{node3d, NodeId, SceneTree};
use gdvariant::Variant;

/// `set_path_roots` is process-global; tests in this binary take turns.
static ROOTS: Mutex<()> = Mutex::new(());

const EPS: f32 = 1e-5;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn probe_tscn() -> String {
    std::fs::read_to_string(repo().join("fixtures/real_godot/basis_row_major_probe.tscn"))
        .expect("Godot-saved probe scene")
}

fn assert_vec3(got: Vector3, want: [f32; 3], what: &str) {
    let ok = (got.x - want[0]).abs() < EPS
        && (got.y - want[1]).abs() < EPS
        && (got.z - want[2]).abs() < EPS;
    assert!(ok, "{what}: Patina {got:?}, Godot {want:?}");
}

fn instance_text(tscn: &str) -> (SceneTree, NodeId) {
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    (tree, id)
}

fn transform_of(tree: &SceneTree, from: NodeId, path: &str) -> Transform3D {
    let id = if path == "." {
        from
    } else {
        tree.get_node_relative(from, path)
            .unwrap_or_else(|| panic!("no node at '{path}'"))
    };
    match tree.get_node(id).unwrap().get_property("transform") {
        Variant::Transform3D(t) => t,
        other => panic!("{path}.transform is {other:?}"),
    }
}

/// The non-symmetric probe rotation, as Godot reports `basis.x/.y/.z`.
const NS_X: [f32; 3] = [0.630329, -0.372026, -0.681383];
const NS_Y: [f32; 3] = [0.473195, 0.879923, -0.042686];
const NS_Z: [f32; 3] = [0.615445, -0.29552, 0.730682];

/// The real file: the platformer_3d follow camera.
#[test]
fn player_tscn_camera_basis_matches_godot() {
    let _g = ROOTS.lock().unwrap_or_else(|e| e.into_inner());
    let root = repo().join("games/platformer_3d");
    gdcore::file_access::set_path_roots(Some(root), None);
    begin_subscene_session();
    let packed = load_scene_from_res("res://player/player.tscn").expect("player.tscn loads");
    let mut tree = SceneTree::new();
    let tree_root = tree.root_id();
    let player = add_packed_scene_to_tree_from_res(&mut tree, tree_root, &packed)
        .expect("player.tscn instances");
    gdcore::file_access::set_path_roots(None, None);

    // Godot: basis.x=(1, 0, 0) basis.y=(0, 0.871263, -0.490817)
    //        basis.z=(0, 0.490817, 0.871263)
    let t = transform_of(&tree, player, "Target/Camera3D");
    assert_vec3(t.basis.x, [1.0, 0.0, 0.0], "camera basis.x");
    assert_vec3(t.basis.y, [0.0, 0.871263, -0.490817], "camera basis.y");
    assert_vec3(t.basis.z, [0.0, 0.490817, 0.871263], "camera basis.z");

    // Godot: rotation = (-0.513027, 0, 0) — the camera pitches DOWN.
    let cam = tree.get_node_relative(player, "Target/Camera3D").unwrap();
    assert_vec3(
        node3d::get_rotation(&tree, cam),
        [-0.513027, 0.0, 0.0],
        "camera rotation (Euler)",
    );

    // Godot: global origin (0, 4.55049, 1.25672), forward -Z = (0, -0.490817,
    // -0.871263): looking down and ahead at the player, never at the sky.
    let g = node3d::get_global_transform(&tree, cam);
    assert_vec3(g.origin, [0.0, 4.55049, 1.25672], "camera global origin");
    assert_vec3(
        g.basis.z * -1.0,
        [0.0, -0.490817, -0.871263],
        "camera global forward",
    );
}

/// A Godot-saved non-symmetric rotation loads with Godot's columns, and a
/// child under it lands at Godot's global position.
#[test]
fn godot_saved_rotation_loads_with_godots_columns_and_child_global_position() {
    let (tree, parent) = instance_text(&probe_tscn());
    let t = transform_of(&tree, parent, ".");
    assert_vec3(t.basis.x, NS_X, "parent basis.x");
    assert_vec3(t.basis.y, NS_Y, "parent basis.y");
    assert_vec3(t.basis.z, NS_Z, "parent basis.z");
    assert_vec3(t.origin, [1.0, 2.0, 3.0], "parent origin");

    // Godot: CHILD global_position = (0.636037, 2.658977, 0.835911).
    let child = tree.get_node_relative(parent, "Child").unwrap();
    assert_vec3(
        node3d::get_global_transform(&tree, child).origin,
        [0.636037, 2.658977, 0.835911],
        "child global position",
    );
}

/// The same literals nested in an Array and a Dictionary (metadata) take the
/// same row-major path.
#[test]
fn nested_transform_and_basis_literals_are_row_major() {
    let (tree, parent) = instance_text(&probe_tscn());
    let holder = tree.get_node_relative(parent, "Holder").unwrap();
    let Variant::Array(xs) = tree.get_node(holder).unwrap().get_meta("xs") else {
        panic!("metadata/xs must be an Array");
    };
    let xs = xs.to_vec();
    let Variant::Transform3D(t) = &xs[0] else {
        panic!("xs[0] is {:?}", xs[0]);
    };
    assert_vec3(t.basis.y, NS_Y, "array Transform3D basis.y");
    let Variant::Dictionary(d) = &xs[1] else {
        panic!("xs[1] is {:?}", xs[1]);
    };
    let Some(Variant::Basis(b)) = d.get("k") else {
        panic!("xs[1].k must be a Basis");
    };
    assert_vec3(b.x, NS_X, "dictionary Basis.x");
    assert_vec3(b.z, NS_Z, "dictionary Basis.z");
}

/// Godot's binary `.scn` of the same scene (resource_format_binary.cpp writes
/// `rows[i][j]` too) must agree with the text file.
#[test]
fn godot_binary_scn_loads_the_same_columns() {
    let bytes = std::fs::read(repo().join("fixtures/real_godot/basis_row_major_probe.scn"))
        .expect("Godot-saved probe .scn");
    let scene = gdscene::binary_scene::packed_scene_from_bytes(&bytes, "res://probe.scn")
        .expect(".scn parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let parent = add_packed_scene_to_tree(&mut tree, root, &scene).expect(".scn instances");
    let t = transform_of(&tree, parent, ".");
    assert_vec3(t.basis.x, NS_X, ".scn basis.x");
    assert_vec3(t.basis.y, NS_Y, ".scn basis.y");
    assert_vec3(t.basis.z, NS_Z, ".scn basis.z");
}

/// Every `Transform3D(...)` / `Basis(...)` literal in `text`, verbatim.
fn literals(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for head in ["Transform3D(", "Basis("] {
        let mut rest = text;
        while let Some(at) = rest.find(head) {
            let tail = &rest[at..];
            let end = tail.find(')').expect("closing paren");
            out.push(tail[..=end].to_string());
            rest = &tail[end..];
        }
    }
    out
}

/// Load -> save writes Godot's own bytes for every Transform3D/Basis literal
/// (a top-level property, inside an Array, inside a Dictionary), and
/// load -> save -> load -> save is a fixed point.
///
/// Literals, not whole lines: Patina's saver formats a Dictionary on one line
/// and adds derived Node3D rows, which are separate from pat-sazee.
#[test]
fn rotation_round_trips_byte_identically_against_godots_output() {
    let godot = probe_tscn();
    let want = literals(&godot);
    // Parent transform, Child transform, Array element, Dictionary value.
    assert_eq!(want.len(), 4, "fixture literals: {want:?}");
    assert!(
        want.iter()
            .any(|l| l.starts_with("Transform3D(0.63032913, 0.47319457, 0.61544466, ")),
        "the fixture must hold Godot's row-major literal: {want:?}"
    );

    let (tree, parent) = instance_text(&godot);
    let saved = TscnSaver::save_tree(&tree, parent);
    let got = literals(&saved);
    for lit in &want {
        assert!(
            got.contains(lit),
            "Godot wrote\n  {lit}\nbut Patina's save of the loaded scene was:\n{saved}"
        );
    }

    // ⚠️ A load -> save round trip alone cannot see this bug: a transposing
    // reader and a transposing writer cancel out. So the round trip is pinned
    // to Godot's COLUMNS at both ends, in memory, not only to the text.
    let (tree2, parent2) = instance_text(&saved);
    let t2 = transform_of(&tree2, parent2, ".");
    assert_vec3(t2.basis.x, NS_X, "reloaded basis.x");
    assert_vec3(t2.basis.y, NS_Y, "reloaded basis.y");
    assert_vec3(t2.basis.z, NS_Z, "reloaded basis.z");
    let saved2 = TscnSaver::save_tree(&tree2, parent2);
    assert_eq!(saved, saved2, "load -> save must be a fixed point");
}

/// The write direction on its own: a transform built in memory from known
/// COLUMNS saves as Godot's row-major literal. Godot 4.6.1:
/// `var_to_str(Transform3D(Basis(Vector3(1, 4, 7), Vector3(2, 5, 8),
/// Vector3(3, 6, 9)), Vector3(10, 11, 12)))` =
/// `Transform3D(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12)`.
#[test]
fn saver_writes_an_in_memory_rotation_row_major_like_godot() {
    let (mut tree, root) =
        instance_text("[gd_scene format=3]\n\n[node name=\"R\" type=\"Node3D\"]\n");
    let t = Transform3D {
        basis: gdcore::math3d::Basis {
            x: Vector3::new(1.0, 4.0, 7.0),
            y: Vector3::new(2.0, 5.0, 8.0),
            z: Vector3::new(3.0, 6.0, 9.0),
        },
        origin: Vector3::new(10.0, 11.0, 12.0),
    };
    tree.get_node_mut(root)
        .unwrap()
        .set_property("transform", Variant::Transform3D(t));
    let saved = TscnSaver::save_tree(&tree, root);
    assert!(
        saved
            .lines()
            .any(|l| l == "transform = Transform3D(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12)"),
        "expected Godot's row-major literal, saved:\n{saved}"
    );
    assert_eq!(
        gdscene::scene_saver::format_variant_value(&Variant::Basis(t.basis)),
        "Basis(1, 2, 3, 4, 5, 6, 7, 8, 9)"
    );

    // The .tres writer must agree.
    let mut r = gdresource::resource::Resource::new("Resource");
    r.set_property("xf", Variant::Transform3D(t));
    r.set_property("b", Variant::Basis(t.basis));
    let tres = gdresource::saver::TresSaver::new()
        .save_to_string(&r)
        .expect("tres saves");
    assert!(
        tres.lines()
            .any(|l| l == "xf = Transform3D(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12)"),
        "expected Godot's row-major literal in the .tres:\n{tres}"
    );
    assert!(
        tres.lines()
            .any(|l| l == "b = Basis(1, 2, 3, 4, 5, 6, 7, 8, 9)"),
        "expected Godot's row-major Basis in the .tres:\n{tres}"
    );
}
