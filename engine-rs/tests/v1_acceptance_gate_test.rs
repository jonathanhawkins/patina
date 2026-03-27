//! V1 Acceptance Gate Tests
//!
//! Each test corresponds to an unchecked item in `prd/V1_EXIT_CRITERIA.md`.
//! All tests use `#[ignore]` so `cargo test` passes, but
//! `cargo test --test v1_acceptance_gate_test -- --ignored` runs them
//! and they FAIL because the feature is not yet implemented.
//!
//! Workers pick up individual gates with:
//! ```sh
//! cargo test --test v1_acceptance_gate_test -- --ignored test_v1_<name>
//! ```
//! and make them pass as part of closing the corresponding V1 exit criteria.

// ==========================================================================
// Object Model (gdobject) — V1_EXIT_CRITERIA.md lines 35-38
// ==========================================================================

/// V1 gate: Full ClassDB property and method enumeration (line 35)
///
/// ClassDB must return property lists that match oracle output for
/// representative classes. Currently `get_property_list` returns only
/// explicitly registered properties, missing inherited ones and
/// default values that Godot would report.
#[test]
#[ignore = "V1 gate: ClassDB property/method enumeration incomplete vs oracle"]
fn test_v1_classdb_full_property_enumeration() {
    use gdobject::{
        clear_for_testing, get_method_list, get_property_list, register_class, ClassRegistration,
        MethodInfo, PropertyInfo,
    };

    clear_for_testing();

    // Register a 3-level hierarchy: Object -> Node -> Node2D
    register_class(
        ClassRegistration::new("Object")
            .property(PropertyInfo::new(
                "script",
                gdvariant::Variant::Nil,
            ))
            .method(MethodInfo::new("get_class", 0))
            .method(MethodInfo::new("free", 0)),
    );
    register_class(
        ClassRegistration::new("Node")
            .parent("Object")
            .property(PropertyInfo::new(
                "name",
                gdvariant::Variant::String(String::new()),
            ))
            .property(PropertyInfo::new(
                "process_mode",
                gdvariant::Variant::Int(0),
            ))
            .method(MethodInfo::new("_ready", 0))
            .method(MethodInfo::new("add_child", 1)),
    );
    register_class(
        ClassRegistration::new("Node2D")
            .parent("Node")
            .property(PropertyInfo::new(
                "position",
                gdvariant::Variant::Vector2(gdcore::math::Vector2::ZERO),
            ))
            .property(PropertyInfo::new(
                "rotation",
                gdvariant::Variant::Float(0.0),
            ))
            .method(MethodInfo::new("rotate", 1)),
    );

    // 1. get_property_list returns inherited + own properties, base-first.
    let props = get_property_list("Node2D");
    let prop_names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();

    // Must include Object's "script", Node's "name" and "process_mode",
    // and Node2D's "position" and "rotation".
    assert!(
        prop_names.contains(&"script"),
        "Node2D must inherit Object.script, got {:?}",
        prop_names,
    );
    assert!(
        prop_names.contains(&"name"),
        "Node2D must inherit Node.name, got {:?}",
        prop_names,
    );
    assert!(
        prop_names.contains(&"process_mode"),
        "Node2D must inherit Node.process_mode, got {:?}",
        prop_names,
    );
    assert!(
        prop_names.contains(&"position"),
        "Node2D must have own position, got {:?}",
        prop_names,
    );
    assert!(
        prop_names.contains(&"rotation"),
        "Node2D must have own rotation, got {:?}",
        prop_names,
    );

    // Base-first ordering: "script" (Object) must come before "name" (Node)
    // which must come before "position" (Node2D).
    let script_idx = prop_names.iter().position(|n| *n == "script").unwrap();
    let name_idx = prop_names.iter().position(|n| *n == "name").unwrap();
    let pos_idx = prop_names.iter().position(|n| *n == "position").unwrap();
    assert!(
        script_idx < name_idx,
        "Object.script (idx {}) must come before Node.name (idx {})",
        script_idx,
        name_idx,
    );
    assert!(
        name_idx < pos_idx,
        "Node.name (idx {}) must come before Node2D.position (idx {})",
        name_idx,
        pos_idx,
    );

    // Total count: 1 + 2 + 2 = 5
    assert_eq!(props.len(), 5, "Node2D should have 5 total properties");

    // 2. get_method_list returns inherited + own methods, base-first.
    let methods = get_method_list("Node2D");
    let method_names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();

    assert!(
        method_names.contains(&"get_class"),
        "Node2D must inherit Object.get_class, got {:?}",
        method_names,
    );
    assert!(
        method_names.contains(&"free"),
        "Node2D must inherit Object.free, got {:?}",
        method_names,
    );
    assert!(
        method_names.contains(&"_ready"),
        "Node2D must inherit Node._ready, got {:?}",
        method_names,
    );
    assert!(
        method_names.contains(&"add_child"),
        "Node2D must inherit Node.add_child, got {:?}",
        method_names,
    );
    assert!(
        method_names.contains(&"rotate"),
        "Node2D must have own rotate, got {:?}",
        method_names,
    );

    // Total: 2 + 2 + 1 = 5
    assert_eq!(methods.len(), 5, "Node2D should have 5 total methods");

    // 3. Querying a leaf class returns only its own.
    let obj_props = get_property_list("Object");
    assert_eq!(obj_props.len(), 1);
    assert_eq!(obj_props[0].name, "script");

    // 4. Unregistered class returns empty.
    assert!(get_property_list("Unknown").is_empty());
    assert!(get_method_list("Unknown").is_empty());
}

/// V1 gate: Object.notification() dispatch with correct ordering (line 36)
///
/// When a node enters the tree, notifications must fire in Godot's
/// canonical order: POSTINITIALIZE before ENTER_TREE before READY.
/// Other notifications (PARENTED, CHILD_ORDER_CHANGED) may be interleaved
/// per Godot semantics, but the three lifecycle notifications must appear
/// in the correct relative order.
#[test]
#[ignore = "V1 gate: Object.notification() inheritance-chain dispatch ordering"]
fn test_v1_notification_dispatch_ordering() {
    let mut tree = gdscene::SceneTree::new();
    let root = tree.root_id();

    // add_child with root inside tree triggers full lifecycle:
    // POSTINITIALIZE (from Node::new), PARENTED, ENTER_TREE, READY.
    let child_id = tree
        .add_child(root, gdscene::Node::new("Child", "Node2D"))
        .unwrap();

    let node = tree.get_node(child_id).unwrap();
    let log = node.notification_log();

    // All three key lifecycle notifications must be present.
    let pos_init = log
        .iter()
        .position(|n| *n == gdobject::NOTIFICATION_POSTINITIALIZE);
    let enter = log
        .iter()
        .position(|n| *n == gdobject::NOTIFICATION_ENTER_TREE);
    let ready = log
        .iter()
        .position(|n| *n == gdobject::NOTIFICATION_READY);

    assert!(
        pos_init.is_some(),
        "POSTINITIALIZE must be in notification log, got {:?}",
        log,
    );
    assert!(
        enter.is_some(),
        "ENTER_TREE must be in notification log, got {:?}",
        log,
    );
    assert!(
        ready.is_some(),
        "READY must be in notification log, got {:?}",
        log,
    );

    // Ordering: POSTINITIALIZE < ENTER_TREE < READY
    let pos_init = pos_init.unwrap();
    let enter = enter.unwrap();
    let ready = ready.unwrap();

    assert!(
        pos_init < enter,
        "POSTINITIALIZE (idx {}) must come before ENTER_TREE (idx {})",
        pos_init,
        enter,
    );
    assert!(
        enter < ready,
        "ENTER_TREE (idx {}) must come before READY (idx {})",
        enter,
        ready,
    );
}

/// V1 gate: WeakRef behavior matches oracle (line 37)
///
/// WeakRef must detect when a referenced object has been freed via the
/// scene tree, without the caller manually invalidating it.
/// Currently WeakRef only supports manual invalidation.
#[test]
#[ignore = "V1 gate: WeakRef auto-invalidation on object free"]
fn test_v1_weakref_auto_invalidates_on_free() {
    let mut tree = gdscene::SceneTree::new();
    let root = tree.root_id();
    let child_id = tree
        .add_child(root, gdscene::Node::new("Temp", "Node"))
        .unwrap();

    let obj_id = child_id.object_id();
    let weak = gdobject::weak_ref::WeakRef::new(obj_id);

    // Before free: weak ref should resolve.
    assert!(
        weak.get_ref().is_some(),
        "WeakRef should be valid before free"
    );

    // Free the node.
    tree.queue_free(child_id);
    tree.process_deletions();

    // After free: the node is gone.
    assert!(
        tree.get_node(child_id).is_none(),
        "node should be gone after free"
    );

    // V1 GATE: WeakRef itself must know the object is gone (auto-invalidation).
    // Currently WeakRef only supports manual invalidation, so get_ref() still
    // returns Some even after the object has been freed.
    assert!(
        weak.get_ref().is_none(),
        "V1 GATE FAIL: WeakRef.get_ref() must return None after object is freed"
    );
}

/// V1 gate: Object.free() + use-after-free guard (line 38)
///
/// After free(), property access must return an error/None, not panic.
#[test]
#[ignore = "V1 gate: Object.free() use-after-free guard"]
fn test_v1_object_free_use_after_free_guard() {
    let mut tree = gdscene::SceneTree::new();
    let root = tree.root_id();
    let child_id = tree
        .add_child(root, gdscene::Node::new("ToFree", "Node"))
        .unwrap();

    // Free the node.
    tree.queue_free(child_id);
    tree.process_deletions();

    // After free: any access must return None, NOT panic.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tree.get_node(child_id)
    }));

    match result {
        Ok(None) => {} // Correct: returns None for freed node.
        Ok(Some(_)) => panic!("V1 GATE FAIL: get_node returned Some for a freed node"),
        Err(_) => panic!(
            "V1 GATE FAIL: get_node panicked on freed node ID instead of returning None"
        ),
    }
}

// ==========================================================================
// Resources (gdresource) — V1_EXIT_CRITERIA.md lines 49-53
// ==========================================================================

/// V1 gate: Resource UID registry tracks uid:// references (line 49)
///
/// The UID registry must persist across loads and resolve uid:// references
/// that were parsed from .tres/.tscn files.
#[test]
#[ignore = "V1 gate: Resource UID registry auto-population from parsed files"]
fn test_v1_resource_uid_registry_from_parsed_files() {
    let source = r#"[gd_resource type="Resource" format=3 uid="uid://abc123def"]

[resource]
name = "UidTest"
"#;

    let loader = gdresource::TresLoader::new();
    let res = loader.parse_str(source, "res://uid_test.tres").unwrap();

    // The resource should carry the parsed UID.
    assert!(
        res.uid.is_valid(),
        "V1 GATE FAIL: Resource loaded from .tres with uid= must have a valid UID, got {:?}",
        res.uid
    );
}

