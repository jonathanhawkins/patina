//! pat-s3rrv.3 — the glTF scene importer must produce a real `PackedScene`.
//!
//! `GltfSceneImporter::import_scene` used to read the file's bytes and then
//! **fabricate** its output: one `MeshInstance3D` named `<stem>_Mesh` and, when
//! `content.len() > 100`, an animation literally named `"default"`. Nothing
//! parsed the glTF, nothing honoured Godot's node-name suffixes, and no
//! `PackedScene` was ever written — `SceneFormatImporterRegistry` had no caller
//! outside its own re-export.
//!
//! These tests drive the real path end to end: a `.glb` container →
//! `gdresource::import_gltf_bytes` → `ImportedScene` → `.scn` on disk →
//! `gdscene::PackedScene::from_tscn` → an instanced node tree.
//!
//! **Why this is a top-level test file** and not a module inside
//! `tests/editor_suite_*` (AGENTS.md:89): reporting a grouped test as the bare
//! `--test editor_suite_x` runs the whole suite under nextest's fail-fast, so an
//! unrelated red anywhere in that group cancels the run and fails this bead for
//! something it did not touch — and those suites are where concurrent agents
//! pile up. `cargo nextest run -p gdeditor --test gltf_scene_import` names only
//! this file's tests.
//!
//! Note for anyone editing this header: there is **no 58-character truncation**
//! of reported test commands. `apps/orchestrator/crate/src/verifier.rs:626`
//! `tail_chars` is applied only to failure *output* (800/400 chars at :1511 and
//! :1520); `run_command_capture` passes the command whole to `bash -lc`. Do not
//! shorten a test filename for command length — that folklore is wrong and has
//! already produced beads naming binaries that do not exist.

use std::collections::HashMap;

use gdeditor::import::{
    default_scene_importer_registry, import_scene_to_packed_scene, split_name_suffix,
    NodeNameSuffix,
};
use gdscene::animation_tscn::build_animation_player;
use gdscene::node::{Node, NodeId};
use gdscene::packed_scene::PackedScene;
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

