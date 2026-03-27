//! INI-style configuration file, matching Godot's ConfigFile API.
//!
//! Supports loading and saving configuration data organized into
//! `[section]` blocks with `key=value` pairs, where values are
//! Godot Variant types serialized as text.

use std::collections::BTreeMap;

use gdvariant::Variant;

/// An INI-style configuration file with section/key/value storage.
///
/// Matches Godot's `ConfigFile` API surface: values are organized into
/// named sections, each containing key-value pairs where values are
/// [`Variant`]s.
#[derive(Debug, Clone, Default)]
pub struct ConfigFile {
    sections: BTreeMap<String, BTreeMap<String, Variant>>,
}

impl ConfigFile {
    /// Creates a new, empty ConfigFile.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a value for the given section and key.
    pub fn set_value(&mut self, section: &str, key: &str, value: Variant) {
        self.sections
            .entry(section.to_string())
            .or_default()
            .insert(key.to_string(), value);
    }

    /// Gets a value for the given section and key, or `None` if not found.
    pub fn get_value(&self, section: &str, key: &str) -> Option<&Variant> {
        self.sections.get(section)?.get(key)
    }

    /// Gets a value with a default fallback.
    pub fn get_value_or(&self, section: &str, key: &str, default: Variant) -> Variant {
        self.get_value(section, key).cloned().unwrap_or(default)
    }

    /// Returns `true` if the section exists.
    pub fn has_section(&self, section: &str) -> bool {
        self.sections.contains_key(section)
    }

    /// Returns `true` if the section has the given key.
    pub fn has_section_key(&self, section: &str, key: &str) -> bool {
        self.sections
            .get(section)
            .map_or(false, |s| s.contains_key(key))
    }

    /// Returns all section names in sorted order.
    pub fn get_sections(&self) -> Vec<&str> {
        self.sections.keys().map(String::as_str).collect()
    }

    /// Returns all keys in a section, in sorted order.
    pub fn get_section_keys(&self, section: &str) -> Vec<&str> {
        self.sections
            .get(section)
            .map(|s| s.keys().map(String::as_str).collect())
            .unwrap_or_default()
    }

    /// Removes a section and all its keys.
    pub fn erase_section(&mut self, section: &str) {
        self.sections.remove(section);
    }

    /// Removes a single key from a section.
    pub fn erase_section_key(&mut self, section: &str, key: &str) {
        if let Some(s) = self.sections.get_mut(section) {
            s.remove(key);
            if s.is_empty() {
                self.sections.remove(section);
            }
        }
    }

    /// Removes all sections and keys.
    pub fn clear(&mut self) {
        self.sections.clear();
    }

    /// Serializes the config to an INI-style string.
    ///
    /// Format matches Godot's ConfigFile.save() output:
    /// ```text
    /// [section]
    ///
    /// key=value
    /// other_key="string value"
    /// ```
    pub fn save_to_string(&self) -> String {
        let mut out = String::new();
        for (i, (section, keys)) in self.sections.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push('[');
            out.push_str(section);
            out.push_str("]\n\n");
            for (key, value) in keys {
                out.push_str(key);
                out.push('=');
                out.push_str(&variant_to_ini(value));
                out.push('\n');
            }
        }
        out
    }

    /// Parses an INI-style string into a ConfigFile.
    ///
    /// Supports `[section]` headers, `key=value` pairs, blank lines,
    /// and `; comment` / `# comment` lines.
    pub fn load_from_string(source: &str) -> Result<Self, String> {
        let mut cfg = ConfigFile::new();
        let mut current_section = String::new();

        for (line_num, raw_line) in source.lines().enumerate() {
            let line = raw_line.trim();

            // Skip empty lines and comments.
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }

            // Section header.
            if line.starts_with('[') {
                if let Some(end) = line.find(']') {
                    current_section = line[1..end].to_string();
                } else {
                    return Err(format!("line {}: unclosed section bracket", line_num + 1));
                }
                continue;
            }

            // Key=value pair.
            if let Some(eq_pos) = line.find('=') {
                let key = line[..eq_pos].trim();
                let val_str = line[eq_pos + 1..].trim();
                if current_section.is_empty() {
                    return Err(format!(
                        "line {}: key '{}' before any section",
                        line_num + 1,
                        key
                    ));
                }
                let value = ini_to_variant(val_str);
                cfg.set_value(&current_section, key, value);
            } else {
                return Err(format!("line {}: expected key=value, got '{}'", line_num + 1, line));
            }
        }

        Ok(cfg)
    }
}