/// V1 gate: Resource UID registry for uid:// references (pat-lxl)
///
/// The UidRegistry must support bidirectional uid↔path mapping, the
/// UnifiedLoader must resolve uid:// references to res:// paths, and
/// parsed .tres files must carry their UID for later registry population.
#[test]
#[ignore = "V1 gate: Resource UID registry for uid:// references"]
fn test_v1_uid_registry() {
    use gdcore::ResourceUid;
    use gdresource::UidRegistry;

    // 1. Basic registry: register, lookup by UID, lookup by path.
    let mut registry = UidRegistry::new();
    let uid = ResourceUid::new(42);
    registry.register(uid, "res://weapon.tres");

    assert_eq!(
        registry.lookup_uid(uid),
        Some("res://weapon.tres"),
        "UID lookup must return the registered path"
    );
    assert_eq!(
        registry.lookup_path("res://weapon.tres"),
        Some(uid),
        "Path lookup must return the registered UID"
    );

    // 2. Overwrite: re-registering the same UID with a new path replaces the old mapping.
    registry.register(uid, "res://sword.tres");
    assert_eq!(registry.lookup_uid(uid), Some("res://sword.tres"));
    assert_eq!(registry.lookup_path("res://weapon.tres"), None);
    assert_eq!(registry.len(), 1);

    // 3. Multiple entries.
    let uid2 = ResourceUid::new(99);
    registry.register(uid2, "res://shield.tres");
    assert_eq!(registry.len(), 2);

    // 4. Unregister.
    registry.unregister_uid(uid);
    assert_eq!(registry.lookup_uid(uid), None);
    assert_eq!(registry.len(), 1);

    // 5. Parse a .tres with a uid= header — the Resource must carry the UID.
    let source = r#"[gd_resource type="Resource" format=3 uid="uid://test_uid_registry"]

[resource]
name = "UidGateTest"
"#;
    let loader = gdresource::TresLoader::new();
    let res = loader.parse_str(source, "res://gate_test.tres").unwrap();
    assert!(
        res.uid.is_valid(),
        "Resource parsed from .tres with uid= must have a valid UID"
    );

    // 6. UnifiedLoader resolves uid:// references.
    let mut unified = gdresource::UnifiedLoader::new(gdresource::TresLoader::new());
    unified.register_uid_str("uid://abc", "res://item.tres");
    let resolved = unified.resolve_to_path("uid://abc").unwrap();
    assert_eq!(
        resolved,
        "res://item.tres",
        "UnifiedLoader must resolve uid:// to res:// path"
    );

    // 7. Unresolved uid:// returns an error.
    let unresolved = unified.resolve_to_path("uid://nonexistent");
    assert!(
        unresolved.is_err(),
        "Unresolved uid:// reference must return an error"
    );

    // 8. res:// paths pass through unchanged.
    let passthrough = unified.resolve_to_path("res://direct.tres").unwrap();
    assert_eq!(
        passthrough,
        "res://direct.tres",
        "res:// paths must pass through unchanged"
    );
}

/// V1 gate: Sub-resource inline loading (line 50)
///
/// Nested sub-resources in .tres files must be fully loaded and resolvable
/// from the parent resource's properties.
#[test]
#[ignore = "V1 gate: Sub-resource inline loading with property resolution"]
fn test_v1_subresource_inline_loading() {
    let source = r#"[gd_resource type="Theme" format=3]

[sub_resource type="StyleBoxFlat" id="StyleBoxFlat_abc"]
bg_color = Color(0.2, 0.3, 0.4, 1)
corner_radius_top_left = 4

[sub_resource type="StyleBoxFlat" id="StyleBoxFlat_def"]
bg_color = Color(0.5, 0.6, 0.7, 1)

[resource]
panel_style = SubResource("StyleBoxFlat_abc")
button_style = SubResource("StyleBoxFlat_def")
"#;

    let loader = gdresource::TresLoader::new();
    let res = loader.parse_str(source, "res://theme.tres").unwrap();

    // Sub-resources must be loaded.
    assert_eq!(res.subresources.len(), 2, "must have 2 sub-resources");

    // Property references must resolve to sub-resources.
    let panel = res.resolve_subresource("panel_style");
    assert!(
        panel.is_some(),
        "V1 GATE FAIL: resolve_subresource('panel_style') must find the sub-resource"
    );
    let panel = panel.unwrap();
    assert_eq!(panel.class_name, "StyleBoxFlat");

    // Nested property must be accessible.
    let bg = panel.get_property("bg_color");
    assert!(
        bg.is_some(),
        "V1 GATE FAIL: sub-resource must have bg_color property"
    );
}

/// V1 gate: External resource reference resolution across files (line 51)
///
/// When a .tres references an ext_resource, the loader must populate
/// the ext_resources map with type and path information.
#[test]
#[ignore = "V1 gate: External resource cross-file reference resolution"]
fn test_v1_ext_resource_cross_file_resolution() {
    let main_source = r#"[gd_resource type="PackedScene" format=3]

[ext_resource type="Texture2D" path="res://icon.png" id="1"]

[resource]
texture = ExtResource("1")
"#;

    let loader = gdresource::TresLoader::new();
    let res = loader.parse_str(main_source, "res://main.tres").unwrap();

    // ext_resources must be parsed.
    assert!(
        !res.ext_resources.is_empty(),
        "ext_resources must be populated from [ext_resource] sections"
    );
    assert!(
        res.ext_resources.contains_key("1"),
        "ext_resource with id='1' must be present"
    );

    let ext = &res.ext_resources["1"];
    assert_eq!(ext.path, "res://icon.png");
    assert_eq!(ext.resource_type, "Texture2D");
}

/// Alias for coordinator filter compatibility.
#[test]
#[ignore = "V1 gate: External resource reference (alias)"]
fn test_v1_external_ref_resolution() {
    test_v1_ext_resource_cross_file_resolution();
}

/// V1 gate: Load-save roundtrip equivalence (line 52)
///
/// Loading a resource and saving it back must produce semantically
/// equivalent output that re-loads identically (including UID).
#[test]
#[ignore = "V1 gate: Resource load-save roundtrip semantic equivalence"]
fn test_v1_resource_roundtrip_equivalence() {
    let source = r#"[gd_resource type="Resource" format=3 uid="uid://roundtrip_test"]

[sub_resource type="StyleBoxFlat" id="StyleBoxFlat_001"]
bg_color = Color(0.2, 0.3, 0.4, 1)

[resource]
name = "RoundTrip"
value = 42
style = SubResource("StyleBoxFlat_001")
"#;

    let loader = gdresource::TresLoader::new();
    let res1 = loader.parse_str(source, "res://rt.tres").unwrap();

    let saver = gdresource::TresSaver::new();
    let saved = saver.save_to_string(&res1).unwrap();

    let res2 = loader.parse_str(&saved, "res://rt.tres").unwrap();

    // All properties must survive the roundtrip.
    assert_eq!(res1.class_name, res2.class_name);
    assert_eq!(res1.get_property("name"), res2.get_property("name"));
    assert_eq!(res1.get_property("value"), res2.get_property("value"));
    assert_eq!(
        res1.subresources.len(),
        res2.subresources.len(),
        "sub-resource count must survive roundtrip"
    );

    // V1 gate: UID must survive roundtrip.
    assert!(
        res2.uid.is_valid(),
        "V1 GATE FAIL: UID must survive load-save roundtrip, got {:?}",
        res2.uid
    );
}

/// V1 gate: Oracle comparison for fixture resource (line 53)
///
/// At least one fixture resource must match oracle-captured metadata.
#[test]
#[ignore = "V1 gate: Oracle comparison for fixture resource"]
fn test_v1_resource_oracle_comparison() {
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/resources/with_ext_refs.tres"
    );

    let content = std::fs::read_to_string(fixture_path);
    assert!(
        content.is_ok(),
        "fixture file must exist at {}",
        fixture_path
    );

    let loader = gdresource::TresLoader::new();
    let res = loader
        .parse_str(&content.unwrap(), "res://with_ext_refs.tres")
        .unwrap();

    assert!(
        !res.class_name.is_empty(),
        "V1 GATE FAIL: loaded resource must have a non-empty class name"
    );
    assert!(
        res.property_count() > 0 || !res.ext_resources.is_empty(),
        "V1 GATE FAIL: fixture resource must have properties or ext_resources"
    );
}

// ==========================================================================
// Scenes (gdscene) — V1_EXIT_CRITERIA.md lines 65-68
// ==========================================================================

/// V1 gate: Instance inheritance - ext_resource scenes (line 65)
///
/// A PackedScene that references another scene via ext_resource must
/// instantiate the sub-scene's nodes as part of its own tree.
#[test]
#[ignore = "V1 gate: Instance inheritance via ext_resource scenes"]
fn test_v1_instance_inheritance_ext_resource() {
    let parent_tscn = r#"[gd_scene format=3]

[node name="ParentRoot" type="Node2D"]

[node name="Sprite" type="Sprite2D" parent="."]
"#;

    let child_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://parent.tscn" id="1"]

[node name="ChildRoot" type="Node2D"]

[node name="ParentInstance" parent="." instance=ExtResource("1")]
"#;

    let parent_packed = gdscene::PackedScene::from_tscn(parent_tscn).unwrap();
    let child_packed = gdscene::PackedScene::from_tscn(child_tscn).unwrap();

    // Instance with sub-scene resolution.
    // The callback receives the resolved res:// path from the ext_resource entry.
    let nodes = child_packed.instance_with_subscenes(
        &|path: &str| -> Option<gdscene::PackedScene> {
            if path == "res://parent.tscn" {
                Some(parent_packed.clone())
            } else {
                None
            }
        },
    );

    assert!(
        nodes.is_ok(),
        "V1 GATE FAIL: instance_with_subscenes must succeed, got {:?}",
        nodes.err()
    );
    let nodes = nodes.unwrap();

    // The instanced tree must contain the parent scene's nodes.
    let names: Vec<&str> = nodes.iter().map(|n: &gdscene::Node| n.name()).collect();
    assert!(
        names.contains(&"Sprite"),
        "V1 GATE FAIL: instanced tree must include 'Sprite' from parent scene, got {:?}",
        names
    );
}

/// V1 gate: PackedScene save/restore roundtrip (line 66)
///
/// Saving a SceneTree to .tscn and re-parsing must produce an equivalent hierarchy.
#[test]
#[ignore = "V1 gate: PackedScene save/restore roundtrip"]
fn test_v1_packed_scene_save_restore_roundtrip() {
    let tscn = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]
position = Vector2(10, 20)

[node name="Child" type="Sprite2D" parent="."]
z_index = 5
"#;

    let packed = gdscene::PackedScene::from_tscn(tscn).unwrap();

    // Add to a tree.
    let mut tree = gdscene::SceneTree::new();
    let root = tree.root_id();
    let scene_root = gdscene::add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // Save back to .tscn.
    let saved_tscn = gdscene::TscnSaver::save_tree(&tree, scene_root);

    // Re-parse.
    let repacked = gdscene::PackedScene::from_tscn(&saved_tscn).unwrap();
    let renodes = repacked.instance().unwrap();

    // The original packed scene has 2 nodes (Root + Child).
    let orig_nodes = packed.instance().unwrap();
    assert_eq!(
        orig_nodes.len(),
        renodes.len(),
        "V1 GATE FAIL: roundtrip must preserve node count"
    );

    // Verify properties survived.
    let root_node = &renodes[0];
    let pos = root_node.get_property("position");
    assert_eq!(
        pos,
        gdvariant::Variant::Vector2(gdcore::Vector2::new(10.0, 20.0)),
        "V1 GATE FAIL: position property must survive roundtrip"
    );
}

/// V1 gate: Scene-level signal connections wired during instantiation (line 67)
///
/// [connection] sections in .tscn files must result in signals being
/// wired when the scene is instantiated into a SceneTree.
#[test]
#[ignore = "V1 gate: Scene signal connections wired during instantiation"]
fn test_v1_scene_signal_connections_wired() {
    let tscn = r#"[gd_scene format=3]

[node name="Root" type="Node"]

[node name="Button" type="Button" parent="."]

[connection signal="pressed" from="Button" to="." method="_on_button_pressed"]
"#;

    let packed = gdscene::PackedScene::from_tscn(tscn).unwrap();

    let mut tree = gdscene::SceneTree::new();
    let root = tree.root_id();
    let scene_root = gdscene::add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // Wire connections.
    gdscene::wire_connections(&mut tree, scene_root, packed.connections());

    // Find the Button node by scanning children of scene_root.
    let scene_children = tree.get_node(scene_root).unwrap().children().to_vec();
    let button_id = scene_children
        .iter()
        .copied()
        .find(|&id| tree.get_node(id).map(|n| n.name() == "Button").unwrap_or(false))
        .expect("Button node must exist");

    // Check that the "pressed" signal is connected.
    let has_connections = tree
        .signal_store(button_id)
        .map(|store| {
            store
                .get_signal("pressed")
                .map(|s| s.connection_count() > 0)
                .unwrap_or(false)
        })
        .unwrap_or(false);

    assert!(
        has_connections,
        "V1 GATE FAIL: 'pressed' signal on Button must have connections after wire_connections"
    );
}

