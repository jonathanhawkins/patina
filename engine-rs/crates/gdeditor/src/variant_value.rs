//! **Variant type coverage** for settings/export serialization (pat-pk7au).
//!
//! Every supported Variant type must round-trip losslessly through the settings
//! and preset stores. This module defines a `Variant` covering the core types
//! (nil, bool, int, float, string, Vector2/3, Color, Array, Dictionary) and a
//! keyed `VariantStore` that serializes to and from JSON without loss.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A serializable Variant value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Variant {
    /// No value.
    Nil,
    /// A boolean.
    Bool(bool),
    /// A 64-bit integer.
    Int(i64),
    /// A 32-bit float.
    Float(f32),
    /// A UTF-8 string.
    String(String),
    /// A 2D vector.
    Vector2(f32, f32),
    /// A 3D vector.
    Vector3(f32, f32, f32),
    /// An RGBA color.
    Color(f32, f32, f32, f32),
    /// An ordered array of variants.
    Array(Vec<Variant>),
    /// A string-keyed dictionary of variants.
    Dictionary(BTreeMap<String, Variant>),
}

impl Variant {
    /// The Variant type name (as used in settings UIs).
    pub fn type_name(&self) -> &'static str {
        match self {
            Variant::Nil => "Nil",
            Variant::Bool(_) => "bool",
            Variant::Int(_) => "int",
            Variant::Float(_) => "float",
            Variant::String(_) => "String",
            Variant::Vector2(..) => "Vector2",
            Variant::Vector3(..) => "Vector3",
            Variant::Color(..) => "Color",
            Variant::Array(_) => "Array",
            Variant::Dictionary(_) => "Dictionary",
        }
    }
}

/// A keyed store of Variant settings that serializes without loss.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VariantStore {
    values: BTreeMap<String, Variant>,
}

impl VariantStore {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a keyed value.
    pub fn set(&mut self, key: impl Into<String>, value: Variant) {
        self.values.insert(key.into(), value);
    }

    /// Gets a keyed value.
    pub fn get(&self, key: &str) -> Option<&Variant> {
        self.values.get(key)
    }

    /// The number of stored values.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Serializes the store to JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("VariantStore serializes")
    }

    /// Loads the store from JSON.
    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-pk7au): each supported Variant type set as a setting
    /// serializes and deserializes without loss.
    #[test]
    fn systems_variant_coverage_round_trips() {
        let mut store = VariantStore::new();
        store.set("nil", Variant::Nil);
        store.set("bool", Variant::Bool(true));
        store.set("int", Variant::Int(-42));
        store.set("float", Variant::Float(3.5));
        store.set("string", Variant::String("hello".to_string()));
        store.set("vec2", Variant::Vector2(1.0, 2.0));
        store.set("vec3", Variant::Vector3(1.0, 2.0, 3.0));
        store.set("color", Variant::Color(1.0, 0.5, 0.25, 1.0));
        store.set(
            "array",
            Variant::Array(vec![Variant::Int(1), Variant::String("x".to_string())]),
        );
        let mut dict = BTreeMap::new();
        dict.insert("k".to_string(), Variant::Bool(false));
        dict.insert("n".to_string(), Variant::Float(0.25));
        store.set("dict", Variant::Dictionary(dict));
        assert_eq!(store.len(), 10);

        // The whole store round-trips through JSON without loss.
        let json = store.to_json();
        let loaded = VariantStore::from_json(&json).expect("loads");
        assert_eq!(loaded, store);

        // Spot-check each type and its declared name.
        assert_eq!(loaded.get("bool"), Some(&Variant::Bool(true)));
        assert_eq!(loaded.get("int"), Some(&Variant::Int(-42)));
        assert_eq!(loaded.get("float"), Some(&Variant::Float(3.5)));
        assert_eq!(loaded.get("vec3"), Some(&Variant::Vector3(1.0, 2.0, 3.0)));
        assert_eq!(
            loaded.get("color"),
            Some(&Variant::Color(1.0, 0.5, 0.25, 1.0))
        );
        assert_eq!(loaded.get("nil").unwrap().type_name(), "Nil");
        match loaded.get("array") {
            Some(Variant::Array(a)) => {
                assert_eq!(a.len(), 2);
                assert_eq!(a[0], Variant::Int(1));
            }
            other => panic!("expected Array, got {other:?}"),
        }
        match loaded.get("dict") {
            Some(Variant::Dictionary(d)) => {
                assert_eq!(d.get("k"), Some(&Variant::Bool(false)));
                assert_eq!(d.get("n"), Some(&Variant::Float(0.25)));
            }
            other => panic!("expected Dictionary, got {other:?}"),
        }
    }

    /// Every Variant type reports a distinct type name.
    #[test]
    fn variant_type_names() {
        let samples = [
            Variant::Nil,
            Variant::Bool(false),
            Variant::Int(0),
            Variant::Float(0.0),
            Variant::String(String::new()),
            Variant::Vector2(0.0, 0.0),
            Variant::Vector3(0.0, 0.0, 0.0),
            Variant::Color(0.0, 0.0, 0.0, 0.0),
            Variant::Array(Vec::new()),
            Variant::Dictionary(BTreeMap::new()),
        ];
        let names: Vec<&str> = samples.iter().map(|v| v.type_name()).collect();
        let unique: std::collections::BTreeSet<&str> = names.iter().copied().collect();
        assert_eq!(names.len(), unique.len(), "each type name is distinct");
    }
}