/// Wraps a glTF JSON document and its binary blob in a real `.glb` container,
/// so the test exercises the same entry point a shipped asset would.
fn glb(json: &str, bin: &[u8]) -> Vec<u8> {
    let mut json_chunk = json.as_bytes().to_vec();
    while json_chunk.len() % 4 != 0 {
        json_chunk.push(b' '); // JSON chunks pad with spaces
    }
    let mut bin_chunk = bin.to_vec();
    while bin_chunk.len() % 4 != 0 {
        bin_chunk.push(0); // BIN chunks pad with zeros
    }

    let total = 12 + 8 + json_chunk.len() + 8 + bin_chunk.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());

    out.extend_from_slice(&(json_chunk.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F_534Au32.to_le_bytes()); // "JSON"
    out.extend_from_slice(&json_chunk);

    out.extend_from_slice(&(bin_chunk.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x004E_4942u32.to_le_bytes()); // "BIN\0"
    out.extend_from_slice(&bin_chunk);
    out
}

/// The acceptance fixture: two nodes (`Root` → `Body-col`), where `Body-col`
/// draws a one-primitive triangle mesh and a clip named `Walk` slides it +2 on X
/// over half a second.
///
/// Buffer layout (74 B, padded to 76): 3 VEC3 positions (36) | 3 u16 indices (6)
/// | 2 SCALAR times (8) | 2 VEC3 translations (24).
fn body_col_glb() -> Vec<u8> {
    let mut bin = Vec::new();
    for p in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        for v in p {
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    for i in [0u16, 1, 2] {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    assert_eq!(bin.len(), 42, "fixture buffer layout changed");
    bin.extend_from_slice(&[0, 0]); // pad the times accessor to a 4-byte offset
    for t in [0.0f32, 0.5] {
        bin.extend_from_slice(&t.to_le_bytes());
    }
    for p in [[0.0f32, 0.0, 0.0], [2.0, 0.0, 0.0]] {
        for v in p {
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    assert_eq!(bin.len(), 76, "fixture buffer layout changed");

    let json = r#"{
      "asset": {"version": "2.0"},
      "scene": 0,
      "scenes": [{"nodes": [0]}],
      "nodes": [
        {"name": "Root", "children": [1]},
        {"name": "Body-col", "mesh": 0}
      ],
      "meshes": [{
        "name": "BodyMesh",
        "primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]
      }],
      "animations": [{
        "name": "Walk",
        "samplers": [{"input": 2, "output": 3, "interpolation": "LINEAR"}],
        "channels": [{"sampler": 0, "target": {"node": 1, "path": "translation"}}]
      }],
      "buffers": [{"byteLength": 76}],
      "bufferViews": [
        {"buffer": 0, "byteOffset": 0,  "byteLength": 36},
        {"buffer": 0, "byteOffset": 36, "byteLength": 6},
        {"buffer": 0, "byteOffset": 44, "byteLength": 8},
        {"buffer": 0, "byteOffset": 52, "byteLength": 24}
      ],
      "accessors": [
        {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"},
        {"bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR"},
        {"bufferView": 2, "componentType": 5126, "count": 2, "type": "SCALAR"},
        {"bufferView": 3, "componentType": 5126, "count": 2, "type": "VEC3"}
      ]
    }"#;
    glb(json, &bin)
}

// ---------------------------------------------------------------------------
// Helpers over an instanced tree
// ---------------------------------------------------------------------------

fn find_by_name<'a>(nodes: &'a [Node], name: &str) -> Option<&'a Node> {
    nodes.iter().find(|n| n.name() == name)
}

fn children_of<'a>(nodes: &'a [Node], parent: NodeId) -> Vec<&'a Node> {
    nodes
        .iter()
        .filter(|n| n.parent() == Some(parent))
        .collect()
}

/// Imports the fixture into a temp project and returns the instanced tree plus
/// the parsed scene (needed to resolve the AnimationPlayer's library).
fn import_fixture(dir: &std::path::Path) -> (PackedScene, Vec<Node>) {
    let source = dir.join("character.glb");
    std::fs::write(&source, body_col_glb()).unwrap();

    let registry = default_scene_importer_registry();
    let written = import_scene_to_packed_scene(&registry, &source, dir)
        .expect("a real .glb must import to a PackedScene file");

    assert!(
        written.starts_with(dir.join(".godot").join("imported")),
        "the PackedScene must land under .godot/imported/, got {}",
        written.display()
    );

    let text = std::fs::read_to_string(&written).unwrap();
    let scene = PackedScene::from_tscn(&text)
        .unwrap_or_else(|e| panic!("the written scene must parse back: {e}\n{text}"));
    let nodes = scene.instance().expect("the scene must instantiate");
    (scene, nodes)
}

// ---------------------------------------------------------------------------
// The bead's acceptance test
// ---------------------------------------------------------------------------

/// **The acceptance test.** A real 2-node `.glb` whose mesh is named `Body-col`
/// and whose single clip is named `Walk` imports to a `PackedScene` that:
/// keeps the glTF's real node name with the suffix stripped (`Body`), grows the
/// `StaticBody3D` + `CollisionShape3D` the `-col` suffix asks for, carries an
/// `AnimationPlayer` holding `Walk` (never the old fabricated `"default"`), and
/// reports the glTF's real primitive count as its mesh count.
#[test]
fn gltf_scene_import_produces_packed_scene() {
    let dir = tempfile::TempDir::new().unwrap();
    let (scene, nodes) = import_fixture(dir.path());

    // --- real node names, from the document, not the filename ---
    assert!(
        find_by_name(&nodes, "Root").is_some(),
        "the glTF's own node names must survive: {:?}",
        nodes.iter().map(|n| n.name()).collect::<Vec<_>>()
    );
    let body = find_by_name(&nodes, "Body").unwrap_or_else(|| {
        panic!(
            "`Body-col` must import as `Body` (suffix stripped); got {:?}",
            nodes.iter().map(|n| n.name()).collect::<Vec<_>>()
        )
    });
    assert_eq!(
        body.class_name(),
        "MeshInstance3D",
        "`-col` keeps the visual mesh"
    );
    assert!(
        find_by_name(&nodes, "character_Mesh").is_none(),
        "the fabricated `<stem>_Mesh` node must be gone"
    );

    // --- the -col suffix grew a StaticBody3D + CollisionShape3D ---
    let body_children = children_of(&nodes, body.id());
    let static_body = body_children
        .iter()
        .find(|n| n.class_name() == "StaticBody3D")
        .unwrap_or_else(|| {
            panic!(
                "`-col` must add a StaticBody3D child; `Body` has {:?}",
                body_children
                    .iter()
                    .map(|n| n.class_name())
                    .collect::<Vec<_>>()
            )
        });
    let shapes = children_of(&nodes, static_body.id());
    let shape = shapes
        .iter()
        .find(|n| n.class_name() == "CollisionShape3D")
        .expect("the generated StaticBody3D must carry a CollisionShape3D");
    // The shape is a real resource reference, not an empty placeholder.
    assert!(
        matches!(shape.get_property("shape"), Variant::String(ref s) if s.starts_with("SubResource:")),
        "the CollisionShape3D must point at a generated shape sub-resource, got {:?}",
        shape.get_property("shape")
    );

    // --- the AnimationPlayer holds the glTF's real clip name ---
    let player_node = nodes
        .iter()
        .find(|n| n.class_name() == "AnimationPlayer")
        .expect("an animated glTF must produce an AnimationPlayer");
    let props: HashMap<String, Variant> = player_node
        .properties()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let player =
        build_animation_player(&scene, &props).expect("the player must resolve its library");
    let clips: Vec<&String> = player.animations.keys().collect();
    assert!(
        player.animations.contains_key("Walk"),
        "the clip must be the glTF's `Walk`, got {clips:?}"
    );
    assert!(
        !player.animations.contains_key("default"),
        "`default` was the fabricated name; it must never appear: {clips:?}"
    );

    // --- mesh count is the document's real primitive count ---
    let registry = default_scene_importer_registry();
    let imported = registry
        .import_scene(&dir.path().join("character.glb"), None)
        .unwrap();
    assert_eq!(
        imported.mesh_count, 1,
        "one glTF primitive means one imported mesh"
    );
    assert_eq!(imported.animations.len(), 1);
    assert_eq!(imported.animations[0].name, "Walk");
    assert!(
        !imported.animations[0].tracks.is_empty(),
        "the clip must carry the sampler's real keyframes"
    );
}

/// The other half of the acceptance: bytes that are not glTF must fail, where
/// the old importer returned a scene with one mesh and a `"default"` animation
/// for any file over 100 bytes.
#[test]
fn non_gltf_bytes_fail_instead_of_fabricating_a_scene() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("model.gltf");
    std::fs::write(&path, "x".repeat(200)).unwrap();

    let registry = default_scene_importer_registry();
    let result = registry.import_scene(&path, None);
    assert!(
        result.is_err(),
        "200 bytes of non-glTF must not import: {:?}",
        result.map(|s| s.root.name)
    );
}