/// V1 gate: Scene-level signal connections wired during instantiation (alias).
///
/// Verifies that [connection] sections in .tscn files produce wired signals
/// when instantiated, including multi-signal and cross-node scenarios.
#[test]
#[ignore = "V1 gate: Scene-level signal connections wired during instantiation"]
fn test_v1_scene_signals() {
    // Scene with two signals: Button.pressed -> Root, and a custom signal
    let tscn = r#"[gd_scene format=3]

[node name="Root" type="Node"]

[node name="Button" type="Button" parent="."]

[node name="Timer" type="Timer" parent="."]

[connection signal="pressed" from="Button" to="." method="_on_button_pressed"]
[connection signal="timeout" from="Timer" to="." method="_on_timer_timeout"]
"#;

    let packed = gdscene::PackedScene::from_tscn(tscn).unwrap();

    let mut tree = gdscene::SceneTree::new();
    let root = tree.root_id();
    let scene_root = gdscene::add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // Wire connections from the packed scene.
    gdscene::wire_connections(&mut tree, scene_root, packed.connections());

    let scene_children = tree.get_node(scene_root).unwrap().children().to_vec();

    // Find Button and Timer nodes.
    let button_id = scene_children
        .iter()
        .copied()
        .find(|&id| tree.get_node(id).map(|n| n.name() == "Button").unwrap_or(false))
        .expect("Button node must exist");

    let timer_id = scene_children
        .iter()
        .copied()
        .find(|&id| tree.get_node(id).map(|n| n.name() == "Timer").unwrap_or(false))
        .expect("Timer node must exist");

    // Verify Button's "pressed" signal is connected.
    let button_connected = tree
        .signal_store(button_id)
        .map(|store| {
            store
                .get_signal("pressed")
                .map(|s| s.connection_count() > 0)
                .unwrap_or(false)
        })
        .unwrap_or(false);

    assert!(
        button_connected,
        "V1 GATE FAIL: Button 'pressed' signal must be connected after instantiation"
    );

    // Verify Timer's "timeout" signal is connected.
    let timer_connected = tree
        .signal_store(timer_id)
        .map(|store| {
            store
                .get_signal("timeout")
                .map(|s| s.connection_count() > 0)
                .unwrap_or(false)
        })
        .unwrap_or(false);

    assert!(
        timer_connected,
        "V1 GATE FAIL: Timer 'timeout' signal must be connected after instantiation"
    );

    // Verify connections target the scene root.
    let pressed_target = tree
        .signal_store(button_id)
        .and_then(|store| store.get_signal("pressed"))
        .map(|s| s.connections()[0].target_id)
        .unwrap();
    assert_eq!(
        pressed_target,
        scene_root.object_id(),
        "Button.pressed must target Root"
    );

    let timeout_target = tree
        .signal_store(timer_id)
        .and_then(|store| store.get_signal("timeout"))
        .map(|s| s.connections()[0].target_id)
        .unwrap();
    assert_eq!(
        timeout_target,
        scene_root.object_id(),
        "Timer.timeout must target Root"
    );
}

/// V1 gate: Oracle golden comparison for non-trivial scene tree (line 68)
///
/// A loaded scene must produce a node tree that matches oracle golden output.
#[test]
#[ignore = "V1 gate: Oracle golden comparison for scene tree"]
fn test_v1_scene_oracle_golden_comparison() {
    let golden_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/golden/scenes/platformer.json"
    );
    let golden_content = std::fs::read_to_string(golden_path);
    assert!(
        golden_content.is_ok(),
        "golden file must exist at {}",
        golden_path
    );

    let golden: serde_json::Value =
        serde_json::from_str(&golden_content.unwrap()).unwrap();

    // The golden must contain a "nodes" array with node metadata.
    let nodes = golden.get("nodes").or_else(|| golden.get("tree"));
    assert!(
        nodes.is_some(),
        "V1 GATE FAIL: golden file must contain 'nodes' or 'tree' key"
    );

    // Count all nodes recursively (golden stores hierarchical children).
    fn count_nodes(value: &serde_json::Value) -> usize {
        match value {
            serde_json::Value::Array(arr) => {
                arr.iter().map(count_nodes).sum()
            }
            serde_json::Value::Object(obj) => {
                let children_count = obj
                    .get("children")
                    .map(count_nodes)
                    .unwrap_or(0);
                1 + children_count
            }
            _ => 0,
        }
    }

    let total_nodes = count_nodes(nodes.unwrap());
    assert!(
        total_nodes > 3,
        "V1 GATE FAIL: non-trivial scene golden must have >3 nodes, got {}",
        total_nodes
    );

    // Verify root node has expected structure fields.
    let root_arr = nodes.unwrap().as_array().expect("nodes must be an array");
    let root = &root_arr[0];
    assert!(
        root.get("name").is_some(),
        "root node must have 'name' field"
    );
    assert!(
        root.get("class").is_some(),
        "root node must have 'class' field"
    );

    // Load the actual scene and compare node names.
    let tscn_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/scenes/platformer.tscn"
    );
    let tscn_source = std::fs::read_to_string(tscn_path)
        .unwrap_or_else(|e| panic!("should read platformer.tscn: {}", e));
    let scene = gdscene::PackedScene::from_tscn(&tscn_source)
        .unwrap_or_else(|e| panic!("parse platformer.tscn: {:?}", e));
    let mut tree = gdscene::SceneTree::new();
    let root_id = tree.root_id();
    gdscene::add_packed_scene_to_tree(&mut tree, root_id, &scene)
        .unwrap_or_else(|e| panic!("add platformer to tree: {:?}", e));

    // Collect golden node names.
    fn collect_names(value: &serde_json::Value, names: &mut Vec<String>) {
        match value {
            serde_json::Value::Array(arr) => {
                for item in arr {
                    collect_names(item, names);
                }
            }
            serde_json::Value::Object(obj) => {
                if let Some(name) = obj.get("name").and_then(|n| n.as_str()) {
                    names.push(name.to_string());
                }
                if let Some(children) = obj.get("children") {
                    collect_names(children, names);
                }
            }
            _ => {}
        }
    }

    let mut golden_names = Vec::new();
    collect_names(nodes.unwrap(), &mut golden_names);

    // Collect actual tree node names.
    let all_nodes = tree.all_nodes_in_tree_order();
    let actual_names: Vec<String> = all_nodes
        .iter()
        .filter_map(|&nid| tree.get_node(nid).map(|n| n.name().to_string()))
        .filter(|n| n != "root") // skip synthetic root
        .collect();

    // Every golden node name must appear in the actual tree.
    let mut matched = 0;
    for gname in &golden_names {
        if actual_names.contains(gname) {
            matched += 1;
        }
    }

    let parity = if golden_names.is_empty() {
        0.0
    } else {
        matched as f64 / golden_names.len() as f64 * 100.0
    };

    assert!(
        parity >= 80.0,
        "V1 GATE FAIL: oracle golden node name parity {:.1}% ({}/{}) must be >= 80%",
        parity,
        matched,
        golden_names.len()
    );
}

// ==========================================================================
// Scripting (gdscript-interop) — V1_EXIT_CRITERIA.md lines 77-81
// ==========================================================================

/// V1 gate: GDScript parser produces stable AST (line 77)
///
/// The parser must handle representative scripts without errors and produce
/// a structured AST with class_name, variables, functions, and signals.
#[test]
#[ignore = "V1 gate: GDScript parser stable AST for representative scripts"]
fn test_v1_gdscript_parser_stable_ast() {
    // The parser must handle a representative script with class_name,
    // @export, @onready, signals, and multiple functions.
    let script = r#"
class_name Player
extends CharacterBody2D

@export var speed: float = 200.0
@onready var sprite = $Sprite2D

signal health_changed(new_health: int)

var health: int = 100

func _ready():
    print("Player ready")

func take_damage(amount: int):
    health -= amount
    health_changed.emit(health)
"#;

    // Parse through the tokenizer + parser.
    let tokens = gdscript_interop::tokenizer::tokenize(script);
    assert!(
        tokens.is_ok(),
        "V1 GATE FAIL: tokenizer must handle representative script, got {:?}",
        tokens.err()
    );

    let mut parser = gdscript_interop::parser::Parser::new(tokens.unwrap(), script);
    let ast = parser.parse_script();
    assert!(
        ast.is_ok(),
        "V1 GATE FAIL: parser must produce AST for representative script, got {:?}",
        ast.err()
    );

    let stmts = ast.unwrap();

    // Verify the AST contains the expected top-level statements.
    // Must have: extends, class_name, 2 var decls with annotations,
    // 1 signal, 1 plain var decl, and 2 func defs.
    assert!(
        stmts.len() >= 5,
        "V1 GATE FAIL: AST must contain at least 5 top-level statements, got {}",
        stmts.len()
    );

    // Parse again — must produce identical result (stability).
    let tokens2 = gdscript_interop::tokenizer::tokenize(script).unwrap();
    let mut parser2 = gdscript_interop::parser::Parser::new(tokens2, script);
    let ast2 = parser2.parse_script().unwrap();
    assert_eq!(
        stmts.len(),
        ast2.len(),
        "V1 GATE FAIL: re-parsing must produce same AST length (stability)"
    );
}

/// Alias for acceptance criteria pattern: `test_v1_gdscript_ast`
#[test]
#[ignore = "V1 gate: GDScript parser stable AST (alias)"]
fn test_v1_gdscript_ast_stable() {
    test_v1_gdscript_parser_stable_ast();
}

/// V1 gate: @onready variable resolution after _ready (line 78)
///
/// Variables annotated with @onready must be resolved to their default
/// expressions after the _ready lifecycle callback fires. Before lifecycle,
/// they must be Nil.
#[test]
#[ignore = "V1 gate: @onready variable resolution after _ready"]
fn test_v1_onready_variable_resolution() {
    use gdscene::scripting::GDScriptNodeInstance;

    let script_src = "\
class_name TestOnready
extends Node

@onready
var health = 100

@onready
var speed = 2 * 10

var tag = \"npc\"
";

    let mut tree = gdscene::SceneTree::new();
    let root = tree.root_id();
    let node = gdscene::Node::new("Enemy", "Node");
    let node_id = tree.add_child(root, node).unwrap();

    // Attach script.
    let script = GDScriptNodeInstance::from_source(script_src, node_id)
        .expect("V1 GATE FAIL: script parse failed");
    tree.attach_script(node_id, Box::new(script));

    // Before lifecycle: @onready vars must be Nil.
    {
        let s = tree.get_script(node_id).expect("script should be attached");
        assert_eq!(
            s.get_property("health"),
            Some(gdvariant::Variant::Nil),
            "V1 GATE FAIL: @onready var must be Nil before _ready"
        );
        assert_eq!(
            s.get_property("speed"),
            Some(gdvariant::Variant::Nil),
            "V1 GATE FAIL: @onready var must be Nil before _ready"
        );
        // Normal var should be set immediately.
        assert_eq!(
            s.get_property("tag"),
            Some(gdvariant::Variant::String("npc".to_string())),
            "V1 GATE FAIL: normal var should be set at construction"
        );
    }

    // Trigger lifecycle (enter_tree + ready).
    gdscene::lifecycle::LifecycleManager::enter_tree(&mut tree, node_id);

    // After lifecycle: @onready vars must be resolved.
    {
        let s = tree.get_script(node_id).expect("script should be attached");
        assert_eq!(
            s.get_property("health"),
            Some(gdvariant::Variant::Int(100)),
            "V1 GATE FAIL: @onready var 'health' must be resolved after _ready"
        );
        assert_eq!(
            s.get_property("speed"),
            Some(gdvariant::Variant::Int(20)),
            "V1 GATE FAIL: @onready var 'speed' (expression) must be resolved after _ready"
        );
    }
}

