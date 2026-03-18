//! Class database and inheritance metadata registry.
//!
//! Godot's `ClassDB` is a global singleton that maps class names to
//! metadata: parent class, registered properties, methods, and a factory
//! function to create instances. This module provides a thread-safe
//! registry with the same semantics.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use gdcore::id::ClassId;
use gdvariant::Variant;

use crate::object::{GenericObject, GodotObject};

/// Metadata for a single registered property.
#[derive(Debug, Clone)]
pub struct PropertyInfo {
    /// The property name (e.g., `"position"`).
    pub name: String,
    /// The default value for this property.
    pub default_value: Variant,
}

impl PropertyInfo {
    /// Creates a new property info entry.
    pub fn new(name: impl Into<String>, default_value: Variant) -> Self {
        Self {
            name: name.into(),
            default_value,
        }
    }
}

/// Metadata for a single registered method.
#[derive(Debug, Clone)]
pub struct MethodInfo {
    /// The method name (e.g., `"_ready"`).
    pub name: String,
    /// Number of expected arguments (for validation).
    pub argument_count: usize,
}

impl MethodInfo {
    /// Creates a new method info entry.
    pub fn new(name: impl Into<String>, argument_count: usize) -> Self {
        Self {
            name: name.into(),
            argument_count,
        }
    }
}

/// Complete registration record for a class.
#[derive(Debug, Clone)]
pub struct ClassInfo {
    /// Unique numeric ID for this class.
    pub class_id: ClassId,
    /// The class name (e.g., `"Node2D"`).
    pub class_name: String,
    /// The parent class name, or empty string for `Object` (the root).
    pub parent_class: String,
    /// Registered properties.
    pub properties: Vec<PropertyInfo>,
    /// Registered methods.
    pub methods: Vec<MethodInfo>,
}

/// A builder for registering a class with the database.
pub struct ClassRegistration {
    class_name: String,
    parent_class: String,
    properties: Vec<PropertyInfo>,
    methods: Vec<MethodInfo>,
}

impl ClassRegistration {
    /// Starts building a registration for the given class.
    pub fn new(class_name: impl Into<String>) -> Self {
        Self {
            class_name: class_name.into(),
            parent_class: String::new(),
            properties: Vec::new(),
            methods: Vec::new(),
        }
    }

    /// Sets the parent class name.
    pub fn parent(mut self, parent: impl Into<String>) -> Self {
        self.parent_class = parent.into();
        self
    }

    /// Adds a property to the class registration.
    pub fn property(mut self, info: PropertyInfo) -> Self {
        self.properties.push(info);
        self
    }

    /// Adds a method to the class registration.
    pub fn method(mut self, info: MethodInfo) -> Self {
        self.methods.push(info);
        self
    }
}

/// The class database — a global registry of class metadata and factories.
///
/// Thread-safe via internal `Mutex`. Intended to be accessed through the
/// module-level functions [`register_class`], [`get_class_info`], etc.
#[derive(Debug)]
struct ClassDB {
    classes: HashMap<String, ClassInfo>,
    by_id: HashMap<ClassId, String>,
    next_id: u32,
}

impl ClassDB {
    fn new() -> Self {
        Self {
            classes: HashMap::new(),
            by_id: HashMap::new(),
            next_id: 1,
        }
    }

    fn register(&mut self, reg: ClassRegistration) -> ClassId {
        let id = ClassId::new(self.next_id);
        self.next_id += 1;

        let info = ClassInfo {
            class_id: id,
            class_name: reg.class_name.clone(),
            parent_class: reg.parent_class,
            properties: reg.properties,
            methods: reg.methods,
        };

        self.by_id.insert(id, reg.class_name.clone());
        self.classes.insert(reg.class_name, info);
        id
    }

    fn get_by_name(&self, name: &str) -> Option<&ClassInfo> {
        self.classes.get(name)
    }

    fn get_by_id(&self, id: ClassId) -> Option<&ClassInfo> {
        let name = self.by_id.get(&id)?;
        self.classes.get(name)
    }

    fn class_exists(&self, name: &str) -> bool {
        self.classes.contains_key(name)
    }

    fn class_count(&self) -> usize {
        self.classes.len()
    }

    fn inheritance_chain(&self, class_name: &str) -> Vec<String> {
        let mut chain = Vec::new();
        let mut current = class_name.to_owned();
        while let Some(info) = self.classes.get(&current) {
            chain.push(current.clone());
            if info.parent_class.is_empty() {
                break;
            }
            current = info.parent_class.clone();
        }
        chain
    }

    fn is_parent_class(&self, child: &str, parent: &str) -> bool {
        let chain = self.inheritance_chain(child);
        chain.iter().any(|c| c == parent)
    }
}

/// Returns a reference to the global ClassDB instance.
fn global_db() -> &'static Mutex<ClassDB> {
    static DB: OnceLock<Mutex<ClassDB>> = OnceLock::new();
    DB.get_or_init(|| Mutex::new(ClassDB::new()))
}

// ── Public API ──────────────────────────────────────────────────────

/// Registers a class in the global ClassDB.
///
/// Returns the assigned `ClassId`. Panics if the lock is poisoned.
pub fn register_class(reg: ClassRegistration) -> ClassId {
    global_db().lock().expect("ClassDB lock poisoned").register(reg)
}

/// Returns class info by name, if registered.
pub fn get_class_info(name: &str) -> Option<ClassInfo> {
    global_db()
        .lock()
        .expect("ClassDB lock poisoned")
        .get_by_name(name)
        .cloned()
}

