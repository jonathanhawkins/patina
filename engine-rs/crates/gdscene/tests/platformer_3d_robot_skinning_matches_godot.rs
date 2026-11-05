//! pat-ghw2s / pat-3mbf5 — the official 3D platformer's skinned robot poses,
//! deforms and loops like Godot 4.6.1, on the REAL, unmodified
//! `games/platformer_3d` files.
//!
//! Every expected number below was captured from Godot 4.6.1 headless on a
//! copy of the same project (`Skeleton3D.get_bone_global_pose`, the robot's
//! `Skin` and `Mesh.surface_get_arrays`), not derived from Patina.
//!
//! What each test caught at the parent commit:
//!
//! * A bone is driven by THREE tracks sharing one path (`Skeleton/Skeleton3D:hip`
//!   as position, rotation and scale). Both mixers paired values with track
//!   types through a `(node, property)` map, so all three got ONE type: the
//!   player wrote every bone position as a scale and dropped the rotation, and
//!   the AnimationTree's blend nodes blended a position against a scale. Hip
//!   was 0.7 units off Godot and the robot rendered crumpled.
//! * `player.glb`'s robot mesh has TWO primitives; only the first carried bone
//!   influences, so the second surface (639 vertices) drew unposed, in bind
//!   space, at 0.3x scale.
//! * An AnimationTree leaf wrapped EVERY clip, ignoring the clip's
//!   `loop_mode`; Godot holds a non-looping clip's last frame.
//! * `player.gd` calls `reset_physics_interpolation()` after a respawn;
//!   "method not found" fired every such frame and the respawn streaked.

use std::path::PathBuf;
use std::sync::Mutex;

use gdcore::math::Vector3;
use gdcore::math3d::Transform3D;
use gdscene::packed_scene::{
    add_packed_scene_to_tree_from_res, begin_subscene_session, load_scene_from_res,
};
use gdscene::render_server_3d::{deformed_mesh_for_node, RenderServer3DAdapter};
use gdscene::{skeleton3d, NodeId, SceneTree};
use gdvariant::Variant;

/// `set_path_roots` is process-global; tests in this binary take turns.
static ROOTS: Mutex<()> = Mutex::new(());

fn platformer_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../games/platformer_3d")
}

fn boot(scene: &str) -> (SceneTree, NodeId) {
    let root = platformer_root();
    assert!(root.join("project.godot").exists(), "fixture missing");
    gdcore::file_access::set_path_roots(Some(root), None);
    begin_subscene_session();
    let packed = load_scene_from_res(&format!("res://{scene}"))
        .unwrap_or_else(|| panic!("res://{scene} must load"));
    let mut tree = SceneTree::new();
    let tree_root = tree.root_id();
    let id = add_packed_scene_to_tree_from_res(&mut tree, tree_root, &packed)
        .unwrap_or_else(|e| panic!("res://{scene} must instance: {e}"));
    (tree, id)
}

fn id_of(tree: &SceneTree, from: NodeId, path: &str) -> NodeId {
    tree.get_node_relative(from, path)
        .unwrap_or_else(|| panic!("no node at '{path}'"))
}

/// Godot's `Transform3D` as 12 floats: basis columns x, y, z, then origin —
/// the order the capture script printed.
fn flat(t: &Transform3D) -> [f32; 12] {
    let b = &t.basis;
    [
        b.x.x, b.x.y, b.x.z, b.y.x, b.y.y, b.y.z, b.z.x, b.z.y, b.z.z, t.origin.x, t.origin.y,
        t.origin.z,
    ]
}

