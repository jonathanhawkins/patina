//! pat-mgixk.29: every Node3D transform write reaches the RENDERED transform.
//!
//! In Godot every Node3D transform property — `position`, `rotation`,
//! `rotation_degrees`, `quaternion`, `basis`, `scale`, `transform` — is a view of
//! one `data.local_transform` (`scene/3d/node_3d.cpp`), so it does not matter
//! which one a tween, an animation track or root motion writes.
//!
//! Patina had exactly one writer that honoured that (`scripting.rs`, and only
//! for `Vector3` position/rotation/scale). Everything else called
//! `Node::set_property` raw, which has no transform hook. Two distinct failures
//! followed:
//!
//!   * **Stale `transform` wins.** `local_transform_of` returns an explicit
//!     `transform` property when present, and a `.tscn`-loaded Node3D always has
//!     one. So an animated or tweened `position` was written to a key that
//!     `get_global_transform` never consulted — the node did not move on screen.
//!   * **`quaternion` was never read at all.** `local_transform_of` had zero
//!     references to `quaternion` or `basis`, so a Rotation3D track — which is
//!     exactly what the glTF importer emits — was inert on EVERY node, `.tscn`
//!     or not.
//!
//! The pre-existing test at `scene_tree.rs` only ever asserted the raw
//! properties, never `get_global_transform`, which is why both went unnoticed.
//! Everything here asserts through `node3d::get_global_transform` — the value
//! the 3D render path actually collects.

use gdcore::math::Vector3;
use gdcore::math3d::Quaternion;
use gdscene::animation::{Animation, AnimationPlayer, AnimationTrack, KeyFrame, TrackType};
use gdscene::main_loop::MainLoop;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::tween::TweenBuilder;
use gdscene::{node3d, NodeId, SceneTree};
use gdvariant::Variant;

const DT: f64 = 1.0 / 60.0;

/// The `.tscn` condition that made this invisible: the MeshInstance3D carries an
/// explicit `transform =`, exactly as a scene saved from the editor does.
const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Mesh" type="MeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 7, 8, 9)
"#;