/// Returns class info by `ClassId`, if registered.
pub fn get_class_info_by_id(id: ClassId) -> Option<ClassInfo> {
    global_db()
        .lock()
        .expect("ClassDB lock poisoned")
        .get_by_id(id)
        .cloned()
}

/// Returns `true` if a class with the given name is registered.
pub fn class_exists(name: &str) -> bool {
    global_db()
        .lock()
        .expect("ClassDB lock poisoned")
        .class_exists(name)
}

/// Returns the total number of registered classes.
pub fn class_count() -> usize {
    global_db()
        .lock()
        .expect("ClassDB lock poisoned")
        .class_count()
}

/// Returns the inheritance chain from child to root.
///
/// For example, if `Sprite2D -> Node2D -> Node -> Object`, calling
/// `inheritance_chain("Sprite2D")` returns
/// `["Sprite2D", "Node2D", "Node", "Object"]`.
pub fn inheritance_chain(class_name: &str) -> Vec<String> {
    global_db()
        .lock()
        .expect("ClassDB lock poisoned")
        .inheritance_chain(class_name)
}

/// Returns `true` if `child` inherits from `parent` (or is `parent` itself).
pub fn is_parent_class(child: &str, parent: &str) -> bool {
    global_db()
        .lock()
        .expect("ClassDB lock poisoned")
        .is_parent_class(child, parent)
}

/// Creates a new `GenericObject` instance from a registered class name.
///
/// The returned object has all registered default property values pre-set.
/// Returns `None` if the class is not registered.
pub fn instantiate(class_name: &str) -> Option<GenericObject> {
    let db = global_db().lock().expect("ClassDB lock poisoned");
    let info = db.get_by_name(class_name)?;

    let mut obj = GenericObject::new(&info.class_name);

    // Walk up the inheritance chain and apply default properties.
    let chain = db.inheritance_chain(class_name);
    // Apply from base to derived so derived defaults override base defaults.
    for ancestor in chain.iter().rev() {
        if let Some(ancestor_info) = db.get_by_name(ancestor) {
            for prop in &ancestor_info.properties {
                obj.set_property(&prop.name, prop.default_value.clone());
            }
        }
    }

    Some(obj)
}

/// Clears all registered classes. **For testing only.**
///
/// This is necessary because the ClassDB is global and tests run in the
/// same process. Each test that registers classes should call this first.
pub fn clear_for_testing() {
    let mut db = global_db().lock().expect("ClassDB lock poisoned");
    db.classes.clear();
    db.by_id.clear();
    db.next_id = 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // ClassDB is a global singleton, so tests that mutate it must be
    // serialized. This mutex ensures only one test touches the DB at a time.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn setup() -> std::sync::MutexGuard<'static, ()> {
        let guard = TEST_LOCK.lock().expect("test lock poisoned");
        clear_for_testing();
        guard
    }

    #[test]
    fn register_and_lookup() {
        let _g = setup();

        let id = register_class(
            ClassRegistration::new("Object"),
        );

        assert!(class_exists("Object"));
        assert!(!class_exists("Node"));

        let info = get_class_info("Object").unwrap();
        assert_eq!(info.class_name, "Object");
        assert_eq!(info.class_id, id);
        assert!(info.parent_class.is_empty());
    }

    #[test]
    fn lookup_by_id() {
        let _g = setup();

        let id = register_class(ClassRegistration::new("TestClass"));
        let info = get_class_info_by_id(id).unwrap();
        assert_eq!(info.class_name, "TestClass");
    }

    #[test]
    fn inheritance_chain_works() {
        let _g = setup();

        register_class(ClassRegistration::new("Object"));
        register_class(ClassRegistration::new("Node").parent("Object"));
        register_class(ClassRegistration::new("Node2D").parent("Node"));
        register_class(ClassRegistration::new("Sprite2D").parent("Node2D"));

        let chain = inheritance_chain("Sprite2D");
        assert_eq!(chain, vec!["Sprite2D", "Node2D", "Node", "Object"]);

        assert!(is_parent_class("Sprite2D", "Object"));
        assert!(is_parent_class("Sprite2D", "Sprite2D"));
        assert!(!is_parent_class("Node", "Sprite2D"));
    }

    #[test]
    fn instantiate_with_defaults() {
        let _g = setup();

        register_class(
            ClassRegistration::new("Object"),
        );
        register_class(
            ClassRegistration::new("Node")
                .parent("Object")
                .property(PropertyInfo::new("name", Variant::String(String::new())))
                .method(MethodInfo::new("_ready", 0)),
        );
        register_class(
            ClassRegistration::new("Node2D")
                .parent("Node")
                .property(PropertyInfo::new(
                    "position",
                    Variant::Vector2(gdcore::math::Vector2::ZERO),
                )),
        );

        let obj = instantiate("Node2D").expect("should create Node2D");
        assert_eq!(obj.get_class(), "Node2D");
        assert_eq!(
            obj.get_property("name"),
            Variant::String(String::new()),
        );
        assert_eq!(
            obj.get_property("position"),
            Variant::Vector2(gdcore::math::Vector2::ZERO),
        );
    }

    #[test]
    fn instantiate_nonexistent_returns_none() {
        let _g = setup();
        assert!(instantiate("DoesNotExist").is_none());
    }

    #[test]
    fn class_count_tracks() {
        let _g = setup();
        assert_eq!(class_count(), 0);
        register_class(ClassRegistration::new("A"));
        register_class(ClassRegistration::new("B"));
        assert_eq!(class_count(), 2);
    }
}
