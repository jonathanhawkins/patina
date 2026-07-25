//! pat-0q2kx.7 — `Resource` is a `RefCounted`, not a direct `Object`.
//!
//! Godot ground truth: `extension_api.json` gives `Resource <- RefCounted <-
//! Object`, and `upstream/godot/core/io/resource.h:53` declares
//! `class Resource : public RefCounted`.
//!
//! Patina registered `Resource` with parent `Object`, which made `Resource` and
//! `RefCounted` *siblings*. `is_parent_class` is the implementation behind
//! `Object.is_class()` / `Node.is_class()` (gdobject/src/object.rs,
//! gdscene/src/node.rs), the editor Create-Node "inherits" filter
//! (gdeditor/src/create_dialog.rs) and the inspector's resource-slot type check
//! (gdeditor/src/inspector.rs) — so `res is RefCounted` was false for all ~200
//! resource types, and every RefCounted-typed slot rejected every resource.
//!
//! ## Why this file re-clears the ClassDB per entry point
//!
//! `inheritance_chain` resolves parents **by name, at query time**, and stops
//! dead at the first name that is not registered. Re-parenting `Resource` onto
//! `RefCounted` therefore makes every resource chain depend on `RefCounted`
//! being registered — an unregistered base silently severs the chain from
//! `Object` (the pat-nzgdu.98 failure mode). Eight `register_*_classes` entry
//! points bootstrap `Resource`, and three of them (`register_3d_classes`,
//! `register_viewport_classes`, `register_navigation_resource_classes`) did not
//! bootstrap `RefCounted` at all. Asserting only against the fully-populated DB
//! would hide that: some *other* entry point would have registered `RefCounted`
//! first. So each public entry point is exercised against a cleared DB.

use gdobject::{
    class_exists, clear_for_testing, get_class_info, inheritance_chain, is_parent_class,
    register_2d_classes, register_3d_classes, register_audio_classes, register_editor_classes,
    register_io_classes, register_navigation_classes, register_viewport_classes,
};

/// Asserts `class` reaches `RefCounted` *and then* `Object`, in that order, and
/// that neither is merely a dangling parent name.
fn assert_refcounted_then_object(class: &str) {
    let chain = inheritance_chain(class);
    assert!(
        chain.contains(&"Resource".to_string()),
        "{class} should be a Resource; chain={chain:?}"
    );

    let rc = chain
        .iter()
        .position(|c| c == "RefCounted")
        .unwrap_or_else(|| panic!("{class} never reaches RefCounted; chain={chain:?}"));
    let obj = chain
        .iter()
        .position(|c| c == "Object")
        .unwrap_or_else(|| panic!("{class} never reaches Object; chain={chain:?}"));

    assert!(
        rc < obj,
        "RefCounted must sit between the resource and Object, as in Godot; chain={chain:?}"
    );
    assert_eq!(
        chain[chain.len() - 1],
        "Object",
        "Object is the root of every chain; chain={chain:?}"
    );

    // `is_parent_class` is what the callers above actually use.
    assert!(
        is_parent_class(class, "RefCounted"),
        "{class} is RefCounted"
    );
    assert!(is_parent_class(class, "Object"), "{class} is Object");
}

#[test]
fn resource_inherits_refcounted_chain() {
    // ── 1. Every entry point that bootstraps `Resource` keeps the chain whole
    //       when it is the ONLY thing that ran. ─────────────────────────────
    //
    // `register_viewport_classes` also covers the private
    // `register_rendering_device_classes`; `register_navigation_classes` covers
    // the private `register_navigation_resource_classes`; `register_io_classes`
    // covers the private `register_gltf_classes` — each of which bootstraps
    // `Resource` on its own path.
    let entry_points: [(&str, fn()); 6] = [
        ("register_3d_classes", register_3d_classes),
        ("register_2d_classes", register_2d_classes),
        ("register_viewport_classes", register_viewport_classes),
        ("register_audio_classes", register_audio_classes),
        ("register_io_classes", register_io_classes),
        ("register_navigation_classes", register_navigation_classes),
    ];

    for (name, register) in entry_points {
        clear_for_testing();
        register();

        assert!(
            class_exists("RefCounted"),
            "{name} registers Resource, so it must also register its base"
        );
        assert_eq!(
            get_class_info("Resource")
                .unwrap_or_else(|| panic!("{name} should register Resource"))
                .parent_class,
            "RefCounted",
            "{name}: Godot has `class Resource : public RefCounted`"
        );
        assert!(
            is_parent_class("Resource", "RefCounted"),
            "{name}: Resource <- RefCounted"
        );
        // The regression this guards: an unregistered `RefCounted` would sever
        // Resource from Object entirely, which is strictly worse than the bug
        // being fixed.
        assert!(
            is_parent_class("Resource", "Object"),
            "{name}: Resource must still reach Object through RefCounted"
        );
        assert_eq!(
            inheritance_chain("Resource"),
            vec!["Resource", "RefCounted", "Object"],
            "{name}: exact Godot chain"
        );
    }

    // ── 2. The acceptance's named classes, against the full registry. ───────
    clear_for_testing();
    register_editor_classes();

    // A deep 3D resource: StandardMaterial3D <- BaseMaterial3D <- Material <-
    // Resource. Before the fix its chain stopped at Resource under
    // `register_3d_classes` alone.
    assert_refcounted_then_object("StandardMaterial3D");
    // An editor resource — the one whose test asserted the *negation* of this.
    assert_refcounted_then_object("EditorSettings");

    assert!(is_parent_class("Resource", "RefCounted"));
    assert!(is_parent_class("EditorSettings", "RefCounted"));
    assert!(is_parent_class("StandardMaterial3D", "RefCounted"));

    // ── 3. Not vacuous. ────────────────────────────────────────────────────
    //
    // `RefCounted` is genuinely registered and genuinely derives `Object`, so
    // the assertions above discriminate rather than passing on an absent class.
    assert!(class_exists("RefCounted"));
    assert_eq!(
        inheritance_chain("RefCounted"),
        vec!["RefCounted", "Object"]
    );
    // And the change does not make *everything* a RefCounted: Godot's Node
    // family descends straight from Object, and so do the plain-Object helpers.
    assert!(!is_parent_class("Node", "RefCounted"));
    assert!(!is_parent_class("Node2D", "RefCounted"));
    assert!(is_parent_class("Node", "Object"));

    // ── 4. Sample breadth: the fix reaches all resource families, not just
    //       the three the acceptance names. ────────────────────────────────
    for class in [
        "Material",            // 3D (register_3d_classes)
        "StyleBox",            // 2D / GUI (register_2d_classes)
        "Texture",             // rendering (register_viewport_classes)
        "AudioStream",         // audio (register_audio_classes)
        "PackedDataContainer", // io (register_io_classes)
        "NavigationPolygon",   // navigation (register_navigation_resource_classes)
    ] {
        assert!(
            is_parent_class(class, "RefCounted"),
            "{class} is a Resource, therefore a RefCounted"
        );
        assert!(
            is_parent_class(class, "Object"),
            "{class} must still reach Object"
        );
    }
}
