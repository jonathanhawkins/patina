//! pat-0q2kx.3 — a `Skeleton3D` in a running scene actually deforms the mesh
//! bound to it.
//!
//! Godot ground truth: `upstream/godot/scene/3d/skeleton_3d.cpp` defers
//! `NOTIFICATION_UPDATE_SKELETON` every frame, runs the child
//! `SkeletonModifier3D` stack, updates `BoneAttachment3D` overrides, and
//! publishes the final global bone poses to the RenderingServer's skeleton
//! storage, which skins the bound `MeshInstance3D`'s vertices.
//!
//! Patina had all three pieces as libraries with their own tests
//! (`skeleton3d::process_modifiers`, `skeleton3d::update_bone_attachment`,
//! `gdserver3d::skin::skin_mesh`) and **no runtime caller** for any of them, so
//! a rigged scene rendered its undeformed bind pose forever. This test proves
//! the whole chain through the real scene path, with no direct calls into those
//! library functions:
//!
//! ```text
//! .glb (JOINTS_0/WEIGHTS_0) + .tscn  ->  PackedScene  ->  SceneTree
//!   ->  MainLoop::step (skeleton phase: modifiers -> bone poses)
//!   ->  RenderServer3DAdapter::render_frame (skin -> rasterize)
//! ```

use gdcore::math::Color;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::render_server_3d::RenderServer3DAdapter;
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;

const W: u32 = 64;
const H: u32 = 64;

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

/// A two-bone rig driven by a `FABRIK3D` modifier.
///
/// * `Skeleton3D` — bone 0 "Root" at the origin, bone 1 "Tip" resting one unit
///   up its parent. Written as Godot writes them: `bones/{idx}/…` records with
///   no explicit count.
/// * `IK` — a `FABRIK3D` whose target is off to the right, so solving swings the
///   chain away from its rest pose.
/// * `Mesh` — the rigged quad, parented to the skeleton, so Godot's default
///   `MeshInstance3D.skeleton = NodePath("..")` binds it without any property.
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

[node name="IK" type="FABRIK3D" parent="Skeleton3D"]
root_bone = 0
tip_bone = 1
iterations = 10
target_position = Vector3(2, 1, 0)

[node name="Mesh" type="MeshInstance3D" parent="Skeleton3D"]
mesh = "{mesh_path}"
"#
    )
}

fn load(tscn: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

fn nonblack(pixels: &[Color]) -> usize {
    pixels.iter().filter(|c| **c != Color::BLACK).count()
}

#[test]
fn skeleton_pose_deforms_rendered_mesh_from_scene() {
    let dir = tempfile::TempDir::new().unwrap();
    let glb_path = dir.path().join("rig.glb");
    std::fs::write(&glb_path, rigged_quad_glb()).unwrap();

    let tree = load(&rig_tscn(glb_path.to_str().unwrap()));
    let mesh_id = tree
        .get_node_by_path("/root/Rig/Skeleton3D/Mesh")
        .expect("MeshInstance3D node");

    let mut ml = MainLoop::new(tree);
    let mut adapter = RenderServer3DAdapter::new(W, H);

    // -- rest ---------------------------------------------------------------
    // Nothing has stepped yet, so every bone pose is identity. The default skin
    // is built from the bone rests, so each per-bind matrix collapses to the
    // identity and the rig must render its bind pose exactly.
    let (rest_snap, rest_frame) = adapter.render_frame(ml.tree());
    let rest_verts = adapter
        .skinned_vertices(mesh_id)
        .expect(
            "MeshInstance3D parented to a Skeleton3D must be skinned by the render bridge \
             (glTF JOINTS_0/WEIGHTS_0 must survive import)",
        )
        .to_vec();
    let rest_pixels = rest_frame.pixels.clone();

    assert_eq!(
        rest_snap.visible_mesh_count, 1,
        "the rigged mesh should be the one visible instance"
    );
    assert_eq!(rest_verts.len(), 4, "rigged quad has four vertices");
    assert!(
        nonblack(&rest_pixels) > 20,
        "the rig should actually rasterize at rest ({} non-black px)",
        nonblack(&rest_pixels)
    );
    // Rest-pose skinning is the identity — the defining correctness property.
    let bind_pose = [(-0.4f32, 0.0f32), (0.4, 0.0), (0.4, 1.0), (-0.4, 1.0)];
    for (v, (x, y)) in rest_verts.iter().zip(bind_pose.iter()) {
        assert!(
            (v.x - x).abs() < 1e-4 && (v.y - y).abs() < 1e-4,
            "rest pose moved a vertex: {v:?} != ({x}, {y}, 0)"
        );
    }

    // -- posed --------------------------------------------------------------
    // One frame of the main loop runs the skeleton phase: the FABRIK3D modifier
    // swings the chain toward its target and writes bone poses. The render
    // bridge must then skin the mesh with those live poses.
    for _ in 0..3 {
        ml.step(1.0 / 60.0);
    }
    let (posed_snap, posed_frame) = adapter.render_frame(ml.tree());
    let posed_verts = adapter
        .skinned_vertices(mesh_id)
        .expect("skinned vertices after stepping")
        .to_vec();
    let posed_pixels = posed_frame.pixels.clone();

    assert_eq!(posed_snap.visible_mesh_count, 1);
    assert_ne!(
        posed_verts, rest_verts,
        "stepping the main loop with a FABRIK3D on the skeleton must deform the \
         mesh the render path draws — the skeleton is still unwired if these match"
    );
    // The top of the quad follows bone 1, which the solve swung to the right.
    assert!(
        posed_verts[2].x > rest_verts[2].x + 0.1,
        "bone 1's verts should swing toward the +X target (rest {:?} -> posed {:?})",
        rest_verts[2],
        posed_verts[2]
    );
    assert!(
        nonblack(&posed_pixels) > 20,
        "the posed rig should still rasterize ({} non-black px)",
        nonblack(&posed_pixels)
    );
    assert_ne!(
        posed_pixels, rest_pixels,
        "the deformed rig must produce a different framebuffer than its bind pose"
    );
}

/// A `BoneAttachment3D` child of a scene-loaded `Skeleton3D` follows its bone
/// once the main loop steps — the second half of Godot's per-frame skeleton
/// update (`bone_attachment_3d.cpp`), which also had no runtime caller.
#[test]
fn bone_attachment_follows_bone_after_step() {
    let tscn = r#"[gd_scene format=3]

[node name="Rig" type="Node3D"]

[node name="Skeleton3D" type="Skeleton3D" parent="."]
bones/0/name = "Root"
bones/0/parent = -1
bones/0/rest = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0)
bones/1/name = "Tip"
bones/1/parent = 0
bones/1/rest = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 0)

[node name="Hat" type="BoneAttachment3D" parent="Skeleton3D"]
bone_name = "Tip"
"#;

    let tree = load(tscn);
    let hat_id = tree
        .get_node_by_path("/root/Rig/Skeleton3D/Hat")
        .expect("BoneAttachment3D node");

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    let t = gdscene::node3d::get_global_transform(ml.tree(), hat_id);
    assert!(
        (t.origin.y - 2.0).abs() < 1e-4,
        "BoneAttachment3D should sit at its bone's global pose (y=2), got {:?}",
        t.origin
    );
}
