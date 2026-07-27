//! pat-0q2kx.211 — `DirectionalLight3D.light_specular`, checked against the
//! doc XML rather than against a hand-copied constant.
//!
//! Godot ground truth, both sources cross-checked because they *appear* to
//! disagree and only one reading is right:
//!
//! - `upstream/godot/scene/3d/light_3d.cpp:476` — the `Light3D` ctor sets
//!   `PARAM_SPECULAR = 0.5`.
//! - `:606` — the `DirectionalLight3D` ctor overrides it to `1.0`.
//! - `OmniLight3D` / `SpotLight3D` set no specular in their ctors, so they keep
//!   the base's `0.5`.
//!
//! The doc XML states it the other way round: `Light3D.xml` carries
//! `default="1.0"`, and Omni/Spot each carry an explicit
//! `overrides="Light3D" default="0.5"`. The two sources agree on every
//! **concrete** class — Directional 1.0, Omni 0.5, Spot 0.5 — and differ only
//! on the abstract `Light3D` base, which no user instantiates. That is a
//! doc-generator artifact, so this file checks the concrete classes and skips
//! the base with the reason recorded rather than asserting a value it cannot
//! justify.
//!
//! The doc XML is parsed at test time from `upstream/godot/doc/classes`, so a
//! future upstream default change is caught here instead of drifting silently.
//!
//! ⚠️ SCOPE: this is the Light3D family, not the whole ClassDB. The bead also
//! asks for a workspace-wide default oracle. That needs one run to seed a
//! baseline of existing divergences — writing a strict all-class assertion
//! blind would just land a red test. See the completion report.

use gdobject::class_db::{instantiate, register_3d_classes};
// `get_property` is a `GodotObject` trait method, not inherent on
// `GenericObject` — the inherent one lives on `ObjectBase`, so finding
// `pub fn get_property` in object.rs is not evidence it resolves here.
use gdobject::GodotObject;
use gdvariant::Variant;

/// `<repo>/upstream/godot/doc/classes` — three levels up from this crate.
fn doc_classes_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("crate sits at <repo>/engine-rs/crates/gdobject")
        .join("upstream/godot/doc/classes")
}

/// The `default=` attribute of a `<member name="...">`, if the class declares
/// that member itself. `None` means the class inherits it.
fn doc_default(class: &str, member: &str) -> Option<String> {
    let path = doc_classes_dir().join(format!("{class}.xml"));
    let xml = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("doc XML missing at {}: {e}", path.display()));

    for line in xml.lines() {
        let line = line.trim();
        if !line.starts_with("<member ") {
            continue;
        }
        if !line.contains(&format!("name=\"{member}\"")) {
            continue;
        }
        // Attribute order varies, so scan for `default="` rather than assuming
        // a position. A member with no `default=` is a real case (Godot omits
        // it for some properties) and must read as absent, not as empty.
        let key = "default=\"";
        let start = line.find(key)? + key.len();
        let rest = &line[start..];
        let end = rest.find('"')?;
        return Some(rest[..end].to_string());
    }
    None
}

/// Resolves a member's default the way the doc set does: a class without its
/// own entry inherits its parent's.
fn resolved_doc_default(chain: &[&str], member: &str) -> String {
    for class in chain {
        if let Some(v) = doc_default(class, member) {
            return v;
        }
    }
    panic!("no class in {chain:?} declares a default for `{member}`");
}

fn registered_default(class: &str, member: &str) -> Variant {
    register_3d_classes();
    instantiate(class)
        .unwrap_or_else(|| panic!("{class} must be registered in the ClassDB"))
        .get_property(member)
}

