//! pat-mgixk.3 — a bone pose is the bone's FULL LOCAL TRANSFORM, so an
//! absolute animation key must not have the rest applied on top of it.
//!
//! Godot ground truth: `upstream/godot/scene/3d/skeleton_3d.cpp:1108-1116`
//! composes `global_pose = parent.global_pose * pose_cache`; the rest is only
//! the pose's default and is consulted on its own just for a *disabled* bone.
//! The scene format stores a pose as `bones/N/position|rotation|scale`
//! (`:98-102`, `:199-201`), and `reset_bone_pose` (`:908`) copies rest → pose.
//!
//! Patina composed `parent_global * rest * pose` — Godot *3* semantics, where
//! the pose was an offset layered onto the rest. Because
//! `apply_animation_bone_tracks` samples ABSOLUTE local values and writes them
//! straight through `set_bone_pose_*`, every imported rig with non-identity
//! rests had its rest applied **twice** and came out distorted. The pose
//! getters also defaulted to `ZERO`/`IDENTITY`/`ONE` instead of the rest, and
//! Patina read only its own `bones/N/pose_position` spelling, so a pose saved
//! by Godot was silently dropped.
//!
//! Entry point, per the bead's wiring requirement — no direct calls into the
//! composition under test:
//!
//! ```text
//! .glb (JOINTS_0/WEIGHTS_0) + .tscn  ->  PackedScene  ->  SceneTree
//!   ->  MainLoop::step  ->  skeleton3d::process_skeletons_with_delta
//!        (apply_animation_bone_tracks -> bone poses)
//!   ->  RenderServer3DAdapter::render_frame -> bone_pose_array -> skin_mesh
//!   ->  RenderServer3DAdapter::skinned_vertices
//! ```
//!
//! ⚠️ The discriminating fixture is a clip whose keys EQUAL the rest. Under
//! Godot's semantics that is a no-op and the rig must render its bind pose
//! exactly; under the old composition the rest lands twice and the skinned
//! vertices shift by a whole bone length. A clip keyed to anything else would
//! move the mesh under BOTH semantics and prove nothing.

use gdcore::math::Vector3;
use gdscene::animation::{Animation, AnimationPlayer, AnimationTrack, KeyFrame, TrackType};
use gdscene::node::Node;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::render_server_3d::RenderServer3DAdapter;
use gdscene::scene_tree::SceneTree;
use gdscene::{skeleton3d, GDScriptNodeInstance, LifecycleManager, MainLoop, NodeId};
// `list_properties` below is a trait method; without this the read of a script
// var does not resolve.
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

const W: u32 = 64;
const H: u32 = 64;

/// One unit up — bone 1's rest offset, and the value the clip keys.
const TIP_REST_Y: f32 = 1.0;

