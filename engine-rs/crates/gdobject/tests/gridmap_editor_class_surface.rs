//! pat-nzgdu.193 — the GridMap editing tool's class surface.
//!
//! **This is the bead's explicitly SECONDARY half.** The bead's primary ask is
//! the served-editor feature (palette dock, viewport painting, `.tscn`
//! persistence) — not done. See the bead notes.
//!
//! What is here is the foundation that feature needs: `GridMap` was registered
//! bare, so the inspector had nothing to show and `set_cell_item` was invisible
//! to ClassDB even though `gdscene/src/grid_map.rs` implements it.
//!
//! Schemas diffed against Godot 4.6.1.stable's `extension_api.json`; defaults
//! against its `--doctool` XML.

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::{
    class_exists, class_has_signal, get_method_list, get_property_list, instantiate,
    is_parent_class, register_editor_classes,
};
// `get_class` is a `GodotObject` trait method, not inherent on GenericObject.
use gdobject::GodotObject;
use gdvariant::Variant;

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

fn arity(class: &str, method: &str) -> usize {
    get_method_list(class)
        .iter()
        .rev()
        .find(|m| m.name == method)
        .unwrap_or_else(|| panic!("{class} has no method `{method}`"))
        .argument_count
}

#[test]
fn gridmap_family_is_registered() {
    register_editor_classes();

    assert!(class_exists("GridMap"));
    assert!(
        is_parent_class("GridMap", "Node3D"),
        "a GridMap is a 3D node"
    );
    assert!(class_exists("MeshLibrary"));
    assert!(
        is_parent_class("MeshLibrary", "Resource"),
        "the palette is a Resource — that is what lets it be saved and shared"
    );
    assert!(class_exists("GridMapEditorPlugin"));
    assert!(
        is_parent_class("GridMapEditorPlugin", "EditorPlugin"),
        "the tool is an EditorPlugin"
    );

    for class in ["GridMap", "MeshLibrary", "GridMapEditorPlugin"] {
        assert!(
            is_parent_class(class, "Object"),
            "{class} must reach Object"
        );
        assert!(!is_abstract(class));
        assert!(can_instantiate(class));
        let obj = instantiate(class).unwrap_or_else(|| panic!("{class} instantiates"));
        assert_eq!(obj.get_class(), class);
    }
}

/// ⚠️ `cell_size` defaults to **Vector3(2, 2, 2)**, not ones.
///
/// A 1×1×1 default would misplace every painted cell by half a cell — the tool
/// would look subtly broken rather than obviously wrong, which is worse.
#[test]
fn gridmap_defaults_match_godot() {
    register_editor_classes();

    match default_of("GridMap", "cell_size") {
        Variant::Vector3(v) => assert!(
            (v.x - 2.0).abs() < 1e-6 && (v.y - 2.0).abs() < 1e-6 && (v.z - 2.0).abs() < 1e-6,
            "Godot's default is Vector3(2, 2, 2), got {v:?}"
        ),
        other => panic!("cell_size must be a Vector3, got {other:?}"),
    }
    assert_eq!(default_of("GridMap", "cell_octant_size"), Variant::Int(8));
    assert_eq!(default_of("GridMap", "cell_scale"), Variant::Float(1.0));
    // Cells are centred on all three axes by default.
    for axis in ["cell_center_x", "cell_center_y", "cell_center_z"] {
        assert_eq!(default_of("GridMap", axis), Variant::Bool(true), "{axis}");
    }
    assert_eq!(default_of("GridMap", "collision_layer"), Variant::Int(1));
    assert_eq!(default_of("GridMap", "collision_mask"), Variant::Int(1));
    assert_eq!(
        default_of("GridMap", "collision_priority"),
        Variant::Float(1.0)
    );
    // Navigation baking is OFF by default — it is expensive.
    assert_eq!(
        default_of("GridMap", "bake_navigation"),
        Variant::Bool(false)
    );
    // A fresh GridMap holds no palette until one is assigned.
    assert_eq!(default_of("GridMap", "mesh_library"), Variant::Nil);
}