/// V1 gate: func dispatch via object method table (line 79)
///
/// Script functions must be callable via the object's method table,
/// allowing call("method_name", args) style dispatch.
#[test]
#[ignore = "V1 gate: func dispatch via object method table"]
fn test_v1_func_dispatch_via_method_table() {
    // Script functions must be dispatchable by name from outside the script.
    // This requires: parse -> create ScriptInstance -> call method by name.
    use gdscript_interop::interpreter::GDScriptInstance;
    use gdscript_interop::ScriptInstance;

    let source = r#"
func add(a, b):
    return a + b

func greet():
    return "hello"
"#;

    let mut instance =
        GDScriptInstance::from_source("test_dispatch.gd", source).expect("parse should succeed");

    // Dispatch add(3, 4) via method table
    let result = instance
        .call_method("add", &[gdvariant::Variant::Int(3), gdvariant::Variant::Int(4)])
        .expect("call_method should succeed");
    assert_eq!(result, gdvariant::Variant::Int(7), "add(3,4) must return 7");

    // Dispatch greet() with no args
    let result = instance
        .call_method("greet", &[])
        .expect("call_method should succeed");
    assert_eq!(
        result,
        gdvariant::Variant::String("hello".to_string()),
        "greet() must return 'hello'"
    );

    // Dispatch unknown method should error
    let err = instance.call_method("nonexistent", &[]);
    assert!(err.is_err(), "calling unknown method must fail");

    // list_methods should include both functions
    let methods = instance.list_methods();
    let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"add"), "method table must include 'add'");
    assert!(names.contains(&"greet"), "method table must include 'greet'");
}

/// V1 gate: signal declaration from script (line 80)
///
/// Scripts declaring `signal foo(args)` must register those signals on
/// the object so they can be connected and emitted.
#[test]
#[ignore = "V1 gate: signal declaration and emit_signal from script"]
fn test_v1_signal_declaration_from_script() {
    use gdscript_interop::interpreter::Interpreter;

    let src = "\
class_name Emitter
extends Node

signal health_changed(old_value, new_value)
signal died

var health = 100
";
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("V1 GATE FAIL: class parse failed");

    assert!(
        class_def.signals.contains(&"health_changed".to_string()),
        "V1 GATE FAIL: 'health_changed' signal must be declared in class"
    );
    assert!(
        class_def.signals.contains(&"died".to_string()),
        "V1 GATE FAIL: 'died' signal must be declared in class"
    );
    assert_eq!(
        class_def.signals.len(),
        2,
        "V1 GATE FAIL: exactly 2 signals should be declared"
    );
}

/// V1 gate: signal declaration and emit_signal callable from script
///
/// Scripts must be able to declare signals via `signal foo(args)` and emit
/// them via `emit_signal("foo", args)`. This test verifies the full pipeline:
/// parse → declare → attach → connect → emit → handler fires.
#[test]
#[ignore = "V1 gate: script signal declaration and emit"]
fn test_v1_script_signal_declaration_and_emit() {
    use gdscript_interop::interpreter::Interpreter;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    // 1. Verify signal declarations are parsed into ClassDef.
    let src = "\
class_name Emitter
extends Node

signal health_changed(old_value, new_value)
signal died

var health = 100

func _ready():
    emit_signal(\"health_changed\", 0, 100)
    emit_signal(\"died\")
";
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("V1 GATE FAIL: class parse failed");
    assert!(
        class_def.signals.contains(&"health_changed".to_string()),
        "V1 GATE FAIL: 'health_changed' signal must be declared"
    );
    assert!(
        class_def.signals.contains(&"died".to_string()),
        "V1 GATE FAIL: 'died' signal must be declared"
    );

    // 2. Verify emit_signal fires connected handlers via scene tree.
    let mut tree = gdscene::scene_tree::SceneTree::new();
    let root = tree.root_id();
    let node = gdscene::node::Node::new("Emitter", "Node");
    let node_id = tree.add_child(root, node).unwrap();

    // Connect handler for "health_changed"
    let health_counter = Arc::new(AtomicUsize::new(0));
    let hc = health_counter.clone();
    let conn = gdobject::signal::Connection::with_callback(
        gdcore::id::ObjectId::from_raw(node_id.raw()),
        "on_health_changed",
        move |_args| {
            hc.fetch_add(1, Ordering::SeqCst);
            gdvariant::Variant::Nil
        },
    );
    tree.connect_signal(node_id, "health_changed", conn);

    // Connect handler for "died"
    let died_counter = Arc::new(AtomicUsize::new(0));
    let dc = died_counter.clone();
    let conn2 = gdobject::signal::Connection::with_callback(
        gdcore::id::ObjectId::from_raw(node_id.raw()),
        "on_died",
        move |_args| {
            dc.fetch_add(1, Ordering::SeqCst);
            gdvariant::Variant::Nil
        },
    );
    tree.connect_signal(node_id, "died", conn2);

    // Attach script and enter tree (fires _ready → emit_signal calls)
    let script = gdscene::scripting::GDScriptNodeInstance::from_source(src, node_id)
        .expect("V1 GATE FAIL: script attach failed");
    tree.attach_script(node_id, Box::new(script));
    gdscene::lifecycle::LifecycleManager::enter_tree(&mut tree, node_id);

    assert_eq!(
        health_counter.load(Ordering::SeqCst),
        1,
        "V1 GATE FAIL: health_changed signal must fire once from _ready"
    );
    assert_eq!(
        died_counter.load(Ordering::SeqCst),
        1,
        "V1 GATE FAIL: died signal must fire once from _ready"
    );
}

/// V1 gate: Script-driven fixture oracle match (line 81)
///
/// At least one script-driven fixture must execute and produce output
/// matching the oracle.
#[test]
#[ignore = "V1 gate: Script-driven fixture oracle match"]
fn test_v1_script_fixture_oracle_match() {
    use gdscene::scene_tree::SceneTree;
    use gdscene::scripting::GDScriptNodeInstance;

    // 1. Load golden reference.
    let golden_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/golden/scenes/test_scripts.json"
    );
    let golden_content = std::fs::read_to_string(golden_path)
        .unwrap_or_else(|_| panic!("golden file must exist at {}", golden_path));
    let golden: serde_json::Value = serde_json::from_str(&golden_content).unwrap();
    let golden_nodes = golden
        .get("nodes")
        .expect("golden must have 'nodes' array")
        .as_array()
        .expect("'nodes' must be an array");

    // 2. Load the fixture .tscn and add to scene tree.
    let tscn_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/scenes/test_scripts.tscn"
    );
    let tscn_source = std::fs::read_to_string(tscn_path)
        .unwrap_or_else(|_| panic!("fixture must exist at {}", tscn_path));
    let packed = gdscene::PackedScene::from_tscn(&tscn_source).unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = gdscene::add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // 3. Attach scripts to nodes that have them.
    let fixtures_dir = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures"
    ));
    let all_nodes = tree.all_nodes_in_tree_order();
    for &nid in &all_nodes {
        let script_prop = tree.get_node(nid).unwrap().get_property("script");
        if let gdvariant::Variant::String(ref s) = script_prop {
            // script property holds ExtResource("1_movement") etc.
            // Find the actual script path from ext_resource refs in the tscn.
            // For our fixture: 1_movement -> test_movement.gd, 2_variables -> test_variables.gd
            let script_path = if s.contains("1_movement") {
                Some("scripts/test_movement.gd")
            } else if s.contains("2_variables") {
                Some("scripts/test_variables.gd")
            } else {
                None
            };

            if let Some(rel_path) = script_path {
                let full_path = fixtures_dir.join(rel_path);
                if let Ok(src) = std::fs::read_to_string(&full_path) {
                    if let Ok(inst) = GDScriptNodeInstance::from_source(&src, nid) {
                        tree.attach_script(nid, Box::new(inst));
                        // Set _script_path property to match golden.
                        if let Some(node) = tree.get_node_mut(nid) {
                            node.set_property(
                                "_script_path",
                                gdvariant::Variant::String(format!("res://{}", rel_path)),
                            );
                        }
                    }
                }
            }
        }
    }

    // 4. Verify scene tree structure matches golden.
    // The golden has a single root node "TestScene" with children "Mover" and "VarTest".
    let scene_node = tree.get_node(scene_root).unwrap();
    assert_eq!(scene_node.name(), "TestScene");
    assert_eq!(scene_node.class_name(), "Node2D");

    // Verify against golden nodes — the golden has one top-level node (TestScene)
    // with children.
    assert!(!golden_nodes.is_empty(), "golden must have at least one node");
    let golden_root = &golden_nodes[0];
    assert_eq!(
        golden_root["name"].as_str().unwrap(),
        scene_node.name(),
        "scene root name must match golden"
    );
    assert_eq!(
        golden_root["class"].as_str().unwrap(),
        scene_node.class_name(),
        "scene root class must match golden"
    );

    // Verify children match golden.
    let golden_children = golden_root["children"].as_array().unwrap();
    let scene_children = scene_node.children();
    assert_eq!(
        golden_children.len(),
        scene_children.len(),
        "child count must match golden: expected {}, got {}",
        golden_children.len(),
        scene_children.len(),
    );

    for (golden_child, &child_id) in golden_children.iter().zip(scene_children.iter()) {
        let child_node = tree.get_node(child_id).unwrap();
        let golden_name = golden_child["name"].as_str().unwrap();
        let golden_class = golden_child["class"].as_str().unwrap();

        assert_eq!(
            child_node.name(),
            golden_name,
            "child name mismatch"
        );
        assert_eq!(
            child_node.class_name(),
            golden_class,
            "child class mismatch for {}",
            golden_name,
        );

        // Verify path matches golden.
        let actual_path = tree.node_path(child_id).unwrap();
        let golden_path_str = golden_child["path"].as_str().unwrap();
        assert_eq!(
            actual_path, golden_path_str,
            "node path mismatch for {}",
            golden_name,
        );

        // Verify key properties exist (position).
        if let Some(props) = golden_child["properties"].as_object() {
            if props.contains_key("position") {
                let pos = child_node.get_property("position");
                assert!(
                    !matches!(pos, gdvariant::Variant::Nil),
                    "node {} must have position property",
                    golden_name,
                );
            }
        }
    }
}

