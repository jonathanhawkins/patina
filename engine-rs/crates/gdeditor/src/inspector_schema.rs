//! Per-class editor property schema registry (pat-ghfv0.x editor-inspector parity).
//!
//! Godot's inspector does not show only the properties a node happens to have
//! stored; it enumerates the node's **full class property schema** — every
//! `PROPERTY_USAGE_EDITOR` property, resolved through the inheritance chain
//! (`Node` → `CanvasItem`/`Node3D` → leaf), each shown with its class default
//! value unless the node stored an override.
//!
//! Patina's `/api/node/<id>` historically surfaced only the STORED properties
//! (those set in the `.tscn` plus a few engine defaults), which is why the
//! editor-parity oracle (`editor_ui_oracle_test`) once measured ~1% inspector
//! parity against real Godot 4.6.1.
//!
//! This module is the mechanism; the per-class schema data lives in the
//! generated [`crate::inspector_schema_generated`] table (produced from the
//! captured goldens by `tools/oracle/gen_inspector_schema.py`), covering every
//! class in the oracle scenes (Node, Node2D, Node3D, Control, MeshInstance3D,
//! the light/physics/CSG families, …). Any class NOT directly seeded resolves to
//! its nearest seeded ancestor via [`parent_of`], so new/uncaptured classes still
//! get a sensible base schema.
//!
//! The golden data is deliberately kept in `gdeditor`, separate from the
//! runtime `gdobject::class_db` (a runtime registry with different
//! defaults/hints that does not model editor-synthesized properties such as
//! `position`/`rotation`/`scale`). Property orderings, hints, and defaults are
//! transcribed from `fixtures/editor_oracle/inspector_*.json` — the exact source
//! of truth the oracle diffs against.
//!
//! ⚠️ Separate does not mean *unaware*: [`editor_schema`] also merges in what
//! ClassDB knows about classes the goldens never captured (pat-s3rrv.57). Of the
//! ~688 registered classes only ~68 had their own golden — ~185 showed an
//! ancestor's properties and none of their own, and ~435 resolved to nothing at
//! all. The merge is strictly additive and a golden always wins, so the oracle
//! numbers these fixtures exist to defend cannot move.

use std::borrow::Cow;

use gdvariant::Variant;

/// One editor-visible property in a class's inspector schema: its name, the
/// Godot type name, the `PropertyHint` value, and the class default value used
/// when the node has not stored an override.
#[derive(Debug, Clone)]
pub struct EditorProp {
    /// Property name, e.g. `"position"`.
    ///
    /// ⚠️ `Cow` rather than `&'static str` because a row can now also come from
    /// [`gdobject::class_db`], whose names are owned `String`s (pat-s3rrv.57).
    /// The generated table still passes literals, which borrow.
    pub name: Cow<'static, str>,
    /// Godot type name, e.g. `"Vector2"` / `"int"` / `"Color"`.
    pub type_name: Cow<'static, str>,
    /// Godot `PropertyHint` value (0 = none).
    pub hint: i32,
    /// Godot `hint_string` (e.g. an enum's `"Inherit,Pausable,…"` list), `""` if none.
    pub hint_string: Cow<'static, str>,
    /// The value Godot's inspector *shows* when the node stores no override.
    /// Usually this is the class default, but for a few properties it is a
    /// project-settings-/context-derived display value (e.g. CollisionShape
    /// `debug_color`) that differs from the true ClassDB default below. Kept as
    /// the display fallback so served values match Godot's shown values.
    pub default: Variant,
    /// Godot's TRUE class default (`ClassDB.class_get_property_default_value`),
    /// used to decide `is_default` — whether the shown value is an override. This
    /// is the exact criterion the oracle capture (`editor_inspector_probe.gd`)
    /// uses. Separated from [`Self::default`] so a shown value can match Godot's
    /// display (keeping value-parity) while still being reported as an override
    /// when it differs from the real class default (pat-ghfv0.41).
    pub true_default: Variant,
}

