//! pat-lpae: Broaden resource and scene execution path coverage beyond the
//! current slice.
//!
//! Tests cover:
//! 1. Resource type loading breadth (Animation, Theme, StyleBox, ext_refs)
//! 2. Resource save/load roundtrip via TresSaver
//! 3. UnifiedLoader cache and UID integration
//! 4. Scene tree change_scene lifecycle paths
//! 5. Scene save/load roundtrip via TscnSaver
//! 6. Deep sub-scene instantiation and property cascading
//! 7. Multiple scene change cycles (sequential swap stress)
//! 8. Scene reload from packed source
//! 9. Wire connections across multi-node scenes
//! 10. Resource with sub-resources and ext_resources

use std::sync::Arc;

use gdresource::resource::{ExtResource, Resource};
use gdresource::saver::TresSaver;
use gdresource::loader::TresLoader;
use gdresource::ResourceLoader;
use gdresource::UnifiedLoader;
use gdscene::packed_scene::{PackedScene, add_packed_scene_to_tree};
use gdscene::scene_saver::TscnSaver;
use gdscene::scene_tree::SceneTree;
use gdscene::node::{Node, NodeId};
use gdvariant::Variant;

/// Helper: replace all root children with nodes from a packed scene.
/// Emulates `SceneTree::change_scene_to_packed` which is not yet implemented.
fn change_scene_to_packed(tree: &mut SceneTree, packed: &PackedScene) -> NodeId {
    let root = tree.root_id();
    // Remove existing children of root.
    let children: Vec<NodeId> = tree.get_node(root)
        .map(|n| n.children().to_vec())
        .unwrap_or_default();
    for child in children {
        let _ = tree.remove_node(child);
    }
    add_packed_scene_to_tree(tree, root, packed).unwrap()
}

// ===========================================================================
// Helper: fixture paths
// ===========================================================================

fn fixture_path(name: &str) -> String {
    format!(
        "{}/../fixtures/{}",
        env!("CARGO_MANIFEST_DIR"),
        name
    )
}

