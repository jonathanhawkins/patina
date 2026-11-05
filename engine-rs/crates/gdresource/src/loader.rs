//! Resource loader traits and implementations.
//!
//! Provides the [`ResourceLoader`] trait and a [`TresLoader`] that parses
//! Godot's `.tres` text resource format (simplified subset).

use std::collections::HashMap;
use std::sync::Arc;

use gdcore::error::{EngineError, EngineResult};
use gdcore::math::{
    Color, Rect2, Rect2i, Transform2D, Vector2, Vector2i, Vector3, Vector3i, Vector4, Vector4i,
};
use gdcore::math3d::{Aabb, Basis, Plane, Projection, Quaternion, Transform3D};
use gdcore::node_path::NodePath;
use gdcore::string_name::StringName;
use gdcore::ResourceUid;
use gdvariant::{ContainerType, Dictionary, SharedArray, SharedDictionary, Variant};

use crate::resource::{ExtResource, Resource};

/// Trait for loading resources from a path.
pub trait ResourceLoader {
    /// Loads a resource from the given path.
    fn load(&self, path: &str) -> EngineResult<Arc<Resource>>;
}

/// Loads Godot `.tres` (text resource) files.
///
/// This is a simplified parser that handles the most common cases:
/// `[gd_resource]`, `[ext_resource]`, `[sub_resource]`, and `[resource]`
/// sections, with property values for strings, ints, floats, booleans,
/// `Vector2()`, `Vector3()`, and `Color()`.
#[derive(Debug, Default)]
pub struct TresLoader;

impl TresLoader {
    /// Creates a new loader.
    pub fn new() -> Self {
        Self
    }

    /// Parses a `.tres` file from its text contents.
    pub fn parse_str(&self, source: &str, path: &str) -> EngineResult<Arc<Resource>> {
        let mut resource = Resource::new("Resource");
        resource.path = path.to_string();

        let mut current_section = Section::None;
        let mut current_sub_id = String::new();
        let mut current_sub = Option::<Resource>::None;
        // A property whose value opens more brackets than it closes continues
        // onto the following lines. Godot's text writer puts every Dictionary
        // and Array on several lines (`variant_parser.cpp` `write`), so an
        // `AnimationLibrary`'s `_data` or an `Animation`'s `tracks/N/keys`
        // arrives split. Without buffering, the first line's value is the bare
        // `{`, `parse_variant_value` rejects it, and the `?` below failed the
        // WHOLE file — so no `.tres` the editor actually wrote could be loaded
        // at all (pat-mgixk.1). `packed_scene.rs` has buffered `.tscn` values
        // this way since pat-nzgdu.78; this is the same rule for `.tres`.
        let mut pending_prop = Option::<(String, String)>::None;

        for (line_num, raw_line) in source.lines().enumerate() {
            let line_num = line_num + 1; // 1-based
            let line = raw_line.trim();

            // Inside a multi-line STRING the line is the string's own text:
            // kept verbatim, blank lines included, so this runs before the
            // empty-line skip below.
            if let Some((key, mut acc)) = pending_prop.take() {
                if ends_inside_string(&acc) {
                    acc.push('\n');
                    acc.push_str(raw_line);
                    if value_continues(&acc) {
                        pending_prop = Some((key, acc));
                        continue;
                    }
                    let value = parse_variant_value(acc.trim()).map_err(|e| {
                        EngineError::Parse(format!("{} (at line {} in {})", e, line_num, path))
                    })?;
                    store_property(
                        &mut resource,
                        &mut current_sub,
                        current_section,
                        &key,
                        value,
                        line_num,
                        path,
                    )?;
                    continue;
                }
                pending_prop = Some((key, acc));
            }

            // Skip empty lines and comments.
            if line.is_empty() || line.starts_with(';') {
                continue;
            }

            // Continuation of a multi-line value opened above.
            //
            // Checked BEFORE the section-header branch so a `}` or `]` sitting
            // alone on its own line is not mistaken for a `[...]` header — a
            // closing `]` is exactly that shape.
            if let Some((key, mut acc)) = pending_prop.take() {
                acc.push('\n');
                acc.push_str(line);
                if value_continues(&acc) {
                    pending_prop = Some((key, acc));
                    continue;
                }
                let value = parse_variant_value(acc.trim()).map_err(|e| {
                    EngineError::Parse(format!("{} (at line {} in {})", e, line_num, path))
                })?;
                store_property(
                    &mut resource,
                    &mut current_sub,
                    current_section,
                    &key,
                    value,
                    line_num,
                    path,
                )?;
                continue;
            }

            // Section headers.
            if line.starts_with('[') && line.ends_with(']') {
                // Flush any pending sub-resource.
                if let Some(sub) = current_sub.take() {
                    resource
                        .subresources
                        .insert(current_sub_id.clone(), Arc::new(sub));
                }

                let inner = &line[1..line.len() - 1];
                if inner.starts_with("gd_resource") {
                    current_section = Section::GdResource;
                    parse_gd_resource_header(inner, &mut resource)?;
                } else if inner.starts_with("ext_resource") {
                    current_section = Section::ExtResource;
                    let ext = parse_ext_resource_header(inner)?;
                    resource.ext_resources.insert(ext.id.clone(), ext);
                } else if inner.starts_with("sub_resource") {
                    current_section = Section::SubResource;
                    let (type_name, id) = parse_sub_resource_header(inner)?;
                    current_sub_id = id;
                    current_sub = Some(Resource::new(type_name));
                } else if inner == "resource" {
                    current_section = Section::Resource;
                } else {
                    current_section = Section::Unknown;
                }
                continue;
            }

            // Property lines: key = value
            if let Some((key, value_str)) = line.split_once('=') {
                let key = key.trim().to_string();
                let value_str = value_str.trim();
                if value_continues(value_str) {
                    pending_prop = Some((key, value_str.to_string()));
                    continue;
                }
                let value = parse_variant_value(value_str).map_err(|e| {
                    EngineError::Parse(format!("{} (at line {} in {})", e, line_num, path))
                })?;
                store_property(
                    &mut resource,
                    &mut current_sub,
                    current_section,
                    &key,
                    value,
                    line_num,
                    path,
                )?;
            }
        }

        // Flush final sub-resource.
        if let Some(sub) = current_sub.take() {
            resource.subresources.insert(current_sub_id, Arc::new(sub));
        }

        Ok(Arc::new(resource))
    }
}