/// Builds a GLB holding one rigged quad.
///
/// Four vertices spanning x ∈ [-0.4, 0.4], y ∈ [0, 1] in the z = 0 plane. The
/// two bottom vertices are fully weighted to joint 0 and the two top vertices to
/// joint 1, so moving bone 1 shears the top of the quad — the same rig shape the
/// `gdserver3d::skin` unit tests use, but delivered through the glTF importer so
/// the skinning arrays reach the scene the way a real asset would.
fn rigged_quad_glb() -> Vec<u8> {
    let positions: [[f32; 3]; 4] = [
        [-0.4, 0.0, 0.0], // 0 bottom-left  -> joint 0
        [0.4, 0.0, 0.0],  // 1 bottom-right -> joint 0
        [0.4, 1.0, 0.0],  // 2 top-right    -> joint 1
        [-0.4, 1.0, 0.0], // 3 top-left     -> joint 1
    ];
    let normals: [[f32; 3]; 4] = [[0.0, 0.0, 1.0]; 4];
    let joints: [[u8; 4]; 4] = [[0, 0, 0, 0], [0, 0, 0, 0], [1, 0, 0, 0], [1, 0, 0, 0]];
    let weights: [[f32; 4]; 4] = [[1.0, 0.0, 0.0, 0.0]; 4];
    let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];

    let mut bin = Vec::new();
    for p in &positions {
        for &v in p {
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    debug_assert_eq!(bin.len(), 48);
    for n in &normals {
        for &v in n {
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    debug_assert_eq!(bin.len(), 96);
    for j in &joints {
        bin.extend_from_slice(j);
    }
    debug_assert_eq!(bin.len(), 112);
    for w in &weights {
        for &v in w {
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    debug_assert_eq!(bin.len(), 176);
    for &i in &indices {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    while bin.len() % 4 != 0 {
        bin.push(0);
    }

    let json = serde_json::json!({
        "asset": { "version": "2.0" },
        "buffers": [{ "byteLength": bin.len() }],
        "bufferViews": [
            { "buffer": 0, "byteOffset": 0,   "byteLength": 48, "target": 34962 },
            { "buffer": 0, "byteOffset": 48,  "byteLength": 48, "target": 34962 },
            { "buffer": 0, "byteOffset": 96,  "byteLength": 16, "target": 34962 },
            { "buffer": 0, "byteOffset": 112, "byteLength": 64, "target": 34962 },
            { "buffer": 0, "byteOffset": 176, "byteLength": 12, "target": 34963 }
        ],
        "accessors": [
            {
                "bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3",
                "max": [0.4, 1.0, 0.0], "min": [-0.4, 0.0, 0.0]
            },
            { "bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC3" },
            // JOINTS_0 — UNSIGNED_BYTE VEC4
            { "bufferView": 2, "componentType": 5121, "count": 4, "type": "VEC4" },
            // WEIGHTS_0 — FLOAT VEC4
            { "bufferView": 3, "componentType": 5126, "count": 4, "type": "VEC4" },
            { "bufferView": 4, "componentType": 5123, "count": 6, "type": "SCALAR" }
        ],
        "meshes": [{
            "name": "RiggedQuad",
            "primitives": [{
                "attributes": { "POSITION": 0, "NORMAL": 1, "JOINTS_0": 2, "WEIGHTS_0": 3 },
                "indices": 4
            }]
        }]
    });

    let mut json_bytes = serde_json::to_vec(&json).unwrap();
    while json_bytes.len() % 4 != 0 {
        json_bytes.push(b' ');
    }

    let total_length = 12 + 8 + json_bytes.len() + 8 + bin.len();
    let mut glb = Vec::with_capacity(total_length);

    // GLB header
    glb.extend_from_slice(&0x4654_6C67u32.to_le_bytes()); // "glTF"
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&(total_length as u32).to_le_bytes());

    // JSON chunk
    glb.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
    glb.extend_from_slice(&json_bytes);

    // BIN chunk
    glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x004E_4942u32.to_le_bytes());
    glb.extend_from_slice(&bin);

    glb
}

/// The rig, with a NON-IDENTITY rest on bone 1 and no modifier.
///
/// No `IK` node here on purpose: this test is about the animation → pose →
/// skin path, and a solver writing its own poses would mask the double
/// transform being measured.
fn rig_tscn(mesh_path: &str) -> String {
    format!(
        r#"[gd_scene format=3]

[node name="Rig" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0.5, 2)
current = true

[node name="Skeleton3D" type="Skeleton3D" parent="."]
bones/0/name = "Root"
bones/0/parent = -1
bones/0/rest = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0)
bones/1/name = "Tip"
bones/1/parent = 0
bones/1/rest = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0)

[node name="Mesh" type="MeshInstance3D" parent="Skeleton3D"]
mesh = "{mesh_path}"
"#
    )
}

fn load(tscn: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    // The root must be inside the tree BEFORE instancing, or `add_child` never
    // propagates `enter_tree` and the script phase below never runs.
    LifecycleManager::enter_tree(&mut tree, root);
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

fn find(tree: &SceneTree, name: &str) -> NodeId {
    tree.all_nodes_in_tree_order()
        .into_iter()
        .find(|&id| tree.get_node(id).is_some_and(|n| n.name() == name))
        .unwrap_or_else(|| panic!("node {name} must exist in the fixture scene"))
}

/// The bind pose of the rigged quad: x ∈ [-0.4, 0.4], y ∈ [0, 1].
const BIND_POSE: [(f32, f32); 4] = [(-0.4, 0.0), (0.4, 0.0), (0.4, 1.0), (-0.4, 1.0)];

fn assert_bind_pose(verts: &[Vector3], context: &str) {
    assert_eq!(verts.len(), 4, "rigged quad has four vertices ({context})");
    for (v, (x, y)) in verts.iter().zip(BIND_POSE.iter()) {
        assert!(
            (v.x - x).abs() < 1e-4 && (v.y - y).abs() < 1e-4,
            "{context}: vertex {v:?} left the bind pose ({x}, {y}, 0). A shift of \
             about {TIP_REST_Y} in y on the TOP two vertices is the rest applied twice."
        );
    }
}

#[test]
fn skeleton3d_absolute_pose_skinning_scene() {
    let dir = tempfile::TempDir::new().unwrap();
    let glb_path = dir.path().join("rig.glb");
    std::fs::write(&glb_path, rigged_quad_glb()).unwrap();

    let mut tree = load(&rig_tscn(glb_path.to_str().unwrap()));
    let mesh_id = find(&tree, "Mesh");
    let rig_id = find(&tree, "Rig");

    // An AnimationPlayer whose only key EQUALS bone 1's rest origin — exactly
    // what a glTF clip authored at the bind pose contains.
    let player_node = tree
        .add_child(rig_id, Node::new("AnimationPlayer", "AnimationPlayer"))
        .expect("add AnimationPlayer");
    let mut anim = Animation::new("bind", 1.0);
    let mut track = AnimationTrack::with_type("Skeleton3D", "Tip", TrackType::Position3D);
    track.add_keyframe(KeyFrame::linear(
        0.0,
        Variant::Vector3(Vector3::new(0.0, TIP_REST_Y, 0.0)),
    ));
    anim.tracks.push(track);
    let mut player = AnimationPlayer::new();
    player.root_node = "..".into();
    player.add_animation(anim);
    player.play("bind");
    tree.attach_animation_player(player_node, player)
        .expect("attach AnimationPlayer");

    let mut ml = MainLoop::new(tree);
    let mut adapter = RenderServer3DAdapter::new(W, H);

    // Before stepping, no pose has been written at all. This must already be
    // the bind pose, which is the half that was never broken — it is here so a
    // failure below cannot be blamed on the fixture or the importer.
    let (_, _) = adapter.render_frame(ml.tree());
    let rest_verts = adapter
        .skinned_vertices(mesh_id)
        .expect(
            "a MeshInstance3D parented to a Skeleton3D must be skinned by the render \
             bridge (glTF JOINTS_0/WEIGHTS_0 must survive import)",
        )
        .to_vec();
    assert_bind_pose(&rest_verts, "unposed rig");

    // Stepping runs the mixer and then the skeleton phase, which routes the
    // bone track onto bone 1's pose and re-skins the mesh.
    for _ in 0..3 {
        ml.step(1.0 / 60.0);
    }
    let (snap, _) = adapter.render_frame(ml.tree());
    assert_eq!(
        snap.visible_mesh_count, 1,
        "the rigged mesh should still be the one visible instance"
    );
    let posed_verts = adapter
        .skinned_vertices(mesh_id)
        .expect("skinned vertices after stepping")
        .to_vec();

    // 🔑 The whole bead: the clip keys the bone's ABSOLUTE local position, which
    // already equals its rest, so the rig must not move at all.
    assert_bind_pose(&posed_verts, "after an animation keyed to the rest");

    // And the same statement one layer down, so a failure says which layer.
    let tip_global = skeleton3d::get_bone_global_pose(ml.tree(), find(ml.tree(), "Skeleton3D"), 1);
    assert!(
        (tip_global.origin.y - TIP_REST_Y).abs() < 1e-4,
        "bone 1's global pose must be its rest ({TIP_REST_Y}), not double it: {:?}",
        tip_global.origin
    );
}

/// A pose saved by Godot 4 under `bones/N/position|rotation` is honoured on
/// load. Patina read only its own `bones/N/pose_*` spelling, so these keys were
/// dropped and the bone sat at its rest regardless of what the scene said.
#[test]
fn godot_saved_bone_pose_keys_are_honoured_on_load() {
    // A 90° turn about Z, written the way Godot stores a bone pose.
    const S: f32 = std::f32::consts::FRAC_1_SQRT_2;
    let tscn = r#"[gd_scene format=3]

[node name="Rig" type="Node3D"]

[node name="Skeleton3D" type="Skeleton3D" parent="."]
bones/0/name = "Root"
bones/0/parent = -1
bones/0/rest = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0)
bones/0/rotation = Quaternion(0, 0, 0.70710677, 0.70710677)
bones/1/name = "Tip"
bones/1/parent = 0
bones/1/rest = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0)
bones/1/position = Vector3(0, 3, 0)
"#;
    let tree = load(tscn);
    let skel = find(&tree, "Skeleton3D");

    let rot = skeleton3d::get_bone_pose_rotation(&tree, skel, 0);
    assert!(
        (rot.z - S).abs() < 1e-4 && (rot.w - S).abs() < 1e-4,
        "a `bones/0/rotation` saved by Godot must become bone 0's pose rotation, \
         got {rot:?}"
    );
    let pos = skeleton3d::get_bone_pose_position(&tree, skel, 1);
    assert!(
        (pos.y - 3.0).abs() < 1e-4,
        "a `bones/1/position` saved by Godot must become bone 1's pose position, \
         got {pos:?}"
    );
    // Unset components still fall back to the rest, not to zero/identity.
    let unset = skeleton3d::get_bone_pose_position(&tree, skel, 0);
    assert!(
        unset.length() < 1e-4,
        "bone 0's rest origin is the zero vector, so its pose position is too: {unset:?}"
    );
}

/// Reading a script var: this target's scripts expose their vars through
/// `list_properties`, not `get_property`.
fn script_var(tree: &SceneTree, node: NodeId, name: &str) -> Variant {
    tree.get_script(node)
        .expect("script attached")
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("no var `{name}`"))
        .default_value
}

