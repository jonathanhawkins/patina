//! Integration tests for ReflectionProbe node — local cubemap reflections.
//!
//! Validates:
//! - ClassDB registration with Godot-matching defaults
//! - Inheritance chain (ReflectionProbe → Node3D → Node → Object)
//! - Property round-trips via node3d helpers
//! - Render adapter syncs probe nodes and removes stale probes
//! - Scene tree lifecycle (add/remove)

use gdcore::math::{Color, Vector3};
use gdobject::class_db;
use gdscene::node::Node;
use gdscene::node3d;
use gdscene::render_server_3d::RenderServer3DAdapter;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

fn setup_classdb() {
    class_db::clear_for_testing();
    class_db::register_class(class_db::ClassRegistration::new("Object"));
    class_db::register_class(class_db::ClassRegistration::new("Node").parent("Object"));
    class_db::register_3d_classes();
}

// ---------------------------------------------------------------------------
// ClassDB registration
// ---------------------------------------------------------------------------

#[test]
fn reflection_probe_registered_in_classdb() {
    setup_classdb();
    assert!(
        class_db::class_exists("ReflectionProbe"),
        "ReflectionProbe must be registered"
    );
}

#[test]
fn reflection_probe_inherits_node3d() {
    setup_classdb();
    assert!(
        class_db::is_parent_class("ReflectionProbe", "Node3D"),
        "ReflectionProbe must inherit Node3D"
    );
}

#[test]
fn reflection_probe_default_properties_match_godot() {
    setup_classdb();
    let obj = class_db::instantiate("ReflectionProbe").expect("should instantiate");

    // Godot defaults
    assert_eq!(
        obj.get_property("size"),
        Variant::Vector3(Vector3::new(20.0, 20.0, 20.0)),
        "default size should be (20, 20, 20)"
    );
    assert_eq!(
        obj.get_property("origin_offset"),
        Variant::Vector3(Vector3::ZERO)
    );
    assert_eq!(obj.get_property("box_projection"), Variant::Bool(false));
    assert_eq!(obj.get_property("interior"), Variant::Bool(false));
    assert_eq!(obj.get_property("enable_shadows"), Variant::Bool(false));
    assert_eq!(obj.get_property("max_distance"), Variant::Float(0.0));
    assert_eq!(obj.get_property("intensity"), Variant::Float(1.0));
    assert_eq!(obj.get_property("update_mode"), Variant::Int(0)); // Once
    assert_eq!(obj.get_property("ambient_mode"), Variant::Int(1)); // Environment
    assert_eq!(obj.get_property("ambient_color"), Variant::Color(Color::BLACK));
    assert_eq!(obj.get_property("ambient_color_energy"), Variant::Float(1.0));
    assert_eq!(obj.get_property("cull_mask"), Variant::Int(0xFFFFF));
    assert_eq!(obj.get_property("mesh_lod_threshold"), Variant::Float(1.0));
}

#[test]
fn reflection_probe_has_all_expected_properties() {
    setup_classdb();
    let props = class_db::get_property_list("ReflectionProbe", true);
    let names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    for expected in &[
        "size",
        "origin_offset",
        "box_projection",
        "interior",
        "enable_shadows",
        "max_distance",
        "intensity",
        "update_mode",
        "ambient_mode",
        "ambient_color",
        "ambient_color_energy",
        "cull_mask",
        "mesh_lod_threshold",
    ] {
        assert!(
            names.contains(expected),
            "missing property: {expected}"
        );
    }
}

// ---------------------------------------------------------------------------
// Node3d property helpers
// ---------------------------------------------------------------------------

#[test]
fn probe_size_roundtrip() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let probe = Node::new("Probe", "ReflectionProbe");
    let id = tree.add_child(root, probe).unwrap();

    // Default
    assert_eq!(
        node3d::get_probe_size(&tree, id),
        Vector3::new(20.0, 20.0, 20.0)
    );

    node3d::set_probe_size(&mut tree, id, Vector3::new(10.0, 5.0, 8.0));
    assert_eq!(
        node3d::get_probe_size(&tree, id),
        Vector3::new(10.0, 5.0, 8.0)
    );
}

#[test]
fn probe_origin_offset_roundtrip() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let probe = Node::new("Probe", "ReflectionProbe");
    let id = tree.add_child(root, probe).unwrap();

    assert_eq!(node3d::get_probe_origin_offset(&tree, id), Vector3::ZERO);

    node3d::set_probe_origin_offset(&mut tree, id, Vector3::new(1.0, 2.0, 3.0));
    assert_eq!(
        node3d::get_probe_origin_offset(&tree, id),
        Vector3::new(1.0, 2.0, 3.0)
    );
}

#[test]
fn probe_intensity_roundtrip() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let probe = Node::new("Probe", "ReflectionProbe");
    let id = tree.add_child(root, probe).unwrap();

    assert!((node3d::get_probe_intensity(&tree, id) - 1.0).abs() < 1e-6);
    node3d::set_probe_intensity(&mut tree, id, 2.5);
    assert!((node3d::get_probe_intensity(&tree, id) - 2.5).abs() < 1e-6);
}

#[test]
fn probe_boolean_properties() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let probe = Node::new("Probe", "ReflectionProbe");
    let id = tree.add_child(root, probe).unwrap();

    node3d::set_probe_box_projection(&mut tree, id, true);
    node3d::set_probe_interior(&mut tree, id, true);
    node3d::set_probe_enable_shadows(&mut tree, id, true);

    let n = tree.get_node(id).unwrap();
    assert_eq!(n.get_property("box_projection"), Variant::Bool(true));
    assert_eq!(n.get_property("interior"), Variant::Bool(true));
    assert_eq!(n.get_property("enable_shadows"), Variant::Bool(true));
}

