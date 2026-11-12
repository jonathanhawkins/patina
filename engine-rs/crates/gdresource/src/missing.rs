//! Lossless placeholders for unrecognized classes: `MissingNode` and
//! `MissingResource`.
//!
//! When a `.tscn`/`.tres` references a class or script the engine doesn't
//! recognize, Godot substitutes a `MissingNode`/`MissingResource` that records
//! the `original_class` name plus every property verbatim, so re-saving the
//! file preserves the data instead of silently dropping it. These types hold
//! that state; the loader emits them for unknown types and the saver re-emits
//! the stored class + properties.
//!
//! Properties are kept as their **raw `.tscn` value literals** in first-seen
//! order, which is what makes the round-trip byte-for-byte faithful (no
//! parse/reformat step to lose precision or ordering).

/// An ordered bag of raw property literals preserved for an unrecognized class.
///
/// Insertion order is retained so the saver can re-emit properties in the order
/// they were read; re-setting an existing property overwrites its value in
/// place without changing its position.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PreservedProperties {
    entries: Vec<(String, String)>,
}

impl PreservedProperties {
    /// Creates an empty property bag.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records `name`'s raw value literal, overwriting in place if `name` was
    /// already set (preserving its original position).
    pub fn set(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        for entry in &mut self.entries {
            if entry.0 == name {
                entry.1 = value.into();
                return;
            }
        }
        self.entries.push((name, value.into()));
    }

    /// The raw value literal recorded for `name`, if any.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(n, _)| n.as_str() == name)
            .map(|(_, v)| v.as_str())
    }

    /// Whether `name` has a recorded value.
    pub fn contains(&self, name: &str) -> bool {
        self.entries.iter().any(|(n, _)| n.as_str() == name)
    }

    /// Number of recorded properties.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no properties are recorded.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The `(name, raw_value)` pairs in preserved order, for verbatim re-emit.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(n, v)| (n.as_str(), v.as_str()))
    }
}

/// Placeholder for a scene node whose class is unrecognized, preserving its
/// original class and properties for lossless round-trip (Godot `MissingNode`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingNode {
    /// The name of the class the file referenced but the engine lacks.
    pub original_class: String,
    /// Whether unknown properties are recorded (Godot `recording_properties`).
    /// When `false` the loader should not accumulate properties.
    pub recording_properties: bool,
    /// The preserved raw properties.
    pub properties: PreservedProperties,
}

impl MissingNode {
    /// Creates a placeholder for `original_class`, recording properties.
    pub fn new(original_class: impl Into<String>) -> Self {
        Self {
            original_class: original_class.into(),
            recording_properties: true,
            properties: PreservedProperties::new(),
        }
    }

    /// Records a raw property value, honoring [`recording_properties`].
    ///
    /// [`recording_properties`]: MissingNode::recording_properties
    pub fn set_property(&mut self, name: impl Into<String>, value: impl Into<String>) {
        if self.recording_properties {
            self.properties.set(name, value);
        }
    }
}

/// Placeholder for a resource whose class is unrecognized, preserving its
/// original class and properties for lossless round-trip
/// (Godot `MissingResource`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingResource {
    /// The name of the class the file referenced but the engine lacks.
    pub original_class: String,
    /// Whether unknown properties are recorded (Godot `recording_properties`).
    pub recording_properties: bool,
    /// The preserved raw properties.
    pub properties: PreservedProperties,
}

impl MissingResource {
    /// Creates a placeholder for `original_class`, recording properties.
    pub fn new(original_class: impl Into<String>) -> Self {
        Self {
            original_class: original_class.into(),
            recording_properties: true,
            properties: PreservedProperties::new(),
        }
    }

    /// Records a raw property value, honoring [`recording_properties`].
    ///
    /// [`recording_properties`]: MissingResource::recording_properties
    pub fn set_property(&mut self, name: impl Into<String>, value: impl Into<String>) {
        if self.recording_properties {
            self.properties.set(name, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserved_properties_order_and_overwrite() {
        let mut p = PreservedProperties::new();
        p.set("position", "Vector2(1, 2)");
        p.set("custom", "42");
        p.set("position", "Vector2(3, 4)"); // overwrite in place

        assert_eq!(p.len(), 2);
        assert_eq!(p.get("position"), Some("Vector2(3, 4)"));
        assert_eq!(p.get("custom"), Some("42"));
        assert_eq!(p.get("missing"), None);
        assert!(p.contains("custom"));
        // Insertion order preserved (position stays first despite the overwrite).
        let pairs: Vec<(&str, &str)> = p.iter().collect();
        assert_eq!(pairs, vec![("position", "Vector2(3, 4)"), ("custom", "42")]);
    }

    #[test]
    fn missing_node_records_original_class_and_props() {
        let mut n = MissingNode::new("MyCustomNode");
        assert_eq!(n.original_class, "MyCustomNode");
        assert!(n.recording_properties);
        n.set_property("health", "100");
        n.set_property("name", "\"Enemy\"");
        assert_eq!(n.properties.len(), 2);
        assert_eq!(n.properties.get("health"), Some("100"));

        // With recording off, properties are not accumulated (lossy is opt-in).
        n.recording_properties = false;
        n.set_property("ignored", "1");
        assert!(!n.properties.contains("ignored"));
    }

    #[test]
    fn missing_resource_preserves_class_and_props() {
        let mut r = MissingResource::new("MyCustomResource");
        r.set_property("data", "PackedInt32Array(1, 2, 3)");
        assert_eq!(r.original_class, "MyCustomResource");
        assert_eq!(r.properties.get("data"), Some("PackedInt32Array(1, 2, 3)"));
        assert_eq!(
            r.properties.iter().collect::<Vec<_>>(),
            vec![("data", "PackedInt32Array(1, 2, 3)")]
        );
    }
}