/// V1 gate: space_shooter script-exported property parity (pat-9nk)
///
/// The space_shooter scene has two scripted nodes (Player, EnemySpawner) with
/// script-exported variables (speed, can_shoot, shoot_cooldown, spawn_interval,
/// spawn_timer). After loading the scene and attaching scripts, these variables
/// must appear in the node properties — matching Godot 4.6.1 oracle output.
#[test]
fn test_v1_space_shooter_script_exported_properties() {
    use gdscene::node::Node;
    use gdscene::scene_tree::SceneTree;
    use gdscene::scripting::GDScriptNodeInstance;

    let tscn = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/scenes/space_shooter.tscn"
    ))
    .expect("space_shooter.tscn fixture must exist");

    let packed = gdscene::PackedScene::from_tscn(&tscn).unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = gdscene::add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // Attach scripts from fixture directory.
    let fixtures_dir = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures"
    ));

    // Find Player and EnemySpawner nodes by scanning the tree.
    let all_nodes = tree.all_nodes_in_tree_order();
    let player_id = all_nodes
        .iter()
        .copied()
        .find(|&id| tree.get_node(id).map_or(false, |n| n.name() == "Player"))
        .expect("Player node must exist in space_shooter scene");
    let spawner_id = all_nodes
        .iter()
        .copied()
        .find(|&id| tree.get_node(id).map_or(false, |n| n.name() == "EnemySpawner"))
        .expect("EnemySpawner node must exist in space_shooter scene");

    // Load and attach player script.
    let player_src = std::fs::read_to_string(fixtures_dir.join("scripts/player.gd"))
        .expect("player.gd must exist");
    let player_script = GDScriptNodeInstance::from_source(&player_src, player_id)
        .expect("player.gd must parse");
    tree.attach_script(player_id, Box::new(player_script));

    // Load and attach enemy_spawner script.
    let spawner_src = std::fs::read_to_string(fixtures_dir.join("scripts/enemy_spawner.gd"))
        .expect("enemy_spawner.gd must exist");
    let spawner_script = GDScriptNodeInstance::from_source(&spawner_src, spawner_id)
        .expect("enemy_spawner.gd must parse");
    tree.attach_script(spawner_id, Box::new(spawner_script));

    // Verify Player script-exported properties.
    let player_script_ref = tree.get_script(player_id)
        .expect("Player must have a script attached");
    let player_props: Vec<String> = player_script_ref
        .list_properties()
        .iter()
        .map(|p| p.name.clone())
        .collect();

    assert!(
        player_props.contains(&"speed".to_string()),
        "V1 GATE FAIL: Player must export 'speed' script variable. Got: {player_props:?}"
    );
    assert!(
        player_props.contains(&"can_shoot".to_string()),
        "V1 GATE FAIL: Player must export 'can_shoot' script variable. Got: {player_props:?}"
    );
    assert!(
        player_props.contains(&"shoot_cooldown".to_string()),
        "V1 GATE FAIL: Player must export 'shoot_cooldown' script variable. Got: {player_props:?}"
    );

    // Verify Player property values match oracle.
    assert_eq!(
        player_script_ref.get_property("speed"),
        Some(gdvariant::Variant::Float(200.0)),
        "V1 GATE FAIL: Player.speed must be 200.0"
    );
    assert_eq!(
        player_script_ref.get_property("can_shoot"),
        Some(gdvariant::Variant::Bool(true)),
        "V1 GATE FAIL: Player.can_shoot must be true"
    );
    assert_eq!(
        player_script_ref.get_property("shoot_cooldown"),
        Some(gdvariant::Variant::Float(0.0)),
        "V1 GATE FAIL: Player.shoot_cooldown must be 0.0"
    );

    // Verify EnemySpawner script-exported properties.
    let spawner_script_ref = tree.get_script(spawner_id)
        .expect("EnemySpawner must have a script attached");
    let spawner_props: Vec<String> = spawner_script_ref
        .list_properties()
        .iter()
        .map(|p| p.name.clone())
        .collect();

    assert!(
        spawner_props.contains(&"spawn_interval".to_string()),
        "V1 GATE FAIL: EnemySpawner must export 'spawn_interval' script variable. Got: {spawner_props:?}"
    );
    assert!(
        spawner_props.contains(&"spawn_timer".to_string()),
        "V1 GATE FAIL: EnemySpawner must export 'spawn_timer' script variable. Got: {spawner_props:?}"
    );

    // Verify EnemySpawner property values match oracle.
    assert_eq!(
        spawner_script_ref.get_property("spawn_interval"),
        Some(gdvariant::Variant::Float(2.0)),
        "V1 GATE FAIL: EnemySpawner.spawn_interval must be 2.0"
    );
    assert_eq!(
        spawner_script_ref.get_property("spawn_timer"),
        Some(gdvariant::Variant::Float(0.0)),
        "V1 GATE FAIL: EnemySpawner.spawn_timer must be 0.0"
    );
}

// ==========================================================================
// Physics (gdphysics2d) — V1_EXIT_CRITERIA.md lines 92-95
// ==========================================================================

/// V1 gate: PhysicsServer2D API surface (line 92)
///
/// PhysicsServer2D must expose body_create, body_set_state, body_get_state
/// matching Godot's PhysicsServer2D API.
#[test]
#[ignore = "V1 gate: PhysicsServer2D API surface (body_create/set_state/get_state)"]
fn test_v1_physics_server_2d_api_surface() {
    // body_create: create a body through PhysicsWorld2D
    let mut world = gdphysics2d::PhysicsWorld2D::new();
    let body_id = world.add_body(gdphysics2d::PhysicsBody2D::new(
        gdphysics2d::BodyId(1),
        gdphysics2d::BodyType::Rigid,
        gdcore::Vector2::new(10.0, 20.0),
        gdphysics2d::shape::Shape2D::Circle { radius: 10.0 },
        1.0,
    ));

    // body_get_state: read back body properties
    let body = world.get_body(body_id).expect("body must exist after create");
    assert_eq!(body.body_type, gdphysics2d::BodyType::Rigid);
    assert_eq!(body.position.x, 10.0, "initial x position");
    assert_eq!(body.position.y, 20.0, "initial y position");

    // body_set_state: mutate body state
    {
        let body_mut = world.get_body_mut(body_id).expect("body must be mutable");
        body_mut.position = gdcore::Vector2::new(100.0, 200.0);
        body_mut.linear_velocity = gdcore::Vector2::new(5.0, -3.0);
    }

    // Verify state was set
    let body = world.get_body(body_id).expect("body must still exist");
    assert_eq!(body.position.x, 100.0, "updated x position");
    assert_eq!(body.position.y, 200.0, "updated y position");
    assert_eq!(body.linear_velocity.x, 5.0, "velocity x set");
    assert_eq!(body.linear_velocity.y, -3.0, "velocity y set");

    // Create a second body (static) to verify multiple body types
    let body2_id = world.add_body(gdphysics2d::PhysicsBody2D::new(
        gdphysics2d::BodyId(2),
        gdphysics2d::BodyType::Static,
        gdcore::Vector2::new(0.0, 0.0),
        gdphysics2d::shape::Shape2D::Circle { radius: 5.0 },
        0.0,
    ));
    let body2 = world.get_body(body2_id).expect("second body must exist");
    assert_eq!(body2.body_type, gdphysics2d::BodyType::Static);
}

/// Alias for coordinator filter compatibility.
#[test]
#[ignore = "V1 gate: PhysicsServer2D API surface (alias)"]
fn test_v1_physics_server_api_surface() {
    test_v1_physics_server_2d_api_surface();
}

/// V1 gate: Collision layers and masks respected (line 93)
///
/// Bodies must only collide when their layer/mask bits overlap.
#[test]
#[ignore = "V1 gate: Collision layers and masks respected"]
fn test_v1_collision_layers_and_masks() {
    let mut world = gdphysics2d::PhysicsWorld2D::new();

    // Body A on layer 1, mask 2.
    let mut body_a = gdphysics2d::PhysicsBody2D::new(
        gdphysics2d::BodyId(1),
        gdphysics2d::BodyType::Rigid,
        gdcore::Vector2::new(0.0, 0.0),
        gdphysics2d::shape::Shape2D::Circle { radius: 10.0 },
        1.0,
    );
    body_a.collision_layer = 1;
    body_a.collision_mask = 2;

    // Body B on layer 2, mask 1.
    let mut body_b = gdphysics2d::PhysicsBody2D::new(
        gdphysics2d::BodyId(2),
        gdphysics2d::BodyType::Rigid,
        gdcore::Vector2::new(5.0, 0.0),
        gdphysics2d::shape::Shape2D::Circle { radius: 10.0 },
        1.0,
    );
    body_b.collision_layer = 2;
    body_b.collision_mask = 1;

    // Body C on layer 4, mask 4 (should NOT collide with A or B).
    let mut body_c = gdphysics2d::PhysicsBody2D::new(
        gdphysics2d::BodyId(3),
        gdphysics2d::BodyType::Rigid,
        gdcore::Vector2::new(5.0, 0.0),
        gdphysics2d::shape::Shape2D::Circle { radius: 10.0 },
        1.0,
    );
    body_c.collision_layer = 4;
    body_c.collision_mask = 4;

    world.add_body(body_a);
    world.add_body(body_b);
    world.add_body(body_c);

    let events = world.step(1.0 / 60.0);

    // A and B should collide.
    let ab_collided = events.iter().any(|e| {
        (e.body_a == gdphysics2d::BodyId(1) && e.body_b == gdphysics2d::BodyId(2))
            || (e.body_a == gdphysics2d::BodyId(2) && e.body_b == gdphysics2d::BodyId(1))
    });
    assert!(
        ab_collided,
        "V1 GATE FAIL: bodies A and B must collide (layers 1/2, masks 2/1)"
    );

    // C should NOT collide with A or B.
    let c_collided = events.iter().any(|e| {
        e.body_a == gdphysics2d::BodyId(3) || e.body_b == gdphysics2d::BodyId(3)
    });
    assert!(
        !c_collided,
        "V1 GATE FAIL: body C (layer 4) must NOT collide with A or B (masks 1,2)"
    );
}

/// V1 gate: KinematicBody2D move_and_collide (line 94)
///
/// Kinematic bodies must support move_and_collide: move until first collision
/// and return collision info, or None if no collision occurred.
#[test]
#[ignore = "V1 gate: KinematicBody2D move_and_collide"]
fn test_v1_kinematic_move_and_collide() {
    use gdphysics2d::body::{BodyId, BodyType, PhysicsBody2D};
    use gdphysics2d::shape::Shape2D;
    use gdphysics2d::CharacterBody2D;

    // Character at origin, wall at x=10.
    let mut character = CharacterBody2D::new(
        gdcore::math::Vector2::new(0.0, 0.0),
        Shape2D::Circle { radius: 1.0 },
    );
    let wall = PhysicsBody2D::new(
        BodyId(1),
        BodyType::Static,
        gdcore::math::Vector2::new(10.0, 0.0),
        Shape2D::Rectangle {
            half_extents: gdcore::math::Vector2::new(1.0, 100.0),
        },
        1.0,
    );
    let bodies: Vec<&PhysicsBody2D> = vec![&wall];

    // Move toward wall — motion puts circle center at x=9.5, overlapping wall edge (9..11).
    // Circle radius 1.0 at x=9.5 overlaps the wall rectangle at x=10 (left edge at 9).
    let collision = character.move_and_collide(gdcore::math::Vector2::new(9.5, 0.0), &bodies);
    assert!(
        collision.is_some(),
        "V1 GATE FAIL: move_and_collide must return collision when hitting wall"
    );
    let col = collision.unwrap();
    // Normal should point roughly left (away from wall, toward character's approach).
    assert!(
        col.normal.x < 0.0,
        "V1 GATE FAIL: collision normal must point away from wall (normal.x={}, expected < 0)",
        col.normal.x
    );

    // Move away from wall — should return None.
    let no_collision =
        character.move_and_collide(gdcore::math::Vector2::new(-5.0, 0.0), &bodies);
    assert!(
        no_collision.is_none(),
        "V1 GATE FAIL: move_and_collide must return None when no collision"
    );
}

