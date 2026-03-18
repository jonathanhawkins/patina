//! Base Object type and object lifecycle management.
//!
//! This module provides the core object model for the Patina Engine,
//! mirroring Godot's `Object` class. Every engine object has a unique
//! instance ID, a class name, and a property bag that stores Variant values.

use std::collections::HashMap;

use gdcore::id::ObjectId;
use gdvariant::Variant;

use crate::notification::Notification;
use crate::signal::SignalStore;

/// Trait implemented by all engine objects.
///
/// This is the Rust equivalent of Godot's `Object` base class. It provides
/// uniform access to identity, class metadata, properties, and notifications
/// regardless of the concrete type.
pub trait GodotObject {
    /// Returns the class name as registered in ClassDB (e.g., `"Node2D"`).
    fn get_class(&self) -> &str;

    /// Returns the unique instance ID for this object.
    fn get_instance_id(&self) -> ObjectId;

    /// Sets a property by name. Returns the previous value (or `Nil`).
    fn set_property(&mut self, name: &str, value: Variant) -> Variant;

    /// Gets a property by name. Returns `Nil` if not found.
    fn get_property(&self, name: &str) -> Variant;

    /// Handles a notification. Implementations should dispatch to the
    /// appropriate lifecycle method based on the notification code.
    fn notification(&mut self, what: Notification);
}

/// The base data shared by every engine object instance.
///
/// `ObjectBase` stores the identity and property bag. Concrete object types
/// embed this struct and delegate `GodotObject` methods to it, adding their
/// own typed fields on top.
#[derive(Debug, Clone)]
pub struct ObjectBase {
    /// The unique instance ID, assigned at creation.
    id: ObjectId,
    /// The class name this object was created as.
    class_name: String,
    /// Dynamic property storage. In Godot, properties that aren't backed by
    /// a native field fall through to this map.
    properties: HashMap<String, Variant>,
    /// Per-object signal store.
    signals: SignalStore,
    /// Log of received notifications (useful for tests and debugging).
    notification_log: Vec<Notification>,
}

impl ObjectBase {
    /// Creates a new `ObjectBase` with the given class name and a fresh ID.
    pub fn new(class_name: impl Into<String>) -> Self {
        Self {
            id: ObjectId::next(),
            class_name: class_name.into(),
            properties: HashMap::new(),
            signals: SignalStore::new(),
            notification_log: Vec::new(),
        }
    }

    /// Creates an `ObjectBase` with a specific ID (for deserialization/tests).
    pub fn with_id(class_name: impl Into<String>, id: ObjectId) -> Self {
        Self {
            id,
            class_name: class_name.into(),
            properties: HashMap::new(),
            signals: SignalStore::new(),
            notification_log: Vec::new(),
        }
    }

    /// Returns the unique instance ID.
    pub fn id(&self) -> ObjectId {
        self.id
    }

    /// Returns the class name.
    pub fn class_name(&self) -> &str {
        &self.class_name
    }

    /// Sets a property, returning the previous value (or `Nil`).
    pub fn set_property(&mut self, name: &str, value: Variant) -> Variant {
        self.properties
            .insert(name.to_owned(), value)
            .unwrap_or(Variant::Nil)
    }

    /// Gets a property by name. Returns `Nil` if absent.
    pub fn get_property(&self, name: &str) -> Variant {
        self.properties.get(name).cloned().unwrap_or(Variant::Nil)
    }

    /// Returns `true` if the property exists.
    pub fn has_property(&self, name: &str) -> bool {
        self.properties.contains_key(name)
    }

    /// Returns the names of all stored properties.
    pub fn property_names(&self) -> Vec<&str> {
        self.properties.keys().map(String::as_str).collect()
    }

    /// Records a notification and returns a reference to the log.
    pub fn record_notification(&mut self, what: Notification) {
        self.notification_log.push(what);
    }

    /// Returns the notification history for this object.
    pub fn notification_log(&self) -> &[Notification] {
        &self.notification_log
    }

    /// Returns a mutable reference to the signal store.
    pub fn signals_mut(&mut self) -> &mut SignalStore {
        &mut self.signals
    }

    /// Returns a reference to the signal store.
    pub fn signals(&self) -> &SignalStore {
        &self.signals
    }
}

