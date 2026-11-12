//! pat-mgixk.53 — a `MeshInstance3D` whose `material_override` is an
//! `ExtResource` `.tres` renders the material the artist authored.
//!
//! Godot ground truth: `scene/resources/material.cpp`. An `ExtResource`
//! material loads exactly like a `SubResource` one — there is no second,
//! lesser path for a material that happens to live in its own file.
//!
//! # What was wrong
//!
//! ⚠️ The material did not merely lose its textures; it never reached the
//! renderer at all. `packed_scene` stores a reference property as the RAW
//! STRING the `.tscn` held — `ExtResource("1_abc")` — so
//! `render_server_3d::resolve_material` handed that literal to
//! `material_from_path`, whose filename test for `red`/`green`/`blue` matched
//! nothing and answered the default white. And because that check `return`ed
//! immediately, the `__material__*` bag one step below could never be consulted
//! for an overridden node even once it existed.
//!
//! So this bead needed two things, and either alone is insufficient: the
//! flattening (`packed_scene`) AND the precedence (`resolve_material` must
//! consult the bag before the filename fallback).
//!
//! # The fixture that makes the heuristic's death provable
//!
//! 🔑 `red_but_blue.tres` is named for a colour it is not. The old code keyed
//! on substrings of the reference string, so a file whose NAME says red and
//! whose `albedo_color` says blue is the one fixture that cannot pass by
//! accident: any implementation still guessing from the name renders it red (or
//! white), and only one that reads the file renders it blue.

use gdcore::math::Color;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::render_server_3d::{material3d_for_node, MaterialTextureCache};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// An authored material whose albedo disagrees with its own file name.
const RED_BUT_BLUE_TRES: &str = r#"[gd_resource type="StandardMaterial3D" format=3]

[resource]
albedo_color = Color(0, 0, 1, 1)
roughness = 0.3
"#;

/// A second material whose name says nothing about its colour, so it also
/// proves the loader is reading `albedo_color` rather than matching a word.
const ROCK_TRES: &str = r#"[gd_resource type="StandardMaterial3D" format=3]

[resource]
albedo_color = Color(0, 1, 0, 1)
roughness = 0.3
"#;

fn scene_with_override(material_file: &str) -> String {
    format!(
        r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Material" path="res://{material_file}" id="1_mat"]

[node name="Root" type="Node3D"]

[node name="Mesh" type="MeshInstance3D" parent="."]
material_override = ExtResource("1_mat")
"#
    )
}

/// Loads `tscn` against a `res://` root holding the two `.tres` files.
///
/// ⚠️ `set_path_roots` is a PROCESS-GLOBAL, so both cases in this test share
/// one root directory and both files are written up front rather than one per
/// case. Re-pointing the root mid-test would race the other case.
fn load(dir: &std::path::Path, tscn: &str) -> (SceneTree, NodeId) {
    gdcore::file_access::set_path_roots(Some(dir.to_path_buf()), None);
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let mesh = tree
        .find_child(scene_root, "Mesh", true)
        .expect("the MeshInstance3D exists");
    (tree, mesh)
}

#[track_caller]
fn assert_close(got: Color, want: Color, what: &str) {
    let d = (got.r - want.r).abs() + (got.g - want.g).abs() + (got.b - want.b).abs();
    assert!(
        d < 0.02,
        "{what}: expected about ({}, {}, {}), got ({}, {}, {})",
        want.r,
        want.g,
        want.b,
        got.r,
        got.g,
        got.b
    );
}

#[test]
fn ext_resource_material_renders_authored_albedo() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("rock.tres"), ROCK_TRES).expect("write rock");
    std::fs::write(dir.path().join("red_but_blue.tres"), RED_BUT_BLUE_TRES)
        .expect("write red_but_blue");

    // -- (a) the authored material reaches the node ------------------------
    let (tree, mesh) = load(dir.path(), &scene_with_override("rock.tres"));

    // The flattened bag is the node state a renderer reads. Before this bead
    // the node carried no `__material__*` at all for an ExtResource.
    assert_eq!(
        tree.get_node(mesh)
            .expect("mesh node")
            .get_property("__material__class"),
        Variant::String("StandardMaterial3D".to_string()),
        "the .tres's own class must be flattened onto the node; absent means \
         the ExtResource was never loaded"
    );

    // -- (b) and the RENDER path resolves it, not the filename heuristic ---
    let mut cache = MaterialTextureCache::default();
    let mat = material3d_for_node(&tree, mesh, &mut cache)
        .expect("the flattened bag must resolve to a material");
    assert_close(
        mat.albedo,
        Color::new(0.0, 1.0, 0.0, 1.0),
        "rock.tres albedo",
    );
    assert!(
        (mat.roughness - 0.3).abs() < 0.02,
        "roughness must come from the file too, not just albedo; got {}",
        mat.roughness
    );

    // -- (c) 🔑 the discriminator: a file named for the wrong colour --------
    //
    // Any implementation still guessing from the reference string renders this
    // red or white. Only one that opens the file renders it blue.
    let (tree, mesh) = load(dir.path(), &scene_with_override("red_but_blue.tres"));
    let mut cache = MaterialTextureCache::default();
    let mat = material3d_for_node(&tree, mesh, &mut cache)
        .expect("the flattened bag must resolve to a material");
    assert_close(
        mat.albedo,
        Color::new(0.0, 0.0, 1.0, 1.0),
        "red_but_blue.tres must render BLUE — its authored albedo — not the \
         colour its file name suggests",
    );
}