impl ResourceLoader for TresLoader {
    fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
        let contents = std::fs::read_to_string(path).map_err(EngineError::Io)?;
        self.parse_str(&contents, path)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Stores one parsed property into whichever section is currently open.
///
/// Shared by the single-line and the buffered multi-line path so the two
/// cannot drift — before this existed only the single-line path stored
/// anything, which is why a multi-line value was a hard parse error.
fn store_property(
    resource: &mut Resource,
    current_sub: &mut Option<Resource>,
    section: Section,
    key: &str,
    value: Variant,
    line_num: usize,
    path: &str,
) -> EngineResult<()> {
    match section {
        Section::Resource => resource.set_property(key, value),
        Section::SubResource => {
            if let Some(sub) = current_sub.as_mut() {
                sub.set_property(key, value);
            }
        }
        // A property before any section means the file has no
        // `[gd_resource]`/`[resource]` header — not a valid .tres.
        Section::None => {
            return Err(EngineError::Parse(format!(
                "property '{key}' outside any section (missing [gd_resource]/[resource] header) at line {line_num} in {path}"
            )));
        }
        _ => {}
    }
    Ok(())
}

/// Whether a property value that ends here continues onto the next line of a
/// `.tscn`/`.tres`: it opens more brackets than it closes, OR it ends inside
/// an unterminated string.
///
/// 🔑 Godot writes a newline inside a string LITERALLY — a Label's
/// `text = "Dodge the` then `Creeps"` on the next line (the official demo's
/// HUD). Balancing only brackets read the first line alone, failed to parse it,
/// and dropped the property, so the label came up empty. Shared by this loader
/// and `gdscene::packed_scene` so the two parsers cannot drift.
pub fn value_continues(s: &str) -> bool {
    let (depth, in_string) = scan_value(s);
    depth > 0 || in_string
}

/// Whether `s` ends inside an open string literal — the next line is then the
/// string's own text, joined by the newline it really contains and kept
/// verbatim (not trimmed, and kept even when blank).
pub fn ends_inside_string(s: &str) -> bool {
    scan_value(s).1
}

/// Net bracket/paren/brace depth of `s`, ignoring anything inside a string.
///
/// ⚠️ The string tracking is not optional. A `"{"` or a Windows path inside a
/// quoted value would otherwise skew the depth and swallow the rest of the
/// file. Mirrors `packed_scene.rs`'s scanner of the same name.
#[cfg(test)]
fn bracket_depth(s: &str) -> i32 {
    scan_value(s).0
}

/// `(bracket depth, ends inside a string)` for a property value.
fn scan_value(s: &str) -> (i32, bool) {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for c in s.chars() {
        if escape {
            escape = false;
            continue;
        }
        if in_string {
            match c {
                '\\' => escape = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ => {}
        }
    }
    (depth, in_string)
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Section {
    /// No section opened yet (before any `[...]` header).
    None,
    GdResource,
    ExtResource,
    SubResource,
    Resource,
    /// A recognized `[...]` header that is not one Patina models; its
    /// properties are ignored rather than treated as a malformed stray.
    Unknown,
}

/// Extracts key="value" pairs from a section header string.
fn extract_header_attrs(header: &str) -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    let mut remaining = header;

    // Skip the section name (first word).
    if let Some(idx) = remaining.find(' ') {
        remaining = &remaining[idx..];
    } else {
        return attrs;
    }

    // Parse key="value" pairs.
    while let Some(eq_idx) = remaining.find('=') {
        let key = remaining[..eq_idx].trim();
        remaining = &remaining[eq_idx + 1..];

        if remaining.starts_with('"') {
            remaining = &remaining[1..];
            if let Some(end_quote) = remaining.find('"') {
                let value = &remaining[..end_quote];
                attrs.insert(key.to_string(), value.to_string());
                remaining = &remaining[end_quote + 1..];
            } else {
                break;
            }
        } else {
            // Unquoted value — read until space or end.
            let end = remaining.find(' ').unwrap_or(remaining.len());
            let value = &remaining[..end];
            attrs.insert(key.to_string(), value.to_string());
            remaining = &remaining[end..];
        }
    }

    attrs
}

/// Parses the `[gd_resource ...]` header.
fn parse_gd_resource_header(header: &str, resource: &mut Resource) -> EngineResult<()> {
    let attrs = extract_header_attrs(header);

    if let Some(type_name) = attrs.get("type") {
        resource.class_name = type_name.clone();
    }
    if let Some(uid_str) = attrs.get("uid") {
        resource.uid = parse_uid_string(uid_str);
    }

    Ok(())
}

/// Parses a `[ext_resource ...]` header into an [`ExtResource`].
fn parse_ext_resource_header(header: &str) -> EngineResult<ExtResource> {
    let attrs = extract_header_attrs(header);

    Ok(ExtResource {
        resource_type: attrs.get("type").cloned().unwrap_or_default(),
        uid: attrs.get("uid").cloned().unwrap_or_default(),
        path: attrs.get("path").cloned().unwrap_or_default(),
        id: attrs.get("id").cloned().unwrap_or_default(),
    })
}

/// Parses a `[sub_resource ...]` header, returning `(type, id)`.
fn parse_sub_resource_header(header: &str) -> EngineResult<(String, String)> {
    let attrs = extract_header_attrs(header);
    let type_name = attrs.get("type").cloned().unwrap_or_default();
    let id = attrs.get("id").cloned().unwrap_or_default();
    Ok((type_name, id))
}

/// Converts a `uid://...` string into a [`ResourceUid`].
///
/// Uses Godot's base-34 codec ([`crate::uid::text_to_id`]), so the numeric UID
/// matches the value Godot itself computes. Returns [`ResourceUid::INVALID`] for
/// strings without a `uid://` prefix, the `uid://<invalid>` sentinel, or any
/// character outside `[a-z0-9]`.
pub fn parse_uid_string(uid_str: &str) -> ResourceUid {
    crate::uid::text_to_id(uid_str)
}

/// Parses a single property value string into a [`Variant`].
/// Builtin literals `Variant` has no kind for, preserved verbatim by
/// [`parse_variant_value`] instead of being dropped or retyped (pat-0q2kx.136).
///
/// **Now empty.** It used to hold `Vector4`, `Vector4i`, `Rect2i`, `Projection`
/// and `PackedVector4Array` — the five literals `Variant` could not represent,
/// which were kept as tagged dictionaries so the saver could re-emit the text
/// verbatim while nothing could actually *read* them. pat-s3rrv.59 gave all
/// five real Variant kinds, so they now parse into values like every other
/// builtin and the placeholder is no longer reached.
///
/// The constant and its keys stay because the mechanism is the right answer for
/// the *next* builtin Godot adds ahead of this port: preserving a literal
/// losslessly beats dropping it. An empty list simply means nothing is
/// currently in that position.
pub const UNPARSED_BUILTINS: &[&str] = &[];

/// Dictionary key naming the builtin a preserved literal came from.
pub const UNPARSED_TYPE_KEY: &str = "__gd_unparsed_type__";
/// Dictionary key holding the literal exactly as it was authored.
pub const UNPARSED_LITERAL_KEY: &str = "__gd_unparsed_literal__";
/// Dictionary key holding the literal's parsed numeric components.
pub const UNPARSED_ARGS_KEY: &str = "__gd_unparsed_args__";
/// Dictionary key naming the class of a parsed `Object(...)` literal.
pub const UNPARSED_CLASS_KEY: &str = "__gd_object_class__";

/// If `v` is a preserved builtin literal, returns `(type_name, literal_text)`.
///
/// The saver uses this to re-emit the original text; a consumer uses it to tell
/// "this really was a `Rect2i`" from "someone stored a dictionary".
pub fn unparsed_literal(v: &Variant) -> Option<(String, String)> {
    let Variant::Dictionary(d) = v else {
        return None;
    };
    let (Some(Variant::String(ty)), Some(Variant::String(lit))) =
        (d.get(UNPARSED_TYPE_KEY), d.get(UNPARSED_LITERAL_KEY))
    else {
        return None;
    };
    Some((ty, lit))
}

/// If `v` is a parsed `Object(Class, ...)` literal, returns its class name.
pub fn object_literal_class(v: &Variant) -> Option<String> {
    let Variant::Dictionary(d) = v else {
        return None;
    };
    match d.get(UNPARSED_CLASS_KEY) {
        Some(Variant::String(c)) => Some(c),
        _ => None,
    }
}

/// Builds a column-stored [`Basis`] from the nine numbers of a Godot text
/// `Basis(...)` / `Transform3D(...)` literal, which are ROW-major
/// (`rows[0].x, rows[0].y, rows[0].z, rows[1].x, ...`). See pat-sazee.
pub fn basis_from_godot_rows(r: [f32; 9]) -> Basis {
    Basis {
        x: Vector3::new(r[0], r[3], r[6]),
        y: Vector3::new(r[1], r[4], r[7]),
        z: Vector3::new(r[2], r[5], r[8]),
    }
}

/// The inverse of [`basis_from_godot_rows`]: the nine ROW-major numbers Godot
/// writes for a column-stored [`Basis`].
pub fn basis_to_godot_rows(b: &Basis) -> [f32; 9] {
    [
        b.x.x, b.y.x, b.z.x, //
        b.x.y, b.y.y, b.z.y, //
        b.x.z, b.y.z, b.z.z,
    ]
}

/// Parses a value, additionally reporting a typed array's element type *as a
/// name*.
///
/// ⚠️ The note that used to sit here said `parse_variant_value` returns a
/// *plain* array for `Array[int](…)` so the annotation had to be offered
/// separately (pat-0q2kx.136). That is no longer true: as of pat-s3rrv.265 the
/// array itself carries the element type, so `parse_variant_value` alone is
/// enough for a caller that wants typed data, and this function is only useful
/// to a caller that needs the element type's *spelling* — a writer
/// round-tripping the literal, say. Returns `None` for anything that is not a
/// typed array.
pub fn parse_variant_value_typed(s: &str) -> EngineResult<(Variant, Option<String>)> {
    let trimmed = s.trim();
    let elem = trimmed
        .strip_prefix("Array[")
        .and_then(|rest| rest.find(']').map(|close| rest[..close].trim().to_string()));
    Ok((parse_variant_value(s)?, elem))
}

pub fn parse_variant_value(s: &str) -> EngineResult<Variant> {
    let s = s.trim();

    // Quoted string
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        return Ok(Variant::String(unescape_string(&s[1..s.len() - 1])));
    }

    // Boolean
    if s == "true" {
        return Ok(Variant::Bool(true));
    }
    if s == "false" {
        return Ok(Variant::Bool(false));
    }

    // Null / Nil
    if s == "null" || s == "nil" || s == "Nil" {
        return Ok(Variant::Nil);
    }

    // Vector2(x, y)
    if let Some(inner) = try_strip_call(s, "Vector2") {
        let parts = split_args(inner);
        if parts.len() == 2 {
            let x = parse_f32(&parts[0])?;
            let y = parse_f32(&parts[1])?;
            return Ok(Variant::Vector2(Vector2::new(x, y)));
        }
        return Err(EngineError::Parse(format!("invalid Vector2: {s}")));
    }

    // Vector3(x, y, z)
    if let Some(inner) = try_strip_call(s, "Vector3") {
        let parts = split_args(inner);
        if parts.len() == 3 {
            let x = parse_f32(&parts[0])?;
            let y = parse_f32(&parts[1])?;
            let z = parse_f32(&parts[2])?;
            return Ok(Variant::Vector3(Vector3::new(x, y, z)));
        }
        return Err(EngineError::Parse(format!("invalid Vector3: {s}")));
    }

    // Color(r, g, b, a)
    if let Some(inner) = try_strip_call(s, "Color") {
        let parts = split_args(inner);
        if parts.len() == 4 {
            let r = parse_f32(&parts[0])?;
            let g = parse_f32(&parts[1])?;
            let b = parse_f32(&parts[2])?;
            let a = parse_f32(&parts[3])?;
            return Ok(Variant::Color(Color::new(r, g, b, a)));
        }
        if parts.len() == 3 {
            let r = parse_f32(&parts[0])?;
            let g = parse_f32(&parts[1])?;
            let b = parse_f32(&parts[2])?;
            return Ok(Variant::Color(Color::rgb(r, g, b)));
        }
        return Err(EngineError::Parse(format!("invalid Color: {s}")));
    }

    // Rect2(x, y, w, h)
    if let Some(inner) = try_strip_call(s, "Rect2") {
        let parts = split_args(inner);
        if parts.len() == 4 {
            let x = parse_f32(&parts[0])?;
            let y = parse_f32(&parts[1])?;
            let w = parse_f32(&parts[2])?;
            let h = parse_f32(&parts[3])?;
            return Ok(Variant::Rect2(Rect2::new(
                Vector2::new(x, y),
                Vector2::new(w, h),
            )));
        }
        return Err(EngineError::Parse(format!("invalid Rect2: {s}")));
    }

    // Transform2D(xx, xy, yx, yy, ox, oy)
    if let Some(inner) = try_strip_call(s, "Transform2D") {
        let parts = split_args(inner);
        if parts.len() == 6 {
            let xx = parse_f32(&parts[0])?;
            let xy = parse_f32(&parts[1])?;
            let yx = parse_f32(&parts[2])?;
            let yy = parse_f32(&parts[3])?;
            let ox = parse_f32(&parts[4])?;
            let oy = parse_f32(&parts[5])?;
            return Ok(Variant::Transform2D(Transform2D {
                x: Vector2::new(xx, xy),
                y: Vector2::new(yx, yy),
                origin: Vector2::new(ox, oy),
            }));
        }
        return Err(EngineError::Parse(format!("invalid Transform2D: {s}")));
    }

    // Transform3D(r0.x, r0.y, r0.z, r1.x, r1.y, r1.z, r2.x, r2.y, r2.z, ox, oy, oz)
    //
    // ⚠️ The nine basis numbers are ROW-major (pat-sazee). Godot's writer walks
    // `m3.rows[i][j]` (core/variant/variant_parser.cpp:2110) and its parser
    // feeds them to `Basis(xx, xy, xz, ...)`, which fills `rows`
    // (core/math/basis.h:211). `gdcore::math3d::Basis` stores COLUMNS, so the
    // first three numbers are `x.x, y.x, z.x`, not column `x`. Reading them as
    // columns transposed every authored rotation: platformer_3d's follow
    // camera (player.tscn:106) tilted UP where Godot's tilts down.
    if let Some(inner) = try_strip_call(s, "Transform3D") {
        let parts = split_args(inner);
        if parts.len() == 12 {
            let xx = parse_f32(&parts[0])?;
            let xy = parse_f32(&parts[1])?;
            let xz = parse_f32(&parts[2])?;
            let yx = parse_f32(&parts[3])?;
            let yy = parse_f32(&parts[4])?;
            let yz = parse_f32(&parts[5])?;
            let zx = parse_f32(&parts[6])?;
            let zy = parse_f32(&parts[7])?;
            let zz = parse_f32(&parts[8])?;
            let ox = parse_f32(&parts[9])?;
            let oy = parse_f32(&parts[10])?;
            let oz = parse_f32(&parts[11])?;
            return Ok(Variant::Transform3D(Transform3D {
                basis: basis_from_godot_rows([xx, xy, xz, yx, yy, yz, zx, zy, zz]),
                origin: Vector3::new(ox, oy, oz),
            }));
        }
        return Err(EngineError::Parse(format!("invalid Transform3D: {s}")));
    }

    // NodePath("path") — stored as Variant::NodePath
    if let Some(inner) = try_strip_call(s, "NodePath") {
        let inner = inner.trim();
        if inner.starts_with('"') && inner.ends_with('"') && inner.len() >= 2 {
            let path_str = unescape_string(&inner[1..inner.len() - 1]);
            return Ok(Variant::NodePath(NodePath::new(&path_str)));
        }
        return Err(EngineError::Parse(format!("invalid NodePath: {s}")));
    }

    // ExtResource("id") — stored as Variant::String with "ExtResource:" prefix
    if let Some(inner) = try_strip_call(s, "ExtResource") {
        let inner = inner.trim();
        if inner.starts_with('"') && inner.ends_with('"') && inner.len() >= 2 {
            let id = unescape_string(&inner[1..inner.len() - 1]);
            return Ok(Variant::String(format!("ExtResource:{id}")));
        }
        return Err(EngineError::Parse(format!("invalid ExtResource: {s}")));
    }

    // SubResource("id") — stored as Variant::String with "SubResource:" prefix
    if let Some(inner) = try_strip_call(s, "SubResource") {
        let inner = inner.trim();
        if inner.starts_with('"') && inner.ends_with('"') && inner.len() >= 2 {
            let id = unescape_string(&inner[1..inner.len() - 1]);
            return Ok(Variant::String(format!("SubResource:{id}")));
        }
        return Err(EngineError::Parse(format!("invalid SubResource: {s}")));
    }

    // Packed typed arrays: PackedByteArray(), PackedInt32Array(), etc.
    // Parsed into the matching `Variant::Packed*Array` variant so the packed
    // type is preserved through a parse → save round-trip (pat-u3l8c.11).
    if let Some(inner) = try_strip_call(s, "PackedByteArray") {
        return parse_packed_byte_array(inner);
    }
    if let Some(inner) = try_strip_call(s, "PackedInt32Array") {
        return parse_packed_int32_array(inner);
    }
    if let Some(inner) = try_strip_call(s, "PackedInt64Array") {
        return parse_packed_int64_array(inner);
    }
    if let Some(inner) = try_strip_call(s, "PackedFloat32Array") {
        return parse_packed_float32_array(inner);
    }
    if let Some(inner) = try_strip_call(s, "PackedFloat64Array") {
        return parse_packed_float64_array(inner);
    }
    if let Some(inner) = try_strip_call(s, "PackedStringArray") {
        return parse_packed_string_array(inner);
    }
    if let Some(inner) = try_strip_call(s, "PackedVector2Array") {
        return parse_packed_vector2_array(inner);
    }
    if let Some(inner) = try_strip_call(s, "PackedVector3Array") {
        return parse_packed_vector3_array(inner);
    }
    if let Some(inner) = try_strip_call(s, "PackedColorArray") {
        return parse_packed_color_array(inner);
    }
    if let Some(inner) = try_strip_call(s, "PackedVector4Array") {
        return parse_packed_vector4_array(inner);
    }

    // Vector2i(x, y) — an INTEGER vector, kept integral.
    //
    // These used to be coerced to a float `Vector2` because `Variant` had no int
    // vector, which silently changed a tile coordinate's type: `Vector2i(3, 4)`
    // read back as `Vector2(3.0, 4.0)` and saved back out as a float literal
    // (pat-nzgdu.78).
    if let Some(inner) = try_strip_call(s, "Vector2i") {
        let parts = split_args(inner);
        if parts.len() == 2 {
            let x = parse_i32(&parts[0])?;
            let y = parse_i32(&parts[1])?;
            return Ok(Variant::Vector2i(Vector2i::new(x, y)));
        }
        return Err(EngineError::Parse(format!("invalid Vector2i: {s}")));
    }

    // Rect2i(x, y, w, h) — the INTEGER rectangle.
    //
    // ⚠️ Must be tried before nothing and after `Rect2`: `try_strip_call`
    // matches on the exact `Name(` prefix, so `Rect2i(...)` never satisfies
    // `Rect2(` and the two cannot shadow each other. Widening this to a float
    // `Rect2` would round a TileSet atlas region onto its neighbour, which is
    // the pat-nzgdu.78 corruption one dimension up.
    if let Some(inner) = try_strip_call(s, "Rect2i") {
        let parts = split_args(inner);
        if parts.len() == 4 {
            return Ok(Variant::Rect2i(Rect2i::from_components(
                parse_i32(&parts[0])?,
                parse_i32(&parts[1])?,
                parse_i32(&parts[2])?,
                parse_i32(&parts[3])?,
            )));
        }
        return Err(EngineError::Parse(format!("invalid Rect2i: {s}")));
    }

    // Vector4i(x, y, z, w) — before Vector4 for readability only; the prefixes
    // are exact, so order does not decide the match.
    if let Some(inner) = try_strip_call(s, "Vector4i") {
        let parts = split_args(inner);
        if parts.len() == 4 {
            return Ok(Variant::Vector4i(Vector4i::new(
                parse_i32(&parts[0])?,
                parse_i32(&parts[1])?,
                parse_i32(&parts[2])?,
                parse_i32(&parts[3])?,
            )));
        }
        return Err(EngineError::Parse(format!("invalid Vector4i: {s}")));
    }

    // Vector4(x, y, z, w)
    if let Some(inner) = try_strip_call(s, "Vector4") {
        let parts = split_args(inner);
        if parts.len() == 4 {
            return Ok(Variant::Vector4(Vector4::new(
                parse_f32(&parts[0])?,
                parse_f32(&parts[1])?,
                parse_f32(&parts[2])?,
                parse_f32(&parts[3])?,
            )));
        }
        return Err(EngineError::Parse(format!("invalid Vector4: {s}")));
    }

    // Projection(x, y, z, w) — FOUR Vector4 COLUMNS, written flat as sixteen
    // numbers.
    //
    // ⚠️ The sixteen values are column-major (`m[column][row]`), matching both
    // Godot's storage and `gdcore::math3d::Projection`. Filling row-major here
    // would transpose every camera matrix, and a transposed projection still
    // renders — just wrongly — so nothing would catch it downstream.
    if let Some(inner) = try_strip_call(s, "Projection") {
        let parts = split_args(inner);
        if parts.len() == 16 {
            let mut m = [[0.0f32; 4]; 4];
            for (i, part) in parts.iter().enumerate() {
                m[i / 4][i % 4] = parse_f32(part)?;
            }
            return Ok(Variant::Projection(Projection { m }));
        }
        return Err(EngineError::Parse(format!(
            "Projection needs 16 floats, got {}: {s}",
            parts.len()
        )));
    }

    // Vector3i(x, y, z)
    if let Some(inner) = try_strip_call(s, "Vector3i") {
        let parts = split_args(inner);
        if parts.len() == 3 {
            let x = parse_i32(&parts[0])?;
            let y = parse_i32(&parts[1])?;
            let z = parse_i32(&parts[2])?;
            return Ok(Variant::Vector3i(Vector3i::new(x, y, z)));
        }
        return Err(EngineError::Parse(format!("invalid Vector3i: {s}")));
    }

    // Quaternion(x, y, z, w)
    if let Some(inner) = try_strip_call(s, "Quaternion") {
        let parts = split_args(inner);
        if parts.len() == 4 {
            return Ok(Variant::Quaternion(Quaternion {
                x: parse_f32(&parts[0])?,
                y: parse_f32(&parts[1])?,
                z: parse_f32(&parts[2])?,
                w: parse_f32(&parts[3])?,
            }));
        }
        return Err(EngineError::Parse(format!("invalid Quaternion: {s}")));
    }

    // Plane(a, b, c, d) — normal (a, b, c) and signed distance d
    if let Some(inner) = try_strip_call(s, "Plane") {
        let parts = split_args(inner);
        if parts.len() == 4 {
            return Ok(Variant::Plane(Plane {
                normal: Vector3::new(
                    parse_f32(&parts[0])?,
                    parse_f32(&parts[1])?,
                    parse_f32(&parts[2])?,
                ),
                d: parse_f32(&parts[3])?,
            }));
        }
        return Err(EngineError::Parse(format!("invalid Plane: {s}")));
    }

    // AABB(px, py, pz, sx, sy, sz) — position and size
    if let Some(inner) = try_strip_call(s, "AABB") {
        let parts = split_args(inner);
        if parts.len() == 6 {
            return Ok(Variant::Aabb(Aabb {
                position: Vector3::new(
                    parse_f32(&parts[0])?,
                    parse_f32(&parts[1])?,
                    parse_f32(&parts[2])?,
                ),
                size: Vector3::new(
                    parse_f32(&parts[3])?,
                    parse_f32(&parts[4])?,
                    parse_f32(&parts[5])?,
                ),
            }));
        }
        return Err(EngineError::Parse(format!("invalid AABB: {s}")));
    }

    // Basis(xx, xy, xz, yx, yy, yz, zx, zy, zz) — three ROW vectors, exactly
    // like the basis embedded in Transform3D above (pat-sazee).
    if let Some(inner) = try_strip_call(s, "Basis") {
        let parts = split_args(inner);
        if parts.len() == 9 {
            let mut f = [0.0f32; 9];
            for (slot, p) in f.iter_mut().zip(parts.iter()) {
                *slot = parse_f32(p)?;
            }
            return Ok(Variant::Basis(basis_from_godot_rows(f)));
        }
        return Err(EngineError::Parse(format!("invalid Basis: {s}")));
    }

    // StringName("name") — the call-style form.
    if let Some(inner) = try_strip_call(s, "StringName") {
        // ⚠️ The ampersand is OPTIONAL inside the call form. Godot writes both
        // `StringName("name")` and `StringName(&"name")`, and this arm used to
        // require a quote directly after the paren, so it rejected the second
        // with "invalid StringName" (pat-21oab).
        let inner = inner
            .trim()
            .strip_prefix('&')
            .unwrap_or(inner.trim())
            .trim();
        if inner.starts_with('"') && inner.ends_with('"') && inner.len() >= 2 {
            let name = unescape_string(&inner[1..inner.len() - 1]);
            return Ok(Variant::StringName(StringName::new(&name)));
        }
        return Err(EngineError::Parse(format!("invalid StringName: {s}")));
    }

    // StringName literal: &"name" (Godot's `&`-prefixed form).
    if let Some(rest) = s.strip_prefix('&') {
        let rest = rest.trim();
        if rest.starts_with('"') && rest.ends_with('"') && rest.len() >= 2 {
            let name = unescape_string(&rest[1..rest.len() - 1]);
            return Ok(Variant::StringName(StringName::new(&name)));
        }
        return Err(EngineError::Parse(format!(
            "invalid StringName literal: {s}"
        )));
    }

    // NodePath literal: `^"Path/To/Node"` (pat-0q2kx.136). Godot writes node
    // paths with a `^` sigil exactly as it writes StringNames with `&`; without
    // this arm every authored NodePath property failed to parse and was dropped.
    if let Some(rest) = s.strip_prefix('^') {
        let rest = rest.trim();
        if rest.starts_with('"') && rest.ends_with('"') && rest.len() >= 2 {
            let path = unescape_string(&rest[1..rest.len() - 1]);
            return Ok(Variant::NodePath(NodePath::new(&path)));
        }
        return Err(EngineError::Parse(format!("invalid NodePath literal: {s}")));
    }

    // `Object(Class, "prop": value, ...)` — an inline sub-object, used by theme
    // overrides and embedded InputEvents (pat-0q2kx.136).
    //
    // `Variant` has no object-literal kind, so this becomes a tagged dictionary:
    // the class under `UNPARSED_CLASS_KEY` and every property as a real key. A
    // consumer reads the properties directly; the saver re-emits from the class
    // plus those keys.
    if let Some(inner) = try_strip_call(s, "Object") {
        let parts = split_args(inner);
        let Some((class, props)) = parts.split_first() else {
            return Err(EngineError::Parse(format!("empty Object literal: {s}")));
        };
        let mut dict = Dictionary::new();
        dict.insert(
            UNPARSED_CLASS_KEY,
            Variant::String(class.trim().trim_matches('"').to_string()),
        );
        for entry in props {
            // Each remaining arg is `"name": value`.
            let Some((key, value)) = entry.split_once(':') else {
                return Err(EngineError::Parse(format!(
                    "Object property needs a `name: value` pair, got `{entry}` in {s}"
                )));
            };
            let key = key.trim().trim_matches('"').to_string();
            dict.insert(key, parse_variant_value(value)?);
        }
        return Ok(Variant::Dictionary(dict.into()));
    }

    // Builtins that `Variant` has no kind for yet: Vector4/Vector4i, Rect2i,
    // Projection and PackedVector4Array (pat-0q2kx.136).
    //
    // These are preserved *tagged and verbatim* rather than dropped or
    // retyped. Godot never loses a property it can parse, and silently
    // widening `Rect2i` to a float `Rect2` would corrupt it the same way
    // `Vector2i` used to be corrupted (pat-nzgdu.78) — so the literal is kept
    // exactly as authored, letting the saver round-trip it byte-for-byte and
    // letting a later core-variant bead promote it to a real kind without any
    // data having been lost in the meantime.
    for name in UNPARSED_BUILTINS {
        if let Some(inner) = try_strip_call(s, name) {
            let mut dict = Dictionary::new();
            dict.insert(UNPARSED_TYPE_KEY, Variant::String((*name).to_string()));
            dict.insert(UNPARSED_LITERAL_KEY, Variant::String(s.to_string()));
            // The numeric components, so a consumer can read the value without
            // re-parsing the literal text.
            let args: Vec<Variant> = split_args(inner)
                .iter()
                .map(|a| match a.trim().parse::<f64>() {
                    Ok(f) => Variant::Float(f),
                    Err(_) => Variant::String(a.trim().to_string()),
                })
                .collect();
            dict.insert(UNPARSED_ARGS_KEY, Variant::Array(args.into()));
            return Ok(Variant::Dictionary(dict.into()));
        }
    }

    // Typed dictionary: `Dictionary[K, V]({"k": v, ...})`. Godot 4.4+ writes a
    // typed dictionary in exactly this shape
    // (`core/variant/variant_parser.cpp:2241`). Without this branch the literal
    // fell past every case below to "unrecognized value", so a scene carrying
    // one failed to load at all (pat-mgixk.108).
    if let Some(rest) = s.strip_prefix("Dictionary[") {
        // ⚠️ Depth-aware scan, not `find(']')`: the parameters may themselves be
        // generic, as in `Dictionary[String, Array[int]]`, where the first `]`
        // closes the inner `Array[int` and a naive search would split the hint
        // in the wrong place.
        if let Some(close) = closing_bracket(rest) {
            let params = split_args(&rest[..close]);
            let after = rest[close + 1..].trim();
            if params.len() == 2 {
                if let Some(call) = after.strip_prefix('(').and_then(|c| c.strip_suffix(')')) {
                    let call = call.trim();
                    if let Some(body) = call.strip_prefix('{').and_then(|c| c.strip_suffix('}')) {
                        return as_typed_dictionary(
                            &params[0],
                            &params[1],
                            parse_dictionary(body)?,
                        );
                    }
                }
            }
        }
        return Err(EngineError::Parse(format!("invalid typed Dictionary: {s}")));
    }

    // Typed array: `Array[Type]([elem, ...])`. The element type is retained on
    // the array itself, so a scene property loads as a genuinely typed array
    // and `is_typed()` answers true (pat-s3rrv.265).
    if let Some(rest) = s.strip_prefix("Array[") {
        if let Some(close) = rest.find(']') {
            let elem_name = rest[..close].trim();
            let after = rest[close + 1..].trim();
            if let Some(call) = after.strip_prefix('(').and_then(|c| c.strip_suffix(')')) {
                let call = call.trim();
                if let Some(elems) = call.strip_prefix('[').and_then(|c| c.strip_suffix(']')) {
                    return as_typed_array(elem_name, parse_array(elems)?);
                }
            }
        }
        return Err(EngineError::Parse(format!("invalid typed Array: {s}")));
    }

    // Array: [elem, elem, ...]
    if s.starts_with('[') && s.ends_with(']') {
        return parse_array(&s[1..s.len() - 1]);
    }

    // Dictionary: {key: value, ...}
    if s.starts_with('{') && s.ends_with('}') {
        return parse_dictionary(&s[1..s.len() - 1]);
    }

    // Integer (no decimal point)
    if let Ok(i) = s.parse::<i64>() {
        return Ok(Variant::Int(i));
    }

    // Float. This also covers Godot's non-finite literals: Rust's `f64` parser
    // accepts `inf`, `-inf` and `nan` case-insensitively, which are exactly the
    // spellings `variant_parser.cpp` emits (pat-0q2kx.136).
    if let Ok(f) = s.parse::<f64>() {
        return Ok(Variant::Float(f));
    }

    Err(EngineError::Parse(format!("unrecognized value: {s}")))
}

/// Tries to strip a function-call wrapper, e.g. `Vector2(1, 2)` -> `"1, 2"`.
///
/// Uses balanced parenthesis matching so nested calls like
/// `ExtResource("id")` inside arrays work correctly.
fn try_strip_call<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let s = s.trim();
    if !s.starts_with(name) {
        return None;
    }
    let rest = &s[name.len()..];
    if !rest.starts_with('(') {
        return None;
    }
    // Find the matching closing paren.
    let bytes = rest.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for (i, &b) in bytes.iter().enumerate() {
        if escape {
            escape = false;
            continue;
        }
        if b == b'\\' && in_string {
            escape = true;
            continue;
        }
        if b == b'"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        if b == b'(' {
            depth += 1;
        } else if b == b')' {
            depth -= 1;
            if depth == 0 {
                // Only match if this closing paren is the last character.
                if i == rest.len() - 1 {
                    return Some(&rest[1..i]);
                }
                return None;
            }
        }
    }
    None
}

