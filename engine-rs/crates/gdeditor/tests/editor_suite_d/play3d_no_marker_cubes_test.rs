//! pat-mgixk.33 — a Play frame draws no marker cubes for non-visual Node3D.
//!
//! In Godot a plain `Node3D` has no visual instance: only `VisualInstance3D`
//! subclasses draw (`scene/3d/node_3d.cpp`). So a running game shows nothing at
//! a pivot, a `Marker3D`, an `Area3D` or a `Decal`'s own origin.
//!
//! Patina's `primitive_for_class` sent EVERY non-camera, non-light spatial node
//! that reached the geometry fallback to `PrimitiveShape::Box`, drawn as an
//! orange `Mesh3D::cube(0.4)`, and `is_editor_only_marker` stripped only the
//! camera, light and collision affordances from a game frame. So a player saw a
//! cube at every pivot, Marker3D, Area3D and Decal.
//!
//! # The distinction this pins
//!
//! That one `Box` meant two unrelated things, which is why a naive shape-based
//! strip regressed `play_uses_scene_camera3d`:
//!
//!   * **a pivot with no geometry of its own** — `Node3D`, `Marker3D`, `Area3D`,
//!     and the `VisualInstance3D` effect/probe classes. Editor handle only.
//!   * **a `MeshInstance3D` whose `mesh` did not resolve** — which also lands on
//!     `PrimitiveShape::Box`.
//!
//! 🔑 They are separated by the KIND, not by a new shape. Geometry that resolves
//! becomes `Render3DKind::Mesh`; only the unresolved placeholder stays
//! `Render3DKind::Primitive`, and the strip keys off that. So a `MeshInstance3D`
//! carrying a real `mesh` survives a game frame, and a mesh-less one draws
//! nothing — which is Godot's behaviour (a null `mesh` submits no geometry) and
//! what this bead's Implement line asks for. The fixtures that relied on a
//! mesh-less node standing in for scene content were given real meshes, which is
//! what makes them honest either way.
//!
//! # ⚠️ Why the Decal is NOT toggled
//!
//! The acceptance text asks for a frame "identical to the same scene with those
//! four nodes removed", with a `Decal` among the four. **That comparison cannot
//! pass, however correct the strip is.** A `Decal` is collected into
//! `env_effects` by `scene_env_effects` (viewport_3d_render.rs:2183) and reaches
//! the renderer (`renderer.rs:658`, `:1280`); `strip_editor_only_items` only
//! filters `items` and never touches `env_effects`. So deleting the Decal from
//! the scene removes its shading contribution, and the two frames differ by that
//! no matter what the marker strip does.
//!
//! The Decal therefore lives in BOTH variants, holding `env_effects` constant,
//! and its clause is asserted on the render LIST instead: no item for the Decal
//! survives into a game frame. Same guarantee, with one variable instead of two.
//!
//! # What makes this test non-vacuous
//!
//! Several assertions have to hold together, and none alone is enough:
//!
//! 1. the two GAME frames are pixel-identical — the markers contribute nothing;
//! 2. the two EDITOR frames DIFFER — the markers were genuinely being drawn, in
//!    view, and are still drawn for the editor;
//! 3. each of the four marker NODES is present in the EDITOR list carrying
//!    `Box` — so they were collected and classified, rather than classified as
//!    something this test does not look for;
//! 4. the real mesh SURVIVES the game strip — otherwise "no markers" would be
//!    satisfied by stripping the whole scene.
//!
//! Without (2) and (3), (1) would also pass if the marker nodes were off-screen,
//! if the render list were empty, or if the classification silently changed. A
//! further guard asserts the frame is not a flat background, so "identical" is
//! not two blank images agreeing.

use gdeditor::viewport_3d_render::{
    editor_3d_render_inputs, game_3d_render_inputs, render_editor_3d_frame, render_game_3d_frame,
    viewport_for_scene_camera, PrimitiveShape, Render3DItem, Render3DKind,
};
use gdscene::camera3d::current_camera3d;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

const W: u32 = 96;
const H: u32 = 72;