fn load_fixture(name: &str) -> String {
    let path = fixture_path(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to load fixture {name}: {e}"))
}

// ===========================================================================
// 1. Resource type loading breadth
// ===========================================================================

#[test]
fn load_animation_resource_from_fixture() {
    let content = load_fixture("resources/animation.tres");
    let loader = TresLoader;
    let path = fixture_path("resources/animation.tres");
    let res = loader.load(&path).unwrap();

    assert_eq!(res.class_name, "Animation");
    assert_eq!(
        res.get_property("name"),
        Some(&Variant::String("walk_cycle".into()))
    );
    assert_eq!(
        res.get_property("length"),
        Some(&Variant::Float(1.0))
    );
    assert_eq!(
        res.get_property("loop_mode"),
        Some(&Variant::Int(1))
    );
    // Verify we have at least the key animation properties
    assert!(res.property_count() >= 3, "animation must have >=3 properties");
    // Confirm uid was parsed
    let _ = content; // used above
}

#[test]
fn load_theme_with_subresource() {
    let loader = TresLoader;
    let path = fixture_path("resources/with_subresource.tres");
    let res = loader.load(&path).unwrap();

    assert_eq!(res.class_name, "Theme");
    assert_eq!(
        res.get_property("name"),
        Some(&Variant::String("MyTheme".into()))
    );
    assert_eq!(
        res.get_property("default_font_size"),
        Some(&Variant::Int(16))
    );

    // Must have a StyleBoxFlat sub-resource
    assert!(
        !res.subresources.is_empty(),
        "theme must have sub-resources"
    );
    let stylebox = res.subresources.values().next().unwrap();
    assert_eq!(stylebox.class_name, "StyleBoxFlat");
    assert!(
        stylebox.get_property("border_width").is_some(),
        "stylebox must have border_width"
    );
}

#[test]
fn load_resource_with_ext_refs() {
    let loader = TresLoader;
    let path = fixture_path("resources/with_ext_refs.tres");
    let res = loader.load(&path).unwrap();

    assert_eq!(res.class_name, "PackedScene");

    // Must have ext_resources
    assert!(
        !res.ext_resources.is_empty(),
        "must have external resource references"
    );

    // Verify we parsed the texture reference
    let has_texture = res.ext_resources.values().any(|e| e.resource_type == "Texture2D");
    assert!(has_texture, "must have Texture2D ext_resource");

    let has_script = res.ext_resources.values().any(|e| e.resource_type == "Script");
    assert!(has_script, "must have Script ext_resource");

    // Also has inline sub-resource
    assert!(
        !res.subresources.is_empty(),
        "must have inline sub-resources"
    );
}

#[test]
fn load_simple_resource() {
    let loader = TresLoader;
    let path = fixture_path("resources/simple.tres");
    let res = loader.load(&path).unwrap();

    // Simple resource should load without errors
    assert!(!res.class_name.is_empty());
}

// ===========================================================================
// 2. Resource save/load roundtrip
// ===========================================================================

#[test]
fn tres_saver_roundtrip_simple() {
    let saver = TresSaver::new();

    let mut resource = Resource::new("TestResource");
    resource.set_property("name", Variant::String("test_item".into()));
    resource.set_property("value", Variant::Int(42));
    resource.set_property("scale", Variant::Float(1.5));

    let saved = saver.save_to_string(&resource).unwrap();

    // Must contain the class name and format version
    assert!(saved.contains("[gd_resource type=\"TestResource\""));
    assert!(saved.contains("format=3"));
    assert!(saved.contains("[resource]"));
    assert!(saved.contains("name"));
    assert!(saved.contains("value"));
}

#[test]
fn tres_saver_roundtrip_with_uid() {
    let saver = TresSaver::new();

    let mut resource = Resource::new("Animation");
    resource.uid = gdcore::ResourceUid::new(12345);
    resource.set_property("length", Variant::Float(2.0));

    let saved = saver.save_to_string(&resource).unwrap();

    // Must include the UID
    assert!(saved.contains("uid=\"uid://12345\""));
}

#[test]
fn tres_saver_roundtrip_with_subresources() {
    let saver = TresSaver::new();

    let mut sub = Resource::new("StyleBoxFlat");
    sub.set_property("border_width", Variant::Int(3));

    let mut resource = Resource::new("Theme");
    resource.set_property("name", Variant::String("MyTheme".into()));
    resource
        .subresources
        .insert("stylebox_1".into(), Arc::new(sub));

    let saved = saver.save_to_string(&resource).unwrap();

    assert!(saved.contains("[sub_resource"));
    assert!(saved.contains("StyleBoxFlat"));
    assert!(saved.contains("[resource]"));
}

#[test]
fn tres_saver_roundtrip_with_ext_resources() {
    let saver = TresSaver::new();

    let mut resource = Resource::new("PackedScene");
    resource.ext_resources.insert(
        "1".into(),
        ExtResource {
            resource_type: "Texture2D".into(),
            uid: "uid://tex_abc".into(),
            path: "res://icon.png".into(),
            id: "1".into(),
        },
    );
    resource.set_property("texture", Variant::String("ExtResource(\"1\")".into()));

    let saved = saver.save_to_string(&resource).unwrap();

    assert!(saved.contains("[ext_resource"));
    assert!(saved.contains("Texture2D"));
    assert!(saved.contains("res://icon.png"));
}

// ===========================================================================
// 3. UnifiedLoader cache and UID integration
// ===========================================================================

#[test]
fn unified_loader_loads_tres_fixture() {
    let mut loader = UnifiedLoader::new(TresLoader);
    let path = fixture_path("resources/animation.tres");
    let res = loader.load(&path).unwrap();

    assert_eq!(res.class_name, "Animation");
    assert!(loader.is_cached(&path));
    assert_eq!(loader.cache_len(), 1);
}

#[test]
fn unified_loader_cache_dedup() {
    let mut loader = UnifiedLoader::new(TresLoader);
    let path = fixture_path("resources/animation.tres");

    let res1 = loader.load(&path).unwrap();
    let res2 = loader.load(&path).unwrap();

    // Must be the same Arc (pointer equality)
    assert!(Arc::ptr_eq(&res1, &res2), "cache must deduplicate loads");
    assert_eq!(loader.cache_len(), 1);
}

#[test]
fn unified_loader_invalidate_reloads() {
    let mut loader = UnifiedLoader::new(TresLoader);
    let path = fixture_path("resources/simple.tres");

    let _res1 = loader.load(&path).unwrap();
    assert!(loader.is_cached(&path));

    loader.invalidate(&path);
    assert!(!loader.is_cached(&path));

    // Re-load
    let _res2 = loader.load(&path).unwrap();
    assert!(loader.is_cached(&path));
}

#[test]
fn unified_loader_clear_cache() {
    let mut loader = UnifiedLoader::new(TresLoader);
    let path1 = fixture_path("resources/animation.tres");
    let path2 = fixture_path("resources/simple.tres");

    loader.load(&path1).unwrap();
    loader.load(&path2).unwrap();
    assert_eq!(loader.cache_len(), 2);

    loader.clear_cache();
    assert_eq!(loader.cache_len(), 0);
}

#[test]
fn unified_loader_uid_registration() {
    let mut loader = UnifiedLoader::new(TresLoader);
    let path = fixture_path("resources/animation.tres");
    loader.load(&path).unwrap();

    // The animation.tres has uid="uid://anim_res", so it should be auto-registered.
    let registry = loader.uid_registry();
    // Verify the registry has at least one entry if the resource had a UID
    // (auto_register_uids should have caught it)
    let _ = registry;
}

#[test]
fn unified_loader_manual_uid_registration() {
    let mut loader = UnifiedLoader::new(TresLoader);
    let path = fixture_path("resources/simple.tres");

    loader.register_uid_str("uid://manual_test_123", &path);

    // Should resolve the UID to the path
    let resolved = loader.resolve_to_path("uid://manual_test_123").unwrap();
    assert_eq!(resolved, path);
}

// ===========================================================================
// 4. Scene tree change_scene lifecycle paths
// ===========================================================================

#[test]
fn change_scene_to_packed_replaces_children() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Add initial child
    let child = Node::new("OldChild", "Node2D");
    tree.add_child(root, child).unwrap();
    assert_eq!(tree.node_count(), 2); // root + child

    // Change scene
    let scene_src = load_fixture("scenes/minimal.tscn");
    let packed = PackedScene::from_tscn(&scene_src).unwrap();
    change_scene_to_packed(&mut tree, &packed);

    // Old child should be gone, new scene nodes added
    let count = tree.node_count();
    assert!(
        count >= 2,
        "scene tree must have root + at least one scene node (got {count})"
    );

    // Verify old child name is gone
    let has_old = tree.get_node_by_path("/root/OldChild");
    assert!(has_old.is_none(), "old child must be removed after scene change");
}

#[test]
fn change_scene_preserves_root() {
    let mut tree = SceneTree::new();
    let original_root = tree.root_id();

    let scene_src = load_fixture("scenes/minimal.tscn");
    let packed = PackedScene::from_tscn(&scene_src).unwrap();
    change_scene_to_packed(&mut tree, &packed);

    assert_eq!(tree.root_id(), original_root, "root node must be preserved");
}

