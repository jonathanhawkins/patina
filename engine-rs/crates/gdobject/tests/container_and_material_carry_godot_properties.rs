//! pat-s3rrv.57 (merged finding 3): ScrollContainer, SplitContainer and
//! BaseMaterial3D carry their real Godot members.
//!
//! # The safety rule these three were chosen by
//!
//! `SceneAccess::create_node` seeds a NODE's property bag from ClassDB, so
//! registering a property can change what the runtime reads. A **Resource** is
//! never seeded, so it is safe by construction whatever names it shares with a
//! reader — which is why BaseMaterial3D (an abstract Resource) needed no
//! per-member comparison at all, despite matching seven readers in
//! `render_server_3d.rs` by name.
//!
//! The two containers ARE nodes, so each read member was compared against its
//! reader's fallback: `scroll_horizontal`/`scroll_vertical` (`control.rs:2240`)
//! and `split_offset` (`control.rs:1941`) both fall back to `0`, and
//! `vertical` (`control.rs:1276`) to `false` — all matching Godot.

use gdobject::class_db::{get_class_info, register_all_classes};
use gdvariant::Variant;

fn own(class: &str, name: &str) -> Variant {
    register_all_classes();
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is registered"))
        .properties
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} must declare `{name}`"))
        .default_value
}

fn own_count(class: &str) -> usize {
    register_all_classes();
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is registered"))
        .properties
        .len()
}

#[test]
fn scroll_container_carries_its_eleven_members() {
    assert_eq!(own_count("ScrollContainer"), 11);

    // ⚠️ The scroll MODES default to 1 (AUTO), not 0 (DISABLED). A zero here
    // would silently disable scrolling on every container in the project — the
    // enum-zero trap this bead keeps producing.
    assert_eq!(
        own("ScrollContainer", "horizontal_scroll_mode"),
        Variant::Int(1)
    );
    assert_eq!(
        own("ScrollContainer", "vertical_scroll_mode"),
        Variant::Int(1)
    );

    // ⚠️ And the custom steps default to -1.0, Godot's "use the theme's step"
    // sentinel; 0.0 would mean a step of zero pixels.
    assert_eq!(
        own("ScrollContainer", "scroll_horizontal_custom_step"),
        Variant::Float(-1.0)
    );
    assert_eq!(own("ScrollContainer", "scroll_horizontal"), Variant::Int(0));
    assert_eq!(own("ScrollContainer", "follow_focus"), Variant::Bool(false));
}

#[test]
fn split_container_carries_its_eleven_members() {
    assert_eq!(own_count("SplitContainer"), 11);

    // ⚠️ `dragging_enabled` defaults TRUE — a false would make every splitter
    // in the project immovable, which reads as a broken UI.
    assert_eq!(
        own("SplitContainer", "dragging_enabled"),
        Variant::Bool(true)
    );
    // ⚠️ `split_offsets` is a ONE-element array holding 0, not an empty array.
    assert_eq!(
        own("SplitContainer", "split_offsets"),
        Variant::PackedInt32Array(vec![0])
    );
    // Both members the runtime reads, matching their fallbacks exactly.
    assert_eq!(own("SplitContainer", "split_offset"), Variant::Int(0));
    assert_eq!(own("SplitContainer", "vertical"), Variant::Bool(false));
}

/// BaseMaterial3D is the largest real parity item on the list: 131 members,
/// previously zero.
#[test]
fn base_material_3d_carries_its_stated_members() {
    // ⚠️ 121, not 131. Ten members state NO default in doc/classes/BaseMaterial3D.xml
    // (alpha_scissor_threshold, emission_intensity, particles_anim_h_frames …).
    // Inventing 0.0 for those is exactly the enum-zero trap above, so they are
    // deliberately left unregistered rather than guessed.
    assert_eq!(
        own_count("BaseMaterial3D"),
        121,
        "121 of Godot's 131 members state a default; the other ten are omitted \
         rather than invented"
    );

    assert_eq!(
        own("BaseMaterial3D", "albedo_color"),
        Variant::Color(gdcore::math::Color::new(1.0, 1.0, 1.0, 1.0))
    );
    assert_eq!(own("BaseMaterial3D", "metallic"), Variant::Float(0.0));
    // ⚠️ `roughness` defaults to 1.0, not 0.0 — a zero would make every
    // unconfigured material a mirror.
    assert_eq!(own("BaseMaterial3D", "roughness"), Variant::Float(1.0));
    assert_eq!(own("BaseMaterial3D", "shading_mode"), Variant::Int(1));
    // Texture slots are Nil, the same idiom every other resource-typed property
    // in this file uses.
    assert_eq!(own("BaseMaterial3D", "albedo_texture"), Variant::Nil);
}

/// The registration must not disturb the inheritance chain: StandardMaterial3D
/// and ORMMaterial3D both extend BaseMaterial3D and inherit all of it.
#[test]
fn the_material_subclasses_inherit_the_new_members() {
    register_all_classes();
    let names: Vec<String> = gdobject::class_db::get_property_list("StandardMaterial3D")
        .into_iter()
        .map(|p| p.name)
        .collect();

    for expected in ["albedo_color", "roughness", "metallic"] {
        assert!(
            names.iter().any(|n| n == expected),
            "StandardMaterial3D must inherit `{expected}` from BaseMaterial3D"
        );
    }
    // No duplicates: get_property_list concatenates the chain without deduping.
    for name in &names {
        assert_eq!(
            names.iter().filter(|n| *n == name).count(),
            1,
            "`{name}` appears more than once in StandardMaterial3D's list"
        );
    }
}