/// Splits top-level comma-separated arguments, respecting nested parens,
/// brackets, braces, and quoted strings.
fn split_args(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    let mut start = 0;

    for (i, c) in s.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if c == '\\' && in_string {
            escape = true;
            continue;
        }
        if c == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(s[start..i].trim().to_string());
                start = i + 1;
            }
            _ => {}
        }
    }

    let tail = s[start..].trim();
    if !tail.is_empty() {
        parts.push(tail.to_string());
    }

    parts
}

/// Parses a float from a string, providing a nice error.
fn parse_f32(s: &str) -> EngineResult<f32> {
    s.trim()
        .parse::<f32>()
        .map_err(|_| EngineError::Parse(format!("expected float, got: {s}")))
}

/// Parses an integer component of a `Vector2i`/`Vector3i` literal.
///
/// Godot writes these as bare integers; a component with a fractional part is a
/// malformed int vector rather than something to silently truncate, so it errors
/// out through the same path as any other unparseable literal (pat-nzgdu.78).
fn parse_i32(s: &str) -> EngineResult<i32> {
    s.trim()
        .parse::<i32>()
        .map_err(|_| EngineError::Parse(format!("expected int, got: {s}")))
}

/// Unescapes a Godot-style string (inner content, without surrounding quotes).
fn unescape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('r') => out.push('\r'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Parses an array body (content between `[` and `]`) into `Variant::Array`.
fn parse_array(s: &str) -> EngineResult<Variant> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Variant::Array(Default::default()));
    }
    let elements = split_args(s);
    let mut result = Vec::with_capacity(elements.len());
    for elem in &elements {
        result.push(parse_variant_value(elem)?);
    }
    Ok(Variant::Array(result.into()))
}