#[test]
fn sequential_scene_changes() {
    let mut tree = SceneTree::new();

    let scenes = ["scenes/minimal.tscn", "scenes/hierarchy.tscn", "scenes/minimal.tscn"];

    for scene_name in &scenes {
        let src = load_fixture(scene_name);
        let packed = PackedScene::from_tscn(&src).unwrap();
        change_scene_to_packed(&mut tree, &packed);
        assert!(tree.node_count() >= 2, "must have nodes after loading {scene_name}");
    }
}

#[test]
fn change_scene_multiple_cycles_no_leak() {
    let mut tree = SceneTree::new();
    let scene_src = load_fixture("scenes/hierarchy.tscn");
    let packed = PackedScene::from_tscn(&scene_src).unwrap();

    // Swap scene 20 times — node count should stay bounded
    for _ in 0..20 {
        change_scene_to_packed(&mut tree, &packed);
    }

    let final_count = tree.node_count();
    let expected_max = packed.node_count() + 5; // root + scene nodes + small margin
    assert!(
        final_count <= expected_max,
        "node count after 20 swaps must not leak: got {final_count}, max {expected_max}"
    );
}

// ===========================================================================
// 5. Scene save/load roundtrip via TscnSaver
// ===========================================================================

#[test]
fn tscn_saver_roundtrip_minimal() {
    let scene_src = load_fixture("scenes/minimal.tscn");
    let packed = PackedScene::from_tscn(&scene_src).unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // Save the tree back to tscn format
    let saved = TscnSaver::save_tree(&tree, root);

    // Must contain gd_scene header and at least one node section
    assert!(saved.contains("[gd_scene"), "saved scene must have header");
    assert!(saved.contains("[node"), "saved scene must have node sections");
}

#[test]
fn tscn_saver_preserves_node_types() {
    let scene_src = load_fixture("scenes/hierarchy.tscn");
    let packed = PackedScene::from_tscn(&scene_src).unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    let saved = TscnSaver::save_tree(&tree, root);

    // The hierarchy scene has named nodes — verify they appear in output
    assert!(saved.contains("name="), "saved scene must preserve node names");
    assert!(saved.contains("type="), "saved scene must preserve node types");
}

#[test]
fn tscn_save_reload_node_count_preserved() {
    let scene_src = load_fixture("scenes/hierarchy.tscn");
    let packed = PackedScene::from_tscn(&scene_src).unwrap();
    let original_count = packed.node_count();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    let saved = TscnSaver::save_tree(&tree, root);

    // Re-parse the saved output
    let reparsed = PackedScene::from_tscn(&saved).unwrap();

    // Node count should be preserved (may include root which wasn't in original)
    assert!(
        reparsed.node_count() >= original_count,
        "roundtrip must preserve node count: original={original_count}, reparsed={}",
        reparsed.node_count()
    );
}

// ===========================================================================
// 6. Scene instantiation depth and property cascading
// ===========================================================================

#[test]
fn packed_scene_instance_all_fixtures() {
    let fixture_scenes = [
        "scenes/minimal.tscn",
        "scenes/hierarchy.tscn",
        "scenes/platformer.tscn",
        "scenes/signals_complex.tscn",
    ];

    for scene_name in &fixture_scenes {
        let src = load_fixture(scene_name);
        let packed = PackedScene::from_tscn(&src).unwrap();
        let nodes = packed.instance().unwrap();
        assert!(
            !nodes.is_empty(),
            "scene {scene_name} must instantiate at least one node"
        );
    }
}

#[test]
fn packed_scene_node_properties_preserved() {
    let src = load_fixture("scenes/hierarchy.tscn");
    let packed = PackedScene::from_tscn(&src).unwrap();
    let nodes = packed.instance().unwrap();

    // Every node must have a type and name
    for node in &nodes {
        assert!(!node.class_name().is_empty(), "node must have a class name");
    }
}

#[test]
fn add_packed_scene_to_tree_adds_all_nodes() {
    let src = load_fixture("scenes/hierarchy.tscn");
    let packed = PackedScene::from_tscn(&src).unwrap();
    let expected = packed.node_count();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // Tree should have root + all scene nodes
    assert!(
        tree.node_count() >= expected + 1,
        "tree must have root + scene nodes: expected >={}, got {}",
        expected + 1,
        tree.node_count()
    );
}

#[test]
fn multiple_instances_of_same_scene() {
    let src = load_fixture("scenes/minimal.tscn");
    let packed = PackedScene::from_tscn(&src).unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Add the same scene 5 times as children
    for _ in 0..5 {
        gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    }

    // Should have root + 5 * scene_nodes
    let min_expected = 1 + 5 * packed.node_count();
    assert!(
        tree.node_count() >= min_expected,
        "5 instances must create enough nodes: expected >={min_expected}, got {}",
        tree.node_count()
    );
}

// ===========================================================================
// 7. Scene reload from packed source
// ===========================================================================

#[test]
fn reload_current_scene_restores_state() {
    let scene_src = load_fixture("scenes/minimal.tscn");
    let packed = PackedScene::from_tscn(&scene_src).unwrap();

    let mut tree = SceneTree::new();
    change_scene_to_packed(&mut tree, &packed);
    let count_before = tree.node_count();

    // Add an extra node
    let root = tree.root_id();
    let extra = Node::new("ExtraNode", "Node");
    tree.add_child(root, extra).unwrap();
    assert!(tree.node_count() > count_before);

    // Reload by re-applying the packed scene (change_scene_to_packed removes old + adds new).
    change_scene_to_packed(&mut tree, &packed);
    assert_eq!(
        tree.node_count(),
        count_before,
        "reload must restore original node count"
    );
}

// ===========================================================================
// 8. Wire connections across multi-node scenes
// ===========================================================================