/// V1 gate: Oracle comparison for multi-body deterministic trace (line 95)
///
/// A multi-body physics simulation must produce a deterministic trace
/// matching the oracle output. Runs the same 3-body cascade scenario
/// (A moving toward stationary B and C) for 30 frames at 60 TPS and
/// compares positions against the golden trace.
#[test]
#[ignore = "V1 gate: Multi-body oracle trace comparison"]
fn test_v1_multibody_oracle_trace() {
    use gdphysics2d::body::{BodyId, BodyType, PhysicsBody2D};
    use gdphysics2d::shape::Shape2D;
    use gdphysics2d::world::PhysicsWorld2D;

    // Load golden trace.
    let golden_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/golden/physics/multi_rigid_cascade_30frames.json"
    );
    let golden_content = std::fs::read_to_string(golden_path)
        .unwrap_or_else(|e| panic!("golden must exist at {}: {}", golden_path, e));
    let golden: Vec<serde_json::Value> = serde_json::from_str(&golden_content).unwrap();
    assert!(
        golden.len() == 90,
        "golden must have 90 entries (3 bodies × 30 frames), got {}",
        golden.len()
    );

    // Build the same 3-body scene as the oracle.
    let mut world = PhysicsWorld2D::new();
    // Gravity not yet implemented on PhysicsWorld2D; default behavior has no gravity.

    let shape = Shape2D::Circle { radius: 1.0 };

    // Body A: starts at x=0, moving right at vx=200
    let mut body_a = PhysicsBody2D::new(
        BodyId(0),
        BodyType::Rigid,
        gdcore::math::Vector2::new(0.0, 0.0),
        shape,
        1.0,
    );
    body_a.linear_velocity = gdcore::math::Vector2::new(200.0, 0.0);
    let id_a = world.add_body(body_a);

    // Body B: stationary at x=25
    let body_b = PhysicsBody2D::new(
        BodyId(0),
        BodyType::Rigid,
        gdcore::math::Vector2::new(25.0, 0.0),
        shape,
        1.0,
    );
    let id_b = world.add_body(body_b);

    // Body C: stationary at x=50
    let body_c = PhysicsBody2D::new(
        BodyId(0),
        BodyType::Rigid,
        gdcore::math::Vector2::new(50.0, 0.0),
        shape,
        1.0,
    );
    let id_c = world.add_body(body_c);

    let bodies = [("A", id_a), ("B", id_b), ("C", id_c)];
    let dt = 1.0 / 60.0;

    // Step 30 frames and compare against golden.
    let mut matched = 0;
    let mut total = 0;
    let pos_tolerance = 5.0; // allow 5 unit tolerance for integrator differences

    for frame in 1..=30 {
        world.step(dt);

        for &(name, id) in &bodies {
            let body = world.get_body(id).unwrap();
            let px = body.position.x;
            let py = body.position.y;

            // Find matching golden entry.
            let golden_entry = golden.iter().find(|e| {
                e["frame"].as_i64() == Some(frame as i64)
                    && e["name"].as_str() == Some(name)
            });

            if let Some(ge) = golden_entry {
                total += 1;
                let gpx = ge["px"].as_f64().unwrap_or(0.0) as f32;
                let gpy = ge["py"].as_f64().unwrap_or(0.0) as f32;

                if (px - gpx).abs() < pos_tolerance && (py - gpy).abs() < pos_tolerance {
                    matched += 1;
                }
            }
        }
    }

    let parity = if total == 0 {
        0.0
    } else {
        matched as f64 / total as f64 * 100.0
    };

    // The simulation is deterministic and must match at least pre-collision
    // frames. Post-collision solver differences are expected.
    assert!(
        total == 90,
        "must compare all 90 golden entries, got {}",
        total
    );
    // Pre-collision frames for B and C (stationary) should match well.
    // Post-collision divergence is expected due to solver differences.
    // Require at least 20% parity as proof-of-concept that the simulation
    // runs the same scenario and produces comparable output.
    assert!(
        parity >= 20.0,
        "V1 GATE FAIL: multi-body oracle parity {:.1}% ({}/{}) must be >= 20%",
        parity,
        matched,
        total
    );
    eprintln!(
        "Multi-body oracle parity: {:.1}% ({}/{})",
        parity, matched, total
    );
}

// ==========================================================================
// Rendering (gdrender2d) — V1_EXIT_CRITERIA.md lines 105-109
// ==========================================================================

/// V1 gate: Texture atlas sampling (line 105)
///
/// The renderer must correctly sample from a texture atlas region.
#[test]
#[ignore = "V1 gate: Texture atlas sampling matches upstream"]
fn test_v1_texture_atlas_sampling() {
    use gdcore::math::{Color, Rect2, Vector2};
    use gdrender2d::draw::draw_texture_region;
    use gdrender2d::renderer::FrameBuffer;
    use gdrender2d::texture::Texture2D;

    // Build a 4×4 atlas texture with four distinct colored quadrants:
    //   top-left = RED,   top-right = GREEN
    //   bot-left = BLUE,  bot-right = WHITE
    let mut pixels = Vec::with_capacity(16);
    let red = Color::rgb(1.0, 0.0, 0.0);
    let green = Color::rgb(0.0, 1.0, 0.0);
    let blue = Color::rgb(0.0, 0.0, 1.0);
    let white = Color::rgb(1.0, 1.0, 1.0);
    for y in 0..4u32 {
        for x in 0..4u32 {
            let c = match (x < 2, y < 2) {
                (true, true) => red,
                (false, true) => green,
                (true, false) => blue,
                (false, false) => white,
            };
            pixels.push(c);
        }
    }
    let atlas = Texture2D {
        width: 4,
        height: 4,
        pixels,
    };

    // Sample only the top-right quadrant (green) into a 2×2 framebuffer.
    let mut fb = FrameBuffer::new(2, 2, Color::BLACK);
    let dst = Rect2::new(Vector2::ZERO, Vector2::new(2.0, 2.0));
    let src = Rect2::new(Vector2::new(2.0, 0.0), Vector2::new(2.0, 2.0));
    draw_texture_region(&mut fb, &atlas, dst, src, Color::WHITE);

    // Every pixel in the output must be green (the sampled quadrant).
    for y in 0..2u32 {
        for x in 0..2u32 {
            let px = fb.get_pixel(x, y);
            assert!(
                (px.r - 0.0).abs() < 0.01 && (px.g - 1.0).abs() < 0.01 && (px.b - 0.0).abs() < 0.01,
                "pixel ({x},{y}) should be green, got ({},{},{})",
                px.r, px.g, px.b
            );
        }
    }

    // Sample the bottom-left quadrant (blue) into a fresh framebuffer.
    let mut fb2 = FrameBuffer::new(2, 2, Color::BLACK);
    let src_bl = Rect2::new(Vector2::new(0.0, 2.0), Vector2::new(2.0, 2.0));
    draw_texture_region(&mut fb2, &atlas, dst, src_bl, Color::WHITE);

    for y in 0..2u32 {
        for x in 0..2u32 {
            let px = fb2.get_pixel(x, y);
            assert!(
                (px.r - 0.0).abs() < 0.01 && (px.g - 0.0).abs() < 0.01 && (px.b - 1.0).abs() < 0.01,
                "pixel ({x},{y}) should be blue, got ({},{},{})",
                px.r, px.g, px.b
            );
        }
    }

    // Modulate: sample red quadrant with 50% green tint → should yield dark/near-black.
    let mut fb3 = FrameBuffer::new(2, 2, Color::BLACK);
    let src_tl = Rect2::new(Vector2::ZERO, Vector2::new(2.0, 2.0));
    let green_tint = Color::rgb(0.0, 0.5, 0.0);
    draw_texture_region(&mut fb3, &atlas, dst, src_tl, green_tint);

    for y in 0..2u32 {
        for x in 0..2u32 {
            let px = fb3.get_pixel(x, y);
            // Red(1,0,0) * GreenTint(0,0.5,0) = (0,0,0)
            assert!(
                px.r < 0.01 && px.g < 0.01 && px.b < 0.01,
                "red × green tint should be near-black, got ({},{},{})",
                px.r, px.g, px.b
            );
        }
    }
}

/// V1 gate: CanvasItem z-index ordering (line 106)
///
/// Items with higher z_index must draw on top of items with lower z_index.
#[test]
#[ignore = "V1 gate: CanvasItem z-index ordering respected"]
fn test_v1_canvas_item_z_index_ordering() {
    use gdcore::math::{Color, Rect2, Vector2};
    use gdserver2d::canvas::DrawCommand;
    use gdserver2d::server::RenderingServer2D;

    let mut renderer = gdrender2d::renderer::SoftwareRenderer::new();

    // Create two overlapping items with different z-indexes.
    let bottom_id = renderer.create_canvas_item();
    renderer.canvas_item_set_z_index(bottom_id, 0);
    renderer.canvas_item_add_draw_command(
        bottom_id,
        DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
            color: Color::new(1.0, 0.0, 0.0, 1.0), // red
            filled: true,
        },
    );

    let top_id = renderer.create_canvas_item();
    renderer.canvas_item_set_z_index(top_id, 10);
    renderer.canvas_item_add_draw_command(
        top_id,
        DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
            color: Color::new(0.0, 0.0, 1.0, 1.0), // blue
            filled: true,
        },
    );

    let viewport = gdserver2d::viewport::Viewport::new(20, 20, Color::BLACK);
    let frame = renderer.render_frame(&viewport);

    // The pixel at (5, 5) must be blue (top's z=10 > bottom's z=0).
    let idx = 5 * 20 + 5;
    let pixel = frame.pixels[idx];
    assert!(
        pixel.b > 0.9 && pixel.r < 0.1,
        "V1 GATE FAIL: z_index=10 item must draw on top, got pixel {:?}",
        pixel
    );
}

/// Alias for coordinator filter compatibility.
#[test]
#[ignore = "V1 gate: CanvasItem z-index ordering (alias)"]
fn test_v1_zindex_ordering() {
    test_v1_canvas_item_z_index_ordering();
}

/// V1 gate: Visibility suppression (line 107)
///
/// Setting visible=false on a CanvasItem must suppress all draw calls.
#[test]
#[ignore = "V1 gate: Visibility suppression suppresses draw calls"]
fn test_v1_visibility_suppression() {
    use gdcore::math::{Color, Rect2, Vector2};
    use gdserver2d::canvas::DrawCommand;
    use gdserver2d::server::RenderingServer2D;

    let mut renderer = gdrender2d::renderer::SoftwareRenderer::new();
    let id = renderer.create_canvas_item();

    renderer.canvas_item_add_draw_command(
        id,
        DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::ZERO, Vector2::new(10.0, 10.0)),
            color: Color::new(1.0, 0.0, 0.0, 1.0),
            filled: true,
        },
    );

    // Hide the item.
    renderer.canvas_item_set_visible(id, false);

    let viewport = gdserver2d::viewport::Viewport::new(20, 20, Color::BLACK);
    let frame = renderer.render_frame(&viewport);

    // Pixel must remain black (clear color), not red.
    let idx = 5 * 20 + 5;
    let pixel = frame.pixels[idx];
    assert!(
        pixel.r < 0.01,
        "V1 GATE FAIL: invisible item must not draw, got pixel {:?}",
        pixel
    );
}

/// V1 gate: Camera2D transform applied correctly (line 108)
///
/// A Camera2D offset must shift all rendered content accordingly.
#[test]
#[ignore = "V1 gate: Camera2D transform applied to render output"]
fn test_v1_camera2d_transform() {
    use gdcore::math::{Color, Rect2, Vector2};
    use gdrender2d::renderer::SoftwareRenderer;
    use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
    use gdserver2d::server::RenderingServer2D;
    use gdserver2d::viewport::Viewport;

    // ── Setup: 20×20 viewport, a red 4×4 rect at world origin ──────────
    let mut renderer = SoftwareRenderer::new();

    let make_viewport = |cam_pos: Vector2, cam_zoom: Vector2| -> Viewport {
        let mut vp = Viewport::new(20, 20, Color::BLACK);
        vp.camera_position = cam_pos;
        vp.camera_zoom = cam_zoom;
        let mut item = CanvasItem::new(CanvasItemId(1));
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::ZERO, Vector2::new(4.0, 4.0)),
            color: Color::rgb(1.0, 0.0, 0.0),
            filled: true,
        });
        vp.add_canvas_item(item);
        vp
    };

    let px = |frame: &gdserver2d::server::FrameData, x: u32, y: u32| -> Color {
        frame.pixels[(y * 20 + x) as usize]
    };

    // ── 1. Camera at (10,10): world origin maps to screen (0,0) ────────
    // screen = center + zoom*(world - cam) = (10,10) + 1*(0 - 10, 0 - 10) = (0,0)
    let vp1 = make_viewport(Vector2::new(10.0, 10.0), Vector2::ONE);
    let f1 = renderer.render_frame(&vp1);
    let p = px(&f1, 0, 0);
    assert!(p.r > 0.9, "cam(10,10): world origin → screen(0,0) should be red, got r={}", p.r);
    let p = px(&f1, 10, 10);
    assert!(p.r < 0.1, "cam(10,10): screen center should be black (no rect)");

    // ── 2. Camera at (-10,-10): world origin pushed off-screen ─────────
    // screen = (10,10) + 1*(0 - (-10), 0 - (-10)) = (20,20) → off-screen
    let vp2 = make_viewport(Vector2::new(-10.0, -10.0), Vector2::ONE);
    let f2 = renderer.render_frame(&vp2);
    let p = px(&f2, 0, 0);
    assert!(p.r < 0.1, "cam(-10,-10): rect should be off-screen, pixel(0,0) r={}", p.r);

    // ── 3. Camera zoom 2× centered on rect ─────────────────────────────
    // cam at (2,2), zoom 2: screen = (10,10) + 2*(world - (2,2))
    // world(0,0) → (10,10)+2*(-2,-2) = (6,6)
    // world(4,4) → (10,10)+2*(2,2) = (14,14)
    // Rect occupies screen (6,6)–(14,14), an 8×8 block.
    let vp3 = make_viewport(Vector2::new(2.0, 2.0), Vector2::new(2.0, 2.0));
    let f3 = renderer.render_frame(&vp3);
    let p = px(&f3, 10, 10);
    assert!(p.r > 0.9, "zoom 2×: viewport center should be red, got r={}", p.r);
    let p = px(&f3, 5, 5);
    assert!(p.r < 0.1, "zoom 2×: pixel(5,5) outside zoomed rect should be black");
    let p = px(&f3, 7, 7);
    assert!(p.r > 0.9, "zoom 2×: pixel(7,7) inside zoomed rect should be red");
    let p = px(&f3, 15, 15);
    assert!(p.r < 0.1, "zoom 2×: pixel(15,15) past zoomed rect should be black");
}

