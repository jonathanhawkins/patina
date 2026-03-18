//! Resource loader traits and implementations.
//!
//! Provides the [`ResourceLoader`] trait and a [`TresLoader`] that parses
//! Godot's `.tres` text resource format (simplified subset).

use std::collections::HashMap;
use std::sync::Arc;

use gdcore::error::{EngineError, EngineResult};
use gdcore::math::{Color, Rect2, Transform2D, Vector2, Vector3};
use gdcore::node_path::NodePath;
use gdcore::ResourceUid;
use gdvariant::Variant;

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

        for line in source.lines() {
            let line = line.trim();

            // Skip empty lines and comments.
            if line.is_empty() || line.starts_with(';') {
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
                    current_section = Section::None;
                }
                continue;
            }

            // Property lines: key = value
            if let Some((key, value_str)) = line.split_once('=') {
                let key = key.trim();
                let value_str = value_str.trim();
                let value = parse_variant_value(value_str)?;

                match current_section {
                    Section::Resource => {
                        resource.set_property(key, value);
                    }
                    Section::SubResource => {
                        if let Some(ref mut sub) = current_sub {
                            sub.set_property(key, value);
                        }
                    }
                    _ => {}
                }
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

#[derive(Debug, Clone, Copy, PartialEq)]
enum Section {
    None,
    GdResource,
    ExtResource,
    SubResource,
    Resource,
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
/// For simplicity we hash the string portion to produce a numeric UID.
fn parse_uid_string(uid_str: &str) -> ResourceUid {
    if let Some(rest) = uid_str.strip_prefix("uid://") {
        // Simple hash to produce a stable numeric UID.
        let hash: i64 = rest
            .bytes()
            .fold(0i64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as i64));
        ResourceUid::new(hash)
    } else {
        ResourceUid::INVALID
    }
}

/// Parses a single property value string into a [`Variant`].
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
    // Stored as Variant::Array with the appropriate element types.
    if let Some(inner) = try_strip_call(s, "PackedByteArray") {
        return parse_packed_int_array(inner);
    }
    if let Some(inner) = try_strip_call(s, "PackedInt32Array") {
        return parse_packed_int_array(inner);
    }
    if let Some(inner) = try_strip_call(s, "PackedInt64Array") {
        return parse_packed_int_array(inner);
    }
    if let Some(inner) = try_strip_call(s, "PackedFloat32Array") {
        return parse_packed_float_array(inner);
    }
    if let Some(inner) = try_strip_call(s, "PackedFloat64Array") {
        return parse_packed_float_array(inner);
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

    // Vector2i(x, y)
    if let Some(inner) = try_strip_call(s, "Vector2i") {
        let parts = split_args(inner);
        if parts.len() == 2 {
            let x = parse_f32(&parts[0])?;
            let y = parse_f32(&parts[1])?;
            return Ok(Variant::Vector2(Vector2::new(x, y)));
        }
        return Err(EngineError::Parse(format!("invalid Vector2i: {s}")));
    }

    // Vector3i(x, y, z)
    if let Some(inner) = try_strip_call(s, "Vector3i") {
        let parts = split_args(inner);
        if parts.len() == 3 {
            let x = parse_f32(&parts[0])?;
            let y = parse_f32(&parts[1])?;
            let z = parse_f32(&parts[2])?;
            return Ok(Variant::Vector3(Vector3::new(x, y, z)));
        }
        return Err(EngineError::Parse(format!("invalid Vector3i: {s}")));
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

    // Float
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
        return Ok(Variant::Array(Vec::new()));
    }
    let elements = split_args(s);
    let mut result = Vec::with_capacity(elements.len());
    for elem in &elements {
        result.push(parse_variant_value(elem)?);
    }
    Ok(Variant::Array(result))
}

/// Parses a dictionary body (content between `{` and `}`) into `Variant::Dictionary`.
///
/// Expects `"key": value` pairs separated by commas. Keys must be quoted strings.
fn parse_dictionary(s: &str) -> EngineResult<Variant> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Variant::Dictionary(HashMap::new()));
    }
    let pairs = split_args(s);
    let mut map = HashMap::new();
    for pair in &pairs {
        let pair = pair.trim();
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
    Ok(Variant::Dictionary(map))
}

/// Parses a packed integer array body (e.g. `"1, 2, 3"` or `""`) into `Variant::Array`.
fn parse_packed_int_array(s: &str) -> EngineResult<Variant> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Variant::Array(Vec::new()));
    }
    let parts = split_args(s);
    let mut result = Vec::with_capacity(parts.len());
    for part in &parts {
        let i = part.trim().parse::<i64>().map_err(|_| {
            EngineError::Parse(format!("expected int in packed array, got: {part}"))
        })?;
        result.push(Variant::Int(i));
    }
    Ok(Variant::Array(result))
}

