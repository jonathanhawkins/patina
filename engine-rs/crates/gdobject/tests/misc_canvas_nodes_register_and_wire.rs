//! pat-nzgdu.174 — the seven misc canvas Node2D classes are ClassDB-registered.
//!
//! Covers the bead's "instantiates via ClassDB" clause. The drawing/input half
//! (Marker2D's gizmo, the mesh draw path, CanvasGroup compositing, the
//! framebuffer copy, TouchScreenButton input picking, AudioListener2D selection)
//! and the `.tscn` round-trip are follow-up — see the bead notes.
//!
//! Schemas diffed against Godot 4.6.1.stable's `extension_api.json`.

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::{
    class_exists, class_get_integer_constant, class_has_enum, class_has_signal, get_property_list,
    instantiate, is_parent_class, register_2d_classes,
};
// `get_class` is a `GodotObject` trait method, not inherent on GenericObject.
use gdobject::GodotObject;
use gdvariant::Variant;

/// The seven classes this bead adds. All derive Node2D.
const CANVAS_NODES: &[&str] = &[
    "Marker2D",
    "MeshInstance2D",
    "MultiMeshInstance2D",
    "CanvasGroup",
    "BackBufferCopy",
    "TouchScreenButton",
    "AudioListener2D",
];

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

fn default_of(class: &str, name: &str) -> Variant {
    get_property_list(class)
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} has no property `{name}`"))
        .default_value
}

fn assert_props(class: &str, want: &[&str]) {
    let have = props(class);
    for w in want {
        assert!(
            have.iter().any(|h| h == w),
            "{class} must expose `{w}`; got {have:?}"
        );
    }
}

/// The bead's clause: each of the seven instantiates via ClassDB.
#[test]
fn misc_canvas_nodes_register_and_wire() {
    register_2d_classes();

    assert_eq!(CANVAS_NODES.len(), 7);
    for class in CANVAS_NODES {
        assert!(class_exists(class), "ClassDB must know {class}");
        assert!(
            is_parent_class(class, "Node2D"),
            "{class} is a Node2D — that is what puts it on the canvas"
        );
        // Must reach the root: an unregistered intermediate severs the chain
        // silently and `class_exists` alone would not notice (pat-nzgdu.98).
        assert!(is_parent_class(class, "Node"), "{class} must reach Node");
        assert!(is_parent_class(class, "Object"));

        assert!(!is_abstract(class), "{class} is instantiable in Godot");
        assert!(can_instantiate(class));
        let obj = instantiate(class).unwrap_or_else(|| panic!("{class} instantiates"));
        assert_eq!(obj.get_class(), *class);
    }
}

/// Each node's own schema. These are what a `.tscn` carries, and what would be
/// silently dropped if the property were missing.
#[test]
fn each_node_carries_its_own_schema() {
    register_2d_classes();

    assert_props("Marker2D", &["gizmo_extents"]);
    assert_props("MeshInstance2D", &["mesh", "texture"]);
    assert_props("MultiMeshInstance2D", &["multimesh", "texture"]);
    assert_props("CanvasGroup", &["fit_margin", "clear_margin", "use_mipmaps"]);
    assert_props("BackBufferCopy", &["copy_mode", "rect"]);
    assert_props(
        "TouchScreenButton",
        &[
            "texture_normal",
            "texture_pressed",
            "bitmask",
            "shape",
            "shape_centered",
            "shape_visible",
            "passby_press",
            "action",
            "visibility_mode",
        ],
    );
}

/// ⚠️ The mesh instances are *near*-twins but not twins: one holds a `mesh`, the
/// other a `multimesh`. Copy-pasting the schema gets this wrong.
#[test]
fn mesh_and_multimesh_instances_hold_different_resources() {
    register_2d_classes();
    assert!(props("MeshInstance2D").iter().any(|p| p == "mesh"));
    assert!(
        !props("MeshInstance2D").iter().any(|p| p == "multimesh"),
        "MeshInstance2D holds a single mesh"
    );
    assert!(props("MultiMeshInstance2D").iter().any(|p| p == "multimesh"));
    assert!(
        !props("MultiMeshInstance2D").iter().any(|p| p == "mesh"),
        "MultiMeshInstance2D holds a MultiMesh instance buffer, not a mesh"
    );
    // Both signal on texture change.
    assert!(class_has_signal("MeshInstance2D", "texture_changed"));
    assert!(class_has_signal("MultiMeshInstance2D", "texture_changed"));
}