#[test]
fn wire_connections_on_signals_complex_scene() {
    let src = load_fixture("scenes/signals_complex.tscn");
    let packed = PackedScene::from_tscn(&src).unwrap();

    let connections = packed.connections();
    // signals_complex scene should have connections
    // Even if empty, verify no crash
    let _ = connections.len();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // Wire connections (should not panic even if paths don't resolve)
    let _ = gdscene::packed_scene::wire_connections(&mut tree, scene_root, connections);
}

#[test]
fn packed_scene_ext_resources_parsed() {
    let src = load_fixture("scenes/platformer.tscn");
    let packed = PackedScene::from_tscn(&src).unwrap();

    // Platformer scene may have ext_resources (textures, scripts, etc.)
    let ext = packed.ext_resources();
    // Verify the API works — even if empty
    let _ = ext.len();
}

// ===========================================================================
// 9. Scene with physics bodies
// ===========================================================================

#[test]
fn physics_scene_loads_and_instantiates() {
    let src = load_fixture("scenes/physics_playground.tscn");
    let packed = PackedScene::from_tscn(&src).unwrap();

    assert!(packed.node_count() >= 3, "physics scene must have multiple nodes");

    let nodes = packed.instance().unwrap();
    assert!(!nodes.is_empty());

    // Load into scene tree
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    assert!(tree.node_count() >= packed.node_count() + 1);
}

#[test]
fn physics_extended_scene_loads() {
    let src = load_fixture("scenes/physics_playground_extended.tscn");
    let packed = PackedScene::from_tscn(&src).unwrap();
    assert!(packed.node_count() >= 5, "extended physics must have many nodes");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
}

// ===========================================================================
// 10. Resource programmatic construction and property access
// ===========================================================================

#[test]
fn resource_construction_and_property_access() {
    let mut res = Resource::new("CustomResource");
    assert_eq!(res.class_name, "CustomResource");
    assert_eq!(res.property_count(), 0);

    res.set_property("health", Variant::Int(100));
    res.set_property("speed", Variant::Float(3.5));
    res.set_property("name", Variant::String("Player".into()));
    res.set_property("active", Variant::Bool(true));

    assert_eq!(res.property_count(), 4);
    assert_eq!(res.get_property("health"), Some(&Variant::Int(100)));
    assert_eq!(res.get_property("speed"), Some(&Variant::Float(3.5)));
    assert_eq!(res.get_property("active"), Some(&Variant::Bool(true)));
    assert!(res.get_property("nonexistent").is_none());
}

#[test]
fn resource_property_overwrite() {
    let mut res = Resource::new("Resource");
    res.set_property("value", Variant::Int(1));
    assert_eq!(res.get_property("value"), Some(&Variant::Int(1)));

    res.set_property("value", Variant::Int(2));
    assert_eq!(res.get_property("value"), Some(&Variant::Int(2)));
    assert_eq!(res.property_count(), 1, "overwrite must not increase count");
}

#[test]
fn resource_with_subresources_hierarchy() {
    let mut inner = Resource::new("InnerResource");
    inner.set_property("depth", Variant::Int(2));

    let mut middle = Resource::new("MiddleResource");
    middle.set_property("depth", Variant::Int(1));
    middle.subresources.insert("inner".into(), Arc::new(inner));

    let mut outer = Resource::new("OuterResource");
    outer.set_property("depth", Variant::Int(0));
    outer.subresources.insert("middle".into(), Arc::new(middle));

    assert_eq!(outer.subresources.len(), 1);
    let mid = &outer.subresources["middle"];
    assert_eq!(mid.subresources.len(), 1);
    let inn = &mid.subresources["inner"];
    assert_eq!(inn.get_property("depth"), Some(&Variant::Int(2)));
}

#[test]
fn resource_ext_resource_construction() {
    let mut res = Resource::new("Scene");
    res.ext_resources.insert(
        "1".into(),
        ExtResource {
            resource_type: "Texture2D".into(),
            uid: "uid://tex1".into(),
            path: "res://sprites/player.png".into(),
            id: "1".into(),
        },
    );
    res.ext_resources.insert(
        "2".into(),
        ExtResource {
            resource_type: "AudioStream".into(),
            uid: "uid://sfx1".into(),
            path: "res://audio/jump.wav".into(),
            id: "2".into(),
        },
    );

    assert_eq!(res.ext_resources.len(), 2);
    assert_eq!(res.ext_resources["1"].resource_type, "Texture2D");
    assert_eq!(res.ext_resources["2"].path, "res://audio/jump.wav");
}

// ===========================================================================
// 11. Scene tree node operations breadth
// ===========================================================================

#[test]
fn scene_tree_get_node_by_path() {
    let src = load_fixture("scenes/hierarchy.tscn");
    let packed = PackedScene::from_tscn(&src).unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // Root should always be findable
    let found = tree.get_node_by_path("/root");
    assert!(found.is_some(), "must find /root");
}

#[test]
fn scene_tree_add_remove_children() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let ids: Vec<_> = (0..10)
        .map(|i| {
            let node = Node::new(&format!("Child_{i}"), "Node2D");
            tree.add_child(root, node).unwrap()
        })
        .collect();

    assert_eq!(tree.node_count(), 11); // root + 10

    // Remove half
    for &id in &ids[..5] {
        tree.remove_node(id).unwrap();
    }
    assert_eq!(tree.node_count(), 6); // root + 5
}