impl EditorProp {
    pub(crate) fn new(
        name: impl Into<Cow<'static, str>>,
        type_name: impl Into<Cow<'static, str>>,
        hint: i32,
        hint_string: impl Into<Cow<'static, str>>,
        default: Variant,
        true_default: Variant,
    ) -> Self {
        Self {
            name: name.into(),
            type_name: type_name.into(),
            hint,
            hint_string: hint_string.into(),
            default,
            true_default,
        }
    }
}

/// Returns the flattened editor property schema for `class_name`, resolving
/// through inheritance to the nearest **seeded** class. Returns `None` when no
/// seeded ancestor exists **and** `gdobject::class_db` does not know the class
/// either (the caller then falls back to stored-only enumeration).
///
/// Resolution order, golden-first (pat-s3rrv.57):
/// 1. this class's own captured golden, returned untouched;
/// 2. otherwise the nearest seeded ancestor's schema as the base,
/// 3. plus every ClassDB property for this class the base does not already
///    carry — so a leaf class stops showing only its ancestor's properties;
/// 4. or ClassDB alone when nothing up the chain is seeded.
///
/// Examples: `editor_schema("Node2D")` returns the 27-property `Node2D` schema;
/// `editor_schema("Sprite2D")` returns Sprite2D's own 39-property schema;
/// `editor_schema("AudioStreamPlayer")` returns `Node`'s golden plus its own
/// registered `stream`/`volume_db`/…; `editor_schema("AStarGrid2D")`, whose
/// `RefCounted` base no golden covers, returns its ClassDB properties.
pub fn editor_schema(class_name: &str) -> Option<Vec<EditorProp>> {
    // An exact golden is the transcribed truth for this class — return it
    // untouched. ClassDB has different defaults and no hints, so mixing
    // anything in here would move the oracle-parity numbers the goldens exist
    // to defend.
    if let Some(schema) = seeded_schema(class_name) {
        return Some(schema);
    }

    // Nearest seeded ancestor, exactly as before.
    let mut base: Option<Vec<EditorProp>> = None;
    let mut cur = class_name.to_string();
    for _ in 0..64 {
        let Some(parent) = parent_of(&cur) else {
            break;
        };
        cur = parent.to_string();
        if let Some(schema) = seeded_schema(&cur) {
            base = Some(schema);
            break;
        }
    }

    // …plus whatever ClassDB knows about THIS class that the ancestor schema
    // does not already carry (pat-s3rrv.57).
    let extra = classdb_schema(class_name).unwrap_or_default();
    match base {
        Some(mut base) => {
            if !extra.is_empty() {
                let seen: std::collections::HashSet<String> =
                    base.iter().map(|p| p.name.to_string()).collect();
                base.extend(extra.into_iter().filter(|p| !seen.contains(&*p.name)));
            }
            Some(base)
        }
        // No seeded ancestor at all: 435 registered classes are in this bucket,
        // mostly Resources no captured scene contained. They previously fell
        // through to stored-only enumeration, which shows whatever the `.tscn`
        // happened to save and nothing else.
        None if !extra.is_empty() => Some(extra),
        None => None,
    }
}

/// The schema `gdobject::class_db` implies for `class_name`, or `None` when the
/// class is unregistered or carries no properties (pat-s3rrv.57).
///
/// # Why the two registries are merged rather than one replacing the other
///
/// They answer different questions. The goldens carry Godot's *inspector*
/// truth — property order, hints, and the value the inspector SHOWS, which for
/// a few properties is a context-derived display value rather than the class
/// default. ClassDB carries the *runtime* truth, and knows an order of
/// magnitude more classes. Neither is a superset, so the golden wins wherever
/// both speak and ClassDB fills the rest.
///
/// ⚠️ `get_property_list` already walks the inheritance chain base-first, so
/// this is the flattened list `editor_schema` promises, not just the leaf's own
/// properties.
fn classdb_schema(class_name: &str) -> Option<Vec<EditorProp>> {
    if !gdobject::class_db::class_exists(class_name) {
        return None;
    }
    let props = gdobject::class_db::get_property_list(class_name);
    if props.is_empty() {
        return None;
    }
    Some(
        props
            .into_iter()
            .map(|p| {
                let type_name = classdb_type_name(&p);
                let default = p.default_value.clone();
                EditorProp::new(
                    p.name,
                    type_name,
                    p.hint,
                    p.hint_string,
                    default,
                    p.default_value,
                )
            })
            .collect(),
    )
}