/// Re-homes an `Array[T](…)` literal's elements into an array that actually
/// carries the element type `T` (pat-s3rrv.265).
///
/// The element type has to be applied by *constructing* a typed array and
/// assigning into it, not by tagging the parsed one: `SharedArray::set_typed`
/// refuses a non-empty array, exactly as Godot's `array.cpp` does.
///
/// ⚠️ Element equality is unaffected — `SharedArray`'s `PartialEq` is Godot's
/// structural `recursive_equal`, which compares elements and not the type — so
/// `Array[int]([1, 2])` still compares equal to `[1, 2]`.
fn as_typed_array(elem_name: &str, parsed: Variant) -> EngineResult<Variant> {
    let Variant::Array(items) = parsed else {
        return Ok(parsed);
    };
    let want = ContainerType::from_hint_name(elem_name);
    // `Array[Variant](…)` and an unreadable element name leave the array
    // untyped rather than inventing a constraint the scene never asked for.
    if !want.is_typed() {
        return Ok(Variant::Array(items));
    }
    let typed = SharedArray::typed(want);
    // A scene whose declared element type disagrees with its own literal is a
    // corrupt file; reporting it beats loading a silently untyped array.
    typed
        .assign(&items)
        .map_err(|err| EngineError::Parse(format!("Array[{elem_name}]: {err}")))?;
    Ok(Variant::Array(typed))
}

