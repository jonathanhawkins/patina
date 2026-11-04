//! Resource saver traits and implementations.
//!
//! Provides the [`ResourceSaver`] trait and a [`TresSaver`] that writes
//! Godot's `.tres` text resource format.

use std::fmt::Write as FmtWrite;
use std::sync::Arc;

use gdcore::error::{EngineError, EngineResult};
use gdvariant::Variant;

use crate::resource::Resource;

/// Trait for saving resources to a path.
pub trait ResourceSaver {
    /// Saves a resource to the given file path.
    fn save(&self, resource: &Arc<Resource>, path: &str) -> EngineResult<()>;
}

/// Writes Godot `.tres` (text resource) files.
#[derive(Debug, Default)]
pub struct TresSaver;

impl TresSaver {
    /// Creates a new saver.
    pub fn new() -> Self {
        Self
    }

    /// Serializes a resource to a `.tres`-format string.
    pub fn save_to_string(&self, resource: &Resource) -> EngineResult<String> {
        let mut out = String::new();

        // [gd_resource] header
        write!(out, "[gd_resource type=\"{}\"", resource.class_name)
            .map_err(|e| EngineError::Parse(e.to_string()))?;
        if resource.uid.is_valid() {
            // Godot base-34 encoding; id_to_text already includes the `uid://` prefix.
            write!(
                out,
                " uid=\"{}\"",
                crate::uid::id_to_text(resource.uid.raw())
            )
            .map_err(|e| EngineError::Parse(e.to_string()))?;
        }
        writeln!(out, " format=3]").map_err(|e| EngineError::Parse(e.to_string()))?;

        // [ext_resource] sections
        if !resource.ext_resources.is_empty() {
            writeln!(out).map_err(|e| EngineError::Parse(e.to_string()))?;
            let mut ext_ids: Vec<_> = resource.ext_resources.keys().collect();
            ext_ids.sort();
            for id in ext_ids {
                let ext = &resource.ext_resources[id];
                // ⚠️ The `uid` attribute is OMITTED when we have none, not
                // written as `uid=""` (pat-fk17e.1). Godot's
                // `resource_format_text.cpp` only emits the attribute for a
                // resource that actually carries a UID, and `uid=""` is not
                // merely untidy — it is a claim. `ResourceUID` treats an empty
                // text id as invalid, so a re-save of a file whose UID we never
                // knew would stamp an attribute asserting "this reference has
                // no UID", which is different from "we did not record one".
                let uid_attr = if ext.uid.is_empty() {
                    String::new()
                } else {
                    format!(" uid=\"{}\"", ext.uid)
                };
                writeln!(
                    out,
                    "[ext_resource type=\"{}\"{} path=\"{}\" id=\"{}\"]",
                    ext.resource_type, uid_attr, ext.path, ext.id
                )
                .map_err(|e| EngineError::Parse(e.to_string()))?;
            }
        }

        // [sub_resource] sections
        if !resource.subresources.is_empty() {
            let mut sub_ids: Vec<_> = resource.subresources.keys().collect();
            sub_ids.sort();
            for id in sub_ids {
                let sub = &resource.subresources[id];
                writeln!(out).map_err(|e| EngineError::Parse(e.to_string()))?;
                writeln!(
                    out,
                    "[sub_resource type=\"{}\" id=\"{}\"]",
                    sub.class_name, id
                )
                .map_err(|e| EngineError::Parse(e.to_string()))?;
                write_properties(&mut out, sub)?;
            }
        }

        // [resource] section
        if resource.property_count() > 0 {
            writeln!(out).map_err(|e| EngineError::Parse(e.to_string()))?;
            writeln!(out, "[resource]").map_err(|e| EngineError::Parse(e.to_string()))?;
            write_properties(&mut out, resource)?;
        }

        Ok(out)
    }

    /// Serializes a resource preserving original subresource IDs (no renumbering).
    ///
    /// This is identical to `save_to_string` — the default serializer already
    /// preserves original IDs. The `_raw` variant exists to contrast with a
    /// hypothetical renumbered output.
    pub fn save_to_string_raw(&self, resource: &Resource) -> EngineResult<String> {
        self.save_to_string(resource)
    }
}

