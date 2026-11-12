//! SubViewport + ViewportTexture sampling path (pat-zx775).
//!
//! Observable behavior checked:
//! 1. ClassDB recognizes SubViewport and ViewportTexture with Godot inheritance
//!    and size/update_mode defaults.
//! 2. A scene with a SubViewport can allocate a software color buffer and
//!    software-render a ColorRect-style solid fill so ViewportTexture sampling
//!    yields nonzero pixels.
//! 3. MeshInstance-style marker fill also produces nonzero sampled pixels.

use std::sync::Mutex;

use gdcore::math::{Color, Vector2i};
use gdobject::class_db::{
    class_exists, clear_for_testing, get_class_info, inheritance_chain, instantiate,
    is_parent_class, register_2d_classes, register_viewport_classes,
};
use gdobject::object::GodotObject;
use gdscene::node::Node;
use gdscene::sub_viewport::{
    apply_subviewport_defaults, get_size, get_update_mode, render_subviewport_to_texture, set_size,
    SubViewport, UpdateMode, ViewportTexture,
};
use gdscene::SceneTree;
use gdvariant::Variant;

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn setup_classdb() -> std::sync::MutexGuard<'static, ()> {
    let guard = TEST_LOCK.lock().expect("test lock poisoned");
    clear_for_testing();
    register_viewport_classes();
    guard
}

#[test]
fn classdb_subviewport_viewport_texture_registered() {
    let _g = setup_classdb();

    assert!(class_exists("SubViewport"));
    assert!(class_exists("ViewportTexture"));
    assert!(class_exists("Viewport"));
    assert!(class_exists("Texture2D"));

    assert!(is_parent_class("SubViewport", "Viewport"));
    assert!(is_parent_class("SubViewport", "Node"));
    assert!(is_parent_class("ViewportTexture", "Texture2D"));

    let chain = inheritance_chain("SubViewport");
    assert!(chain.iter().any(|c| c == "Viewport"), "chain={chain:?}");
    assert!(chain.iter().any(|c| c == "Node"));

    let info = get_class_info("SubViewport").expect("SubViewport ClassInfo");
    let names: Vec<&str> = info.properties.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"size"));
    assert!(names.contains(&"render_target_update_mode"));
    assert!(names.contains(&"render_target_clear_mode"));
    assert!(names.contains(&"transparent_bg"));

    let size = info
        .properties
        .iter()
        .find(|p| p.name == "size")
        .map(|p| &p.default_value)
        .unwrap();
    match size {
        Variant::Vector2(v) => {
            assert!((v.x - 512.0).abs() < f32::EPSILON);
            assert!((v.y - 512.0).abs() < f32::EPSILON);
        }
        other => panic!("expected Vector2 size default, got {other:?}"),
    }
    let update = info
        .properties
        .iter()
        .find(|p| p.name == "render_target_update_mode")
        .map(|p| &p.default_value)
        .unwrap();
    assert_eq!(*update, Variant::Int(2)); // UPDATE_WHEN_VISIBLE

    let obj = instantiate("SubViewport").expect("instantiate SubViewport");
    assert_eq!(
        obj.get_property("render_target_update_mode"),
        Variant::Int(2)
    );
    assert_eq!(
        obj.get_property("render_target_clear_mode"),
        Variant::Int(0)
    );
}

#[test]
fn register_2d_classes_includes_subviewport() {
    let _g = TEST_LOCK.lock().expect("lock");
    clear_for_testing();
    register_2d_classes();
    assert!(class_exists("SubViewport"));
    assert!(class_exists("ViewportTexture"));
}

#[test]
fn scene_subviewport_colorrect_to_viewport_texture_nonzero() {
    let _g = setup_classdb();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let sv_id = tree
        .add_child(root, Node::new("SubVP", "SubViewport"))
        .expect("add SubViewport");
    // Child ColorRect node (content marker; software fill uses fill_color below).
    let _rect = tree
        .add_child(sv_id, Node::new("Fill", "ColorRect"))
        .expect("add ColorRect");

    apply_subviewport_defaults(&mut tree, sv_id);
    set_size(&mut tree, sv_id, Vector2i::new(64, 64));
    assert_eq!(get_size(&tree, sv_id), Vector2i::new(64, 64));
    assert_eq!(get_update_mode(&tree, sv_id), UpdateMode::WhenVisible);

    let red = Color::rgb(1.0, 0.2, 0.1);
    let (sv, tex) = render_subviewport_to_texture(&tree, sv_id, Some(red));

    assert_eq!(sv.width(), 64);
    assert_eq!(sv.height(), 64);
    assert!(
        sv.buffer().has_nonzero_pixels(),
        "SubViewport color buffer must contain ColorRect fill"
    );
    assert!(
        tex.has_nonzero_pixels(),
        "ViewportTexture sample must be nonzero after ColorRect fill"
    );
    let p = tex.get_pixel(0, 0).expect("pixel");
    assert!((p.r - 1.0).abs() < 0.01, "expected red channel, got {p:?}");
}

#[test]
fn scene_subviewport_mesh_marker_to_viewport_texture() {
    let mut sv = SubViewport::new(Vector2i::new(32, 32));
    // MeshInstance-style solid marker in the center of the target.
    sv.render_mesh_marker(Color::rgb(0.0, 0.8, 0.2));

    let mut tex = ViewportTexture::new();
    tex.bind_subviewport(
        // Dummy id not required for pure buffer sample.
        gdscene::NodeId::next(),
        "SubVP",
    );
    tex.sample(&sv);

    assert!(tex.has_nonzero_pixels());
    assert_eq!(tex.width(), 32);
    // Corner remains clear (black), center is green.
    let corner = tex.get_pixel(0, 0).unwrap();
    assert!(corner.g < 0.01, "corner clear: {corner:?}");
    let center = tex.get_pixel(16, 16).unwrap();
    assert!(center.g > 0.5, "mesh marker center: {center:?}");
}

#[test]
fn subviewport_allocate_buffer_and_sample_after_software_fill() {
    let mut sv = SubViewport::with_defaults();
    assert_eq!(sv.size, Vector2i::new(512, 512));
    assert_eq!(sv.update_mode, UpdateMode::WhenVisible);

    // Before fill, buffer is black (alpha=1) — RGB zero; treat "nonzero" as any channel.
    // After solid fill RGB is nonzero.
    sv.fill_solid(Color::rgb(0.5, 0.0, 0.5));
    let mut tex = ViewportTexture::new();
    tex.sample(&sv);
    assert!(tex.has_nonzero_pixels());
    assert_eq!(tex.width(), 512);
    assert_eq!(tex.height(), 512);
}