/// Byte offset of the `]` that closes the bracket this slice opens inside, or
/// `None` if the brackets never balance.
///
/// `s` starts just *after* a consumed `[`, so the scan begins at depth 1.
/// Quoted segments are skipped, because a type parameter can legitimately be a
/// string-keyed nested container.
fn closing_bracket(s: &str) -> Option<usize> {
    let mut depth = 1i32;
    let mut in_string = false;
    let mut escape = false;
    for (i, c) in s.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        match c {
            '\\' if in_string => escape = true,
            '"' => in_string = !in_string,
            _ if in_string => {}
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Re-homes a `Dictionary[K, V](…)` literal into a dictionary that actually
/// carries its key and value types, so `is_typed()` answers true for a
/// scene-loaded property — the dictionary counterpart of [`as_typed_array`].
///
/// The types must be applied to an **empty** dictionary and the entries
/// assigned in: `SharedDictionary::set_typed` refuses a populated one, matching
/// Godot's `dictionary.cpp`.
fn as_typed_dictionary(key_name: &str, value_name: &str, parsed: Variant) -> EngineResult<Variant> {
    let Variant::Dictionary(entries) = parsed else {
        return Ok(parsed);
    };
    let key = ContainerType::from_hint_name(key_name);
    let value = ContainerType::from_hint_name(value_name);
    // `Dictionary[Variant, Variant]` constrains nothing, so leave the parsed
    // dictionary as it is rather than inventing a type the scene never asked
    // for — the same rule `as_typed_array` applies to `Array[Variant]`.
    if !key.is_typed() && !value.is_typed() {
        return Ok(Variant::Dictionary(entries));
    }
    let typed = SharedDictionary::new();
    typed.set_typed(key, value).map_err(|err| {
        EngineError::Parse(format!("Dictionary[{key_name}, {value_name}]: {err}"))
    })?;
    // A scene whose declared types disagree with its own literal is corrupt;
    // reporting it beats loading a silently untyped dictionary.
    typed.assign(&entries).map_err(|err| {
        EngineError::Parse(format!("Dictionary[{key_name}, {value_name}]: {err}"))
    })?;
    Ok(Variant::Dictionary(typed))
}

/// Parses a dictionary body (content between `{` and `}`) into `Variant::Dictionary`.
///
/// Expects `"key": value` pairs separated by commas. Keys must be quoted strings.
fn parse_dictionary(s: &str) -> EngineResult<Variant> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Variant::Dictionary(Default::default()));
    }
    let pairs = split_args(s);
    let mut map = Dictionary::new();
    for pair in &pairs {
        let pair = pair.trim();
        // ⚠️ A `StringName` key is written `&"name"`. Godot keys an
        // `AnimationLibrary`'s animations by StringName, so `_get_data` saves
        // `_data = {&"clip": SubResource(..)}` (upstream
        // animation_library.cpp `_get_data`, variant_parser.cpp STRING_NAME).
        // Rejecting the ampersand threw out the whole dictionary, so a library
        // written by Godot 4.6 yielded no clips at all (pat-mgixk.1).
        //
        // The ampersand is dropped rather than preserved as a
        // `Variant::StringName`: in Godot a String and a StringName of the same
        // text are the same Dictionary key (they compare and hash equal), and
        // every consumer in this workspace matches on `Variant::String`.
        let pair = match pair.strip_prefix('&') {
            Some(rest) => rest.trim_start(),
            None => pair,
        };
        // Find the colon separating key from value, but only outside of the key string.
        if !pair.starts_with('"') {
            return Err(EngineError::Parse(format!(
                "dictionary key must be a quoted string: {pair}"
            )));
        }
        // Find end of key string.
        let key_end = find_closing_quote(pair, 1)
            .ok_or_else(|| EngineError::Parse(format!("unterminated dictionary key: {pair}")))?;
        let key = unescape_string(&pair[1..key_end]);
        let rest = pair[key_end + 1..].trim();
        let rest = rest
            .strip_prefix(':')
            .ok_or_else(|| {
                EngineError::Parse(format!("expected ':' after dictionary key: {pair}"))
            })?
            .trim();
        let value = parse_variant_value(rest)?;
        map.insert(key, value);
    }
    Ok(Variant::Dictionary(map.into()))
}

/// Parses a `PackedByteArray` body in either form Godot writes.
///
/// Godot 4 serializes a byte array as `PackedByteArray("<base64>")`
/// (`core/variant/variant_parser.cpp` `VariantWriter::write`, the non-`p_compat`
/// branch) and only falls back to `PackedByteArray(1, 2, 3)` in compatibility
/// mode. Patina understood the compat form only, which meant any real
/// Godot-authored byte array — `TileMapLayer.tile_map_data` above all — failed
/// to parse (pat-0q2kx.108). Both are accepted here; the quoted form is base64.
fn parse_packed_byte_array(s: &str) -> EngineResult<Variant> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Variant::PackedByteArray(Vec::new()));
    }
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        return base64_decode(&s[1..s.len() - 1])
            .map(Variant::PackedByteArray)
            .ok_or_else(|| EngineError::Parse(format!("invalid base64 in PackedByteArray: {s}")));
    }
    let parts = split_args(s);
    let mut result = Vec::with_capacity(parts.len());
    for part in &parts {
        let b = part.trim().parse::<u8>().map_err(|_| {
            EngineError::Parse(format!(
                "expected byte (0-255) in PackedByteArray, got: {part}"
            ))
        })?;
        result.push(b);
    }
    Ok(Variant::PackedByteArray(result))
}

/// Decodes standard base64 (RFC 4648, `+/` alphabet, optional `=` padding).
///
/// Returns `None` on any character outside the alphabet or a length that cannot
/// form whole bytes. Whitespace is skipped, since a long `tile_map_data` line
/// can be wrapped by an editor.
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    const INVALID: u8 = 0xFF;
    fn sextet(c: u8) -> u8 {
        match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => INVALID,
        }
    }

    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for c in s.bytes() {
        if c.is_ascii_whitespace() || c == b'=' {
            continue;
        }
        let v = sextet(c);
        if v == INVALID {
            return None;
        }
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            // Drop the consumed bits so `acc` can never overflow on a long
            // payload — a debug build would panic on the shift otherwise.
            acc &= (1u32 << bits) - 1;
        }
    }
    // Leftover bits must be zero padding, never a truncated byte.
    if bits >= 6 || (acc & ((1 << bits) - 1)) != 0 {
        return None;
    }
    Some(out)
}

/// Parses a `PackedInt32Array` body into `Variant::PackedInt32Array`.
fn parse_packed_int32_array(s: &str) -> EngineResult<Variant> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Variant::PackedInt32Array(Vec::new()));
    }
    let parts = split_args(s);
    let mut result = Vec::with_capacity(parts.len());
    for part in &parts {
        let i = part.trim().parse::<i32>().map_err(|_| {
            EngineError::Parse(format!("expected i32 in PackedInt32Array, got: {part}"))
        })?;
        result.push(i);
    }
    Ok(Variant::PackedInt32Array(result))
}

/// Parses a `PackedInt64Array` body into `Variant::PackedInt64Array`.
fn parse_packed_int64_array(s: &str) -> EngineResult<Variant> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Variant::PackedInt64Array(Vec::new()));
    }
    let parts = split_args(s);
    let mut result = Vec::with_capacity(parts.len());
    for part in &parts {
        let i = part.trim().parse::<i64>().map_err(|_| {
            EngineError::Parse(format!("expected i64 in PackedInt64Array, got: {part}"))
        })?;
        result.push(i);
    }
    Ok(Variant::PackedInt64Array(result))
}