/// The Godot type name for a registered property.
///
/// `VariantType`'s `Display` is Godot's own vocabulary (`int`, `float`, `AABB`,
/// `RID`), which is the same spelling the generated table uses, so the two
/// sources describe a type identically.
///
/// ⚠️ A `Nil` default carrying a `hint_string` is an OBJECT slot, not an absent
/// value: that is how a registered object-typed property spells "no instance
/// yet", and the generated table calls those `"Object"`. Reporting `"Nil"`
/// would make the inspector render an empty cell where a resource picker
/// belongs.
fn classdb_type_name(prop: &gdobject::class_db::PropertyInfo) -> Cow<'static, str> {
    match &prop.default_value {
        Variant::Nil if !prop.hint_string.is_empty() => Cow::Borrowed("Object"),
        other => Cow::Owned(other.variant_type().to_string()),
    }
}

/// The classes that have a seeded flattened editor schema. The table is
/// generated from the Godot 4.6.1 goldens by `tools/oracle/gen_inspector_schema.py`
/// (see `inspector_schema_generated.rs`) — every class in the captured oracle
/// scenes, plus the common node/resource classes no scene contained, whose
/// schema is synthesized from the vendored `doc/classes/*.xml` (pat-0q2kx.128).
fn seeded_schema(class_name: &str) -> Option<Vec<EditorProp>> {
    crate::inspector_schema_generated::generated_schema(class_name)
}

/// True if `class_name` is `ancestor` or inherits from it (walking [`parent_of`]).
pub fn class_inherits(class_name: &str, ancestor: &str) -> bool {
    let mut cur = class_name;
    if cur == ancestor {
        return true;
    }
    for _ in 0..64 {
        match parent_of(cur) {
            Some(p) => {
                if p == ancestor {
                    return true;
                }
                cur = p;
            }
            None => return false,
        }
    }
    false
}