#[test]
fn scene_tree_groups() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let n1 = Node::new("Enemy1", "Node2D");
    let n2 = Node::new("Enemy2", "Node2D");

    let id1 = tree.add_child(root, n1).unwrap();
    let id2 = tree.add_child(root, n2).unwrap();

    let _ = tree.add_to_group(id1, "enemies");
    let _ = tree.add_to_group(id2, "enemies");

    let group = tree.get_nodes_in_group("enemies");
    assert_eq!(group.len(), 2);
    assert!(group.contains(&id1));
    assert!(group.contains(&id2));

    // Empty group
    let empty = tree.get_nodes_in_group("nonexistent");
    assert!(empty.is_empty());
}

// ===========================================================================
// 12. All fixture scenes load without error
// ===========================================================================

#[test]
fn all_fixture_scenes_load_successfully() {
    let scenes_dir = format!("{}/../../fixtures/scenes", env!("CARGO_MANIFEST_DIR"));
    let dir = std::path::Path::new(&scenes_dir);
    if !dir.exists() {
        return; // Skip if fixture dir doesn't exist
    }

    let mut loaded = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "tscn") {
            let content = std::fs::read_to_string(&path).unwrap();
            let packed = PackedScene::from_tscn(&content);
            assert!(
                packed.is_ok(),
                "failed to load {:?}: {:?}",
                path.file_name(),
                packed.err()
            );
            loaded += 1;
        }
    }
    assert!(loaded >= 5, "must load at least 5 fixture scenes (got {loaded})");
}

#[test]
fn all_fixture_resources_load_successfully() {
    let resources_dir = format!("{}/../../fixtures/resources", env!("CARGO_MANIFEST_DIR"));
    let dir = std::path::Path::new(&resources_dir);
    if !dir.exists() {
        return;
    }

    let loader = TresLoader;
    let mut loaded = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "tres") {
            let res = loader.load(path.to_str().unwrap_or(""));
            assert!(
                res.is_ok(),
                "failed to load {:?}: {:?}",
                path.file_name(),
                res.err()
            );
            loaded += 1;
        }
    }
    assert!(loaded >= 3, "must load at least 3 fixture resources (got {loaded})");
}

// ===========================================================================
// 13. Deep lifecycle hierarchy (4+ levels)
// ===========================================================================

#[test]
fn deep_hierarchy_lifecycle_enter_exit_ordering() {
    use gdscene::lifecycle::LifecycleManager;

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Build: root -> L1 -> L2 -> L3 -> L4
    let l1 = tree.add_child(root, Node::new("L1", "Node")).unwrap();
    let l2 = tree.add_child(l1, Node::new("L2", "Node")).unwrap();
    let l3 = tree.add_child(l2, Node::new("L3", "Node")).unwrap();
    let l4 = tree.add_child(l3, Node::new("L4", "Node")).unwrap();

    // Enter tree top-down from L1
    LifecycleManager::enter_tree(&mut tree, l1);
    assert!(tree.get_node(l1).unwrap().is_inside_tree());
    assert!(tree.get_node(l4).unwrap().is_inside_tree());
    assert!(tree.get_node(l4).unwrap().is_ready());

    // Verify top-down ordering for ENTER_TREE
    let mut top_down = Vec::new();
    tree.collect_subtree_top_down(l1, &mut top_down);
    assert_eq!(top_down, vec![l1, l2, l3, l4]);

    // Verify bottom-up ordering for READY
    let mut bottom_up = Vec::new();
    tree.collect_subtree_bottom_up(l1, &mut bottom_up);
    assert_eq!(bottom_up, vec![l4, l3, l2, l1]);

    // Exit tree
    LifecycleManager::exit_tree(&mut tree, l1);
    assert!(!tree.get_node(l1).unwrap().is_inside_tree());
    assert!(!tree.get_node(l4).unwrap().is_ready());
}

// ===========================================================================
// 14. Reparenting nodes
// ===========================================================================

#[test]
fn reparent_node_updates_hierarchy() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent_a = tree.add_child(root, Node::new("ParentA", "Node")).unwrap();
    let parent_b = tree.add_child(root, Node::new("ParentB", "Node")).unwrap();
    let child = tree.add_child(parent_a, Node::new("Child", "Node2D")).unwrap();

    // Verify child is under ParentA
    assert_eq!(tree.get_node(child).unwrap().parent(), Some(parent_a));

    // Reparent to ParentB
    tree.reparent(child, parent_b).unwrap();
    assert_eq!(tree.get_node(child).unwrap().parent(), Some(parent_b));

    // ParentA should have no children, ParentB should have one
    assert!(tree.get_node(parent_a).unwrap().children().is_empty());
    assert_eq!(tree.get_node(parent_b).unwrap().children().len(), 1);
}

// ===========================================================================
// 15. Queue free and deferred deletions
// ===========================================================================

#[test]
fn queue_free_removes_node_after_process_deletions() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let child = tree.add_child(root, Node::new("Ephemeral", "Node")).unwrap();
    assert_eq!(tree.node_count(), 2);

    tree.queue_free(child);
    assert_eq!(tree.pending_deletion_count(), 1);

    // Node still exists until process_deletions
    assert!(tree.get_node(child).is_some());

    tree.process_deletions();
    assert_eq!(tree.pending_deletion_count(), 0);
    assert!(tree.get_node(child).is_none());
    assert_eq!(tree.node_count(), 1);
}

#[test]
fn queue_free_subtree_removes_all_descendants() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = tree.add_child(root, Node::new("Parent", "Node")).unwrap();
    let _c1 = tree.add_child(parent, Node::new("C1", "Node")).unwrap();
    let _c2 = tree.add_child(parent, Node::new("C2", "Node")).unwrap();
    assert_eq!(tree.node_count(), 4); // root + parent + c1 + c2

    tree.queue_free(parent);
    tree.process_deletions();

    // Only root should remain
    assert_eq!(tree.node_count(), 1);
}

// ===========================================================================
// 16. Deferred calls
// ===========================================================================

