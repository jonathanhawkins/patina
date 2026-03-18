//! Base Resource type and resource management.
//!
//! A [`Resource`] is a loadable, cacheable data object identified by a path
//! or UID. Resources hold typed properties as [`Variant`] values and may
//! contain sub-resources.

use std::collections::HashMap;
use std::sync::Arc;

use gdcore::ResourceUid;
use gdvariant::Variant;

/// A resource — a named bag of properties that can be loaded from and saved
/// to disk.
///
/// Resources are reference-counted via [`Arc`] so that multiple owners
/// (the cache, scene nodes, etc.) can share the same data cheaply.
#[derive(Debug, Clone)]
pub struct Resource {
    /// The resource's unique ID (may be [`ResourceUid::INVALID`]).
    pub uid: ResourceUid,
    /// The `res://` path this resource was loaded from (or saved to).
    pub path: String,
    /// The Godot class name (e.g. `"Resource"`, `"Texture2D"`).
    pub class_name: String,
    /// Key-value properties.
    properties: HashMap<String, Variant>,
    /// Sub-resources keyed by their section ID (e.g. `"StyleBoxFlat_abc"`).
    pub subresources: HashMap<String, Arc<Resource>>,
    /// External resource references keyed by their numeric ID string.
    pub ext_resources: HashMap<String, ExtResource>,
}

/// A reference to an external resource (from `[ext_resource]` sections).
#[derive(Debug, Clone, PartialEq)]
pub struct ExtResource {
    /// The Godot class type (e.g. `"Texture2D"`).
    pub resource_type: String,
    /// The UID string (e.g. `"uid://xyz"`).
    pub uid: String,
    /// The resource path (e.g. `"res://icon.png"`).
    pub path: String,
    /// The section ID (e.g. `"1"`).
    pub id: String,
}

impl Resource {
    /// Creates a new, empty resource.
    pub fn new(class_name: impl Into<String>) -> Self {
        Self {
            uid: ResourceUid::INVALID,
            path: String::new(),
            class_name: class_name.into(),
            properties: HashMap::new(),
            subresources: HashMap::new(),
            ext_resources: HashMap::new(),
        }
    }

    /// Sets a property value.
    pub fn set_property(&mut self, key: impl Into<String>, value: Variant) {
        self.properties.insert(key.into(), value);
    }

    /// Gets a property value by name, returning `None` if absent.
    pub fn get_property(&self, key: &str) -> Option<&Variant> {
        self.properties.get(key)
    }

    /// Returns an iterator over all properties.
    pub fn properties(&self) -> impl Iterator<Item = (&String, &Variant)> {
        self.properties.iter()
    }

    /// Returns the number of properties.
    pub fn property_count(&self) -> usize {
        self.properties.len()
    }

    /// Removes a property, returning its value if it existed.
    pub fn remove_property(&mut self, key: &str) -> Option<Variant> {
        self.properties.remove(key)
    }

    /// Returns a sorted list of property keys (for deterministic output).
    pub fn sorted_property_keys(&self) -> Vec<&String> {
        let mut keys: Vec<_> = self.properties.keys().collect();
        keys.sort();
        keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::Vector2;

    #[test]
    fn create_resource_and_set_properties() {
        let mut r = Resource::new("TestResource");
        assert_eq!(r.class_name, "TestResource");
        assert_eq!(r.property_count(), 0);

        r.set_property("name", Variant::String("hello".into()));
        r.set_property("value", Variant::Int(42));
        r.set_property("pos", Variant::Vector2(Vector2::new(1.0, 2.0)));

        assert_eq!(r.property_count(), 3);
        assert_eq!(r.get_property("name"), Some(&Variant::String("hello".into())));
        assert_eq!(r.get_property("value"), Some(&Variant::Int(42)));
        assert_eq!(r.get_property("missing"), None);
    }

    #[test]
    fn remove_property() {
        let mut r = Resource::new("Res");
        r.set_property("x", Variant::Int(10));
        assert_eq!(r.remove_property("x"), Some(Variant::Int(10)));
        assert_eq!(r.property_count(), 0);
    }

    #[test]
    fn resource_is_arc_shareable() {
        let mut r = Resource::new("Shared");
        r.set_property("v", Variant::Bool(true));
        let a = Arc::new(r);
        let b = Arc::clone(&a);
        assert!(Arc::ptr_eq(&a, &b));
    }
}