/// Asserts each named bone's global pose matches Godot's within `tol` (max
/// absolute component error).
fn assert_bones(tree: &SceneTree, skeleton: NodeId, godot: &[(&str, [f32; 12])], tol: f32, ctx: &str) {
    for (name, want) in godot {
        let bone = skeleton3d::find_bone(tree, skeleton, name)
            .unwrap_or_else(|| panic!("bone {name}"));
        let got = flat(&skeleton3d::get_bone_global_pose(tree, skeleton, bone));
        let err = got
            .iter()
            .zip(want.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(
            err <= tol,
            "{ctx}: bone {name} global pose differs from Godot by {err} (tol {tol})\n  \
             patina {got:?}\n  godot  {want:?}"
        );
    }
}

/// `run` at 0.4 s through the glb's own AnimationPlayer.
const GODOT_RUN_0_4: &[(&str, [f32; 12])] = &[
    ("hip", [0.985202, 0.031891, -0.168405, -0.007677, 0.989762, 0.142519, 0.171226, -0.139117, 0.975360, -0.035471, 1.992790, 0.705748]),
    ("chest", [0.980053, -0.039838, 0.194702, -0.040586, 0.918933, 0.392321, -0.194547, -0.392397, 0.898986, -0.044390, 3.168282, 0.968943]),
    ("r-forearm", [-0.815120, 0.417122, -0.401980, -0.529200, -0.253914, 0.809614, 0.235639, 0.872660, 0.427712, 1.315095, 3.030403, 1.760933]),
    ("l-foot", [-0.999943, 0.002816, -0.011653, -0.011724, -0.024861, 0.999625, 0.002524, 0.999688, 0.024895, -0.379473, 0.280894, 0.700818]),
];

#[test]
fn player_glb_run_clip_poses_every_bone_like_godot() {
    let _g = ROOTS.lock().unwrap_or_else(|e| e.into_inner());
    let (mut tree, glb) = boot("player/player.glb");
    let skeleton = id_of(&tree, glb, "Skeleton/Skeleton3D");
    let ap = id_of(&tree, glb, "AnimationPlayer");
    let player = tree.get_animation_player_mut(ap).unwrap();
    player.play("run");
    player.seek(0.4, true);
    skeleton3d::process_skeletons(&mut tree);
    assert_bones(&tree, skeleton, GODOT_RUN_0_4, 1e-4, "AnimationPlayer run@0.4");
}

/// Drives player.tscn's AnimationTree with fixed parameters and `frames`
/// advances of 1/60 s, then runs the skeleton phase.
///
/// ⚠️ Godot's FIRST process of an activated AnimationTree is a seek to 0
/// (`animation_tree.cpp` `started` -> `pi.seeked = true`), so Godot after N+1
/// advances shows what Patina shows after N. The Godot numbers below were
/// captured with 31 manual `advance()` calls; Patina advances 30 times.
fn tree_pose(run: f64, speed: f64, state: f64, air_dir: f64, frames: usize) -> (SceneTree, NodeId) {
    let (mut tree, player) = boot("player/player.tscn");
    let skeleton = id_of(&tree, player, "Player/Skeleton/Skeleton3D");
    let at = id_of(&tree, player, "AnimationTree");
    let anim_tree = tree.get_animation_tree_mut(at).expect("AnimationTree attached");
    for (name, v) in [("run", run), ("speed", speed), ("state", state), ("air_dir", air_dir)] {
        anim_tree.set_parameter(format!("parameters/{name}/blend_amount"), Variant::Float(v));
    }
    for _ in 0..frames {
        anim_tree.advance(1.0 / 60.0);
    }
    skeleton3d::process_skeletons(&mut tree);
    (tree, skeleton)
}

/// Floor locomotion: idle 0.4 / walk 0.3 / run 0.3 through Blend2 nodes and the
/// 1.5x TimeScale.
///
/// ⚠️ The tolerance is NOT slack in the fix, it is a known, separate gap:
/// Godot's mixer accumulates rotations from REST
/// (`rot = rot * slerp(I, rest^-1 * q, w)` per clip, `animation_mixer.cpp`
/// `interpolate_via_rest`) while Patina slerps the two inputs of each Blend2.
/// Measured over all 27 bones: 0.063 worst (l-forearm), 0.002-0.02 for the
/// rest. A single-clip tree (run=1, speed=1) matches to 3e-6, and before the
/// channel fix the hip alone was 0.7 off — so 0.08 still fails the bug.
#[test]
fn player_animation_tree_floor_blend_poses_bones_like_godot() {
    let _g = ROOTS.lock().unwrap_or_else(|e| e.into_inner());
    let (tree, skeleton) = tree_pose(0.6, 0.5, 0.0, 0.5, 30);
    assert_bones(
        &tree,
        skeleton,
        &[
            ("hip", [0.999534, -0.012885, 0.027681, 0.011468, 0.998646, 0.050739, -0.028297, -0.050398, 0.998328, -0.003680, 2.040161, 0.443860]),
            ("chest", [0.992657, 0.008815, -0.120640, 0.015676, 0.979555, 0.200564, 0.119942, -0.200982, 0.972224, 0.009452, 3.238401, 0.574183]),
            ("l-forearm", [-0.820371, -0.321271, -0.473050, 0.129979, -0.910365, 0.392861, -0.556864, 0.260805, 0.788595, -1.449499, 3.345957, 0.513669]),
            ("r-foot", [-0.994265, -0.074277, 0.077139, 0.100243, -0.392244, 0.914383, -0.037657, 0.916859, 0.397437, 0.605703, 0.544136, 0.213666]),
        ],
        0.08,
        "AnimationTree floor blend",
    );
}

/// pat-3mbf5: the air branch alone (`state` = 1, `air_dir` = 0: `jump`,
/// 0.433 s, NOT under the TimeScale) at 0.5 s. The fixture ships no
/// `.glb.import`, so in Godot the clip does not loop and holds its last frame;
/// Patina's leaf used to wrap it to 0.067 s regardless. One clip at weight 1,
/// so no blending is involved and Godot is matched to 5e-6.
#[test]
fn player_animation_tree_jump_holds_its_last_frame_like_godot() {
    let _g = ROOTS.lock().unwrap_or_else(|e| e.into_inner());
    let (tree, skeleton) = tree_pose(0.6, 0.5, 1.0, 0.0, 30);
    assert_bones(
        &tree,
        skeleton,
        &[
            ("hip", [0.984808, -0.024125, 0.171964, 0.000000, 0.990303, 0.138927, -0.173648, -0.136816, 0.975258, 0.050238, 1.985603, 0.698392]),
            ("vent", [0.999774, 0.000004, 0.021254, 0.002289, 0.994167, -0.107824, -0.021130, 0.107849, 0.993942, 0.086425, 5.815497, 1.822357]),
            ("HEAD", [1.000000, -0.000000, 0.000000, -0.000000, -1.000000, -0.000001, 0.000000, 0.000001, -1.000000, -0.000000, 6.343324, 5.887968]),
            ("l-forearm", [-0.789886, -0.150032, 0.594618, 0.330569, 0.712517, 0.618905, -0.516531, 0.685427, -0.513211, -1.008846, 3.741643, 2.541861]),
        ],
        1e-4,
        "AnimationTree jump held",
    );
}

/// pat-3mbf5: idle alone for 90 frames is 2.25 s into a 1.233 s clip (1.5x
/// TimeScale). Without its `.import` sidecar the clip does not loop in Godot,
/// so it rests on its last frame — matched here to 1e-4, no blending involved.
#[test]
fn player_animation_tree_idle_without_sidecar_holds_its_last_frame_like_godot() {
    let _g = ROOTS.lock().unwrap_or_else(|e| e.into_inner());
    let (tree, skeleton) = tree_pose(0.0, 0.0, 0.0, 0.5, 90);
    assert_bones(
        &tree,
        skeleton,
        &[
            ("hip", [1.000000, 0.000000, -0.000000, 0.000000, 1.000000, 0.000001, 0.000000, -0.000001, 1.000000, 0.000000, 2.043380, 0.394217]),
            ("head", [1.000000, -0.000000, -0.000002, 0.000000, 0.999650, 0.026433, 0.000002, -0.026433, 0.999650, 0.000000, 4.976381, 0.236348]),
            ("r-forearm", [-0.943256, 0.083408, 0.321420, -0.040236, -0.989517, 0.138699, 0.329619, 0.117896, 0.936724, 1.530182, 3.305705, 0.382994]),
            ("l-foot", [-0.999651, 0.026910, 0.002040, 0.001038, -0.037307, 0.999307, 0.026961, 0.998942, 0.037266, -0.739151, 0.193491, 0.322822]),
        ],
        1e-4,
        "AnimationTree idle clamped",
    );
}

/// The enemy's clips are authored in `enemy.tscn` (`position_3d`/`rotation_3d`/
/// `scale_3d` per bone, the same shared-path shape) and replace the glb's
/// library. `walk`@0.3 s moves bones up to 0.36 from rest; Godot matched to 3e-7.
#[test]
fn enemy_walk_clip_poses_every_bone_like_godot() {
    let _g = ROOTS.lock().unwrap_or_else(|e| e.into_inner());
    let (mut tree, enemy) = boot("enemy/enemy.tscn");
    let skeleton = id_of(&tree, enemy, "Enemy/Skeleton/Skeleton3D");
    let ap = id_of(&tree, enemy, "Enemy/AnimationPlayer");
    let player = tree.get_animation_player_mut(ap).unwrap();
    player.play("walk");
    player.seek(0.3, true);
    skeleton3d::process_skeletons(&mut tree);
    assert_bones(
        &tree,
        skeleton,
        &[
            ("body", [1.000000, -0.000001, -0.000000, -0.000000, -0.011693, 0.999932, -0.000001, -0.999932, -0.011693, -0.000000, 0.657529, 0.356541]),
            ("eyes", [-1.000000, 0.000000, -0.000000, -0.000000, 0.999931, 0.011693, 0.000000, 0.011693, -0.999931, -0.000000, 0.713674, 0.832957]),
            ("r-frontleg001", [-1.000000, -0.000000, -0.000000, 0.000000, -0.000001, 1.000000, -0.000000, 1.000000, 0.000000, 0.913587, 0.047126, 0.684520]),
            ("l-backleg001", [-1.000000, -0.000000, -0.000000, 0.000000, -0.000001, 1.000000, -0.000000, 1.000000, 0.000000, -1.054360, 0.047127, -0.387757]),
        ],
        1e-4,
        "enemy AnimationPlayer walk@0.3",
    );
}

/// Both of the robot's surfaces are deformed by the skeleton. Godot's own
/// numbers: in `run`@0.4 the first vertex of each surface (both weighted 1.0
/// to bind 6, `head`) lands at `global_pose(head) * bind_pose(head) * v`,
/// computed from Godot's captured pose, bind and `surface_get_arrays`.
#[test]
fn player_glb_every_robot_surface_is_skinned_like_godot() {
    let _g = ROOTS.lock().unwrap_or_else(|e| e.into_inner());
    let (mut tree, glb) = boot("player/player.glb");
    let robot = id_of(&tree, glb, "Skeleton/Skeleton3D/Robot");
    let ap = id_of(&tree, glb, "AnimationPlayer");
    let player = tree.get_animation_player_mut(ap).unwrap();
    player.play("run");
    player.seek(0.4, true);
    skeleton3d::process_skeletons(&mut tree);

    let base = RenderServer3DAdapter::resolve_mesh(&tree, robot);
    assert_eq!(base.surfaces.len(), 1, "two glTF primitives: primary + one extra surface");
    assert!(base.surfaces[0].is_skinned(), "the second primitive's JOINTS_0/WEIGHTS_0 must be kept");
    let posed = deformed_mesh_for_node(&tree, robot, &base).expect("the robot is skinned");

    // (raw vertex from Godot's surface arrays, Godot-skinned position)
    let cases: [(&[Vector3], &[Vector3], Vector3, Vector3); 2] = [
        (
            &base.vertices,
            &posed.vertices,
            Vector3::new(0.098_730_57, 1.642_970_4, 0.299_999_24),
            Vector3::new(0.258_371_87, 5.126_818_6, 2.758_153_3),
        ),
        (
            &base.surfaces[0].vertices,
            &posed.surfaces[0].vertices,
            Vector3::new(0.479_882_7, 1.517_579_3, 0.030_925_506),
            Vector3::new(1.506_788, 4.797_68, 1.795_623_5),
        ),
    ];
    for (surface, (raw, deformed, godot_raw, godot_skinned)) in cases.iter().enumerate() {
        let k = raw
            .iter()
            .position(|v| (*v - *godot_raw).length() < 1e-5)
            .unwrap_or_else(|| panic!("surface {surface}: Godot's vertex 0 must exist in Patina's import"));
        let got = deformed[k];
        assert!(
            (got - *godot_skinned).length() < 1e-3,
            "surface {surface}: skinned vertex {k} is {got:?}, Godot puts it at {godot_skinned:?}"
        );
    }
}