fn load_scene() -> (SceneTree, NodeId) {
    gdobject::class_db::register_3d_classes();
    let packed = PackedScene::from_tscn(SCENE).expect("fixture scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &packed).expect("scene instances");
    let mesh = tree
        .get_node_or_null(scene_root, "Mesh")
        .expect("Mesh node exists");
    (tree, mesh)
}

/// A clip driving BOTH `position` (Position3D) and `quaternion` (Rotation3D) on
/// `Mesh`, which is the pair the bead names.
fn drive_clip() -> Animation {
    let mut pos = AnimationTrack::with_type("Mesh", "position", TrackType::Position3D);
    pos.add_keyframe(KeyFrame::linear(
        0.0,
        Variant::Vector3(Vector3::new(7.0, 8.0, 9.0)),
    ));
    pos.add_keyframe(KeyFrame::linear(
        1.0,
        Variant::Vector3(Vector3::new(107.0, 8.0, 9.0)),
    ));

    // A quarter turn about Y. `Rotation3D` keys are quaternions, and the
    // `quaternion` property they land on is the one nothing used to read.
    let half = std::f32::consts::FRAC_PI_4; // half of 90 degrees
    let mut rot = AnimationTrack::with_type("Mesh", "quaternion", TrackType::Rotation3D);
    rot.add_keyframe(KeyFrame::linear(
        0.0,
        Variant::Quaternion(Quaternion::IDENTITY),
    ));
    rot.add_keyframe(KeyFrame::linear(
        1.0,
        Variant::Quaternion(Quaternion {
            x: 0.0,
            y: half.sin(),
            z: 0.0,
            w: half.cos(),
        }),
    ));

    let mut clip = Animation::new("drive", 1.0);
    clip.tracks.push(pos);
    clip.tracks.push(rot);
    clip
}

/// **The acceptance test.** An animation track moves AND rotates the rendered
/// transform of a node that carries an explicit `.tscn` transform.
#[test]
fn node3d_tween_and_animation_drive_rendered_transform() {
    let (mut tree, mesh) = load_scene();

    // The control, and the thing that made this bug invisible: the `.tscn`
    // transform is already in force, so `local_transform_of` has an explicit
    // `transform` to prefer over any component key.
    let before = node3d::get_global_transform(&tree, mesh);
    assert_eq!(
        before.origin,
        Vector3::new(7.0, 8.0, 9.0),
        "the .tscn transform must be the starting rendered transform",
    );

    let player_node = tree
        .get_node_or_null(tree.root_id(), "Root")
        .expect("scene root");
    let mut player = AnimationPlayer::new();
    player.add_animation(drive_clip());
    player.play("drive");
    tree.attach_animation_player(player_node, player)
        .expect("attach player");

    let mut ml = MainLoop::new(tree);
    // Half the clip: far enough that both channels have moved measurably, while
    // still mid-clip so nothing depends on end-of-clip behaviour.
    for _ in 0..30 {
        ml.step(DT);
    }

    let after = node3d::get_global_transform(ml.tree(), mesh);

    // -- translation reached the rendered transform -------------------------
    assert!(
        after.origin.x > 20.0,
        "the Position3D track must move the RENDERED transform; origin.x is \
         still {} (started at 7.0, heading to 107.0)",
        after.origin.x,
    );

    // -- and so did rotation, via `quaternion` ------------------------------
    //
    // Asserted on the basis rather than the raw property: the property could be
    // set and still be ignored, which is precisely what used to happen. A
    // quarter turn about Y sends the local X axis toward -Z, so `basis.x.z`
    // leaves zero.
    assert!(
        after.basis.x.z.abs() > 0.1,
        "the Rotation3D track writes `quaternion`, which must reach the rendered \
         basis; basis.x = {:?}",
        after.basis.x,
    );
    // The rotation must not have silently eaten the scale.
    let scale = after.basis.get_scale();
    assert!(
        (scale.x - 1.0).abs() < 1e-3 && (scale.y - 1.0).abs() < 1e-3,
        "rotating through `quaternion` must preserve scale, got {scale:?}",
    );

    // -- the RAW property views agree with the composed transform -------------
    //
    // ⚠️ This is the gap this test shipped with, and it cost a regression. The
    // assertions above read the COMPOSED basis, which is the right oracle for
    // "did the track reach the renderer" — but it meant a funnel that stored the
    // rotation as Euler and never wrote the `quaternion` KEY back passed here,
    // while `gltf_anim_import::gltf_import_animation_clip_plays` failed with
    // "the rotation track did not reach the node, got Nil".
    //
    // A script, and that test, do a plain `get_property`, which reads the raw
    // key and not a derived one. So assert the views too: a quarter turn about
    // Y, sampled mid-clip, is ~sin(22.5 deg) = 0.38 in `q.y`.
    let node = ml.tree().get_node(mesh).expect("mesh node is in the tree");
    match node.get_property("quaternion") {
        Variant::Quaternion(q) => assert!(
            q.y.abs() > 0.05,
            "the raw `quaternion` view must carry the driven rotation, got {q:?}",
        ),
        other => panic!(
            "`quaternion` must read back as a Quaternion once a Rotation3D track \
             has driven it — `Nil` means the transform funnel wrote every other \
             view and skipped this one, got {other:?}"
        ),
    }
    match node.get_property("basis") {
        Variant::Basis(b) => assert!(
            b.x.z.abs() > 0.1,
            "the raw `basis` view must agree with the composed one, got {b:?}",
        ),
        other => panic!("`basis` must read back as a Basis, got {other:?}"),
    }
}

/// The tween half of the acceptance: a Tween on `position` of the same
/// `.tscn`-transformed node also reaches the rendered transform.
///
/// Separate test because it shares only the fixture: the tween apply is a
/// different call site (`process_tweens_in_mode`) from the animation one, and
/// before this change each bypassed the canonical setter independently.
#[test]
fn node3d_tween_drives_rendered_transform() {
    let (mut tree, mesh) = load_scene();
    assert_eq!(
        node3d::get_global_transform(&tree, mesh).origin,
        Vector3::new(7.0, 8.0, 9.0),
        "control: the .tscn transform is in force before the tween runs",
    );

    let tween = TweenBuilder::new()
        .tween_property(
            "position",
            Variant::Vector3(Vector3::new(7.0, 8.0, 9.0)),
            Variant::Vector3(Vector3::new(107.0, 8.0, 9.0)),
            1.0,
        )
        .build();
    tree.add_tween(mesh, tween);

    let mut ml = MainLoop::new(tree);
    for _ in 0..30 {
        ml.step(DT);
    }

    let after = node3d::get_global_transform(ml.tree(), mesh);
    assert!(
        after.origin.x > 20.0,
        "a Tween on `position` must move the RENDERED transform; origin.x is \
         still {}",
        after.origin.x,
    );
}
