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
            write!(out, " uid=\"uid://{}\"", resource.uid.raw())
                .map_err(|e| EngineError::Parse(e.to_string()))?;
        }
        writeln!(out, " format=3]")
            .map_err(|e| EngineError::Parse(e.to_string()))?;

        // [ext_resource] sections
        if !resource.ext_resources.is_empty() {
            writeln!(out).map_err(|e| EngineError::Parse(e.to_string()))?;
            let mut ext_ids: Vec<_> = resource.ext_resources.keys().collect();
            ext_ids.sort();
            for id in ext_ids {
                let ext = &resource.ext_resources[id];
                writeln!(
                    out,
                    "[ext_resource type=\"{}\" uid=\"{}\" path=\"{}\" id=\"{}\"]",
                    ext.resource_type, ext.uid, ext.path, ext.id
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
            writeln!(out, "[resource]")
                .map_err(|e| EngineError::Parse(e.to_string()))?;
            write_properties(&mut out, resource)?;
        }

        Ok(out)
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
    let keys = resource.sorted_property_keys();
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
    match v {
        Variant::Nil => "null".to_string(),
        Variant::Bool(b) => b.to_string(),
        Variant::Int(i) => i.to_string(),
        Variant::Float(f) => format_float(*f),
        Variant::String(s) => {
            let escaped = s
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\t', "\\t");
            format!("\"{escaped}\"")
        }
        Variant::Vector2(v) => format!("Vector2({}, {})", format_f32(v.x), format_f32(v.y)),
        Variant::Vector3(v) => format!(
            "Vector3({}, {}, {})",
            format_f32(v.x),
            format_f32(v.y),
            format_f32(v.z)
        ),
        Variant::Color(c) => format!(
            "Color({}, {}, {}, {})",
            format_f32(c.r),
            format_f32(c.g),
            format_f32(c.b),
            format_f32(c.a)
        ),
        // Fallback for types we don't write in .tres format.
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
        assert!(output.contains("uid=\"uid://12345\""));
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
}