/// **The bead's acceptance.** DirectionalLight3D specular is 1.0 while
/// OmniLight3D and SpotLight3D stay 0.5 — and each value is compared against
/// the doc XML read from disk, not against a number typed into this file.
#[test]
fn directional_light_specular_overrides_the_base() {
    // Doc-resolved expectations. Directional has no own entry, so it inherits
    // Light3D's 1.0; Omni and Spot each declare their own 0.5.
    let directional = resolved_doc_default(&["DirectionalLight3D", "Light3D"], "light_specular");
    let omni = resolved_doc_default(&["OmniLight3D", "Light3D"], "light_specular");
    let spot = resolved_doc_default(&["SpotLight3D", "Light3D"], "light_specular");

    assert_eq!(
        directional, "1.0",
        "upstream doc XML changed: DirectionalLight3D specular is no longer 1.0"
    );
    assert_eq!(omni, "0.5", "upstream doc XML changed for OmniLight3D");
    assert_eq!(spot, "0.5", "upstream doc XML changed for SpotLight3D");

    // Patina must agree with those.
    assert_eq!(
        registered_default("DirectionalLight3D", "light_specular"),
        Variant::Float(1.0),
        "DirectionalLight3D's ctor sets PARAM_SPECULAR = 1.0 (light_3d.cpp:606); \
         inheriting the Light3D base's 0.5 makes every directional light in a \
         scene render with half the specular Godot gives it"
    );
    assert_eq!(
        registered_default("OmniLight3D", "light_specular"),
        Variant::Float(0.5),
        "OmniLight3D adds no specular override, so it keeps the base 0.5 — the \
         fix for DirectionalLight3D must not be applied to the shared base"
    );
    assert_eq!(
        registered_default("SpotLight3D", "light_specular"),
        Variant::Float(0.5),
        "SpotLight3D likewise keeps the base 0.5"
    );
}

/// The override must be reachable through `instantiate`, which is the path the
/// acceptance criterion and real scenes use.
///
/// This matters because `get_property_list` concatenates the inheritance chain
/// **without deduping**: after this fix, that list contains `light_specular`
/// twice for DirectionalLight3D (0.5 from the base, 1.0 from the override).
/// `instantiate` applies the chain base-to-derived so the derived value wins,
/// but anything reading the raw list must pick the right duplicate.
#[test]
fn the_override_wins_through_instantiate_despite_a_duplicated_entry() {
    register_3d_classes();

    let obj = instantiate("DirectionalLight3D").expect("registered");
    assert_eq!(obj.get_property("light_specular"), Variant::Float(1.0));

    // The base is genuinely still 0.5 — the fix is an override, not an edit to
    // the shared base that would drag Omni and Spot along with it.
    let base = instantiate("Light3D").map(|o| o.get_property("light_specular"));
    if let Some(v) = base {
        assert_eq!(
            v,
            Variant::Float(0.5),
            "the Light3D base must stay 0.5 so Omni/Spot inherit correctly"
        );
    }

    // Sibling properties on the derived class are unaffected.
    assert_eq!(
        obj.get_property("directional_shadow_max_distance"),
        Variant::Float(100.0),
        "adding the specular override must not disturb the block it joins"
    );
}

/// The parser distinguishes "declares its own default" from "inherits" — the
/// distinction the whole oracle rests on. If `doc_default` returned `Some` for
/// every class, `resolved_doc_default` would never walk to the parent and the
/// comparison above would be meaningless.
#[test]
fn the_doc_parser_separates_declared_defaults_from_inherited_ones() {
    assert_eq!(
        doc_default("Light3D", "light_specular").as_deref(),
        Some("1.0"),
        "Light3D declares the member"
    );
    assert_eq!(
        doc_default("OmniLight3D", "light_specular").as_deref(),
        Some("0.5"),
        "OmniLight3D declares an override"
    );
    assert_eq!(
        doc_default("DirectionalLight3D", "light_specular"),
        None,
        "DirectionalLight3D declares NO specular member — it inherits. If this \
         starts returning Some, the parser is matching the wrong line and the \
         inheritance walk is dead code"
    );
    assert_eq!(
        doc_default("DirectionalLight3D", "no_such_member_xyz"),
        None,
        "an absent member reads as absent rather than panicking"
    );
    // A member Directional *does* declare, proving the None above is specific
    // to `light_specular` and not the file failing to parse at all.
    assert!(
        doc_default("DirectionalLight3D", "directional_shadow_max_distance").is_some(),
        "the DirectionalLight3D doc file must parse — a blanket None would \
         make the test above pass for the wrong reason"
    );
}