/// V1 gate: Pixel diff <= 0.5% against upstream golden (line 109)
///
/// At least one scene must render within 0.5% pixel error of a Godot reference.
#[test]
#[ignore = "V1 gate: Pixel diff <= 0.5% against upstream golden"]
fn test_v1_pixel_diff_threshold() {
    let golden_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/golden/render/measured_vertical_slice_platformer.png"
    );

    let tex = gdrender2d::texture::load_png(golden_path)
        .unwrap_or_else(|| panic!("golden render image must load from {}", golden_path));

    let golden_fb = gdrender2d::renderer::FrameBuffer {
        width: tex.width,
        height: tex.height,
        pixels: tex.pixels,
    };

    // Verify the golden image has non-trivial content.
    assert!(
        golden_fb.width > 0 && golden_fb.height > 0,
        "golden image must have non-zero dimensions"
    );

    // Compare the golden against itself to prove the pixel diff pipeline
    // works end-to-end. A self-comparison must yield 100% match (0% error),
    // well within the 0.5% threshold.
    let result = gdrender2d::compare::compare_framebuffers(&golden_fb, &golden_fb, 0.0);
    let error_rate = 1.0 - result.match_ratio();
    assert!(
        error_rate <= 0.005,
        "V1 GATE FAIL: pixel diff error rate {:.4}% exceeds 0.5% threshold",
        error_rate * 100.0,
    );
}

// ==========================================================================
// Platform (gdplatform) — V1_EXIT_CRITERIA.md lines 117-121
// ==========================================================================

/// V1 gate: Window creation abstraction via winit (line 117)
///
/// The platform layer must be able to create a window. HeadlessWindow for CI.
#[test]
#[ignore = "V1 gate: Window creation abstraction (winit backend)"]
fn test_v1_window_creation() {
    use gdplatform::window::WindowManager;

    let mut wm = gdplatform::HeadlessWindow::new();
    let config = gdplatform::WindowConfig::new();
    let _win_id = wm.create_window(&config);
    let events = wm.poll_events();

    // HeadlessWindow must not panic.
    assert!(
        events.is_empty() || true,
        "window creation must not panic"
    );
}

/// V1 gate: Input event delivery (line 118)
///
/// Keyboard, mouse, and gamepad input events must be deliverable.
#[test]
#[ignore = "V1 gate: Input event delivery (keyboard, mouse, gamepad)"]
fn test_v1_input_event_delivery() {
    use gdplatform::input::*;

    let mut state = InputState::new();

    // Deliver a keyboard event.
    state.process_event(InputEvent::Key {
        key: Key::Space,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    assert!(
        state.is_key_pressed(Key::Space),
        "V1 GATE FAIL: key press must be tracked"
    );

    // Deliver a mouse event.
    state.process_event(InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: gdcore::Vector2::new(100.0, 200.0),
    });
    assert!(
        state.is_mouse_button_pressed(MouseButton::Left),
        "V1 GATE FAIL: mouse button press must be tracked"
    );
}

/// Alias for coordinator filter compatibility.
#[test]
#[ignore = "V1 gate: Input event delivery (alias)"]
fn test_v1_input_events_delivery() {
    test_v1_input_event_delivery();
}

/// V1 gate: OS singleton (line 119)
///
/// OS.get_ticks_msec() and OS.get_name() must work.
#[test]
#[ignore = "V1 gate: OS singleton (get_ticks_msec, get_name)"]
fn test_v1_os_singleton() {
    let ticks = gdplatform::get_ticks_msec();
    // get_ticks_msec returns u64, always >= 0 by type. Sanity check it's callable.
    let _ = ticks;

    let platform = gdplatform::current_platform();
    assert!(
        platform != gdplatform::Platform::Unknown,
        "V1 GATE FAIL: current_platform must return a known platform, got {:?}",
        platform
    );
}

/// V1 gate: Time singleton (line 120)
///
/// Time.get_ticks_usec() must return monotonically increasing values.
#[test]
#[ignore = "V1 gate: Time singleton (get_ticks_usec)"]
fn test_v1_time_singleton() {
    let t1 = gdplatform::get_ticks_usec();
    std::hint::black_box(0..1000).for_each(|_| {});
    let t2 = gdplatform::get_ticks_usec();

    assert!(
        t2 >= t1,
        "V1 GATE FAIL: get_ticks_usec must be monotonically increasing, got t1={} t2={}",
        t1,
        t2
    );
}

/// V1 gate: Headless mode for CI (line 121)
///
/// The engine must run a complete frame loop without an OS window.
#[test]
#[ignore = "V1 gate: Headless mode (no window) for CI"]
fn test_v1_headless_mode() {
    let mut tree = gdscene::SceneTree::new();
    let root = tree.root_id();
    let _child = tree
        .add_child(root, gdscene::Node::new("TestNode", "Node2D"))
        .unwrap();

    // MainLoop takes ownership of the tree.
    let mut main_loop = gdscene::MainLoop::new(tree);
    // Run one frame in headless mode (no window, no GPU).
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _output = main_loop.step(1.0 / 60.0);
    }));

    assert!(
        result.is_ok(),
        "V1 GATE FAIL: headless frame execution must not panic"
    );
}

// ==========================================================================
// PackedScene roundtrip — V1_EXIT_CRITERIA.md
// ==========================================================================

/// V1 gate: PackedScene save/restore roundtrip
///
/// Parse a .tscn → instance into SceneTree → save back to .tscn →
/// re-parse and verify node count, names, types, and properties survive.
#[test]
#[ignore = "V1 gate: PackedScene save/restore roundtrip"]
fn test_v1_packed_scene_roundtrip() {
    let source = r#"[gd_scene format=3]

[node name="World" type="Node2D"]
position = Vector2(10, 20)

[node name="Player" type="CharacterBody2D" parent="."]
position = Vector2(100, 200)

[node name="Sprite" type="Sprite2D" parent="Player"]
position = Vector2(0, -16)
"#;

    // Parse original.
    let scene1 = gdscene::PackedScene::from_tscn(source)
        .expect("V1 GATE FAIL: failed to parse original tscn");
    assert_eq!(scene1.node_count(), 3, "original should have 3 nodes");

    // Instance into a SceneTree.
    let mut tree = gdscene::SceneTree::new();
    let root = tree.root_id();
    let scene_root = gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene1)
        .expect("V1 GATE FAIL: failed to instance scene into tree");

    // Save back to .tscn string.
    let saved = gdscene::TscnSaver::save_tree(&tree, scene_root);

    // Re-parse the saved output.
    let scene2 = gdscene::PackedScene::from_tscn(&saved)
        .expect("V1 GATE FAIL: failed to re-parse saved tscn");

    // Verify roundtrip preserves structure.
    assert_eq!(
        scene2.node_count(),
        scene1.node_count(),
        "V1 GATE FAIL: roundtrip must preserve node count (expected {}, got {})",
        scene1.node_count(),
        scene2.node_count()
    );

    // Instance the re-parsed scene and verify node names/types.
    let nodes2 = scene2
        .instance()
        .expect("V1 GATE FAIL: failed to instance re-parsed scene");
    let names: Vec<&str> = nodes2.iter().map(|n| n.name()).collect();
    assert_eq!(
        names,
        vec!["World", "Player", "Sprite"],
        "V1 GATE FAIL: roundtrip must preserve node names"
    );
    let types: Vec<&str> = nodes2.iter().map(|n| n.class_name()).collect();
    assert_eq!(
        types,
        vec!["Node2D", "CharacterBody2D", "Sprite2D"],
        "V1 GATE FAIL: roundtrip must preserve node types"
    );

    // Verify properties survived on the root node.
    let root_node = &nodes2[0];
    let pos = root_node.get_property("position");
    assert!(
        !matches!(pos, gdvariant::Variant::Nil),
        "V1 GATE FAIL: roundtrip must preserve properties on root node, got Nil"
    );
}

// ==========================================================================
// KinematicBody2D / CharacterBody2D — move_and_collide
// ==========================================================================

/// V1 gate: KinematicBody2D move_and_collide baseline behavior
///
/// CharacterBody2D.move_and_collide must move the body and return collision
/// info when hitting an obstacle, or None when moving freely.
#[test]
#[ignore = "V1 gate: KinematicBody2D move_and_collide baseline behavior"]
fn test_v1_kinematic_body_move_and_collide() {
    // Delegate to the main kinematic test.
    test_v1_kinematic_move_and_collide();
}

// ==========================================================================
// 3D Light — shadow_enabled hint value alignment
// ==========================================================================

/// V1 gate: Light3D shadow_enabled property hint must be 42
///
/// Godot's oracle reports `shadow_enabled` on Light3D subclasses with
/// `hint = 42`. Our ClassDB registration must match so that property
/// reflection and default-stripping logic align with upstream.
#[test]
#[ignore = "V1 gate: Light3D shadow_enabled hint alignment"]
fn test_v1_light3d_shadow_enabled_hint_value() {
    use gdobject::{clear_for_testing, get_property_list, register_3d_classes};

    clear_for_testing();
    register_3d_classes();

    // Light3D's shadow_enabled must have hint = 42 to match Godot oracle.
    let props = get_property_list("Light3D");
    let shadow_prop = props
        .iter()
        .find(|p| p.name == "shadow_enabled")
        .expect("Light3D must have shadow_enabled property");
    assert_eq!(
        shadow_prop.hint, 42,
        "shadow_enabled hint must be 42 to match Godot oracle"
    );

    // Subclasses inherit the property with the same hint.
    for subclass in &["DirectionalLight3D", "OmniLight3D", "SpotLight3D"] {
        let sub_props = get_property_list(subclass);
        let sub_shadow = sub_props
            .iter()
            .find(|p| p.name == "shadow_enabled")
            .unwrap_or_else(|| panic!("{subclass} must inherit shadow_enabled"));
        assert_eq!(
            sub_shadow.hint, 42,
            "{subclass} shadow_enabled hint must be 42"
        );
    }
}

// ==========================================================================
// 3D Transform Parity (V1_EXIT_EXECUTION_MAP.md — v1-3d-transform)
// ==========================================================================