/// Godot's `_teststr`/`_fixstr` conventions: the longer suffixes must win over
/// their prefixes, and Blender's trailing `.001` (written `_001`) must not hide
/// the marker.
#[test]
fn node_name_suffixes_match_godot() {
    assert_eq!(
        split_name_suffix("Body-col"),
        ("Body".to_string(), NodeNameSuffix::Collision)
    );
    assert_eq!(
        split_name_suffix("Ground-colonly"),
        ("Ground".to_string(), NodeNameSuffix::CollisionOnly)
    );
    assert_eq!(
        split_name_suffix("Crate-convcol"),
        ("Crate".to_string(), NodeNameSuffix::ConvexCollision)
    );
    assert_eq!(
        split_name_suffix("Crate-convcolonly"),
        ("Crate".to_string(), NodeNameSuffix::ConvexCollisionOnly)
    );
    assert_eq!(
        split_name_suffix("Helper-noimp"),
        ("Helper".to_string(), NodeNameSuffix::NoImport)
    );
    // Blender's duplicate marker sits after the suffix.
    assert_eq!(
        split_name_suffix("Body-col_001"),
        ("Body_001".to_string(), NodeNameSuffix::Collision)
    );
    assert_eq!(
        split_name_suffix("Torso"),
        ("Torso".to_string(), NodeNameSuffix::None)
    );
}

/// A `-noimp` node is dropped, and an animation channel aimed at it goes with
/// it rather than retargeting onto whatever node happens to share its index.
#[test]
fn noimp_node_and_its_animation_channel_are_dropped() {
    let dir = tempfile::TempDir::new().unwrap();
    let source = dir.path().join("helper.glb");
    let bytes = {
        let mut bin = Vec::new();
        for t in [0.0f32, 1.0] {
            bin.extend_from_slice(&t.to_le_bytes());
        }
        for p in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0]] {
            for v in p {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        let json = r#"{
          "asset": {"version": "2.0"},
          "scene": 0,
          "scenes": [{"nodes": [0, 1]}],
          "nodes": [{"name": "Keep"}, {"name": "Gizmo-noimp"}],
          "animations": [{
            "name": "Wiggle",
            "samplers": [{"input": 0, "output": 1, "interpolation": "LINEAR"}],
            "channels": [{"sampler": 0, "target": {"node": 1, "path": "translation"}}]
          }],
          "buffers": [{"byteLength": 32}],
          "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": 8},
            {"buffer": 0, "byteOffset": 8, "byteLength": 24}
          ],
          "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 2, "type": "SCALAR"},
            {"bufferView": 1, "componentType": 5126, "count": 2, "type": "VEC3"}
          ]
        }"#;
        glb(json, &bin)
    };
    std::fs::write(&source, bytes).unwrap();

    let registry = default_scene_importer_registry();
    let scene = registry.import_scene(&source, None).unwrap();

    let names: Vec<&str> = scene
        .root
        .children
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert!(names.contains(&"Keep"), "got {names:?}");
    assert!(
        !names.iter().any(|n| n.starts_with("Gizmo")),
        "a `-noimp` node must be dropped, got {names:?}"
    );
    assert_eq!(scene.animations.len(), 1);
    assert!(
        scene.animations[0].tracks.is_empty(),
        "the channel targeting the dropped node has nothing left to drive"
    );
}