impl ResourceSaver for TresSaver {
    fn save(&self, resource: &Arc<Resource>, path: &str) -> EngineResult<()> {
        let contents = self.save_to_string(resource)?;
        std::fs::write(path, contents).map_err(EngineError::Io)
    }
}

/// Writes sorted property lines for a resource section.
fn write_properties(out: &mut String, resource: &Resource) -> EngineResult<()> {
    let mut keys = resource.sorted_property_keys();
    // `script` goes FIRST, ahead of the alphabetical body (pat-fk17e.1).
    //
    // Godot's `resource_format_text.cpp` walks `get_property_list()`, where the
    // `script` slot comes from `Object` and so precedes everything the script
    // itself declares. The ordering is not cosmetic for a script-backed
    // resource: the script line is what decides which class the following lines
    // are properties *of*, so a reader that applies lines in order needs it
    // before them. Sorting put it after every exported var whose name begins
    // a-r, which is most of them.
    if let Some(pos) = keys.iter().position(|k| k.as_str() == "script") {
        let script_key = keys.remove(pos);
        keys.insert(0, script_key);
    }
    for key in keys {
        if let Some(value) = resource.get_property(key) {
            writeln!(out, "{} = {}", key, format_variant(value))
                .map_err(|e| EngineError::Parse(e.to_string()))?;
        }
    }
    Ok(())
}