/// ⚠️ `BackBufferCopy` defaults to COPY_MODE_RECT (1), not disabled — a fresh
/// node copies its rect. A default of 0 would silently disable SCREEN_TEXTURE
/// and read as "the shader is broken".
#[test]
fn back_buffer_copy_defaults_to_copying_its_rect() {
    register_2d_classes();
    assert!(class_has_enum("BackBufferCopy", "CopyMode"));
    assert_eq!(
        class_get_integer_constant("BackBufferCopy", "COPY_MODE_DISABLED"),
        Some(0)
    );
    assert_eq!(
        class_get_integer_constant("BackBufferCopy", "COPY_MODE_RECT"),
        Some(1)
    );
    assert_eq!(
        class_get_integer_constant("BackBufferCopy", "COPY_MODE_VIEWPORT"),
        Some(2)
    );
    assert_eq!(
        default_of("BackBufferCopy", "copy_mode"),
        Variant::Int(1),
        "Godot's default is COPY_MODE_RECT, not DISABLED"
    );
    // ...and the rect has real extent, not a zero-size no-op.
    match default_of("BackBufferCopy", "rect") {
        Variant::Rect2(r) => assert!(
            r.size.x > 0.0 && r.size.y > 0.0,
            "the default copy rect must have extent, got {:?}",
            r.size
        ),
        other => panic!("rect must be a Rect2, got {other:?}"),
    }
}

#[test]
fn touch_screen_button_declares_its_input_surface() {
    register_2d_classes();
    assert!(class_has_signal("TouchScreenButton", "pressed"));
    assert!(class_has_signal("TouchScreenButton", "released"));
    assert!(class_has_enum("TouchScreenButton", "VisibilityMode"));
    assert_eq!(
        class_get_integer_constant("TouchScreenButton", "VISIBILITY_ALWAYS"),
        Some(0)
    );
    assert_eq!(
        class_get_integer_constant("TouchScreenButton", "VISIBILITY_TOUCHSCREEN_ONLY"),
        Some(1)
    );
    // Visible always by default — a touchscreen-only default would make the
    // button vanish on desktop and look like a broken node.
    assert_eq!(default_of("TouchScreenButton", "visibility_mode"), Variant::Int(0));
    // The shape is centred and drawn by default (Godot's behaviour).
    assert_eq!(default_of("TouchScreenButton", "shape_centered"), Variant::Bool(true));
    assert_eq!(default_of("TouchScreenButton", "shape_visible"), Variant::Bool(true));
    assert_eq!(default_of("TouchScreenButton", "passby_press"), Variant::Bool(false));
}

/// Godot gives `AudioListener2D` no properties at all — it is driven purely by
/// `make_current()`. Inventing one would be fiction.
#[test]
fn audio_listener_2d_has_no_properties_of_its_own() {
    register_2d_classes();
    let own: Vec<String> = props("AudioListener2D")
        .into_iter()
        .filter(|p| !props("Node2D").contains(p))
        .collect();
    assert!(
        own.is_empty(),
        "AudioListener2D adds no properties in Godot; found {own:?}"
    );
}

/// Object-typed properties hold nothing until assigned — a fresh node must not
/// claim to own a mesh or a shape.
#[test]
fn object_typed_properties_default_to_nil() {
    register_2d_classes();
    for (class, prop) in [
        ("MeshInstance2D", "mesh"),
        ("MeshInstance2D", "texture"),
        ("MultiMeshInstance2D", "multimesh"),
        ("TouchScreenButton", "shape"),
        ("TouchScreenButton", "bitmask"),
    ] {
        assert_eq!(
            default_of(class, prop),
            Variant::Nil,
            "{class}.{prop} holds no resource until assigned"
        );
    }
}

#[test]
fn registration_is_idempotent() {
    register_2d_classes();
    let before = props("TouchScreenButton");
    register_2d_classes();
    assert_eq!(before, props("TouchScreenButton"));
}