/// Parses a `PackedFloat32Array` body into `Variant::PackedFloat32Array`.
fn parse_packed_float32_array(s: &str) -> EngineResult<Variant> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Variant::PackedFloat32Array(Vec::new()));
    }
    let parts = split_args(s);
    let mut result = Vec::with_capacity(parts.len());
    for part in &parts {
        result.push(parse_f32(part)?);
    }
    Ok(Variant::PackedFloat32Array(result))
}

/// Parses a `PackedFloat64Array` body into `Variant::PackedFloat64Array`.
fn parse_packed_float64_array(s: &str) -> EngineResult<Variant> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Variant::PackedFloat64Array(Vec::new()));
    }
    let parts = split_args(s);
    let mut result = Vec::with_capacity(parts.len());
    for part in &parts {
        let f = part.trim().parse::<f64>().map_err(|_| {
            EngineError::Parse(format!("expected f64 in PackedFloat64Array, got: {part}"))
        })?;
        result.push(f);
    }
    Ok(Variant::PackedFloat64Array(result))
}

/// Parses a `PackedStringArray` body into `Variant::PackedStringArray`.
fn parse_packed_string_array(s: &str) -> EngineResult<Variant> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Variant::PackedStringArray(Vec::new()));
    }
    let parts = split_args(s);
    let mut result = Vec::with_capacity(parts.len());
    for part in &parts {
        let part = part.trim();
        if part.starts_with('"') && part.ends_with('"') && part.len() >= 2 {
            result.push(unescape_string(&part[1..part.len() - 1]));
        } else {
            return Err(EngineError::Parse(format!(
                "expected quoted string in PackedStringArray, got: {part}"
            )));
        }
    }
    Ok(Variant::PackedStringArray(result))
}

/// Parses a `PackedVector2Array` body (flat pairs: `x1, y1, x2, y2, ...`).
fn parse_packed_vector2_array(s: &str) -> EngineResult<Variant> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Variant::PackedVector2Array(Vec::new()));
    }
    let parts = split_args(s);
    if !parts.len().is_multiple_of(2) {
        return Err(EngineError::Parse(format!(
            "PackedVector2Array needs even number of floats, got {}",
            parts.len()
        )));
    }
    let mut result = Vec::with_capacity(parts.len() / 2);
    for chunk in parts.chunks(2) {
        let x = parse_f32(&chunk[0])?;
        let y = parse_f32(&chunk[1])?;
        result.push(Vector2::new(x, y));
    }
    Ok(Variant::PackedVector2Array(result))
}

/// Parses a `PackedVector3Array` body (flat triples: `x1, y1, z1, ...`).
fn parse_packed_vector3_array(s: &str) -> EngineResult<Variant> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Variant::PackedVector3Array(Vec::new()));
    }
    let parts = split_args(s);
    if !parts.len().is_multiple_of(3) {
        return Err(EngineError::Parse(format!(
            "PackedVector3Array needs multiple-of-3 floats, got {}",
            parts.len()
        )));
    }
    let mut result = Vec::with_capacity(parts.len() / 3);
    for chunk in parts.chunks(3) {
        let x = parse_f32(&chunk[0])?;
        let y = parse_f32(&chunk[1])?;
        let z = parse_f32(&chunk[2])?;
        result.push(Vector3::new(x, y, z));
    }
    Ok(Variant::PackedVector3Array(result))
}

/// Parses a `PackedColorArray` body (flat quads: `r1, g1, b1, a1, ...`).
fn parse_packed_color_array(s: &str) -> EngineResult<Variant> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Variant::PackedColorArray(Vec::new()));
    }
    let parts = split_args(s);
    if !parts.len().is_multiple_of(4) {
        return Err(EngineError::Parse(format!(
            "PackedColorArray needs multiple-of-4 floats, got {}",
            parts.len()
        )));
    }
    let mut result = Vec::with_capacity(parts.len() / 4);
    for chunk in parts.chunks(4) {
        let r = parse_f32(&chunk[0])?;
        let g = parse_f32(&chunk[1])?;
        let b = parse_f32(&chunk[2])?;
        let a = parse_f32(&chunk[3])?;
        result.push(Color::new(r, g, b, a));
    }
    Ok(Variant::PackedColorArray(result))
}

/// Parses a `PackedVector4Array` body (flat quads: `x1, y1, z1, w1, ...`).
fn parse_packed_vector4_array(s: &str) -> EngineResult<Variant> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Variant::PackedVector4Array(Vec::new()));
    }
    let parts = split_args(s);
    if !parts.len().is_multiple_of(4) {
        return Err(EngineError::Parse(format!(
            "PackedVector4Array needs multiple-of-4 floats, got {}",
            parts.len()
        )));
    }
    let mut result = Vec::with_capacity(parts.len() / 4);
    for chunk in parts.chunks(4) {
        let x = parse_f32(&chunk[0])?;
        let y = parse_f32(&chunk[1])?;
        let z = parse_f32(&chunk[2])?;
        let w = parse_f32(&chunk[3])?;
        result.push(Vector4::new(x, y, z, w));
    }
    Ok(Variant::PackedVector4Array(result))
}