/// `get_bone_pose_position` reads as the bone's REST from GDScript when nothing
/// has posed it. It answered the ZERO vector before, so a script could not tell
/// a bone sitting at its bind pose from one collapsed onto its parent.
#[test]
fn unanimated_bone_pose_reads_as_its_rest_from_gdscript() {
    let tscn = r#"[gd_scene format=3]

[node name="Rig" type="Node3D"]

[node name="Skeleton3D" type="Skeleton3D" parent="."]
bones/0/name = "Root"
bones/0/parent = -1
bones/0/rest = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0)
bones/1/name = "Tip"
bones/1/parent = 0
bones/1/rest = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0)
"#;
    // ⚠️ The driver script lives on `Rig`, NOT on the Skeleton3D:
    // `call_script_method` takes the running script out of the arena, so a
    // script reaching back into its own node would find it checked out.
    const SRC: &str = "\
extends Node3D
var tip_pose = Vector3(9, 9, 9)
func run():
    var s = get_node(\"Skeleton3D\")
    self.tip_pose = s.get_bone_pose_position(1)
";

    let mut tree = load(tscn);
    let rig = find(&tree, "Rig");
    let skel = find(&tree, "Skeleton3D");
    let script = GDScriptNodeInstance::from_source(SRC, rig).expect("script must parse");
    tree.attach_script(rig, Box::new(script));
    tree.call_script_method(rig, "run", &[]);

    assert_eq!(
        script_var(&tree, rig, "tip_pose"),
        Variant::Vector3(Vector3::new(0.0, TIP_REST_Y, 0.0)),
        "an unposed bone's pose must read as its REST from GDScript, not as zero",
    );
    // The engine store agrees, so this is the pose the renderer samples too.
    assert_eq!(
        skeleton3d::get_bone_pose_position(&tree, skel, 1),
        Vector3::new(0.0, TIP_REST_Y, 0.0),
    );
}