/// The cell API the paint tool drives — and the coordinate conversion without
/// which a viewport click cannot become a cell.
#[test]
fn gridmap_exposes_the_cell_api_the_tool_needs() {
    register_editor_classes();

    // The acceptance's verb. 2 required args (position, item); orientation
    // defaults, so an arity of 3 would be wrong.
    assert_eq!(
        arity("GridMap", "set_cell_item"),
        2,
        "set_cell_item(position, item) — orientation has a default"
    );
    assert_eq!(arity("GridMap", "get_cell_item"), 1);
    assert_eq!(arity("GridMap", "get_cell_item_orientation"), 1);

    // ⚠️ Without these a paint tool cannot turn a viewport ray hit into a cell
    // coordinate — they are the whole reason clicking a cell works.
    assert_eq!(arity("GridMap", "local_to_map"), 1);
    assert_eq!(arity("GridMap", "map_to_local"), 1);

    assert_eq!(arity("GridMap", "get_used_cells"), 0);
    assert_eq!(arity("GridMap", "get_used_cells_by_item"), 1);
    assert_eq!(arity("GridMap", "clear"), 0);
    assert_eq!(arity("GridMap", "set_mesh_library"), 1);

    assert!(class_has_signal("GridMap", "cell_size_changed"));
    assert!(class_has_signal("GridMap", "changed"));
}

/// ⚠️ `MeshLibrary` declares NO properties — its whole surface is the indexed
/// item API, which is exactly what a palette dock reads. Inventing an `items`
/// property would be schema fiction.
#[test]
fn mesh_library_is_an_indexed_item_api_not_a_property_bag() {
    register_editor_classes();

    let base = props("Resource");
    let own: Vec<String> = props("MeshLibrary")
        .into_iter()
        .filter(|p| !base.contains(p))
        .collect();
    assert!(
        own.is_empty(),
        "MeshLibrary declares no properties in Godot; found {own:?}"
    );

    // Item lifecycle, all indexed by id.
    assert_eq!(arity("MeshLibrary", "create_item"), 1);
    assert_eq!(arity("MeshLibrary", "remove_item"), 1);
    assert_eq!(arity("MeshLibrary", "get_item_list"), 0);
    assert_eq!(arity("MeshLibrary", "find_item_by_name"), 1);
    // Godot hands out ids; a palette needs this to add one.
    assert_eq!(arity("MeshLibrary", "get_last_unused_item_id"), 0);
    // Setters take (id, value); getters take (id).
    for m in ["set_item_name", "set_item_mesh", "set_item_preview"] {
        assert_eq!(arity("MeshLibrary", m), 2, "{m}(id, value)");
    }
    for m in ["get_item_name", "get_item_mesh", "get_item_preview"] {
        assert_eq!(arity("MeshLibrary", m), 1, "{m}(id)");
    }
}

/// ⚠️ `GridMapEditorPlugin` is not a marker class — its bound methods ARE the
/// tool's API. A served-editor endpoint has to satisfy exactly this contract.
#[test]
fn editor_plugin_exposes_the_tools_real_api() {
    register_editor_classes();

    // Which GridMap the tool is painting into.
    assert_eq!(arity("GridMapEditorPlugin", "get_current_grid_map"), 0);
    // Which palette mesh is armed — the acceptance's "choosing a mesh".
    assert_eq!(arity("GridMapEditorPlugin", "set_selected_palette_item"), 1);
    assert_eq!(arity("GridMapEditorPlugin", "get_selected_palette_item"), 0);
    // The paint selection.
    assert_eq!(arity("GridMapEditorPlugin", "set_selection"), 2);
    assert_eq!(arity("GridMapEditorPlugin", "get_selected_cells"), 0);
    assert_eq!(arity("GridMapEditorPlugin", "clear_selection"), 0);
    assert_eq!(arity("GridMapEditorPlugin", "has_selection"), 0);
    assert_eq!(arity("GridMapEditorPlugin", "get_selection"), 0);

    // It inherits EditorPlugin's surface — that is what installs it.
    assert!(is_parent_class("GridMapEditorPlugin", "EditorPlugin"));
    assert!(
        !get_method_list("EditorPlugin").is_empty(),
        "EditorPlugin has a real surface, so the inheritance is worth something"
    );
}

#[test]
fn registration_is_idempotent() {
    // `class_count()` and property lists cannot catch a duplicate registration
    // (`classes` is name-keyed, so re-registering overwrites). The ClassId is
    // what actually changes.
    let ids = || -> Vec<_> {
        ["GridMap", "MeshLibrary", "GridMapEditorPlugin"]
            .iter()
            .map(|c| gdobject::get_class_info(c).expect("registered").class_id)
            .collect()
    };

    register_editor_classes();
    let first = ids();
    register_editor_classes();
    assert_eq!(
        first,
        ids(),
        "re-registering must not mint new ClassIds — a guard is missing if it does"
    );
}