/// Converts a Variant to its INI-style text representation.
fn variant_to_ini(v: &Variant) -> String {
    match v {
        Variant::Nil => "null".to_string(),
        Variant::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        Variant::Int(i) => i.to_string(),
        Variant::Float(f) => {
            if f.fract() == 0.0 {
                format!("{f:.1}")
            } else {
                f.to_string()
            }
        }
        Variant::String(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        Variant::Vector2(v) => format!("Vector2({}, {})", v.x, v.y),
        Variant::Vector3(v) => format!("Vector3({}, {}, {})", v.x, v.y, v.z),
        Variant::Color(c) => format!("Color({}, {}, {}, {})", c.r, c.g, c.b, c.a),
        _ => format!("{v}"),
    }
}

/// Parses an INI value string into a Variant.
fn ini_to_variant(s: &str) -> Variant {
    // Null
    if s == "null" {
        return Variant::Nil;
    }
    // Boolean
    if s == "true" {
        return Variant::Bool(true);
    }
    if s == "false" {
        return Variant::Bool(false);
    }
    // Quoted string
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        let inner = &s[1..s.len() - 1];
        return Variant::String(inner.replace("\\\"", "\"").replace("\\\\", "\\"));
    }
    // Integer
    if let Ok(i) = s.parse::<i64>() {
        return Variant::Int(i);
    }
    // Float
    if let Ok(f) = s.parse::<f64>() {
        return Variant::Float(f);
    }
    // Vector2(x, y)
    if let Some(inner) = s.strip_prefix("Vector2(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<f32> = inner.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        if parts.len() == 2 {
            return Variant::Vector2(gdcore::math::Vector2::new(parts[0], parts[1]));
        }
    }
    // Vector3(x, y, z)
    if let Some(inner) = s.strip_prefix("Vector3(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<f32> = inner.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        if parts.len() == 3 {
            return Variant::Vector3(gdcore::math::Vector3::new(parts[0], parts[1], parts[2]));
        }
    }
    // Color(r, g, b, a)
    if let Some(inner) = s.strip_prefix("Color(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<f32> = inner.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        if parts.len() == 4 {
            return Variant::Color(gdcore::math::Color::new(parts[0], parts[1], parts[2], parts[3]));
        }
    }
    // Fallback: treat as string
    Variant::String(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_empty() {
        let cfg = ConfigFile::new();
        assert!(cfg.get_sections().is_empty());
    }

    #[test]
    fn set_and_get_value() {
        let mut cfg = ConfigFile::new();
        cfg.set_value("display", "width", Variant::Int(1920));
        assert_eq!(cfg.get_value("display", "width"), Some(&Variant::Int(1920)));
    }

    #[test]
    fn get_missing_returns_none() {
        let cfg = ConfigFile::new();
        assert_eq!(cfg.get_value("missing", "key"), None);
    }

    #[test]
    fn get_value_or_returns_default() {
        let cfg = ConfigFile::new();
        assert_eq!(
            cfg.get_value_or("audio", "volume", Variant::Float(0.8)),
            Variant::Float(0.8)
        );
    }

    #[test]
    fn has_section_and_key() {
        let mut cfg = ConfigFile::new();
        cfg.set_value("input", "sensitivity", Variant::Float(1.0));
        assert!(cfg.has_section("input"));
        assert!(cfg.has_section_key("input", "sensitivity"));
        assert!(!cfg.has_section("missing"));
        assert!(!cfg.has_section_key("input", "missing"));
    }

    #[test]
    fn get_sections_sorted() {
        let mut cfg = ConfigFile::new();
        cfg.set_value("z_section", "a", Variant::Int(1));
        cfg.set_value("a_section", "b", Variant::Int(2));
        assert_eq!(cfg.get_sections(), vec!["a_section", "z_section"]);
    }

    #[test]
    fn get_section_keys_sorted() {
        let mut cfg = ConfigFile::new();
        cfg.set_value("s", "zebra", Variant::Int(1));
        cfg.set_value("s", "alpha", Variant::Int(2));
        assert_eq!(cfg.get_section_keys("s"), vec!["alpha", "zebra"]);
    }

    #[test]
    fn erase_section() {
        let mut cfg = ConfigFile::new();
        cfg.set_value("temp", "key", Variant::Bool(true));
        cfg.erase_section("temp");
        assert!(!cfg.has_section("temp"));
    }

    #[test]
    fn erase_section_key() {
        let mut cfg = ConfigFile::new();
        cfg.set_value("s", "a", Variant::Int(1));
        cfg.set_value("s", "b", Variant::Int(2));
        cfg.erase_section_key("s", "a");
        assert!(!cfg.has_section_key("s", "a"));
        assert!(cfg.has_section_key("s", "b"));
    }

    #[test]
    fn erase_last_key_removes_section() {
        let mut cfg = ConfigFile::new();
        cfg.set_value("s", "only", Variant::Int(1));
        cfg.erase_section_key("s", "only");
        assert!(!cfg.has_section("s"));
    }

    #[test]
    fn clear_removes_all() {
        let mut cfg = ConfigFile::new();
        cfg.set_value("a", "x", Variant::Int(1));
        cfg.set_value("b", "y", Variant::Int(2));
        cfg.clear();
        assert!(cfg.get_sections().is_empty());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let mut cfg = ConfigFile::new();
        cfg.set_value("display", "width", Variant::Int(1920));
        cfg.set_value("display", "height", Variant::Int(1080));
        cfg.set_value("display", "fullscreen", Variant::Bool(false));
        cfg.set_value("audio", "volume", Variant::Float(0.8));
        cfg.set_value("audio", "mute", Variant::Bool(false));
        cfg.set_value("player", "name", Variant::String("Alice".into()));

        let text = cfg.save_to_string();
        let loaded = ConfigFile::load_from_string(&text).unwrap();

        assert_eq!(loaded.get_value("display", "width"), Some(&Variant::Int(1920)));
        assert_eq!(loaded.get_value("display", "fullscreen"), Some(&Variant::Bool(false)));
        assert_eq!(loaded.get_value("audio", "volume"), Some(&Variant::Float(0.8)));
        assert_eq!(
            loaded.get_value("player", "name"),
            Some(&Variant::String("Alice".into()))
        );
    }

    #[test]
    fn load_with_comments() {
        let source = r#"
; This is a comment
# Another comment

[settings]
; Resolution
width=1280
height=720
"#;
        let cfg = ConfigFile::load_from_string(source).unwrap();
        assert_eq!(cfg.get_value("settings", "width"), Some(&Variant::Int(1280)));
    }

    #[test]
    fn load_error_on_key_before_section() {
        let source = "key=value\n";
        let result = ConfigFile::load_from_string(source);
        assert!(result.is_err());
    }

    #[test]
    fn load_error_on_unclosed_bracket() {
        let source = "[broken\nkey=value\n";
        let result = ConfigFile::load_from_string(source);
        assert!(result.is_err());
    }

    #[test]
    fn variant_roundtrip_bool() {
        assert_eq!(ini_to_variant(&variant_to_ini(&Variant::Bool(true))), Variant::Bool(true));
        assert_eq!(ini_to_variant(&variant_to_ini(&Variant::Bool(false))), Variant::Bool(false));
    }

    #[test]
    fn variant_roundtrip_int() {
        assert_eq!(ini_to_variant(&variant_to_ini(&Variant::Int(42))), Variant::Int(42));
        assert_eq!(ini_to_variant(&variant_to_ini(&Variant::Int(-100))), Variant::Int(-100));
    }

    #[test]
    fn variant_roundtrip_float() {
        let v = Variant::Float(3.14);
        let rt = ini_to_variant(&variant_to_ini(&v));
        if let Variant::Float(f) = rt {
            assert!((f - 3.14).abs() < 1e-10);
        } else {
            panic!("expected Float");
        }
    }

    #[test]
    fn variant_roundtrip_string() {
        let v = Variant::String("hello \"world\"".into());
        assert_eq!(ini_to_variant(&variant_to_ini(&v)), v);
    }

    #[test]
    fn variant_roundtrip_vector2() {
        let v = Variant::Vector2(gdcore::math::Vector2::new(10.0, 20.0));
        let rt = ini_to_variant(&variant_to_ini(&v));
        assert_eq!(rt, v);
    }

    #[test]
    fn variant_roundtrip_nil() {
        assert_eq!(ini_to_variant(&variant_to_ini(&Variant::Nil)), Variant::Nil);
    }

    #[test]
    fn overwrite_value() {
        let mut cfg = ConfigFile::new();
        cfg.set_value("s", "k", Variant::Int(1));
        cfg.set_value("s", "k", Variant::Int(2));
        assert_eq!(cfg.get_value("s", "k"), Some(&Variant::Int(2)));
    }

    #[test]
    fn multiple_sections_in_output() {
        let mut cfg = ConfigFile::new();
        cfg.set_value("a", "x", Variant::Int(1));
        cfg.set_value("b", "y", Variant::Int(2));
        let text = cfg.save_to_string();
        assert!(text.contains("[a]"));
        assert!(text.contains("[b]"));
        assert!(text.contains("x=1"));
        assert!(text.contains("y=2"));
    }
}