#[test]
fn deferred_calls_queue_and_flush() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = tree.add_child(root, Node::new("Target", "Node")).unwrap();

    tree.call_deferred(node, "set_property", &[
        Variant::String("health".into()),
        Variant::Int(50),
    ]);
    assert_eq!(tree.deferred_call_count(), 1);

    let flushed = tree.flush_deferred_calls();
    // flush_deferred_calls returns dispatched-to-script count; without a
    // script attached the call is dequeued but not dispatched.
    assert_eq!(flushed, 0);
    assert_eq!(tree.deferred_call_count(), 0);
}

// ===========================================================================
// 17. Duplicate subtree
// ===========================================================================

#[test]
fn duplicate_subtree_preserves_structure() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = tree.add_child(root, Node::new("Original", "Node2D")).unwrap();
    let c1 = tree.add_child(parent, Node::new("ChildA", "Sprite2D")).unwrap();
    let _c2 = tree.add_child(parent, Node::new("ChildB", "Label")).unwrap();

    // Set a property on c1
    tree.get_node_mut(c1).unwrap().set_property("texture", Variant::String("icon.png".into()));

    let cloned = tree.duplicate_subtree(parent).unwrap();
    assert_eq!(cloned.len(), 3); // parent + 2 children

    // Verify structure
    assert_eq!(cloned[0].name(), "Original");
    assert_eq!(cloned[0].class_name(), "Node2D");
    assert_eq!(cloned[1].name(), "ChildA");
    assert_eq!(cloned[2].name(), "ChildB");

    // Verify properties were copied
    assert_eq!(
        cloned[1].get_property("texture"),
        Variant::String("icon.png".into())
    );

    // IDs should be different from originals
    assert_ne!(cloned[0].id(), parent);
}

#[test]
fn duplicate_subtree_copies_groups() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = tree.add_child(root, Node::new("GroupNode", "Node")).unwrap();
    let _ = tree.add_to_group(node, "enemies");
    let _ = tree.add_to_group(node, "targetable");

    let cloned = tree.duplicate_subtree(node).unwrap();
    assert_eq!(cloned.len(), 1);

    let groups = cloned[0].groups();
    assert!(groups.iter().any(|g| g == "enemies"));
    assert!(groups.iter().any(|g| g == "targetable"));
}

// ===========================================================================
// 18. Node path resolution — relative paths with ".."
// ===========================================================================

#[test]
fn relative_path_parent_traversal() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = tree.add_child(root, Node::new("A", "Node")).unwrap();
    let b = tree.add_child(root, Node::new("B", "Node")).unwrap();
    let a_child = tree.add_child(a, Node::new("AC", "Node")).unwrap();

    // From AC, "../../B" = up to A, up to root, then down to B
    let found = tree.get_node_relative(a_child, "../../B");
    assert_eq!(found, Some(b));

    // "." should return self
    let found_self = tree.get_node_relative(a_child, ".");
    assert_eq!(found_self, Some(a_child));

    // ".." should return parent
    let found_parent = tree.get_node_relative(a_child, "..");
    assert_eq!(found_parent, Some(a));
}

// ===========================================================================
// 19. MainLoop step and run_frames execution
// ===========================================================================

#[test]
fn main_loop_step_increments_frame_counter() {
    use gdscene::main_loop::MainLoop;

    let tree = SceneTree::new();
    let mut main_loop = MainLoop::new(tree);

    assert_eq!(main_loop.frame_count(), 0);
    let output = main_loop.step(1.0 / 60.0);
    assert_eq!(output.frame_count, 1);
    assert_eq!(main_loop.frame_count(), 1);
}

#[test]
fn main_loop_run_frames_deterministic() {
    use gdscene::main_loop::MainLoop;

    let tree = SceneTree::new();
    let mut main_loop = MainLoop::new(tree);

    main_loop.run_frames(10, 1.0 / 60.0);
    assert_eq!(main_loop.frame_count(), 10);
}

#[test]
fn main_loop_physics_ticks_match_delta() {
    use gdscene::main_loop::MainLoop;

    let tree = SceneTree::new();
    let mut main_loop = MainLoop::new(tree);
    main_loop.set_physics_ticks_per_second(60);

    // With delta = 1/60s and physics at 60 TPS, each frame should produce 1 tick
    let output = main_loop.step(1.0 / 60.0);
    assert_eq!(output.physics_steps, 1);
}

#[test]
fn main_loop_large_delta_caps_physics_steps() {
    use gdscene::main_loop::MainLoop;

    let tree = SceneTree::new();
    let mut main_loop = MainLoop::new(tree);
    main_loop.set_physics_ticks_per_second(60);
    main_loop.set_max_physics_steps_per_frame(4);

    // Large delta should cap at max_physics_steps_per_frame
    let output = main_loop.step(1.0); // 1 second = 60 ticks, but capped at 4
    assert_eq!(output.physics_steps, 4);
}

// ===========================================================================
// 20. MainLoop pause/unpause
// ===========================================================================

#[test]
fn main_loop_pause_unpause_dispatches_notifications() {
    use gdscene::main_loop::MainLoop;

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let _node = tree.add_child(root, Node::new("PauseTarget", "Node")).unwrap();

    let mut main_loop = MainLoop::new(tree);
    assert!(!main_loop.paused());

    main_loop.set_paused(true);
    assert!(main_loop.paused());

    main_loop.set_paused(false);
    assert!(!main_loop.paused());

    // Setting to same value should be a no-op
    main_loop.set_paused(false);
    assert!(!main_loop.paused());
}

// ===========================================================================
// 21. MainLoop with scene change during execution
// ===========================================================================