#[test]
fn probe_enum_properties() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let probe = Node::new("Probe", "ReflectionProbe");
    let id = tree.add_child(root, probe).unwrap();

    node3d::set_probe_update_mode(&mut tree, id, 1); // Always
    node3d::set_probe_ambient_mode(&mut tree, id, 2); // ConstantColor

    let n = tree.get_node(id).unwrap();
    assert_eq!(n.get_property("update_mode"), Variant::Int(1));
    assert_eq!(n.get_property("ambient_mode"), Variant::Int(2));
}

#[test]
fn probe_ambient_color() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let probe = Node::new("Probe", "ReflectionProbe");
    let id = tree.add_child(root, probe).unwrap();

    let red = Color::new(1.0, 0.0, 0.0, 1.0);
    node3d::set_probe_ambient_color(&mut tree, id, red);
    let n = tree.get_node(id).unwrap();
    assert_eq!(n.get_property("ambient_color"), Variant::Color(red));
}

// ---------------------------------------------------------------------------
// Render adapter sync
// ---------------------------------------------------------------------------

#[test]
fn render_adapter_syncs_reflection_probe() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Camera so render_frame doesn't use the default.
    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));

    // Add a ReflectionProbe.
    let probe = Node::new("Probe", "ReflectionProbe");
    tree.add_child(root, probe).unwrap();

    let mut adapter = RenderServer3DAdapter::new(32, 32);
    let (snapshot, _) = adapter.render_frame(&tree);

    // The probe shouldn't affect mesh/light counts but should be tracked.
    assert_eq!(snapshot.visible_mesh_count, 0);
    let debug_str = format!("{:?}", adapter);
    assert!(
        debug_str.contains("tracked_probes: 1"),
        "adapter should track 1 probe, debug: {debug_str}"
    );
}

#[test]
fn render_adapter_removes_stale_probe() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let probe = Node::new("Probe", "ReflectionProbe");
    let probe_id = tree.add_child(root, probe).unwrap();

    let mut adapter = RenderServer3DAdapter::new(16, 16);
    adapter.render_frame(&tree);

    let debug1 = format!("{:?}", adapter);
    assert!(debug1.contains("tracked_probes: 1"));

    // Remove the probe node.
    tree.remove_node(probe_id).unwrap();
    adapter.render_frame(&tree);

    let debug2 = format!("{:?}", adapter);
    assert!(
        debug2.contains("tracked_probes: 0"),
        "stale probe should be removed, debug: {debug2}"
    );
}

#[test]
fn multiple_reflection_probes_tracked() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    for i in 0..3 {
        let probe = Node::new(&format!("Probe{i}"), "ReflectionProbe");
        let pid = tree.add_child(root, probe).unwrap();
        node3d::set_position(
            &mut tree,
            pid,
            Vector3::new(i as f32 * 10.0, 0.0, 0.0),
        );
    }

    let mut adapter = RenderServer3DAdapter::new(16, 16);
    adapter.render_frame(&tree);

    let debug = format!("{:?}", adapter);
    assert!(
        debug.contains("tracked_probes: 3"),
        "should track 3 probes, debug: {debug}"
    );
}

// ---------------------------------------------------------------------------
// ReflectionProbe data type (gdserver3d)
// ---------------------------------------------------------------------------

#[test]
fn reflection_probe_type_defaults() {
    use gdserver3d::reflection_probe::*;

    let probe = ReflectionProbe::new(ReflectionProbeId(42));
    assert_eq!(probe.id, ReflectionProbeId(42));
    assert_eq!(probe.size, Vector3::new(20.0, 20.0, 20.0));
    assert!(!probe.box_projection);
    assert!(!probe.interior);
    assert!(!probe.enable_shadows);
    assert!((probe.intensity - 1.0).abs() < f32::EPSILON);
    assert_eq!(probe.update_mode, ReflectionProbeUpdateMode::Once);
    assert_eq!(probe.ambient_mode, ReflectionProbeAmbientMode::Environment);
}

#[test]
fn reflection_probe_contains_point_logic() {
    use gdserver3d::reflection_probe::*;

    let mut probe = ReflectionProbe::new(ReflectionProbeId(1));
    probe.transform.origin = Vector3::new(50.0, 0.0, 0.0);

    // Half-extent = 10, center at (50,0,0), so X range is 40..60
    assert!(probe.contains_point(Vector3::new(50.0, 0.0, 0.0)));
    assert!(probe.contains_point(Vector3::new(40.0, 0.0, 0.0)));
    assert!(!probe.contains_point(Vector3::new(39.9, 0.0, 0.0)));
}

// ---------------------------------------------------------------------------
// Node3D transform integration with ReflectionProbe
// ---------------------------------------------------------------------------

#[test]
fn probe_inherits_node3d_transform() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node3D");
    let parent_id = tree.add_child(root, parent).unwrap();
    node3d::set_position(&mut tree, parent_id, Vector3::new(100.0, 0.0, 0.0));

    let probe = Node::new("Probe", "ReflectionProbe");
    let probe_id = tree.add_child(parent_id, probe).unwrap();
    node3d::set_position(&mut tree, probe_id, Vector3::new(5.0, 0.0, 0.0));

    let global = node3d::get_global_transform(&tree, probe_id);
    let world_pos = global.xform(Vector3::ZERO);

    assert!(
        (world_pos.x - 105.0).abs() < 1e-3,
        "probe global x should be 105, got {}",
        world_pos.x
    );
}
