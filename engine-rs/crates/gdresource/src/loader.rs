//! Resource loader traits and implementations.
//!
//! Provides the [`ResourceLoader`] trait and a [`TresLoader`] that parses
//! Godot's `.tres` text resource format (simplified subset).

use std::collections::HashMap;
use std::sync::Arc;

use gdcore::error::{EngineError, EngineResult};
use gdcore::math::{Color, Vector2, Vector3};
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
            resource
                .subresources
                .insert(current_sub_id, Arc::new(sub));
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
        let inner = &s[1..s.len() - 1];
        // Handle basic escape sequences.
        let unescaped = inner
            .replace("\\n", "\n")
            .replace("\\t", "\t")
            .replace("\\\"", "\"")
            .replace("\\\\", "\\");
        return Ok(Variant::String(unescaped));
    }

    // Boolean
    if s == "true" {
        return Ok(Variant::Bool(true));
    }
    if s == "false" {
        return Ok(Variant::Bool(false));
    }

    // Null
    if s == "null" || s == "nil" {
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
fn try_strip_call<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let s = s.trim();
    if s.starts_with(name) && s.ends_with(')') {
        let rest = &s[name.len()..];
        if rest.starts_with('(') {
            return Some(&rest[1..rest.len() - 1]);
        }
    }
    None
}

/// Splits comma-separated arguments, trimming whitespace.
fn split_args(s: &str) -> Vec<String> {
    s.split(',').map(|p| p.trim().to_string()).collect()
}

/// Parses a float from a string, providing a nice error.
fn parse_f32(s: &str) -> EngineResult<f32> {
    s.trim()
        .parse::<f32>()
        .map_err(|_| EngineError::Parse(format!("expected float, got: {s}")))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