/// A generic object instance created from ClassDB.
///
/// When ClassDB creates an object from a class name, it produces a
/// `GenericObject` that satisfies `GodotObject` using only the dynamic
/// property bag. Concrete types (Node, Resource, etc.) will provide their
/// own `GodotObject` implementations with typed fields.
#[derive(Debug, Clone)]
pub struct GenericObject {
    /// The shared base data.
    pub base: ObjectBase,
}

impl GenericObject {
    /// Creates a new generic object with the given class name.
    pub fn new(class_name: impl Into<String>) -> Self {
        Self {
            base: ObjectBase::new(class_name),
        }
    }
}

impl GodotObject for GenericObject {
    fn get_class(&self) -> &str {
        self.base.class_name()
    }

    fn get_instance_id(&self) -> ObjectId {
        self.base.id()
    }

    fn set_property(&mut self, name: &str, value: Variant) -> Variant {
        self.base.set_property(name, value)
    }

    fn get_property(&self, name: &str) -> Variant {
        self.base.get_property(name)
    }

    fn notification(&mut self, what: Notification) {
        self.base.record_notification(what);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_creation_and_identity() {
        let obj = GenericObject::new("Sprite2D");
        assert_eq!(obj.get_class(), "Sprite2D");
        // ID should be non-zero.
        assert!(obj.get_instance_id().raw() > 0);
    }

    #[test]
    fn unique_ids() {
        let a = GenericObject::new("Node");
        let b = GenericObject::new("Node");
        assert_ne!(a.get_instance_id(), b.get_instance_id());
    }

    #[test]
    fn property_get_set() {
        let mut obj = GenericObject::new("Node2D");

        // Getting a non-existent property returns Nil.
        assert_eq!(obj.get_property("position"), Variant::Nil);

        // Set returns previous (Nil on first set).
        let prev = obj.set_property("position", Variant::Int(42));
        assert_eq!(prev, Variant::Nil);

        // Now it's stored.
        assert_eq!(obj.get_property("position"), Variant::Int(42));

        // Overwrite returns old value.
        let prev = obj.set_property("position", Variant::Int(99));
        assert_eq!(prev, Variant::Int(42));
        assert_eq!(obj.get_property("position"), Variant::Int(99));
    }

    #[test]
    fn property_names_listing() {
        let mut obj = GenericObject::new("Node");
        obj.set_property("alpha", Variant::Float(1.0));
        obj.set_property("beta", Variant::Bool(true));

        let mut names = obj.base.property_names();
        names.sort();
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    fn notification_recording() {
        use crate::notification::{NOTIFICATION_PROCESS, NOTIFICATION_READY};

        let mut obj = GenericObject::new("Node");
        obj.notification(NOTIFICATION_READY);
        obj.notification(NOTIFICATION_PROCESS);

        let log = obj.base.notification_log();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0], NOTIFICATION_READY);
        assert_eq!(log[1], NOTIFICATION_PROCESS);
    }

    #[test]
    fn get_nonexistent_property_returns_nil() {
        let obj = GenericObject::new("Node");
        assert_eq!(obj.get_property("nonexistent"), Variant::Nil);
        assert_eq!(obj.get_property(""), Variant::Nil);
        assert_eq!(obj.get_property("a/b/c"), Variant::Nil);
    }

    #[test]
    fn set_same_property_twice_overwrites() {
        let mut obj = GenericObject::new("Node");
        obj.set_property("x", Variant::Int(1));
        assert_eq!(obj.get_property("x"), Variant::Int(1));
        obj.set_property("x", Variant::Int(2));
        assert_eq!(obj.get_property("x"), Variant::Int(2));
    }

    #[test]
    fn has_property_returns_false_for_missing() {
        let obj = GenericObject::new("Node");
        assert!(!obj.base.has_property("missing"));
    }

    #[test]
    fn object_base_with_id() {
        let id = ObjectId::from_raw(999);
        let base = ObjectBase::with_id("TestClass", id);
        assert_eq!(base.id(), id);
        assert_eq!(base.class_name(), "TestClass");
    }

    #[test]
    fn signals_initially_empty() {
        let obj = GenericObject::new("Node");
        assert!(obj.base.signals().signal_names().is_empty());
    }

    #[test]
    fn empty_property_names_for_new_object() {
        let obj = GenericObject::new("Node");
        assert!(obj.base.property_names().is_empty());
    }

    #[test]
    fn notification_log_initially_empty() {
        let obj = GenericObject::new("Node");
        assert!(obj.base.notification_log().is_empty());
    }
}