/// Formats a Variant value in `.tres` syntax.
fn format_variant(v: &Variant) -> String {
    // A builtin `Variant` has no kind for was preserved verbatim at load;
    // re-emit that exact text (pat-0q2kx.136). Without this the round-trip
    // would write the preservation dictionary into the file — which is a worse
    // corruption than the drop it replaced, so it is checked before the normal
    // match.
    //
    // Since pat-s3rrv.59 the five literals that used to take this path
    // (`Vector4`, `Vector4i`, `Rect2i`, `Projection`, `PackedVector4Array`)
    // are real Variant kinds with arms below, so nothing currently reaches it.
    // It stays for the next builtin Godot adds ahead of this port.
    if let Some((_ty, literal)) = crate::loader::unparsed_literal(v) {
        return literal.to_string();
    }
    match v {
        Variant::Nil => "null".to_string(),
        Variant::Bool(b) => b.to_string(),
        Variant::Int(i) => i.to_string(),
        Variant::Float(f) => format_float(*f),
        Variant::String(s) => {
            // Resource references survive a load as PREFIXED STRINGS: the
            // loader rewrites `SubResource("id")` to `"SubResource:id"` and
            // `ExtResource("id")` to `"ExtResource:id"` (loader.rs:437-444).
            // Re-quoting them here would write `= "SubResource:id"`, which our
            // own loader still reads back — so a Patina round trip looks clean
            // — while Godot reads it as an ordinary String and the reference is
            // gone. `scene_saver::write_node_section` already re-encodes both
            // for .tscn; .tres had no such step (pat-s3rrv.154).
            if let Some(id) = s.strip_prefix("SubResource:") {
                return format!("SubResource(\"{id}\")");
            }
            if let Some(id) = s.strip_prefix("ExtResource:") {
                return format!("ExtResource(\"{id}\")");
            }
            let escaped = s
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\t', "\\t");
            format!("\"{escaped}\"")
        }
        Variant::Vector2(v) => format!("Vector2({}, {})", format_f32(v.x), format_f32(v.y)),
        Variant::Vector2i(v) => format!("Vector2i({}, {})", v.x, v.y),
        Variant::Vector3(v) => format!(
            "Vector3({}, {}, {})",
            format_f32(v.x),
            format_f32(v.y),
            format_f32(v.z)
        ),
        Variant::Vector3i(v) => format!("Vector3i({}, {}, {})", v.x, v.y, v.z),
        Variant::Vector4(v) => format!(
            "Vector4({}, {}, {}, {})",
            format_f32(v.x),
            format_f32(v.y),
            format_f32(v.z),
            format_f32(v.w)
        ),
        Variant::Vector4i(v) => format!("Vector4i({}, {}, {}, {})", v.x, v.y, v.z, v.w),
        Variant::Rect2i(r) => format!(
            "Rect2i({}, {}, {}, {})",
            r.position.x, r.position.y, r.size.x, r.size.y
        ),
        // Sixteen numbers in COLUMN-MAJOR order — the same walk the loader
        // uses, so `parse(format(p)) == p`. Emitting rows would round-trip
        // cleanly through this port while disagreeing with Godot.
        Variant::Projection(p) => format!(
            "Projection({})",
            p.m.iter()
                .flatten()
                .map(|c| format_f32(*c))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        // ⚠️ Every packed array except Vector4 fell through to the catch-all
        // below, whose `Display` writes `[1, 2, 3]` — an ARRAY literal, not the
        // `PackedInt32Array(1, 2, 3)` constructor Godot's .tres grammar needs.
        // Nothing noticed because `TresSaver` had no caller that wrote arrays
        // until pat-s3rrv.154 made it the VisualShader write path, and Patina's
        // own parser is lenient enough to read the literal back. Syntax mirrors
        // `scene_saver::format_variant_value`, which has always been correct.
        Variant::PackedByteArray(v) => format!(
            "PackedByteArray({})",
            v.iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Variant::PackedInt32Array(v) => format!(
            "PackedInt32Array({})",
            v.iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Variant::PackedInt64Array(v) => format!(
            "PackedInt64Array({})",
            v.iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Variant::PackedFloat32Array(v) => format!(
            "PackedFloat32Array({})",
            v.iter()
                .map(|x| format_f32(*x))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Variant::PackedFloat64Array(v) => format!(
            "PackedFloat64Array({})",
            v.iter()
                .map(|x| format_float(*x))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Variant::PackedStringArray(v) => format!(
            "PackedStringArray({})",
            v.iter()
                .map(|x| format!("\"{}\"", x.replace('\\', "\\\\").replace('"', "\\\"")))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Variant::PackedVector2Array(v) => format!(
            "PackedVector2Array({})",
            v.iter()
                .map(|p| format!("{}, {}", format_f32(p.x), format_f32(p.y)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Variant::PackedVector3Array(v) => format!(
            "PackedVector3Array({})",
            v.iter()
                .map(|p| format!(
                    "{}, {}, {}",
                    format_f32(p.x),
                    format_f32(p.y),
                    format_f32(p.z)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Variant::PackedVector4Array(v) => format!(
            "PackedVector4Array({})",
            v.iter()
                .flat_map(|p| [
                    format_f32(p.x),
                    format_f32(p.y),
                    format_f32(p.z),
                    format_f32(p.w)
                ])
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Variant::Color(c) => format!(
            "Color({}, {}, {}, {})",
            format_f32(c.r),
            format_f32(c.g),
            format_f32(c.b),
            format_f32(c.a)
        ),
        // Fallback for types we don't write in .tres format.
        // ⚠️ The arms below used to fall through to `other => format!("{other}")`
        // and were written with `Display`, i.e. as debug text: a Quaternion came
        // out as `(0.1, 0.2, 0.3, 0.9)` with no constructor, and AABB/Basis/
        // Plane/Rect2 as labelled forms like `P: (-1, -2, -3)`. Godot cannot read
        // any of it and neither can our own loader, so saving a resource holding
        // one of these types produced an unloadable file (pat-21oab).
        //
        // Element order in every arm is taken from the matching parse arm in
        // `loader.rs`, so `parse(format(v)) == v` holds by construction.
        Variant::Rect2(r) => format!(
            "Rect2({}, {}, {}, {})",
            format_f32(r.position.x),
            format_f32(r.position.y),
            format_f32(r.size.x),
            format_f32(r.size.y)
        ),
        Variant::Quaternion(q) => format!(
            "Quaternion({}, {}, {}, {})",
            format_f32(q.x),
            format_f32(q.y),
            format_f32(q.z),
            format_f32(q.w)
        ),
        // Normal first, then the signed distance (loader.rs:610).
        Variant::Plane(p) => format!(
            "Plane({}, {}, {}, {})",
            format_f32(p.normal.x),
            format_f32(p.normal.y),
            format_f32(p.normal.z),
            format_f32(p.d)
        ),
        Variant::Aabb(a) => format!(
            "AABB({}, {}, {}, {}, {}, {})",
            format_f32(a.position.x),
            format_f32(a.position.y),
            format_f32(a.position.z),
            format_f32(a.size.x),
            format_f32(a.size.y),
            format_f32(a.size.z)
        ),
        // Three ROW vectors, the same walk the loader uses (loader.rs:647) and
        // the same order as the basis embedded in Transform3D below.
        Variant::Basis(b) => format!(
            "Basis({}, {}, {}, {}, {}, {}, {}, {}, {})",
            format_f32(b.x.x),
            format_f32(b.x.y),
            format_f32(b.x.z),
            format_f32(b.y.x),
            format_f32(b.y.y),
            format_f32(b.y.z),
            format_f32(b.z.x),
            format_f32(b.z.y),
            format_f32(b.z.z)
        ),
        Variant::Transform2D(t) => format!(
            "Transform2D({}, {}, {}, {}, {}, {})",
            format_f32(t.x.x),
            format_f32(t.x.y),
            format_f32(t.y.x),
            format_f32(t.y.y),
            format_f32(t.origin.x),
            format_f32(t.origin.y)
        ),
        Variant::Transform3D(t) => format!(
            "Transform3D({}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {})",
            format_f32(t.basis.x.x),
            format_f32(t.basis.x.y),
            format_f32(t.basis.x.z),
            format_f32(t.basis.y.x),
            format_f32(t.basis.y.y),
            format_f32(t.basis.y.z),
            format_f32(t.basis.z.x),
            format_f32(t.basis.z.y),
            format_f32(t.basis.z.z),
            format_f32(t.origin.x),
            format_f32(t.origin.y),
            format_f32(t.origin.z)
        ),
        // Godot's `&"name"` literal, which the loader accepts (loader.rs:671).
        // Escaped exactly like the String arm above; `Display` wrote `&jump`,
        // unquoted, which the loader rejects outright.
        Variant::StringName(sn) => {
            let escaped = sn
                .as_str()
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\t', "\\t");
            format!("&\"{escaped}\"")
        }
        // Godot writes `NodePath("Player/Sprite2D")`. This fell through to the
        // Display impl and wrote the BARE path, which no loader — ours or
        // Godot's — can read back ("unrecognized value: Player/Sprite2D").
        Variant::NodePath(np) => {
            let escaped = np.to_string().replace('\\', "\\\\").replace('"', "\\\"");
            format!("NodePath(\"{escaped}\")")
        }
        // `{"key": value, ...}` with quoted keys, per `parse_dictionary`.
        //
        // ⚠️ SORTED by formatted key. `Dictionary` is hash-backed, so emitting
        // iteration order would make the saver non-deterministic and every .tres
        // golden unstable — a worse failure than the one this fixes.
        // ⚠️ The payload is a `SharedDictionary` (an `Arc<RwLock<..>>` handle), NOT
        // a `Dictionary`, so `iter()` is not available on it directly — reaching
        // for it is an E0599. `with` borrows the inner `Dictionary` for the
        // closure and returns whatever the closure returns.
        Variant::Dictionary(d) => d.with(|dict| {
            let mut pairs: Vec<String> = dict
                .iter()
                .map(|(k, val)| format!("{}: {}", format_variant(k), format_variant(val)))
                .collect();
            pairs.sort();
            format!("{{{}}}", pairs.join(", "))
        }),
        other => format!("{other}"),
    }
}

/// Formats an f64 without unnecessary trailing zeros, but always with
/// a decimal point to distinguish from integers.
fn format_float(f: f64) -> String {
    if f.fract() == 0.0 {
        format!("{f:.1}")
    } else {
        format!("{f}")
    }
}

/// Formats an f32 cleanly.
fn format_f32(f: f32) -> String {
    if f.fract() == 0.0 {
        format!("{f:.0}")
    } else {
        format!("{f}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::ExtResource;
    use gdcore::math::{Color, Vector2};
    use gdcore::ResourceUid;

    #[test]
    fn save_simple_resource() {
        let mut r = Resource::new("Resource");
        r.uid = ResourceUid::new(12345);
        r.set_property("name", Variant::String("Test".into()));
        r.set_property("value", Variant::Int(42));

        let saver = TresSaver::new();
        let output = saver.save_to_string(&r).unwrap();

        assert!(output.contains("[gd_resource type=\"Resource\""));
        // 12345 encodes to base-34 "kxd" (hand-derived: 12345 = 10*34² + 23*34 + 3
        // → chars 'k','x','d'), and must round-trip back to 12345.
        assert!(
            output.contains("uid=\"uid://kxd\""),
            "expected base-34 uid, got: {output}"
        );
        assert_eq!(crate::uid::text_to_id("uid://kxd").raw(), 12345);
        assert!(output.contains("[resource]"));
        assert!(output.contains("name = \"Test\""));
        assert!(output.contains("value = 42"));
    }

    #[test]
    fn save_with_vector_and_color() {
        let mut r = Resource::new("Resource");
        r.set_property("pos", Variant::Vector2(Vector2::new(10.0, 20.0)));
        r.set_property("col", Variant::Color(Color::new(0.5, 0.6, 0.7, 1.0)));

        let saver = TresSaver::new();
        let output = saver.save_to_string(&r).unwrap();

        assert!(output.contains("pos = Vector2(10, 20)"));
        assert!(output.contains("col = Color(0.5, 0.6, 0.7, 1)"));
    }

    #[test]
    fn save_empty_resource() {
        let r = Resource::new("EmptyResource");
        let saver = TresSaver::new();
        let output = saver.save_to_string(&r).unwrap();
        assert!(output.contains("[gd_resource type=\"EmptyResource\""));
        // No [resource] section since there are no properties
        assert!(!output.contains("[resource]"));
    }

    #[test]
    fn save_resource_without_uid() {
        let r = Resource::new("Resource");
        let saver = TresSaver::new();
        let output = saver.save_to_string(&r).unwrap();
        assert!(!output.contains("uid="));
    }

    #[test]
    fn save_resource_with_nil_property() {
        let mut r = Resource::new("Resource");
        r.set_property("nothing", Variant::Nil);
        let saver = TresSaver::new();
        let output = saver.save_to_string(&r).unwrap();
        assert!(output.contains("nothing = null"));
    }

    #[test]
    fn save_resource_with_bool_properties() {
        let mut r = Resource::new("Resource");
        r.set_property("flag_a", Variant::Bool(true));
        r.set_property("flag_b", Variant::Bool(false));
        let saver = TresSaver::new();
        let output = saver.save_to_string(&r).unwrap();
        assert!(output.contains("flag_a = true"));
        assert!(output.contains("flag_b = false"));
    }

    #[test]
    fn save_resource_with_float() {
        let mut r = Resource::new("Resource");
        r.set_property("pi", Variant::Float(3.14));
        r.set_property("whole", Variant::Float(5.0));
        let saver = TresSaver::new();
        let output = saver.save_to_string(&r).unwrap();
        assert!(output.contains("pi = 3.14"));
        assert!(output.contains("whole = 5.0"));
    }

    #[test]
    fn save_resource_with_escaped_string() {
        let mut r = Resource::new("Resource");
        r.set_property("text", Variant::String("line1\nline2\ttab".into()));
        let saver = TresSaver::new();
        let output = saver.save_to_string(&r).unwrap();
        assert!(output.contains(r#"text = "line1\nline2\ttab""#));
    }

    #[test]
    fn save_with_subresource() {
        let mut r = Resource::new("Resource");
        r.set_property("value", Variant::Int(1));

        let mut sub = Resource::new("StyleBoxFlat");
        sub.set_property("bg_color", Variant::Color(Color::new(0.2, 0.3, 0.4, 1.0)));
        r.subresources
            .insert("StyleBoxFlat_abc".to_string(), Arc::new(sub));

        let saver = TresSaver::new();
        let output = saver.save_to_string(&r).unwrap();

        assert!(output.contains("[sub_resource type=\"StyleBoxFlat\" id=\"StyleBoxFlat_abc\"]"));
        assert!(output.contains("bg_color = Color(0.2, 0.3, 0.4, 1)"));
    }

    /// 🔑 pat-fk17e.1 — A SCRIPT-BACKED RESOURCE WRITES ITS SCRIPT LINE FIRST.
    ///
    /// `script` sorts after every exported var whose name starts a–r, which is
    /// most of them, so the plain alphabetical body put the line that decides
    /// *which class these properties belong to* after the properties. Godot's
    /// `resource_format_text.cpp` emits it first, from `Object`'s own property
    /// slot.
    ///
    /// ⚠️ Asserts on the ORDER, with an index comparison, not on both lines
    /// being present. `contains` for each line passes under either ordering —
    /// it is invariant under exactly the swap this guards.
    #[test]
    fn script_backed_resource_writes_script_line_before_exported_vars() {
        let mut r = Resource::new("Resource");
        // `armour` and `damage` both sort BEFORE `script`.
        r.set_property("armour", Variant::Int(2));
        r.set_property("damage", Variant::Int(7));
        r.set_property("script", Variant::String("ExtResource:1_item".into()));
        r.ext_resources.insert(
            "1_item".to_string(),
            ExtResource {
                resource_type: "Script".to_string(),
                uid: String::new(),
                path: "res://item.gd".to_string(),
                id: "1_item".to_string(),
            },
        );

        let output = TresSaver::new().save_to_string(&r).unwrap();

        let script_at = output
            .find("script = ExtResource(\"1_item\")")
            .expect("the script reference is re-emitted as an ExtResource call");
        let damage_at = output
            .find("damage = 7")
            .expect("exported vars are written");
        let armour_at = output
            .find("armour = 2")
            .expect("exported vars are written");
        assert!(
            script_at < damage_at && script_at < armour_at,
            "`script` must precede the exported vars; got:\n{output}"
        );
    }

    /// 🔑 pat-fk17e.1 — AN UNKNOWN UID IS AN ABSENT ATTRIBUTE, NOT `uid=""`.
    ///
    /// `uid=""` is a claim, not a blank: `ResourceUID` treats an empty text id
    /// as invalid, so re-saving a file whose UID was never recorded would stamp
    /// an attribute asserting the reference *has* no UID.
    #[test]
    fn ext_resource_without_a_uid_omits_the_attribute() {
        let mut r = Resource::new("Resource");
        r.set_property("value", Variant::Int(1));
        r.ext_resources.insert(
            "1_item".to_string(),
            ExtResource {
                resource_type: "Script".to_string(),
                uid: String::new(),
                path: "res://item.gd".to_string(),
                id: "1_item".to_string(),
            },
        );
        let output = TresSaver::new().save_to_string(&r).unwrap();
        assert!(
            output.contains("[ext_resource type=\"Script\" path=\"res://item.gd\" id=\"1_item\"]"),
            "no uid recorded => no uid attribute; got:\n{output}"
        );
        assert!(
            !output.contains("uid=\"\""),
            "an empty uid attribute must never be written; got:\n{output}"
        );

        // A UID we DO have still round-trips — the omission is conditional, not
        // a removal.
        r.ext_resources.get_mut("1_item").unwrap().uid = "uid://abc123".to_string();
        let output = TresSaver::new().save_to_string(&r).unwrap();
        assert!(
            output.contains(
                "[ext_resource type=\"Script\" uid=\"uid://abc123\" path=\"res://item.gd\" id=\"1_item\"]"
            ),
            "a known uid is still emitted; got:\n{output}"
        );
    }
}