/// Immediate parent class, for inheritance resolution. Covers every class that
/// appears in the captured Godot editor goldens plus their ancestors up to
/// `Object`, so any oracle node resolves to its nearest seeded base. Derived
/// from `fixtures/editor_oracle/godot_extension_api.json` (`inherits`).
pub fn parent_of(class_name: &str) -> Option<&'static str> {
    const PARENTS: &[(&str, &str)] = &[
        ("AnimationMixer", "Node"),
        ("AnimationPlayer", "AnimationMixer"),
        ("Area2D", "CollisionObject2D"),
        ("BoneAttachment3D", "Node3D"),
        ("BoxContainer", "Container"),
        ("CSGBox3D", "CSGPrimitive3D"),
        ("CSGCombiner3D", "CSGShape3D"),
        ("CSGCylinder3D", "CSGPrimitive3D"),
        ("CSGPrimitive3D", "CSGShape3D"),
        ("CSGShape3D", "GeometryInstance3D"),
        ("CSGSphere3D", "CSGPrimitive3D"),
        ("Camera2D", "Node2D"),
        ("Camera3D", "Node3D"),
        ("CanvasItem", "Node"),
        ("CharacterBody2D", "PhysicsBody2D"),
        ("CollisionObject2D", "Node2D"),
        ("CollisionObject3D", "Node3D"),
        ("CollisionShape2D", "Node2D"),
        ("CollisionShape3D", "Node3D"),
        ("Container", "Control"),
        ("Control", "CanvasItem"),
        ("DirectionalLight3D", "Light3D"),
        ("FogVolume", "VisualInstance3D"),
        ("GeometryInstance3D", "VisualInstance3D"),
        ("Label", "Control"),
        ("Light3D", "VisualInstance3D"),
        ("Marker3D", "Node3D"),
        ("MeshInstance3D", "GeometryInstance3D"),
        ("Node", "Object"),
        ("Node2D", "CanvasItem"),
        ("Node3D", "Node"),
        ("OmniLight3D", "Light3D"),
        ("PhysicsBody2D", "CollisionObject2D"),
        ("PhysicsBody3D", "CollisionObject3D"),
        ("ReflectionProbe", "VisualInstance3D"),
        ("RigidBody3D", "PhysicsBody3D"),
        ("Skeleton3D", "Node3D"),
        ("SpotLight3D", "Light3D"),
        ("Sprite2D", "Node2D"),
        ("StaticBody2D", "PhysicsBody2D"),
        ("StaticBody3D", "PhysicsBody3D"),
        ("TileMap", "Node2D"),
        ("Timer", "Node"),
        ("VBoxContainer", "BoxContainer"),
        ("Viewport", "Node"),
        ("VisualInstance3D", "Node3D"),
        ("Window", "Viewport"),
        ("WorldEnvironment", "Node"),
    ];
    PARENTS
        .iter()
        .find(|(c, _)| *c == class_name)
        .map(|(_, p)| *p)
        // Curated table first, then every `inherits` edge Godot declares
        // (pat-0q2kx.128). The curated list only covers classes the oracle
        // scenes happened to contain, so without this fallback a class like
        // `Line2D` or `Button` returned None and `editor_schema` gave up
        // instead of walking to its nearest seeded base.
        .or_else(|| crate::inspector_schema_generated::generated_parent_of(class_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(schema: &[EditorProp]) -> Vec<&str> {
        schema.iter().map(|p| p.name.as_ref()).collect()
    }

    #[test]
    fn seeded_base_classes_have_expected_shapes() {
        assert_eq!(editor_schema("Node").unwrap().len(), 8);
        assert_eq!(editor_schema("Node2D").unwrap().len(), 27);
        assert_eq!(editor_schema("Node3D").unwrap().len(), 16);
    }

    #[test]
    fn node2d_schema_matches_godot_order_and_defaults() {
        let s = editor_schema("Node2D").unwrap();
        assert_eq!(names(&s)[0], "process_mode");
        assert_eq!(names(&s).last().copied(), Some("script"));
        // Editor-synthesized transform is present with the type-zero default.
        let pos = s.iter().find(|p| p.name == "position").unwrap();
        assert_eq!(pos.type_name, "Vector2");
        assert!(matches!(&pos.default, Variant::Vector2(v) if v.x == 0.0 && v.y == 0.0));
        // CanvasItem defaults come through: modulate = opaque white, visible = true.
        let modulate = s.iter().find(|p| p.name == "modulate").unwrap();
        assert!(matches!(&modulate.default, Variant::Color(c) if c.r == 1.0 && c.a == 1.0));
        let visible = s.iter().find(|p| p.name == "visible").unwrap();
        assert!(matches!(&visible.default, Variant::Bool(true)));
    }

    #[test]
    fn node3d_defaults_match_godot() {
        let s = editor_schema("Node3D").unwrap();
        let scale = s.iter().find(|p| p.name == "scale").unwrap();
        assert!(
            matches!(&scale.default, Variant::Vector3(v) if v.x == 1.0 && v.y == 1.0 && v.z == 1.0)
        );
        let order = s.iter().find(|p| p.name == "rotation_order").unwrap();
        assert!(matches!(&order.default, Variant::Int(2)));
    }

    #[test]
    fn true_default_models_godot_classdb_default_not_shown_value() {
        // pat-ghfv0.41: `is_default` must reflect Godot's real ClassDB default
        // (`ClassDB.class_get_property_default_value`), which for a few props
        // differs from the value the inspector shows.
        //
        // Camera3D.current: every captured camera is `current=true` (so the shown
        // default is true, keeping value-parity), but the ClassDB default is
        // false — a current camera is therefore an override, not a default.
        let cam = editor_schema("Camera3D").unwrap();
        let current = cam.iter().find(|p| p.name == "current").unwrap();
        assert!(matches!(&current.default, Variant::Bool(true)));
        assert!(matches!(&current.true_default, Variant::Bool(false)));

        // CollisionShape3D.debug_color: the shown value is the project-settings
        // color (value-parity), but the true ClassDB default is transparent black.
        let cs = editor_schema("CollisionShape3D").unwrap();
        let dc = cs.iter().find(|p| p.name == "debug_color").unwrap();
        assert!(matches!(&dc.true_default, Variant::Color(c)
            if c.r == 0.0 && c.g == 0.0 && c.b == 0.0 && c.a == 0.0));

        // A single-golden-sample transform (Marker3D.position) must take the
        // origin as its true default, not the one captured (overridden) value.
        let m = editor_schema("Marker3D").unwrap();
        let pos = m.iter().find(|p| p.name == "position").unwrap();
        assert!(matches!(&pos.true_default, Variant::Vector3(v)
            if v.x == 0.0 && v.y == 0.0 && v.z == 0.0));

        // FogVolume.scale: true default is the unit vector, not the captured (20,4,20).
        let fog = editor_schema("FogVolume").unwrap();
        let scale = fog.iter().find(|p| p.name == "scale").unwrap();
        assert!(matches!(&scale.true_default, Variant::Vector3(v)
            if v.x == 1.0 && v.y == 1.0 && v.z == 1.0));
    }

    #[test]
    fn leaf_classes_are_directly_seeded_with_full_lists() {
        // Fill phase seeds every oracle class with its OWN full flattened list.
        assert_eq!(editor_schema("Sprite2D").unwrap().len(), 39);
        assert_eq!(editor_schema("MeshInstance3D").unwrap().len(), 38);
        assert_eq!(editor_schema("Camera3D").unwrap().len(), 31);
        assert_eq!(editor_schema("DirectionalLight3D").unwrap().len(), 44);
        assert_eq!(editor_schema("Control").unwrap().len(), 72);
        assert_eq!(editor_schema("Label").unwrap().len(), 96);
        assert_eq!(editor_schema("VBoxContainer").unwrap().len(), 74);
        assert_eq!(editor_schema("Timer").unwrap().len(), 13);
    }

    #[test]
    fn uncaptured_class_resolves_to_nearest_seeded_ancestor() {
        // Path2D is not captured; it should resolve up to Node2D (27 props).
        assert!(parent_of("Path2D").is_none()); // not in the parent table either
                                                // but a known intermediate resolves:
        let control: std::collections::HashSet<String> = editor_schema("Control")
            .unwrap()
            .iter()
            .map(|p| p.name.to_string())
            .collect();
        let boxes: std::collections::HashSet<String> = editor_schema("BoxContainer")
            .unwrap()
            .iter()
            .map(|p| p.name.to_string())
            .collect();
        // ⚠️ Deliberately a subset check, not `len() == 72`. `editor_schema`
        // now merges in whatever `gdobject::class_db` knows about the class
        // (pat-s3rrv.57), and ClassDB is a global registry another test in this
        // binary may have populated first — an exact count would depend on test
        // ORDER. What must always hold is that the ancestor schema survives.
        assert!(
            control.is_subset(&boxes),
            "BoxContainer must still carry every Control property"
        );
    }

    #[test]
    fn class_inherits_walks_the_chain() {
        // Container-derived (for Control layout_mode = 2).
        assert!(class_inherits("VBoxContainer", "Container"));
        assert!(class_inherits("VBoxContainer", "Control"));
        // Control-derived but NOT a Container (layout_mode = 0).
        assert!(class_inherits("Label", "Control"));
        assert!(!class_inherits("Label", "Container"));
        // Self.
        assert!(class_inherits("Control", "Control"));
        // Unrelated / non-Control parent (scene-root Control → layout_mode = 3).
        assert!(!class_inherits("Window", "Control"));
        assert!(!class_inherits("Node2D", "Control"));
    }

    #[test]
    fn unknown_class_has_no_schema() {
        assert!(editor_schema("TotallyMadeUpClass").is_none());
        // Object itself is above the seeded bases and has no schema.
        assert!(editor_schema("Object").is_none());
    }
}