/// Finds the index of the closing quote in a string, starting search at `start`.
/// Handles escape sequences.
fn find_closing_quote(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = start;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2; // skip escaped char
            continue;
        }
        if bytes[i] == b'"' {
            return Some(i);
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Rect2;

    #[test]
    fn parse_string_value() {
        assert_eq!(
            parse_variant_value(r#""hello world""#).unwrap(),
            Variant::String("hello world".into())
        );
    }

    #[test]
    fn parse_bool_values() {
        assert_eq!(parse_variant_value("true").unwrap(), Variant::Bool(true));
        assert_eq!(parse_variant_value("false").unwrap(), Variant::Bool(false));
    }

    #[test]
    fn parse_int_value() {
        assert_eq!(parse_variant_value("42").unwrap(), Variant::Int(42));
        assert_eq!(parse_variant_value("-7").unwrap(), Variant::Int(-7));
    }

    #[test]
    fn parse_float_value() {
        assert_eq!(parse_variant_value("3.14").unwrap(), Variant::Float(3.14));
    }

    #[test]
    fn parse_vector2_value() {
        let v = parse_variant_value("Vector2(10, 20)").unwrap();
        assert_eq!(v, Variant::Vector2(Vector2::new(10.0, 20.0)));
    }

    #[test]
    fn parse_vector3_value() {
        let v = parse_variant_value("Vector3(1, 2, 3)").unwrap();
        assert_eq!(v, Variant::Vector3(Vector3::new(1.0, 2.0, 3.0)));
    }

    #[test]
    fn parse_color_value() {
        let v = parse_variant_value("Color(0.2, 0.3, 0.4, 1)").unwrap();
        assert_eq!(v, Variant::Color(Color::new(0.2, 0.3, 0.4, 1.0)));
    }

    #[test]
    fn parse_rect2_value() {
        let v = parse_variant_value("Rect2(10, 20, 100, 50)").unwrap();
        assert_eq!(
            v,
            Variant::Rect2(Rect2::new(
                Vector2::new(10.0, 20.0),
                Vector2::new(100.0, 50.0)
            ))
        );
    }

    #[test]
    fn parse_transform2d_value() {
        let v = parse_variant_value("Transform2D(1, 0, 0, 1, 10, 20)").unwrap();
        assert_eq!(
            v,
            Variant::Transform2D(Transform2D {
                x: Vector2::new(1.0, 0.0),
                y: Vector2::new(0.0, 1.0),
                origin: Vector2::new(10.0, 20.0),
            })
        );
    }

    #[test]
    fn parse_node_path_value() {
        let v = parse_variant_value(r#"NodePath("Player/Sprite")"#).unwrap();
        assert_eq!(
            v,
            Variant::NodePath(gdcore::node_path::NodePath::new("Player/Sprite"))
        );
    }

    #[test]
    fn parse_ext_resource_value() {
        let v = parse_variant_value(r#"ExtResource("1_abc")"#).unwrap();
        assert_eq!(v, Variant::String("ExtResource:1_abc".into()));
    }

    #[test]
    fn parse_sub_resource_value() {
        let v = parse_variant_value(r#"SubResource("StyleBoxFlat_abc")"#).unwrap();
        assert_eq!(v, Variant::String("SubResource:StyleBoxFlat_abc".into()));
    }

    // pat-u3l8c.86 — parser completeness for 3D math + StringName + typed arrays.

    #[test]
    fn parse_quaternion_value() {
        let v = parse_variant_value("Quaternion(0, 0, 0, 1)").unwrap();
        assert_eq!(
            v,
            Variant::Quaternion(Quaternion {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                w: 1.0,
            })
        );
    }

    #[test]
    fn parse_plane_value() {
        let v = parse_variant_value("Plane(0, 1, 0, 5)").unwrap();
        assert_eq!(
            v,
            Variant::Plane(Plane {
                normal: Vector3::new(0.0, 1.0, 0.0),
                d: 5.0,
            })
        );
    }

    #[test]
    fn parse_aabb_value() {
        let v = parse_variant_value("AABB(1, 2, 3, 4, 5, 6)").unwrap();
        assert_eq!(
            v,
            Variant::Aabb(Aabb {
                position: Vector3::new(1.0, 2.0, 3.0),
                size: Vector3::new(4.0, 5.0, 6.0),
            })
        );
    }

    #[test]
    fn parse_basis_value() {
        let v = parse_variant_value("Basis(1, 0, 0, 0, 1, 0, 0, 0, 1)").unwrap();
        assert_eq!(
            v,
            Variant::Basis(Basis {
                x: Vector3::new(1.0, 0.0, 0.0),
                y: Vector3::new(0.0, 1.0, 0.0),
                z: Vector3::new(0.0, 0.0, 1.0),
            })
        );
    }

    #[test]
    fn parse_string_name_call_and_literal() {
        let call = parse_variant_value(r#"StringName("idle")"#).unwrap();
        assert_eq!(call, Variant::StringName(StringName::new("idle")));
        // Godot's `&"name"` shorthand parses to the same StringName.
        let lit = parse_variant_value(r#"&"idle""#).unwrap();
        assert_eq!(lit, Variant::StringName(StringName::new("idle")));
    }

    #[test]
    fn parse_typed_array_value() {
        // `Array[int]([...])` parses identically to the plain `[...]` array.
        let typed = parse_variant_value("Array[int]([1, 2, 3])").unwrap();
        let plain = parse_variant_value("[1, 2, 3]").unwrap();
        assert_eq!(typed, plain);
    }

    /// pat-0q2kx.108 — a `PackedByteArray` parses in *both* forms Godot writes.
    ///
    /// Godot 4's text writer emits `PackedByteArray("<base64>")` and only falls
    /// back to the comma-separated byte list in compatibility mode. Patina read
    /// the compat form only, so every real Godot byte array — a
    /// `TileMapLayer.tile_map_data` above all — was dropped as unparseable.
    #[test]
    fn tscn_parses_packed_byte_array_in_both_forms() {
        // "Hi!" — 3 bytes, so the base64 carries one '=' of padding.
        assert_eq!(
            parse_variant_value(r#"PackedByteArray("SGkh")"#).unwrap(),
            Variant::PackedByteArray(vec![72, 105, 33])
        );
        // The compat form still works, and both agree on the same bytes.
        assert_eq!(
            parse_variant_value("PackedByteArray(72, 105, 33)").unwrap(),
            Variant::PackedByteArray(vec![72, 105, 33])
        );
        // Every padding length round-trips: 1, 2 and 3 bytes exercise the
        // "==", "=" and no-padding cases.
        for (encoded, bytes) in [
            (r#"PackedByteArray("AA==")"#, vec![0u8]),
            (r#"PackedByteArray("//8=")"#, vec![255, 255]),
            (r#"PackedByteArray("AAEC")"#, vec![0, 1, 2]),
        ] {
            assert_eq!(
                parse_variant_value(encoded).unwrap(),
                Variant::PackedByteArray(bytes),
                "failed on {encoded}"
            );
        }
        // Empty stays empty in both spellings.
        assert_eq!(
            parse_variant_value("PackedByteArray()").unwrap(),
            Variant::PackedByteArray(Vec::new())
        );
        assert_eq!(
            parse_variant_value(r#"PackedByteArray("")"#).unwrap(),
            Variant::PackedByteArray(Vec::new())
        );
        // A character outside the alphabet is an error, not silently-dropped
        // data — a truncated `tile_map_data` would lose cells invisibly.
        assert!(parse_variant_value(r#"PackedByteArray("AA*A")"#).is_err());
        // A lone leftover sextet cannot form a byte.
        assert!(parse_variant_value(r#"PackedByteArray("A")"#).is_err());
    }

    /// pat-nzgdu.78 — an int-vector literal keeps its INTEGER type.
    ///
    /// These used to come back as float `Vector2`/`Vector3`, because `Variant`
    /// had no int vector to hold them: a `TileMap` cell authored as
    /// `Vector2i(3, 4)` loaded as `Vector2(3.0, 4.0)` and saved back out as a
    /// float literal, so a round-trip rewrote the scene file.
    #[test]
    fn tscn_parses_intvec_and_math_literals() {
        assert_eq!(
            parse_variant_value("Vector2i(3, 4)").unwrap(),
            Variant::Vector2i(Vector2i::new(3, 4))
        );
        assert_eq!(
            parse_variant_value("Vector3i(-1, 0, 7)").unwrap(),
            Variant::Vector3i(Vector3i::new(-1, 0, 7))
        );

        // ⚠️ The float arms are untouched: `Vector2i` must not be shadowed by
        // `Vector2`, and `Vector2` must not start yielding an int vector.
        assert_eq!(
            parse_variant_value("Vector2(3, 4)").unwrap(),
            Variant::Vector2(Vector2::new(3.0, 4.0))
        );
        assert_eq!(
            parse_variant_value("Vector3(1.5, 2, 3)").unwrap(),
            Variant::Vector3(Vector3::new(1.5, 2.0, 3.0))
        );

        // A fractional component is a malformed int vector, not something to
        // truncate silently — it takes the same error path as any other
        // unparseable literal rather than becoming Vector2i(1, 2).
        assert!(parse_variant_value("Vector2i(1.5, 2)").is_err());
        assert!(parse_variant_value("Vector3i(1, 2)").is_err());
    }

    #[test]
    fn unrecognized_value_errors_not_silently_dropped() {
        // parse_variant_value returns a loud Err for unknown constructs so that
        // packed_scene can warn instead of silently dropping data.
        assert!(parse_variant_value("Frobnicate(1, 2)").is_err());
    }

    #[test]
    fn parse_null_nil() {
        assert_eq!(parse_variant_value("null").unwrap(), Variant::Nil);
        assert_eq!(parse_variant_value("nil").unwrap(), Variant::Nil);
        assert_eq!(parse_variant_value("Nil").unwrap(), Variant::Nil);
    }

    #[test]
    fn parse_string_escape_sequences() {
        let v = parse_variant_value(r#""line1\nline2\ttab\\slash\"quote""#).unwrap();
        assert_eq!(v, Variant::String("line1\nline2\ttab\\slash\"quote".into()));
    }

    #[test]
    fn parse_array_mixed_types() {
        let v = parse_variant_value(r#"[1, 2.5, "three", Vector2(1, 2), true, null]"#).unwrap();
        match v {
            Variant::Array(ref items) => {
                assert_eq!(items.len(), 6);
                assert_eq!(items.to_vec()[0], Variant::Int(1));
                assert_eq!(items.to_vec()[1], Variant::Float(2.5));
                assert_eq!(items.to_vec()[2], Variant::String("three".into()));
                assert_eq!(items.to_vec()[3], Variant::Vector2(Vector2::new(1.0, 2.0)));
                assert_eq!(items.to_vec()[4], Variant::Bool(true));
                assert_eq!(items.to_vec()[5], Variant::Nil);
            }
            other => panic!("expected Array, got {other:?}"),
        }
    }

    #[test]
    fn parse_empty_array() {
        assert_eq!(
            parse_variant_value("[]").unwrap(),
            Variant::Array(Default::default())
        );
    }

    #[test]
    fn parse_dictionary() {
        let v = parse_variant_value(r#"{"name": "Player", "health": 100, "alive": true}"#).unwrap();
        match v {
            Variant::Dictionary(ref map) => {
                assert_eq!(map.len(), 3);
                assert_eq!(map.to_dict()["name"], Variant::String("Player".into()));
                assert_eq!(map.to_dict()["health"], Variant::Int(100));
                assert_eq!(map.to_dict()["alive"], Variant::Bool(true));
            }
            other => panic!("expected Dictionary, got {other:?}"),
        }
    }

    /// ⚠️ Godot keys an `AnimationLibrary`'s animations by `StringName`, so its
    /// saved `_data` uses the `&"name"` spelling (animation_library.cpp
    /// `_get_data` + variant_parser.cpp STRING_NAME). Rejecting the ampersand
    /// threw the whole dictionary away, so a 4.6 library yielded no clips
    /// (pat-mgixk.1).
    #[test]
    fn parse_dictionary_with_stringname_keys() {
        let v = parse_variant_value(r#"{&"slide": SubResource("Animation_x"), &"idle": 2}"#)
            .expect("a StringName-keyed dictionary must parse");
        match v {
            Variant::Dictionary(ref map) => {
                let d = map.to_dict();
                assert_eq!(d.len(), 2);
                // Stored as a plain String key: in Godot a String and a
                // StringName of the same text ARE the same dictionary key, and
                // every consumer here matches on `Variant::String`.
                assert_eq!(
                    d["slide"],
                    Variant::String("SubResource:Animation_x".into())
                );
                assert_eq!(d["idle"], Variant::Int(2));
            }
            other => panic!("expected Dictionary, got {other:?}"),
        }
    }

    /// A key that is neither quoted nor a StringName is still refused — the
    /// ampersand must not have turned the check off.
    #[test]
    fn parse_dictionary_still_rejects_an_unquoted_key() {
        assert!(parse_variant_value(r#"{slide: 1}"#).is_err());
        assert!(parse_variant_value(r#"{&slide: 1}"#).is_err());
    }

    /// ⚠️ **The bug this half fixes.** Godot's text writer puts every Dictionary
    /// and Array on several lines, so an `AnimationLibrary`'s `_data` and an
    /// `Animation`'s `tracks/N/keys` arrive split across lines. Before
    /// pat-mgixk.1 the first line's value was the bare `{`,
    /// `parse_variant_value` rejected it, and the error failed the WHOLE
    /// file — no `.tres` the editor actually wrote could be loaded.
    #[test]
    fn parse_tres_with_multiline_dictionary_and_array_values() {
        let source = r#"
[gd_resource type="AnimationLibrary" load_steps=2 format=3]

[sub_resource type="Animation" id="Animation_slide"]
resource_name = "slide"
length = 1.0
tracks/0/keys = {
"times": PackedFloat32Array(0, 1),
"transitions": PackedFloat32Array(1, 1),
"update": 0,
"values": [Vector2(0, 0), Vector2(100, 0)]
}

[resource]
_data = {
&"slide": SubResource("Animation_slide")
}
"#;
        let res = TresLoader::new()
            .parse_str(source, "res://lib.tres")
            .expect("a .tres with multi-line values must load");
        assert_eq!(res.class_name, "AnimationLibrary");

        // The library's `_data` survived as a dictionary, not as an error.
        match res.get_property("_data") {
            Some(Variant::Dictionary(map)) => {
                assert_eq!(
                    map.to_dict()["slide"],
                    Variant::String("SubResource:Animation_slide".into())
                );
            }
            other => panic!("_data should be a Dictionary, got {other:?}"),
        }

        // And the sub-resource's own multi-line value did too, with the
        // following `[resource]` header NOT swallowed by the continuation.
        let anim = res
            .subresources
            .get("Animation_slide")
            .expect("the Animation sub-resource must still be registered");
        assert_eq!(anim.class_name, "Animation");
        assert_eq!(
            anim.get_property("resource_name"),
            Some(&Variant::String("slide".into()))
        );
        match anim.get_property("tracks/0/keys") {
            Some(Variant::Dictionary(map)) => {
                let d = map.to_dict();
                assert_eq!(d["update"], Variant::Int(0));
                match &d["values"] {
                    Variant::Array(items) => assert_eq!(items.len(), 2),
                    other => panic!("values should be an Array, got {other:?}"),
                }
            }
            other => panic!("tracks/0/keys should be a Dictionary, got {other:?}"),
        }
    }

    /// A quoted `{` must not open a bracket level — otherwise one string value
    /// swallows the rest of the file.
    #[test]
    fn bracket_depth_ignores_brackets_inside_strings() {
        assert_eq!(bracket_depth(r#""{""#), 0);
        assert_eq!(bracket_depth(r#"{"a": "]["#), 1);
        assert_eq!(bracket_depth(r#"{"a": 1}"#), 0);
        assert_eq!(bracket_depth(r#"}"#), -1);
        // An escaped quote does not end the string.
        assert_eq!(bracket_depth(r#""a\"{""#), 0);
    }

    #[test]
    fn parse_empty_dictionary() {
        assert_eq!(
            parse_variant_value("{}").unwrap(),
            Variant::Dictionary(Default::default())
        );
    }

    #[test]
    fn parse_array_with_ext_resource() {
        let v = parse_variant_value(r#"[ExtResource("1"), ExtResource("2")]"#).unwrap();
        match v {
            Variant::Array(ref items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items.to_vec()[0], Variant::String("ExtResource:1".into()));
                assert_eq!(items.to_vec()[1], Variant::String("ExtResource:2".into()));
            }
            other => panic!("expected Array, got {other:?}"),
        }
    }

    #[test]
    fn parse_simple_tres() {
        let source = r#"
[gd_resource type="Resource" format=3 uid="uid://abc123"]

[ext_resource type="Texture2D" uid="uid://xyz" path="res://icon.png" id="1"]

[sub_resource type="StyleBoxFlat" id="StyleBoxFlat_abc"]
bg_color = Color(0.2, 0.3, 0.4, 1)

[resource]
name = "MyResource"
value = 42
position = Vector2(10, 20)
"#;
        let loader = TresLoader::new();
        let res = loader.parse_str(source, "res://test.tres").unwrap();

        assert_eq!(res.class_name, "Resource");
        assert_eq!(res.path, "res://test.tres");
        assert!(res.uid.is_valid());

        // Properties
        assert_eq!(
            res.get_property("name"),
            Some(&Variant::String("MyResource".into()))
        );
        assert_eq!(res.get_property("value"), Some(&Variant::Int(42)));
        assert_eq!(
            res.get_property("position"),
            Some(&Variant::Vector2(Vector2::new(10.0, 20.0)))
        );

        // Ext resource
        assert_eq!(res.ext_resources.len(), 1);
        let ext = &res.ext_resources["1"];
        assert_eq!(ext.resource_type, "Texture2D");
        assert_eq!(ext.path, "res://icon.png");

        // Sub resource
        assert_eq!(res.subresources.len(), 1);
        let sub = &res.subresources["StyleBoxFlat_abc"];
        assert_eq!(sub.class_name, "StyleBoxFlat");
        assert_eq!(
            sub.get_property("bg_color"),
            Some(&Variant::Color(Color::new(0.2, 0.3, 0.4, 1.0)))
        );
    }

    // -- Malformed .tres inputs -----------------------------------------------

    #[test]
    fn parse_tres_missing_header() {
        let source = r#"
[resource]
name = "NoHeader"
"#;
        let loader = TresLoader::new();
        // Should parse but with default class name "Resource"
        let res = loader.parse_str(source, "res://no_header.tres").unwrap();
        assert_eq!(res.class_name, "Resource");
        assert_eq!(
            res.get_property("name"),
            Some(&Variant::String("NoHeader".into()))
        );
    }

    #[test]
    fn parse_tres_no_resource_section() {
        let source = r#"[gd_resource type="Resource" format=3]
"#;
        let loader = TresLoader::new();
        let res = loader.parse_str(source, "res://empty.tres").unwrap();
        assert_eq!(res.class_name, "Resource");
        assert_eq!(res.property_count(), 0);
    }

    #[test]
    fn parse_tres_invalid_value_syntax() {
        let source = r#"[gd_resource type="Resource" format=3]

[resource]
good = 42
bad = Vector2(not_a_number, oops)
"#;
        let loader = TresLoader::new();
        let result = loader.parse_str(source, "res://bad.tres");
        assert!(result.is_err());
    }

    #[test]
    fn parse_tres_only_comments_and_blank_lines() {
        let source = r#"
; This is a comment
; Another comment

"#;
        let loader = TresLoader::new();
        let res = loader.parse_str(source, "res://comments.tres").unwrap();
        assert_eq!(res.property_count(), 0);
    }

    #[test]
    fn parse_tres_unknown_section_ignored() {
        let source = r#"[gd_resource type="Resource" format=3]

[some_unknown_section]
ignored_key = 42

[resource]
kept = 1
"#;
        let loader = TresLoader::new();
        let res = loader.parse_str(source, "res://unknown.tres").unwrap();
        assert_eq!(res.get_property("kept"), Some(&Variant::Int(1)));
        assert_eq!(res.get_property("ignored_key"), None);
    }

    #[test]
    fn parse_color_rgb_three_components() {
        let v = parse_variant_value("Color(1, 0.5, 0)").unwrap();
        assert_eq!(v, Variant::Color(Color::rgb(1.0, 0.5, 0.0)));
    }

    #[test]
    fn parse_invalid_vector2_arg_count() {
        let result = parse_variant_value("Vector2(1)");
        assert!(result.is_err());
    }

    #[test]
    fn parse_invalid_vector3_arg_count() {
        let result = parse_variant_value("Vector3(1, 2)");
        assert!(result.is_err());
    }

    #[test]
    fn parse_invalid_color_arg_count() {
        let result = parse_variant_value("Color(1, 2)");
        assert!(result.is_err());
    }

    #[test]
    fn parse_invalid_rect2_arg_count() {
        let result = parse_variant_value("Rect2(1, 2, 3)");
        assert!(result.is_err());
    }

    #[test]
    fn parse_invalid_transform2d_arg_count() {
        let result = parse_variant_value("Transform2D(1, 0, 0, 1, 10)");
        assert!(result.is_err());
    }

    #[test]
    fn parse_unrecognized_value() {
        let result = parse_variant_value("totally_unknown_value");
        assert!(result.is_err());
    }

    #[test]
    fn parse_tres_with_new_types() {
        let source = r#"
[gd_resource type="Resource" format=3 uid="uid://newtest"]

[ext_resource type="Texture2D" uid="uid://tex1" path="res://icon.png" id="tex_1"]

[sub_resource type="RectangleShape2D" id="shape_1"]
size = Vector2(64, 64)

[resource]
region = Rect2(0, 0, 256, 256)
xform = Transform2D(1, 0, 0, 1, 50, 100)
target = NodePath("Player/Sprite")
texture = ExtResource("tex_1")
style = SubResource("shape_1")
items = [1, "two", Vector2(3, 4)]
metadata = {"version": 2, "label": "test"}
nothing = null
"#;
        let loader = TresLoader::new();
        let res = loader.parse_str(source, "res://new.tres").unwrap();

        assert_eq!(
            res.get_property("region"),
            Some(&Variant::Rect2(Rect2::new(
                Vector2::new(0.0, 0.0),
                Vector2::new(256.0, 256.0)
            )))
        );
        assert_eq!(
            res.get_property("xform"),
            Some(&Variant::Transform2D(Transform2D {
                x: Vector2::new(1.0, 0.0),
                y: Vector2::new(0.0, 1.0),
                origin: Vector2::new(50.0, 100.0),
            }))
        );
        assert_eq!(
            res.get_property("target"),
            Some(&Variant::NodePath(gdcore::node_path::NodePath::new(
                "Player/Sprite"
            )))
        );
        assert_eq!(
            res.get_property("texture"),
            Some(&Variant::String("ExtResource:tex_1".into()))
        );
        assert_eq!(
            res.get_property("style"),
            Some(&Variant::String("SubResource:shape_1".into()))
        );
        assert_eq!(res.get_property("nothing"), Some(&Variant::Nil));

        // Array property
        match res.get_property("items") {
            Some(Variant::Array(items)) => {
                assert_eq!(items.len(), 3);
                assert_eq!(items.to_vec()[0], Variant::Int(1));
                assert_eq!(items.to_vec()[1], Variant::String("two".into()));
                assert_eq!(items.to_vec()[2], Variant::Vector2(Vector2::new(3.0, 4.0)));
            }
            other => panic!("expected Array, got {other:?}"),
        }

        // Dictionary property
        match res.get_property("metadata") {
            Some(Variant::Dictionary(map)) => {
                assert_eq!(map.len(), 2);
                assert_eq!(map.to_dict()["version"], Variant::Int(2));
                assert_eq!(map.to_dict()["label"], Variant::String("test".into()));
            }
            other => panic!("expected Dictionary, got {other:?}"),
        }
    }

    #[test]
    fn parse_tres_error_includes_line_number() {
        let source = "[gd_resource type=\"Resource\" format=3]\n\n[resource]\nvalue = totally_invalid_value\n";
        let loader = TresLoader;
        let result = loader.parse_str(source, "test.tres");
        match result {
            Err(e) => {
                let msg = format!("{e}");
                assert!(
                    msg.contains("line 4"),
                    "error should mention line 4, got: {msg}"
                );
                assert!(
                    msg.contains("test.tres"),
                    "error should mention file path, got: {msg}"
                );
            }
            Ok(_) => panic!("expected error for invalid value"),
        }
    }
}
