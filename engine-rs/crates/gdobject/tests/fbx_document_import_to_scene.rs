//! pat-nzgdu.217 — `FBXDocument`/`FBXState` are ClassDB-registered over the glTF
//! document model, and `GLTFDocument`'s import API resolves.
//!
//! **The FBX *parser* is not implemented, and that is a vendoring decision, not
//! an oversight.** The bead asks for FBX parsed "via ufbx compiled to wasm per
//! THIRDPARTY_STRATEGY" — but `THIRDPARTY_STRATEGY.md:230` is a *plan*
//! ("Classification: Wrap via FFI / ufbx (bucket 2) — prefer `ufbx` (C,
//! permissive) via FFI or pure-Rust port when FBX import is needed"), and **ufbx
//! is not vendored anywhere in the workspace**. Nothing can parse an `.fbx`
//! byte, so the acceptance's "returns a Node tree ... for a sample .fbx" cannot
//! be met by any amount of work inside this bead.
//!
//! The same gap exists one level up: pat-nzgdu.164 registered the 19 GLTF\*
//! classes and recorded that routing `gdresource`'s real `gltf_import.rs` parser
//! (1810 lines, and it works) through `GLTFDocument.append_from_buffer` /
//! `generate_scene` is follow-up. **glTF cannot import through this API either
//! yet** — so FBX, which reuses it wholesale, is blocked twice over.
//!
//! What this bead delivers is the ClassDB clause the acceptance names, plus the
//! `GLTFDocument` method surface that `FBXDocument` inherits — without which
//! `FBXDocument.append_from_buffer` does not resolve at all.
//!
//! Schemas diffed against Godot 4.6.1.stable's `extension_api.json`; the
//! `allow_geometry_helper_nodes` default against its doc XML.

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::GodotObject;
use gdobject::{
    class_exists, class_has_method, get_method_list, get_property_list, instantiate,
    is_parent_class, register_io_classes,
};
use gdvariant::Variant;

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

/// ⚠️ `.rev()`: `get_property_list` is base-first, so a plain `.find()` returns
/// the most-BASE match on a shadowed name while `instantiate` lets the derived
/// default win.
fn default_of(class: &str, name: &str) -> Variant {
    get_property_list(class)
        .into_iter()
        .rev()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} has no property `{name}`"))
        .default_value
}

fn arity(class: &str, method: &str) -> usize {
    get_method_list(class)
        .iter()
        .rev()
        .find(|m| m.name == method)
        .unwrap_or_else(|| panic!("{class} has no method `{method}`"))
        .argument_count
}

fn own_methods(class: &str, base: &str) -> Vec<String> {
    let base_methods: Vec<String> = get_method_list(base).into_iter().map(|m| m.name).collect();
    get_method_list(class)
        .into_iter()
        .map(|m| m.name)
        .filter(|m| !base_methods.contains(m))
        .collect()
}

/// The acceptance's checkable clause: `FBXDocument : GLTFDocument` and
/// `FBXState : GLTFState`.
#[test]
fn fbx_document_import_to_scene() {
    register_io_classes();

    assert!(class_exists("FBXDocument"));
    assert!(
        is_parent_class("FBXDocument", "GLTFDocument"),
        "the acceptance's chain: FBXDocument : GLTFDocument"
    );
    assert!(class_exists("FBXState"));
    assert!(
        is_parent_class("FBXState", "GLTFState"),
        "the acceptance's chain: FBXState : GLTFState"
    );

    // ⚠️ Godot imports FBX by REUSING the glTF model wholesale — that is why
    // these subclass the glTF types rather than forming their own family. Both
    // reach Resource through it.
    for c in ["FBXDocument", "FBXState"] {
        assert!(is_parent_class(c, "Resource"), "{c} <- .. <- Resource");
        assert!(is_parent_class(c, "Object"));
        assert!(!is_abstract(c), "{c} is GDREGISTER_CLASS in Godot");
        assert!(can_instantiate(c));
        let obj = instantiate(c).unwrap_or_else(|| panic!("{c} instantiates"));
        assert_eq!(obj.get_class(), c);
    }
}