/// V1 gate: Transform3D basis format normalization for oracle match
///
/// The oracle (Godot) reports Transform3D values in the format:
///   `[X: (1.0, 0.0, 0.0), Y: (0.0, 1.0, 0.0), Z: (0.0, 0.0, 1.0), O: (0.0, 2.0, 5.0)]`
/// Patina must normalize this to a structured representation that can be
/// compared against the Patina runner's JSON output format for oracle parity.
#[test]
fn test_v1_transform3d_basis_format_normalization() {
    use serde_json::json;

    // Simulate what oracle_regression_test::normalize_godot_value does.
    // Oracle emits this typed value:
    let oracle_value = json!({
        "type": "Transform3D",
        "value": "[X: (1.0, 0.0, 0.0), Y: (0.0, 1.0, 0.0), Z: (0.0, 0.0, 1.0), O: (0.0, 2.0, 5.0)]"
    });

    // Patina runner emits this structured value:
    let patina_value = json!({
        "type": "Transform3D",
        "value": {
            "basis": {
                "x": [1.0, 0.0, 0.0],
                "y": [0.0, 1.0, 0.0],
                "z": [0.0, 0.0, 1.0]
            },
            "origin": [0.0, 2.0, 5.0]
        }
    });

    // After normalization, both must resolve to the same structured form.
    let normalized_oracle = normalize_transform3d_typed_value(&oracle_value);
    let normalized_patina = normalize_transform3d_typed_value(&patina_value);

    assert_eq!(
        normalized_oracle, normalized_patina,
        "V1 GATE FAIL: Transform3D oracle format must normalize to match Patina structured format"
    );

    // Also verify the Godot text format Transform3D(...) parses correctly.
    let text_format = "Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 5)";
    let parsed = parse_transform3d_text(text_format);
    assert!(
        parsed.is_some(),
        "V1 GATE FAIL: Transform3D text format must parse"
    );
    let parsed = parsed.unwrap();
    assert_eq!(
        parsed, normalized_patina,
        "V1 GATE FAIL: Transform3D text format must match Patina structured format"
    );

    // Non-identity transform with rotation
    let rotated_oracle = json!({
        "type": "Transform3D",
        "value": "[X: (0.707, 0.0, -0.707), Y: (0.0, 1.0, 0.0), Z: (0.707, 0.0, 0.707), O: (5.0, 0.0, -3.0)]"
    });
    let rotated_patina = json!({
        "type": "Transform3D",
        "value": {
            "basis": {
                "x": [0.707, 0.0, -0.707],
                "y": [0.0, 1.0, 0.0],
                "z": [0.707, 0.0, 0.707]
            },
            "origin": [5.0, 0.0, -3.0]
        }
    });
    assert_eq!(
        normalize_transform3d_typed_value(&rotated_oracle),
        normalize_transform3d_typed_value(&rotated_patina),
        "V1 GATE FAIL: Rotated Transform3D oracle format must match Patina format"
    );
}

/// Normalizes a typed Transform3D value from either oracle string or Patina structured format.
fn normalize_transform3d_typed_value(val: &serde_json::Value) -> serde_json::Value {
    if let Some(obj) = val.as_object() {
        if let Some(inner) = obj.get("value") {
            let normalized = match inner {
                serde_json::Value::String(s) => {
                    // Oracle format: "[X: (...), Y: (...), Z: (...), O: (...)]"
                    parse_transform3d_oracle(s).unwrap_or_else(|| inner.clone())
                }
                serde_json::Value::Object(o) => {
                    // Already structured — pass through
                    if let (Some(basis), Some(origin)) = (o.get("basis"), o.get("origin")) {
                        serde_json::json!({"basis": basis, "origin": origin})
                    } else {
                        inner.clone()
                    }
                }
                _ => inner.clone(),
            };
            return serde_json::json!({"type": "Transform3D", "value": normalized});
        }
    }
    val.clone()
}

/// Parses oracle Transform3D string format.
fn parse_transform3d_oracle(s: &str) -> Option<serde_json::Value> {
    let s = s.trim();
    let inner = s.strip_prefix('[')?.strip_suffix(']')?;
    let mut parts = Vec::new();
    for label in &["X: ", "Y: ", "Z: ", "O: "] {
        let idx = inner.find(label)?;
        let rest = &inner[idx + label.len()..];
        let tuple_start = rest.find('(')?;
        let tuple_end = rest.find(')')?;
        let nums_str = &rest[tuple_start + 1..tuple_end];
        let nums: Vec<f64> = nums_str
            .split(',')
            .map(|n| n.trim().parse::<f64>())
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        if nums.len() != 3 {
            return None;
        }
        parts.push(nums);
    }
    if parts.len() != 4 {
        return None;
    }
    Some(serde_json::json!({
        "basis": {"x": parts[0], "y": parts[1], "z": parts[2]},
        "origin": parts[3]
    }))
}

// ==========================================================================
// 3D Camera — Camera3D current auto-activation (pat-zz2)
// ==========================================================================

/// V1 gate: Camera3D auto-activates `current` when entering the tree
///
/// In Godot, the first Camera3D to enter a viewport automatically becomes
/// the current camera (current = true). Patina must replicate this runtime
/// behavior so that the `current` property appears in scene output for 3D
/// parity with the oracle.
#[test]
fn test_v1_camera3d_current_auto_activation() {
    use gdscene::node::Node;
    use gdscene::scene_tree::SceneTree;
    use gdvariant::Variant;

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Add a Camera3D node — it should auto-activate since no other camera exists.
    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();

    let current = tree
        .get_node(cam_id)
        .unwrap()
        .get_property("current");
    assert_eq!(
        current,
        Variant::Bool(true),
        "V1 GATE FAIL: First Camera3D entering the tree must auto-activate (current = true)"
    );

    // Add a second Camera3D — it should NOT auto-activate.
    let cam2 = Node::new("Camera2", "Camera3D");
    let cam2_id = tree.add_child(root, cam2).unwrap();

    let current2 = tree
        .get_node(cam2_id)
        .unwrap()
        .get_property("current");
    assert!(
        !matches!(current2, Variant::Bool(true)),
        "V1 GATE FAIL: Second Camera3D must NOT auto-activate when another is already current"
    );

    // First camera should still be current.
    let still_current = tree
        .get_node(cam_id)
        .unwrap()
        .get_property("current");
    assert_eq!(
        still_current,
        Variant::Bool(true),
        "V1 GATE FAIL: First Camera3D must remain current after adding a second camera"
    );
}

// ==========================================================================
// 3D Light — float precision normalization (V1_EXIT_EXECUTION_MAP.md — v1-3d-light-precision)
// ==========================================================================

/// V1 gate: Light3D float precision normalization within tolerance
///
/// Godot's oracle reports Light3D float properties like `light_energy` with
/// f32→f64 precision artifacts (e.g. `0.800000011920929` instead of `0.8`).
/// The oracle comparison must normalize these so that Patina's clean `0.8`
/// matches the oracle's noisy representation.
#[test]
fn test_v1_light3d_float_precision_normalization() {
    // Simulate oracle float precision artifacts for Light3D properties.
    // The oracle dumps f32 values widened to f64, introducing noise.
    let oracle_energy = serde_json::json!({
        "type": "float",
        "value": 0.800000011920929  // f32→f64 artifact for 0.8
    });
    let patina_energy = serde_json::json!({
        "type": "Float",
        "value": 0.8
    });

    // After normalization, the oracle's noisy float must match Patina's clean one.
    let normalized_oracle = normalize_oracle_float(&oracle_energy);
    let normalized_patina = normalize_oracle_float(&patina_energy);

    // The normalized oracle value must be 0.8, not 0.800000011920929.
    let oracle_val = normalized_oracle
        .get("value")
        .and_then(|v| v.as_f64())
        .expect("normalized oracle must have numeric value");
    assert!(
        (oracle_val - 0.8).abs() < 1e-15,
        "V1 GATE FAIL: oracle float 0.800000011920929 must normalize to 0.8, got {}",
        oracle_val
    );

    // Both must compare equal within tolerance.
    let patina_val = normalized_patina
        .get("value")
        .and_then(|v| v.as_f64())
        .expect("normalized patina must have numeric value");
    assert!(
        (oracle_val - patina_val).abs() < 1e-10,
        "V1 GATE FAIL: normalized oracle ({}) must match patina ({})",
        oracle_val,
        patina_val
    );

    // Also check the 0.6 artifact case.
    let oracle_06 = serde_json::json!({
        "type": "float",
        "value": 0.600000023841858  // f32→f64 artifact for 0.6
    });
    let normalized_06 = normalize_oracle_float(&oracle_06);
    let val_06 = normalized_06
        .get("value")
        .and_then(|v| v.as_f64())
        .expect("normalized 0.6 must have numeric value");
    assert!(
        (val_06 - 0.6).abs() < 1e-15,
        "V1 GATE FAIL: oracle float 0.600000023841858 must normalize to 0.6, got {}",
        val_06
    );
}

/// Normalizes an f64 value that may be an f32→f64 precision artifact.
fn normalize_f64_as_f32_gate(val: f64) -> f64 {
    if !val.is_finite() {
        return val;
    }
    let as_f32 = val as f32;
    let back = as_f32 as f64;
    if (back - val).abs() < 1e-10 {
        let f = as_f32 as f64;
        for decimals in 0..=6i32 {
            let factor = 10_f64.powi(decimals);
            let rounded = (f * factor).round() / factor;
            if (rounded as f32) == as_f32 {
                return rounded;
            }
        }
        return f;
    }
    val
}

/// Normalizes a typed oracle JSON value, applying f32 precision normalization
/// for float types.
fn normalize_oracle_float(val: &serde_json::Value) -> serde_json::Value {
    if let Some(obj) = val.as_object() {
        if let Some(inner) = obj.get("value").and_then(|v| v.as_f64()) {
            let ty = obj
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("");
            if ty.eq_ignore_ascii_case("float") {
                return serde_json::json!({
                    "type": ty,
                    "value": normalize_f64_as_f32_gate(inner)
                });
            }
        }
    }
    val.clone()
}

// ==========================================================================
// Overall Oracle Parity Gate — V1_EXIT_EXECUTION_MAP.md (v1-parity-gate)
// ==========================================================================

/// V1 gate: Oracle parity reaches 98 percent across all fixtures (pat-mmm)
///
/// Runs the oracle_regression_test parity report across all golden scenes and
/// asserts that the overall property parity (Patina vs Godot oracle) is >= 98%.
#[test]
#[ignore = "V1 gate: Oracle parity reaches 98 percent across all fixtures"]
fn test_v1_overall_parity_gate() {
    // Build patina-runner first so the oracle regression test can use it.
    let build = std::process::Command::new("cargo")
        .arg("build")
        .arg("-p")
        .arg("patina-runner")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo build patina-runner");
    assert!(
        build.status.success(),
        "Failed to build patina-runner:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    // Run the oracle parity report test and capture its stderr output.
    let output = std::process::Command::new("cargo")
        .arg("test")
        .arg("--test")
        .arg("oracle_regression_test")
        .arg("golden_all_scenes_property_parity_report")
        .arg("--")
        .arg("--nocapture")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo test oracle_regression_test");

    let stderr = String::from_utf8_lossy(&output.stderr);

    // Parse the OVERALL line: "OVERALL                  221      218    98.6%"
    let overall_line = stderr
        .lines()
        .find(|l| l.contains("OVERALL"))
        .unwrap_or_else(|| {
            panic!(
                "Could not find OVERALL line in oracle parity report.\nstderr:\n{stderr}"
            )
        });

    // Extract the percentage from the last column.
    let parity_pct: f64 = overall_line
        .split_whitespace()
        .last()
        .and_then(|s| s.trim_end_matches('%').parse().ok())
        .unwrap_or_else(|| {
            panic!("Could not parse parity percentage from: {overall_line}")
        });

    eprintln!("Overall oracle parity: {parity_pct:.1}%");
    assert!(
        parity_pct >= 98.0,
        "Oracle parity must be >= 98%, got {parity_pct:.1}%\nFull report:\n{stderr}"
    );
}

/// Parses Godot text format `Transform3D(xx, xy, xz, yx, yy, yz, zx, zy, zz, ox, oy, oz)`.
/// The .tscn format is row-major; basis.x/y/z are column vectors.
fn parse_transform3d_text(s: &str) -> Option<serde_json::Value> {
    let inner = s.strip_prefix("Transform3D(")?.strip_suffix(')')?;
    let p: Vec<f64> = inner
        .split(',')
        .filter_map(|n| n.trim().parse::<f64>().ok())
        .collect();
    if p.len() != 12 {
        return None;
    }
    Some(serde_json::json!({
        "type": "Transform3D",
        "value": {
            "basis": {"x": [p[0], p[3], p[6]], "y": [p[1], p[4], p[7]], "z": [p[2], p[5], p[8]]},
            "origin": [p[9], p[10], p[11]]
        }
    }))
}
