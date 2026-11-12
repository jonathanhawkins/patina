//! pat-dy5: Instance inheritance for scenes using ext_resource.
//!
//! Validates that PackedScene.instance_with_subscenes() correctly resolves
//! ext_resource references to sub-scenes and instantiates their nodes into
//! the parent scene's tree.

use gdscene::packed_scene::PackedScene;

#[test]
fn instance_with_subscenes_includes_sub_scene_nodes() {
    let parent_tscn = r#"[gd_scene format=3]

[node name="ParentRoot" type="Node2D"]

[node name="Sprite" type="Sprite2D" parent="."]
"#;

    let child_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://parent.tscn" id="1"]

[node name="ChildRoot" type="Node2D"]

[node name="ParentInstance" parent="." instance=ExtResource("1")]
"#;

    let parent_packed = PackedScene::from_tscn(parent_tscn).unwrap();
    let child_packed = PackedScene::from_tscn(child_tscn).unwrap();

    let nodes = child_packed.instance_with_subscenes(&|path: &str| -> Option<PackedScene> {
        if path == "res://parent.tscn" {
            Some(parent_packed.clone())
        } else {
            None
        }
    });

    assert!(
        nodes.is_ok(),
        "instance_with_subscenes must succeed, got {:?}",
        nodes.err()
    );
    let nodes = nodes.unwrap();

    let names: Vec<&str> = nodes.iter().map(|n| n.name()).collect();
    assert!(
        names.contains(&"Sprite"),
        "instanced tree must include 'Sprite' from parent scene, got {:?}",
        names
    );
}

#[test]
fn instance_with_subscenes_renames_sub_root() {
    let parent_tscn = r#"[gd_scene format=3]

[node name="ParentRoot" type="Node2D"]

[node name="Child" type="Node" parent="."]
"#;

    let child_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://parent.tscn" id="1"]

[node name="Main" type="Node2D"]

[node name="MyInstance" parent="." instance=ExtResource("1")]
"#;

    let parent_packed = PackedScene::from_tscn(parent_tscn).unwrap();
    let child_packed = PackedScene::from_tscn(child_tscn).unwrap();

    let nodes = child_packed
        .instance_with_subscenes(&|path: &str| -> Option<PackedScene> {
            if path == "res://parent.tscn" {
                Some(parent_packed.clone())
            } else {
                None
            }
        })
        .unwrap();

    let names: Vec<&str> = nodes.iter().map(|n| n.name()).collect();
    // Sub-scene root should be renamed to "MyInstance" (from the parent scene template)
    assert!(
        names.contains(&"MyInstance"),
        "sub-scene root should be renamed to 'MyInstance', got {:?}",
        names
    );
    // Original parent root name should NOT appear
    assert!(
        !names.contains(&"ParentRoot"),
        "original parent root name should be replaced, got {:?}",
        names
    );
}

#[test]
fn instance_with_subscenes_applies_property_overrides() {
    let parent_tscn = r#"[gd_scene format=3]

[node name="ParentRoot" type="Node2D"]
position = Vector2(10, 20)
"#;

    let child_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://parent.tscn" id="1"]

[node name="Main" type="Node2D"]

[node name="Inst" parent="." instance=ExtResource("1")]
position = Vector2(99, 88)
"#;

    let parent_packed = PackedScene::from_tscn(parent_tscn).unwrap();
    let child_packed = PackedScene::from_tscn(child_tscn).unwrap();

    let nodes = child_packed
        .instance_with_subscenes(&|path: &str| -> Option<PackedScene> {
            if path == "res://parent.tscn" {
                Some(parent_packed.clone())
            } else {
                None
            }
        })
        .unwrap();

    // Find the instanced node
    let inst = nodes.iter().find(|n| n.name() == "Inst").unwrap();
    let pos = inst.get_property("position");

    // The child scene overrides position to (99, 88)
    match pos {
        gdvariant::Variant::Vector2(v) => {
            assert!(
                (v.x - 99.0).abs() < 0.001 && (v.y - 88.0).abs() < 0.001,
                "position should be overridden to (99, 88), got ({}, {})",
                v.x,
                v.y
            );
        }
        other => panic!("position should be Vector2, got {:?}", other),
    }
}

#[test]
fn instance_with_subscenes_unresolved_returns_error_or_skips() {
    let child_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://missing.tscn" id="1"]

[node name="Main" type="Node2D"]

[node name="Missing" parent="." instance=ExtResource("1")]
"#;

    let child_packed = PackedScene::from_tscn(child_tscn).unwrap();

    // When the resolver returns None, the node should be skipped or
    // created as a placeholder.
    let result = child_packed.instance_with_subscenes(&|_path: &str| -> Option<PackedScene> {
        None // can't resolve
    });

    // Either succeeds (skipping the unresolved instance) or fails gracefully
    match result {
        Ok(nodes) => {
            let names: Vec<&str> = nodes.iter().map(|n| n.name()).collect();
            assert!(
                names.contains(&"Main"),
                "root node should still be present, got {:?}",
                names
            );
        }
        Err(_) => {
            // Acceptable — failing on unresolved sub-scene is valid behavior
        }
    }
}

#[test]
fn instance_with_subscenes_nested_deep() {
    let grandparent_tscn = r#"[gd_scene format=3]

[node name="GrandparentRoot" type="Node"]

[node name="Leaf" type="Node" parent="."]
"#;

    let parent_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://grandparent.tscn" id="1"]

[node name="ParentRoot" type="Node"]

[node name="GPInst" parent="." instance=ExtResource("1")]
"#;

    let child_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://parent.tscn" id="1"]

[node name="ChildRoot" type="Node"]

[node name="PInst" parent="." instance=ExtResource("1")]
"#;

    let gp = PackedScene::from_tscn(grandparent_tscn).unwrap();
    let parent = PackedScene::from_tscn(parent_tscn).unwrap();
    let child = PackedScene::from_tscn(child_tscn).unwrap();

    let nodes = child
        .instance_with_subscenes(&|path: &str| -> Option<PackedScene> {
            match path {
                "res://grandparent.tscn" => Some(gp.clone()),
                "res://parent.tscn" => Some(parent.clone()),
                _ => None,
            }
        })
        .unwrap();

    let names: Vec<&str> = nodes.iter().map(|n| n.name()).collect();

    assert!(names.contains(&"ChildRoot"), "root: {:?}", names);
    assert!(names.contains(&"PInst"), "parent instance: {:?}", names);
    // The grandparent's "Leaf" should be present through nested instancing
    assert!(
        names.contains(&"Leaf"),
        "deeply nested Leaf from grandparent should be present: {:?}",
        names
    );
}