/// ⚠️ `FBXDocument` binds NOTHING — zero methods, zero properties.
///
/// Its entire surface is `GLTFDocument`'s; it exists to *name* the format. The
/// temptation to give it `append_from_buffer` is exactly wrong — that method is
/// inherited, and re-declaring it would invent a ClassDB entry Godot lacks.
#[test]
fn fbx_document_binds_nothing_and_inherits_the_whole_gltf_surface() {
    register_io_classes();

    assert!(
        own_methods("FBXDocument", "GLTFDocument").is_empty(),
        "FBXDocument declares no methods of its own in Godot"
    );
    let base_props = props("GLTFDocument");
    let own_props: Vec<String> = props("FBXDocument")
        .into_iter()
        .filter(|p| !base_props.contains(p))
        .collect();
    assert!(
        own_props.is_empty(),
        "FBXDocument declares no properties of its own; found {own_props:?}"
    );

    // ...yet the inherited API is reachable, which is the point.
    for m in [
        "append_from_buffer",
        "append_from_file",
        "generate_scene",
        "get_supported_gltf_extensions",
    ] {
        assert!(
            class_has_method("FBXDocument", m),
            "FBXDocument must reach {m} through GLTFDocument"
        );
    }
}

/// ⚠️ The import API's real arities. The acceptance writes
/// `append_from_buffer(bytes, state)` and `generate_scene(state)` — **neither is
/// the actual signature**. Both take four arguments.
#[test]
fn the_import_api_has_godots_arities_not_the_beads() {
    register_io_classes();

    // (bytes, base_path, state, flags)
    assert_eq!(arity("GLTFDocument", "append_from_buffer"), 4);
    // (state, bake_fps, trimming, remove_immutable_tracks)
    assert_eq!(arity("GLTFDocument", "generate_scene"), 4);
    // (path, base_path, state, flags)
    assert_eq!(arity("GLTFDocument", "append_from_file"), 4);
    assert_eq!(arity("GLTFDocument", "append_from_scene"), 3);
    assert_eq!(arity("GLTFDocument", "generate_buffer"), 1);
    assert_eq!(arity("GLTFDocument", "write_to_filesystem"), 2);

    // The arities are inherited unchanged by FBXDocument.
    assert_eq!(arity("FBXDocument", "append_from_buffer"), 4);
    assert_eq!(arity("FBXDocument", "generate_scene"), 4);

    // Godot binds 23 methods on GLTFDocument. pat-nzgdu.164 registered the
    // family's properties but none of its methods, so this whole API was
    // unreachable before this bead.
    assert_eq!(
        own_methods("GLTFDocument", "Resource").len(),
        23,
        "Godot binds 23 methods on GLTFDocument"
    );
}

/// ⚠️ `FBXState` adds exactly ONE member over `GLTFState`, and it defaults to
/// **false**.
///
/// `allow_geometry_helper_nodes` is an FBX-only concept — the transform/pivot
/// helper nodes glTF has no equivalent for. Godot collapses them into transforms
/// unless asked to keep them, so a `true` default would silently produce a
/// deeper node tree than Godot for the same file.
#[test]
fn fbx_state_adds_only_the_geometry_helper_toggle() {
    register_io_classes();

    let base_props = props("GLTFState");
    let own: Vec<String> = props("FBXState")
        .into_iter()
        .filter(|p| !base_props.contains(p))
        .collect();
    assert_eq!(
        own,
        vec!["allow_geometry_helper_nodes"],
        "FBXState adds exactly one member over GLTFState"
    );

    assert_eq!(
        default_of("FBXState", "allow_geometry_helper_nodes"),
        Variant::Bool(false),
        "Godot collapses FBX helper nodes into transforms by default"
    );

    let own_m = own_methods("FBXState", "GLTFState");
    assert_eq!(own_m.len(), 2, "just the accessor pair");
    assert_eq!(arity("FBXState", "set_allow_geometry_helper_nodes"), 1);
    assert_eq!(arity("FBXState", "get_allow_geometry_helper_nodes"), 0);

    // ...and it inherits GLTFState's 31-property model — the "same
    // node/mesh/material/animation model" the bead describes.
    for p in ["json", "major_version", "minor_version"] {
        assert!(
            props("FBXState").iter().any(|x| x == p),
            "FBXState must inherit GLTFState::{p}"
        );
    }
}

#[test]
fn registration_is_idempotent() {
    // `class_count()` and property lists cannot catch a duplicate registration
    // (`classes` is name-keyed, so re-registering overwrites). The ClassId is
    // what actually changes.
    //
    // ⚠️ Only the two classes this bead added are covered: `register_gltf_classes`
    // registers the 19 GLTF* classes UNGUARDED, so their ClassIds are re-minted
    // on every call. That is pat-nzgdu.164's latent bug, not this bead's — see
    // the notes.
    let ids = || -> Vec<_> {
        ["FBXDocument", "FBXState"]
            .iter()
            .map(|c| gdobject::get_class_info(c).expect("registered").class_id)
            .collect()
    };

    register_io_classes();
    let first = ids();
    register_io_classes();
    assert_eq!(
        first,
        ids(),
        "re-registering must not mint new ClassIds — a guard is missing if it does"
    );
}