/// The scene as a real `.tscn`, because this bead carries a Wiring requirement:
/// the markers have to arrive the way a game's scene does, through
/// `PackedScene::from_tscn`, not from nodes hand-built in Rust.
///
/// `MARKERS` holds the four non-visual classes the bead names. Each sits at
/// z = +1.0 — between the camera at +2 and the mesh at the origin — so it is
/// unambiguously inside the frustum. That placement is the point: a marker
/// off-screen would make the frame comparison pass for the wrong reason.
/// The three non-visual nodes whose marker cubes must vanish from a game frame
/// and survive in the editor viewport.
///
/// 🔑 **They are offset to x = 0.6, and that is load-bearing.** With all three
/// at the origin's z = 1 they shared a transform with `Splat`, the `Decal` that
/// is present in BOTH arms of the control — and a Decal is a `Node3D`, so
/// `collect_scene_content` admits it (no exclusion for Decal/VisualInstance) and
/// `render_item_for` falls through to `Primitive { Box }` for any class without
/// "Camera"/"Light" in its name. The Decal painted its own grey cube at exactly
/// this spot, coincident with the markers, so REMOVING the markers changed no
/// pixel and the editor-side control passed vacuously — it reported "all 6912
/// pixels match", i.e. the thing it varies was being supplied by something else.
///
/// ⚠️ x = 0.6 rather than a rounder 0.9: at the default 75 deg VERTICAL fov the
/// half-extent at distance 1 is tan(37.5) = 0.767, so 1.02 horizontally at
/// 96x72 — a 0.4-wide cube at 0.9 would span [0.7, 1.1] and clip. At 0.6 it
/// spans [0.4, 0.8], while the Box's near face covers only screen |x| <= 0.33
/// and the Decal's cube only |x| <= 0.2. So these are measured against the
/// BACKGROUND, with nothing else able to refill them.
const MARKERS: &str = r#"
[node name="Pivot" type="Node3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0.6, 0, 1)

[node name="Mark" type="Marker3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0.6, 0, 1)

[node name="Zone" type="Area3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0.6, 0, 1)
"#;

/// A lit mesh at the origin with a `Camera3D` two units down +Z looking at it
/// (identity basis = looking down −Z, as in Godot).
fn scene_source(with_markers: bool) -> String {
    format!(
        r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Box" type="MeshInstance3D" parent="."]
mesh = "primitive:BoxMesh"

[node name="Sun" type="DirectionalLight3D" parent="."]

[node name="Splat" type="Decal" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1)

[node name="Cam" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 2)
fov = 60.0
current = true
{}"#,
        if with_markers { MARKERS } else { "" }
    )
}

/// Returns the tree, the scene root, and the `MeshInstance3D` whose REAL mesh
/// (`mesh = "primitive:BoxMesh"`) must survive the strip.
///
/// ⚠️ An earlier revision of this fixture left that node mesh-LESS, and this
/// comment still said so. Under the design this test pins, the two readings are
/// opposites: a mesh-less `MeshInstance3D` stays `Render3DKind::Primitive` and
/// is *correctly* stripped from a game frame, so taking the mesh away would make
/// the "real scene content survives" assertion fail rather than pass. Read this
/// as the warning it is — the doc comment that attempt 1 of this bead deleted
/// ("box markers are kept: those stand in for scene content") was also right.
fn fixture(with_markers: bool) -> (SceneTree, NodeId, NodeId) {
    // The render list filters on `is_class("Node3D")`, which reads the ClassDB.
    // Without this the 3D classes are unknown, the list comes back EMPTY, and
    // every assertion below passes vacuously — the reference test for this
    // subsystem documents the same trap.
    gdobject::class_db::register_3d_classes();

    let packed = PackedScene::from_tscn(&scene_source(with_markers)).expect("fixture .tscn parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &packed).expect("scene instances");
    let mesh = tree
        .get_node_or_null(scene_root, "Box")
        .expect("fixture has the MeshInstance3D");
    (tree, scene_root, mesh)
}

/// The EDITOR render list (markers NOT stripped), rendered with the editor's own
/// chrome turned OFF.
///
/// 🔑 `grid.visible = false` and `show_origin = false` are the whole point, and
/// they STRENGTHEN the clause below rather than weakening it. The editor draws
/// its ground grid and its coloured origin axes **with no depth test**, over
/// whatever geometry is behind them. The fixture's markers sit on the camera's
/// forward axis, so they project to the exact centre of frame — which is also
/// where the origin axes and the edge-on grid land. Chrome drawn with no depth
/// test can only ever MASK a difference between the two frames, never create
/// one, so leaving it on makes "the frames match" ambiguous between "the markers
/// drew nothing" and "the markers drew and the chrome painted over both".
///
/// Turning it off is exactly what `render_game_3d_frame` does for the game path
/// (`viewport.grid.visible = false; viewport.show_origin = false;`), so this is
/// the same viewport configuration, differing from the game frame only in that
/// the render list is UNSTRIPPED — which is the one variable the clause is about.
///
/// ⚠️ This is also why the sibling clause in `shaded_box_tscn`'s file renders
/// through `render_game_3d_frame`: an earlier revision of THAT test asserted a
/// control had zero red-dominant pixels and failed with 28, because the editor's
/// red X axis is alpha-blended over the thing under test.
fn editor_frame(tree: &SceneTree) -> Vec<gdcore::math::Color> {
    let camera = current_camera3d(tree).expect("fixture has a current Camera3D");
    let mut viewport = viewport_for_scene_camera(tree, camera, W, H);
    viewport.grid.visible = false;
    viewport.show_origin = false;
    render_editor_3d_frame(&viewport, tree).pixels
}

/// Number of distinct colours in `pixels`, capped at `cap`.
///
/// A frame of one flat colour compares equal to any other frame of that same
/// colour, so a difference assertion over two such frames is vacuous. The game
/// clause has had this guard from the start; the editor clause did not, which is
/// why its failure could not be read.
fn distinct_colours(pixels: &[gdcore::math::Color], cap: usize) -> usize {
    let mut seen: Vec<gdcore::math::Color> = Vec::new();
    for px in pixels {
        if !seen.iter().any(|c| c == px) {
            seen.push(*px);
        }
        if seen.len() >= cap {
            break;
        }
    }
    seen.len()
}

/// Indices where two equal-length frames differ.
///
/// 🔑 THE REASON THIS EXISTS IS DIAGNOSABILITY, not style. The frame clauses
/// below used to be `assert_eq!(a.pixels, b.pixels, "...")` over 96x72 = 6912
/// `Color`s. On a failure that Debug-prints ~13800 lines, and the verifier
/// captures only a TAIL of the output — so the custom message and the
/// `file:line`, which `assert_eq!` prints FIRST, scrolled out every single time.
/// Three separate runs of this test, under three different implementations, left
/// nothing in the log but the end of a `Vec<Color>` dump with the same
/// `r: 0.2433896, g: 0.20244378, b: 0.15651318` tail, and a
/// `grep -E "assertion|left|right|panicked"` over the whole captured window
/// returned NOTHING. The test was unfalsifiable in practice.
///
/// Returning indices lets the assertions report a COUNT plus the first offending
/// pixel's coordinates and the two colours — a one-line diagnosis — while
/// bounding the output to a constant. Test-only, so it cannot move the red
/// check.
fn differing_pixels(a: &[gdcore::math::Color], b: &[gdcore::math::Color]) -> Vec<usize> {
    a.iter()
        .zip(b.iter())
        .enumerate()
        .filter(|(_, (x, y))| x != y)
        .map(|(i, _)| i)
        .collect()
}

/// `(count, "first difference at (x, y): <a> vs <b>")` for a failure message.
fn diff_report(a: &[gdcore::math::Color], b: &[gdcore::math::Color]) -> (usize, String) {
    let diff = differing_pixels(a, b);
    let detail = match diff.first() {
        Some(&i) => format!(
            "first at index {i} = ({}, {}): {:?} vs {:?}",
            i % (W as usize),
            i / (W as usize),
            a[i],
            b[i]
        ),
        None => "no pixel differs".to_string(),
    };
    (diff.len(), detail)
}

fn shape_count(items: &[Render3DItem], want: PrimitiveShape) -> usize {
    items
        .iter()
        // `&item.kind` deliberately: with a reference scrutinee `shape` binds as
        // `&PrimitiveShape`, so `*shape == want` is a value comparison. Matching
        // the place expression would leave the binding mode to inference.
        .filter(|item| matches!(&item.kind, Render3DKind::Primitive { shape } if *shape == want))
        .count()
}

/// Whether `target` is in `items` carrying exactly `want`.
fn has_shape(items: &[Render3DItem], target: NodeId, want: PrimitiveShape) -> bool {
    items.iter().any(|item| {
        item.node_id == target.raw()
            && matches!(&item.kind, Render3DKind::Primitive { shape } if *shape == want)
    })
}

/// **The acceptance test.**
#[test]
fn play3d_no_marker_cubes() {
    let (with, scene_root, mesh) = fixture(true);
    let (without, _, _) = fixture(false);

    let game_with = render_game_3d_frame(&with, W, H).expect("scene with a Camera3D renders");
    let game_without = render_game_3d_frame(&without, W, H).expect("bare scene renders");

    // -- guard: the frame has real content ----------------------------------
    //
    // Two flat background images would compare equal and prove nothing.
    let distinct = distinct_colours(&game_with.pixels, 2);
    assert!(
        distinct > 1,
        "the game frame must show the lit mesh against the background, not one \
         flat colour — otherwise frame equality below is vacuous",
    );

    // -- (1) the strip removed them, not the collection step -----------------
    //
    // 🔑 ORDERED FIRST on purpose. These are the deterministic, chrome-free
    // claims: they read the render LISTS, so a failure names the node and the
    // shape instead of a pixel count. The frame comparisons below are the
    // observable confirmation, but they are also the noisier signal — a
    // failure there with these passing says the lists are right and something
    // outside the item list (lighting, `env_effects`, the editor's own grid
    // and depth-test-free origin axes) moved the pixels. Running them last is
    // what makes that distinction readable in one log line; with the old
    // order, a frame failure aborted the test before any list was inspected.
    let camera = current_camera3d(&with).expect("current camera");
    let viewport = viewport_for_scene_camera(&with, camera, W, H);
    let editor_items = editor_3d_render_inputs(&viewport, &with).items;
    // Addressed by node id, not counted: the scene ROOT is a Node3D too, so a
    // count would be five, and an off-by-one there would be indistinguishable
    // from a classification bug.
    for name in ["Pivot", "Mark", "Zone", "Splat"] {
        let id = with
            .get_node_or_null(scene_root, name)
            .unwrap_or_else(|| panic!("fixture has {name}"));
        assert!(
            has_shape(&editor_items, id, PrimitiveShape::Box),
            "{name} must carry a Box placeholder and be present \
             in the EDITOR list — if it is absent or carries another shape, the \
             game-list assertion below proves nothing",
        );
    }

    let game_items = game_3d_render_inputs(&with, camera, &viewport).items;
    assert_eq!(
        shape_count(&game_items, PrimitiveShape::Box),
        0,
        "no Box placeholder may survive into the game render list — a pivot, a \
         Marker3D, an Area3D and a Decal all draw nothing in Godot",
    );

    // -- and REAL scene content MUST survive ---------------------------------
    //
    // 🔑 This is the half that stops the fix being "strip everything", and it is
    // why the fixture's `MeshInstance3D` carries `mesh = "primitive:BoxMesh"`.
    // Resolved geometry becomes `Render3DKind::Mesh`, a different kind from the
    // `Primitive` placeholder, so the strip cannot reach it. A mesh-LESS
    // MeshInstance3D stays a `Primitive` and is dropped — which is Godot's own
    // behaviour (a `MeshInstance3D` with a null mesh submits nothing) and what
    // this bead's Implement line asks for.
    assert!(
        game_items.iter().any(
            |item| item.node_id == mesh.raw() && matches!(item.kind, Render3DKind::Mesh { .. })
        ),
        "the MeshInstance3D with real geometry must stay in the game list: the \
         bead drops editor furniture, not scene content",
    );

    // …and the Decal's own placeholder is gone from the game list, which is the
    // Decal clause of the acceptance. Asserted on the LIST rather than by
    // removing the Decal from the scene, for the reason in the module header:
    // a Decal also feeds `env_effects`, which the strip does not touch.
    let splat = with
        .get_node_or_null(scene_root, "Splat")
        .expect("fixture has the Decal");
    assert!(
        !game_items.iter().any(|item| item.node_id == splat.raw()),
        "the Decal must contribute no render item to a game frame",
    );

    // -- (2) the markers contribute NOTHING to a game frame ------------------
    assert_eq!(
        (game_with.width, game_with.height),
        (game_without.width, game_without.height),
    );
    let (game_diff_count, game_diff_detail) = diff_report(&game_with.pixels, &game_without.pixels);
    assert_eq!(
        game_diff_count,
        0,
        "a Play frame with a Node3D pivot, Marker3D, Area3D and Decal in front \
         of the mesh must be pixel-identical to the same scene without them; \
         {game_diff_count} of {} pixels differ, {game_diff_detail}",
        game_with.pixels.len(),
    );

    // -- (3) …and they really were in view, and the editor still shows them --
    //
    // This is what rules out "the frames matched because nothing was drawn".
    let editor_with = editor_frame(&with);
    let editor_without = editor_frame(&without);
    // 🔑 GUARD FIRST, so a failure below is readable. Without this, "the two
    // editor frames match" has two very different causes and the log cannot tell
    // them apart: the markers drew nothing, or the editor frame drew nothing at
    // all (which would make the clause a statement about the editor's camera
    // setup rather than about this bead). The game clause has always had the
    // equivalent guard.
    let editor_palette = distinct_colours(&editor_with, 2);
    assert!(
        editor_palette > 1,
        "the EDITOR frame must show scene geometry against the background, not \
         one flat colour — otherwise the difference assertion below is vacuous. \
         A flat frame here means the editor render produced nothing for this \
         fixture's camera, which is a different defect from the one this bead \
         is about",
    );
    let (editor_diff_count, editor_diff_detail) = diff_report(&editor_with, &editor_without);
    assert!(
        editor_diff_count > 0,
        "the EDITOR frame must still draw the markers; all {} pixels match \
         ({editor_diff_detail}), which means the marker nodes were never \
         visible and assertion (2) proved nothing. The grid and origin axes are \
         OFF for this render and the frame is confirmed non-flat by the guard \
         above, so editor chrome masking the difference is already excluded — a \
         failure here is the `Primitive {{ Box }}` draw itself not reaching \
         pixels, even though clause (1) proved the items are in this very list",
        editor_with.len(),
    );
}