/// Parses a packed float array body into `Variant::Array`.
fn parse_packed_float_array(s: &str) -> EngineResult<Variant> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Variant::Array(Vec::new()));
    }
    let parts = split_args(s);
    let mut result = Vec::with_capacity(parts.len());
    for part in &parts {
        let f = part.trim().parse::<f64>().map_err(|_| {
            EngineError::Parse(format!("expected float in packed array, got: {part}"))
        })?;
        result.push(Variant::Float(f));
    }
    Ok(Variant::Array(result))
}

/// Parses a packed string array body into `Variant::Array`.
fn parse_packed_string_array(s: &str) -> EngineResult<Variant> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Variant::Array(Vec::new()));
    }
    let parts = split_args(s);
    let mut result = Vec::with_capacity(parts.len());
    for part in &parts {
        let part = part.trim();
        if part.starts_with('"') && part.ends_with('"') && part.len() >= 2 {
            result.push(Variant::String(unescape_string(&part[1..part.len() - 1])));
        } else {
            return Err(EngineError::Parse(format!(
                "expected quoted string in packed array, got: {part}"
            )));
        }
    }
    Ok(Variant::Array(result))
}

/// Parses a packed Vector2 array body (flat pairs: `x1, y1, x2, y2, ...`) into `Variant::Array`.
fn parse_packed_vector2_array(s: &str) -> EngineResult<Variant> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Variant::Array(Vec::new()));
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
        result.push(Variant::Vector2(Vector2::new(x, y)));
    }
    Ok(Variant::Array(result))
}

/// Parses a packed Vector3 array body (flat triples: `x1, y1, z1, ...`) into `Variant::Array`.
fn parse_packed_vector3_array(s: &str) -> EngineResult<Variant> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Variant::Array(Vec::new()));
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
        result.push(Variant::Vector3(Vector3::new(x, y, z)));
    }
    Ok(Variant::Array(result))
}

/// Parses a packed Color array body (flat quads: `r1, g1, b1, a1, ...`) into `Variant::Array`.
fn parse_packed_color_array(s: &str) -> EngineResult<Variant> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Variant::Array(Vec::new()));
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
        result.push(Variant::Color(Color::new(r, g, b, a)));
    }
    Ok(Variant::Array(result))
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
                assert_eq!(items[0], Variant::Int(1));
                assert_eq!(items[1], Variant::Float(2.5));
                assert_eq!(items[2], Variant::String("three".into()));
                assert_eq!(items[3], Variant::Vector2(Vector2::new(1.0, 2.0)));
                assert_eq!(items[4], Variant::Bool(true));
                assert_eq!(items[5], Variant::Nil);
            }
            other => panic!("expected Array, got {other:?}"),
        }
    }

    #[test]
    fn parse_empty_array() {
        assert_eq!(parse_variant_value("[]").unwrap(), Variant::Array(vec![]));
    }

    #[test]
    fn parse_dictionary() {
        let v = parse_variant_value(r#"{"name": "Player", "health": 100, "alive": true}"#).unwrap();
        match v {
            Variant::Dictionary(ref map) => {
                assert_eq!(map.len(), 3);
                assert_eq!(map["name"], Variant::String("Player".into()));
                assert_eq!(map["health"], Variant::Int(100));
                assert_eq!(map["alive"], Variant::Bool(true));
            }
            other => panic!("expected Dictionary, got {other:?}"),
        }
    }

    #[test]
    fn parse_empty_dictionary() {
        assert_eq!(
            parse_variant_value("{}").unwrap(),
            Variant::Dictionary(HashMap::new())
        );
    }

    #[test]
    fn parse_array_with_ext_resource() {
        let v = parse_variant_value(r#"[ExtResource("1"), ExtResource("2")]"#).unwrap();
        match v {
            Variant::Array(ref items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0], Variant::String("ExtResource:1".into()));
                assert_eq!(items[1], Variant::String("ExtResource:2".into()));
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
                assert_eq!(items[0], Variant::Int(1));
                assert_eq!(items[1], Variant::String("two".into()));
                assert_eq!(items[2], Variant::Vector2(Vector2::new(3.0, 4.0)));
            }
            other => panic!("expected Array, got {other:?}"),
        }

        // Dictionary property
        match res.get_property("metadata") {
            Some(Variant::Dictionary(map)) => {
                assert_eq!(map.len(), 2);
                assert_eq!(map["version"], Variant::Int(2));
                assert_eq!(map["label"], Variant::String("test".into()));
            }
            other => panic!("expected Dictionary, got {other:?}"),
        }
    }
}
