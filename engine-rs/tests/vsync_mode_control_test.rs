//! Integration tests for Vsync mode control (enabled, disabled, adaptive, mailbox).
//!
//! Covers ClassDB registration, VsyncMode enum, DisplayServer vsync get/set,
//! int conversion roundtrip, wgpu present mode mapping, and behavior properties.

use gdobject::class_db;
use gdplatform::DisplayServer;
use gdplatform::VsyncMode;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;

// ── ClassDB Registration ─────────────────────────────────────────────────────

#[test]
fn classdb_registers_display_server() {
    class_db::register_3d_classes();
    assert!(class_db::class_exists("DisplayServer"));
}

#[test]
fn classdb_display_server_inherits_object() {
    class_db::register_3d_classes();
    let info = class_db::get_class_info("DisplayServer").unwrap();
    assert_eq!(info.parent_class.as_str(), "Object");
}

#[test]
fn classdb_display_server_has_vsync_property() {
    class_db::register_3d_classes();
    let props = class_db::get_property_list("DisplayServer", false);
    let names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"vsync_mode"));
}

#[test]
fn classdb_display_server_has_vsync_methods() {
    class_db::register_3d_classes();
    assert!(class_db::class_has_method("DisplayServer", "window_get_vsync_mode"));
    assert!(class_db::class_has_method("DisplayServer", "window_set_vsync_mode"));
}

#[test]
fn classdb_display_server_has_screen_methods() {
    class_db::register_3d_classes();
    assert!(class_db::class_has_method("DisplayServer", "screen_get_size"));
    assert!(class_db::class_has_method("DisplayServer", "screen_get_dpi"));
    assert!(class_db::class_has_method("DisplayServer", "screen_get_refresh_rate"));
    assert!(class_db::class_has_method("DisplayServer", "screen_get_scale"));
}

// ── VsyncMode Enum ───────────────────────────────────────────────────────────

#[test]
fn vsync_mode_default_is_enabled() {
    assert_eq!(VsyncMode::default(), VsyncMode::Enabled);
}

#[test]
fn vsync_mode_all_variants_exist() {
    let all = VsyncMode::all();
    assert_eq!(all.len(), 4);
    assert!(all.contains(&VsyncMode::Disabled));
    assert!(all.contains(&VsyncMode::Enabled));
    assert!(all.contains(&VsyncMode::Adaptive));
    assert!(all.contains(&VsyncMode::Mailbox));
}

#[test]
fn vsync_mode_int_roundtrip_disabled() {
    assert_eq!(VsyncMode::from_godot_int(VsyncMode::Disabled.to_godot_int()), VsyncMode::Disabled);
}

#[test]
fn vsync_mode_int_roundtrip_enabled() {
    assert_eq!(VsyncMode::from_godot_int(VsyncMode::Enabled.to_godot_int()), VsyncMode::Enabled);
}

#[test]
fn vsync_mode_int_roundtrip_adaptive() {
    assert_eq!(VsyncMode::from_godot_int(VsyncMode::Adaptive.to_godot_int()), VsyncMode::Adaptive);
}

#[test]
fn vsync_mode_int_roundtrip_mailbox() {
    assert_eq!(VsyncMode::from_godot_int(VsyncMode::Mailbox.to_godot_int()), VsyncMode::Mailbox);
}

#[test]
fn vsync_mode_godot_int_values() {
    assert_eq!(VsyncMode::Disabled.to_godot_int(), 0);
    assert_eq!(VsyncMode::Enabled.to_godot_int(), 1);
    assert_eq!(VsyncMode::Adaptive.to_godot_int(), 2);
    assert_eq!(VsyncMode::Mailbox.to_godot_int(), 3);
}

#[test]
fn vsync_mode_invalid_int_defaults_to_enabled() {
    assert_eq!(VsyncMode::from_godot_int(99), VsyncMode::Enabled);
    assert_eq!(VsyncMode::from_godot_int(-1), VsyncMode::Enabled);
}

// ── wgpu PresentMode Mapping ─────────────────────────────────────────────────

#[test]
fn vsync_wgpu_present_mode_disabled() {
    assert_eq!(VsyncMode::Disabled.wgpu_present_mode_name(), "Immediate");
}

#[test]
fn vsync_wgpu_present_mode_enabled() {
    assert_eq!(VsyncMode::Enabled.wgpu_present_mode_name(), "Fifo");
}

#[test]
fn vsync_wgpu_present_mode_adaptive() {
    assert_eq!(VsyncMode::Adaptive.wgpu_present_mode_name(), "FifoRelaxed");
}

#[test]
fn vsync_wgpu_present_mode_mailbox() {
    assert_eq!(VsyncMode::Mailbox.wgpu_present_mode_name(), "Mailbox");
}

// ── Behavior Properties ──────────────────────────────────────────────────────

#[test]
fn vsync_prevents_tearing() {
    assert!(!VsyncMode::Disabled.prevents_tearing());
    assert!(VsyncMode::Enabled.prevents_tearing());
    assert!(!VsyncMode::Adaptive.prevents_tearing());
    assert!(VsyncMode::Mailbox.prevents_tearing());
}

#[test]
fn vsync_caps_framerate() {
    assert!(!VsyncMode::Disabled.caps_framerate());
    assert!(VsyncMode::Enabled.caps_framerate());
    assert!(VsyncMode::Adaptive.caps_framerate());
    assert!(!VsyncMode::Mailbox.caps_framerate());
}

// ── Display Names ────────────────────────────────────────────────────────────

#[test]
fn vsync_display_names() {
    assert_eq!(VsyncMode::Disabled.display_name(), "Disabled");
    assert_eq!(VsyncMode::Enabled.display_name(), "Enabled");
    assert_eq!(VsyncMode::Adaptive.display_name(), "Adaptive");
    assert_eq!(VsyncMode::Mailbox.display_name(), "Mailbox");
}

// ── DisplayServer Integration ────────────────────────────────────────────────

#[test]
fn display_server_default_vsync_enabled() {
    let ds = DisplayServer::new();
    assert_eq!(ds.vsync(), VsyncMode::Enabled);
}

#[test]
fn display_server_set_vsync_disabled() {
    let mut ds = DisplayServer::new();
    ds.set_vsync(VsyncMode::Disabled);
    assert_eq!(ds.vsync(), VsyncMode::Disabled);
}

#[test]
fn display_server_set_vsync_adaptive() {
    let mut ds = DisplayServer::new();
    ds.set_vsync(VsyncMode::Adaptive);
    assert_eq!(ds.vsync(), VsyncMode::Adaptive);
}

#[test]
fn display_server_set_vsync_mailbox() {
    let mut ds = DisplayServer::new();
    ds.set_vsync(VsyncMode::Mailbox);
    assert_eq!(ds.vsync(), VsyncMode::Mailbox);
}

#[test]
fn display_server_vsync_cycle_all_modes() {
    let mut ds = DisplayServer::new();
    for mode in VsyncMode::all() {
        ds.set_vsync(*mode);
        assert_eq!(ds.vsync(), *mode, "failed for mode {:?}", mode);
    }
}

// ── SceneTree ────────────────────────────────────────────────────────────────

#[test]
fn scene_tree_display_server_node() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("Display", "DisplayServer");
    tree.add_child(root, node).unwrap();
    assert_eq!(tree.node_count(), 2);
}
