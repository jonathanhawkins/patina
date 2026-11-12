//! pat-0q2kx.155 — glTF animations must reach the scene as playable clips.
//!
//! `gdresource::gltf_import::parse_animations` built `GltfAnimation` data and
//! **nothing consumed it**: its only callers were inside `#[cfg(test)]`, so an
//! imported `.glb` produced no `Animation` at all and the sampler keys were
//! dropped on the floor.
//!
//! Two halves had to be joined, and they cannot live in one crate: `Animation`
//! is a `gdscene` type and `gdscene` depends on `gdresource`, so the importer
//! publishes the parsed clips on the document and `gdscene` converts them.
//! These tests drive the real `import_gltf_bytes` → `animations_from_gltf_resource`
//! → `process_animations` pipeline, not the parser in isolation.

use gdscene::animation::{animations_from_gltf_resource, AnimationPlayer, TrackType};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// Wraps a glTF JSON document and its binary blob in a real `.glb` container,
/// so the tests exercise the same entry point a shipped asset would.
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

/// `Root -> Cube`, where `Cube` slides +10 on X and yaws 90° about Z over one
/// second, and also carries a mesh so the mesh path can be checked alongside.
///
/// Buffer layout (100 B): 2 times (8) | 2 VEC3 translations (24) |
/// 2 VEC4 rotations (32) | 3 VEC3 positions (36).
fn animated_cube_glb() -> Vec<u8> {
    let mut bin = Vec::new();
    for t in [0.0f32, 1.0] {
        bin.extend_from_slice(&t.to_le_bytes());
    }
    for p in [[0.0f32, 0.0, 0.0], [10.0, 0.0, 0.0]] {
        for v in p {
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    // Identity → 90° about +Z, as xyzw quaternions.
    let half = std::f32::consts::FRAC_1_SQRT_2;
    for q in [[0.0f32, 0.0, 0.0, 1.0], [0.0, 0.0, half, half]] {
        for v in q {
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    for p in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        for v in p {
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    assert_eq!(bin.len(), 100, "fixture buffer layout changed");

    let json = r#"{
      "asset": {"version": "2.0"},
      "scene": 0,
      "scenes": [{"nodes": [0]}],
      "nodes": [
        {"name": "Root", "children": [1]},
        {"name": "Cube", "mesh": 0}
      ],
      "meshes": [{"name": "CubeMesh", "primitives": [{"attributes": {"POSITION": 3}}]}],
      "animations": [{
        "name": "Slide",
        "samplers": [
          {"input": 0, "output": 1, "interpolation": "LINEAR"},
          {"input": 0, "output": 2, "interpolation": "LINEAR"}
        ],
        "channels": [
          {"sampler": 0, "target": {"node": 1, "path": "translation"}},
          {"sampler": 1, "target": {"node": 1, "path": "rotation"}}
        ]
      }],
      "buffers": [{"byteLength": 100}],
      "bufferViews": [
        {"buffer": 0, "byteOffset": 0,  "byteLength": 8},
        {"buffer": 0, "byteOffset": 8,  "byteLength": 24},
        {"buffer": 0, "byteOffset": 32, "byteLength": 32},
        {"buffer": 0, "byteOffset": 64, "byteLength": 36}
      ],
      "accessors": [
        {"bufferView": 0, "componentType": 5126, "count": 2, "type": "SCALAR"},
        {"bufferView": 1, "componentType": 5126, "count": 2, "type": "VEC3"},
        {"bufferView": 2, "componentType": 5126, "count": 2, "type": "VEC4"},
        {"bufferView": 3, "componentType": 5126, "count": 3, "type": "VEC3"}
      ]
    }"#;
    glb(json, &bin)
}

fn import() -> std::sync::Arc<gdresource::Resource> {
    gdresource::import_gltf_bytes(&animated_cube_glb(), std::path::Path::new("res://cube.glb"))
        .expect("the fixture must import")
}

// ---------------------------------------------------------------------------
// The bead's acceptance test
// ---------------------------------------------------------------------------

/// **The acceptance test.** Importing the `.glb` yields a clip an
/// `AnimationPlayer` can list and play, and playing it moves the target node's
/// transform to the sampler values — checked at t=0 and at the midpoint.
#[test]
fn gltf_import_animation_clip_plays() {
    let doc = import();
    let clips = animations_from_gltf_resource(&doc);
    assert_eq!(
        clips.len(),
        1,
        "the .glb's one animation must become one clip"
    );
    let clip = clips[0].clone();
    assert_eq!(clip.name, "Slide");
    assert!(
        (clip.length - 1.0).abs() < 1e-6,
        "clip length is the max key time, got {}",
        clip.length
    );

    // Two channels → two typed tracks, both targeting the imported node by name.
    assert_eq!(clip.tracks.len(), 2, "translation + rotation tracks");
    let types: Vec<TrackType> = clip.tracks.iter().map(|t| t.track_type()).collect();
    assert!(
        types.contains(&TrackType::Position3D) && types.contains(&TrackType::Rotation3D),
        "glTF translation/rotation must map to Position3D/Rotation3D, got {types:?}"
    );
    for t in &clip.tracks {
        assert_eq!(
            t.node_path, "Cube",
            "tracks must target the imported node's name"
        );
    }

    // -- t = 0: the first sampler key ---------------------------------------
    let pos_idx = clip
        .tracks
        .iter()
        .position(|t| t.track_type() == TrackType::Position3D)
        .expect("a Position3D track");
    match clip.sample_track(pos_idx, 0.0) {
        Some(Variant::Vector3(v)) => assert!(
            v.x.abs() < 1e-4,
            "at t=0 the sampler value is x=0, got {v:?}"
        ),
        other => panic!("Position3D at t=0 should sample a Vector3, got {other:?}"),
    }

    // -- play it through the real pipeline ----------------------------------
    // `Root -> Cube` under the player, so the track path "Cube" resolves against
    // the player's default root_node ".".
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let player_node = tree
        .add_child(root, Node::new("AnimationPlayer", "AnimationPlayer"))
        .unwrap();
    let cube = tree
        .add_child(player_node, Node::new("Cube", "Node3D"))
        .unwrap();

    let mut player = AnimationPlayer::new();
    player.add_animation(clip);
    player.play("Slide");
    tree.attach_animation_player(player_node, player).unwrap();

    // Advance to the midpoint: x lerps 0 → 10, so it must land near 5.
    tree.process_animations(0.5);

    match tree.get_node(cube).unwrap().get_property("position") {
        Variant::Vector3(v) => assert!(
            (v.x - 5.0).abs() < 0.01,
            "playing the imported clip must move the node to the sampled midpoint \
             x=5, got {v:?}"
        ),
        v => panic!(
            "the imported clip did not drive the node's position (still unwired?), got {v:?}"
        ),
    }

    // The rotation track must drive the node too — halfway to 90° about +Z.
    // `quaternion`, not `rotation`: Godot's `rotation` is Euler Vector3, and
    // `implicit_property_path` (animation_tscn.rs) names this one `quaternion`.
    match tree.get_node(cube).unwrap().get_property("quaternion") {
        Variant::Quaternion(q) => assert!(
            q.z > 0.05 && q.w > 0.05,
            "rotation must slerp toward 90° about +Z, got {q:?}"
        ),
        v => panic!("the rotation track did not reach the node, got {v:?}"),
    }
}

/// The importer must *publish* the animation data. This is the regression guard
/// for the original defect: `parse_animations` ran but its output never left the
/// crate, so the document carried no `animations` at all.
#[test]
fn import_publishes_animation_data_on_the_document() {
    let doc = import();
    assert_eq!(
        doc.get_property("animation_count"),
        Some(&Variant::Int(1)),
        "the imported document must report its animation count"
    );
    let Some(Variant::Array(clips)) = doc.get_property("animations") else {
        panic!("the imported document must carry an `animations` array");
    };
    let Some(Variant::Dictionary(clip)) = clips.to_vec().first().cloned() else {
        panic!("the clip must be a dictionary");
    };
    assert_eq!(clip.get("name"), Some(Variant::String("Slide".into())));
    let Some(Variant::Array(channels)) = clip.get("channels") else {
        panic!("the clip must carry its channels");
    };
    assert_eq!(channels.len(), 2, "translation + rotation");
}

/// Publishing animations must not disturb mesh import.
///
/// Clips are stored as document **properties** rather than subresources for
/// exactly this reason: `mesh3d_from_gltf_resource` sorts `subresources` by key
/// and treats `subs[0]` as the primary surface, aborting when it has no
/// `vertices`. An `animation_*` subresource key sorts before `mesh_0_prim_0`,
/// so storing clips there would have silently broken mesh loading for every
/// animated `.glb` — the exact files this bead is about.
#[test]
fn animations_do_not_shadow_the_mesh_subresource() {
    let doc = import();
    let mesh = gdscene::render_server_3d::mesh3d_from_gltf_resource(&doc)
        .expect("an animated .glb must still yield its mesh");
    assert_eq!(
        mesh.vertices.len(),
        3,
        "the fixture's triangle must survive alongside the animation data"
    );
}

/// A `.glb` with no animations yields no clips, and must not fabricate an empty
/// one — an empty clip list is the honest result, and `animation_count` stays
/// absent rather than reporting 0.
#[test]
fn glb_without_animations_yields_no_clips() {
    let json = r#"{
      "asset": {"version": "2.0"},
      "scene": 0,
      "scenes": [{"nodes": [0]}],
      "nodes": [{"name": "Bare"}]
    }"#;
    let doc =
        gdresource::import_gltf_bytes(&glb(json, &[]), std::path::Path::new("res://bare.glb"))
            .expect("import");
    assert!(doc.get_property("animations").is_none());
    assert!(animations_from_gltf_resource(&doc).is_empty());
}
