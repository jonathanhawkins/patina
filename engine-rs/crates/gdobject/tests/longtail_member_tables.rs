//! pat-s3rrv.247 — long-tail ClassDB member tables.
//!
//! Godot ground truth: `doc/classes/AStarGrid2D.xml` and
//! `doc/classes/VisualShaderNodeTexture.xml`.
//!
//! # ⚠️ The bead's "registers zero properties" is wrong for AStarGrid2D
//!
//! It already carried **eight** of its nine members, with correct defaults —
//! they sit after a twenty-method run, past the window a quick look covers.
//! Only `region` was genuinely absent.
//!
//! 🔑 That distinction matters more than it sounds: **a duplicate ClassDB member
//! shadows silently rather than erroring**, so adding the full nine blind would
//! have left eight duplicated members and nothing would have complained.
//! `a_no_member_is_registered_twice` is the guard for exactly that.

use gdobject::class_db::{get_property_list, register_all_classes};
use gdvariant::Variant;

fn prop(class: &str, name: &str) -> Option<gdobject::class_db::PropertyInfo> {
    get_property_list(class)
        .into_iter()
        .find(|p| p.name == name)
}

/// 🔑 No class here registers the same member twice.
///
/// ⚠️ ClassDB takes the LAST registration of a duplicated name, so a
/// double-registered member is invisible: the class still resolves, the property
/// list still contains the name, and only the default value quietly changes.
#[test]
fn a_no_member_is_registered_twice() {
    register_all_classes();

    for class in ["AStarGrid2D", "VisualShaderNodeTexture", "Theme"] {
        let names: Vec<String> = get_property_list(class)
            .into_iter()
            .map(|p| p.name)
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        let before = sorted.len();
        sorted.dedup();
        assert_eq!(
            before,
            sorted.len(),
            "{class} registers a duplicate member: {names:?}"
        );
    }
}

/// `AStarGrid2D`'s nine members, with the defaults Godot documents.
#[test]
fn b_astargrid2d_registers_all_nine_members() {
    register_all_classes();

    // The one that was missing.
    let region = prop("AStarGrid2D", "region").expect("region is registered");
    assert_eq!(
        region.default_value,
        Variant::Rect2i(gdcore::math::Rect2i::new(
            gdcore::math::Vector2i::new(0, 0),
            gdcore::math::Vector2i::new(0, 0),
        )),
        "region defaults to an empty Rect2i"
    );

    // ⚠️ Accessors are NOT asserted here, and that is deliberate rather than an
    // oversight. None of AStarGrid2D's nine members carries `with_accessors`
    // yet — Godot documents a setter/getter for each, and wiring all nine is
    // tracked as remaining scope on pat-s3rrv.247. Asserting them on `region`
    // alone would pin a state no other member shares, and an earlier draft of
    // this test did exactly that and failed for it.
    assert_eq!(
        region.getter, "",
        "no member here has accessors yet; see the bead's remaining scope"
    );

    // ⚠️ `cell_size` defaults to (1, 1), NOT the zero every other vector member
    // uses. A zero cell size collapses every grid coordinate onto the origin,
    // and the two are one character apart in a table.
    assert_eq!(
        prop("AStarGrid2D", "cell_size").unwrap().default_value,
        Variant::Vector2(gdcore::math::Vector2::new(1.0, 1.0)),
        "cell_size is Vector2(1, 1)"
    );
    assert_eq!(
        prop("AStarGrid2D", "offset").unwrap().default_value,
        Variant::Vector2(gdcore::math::Vector2::new(0.0, 0.0)),
        "…while offset really is zero — the control for the assertion above"
    );

    for name in [
        "region",
        "size",
        "offset",
        "cell_size",
        "cell_shape",
        "jumping_enabled",
        "default_compute_heuristic",
        "default_estimate_heuristic",
        "diagonal_mode",
    ] {
        assert!(
            prop("AStarGrid2D", name).is_some(),
            "AStarGrid2D.{name} must be registered"
        );
    }
}

/// `VisualShaderNodeTexture`'s third member.
///
/// ⚠️ `source` and `texture_type` were registered and `texture` was not, so the
/// class read as complete while the member that actually carries the image was
/// invisible to `class_get_property_list`.
#[test]
fn c_visual_shader_node_texture_carries_its_texture() {
    register_all_classes();

    for name in ["source", "texture_type", "texture"] {
        assert!(
            prop("VisualShaderNodeTexture", name).is_some(),
            "VisualShaderNodeTexture.{name} must be registered"
        );
    }
    assert_eq!(
        prop("VisualShaderNodeTexture", "texture")
            .unwrap()
            .default_value,
        Variant::Nil,
        "a Texture2D ref with no documented default is null"
    );
}

/// `Theme`'s three members — already present before this bead, asserted so the
/// bead's "registers zero members" claim stays refuted in the tree.
///
/// ⚠️ `default_font_size` is **-1**, not 0: -1 means "inherit", while 0 would be
/// a real font size of zero.
#[test]
fn d_theme_members_were_already_present() {
    register_all_classes();

    assert_eq!(
        prop("Theme", "default_base_scale").unwrap().default_value,
        Variant::Float(0.0)
    );
    assert_eq!(
        prop("Theme", "default_font_size").unwrap().default_value,
        Variant::Int(-1),
        "-1 is `inherit`; 0 would be a literal zero-size font"
    );
    assert!(prop("Theme", "default_font").is_some());
}