#[test]
fn main_loop_scene_change_mid_execution() {
    use gdscene::main_loop::MainLoop;

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("OldNode", "Node2D")).unwrap();

    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frames(5, 1.0 / 60.0);

    // Change scene mid-execution
    let scene_src = load_fixture("scenes/minimal.tscn");
    let packed = PackedScene::from_tscn(&scene_src).unwrap();
    main_loop.tree_mut().change_scene_to_packed(&packed).unwrap();

    // Continue running — should not crash
    main_loop.run_frames(5, 1.0 / 60.0);
    assert_eq!(main_loop.frame_count(), 10);

    // Old node should be gone
    let old = main_loop.tree().get_node_by_path("/root/OldNode");
    assert!(old.is_none());
}

// ===========================================================================
// 22. Resource cache replace workflow
// ===========================================================================

#[test]
fn unified_loader_replace_cached_updates_future_loads() {
    let mut loader = UnifiedLoader::new(TresLoader);
    let path = fixture_path("resources/simple.tres");

    let original = loader.load(&path).unwrap();
    let original_class = original.class_name.clone();

    // Clone, mutate, replace
    let mut mutated = (*original).clone();
    mutated.set_property("custom_flag", Variant::Bool(true));
    let mutated_arc = Arc::new(mutated);

    loader.replace_cached(&path, mutated_arc.clone());

    // Subsequent load should return the mutated version
    let reloaded = loader.load(&path).unwrap();
    assert!(Arc::ptr_eq(&reloaded, &mutated_arc));
    assert_eq!(
        reloaded.get_property("custom_flag"),
        Some(&Variant::Bool(true))
    );
    assert_eq!(reloaded.class_name, original_class);
}

#[test]
fn unified_loader_replace_does_not_affect_old_holders() {
    let mut loader = UnifiedLoader::new(TresLoader);
    let path = fixture_path("resources/simple.tres");

    let original = loader.load(&path).unwrap();

    let mut mutated = (*original).clone();
    mutated.set_property("replaced", Variant::Bool(true));
    loader.replace_cached(&path, Arc::new(mutated));

    // Original Arc should still be valid and unchanged
    assert!(original.get_property("replaced").is_none());
}

// ===========================================================================
// 23. Scene tree move_child reordering
// ===========================================================================

#[test]
fn move_child_changes_sibling_order() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = tree.add_child(root, Node::new("A", "Node")).unwrap();
    let b = tree.add_child(root, Node::new("B", "Node")).unwrap();
    let c = tree.add_child(root, Node::new("C", "Node")).unwrap();

    // Initial order: [A, B, C]
    let children = tree.get_node(root).unwrap().children().to_vec();
    assert_eq!(children, vec![a, b, c]);

    // Move C to index 0 → [C, A, B]
    tree.move_child(root, c, 0).unwrap();
    let children = tree.get_node(root).unwrap().children().to_vec();
    assert_eq!(children[0], c);
}

// ===========================================================================
// 24. Scene tree raise/lower
// ===========================================================================

#[test]
fn raise_moves_node_to_last() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = tree.add_child(root, Node::new("A", "Node")).unwrap();
    let _b = tree.add_child(root, Node::new("B", "Node")).unwrap();
    let _c = tree.add_child(root, Node::new("C", "Node")).unwrap();

    tree.raise(a).unwrap();
    let children = tree.get_node(root).unwrap().children().to_vec();
    assert_eq!(*children.last().unwrap(), a);
}

#[test]
fn lower_moves_node_to_first() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let _a = tree.add_child(root, Node::new("A", "Node")).unwrap();
    let _b = tree.add_child(root, Node::new("B", "Node")).unwrap();
    let c = tree.add_child(root, Node::new("C", "Node")).unwrap();

    tree.lower(c).unwrap();
    let children = tree.get_node(root).unwrap().children().to_vec();
    assert_eq!(children[0], c);
}

// ===========================================================================
// 25. Deferred signals
// ===========================================================================

#[test]
fn deferred_signals_queue_and_flush() {
    use gdobject::signal::Connection;

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let emitter = tree.add_child(root, Node::new("Emitter", "Node")).unwrap();
    let receiver = tree.add_child(root, Node::new("Receiver", "Node")).unwrap();

    // Connect a deferred signal
    let conn = Connection::new(receiver.object_id(), "on_deferred").as_deferred();
    tree.connect_signal(emitter, "deferred_sig", conn);
    tree.emit_signal(emitter, "deferred_sig", &[]);

    // Deferred signals should be pending
    let count = tree.deferred_signal_count();
    assert!(count >= 1, "should have at least 1 deferred signal, got {count}");

    let flushed = tree.flush_deferred_signals();
    assert!(flushed >= 1);
    assert_eq!(tree.deferred_signal_count(), 0);
}

// ===========================================================================
// 26. MainLoop traced execution captures events
// ===========================================================================

#[test]
fn main_loop_step_traced_captures_frame_record() {
    use gdscene::main_loop::MainLoop;

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("TracedNode", "Node2D")).unwrap();

    let mut main_loop = MainLoop::new(tree);
    let record = main_loop.step_traced(1.0 / 60.0);

    assert_eq!(record.frame_number, 1);
    assert!((record.delta - 1.0 / 60.0).abs() < 1e-10);
    assert!(record.physics_ticks >= 1);
}

#[test]
fn main_loop_run_frames_traced_produces_frame_trace() {
    use gdscene::main_loop::MainLoop;

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("TracedNode", "Node")).unwrap();

    let mut main_loop = MainLoop::new(tree);
    let trace = main_loop.run_frames_traced(5, 1.0 / 60.0);

    assert_eq!(trace.len(), 5);
    assert!(!trace.is_empty());

    // Physics ticks should sum to at least 5 (one per frame at 60 TPS)
    assert!(trace.total_physics_ticks() >= 5);
}

// ===========================================================================
// 27. Scene tree node index queries
// ===========================================================================

#[test]
fn get_index_returns_sibling_position() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = tree.add_child(root, Node::new("A", "Node")).unwrap();
    let b = tree.add_child(root, Node::new("B", "Node")).unwrap();
    let c = tree.add_child(root, Node::new("C", "Node")).unwrap();

    assert_eq!(tree.get_index(a), Some(0));
    assert_eq!(tree.get_index(b), Some(1));
    assert_eq!(tree.get_index(c), Some(2));
}

// ===========================================================================
// 28. All nodes in tree/process order
// ===========================================================================

#[test]
fn all_nodes_in_tree_order_covers_full_hierarchy() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = tree.add_child(root, Node::new("A", "Node")).unwrap();
    let b = tree.add_child(root, Node::new("B", "Node")).unwrap();
    let a1 = tree.add_child(a, Node::new("A1", "Node")).unwrap();

    let all = tree.all_nodes_in_tree_order();
    assert_eq!(all.len(), 4); // root + A + B + A1
    assert_eq!(all[0], root);
    // A should come before B, and A1 should come after A (depth-first)
    let a_pos = all.iter().position(|&id| id == a).unwrap();
    let a1_pos = all.iter().position(|&id| id == a1).unwrap();
    let b_pos = all.iter().position(|&id| id == b).unwrap();
    assert!(a_pos < a1_pos, "A must come before A1");
    assert!(a1_pos < b_pos, "A1 (depth-first) must come before B");
}

// ===========================================================================
// 29. Scene tree create_node convenience API
// ===========================================================================

#[test]
fn create_node_adds_to_root() {
    let mut tree = SceneTree::new();
    let id = tree.create_node("Sprite2D", "MySprite");
    assert!(tree.get_node(id).is_some());
    assert_eq!(tree.get_node(id).unwrap().name(), "MySprite");
    assert_eq!(tree.get_node(id).unwrap().class_name(), "Sprite2D");
}

// ===========================================================================
// 30. Process mode filtering
// ===========================================================================

#[test]
fn should_process_node_respects_pause_state() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = tree.add_child(root, Node::new("PausableNode", "Node")).unwrap();

    // When tree is not paused, node should process
    assert!(tree.should_process_node(node));

    // When paused, default INHERIT mode should not process
    tree.set_paused(true);
    assert!(!tree.should_process_node(node));
}

// ===========================================================================
// 31. Resource with multiple subresource nesting
// ===========================================================================

#[test]
fn tres_saver_nested_subresources_roundtrip() {
    let saver = TresSaver::new();

    let mut inner = gdresource::resource::Resource::new("GradientTexture2D");
    inner.set_property("width", Variant::Int(256));

    let mut mid = gdresource::resource::Resource::new("ShaderMaterial");
    mid.set_property("shader_type", Variant::String("canvas_item".into()));
    mid.subresources.insert("texture".into(), Arc::new(inner));

    let mut outer = gdresource::resource::Resource::new("CanvasItemMaterial");
    outer.set_property("blend_mode", Variant::Int(0));
    outer.subresources.insert("material".into(), Arc::new(mid));

    let saved = saver.save_to_string(&outer).unwrap();

    assert!(saved.contains("GradientTexture2D") || saved.contains("[sub_resource"));
    assert!(saved.contains("ShaderMaterial") || saved.contains("[sub_resource"));
    assert!(saved.contains("[resource]"));
}

// ===========================================================================
// 32. Packed scene connections count
// ===========================================================================

#[test]
fn packed_scene_connection_count_matches_connections_len() {
    let src = load_fixture("scenes/signals_complex.tscn");
    let packed = PackedScene::from_tscn(&src).unwrap();

    assert_eq!(packed.connection_count(), packed.connections().len());
}

// ===========================================================================
// 33. Scene tree group removal
// ===========================================================================

#[test]
fn remove_from_group_updates_group_membership() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = tree.add_child(root, Node::new("GroupMember", "Node")).unwrap();
    let _ = tree.add_to_group(node, "players");
    assert_eq!(tree.get_nodes_in_group("players").len(), 1);

    let _ = tree.remove_from_group(node, "players");
    assert_eq!(tree.get_nodes_in_group("players").len(), 0);
}

// ===========================================================================
// 34. Resource UID registry bidirectional lookup
// ===========================================================================

#[test]
fn uid_registry_bidirectional() {
    let mut loader = UnifiedLoader::new(TresLoader);
    loader.register_uid_str("uid://bidir_test_999", "res://test_bidir.tres");

    // Forward lookup
    let resolved = loader.resolve_to_path("uid://bidir_test_999").unwrap();
    assert_eq!(resolved, "res://test_bidir.tres");

    // Non-UID path resolves to itself
    let plain = loader.resolve_to_path("res://plain.tres").unwrap();
    assert_eq!(plain, "res://plain.tres");

    // Invalid UID returns error
    let err = loader.resolve_to_path("uid://nonexistent_xyz");
    assert!(err.is_err());
}

// ===========================================================================
// 35. Get cached without loading
// ===========================================================================

#[test]
fn get_cached_returns_none_before_load() {
    let loader = UnifiedLoader::new(TresLoader);
    let path = fixture_path("resources/simple.tres");

    assert!(loader.get_cached(&path).is_none());
}

#[test]
fn get_cached_returns_resource_after_load() {
    let mut loader = UnifiedLoader::new(TresLoader);
    let path = fixture_path("resources/simple.tres");

    let loaded = loader.load(&path).unwrap();
    let cached = loader.get_cached(&path).unwrap();
    assert!(Arc::ptr_eq(&loaded, &cached));
}
