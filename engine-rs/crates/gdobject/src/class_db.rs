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
/// `PROPERTY_USAGE_NONE` — the property is neither stored nor shown.
pub const PROPERTY_USAGE_NONE: i32 = 0;
/// `PROPERTY_USAGE_STORAGE` — serialized into scenes/resources.
pub const PROPERTY_USAGE_STORAGE: i32 = 2;
/// `PROPERTY_USAGE_EDITOR` — shown in the inspector.
pub const PROPERTY_USAGE_EDITOR: i32 = 4;
/// `PROPERTY_USAGE_DEFAULT` — Godot's default for an exposed property:
/// `STORAGE | EDITOR` (`core/object/object.h`).
pub const PROPERTY_USAGE_DEFAULT: i32 = PROPERTY_USAGE_STORAGE | PROPERTY_USAGE_EDITOR;

#[derive(Debug, Clone)]
pub struct PropertyInfo {
    /// The property name (e.g., `"position"`).
    pub name: String,
    /// The default value for this property.
    ///
    /// Doubles as the property's TYPE: Godot's `get_property_list()` dict
    /// reports `type` as a Variant type id, and the default already carries it,
    /// so the two can never disagree (pat-0q2kx.64).
    pub default_value: Variant,
    /// Optional property hint value (matches Godot's PropertyHint enum).
    /// 0 means no hint (default).
    pub hint: i32,
    /// Hint detail — an enum's comma-separated names, a resource type, a numeric
    /// range (Godot `PropertyInfo::hint_string`). Empty when unhinted.
    pub hint_string: String,
    /// Storage/editor visibility flags (Godot `PropertyInfo::usage`). Defaults
    /// to [`PROPERTY_USAGE_DEFAULT`]; a save system reads this to decide what to
    /// persist.
    pub usage: i32,
}

impl PropertyInfo {
    /// Creates a new property info entry with no hint and Godot's default usage.
    pub fn new(name: impl Into<String>, default_value: Variant) -> Self {
        Self {
            name: name.into(),
            default_value,
            hint: 0,
            hint_string: String::new(),
            usage: PROPERTY_USAGE_DEFAULT,
        }
    }

    /// Sets the property hint value (builder pattern).
    pub fn with_hint(mut self, hint: i32) -> Self {
        self.hint = hint;
        self
    }

    /// Sets the hint detail string (builder pattern).
    pub fn with_hint_string(mut self, hint_string: impl Into<String>) -> Self {
        self.hint_string = hint_string.into();
        self
    }

    /// Sets the usage flags (builder pattern).
    pub fn with_usage(mut self, usage: i32) -> Self {
        self.usage = usage;
        self
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

/// Metadata for a single registered signal (pat-nzgdu.98).
///
/// Mirrors an entry of Godot's `ClassDB.class_get_signal_list`: the signal name
/// plus the names of the arguments it carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalInfo {
    /// The signal name (e.g., `"tree_entered"`).
    pub name: String,
    /// Argument names, in order (empty for a no-argument signal).
    pub arguments: Vec<String>,
}

impl SignalInfo {
    /// Creates a signal that carries no arguments.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            arguments: Vec::new(),
        }
    }

    /// Adds an argument name (builder pattern).
    pub fn arg(mut self, name: impl Into<String>) -> Self {
        self.arguments.push(name.into());
        self
    }
}

/// Metadata for a named enum on a class (pat-nzgdu.98).
///
/// Mirrors Godot's `ClassDB.class_get_enum_list` / `class_get_enum_constants`:
/// enum constants are *also* integer constants of the class, which is why
/// [`ClassRegistration::enumeration`] registers them in both places.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumInfo {
    /// The enum name (e.g., `"ProcessMode"`).
    pub name: String,
    /// The enum's constants as `(name, value)` pairs, in declaration order.
    pub constants: Vec<(String, i64)>,
}

impl EnumInfo {
    /// Creates an empty enum.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            constants: Vec::new(),
        }
    }

    /// Adds a constant (builder pattern).
    pub fn constant(mut self, name: impl Into<String>, value: i64) -> Self {
        self.constants.push((name.into(), value));
        self
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
    /// Registered signals (pat-nzgdu.98).
    pub signals: Vec<SignalInfo>,
    /// Registered integer constants as `(name, value)` pairs, including every
    /// enum constant (pat-nzgdu.98).
    pub integer_constants: Vec<(String, i64)>,
    /// Registered named enums (pat-nzgdu.98).
    pub enums: Vec<EnumInfo>,
    /// Whether the class is abstract (cannot be instantiated in Godot).
    ///
    /// Mirrors `!ClassDB::can_instantiate()` in Godot: abstract bases like
    /// `CanvasItem` or `PhysicsBody2D` appear in the Create Node dialog only
    /// as greyed-out ancestors and cannot be created directly.
    pub is_abstract: bool,
}

/// A builder for registering a class with the database.
pub struct ClassRegistration {
    class_name: String,
    parent_class: String,
    properties: Vec<PropertyInfo>,
    methods: Vec<MethodInfo>,
    signals: Vec<SignalInfo>,
    integer_constants: Vec<(String, i64)>,
    enums: Vec<EnumInfo>,
    is_abstract: bool,
}

impl ClassRegistration {
    /// Starts building a registration for the given class.
    pub fn new(class_name: impl Into<String>) -> Self {
        Self {
            class_name: class_name.into(),
            parent_class: String::new(),
            properties: Vec::new(),
            methods: Vec::new(),
            signals: Vec::new(),
            integer_constants: Vec::new(),
            enums: Vec::new(),
            is_abstract: false,
        }
    }

    /// Sets the parent class name.
    pub fn parent(mut self, parent: impl Into<String>) -> Self {
        self.parent_class = parent.into();
        self
    }

    /// Marks the class as abstract — it cannot be instantiated directly.
    pub fn abstract_class(mut self) -> Self {
        self.is_abstract = true;
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

    /// Adds a signal to the class registration (pat-nzgdu.98).
    pub fn signal(mut self, info: SignalInfo) -> Self {
        self.signals.push(info);
        self
    }

    /// Adds a standalone integer constant (pat-nzgdu.98).
    pub fn constant(mut self, name: impl Into<String>, value: i64) -> Self {
        self.integer_constants.push((name.into(), value));
        self
    }

    /// Adds a named enum, and registers each of its constants as an integer
    /// constant of the class too — matching Godot, where enum constants are
    /// reachable via both `class_get_integer_constant` and
    /// `class_get_enum_constants` (pat-nzgdu.98).
    pub fn enumeration(mut self, info: EnumInfo) -> Self {
        self.integer_constants
            .extend(info.constants.iter().cloned());
        self.enums.push(info);
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
            signals: reg.signals,
            integer_constants: reg.integer_constants,
            enums: reg.enums,
            is_abstract: reg.is_abstract,
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

    /// Checks whether `method_name` is registered on `class_name` or any
    /// of its ancestors in the inheritance chain.
    fn has_method(&self, class_name: &str, method_name: &str) -> bool {
        let chain = self.inheritance_chain(class_name);
        for ancestor in &chain {
            if let Some(info) = self.classes.get(ancestor) {
                if info.methods.iter().any(|m| m.name == method_name) {
                    return true;
                }
            }
        }
        false
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
    global_db()
        .lock()
        .expect("ClassDB lock poisoned")
        .register(reg)
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

/// Returns the names of all registered classes (sorted alphabetically).
pub fn get_class_list() -> Vec<String> {
    let db = global_db().lock().expect("ClassDB lock poisoned");
    let mut names: Vec<String> = db.classes.keys().cloned().collect();
    names.sort();
    names
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

/// Returns `true` if `method_name` is registered on `class_name` or any
/// of its ancestors in the ClassDB inheritance chain.
pub fn class_has_method(class_name: &str, method_name: &str) -> bool {
    global_db()
        .lock()
        .expect("ClassDB lock poisoned")
        .has_method(class_name, method_name)
}

/// Returns all properties for a class, including inherited properties.
///
/// Properties are returned base-first (Object properties first, then each
/// derived class's properties in order). If the class is not registered,
/// returns an empty list.
pub fn get_property_list(class_name: &str) -> Vec<PropertyInfo> {
    let db = global_db().lock().expect("ClassDB lock poisoned");
    let chain = db.inheritance_chain(class_name);
    let mut props = Vec::new();
    // Walk from base to derived so base properties come first
    for ancestor in chain.iter().rev() {
        if let Some(info) = db.get_by_name(ancestor) {
            props.extend(info.properties.iter().cloned());
        }
    }
    props
}

/// Whether `property` on `class_name` has a class default to revert to
/// (Godot `Object::property_can_revert`).
///
/// True exactly when the ClassDB knows the property — a script-only variable
/// with no registered default has nothing to revert TO, which is what Godot
/// reports too.
pub fn property_can_revert(class_name: &str, property: &str) -> bool {
    get_property_list(class_name)
        .iter()
        .any(|p| p.name == property)
}

/// The class default for `property` (Godot `Object::property_get_revert`).
///
/// The LAST match in the merged list wins, so a derived class that re-declares a
/// base property reverts to the DERIVED default — the same override order
/// `get_property_list` builds (base first, derived last).
pub fn property_get_revert(class_name: &str, property: &str) -> Option<Variant> {
    get_property_list(class_name)
        .into_iter()
        .filter(|p| p.name == property)
        .next_back()
        .map(|p| p.default_value)
}

/// Returns all methods for a class, including inherited methods.
///
/// Methods are returned base-first (Object methods first, then each
/// derived class's methods in order). If the class is not registered,
/// returns an empty list.
pub fn get_method_list(class_name: &str) -> Vec<MethodInfo> {
    let db = global_db().lock().expect("ClassDB lock poisoned");
    let chain = db.inheritance_chain(class_name);
    let mut methods = Vec::new();
    // Walk from base to derived so base methods come first
    for ancestor in chain.iter().rev() {
        if let Some(info) = db.get_by_name(ancestor) {
            methods.extend(info.methods.iter().cloned());
        }
    }
    methods
}

/// Returns all signals for a class, including inherited ones (pat-nzgdu.98).
///
/// Base-first, mirroring [`get_property_list`]. Empty if the class is not
/// registered. Godot: `ClassDB.class_get_signal_list`.
pub fn get_signal_list(class_name: &str) -> Vec<SignalInfo> {
    let db = global_db().lock().expect("ClassDB lock poisoned");
    let chain = db.inheritance_chain(class_name);
    let mut signals = Vec::new();
    for ancestor in chain.iter().rev() {
        if let Some(info) = db.get_by_name(ancestor) {
            signals.extend(info.signals.iter().cloned());
        }
    }
    signals
}

/// Returns `true` if `signal_name` is registered on `class_name` or any of its
/// ancestors (pat-nzgdu.98). Godot: `ClassDB.class_has_signal`.
pub fn class_has_signal(class_name: &str, signal_name: &str) -> bool {
    get_signal_list(class_name)
        .iter()
        .any(|s| s.name == signal_name)
}

/// Returns all integer constants for a class, including inherited ones
/// (pat-nzgdu.98). Godot: `ClassDB.class_get_integer_constant_list`.
pub fn get_integer_constant_list(class_name: &str) -> Vec<String> {
    let db = global_db().lock().expect("ClassDB lock poisoned");
    let chain = db.inheritance_chain(class_name);
    let mut names = Vec::new();
    for ancestor in chain.iter().rev() {
        if let Some(info) = db.get_by_name(ancestor) {
            names.extend(info.integer_constants.iter().map(|(n, _)| n.clone()));
        }
    }
    names
}

/// Looks up an integer constant on a class or its ancestors (pat-nzgdu.98).
///
/// The most-derived definition wins, matching Godot's shadowing. Returns `None`
/// when the constant is unknown — Godot returns 0 there, but `None` lets the
/// caller tell "absent" from "genuinely zero".
pub fn class_get_integer_constant(class_name: &str, constant_name: &str) -> Option<i64> {
    let db = global_db().lock().expect("ClassDB lock poisoned");
    // Derived-first: the chain runs derived -> base, so the first hit wins.
    for ancestor in db.inheritance_chain(class_name) {
        if let Some(info) = db.get_by_name(&ancestor) {
            if let Some((_, value)) = info
                .integer_constants
                .iter()
                .find(|(n, _)| n == constant_name)
            {
                return Some(*value);
            }
        }
    }
    None
}

/// Returns the names of every enum on a class, including inherited ones
/// (pat-nzgdu.98). Godot: `ClassDB.class_get_enum_list`.
pub fn get_enum_list(class_name: &str) -> Vec<String> {
    let db = global_db().lock().expect("ClassDB lock poisoned");
    let chain = db.inheritance_chain(class_name);
    let mut names = Vec::new();
    for ancestor in chain.iter().rev() {
        if let Some(info) = db.get_by_name(ancestor) {
            names.extend(info.enums.iter().map(|e| e.name.clone()));
        }
    }
    names
}

/// Returns `true` if `enum_name` is registered on `class_name` or an ancestor
/// (pat-nzgdu.98). Godot: `ClassDB.class_has_enum`.
pub fn class_has_enum(class_name: &str, enum_name: &str) -> bool {
    get_enum_list(class_name).iter().any(|n| n == enum_name)
}

/// Returns the constant names of one enum on a class or its ancestors
/// (pat-nzgdu.98). Empty if the enum is unknown.
/// Godot: `ClassDB.class_get_enum_constants`.
pub fn get_enum_constants(class_name: &str, enum_name: &str) -> Vec<String> {
    let db = global_db().lock().expect("ClassDB lock poisoned");
    for ancestor in db.inheritance_chain(class_name) {
        if let Some(info) = db.get_by_name(&ancestor) {
            if let Some(e) = info.enums.iter().find(|e| e.name == enum_name) {
                return e.constants.iter().map(|(n, _)| n.clone()).collect();
            }
        }
    }
    Vec::new()
}

/// Returns `true` if the class is registered and marked abstract.
pub fn is_abstract(class_name: &str) -> bool {
    global_db()
        .lock()
        .expect("ClassDB lock poisoned")
        .get_by_name(class_name)
        .map(|info| info.is_abstract)
        .unwrap_or(false)
}

/// Returns `true` if the class is registered and not abstract — i.e. the
/// Create Node dialog may instantiate it. Mirrors Godot's
/// `ClassDB::can_instantiate`.
pub fn can_instantiate(class_name: &str) -> bool {
    global_db()
        .lock()
        .expect("ClassDB lock poisoned")
        .get_by_name(class_name)
        .map(|info| !info.is_abstract)
        .unwrap_or(false)
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

/// Returns `true` if the given class (or any ancestor) has a property
/// with the given name registered in the ClassDB.
pub fn class_has_property(class_name: &str, property_name: &str) -> bool {
    let props = get_property_list(class_name);
    props.iter().any(|p| p.name == property_name)
}

/// Registers the standard Godot 3D class hierarchy in the ClassDB.
///
/// This includes Node3D, Camera3D, MeshInstance3D, the Light3D family,
/// physics bodies, and related 3D node types with their default properties.
/// The full `Object` root registration (pat-nzgdu.98).
///
/// Several `register_*_classes` entry points bootstrap `Object`/`Node`, and
/// each is `class_exists`-guarded — so whichever runs first decides the
/// metadata. Both share these constructors so the result cannot depend on
/// registration order.
///
/// Notification constants are sourced from [`crate::notification`] rather than
/// hardcoded, so ClassDB can never disagree with the codes the engine actually
/// dispatches.
fn object_class() -> ClassRegistration {
    use crate::notification::{NOTIFICATION_POSTINITIALIZE, NOTIFICATION_PREDELETE};

    ClassRegistration::new("Object")
        .method(MethodInfo::new("get_class", 0))
        .method(MethodInfo::new("is_class", 1))
        .method(MethodInfo::new("get_instance_id", 0))
        .method(MethodInfo::new("has_method", 1))
        .method(MethodInfo::new("has_signal", 1))
        .method(MethodInfo::new("get_method_list", 0))
        .method(MethodInfo::new("get_signal_list", 0))
        .method(MethodInfo::new("get_property_list", 0))
        .method(MethodInfo::new("connect", 2))
        .method(MethodInfo::new("disconnect", 2))
        .method(MethodInfo::new("emit_signal", 1))
        .method(MethodInfo::new("call", 1))
        .method(MethodInfo::new("set", 2))
        .method(MethodInfo::new("get", 1))
        .method(MethodInfo::new("notification", 1))
        .signal(SignalInfo::new("script_changed"))
        .signal(SignalInfo::new("property_list_changed"))
        .constant(
            "NOTIFICATION_POSTINITIALIZE",
            NOTIFICATION_POSTINITIALIZE.code() as i64,
        )
        .constant(
            "NOTIFICATION_PREDELETE",
            NOTIFICATION_PREDELETE.code() as i64,
        )
}

/// The full `Node` registration — see [`object_class`] (pat-nzgdu.98).
fn node_class() -> ClassRegistration {
    use crate::notification::{
        NOTIFICATION_CHILD_ORDER_CHANGED, NOTIFICATION_ENTER_TREE, NOTIFICATION_EXIT_TREE,
        NOTIFICATION_PARENTED, NOTIFICATION_PAUSED, NOTIFICATION_PHYSICS_PROCESS,
        NOTIFICATION_PROCESS, NOTIFICATION_READY, NOTIFICATION_UNPARENTED, NOTIFICATION_UNPAUSED,
    };

    ClassRegistration::new("Node")
        .parent("Object")
        .method(MethodInfo::new("get_parent", 0))
        .method(MethodInfo::new("get_node", 1))
        .method(MethodInfo::new("get_node_or_null", 1))
        .method(MethodInfo::new("find_child", 1))
        .method(MethodInfo::new("add_child", 1))
        .method(MethodInfo::new("remove_child", 1))
        .method(MethodInfo::new("get_child", 1))
        .method(MethodInfo::new("get_child_count", 0))
        .method(MethodInfo::new("get_children", 0))
        .method(MethodInfo::new("get_name", 0))
        .method(MethodInfo::new("set_name", 1))
        .method(MethodInfo::new("get_path", 0))
        .method(MethodInfo::new("get_tree", 0))
        .method(MethodInfo::new("is_inside_tree", 0))
        .method(MethodInfo::new("queue_free", 0))
        .method(MethodInfo::new("set_process", 1))
        .method(MethodInfo::new("is_processing", 0))
        .method(MethodInfo::new("set_physics_process", 1))
        .method(MethodInfo::new("is_physics_processing", 0))
        .method(MethodInfo::new("add_to_group", 1))
        .method(MethodInfo::new("remove_from_group", 1))
        .method(MethodInfo::new("is_in_group", 1))
        .method(MethodInfo::new("duplicate", 0))
        .signal(SignalInfo::new("ready"))
        .signal(SignalInfo::new("renamed"))
        .signal(SignalInfo::new("tree_entered"))
        .signal(SignalInfo::new("tree_exiting"))
        .signal(SignalInfo::new("tree_exited"))
        .signal(SignalInfo::new("child_entered_tree").arg("node"))
        .signal(SignalInfo::new("child_exiting_tree").arg("node"))
        .signal(SignalInfo::new("child_order_changed"))
        .signal(SignalInfo::new("replacing_by").arg("node"))
        .constant(
            "NOTIFICATION_ENTER_TREE",
            NOTIFICATION_ENTER_TREE.code() as i64,
        )
        .constant(
            "NOTIFICATION_EXIT_TREE",
            NOTIFICATION_EXIT_TREE.code() as i64,
        )
        .constant("NOTIFICATION_READY", NOTIFICATION_READY.code() as i64)
        .constant("NOTIFICATION_PAUSED", NOTIFICATION_PAUSED.code() as i64)
        .constant("NOTIFICATION_UNPAUSED", NOTIFICATION_UNPAUSED.code() as i64)
        .constant("NOTIFICATION_PROCESS", NOTIFICATION_PROCESS.code() as i64)
        .constant(
            "NOTIFICATION_PHYSICS_PROCESS",
            NOTIFICATION_PHYSICS_PROCESS.code() as i64,
        )
        .constant("NOTIFICATION_PARENTED", NOTIFICATION_PARENTED.code() as i64)
        .constant(
            "NOTIFICATION_UNPARENTED",
            NOTIFICATION_UNPARENTED.code() as i64,
        )
        .constant(
            "NOTIFICATION_CHILD_ORDER_CHANGED",
            NOTIFICATION_CHILD_ORDER_CHANGED.code() as i64,
        )
        .enumeration(
            EnumInfo::new("ProcessMode")
                .constant("PROCESS_MODE_INHERIT", 0)
                .constant("PROCESS_MODE_PAUSABLE", 1)
                .constant("PROCESS_MODE_WHEN_PAUSED", 2)
                .constant("PROCESS_MODE_ALWAYS", 3)
                .constant("PROCESS_MODE_DISABLED", 4),
        )
        .enumeration(
            EnumInfo::new("InternalMode")
                .constant("INTERNAL_MODE_DISABLED", 0)
                .constant("INTERNAL_MODE_FRONT", 1)
                .constant("INTERNAL_MODE_BACK", 2),
        )
        .enumeration(
            EnumInfo::new("DuplicateFlags")
                .constant("DUPLICATE_SIGNALS", 1)
                .constant("DUPLICATE_GROUPS", 2)
                .constant("DUPLICATE_SCRIPTS", 4)
                .constant("DUPLICATE_USE_INSTANTIATION", 8),
        )
}

/// Registers the `Joint3D` node family (pat-0q2kx.118).
///
/// `gdphysics3d::joint` carried the parameter structs and the solver helpers for
/// all five kinds, but none of the classes was in ClassDB and no scene path ever
/// built one — an authored `PinJoint3D` instanced as an unknown class and
/// constrained nothing.
///
/// Parents and defaults come from `upstream/godot/doc/classes/<Name>.xml`. Godot
/// spells the subclass parameters with slashes (`angular_limit/upper`,
/// `params/bias`); those names are kept verbatim because that is exactly what a
/// `.tscn` writes, and `PhysicsServer3D::register_joints` reads them back by the
/// same key. Note `exclude_nodes_from_collision` defaults to **true**: jointed
/// bodies ignore each other unless the author opts back in.
fn register_joint_3d_classes() {
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

    register_class(
        ClassRegistration::new("Joint3D")
            .parent("Node3D")
            .abstract_class()
            .property(PropertyInfo::new(
                "node_a",
                Variant::NodePath(gdcore::node_path::NodePath::empty()),
            ))
            .property(PropertyInfo::new(
                "node_b",
                Variant::NodePath(gdcore::node_path::NodePath::empty()),
            ))
            .property(PropertyInfo::new(
                "exclude_nodes_from_collision",
                Variant::Bool(true),
            ))
            .property(PropertyInfo::new("solver_priority", Variant::Int(1))),
    );

    register_class(
        ClassRegistration::new("PinJoint3D")
            .parent("Joint3D")
            .property(PropertyInfo::new("params/bias", Variant::Float(0.3)))
            .property(PropertyInfo::new("params/damping", Variant::Float(1.0)))
            .property(PropertyInfo::new(
                "params/impulse_clamp",
                Variant::Float(0.0),
            )),
    );

    register_class(
        ClassRegistration::new("HingeJoint3D")
            .parent("Joint3D")
            .property(PropertyInfo::new("params/bias", Variant::Float(0.3)))
            .property(PropertyInfo::new(
                "angular_limit/enable",
                Variant::Bool(false),
            ))
            .property(PropertyInfo::new(
                "angular_limit/upper",
                Variant::Float(FRAC_PI_2 as f64),
            ))
            .property(PropertyInfo::new(
                "angular_limit/lower",
                Variant::Float(-FRAC_PI_2 as f64),
            ))
            .property(PropertyInfo::new("angular_limit/bias", Variant::Float(0.3)))
            .property(PropertyInfo::new(
                "angular_limit/softness",
                Variant::Float(0.9),
            ))
            .property(PropertyInfo::new(
                "angular_limit/relaxation",
                Variant::Float(1.0),
            ))
            .property(PropertyInfo::new("motor/enable", Variant::Bool(false)))
            .property(PropertyInfo::new(
                "motor/target_velocity",
                Variant::Float(1.0),
            ))
            .property(PropertyInfo::new("motor/max_impulse", Variant::Float(1.0))),
    );

    register_class(
        ClassRegistration::new("SliderJoint3D")
            .parent("Joint3D")
            .property(PropertyInfo::new(
                "linear_limit/upper_distance",
                Variant::Float(1.0),
            ))
            .property(PropertyInfo::new(
                "linear_limit/lower_distance",
                Variant::Float(-1.0),
            ))
            .property(PropertyInfo::new(
                "linear_limit/softness",
                Variant::Float(1.0),
            ))
            .property(PropertyInfo::new(
                "linear_limit/restitution",
                Variant::Float(0.7),
            ))
            .property(PropertyInfo::new(
                "linear_limit/damping",
                Variant::Float(1.0),
            ))
            .property(PropertyInfo::new(
                "angular_limit/upper_angle",
                Variant::Float(0.0),
            ))
            .property(PropertyInfo::new(
                "angular_limit/lower_angle",
                Variant::Float(0.0),
            ))
            .property(PropertyInfo::new(
                "angular_limit/softness",
                Variant::Float(1.0),
            ))
            .property(PropertyInfo::new(
                "angular_limit/restitution",
                Variant::Float(0.7),
            ))
            .property(PropertyInfo::new(
                "angular_limit/damping",
                Variant::Float(0.0),
            )),
    );

    register_class(
        ClassRegistration::new("ConeTwistJoint3D")
            .parent("Joint3D")
            .property(PropertyInfo::new(
                "swing_span",
                Variant::Float(FRAC_PI_4 as f64),
            ))
            .property(PropertyInfo::new("twist_span", Variant::Float(PI as f64)))
            .property(PropertyInfo::new("bias", Variant::Float(0.3)))
            .property(PropertyInfo::new("softness", Variant::Float(0.8)))
            .property(PropertyInfo::new("relaxation", Variant::Float(1.0))),
    );

    // Generic6DOFJoint3D carries the same six knobs per axis; only the limit
    // enable/bounds are modelled here because those are what the solver reads.
    // Godot enables every axis limit by default with a zero-width range, which
    // locks the joint until the author widens it.
    let mut g6dof = ClassRegistration::new("Generic6DOFJoint3D").parent("Joint3D");
    for axis in ["x", "y", "z"] {
        g6dof = g6dof
            .property(PropertyInfo::new(
                format!("linear_limit_{axis}/enabled"),
                Variant::Bool(true),
            ))
            .property(PropertyInfo::new(
                format!("linear_limit_{axis}/upper_distance"),
                Variant::Float(0.0),
            ))
            .property(PropertyInfo::new(
                format!("linear_limit_{axis}/lower_distance"),
                Variant::Float(0.0),
            ))
            .property(PropertyInfo::new(
                format!("angular_limit_{axis}/enabled"),
                Variant::Bool(true),
            ))
            .property(PropertyInfo::new(
                format!("angular_limit_{axis}/upper_angle"),
                Variant::Float(0.0),
            ))
            .property(PropertyInfo::new(
                format!("angular_limit_{axis}/lower_angle"),
                Variant::Float(0.0),
            ));
    }
    register_class(g6dof);
}

pub fn register_3d_classes() {
    use gdcore::math::Vector3;
    use gdcore::math3d::{Basis, Transform3D};

    // Bootstrap the roots like every other register_*_classes does. Without
    // them this pass registers `Node3D` under a `Node` that does not exist, so
    // `inheritance_chain` stops dead at Node3D: run on its own,
    // `is_parent_class("Node3D", "Node")` and `is_parent_class("XRServer",
    // "Object")` were both false (pat-nzgdu.98).
    if !class_exists("Object") {
        register_class(object_class());
    }
    if !class_exists("Node") {
        register_class(node_class());
    }

    let identity_transform = Transform3D {
        basis: Basis::IDENTITY,
        origin: Vector3::ZERO,
    };

    // -- Node3D (base for all 3D nodes) --
    register_class(
        ClassRegistration::new("Node3D")
            .parent("Node")
            .property(PropertyInfo::new(
                "transform",
                Variant::Transform3D(identity_transform),
            ))
            .property(PropertyInfo::new("visible", Variant::Bool(true)))
            .property(PropertyInfo::new(
                "position",
                Variant::Vector3(Vector3::ZERO),
            ))
            .property(PropertyInfo::new(
                "rotation",
                Variant::Vector3(Vector3::ZERO),
            ))
            .property(PropertyInfo::new("scale", Variant::Vector3(Vector3::ONE))),
    );

    // -- Camera3D --
    register_class(
        ClassRegistration::new("Camera3D")
            .parent("Node3D")
            .property(PropertyInfo::new("fov", Variant::Float(75.0)))
            .property(PropertyInfo::new("near", Variant::Float(0.05)))
            .property(PropertyInfo::new("far", Variant::Float(4000.0)))
            .property(PropertyInfo::new("current", Variant::Bool(false))),
    );

    // -- AudioListener3D --
    //
    // Overrides where 3D audio is heard from, taking precedence over the current
    // camera. The 2D counterpart was registered but this one was missing
    // entirely, so a scene could not even name it (pat-0q2kx.57). Like Camera3D
    // it is driven by `current`, which `make_current()`/`clear_current()` set and
    // the audio bridge reads when resolving the listener.
    register_class(
        ClassRegistration::new("AudioListener3D")
            .parent("Node3D")
            .property(PropertyInfo::new("current", Variant::Bool(false)))
            .method(MethodInfo::new("make_current", 0))
            .method(MethodInfo::new("clear_current", 0))
            .method(MethodInfo::new("is_current", 0)),
    );

    // -- MeshInstance3D (inherits GeometryInstance3D → VisualInstance3D → Node3D;
    //    cast_shadow and the rest of the geometry surface come from the base). --
    register_class(ClassRegistration::new("MeshInstance3D").parent("GeometryInstance3D"));

    // -- MultiMeshInstance3D --
    register_class(ClassRegistration::new("MultiMeshInstance3D").parent("GeometryInstance3D"));

    // -- Sprite3D family (pat-0q2kx.111) --
    //
    // `gdserver3d::sprite3d` builds the textured quad and `render_server_3d.rs`
    // syncs it every frame, but none of these three classes had a ClassDB entry.
    // Since `Node::is_class` resolves through ClassDB, an authored `Sprite3D`
    // could not be recognised as a `GeometryInstance3D` — it fell out of class
    // walks and never appeared in the Create-Node dialog.
    //
    // Parents and defaults are each class's `doc/classes/<Name>.xml`.
    // `SpriteBase3D` is abstract in Godot: it carries the whole shared surface
    // but is never instantiated directly.
    register_class(
        ClassRegistration::new("SpriteBase3D")
            .parent("GeometryInstance3D")
            .abstract_class()
            .property(PropertyInfo::new("centered", Variant::Bool(true)))
            .property(PropertyInfo::new(
                "offset",
                Variant::Vector2(gdcore::math::Vector2::ZERO),
            ))
            .property(PropertyInfo::new("flip_h", Variant::Bool(false)))
            .property(PropertyInfo::new("flip_v", Variant::Bool(false)))
            // Fully qualified: `register_3d_classes` imports only Vector3/Basis/
            // Transform3D, so `Color` is not in scope here the way it is in
            // `register_2d_classes`.
            .property(PropertyInfo::new(
                "modulate",
                Variant::Color(gdcore::math::Color::WHITE),
            ))
            .property(PropertyInfo::new("pixel_size", Variant::Float(0.01)))
            // Godot's default axis is Vector3.AXIS_Z = 2.
            .property(PropertyInfo::new("axis", Variant::Int(2)))
            .property(PropertyInfo::new("billboard", Variant::Int(0)))
            .property(PropertyInfo::new("transparent", Variant::Bool(true)))
            .property(PropertyInfo::new("shaded", Variant::Bool(false)))
            .property(PropertyInfo::new("double_sided", Variant::Bool(true)))
            .property(PropertyInfo::new("no_depth_test", Variant::Bool(false)))
            .property(PropertyInfo::new("fixed_size", Variant::Bool(false)))
            .property(PropertyInfo::new("alpha_cut", Variant::Int(0)))
            .property(PropertyInfo::new(
                "alpha_scissor_threshold",
                Variant::Float(0.5),
            ))
            .property(PropertyInfo::new("alpha_hash_scale", Variant::Float(1.0)))
            .property(PropertyInfo::new(
                "alpha_antialiasing_mode",
                Variant::Int(0),
            ))
            .property(PropertyInfo::new(
                "alpha_antialiasing_edge",
                Variant::Float(0.0),
            ))
            // Godot's default is TEXTURE_FILTER_LINEAR_WITH_MIPMAPS = 3.
            .property(PropertyInfo::new("texture_filter", Variant::Int(3)))
            .property(PropertyInfo::new("render_priority", Variant::Int(0)))
            .enumeration(
                EnumInfo::new("AlphaCutMode")
                    .constant("ALPHA_CUT_DISABLED", 0)
                    .constant("ALPHA_CUT_DISCARD", 1)
                    .constant("ALPHA_CUT_OPAQUE_PREPASS", 2)
                    .constant("ALPHA_CUT_HASH", 3),
            )
            .enumeration(
                EnumInfo::new("DrawFlags")
                    .constant("FLAG_TRANSPARENT", 0)
                    .constant("FLAG_SHADED", 1)
                    .constant("FLAG_DOUBLE_SIDED", 2)
                    .constant("FLAG_DISABLE_DEPTH_TEST", 3)
                    .constant("FLAG_FIXED_SIZE", 4)
                    .constant("FLAG_MAX", 5),
            ),
    );
    register_class(
        ClassRegistration::new("Sprite3D")
            .parent("SpriteBase3D")
            .property(PropertyInfo::new("texture", Variant::Nil))
            .property(PropertyInfo::new("hframes", Variant::Int(1)))
            .property(PropertyInfo::new("vframes", Variant::Int(1)))
            .property(PropertyInfo::new("frame", Variant::Int(0)))
            .property(PropertyInfo::new(
                "frame_coords",
                Variant::Vector2i(gdcore::math::Vector2i::new(0, 0)),
            ))
            .property(PropertyInfo::new("region_enabled", Variant::Bool(false)))
            .property(PropertyInfo::new(
                "region_rect",
                Variant::Rect2(gdcore::math::Rect2::new(
                    gdcore::math::Vector2::ZERO,
                    gdcore::math::Vector2::ZERO,
                )),
            ))
            .signal(SignalInfo::new("frame_changed"))
            .signal(SignalInfo::new("texture_changed")),
    );
    register_class(
        ClassRegistration::new("AnimatedSprite3D")
            .parent("SpriteBase3D")
            .property(PropertyInfo::new("sprite_frames", Variant::Nil))
            .property(PropertyInfo::new(
                "animation",
                Variant::StringName(gdcore::string_name::StringName::from("default")),
            ))
            .property(PropertyInfo::new(
                "autoplay",
                Variant::String(String::new()),
            ))
            .property(PropertyInfo::new("frame", Variant::Int(0)))
            .property(PropertyInfo::new("frame_progress", Variant::Float(0.0)))
            .property(PropertyInfo::new("speed_scale", Variant::Float(1.0)))
            .signal(SignalInfo::new("frame_changed"))
            .signal(SignalInfo::new("animation_changed"))
            .signal(SignalInfo::new("animation_finished"))
            .signal(SignalInfo::new("animation_looped"))
            .signal(SignalInfo::new("sprite_frames_changed")),
    );

    // -- Light3D (abstract base for all lights) --
    register_class(
        ClassRegistration::new("Light3D")
            .parent("VisualInstance3D")
            .abstract_class()
            .property(PropertyInfo::new("light_energy", Variant::Float(1.0)))
            .property(PropertyInfo::new(
                "light_color",
                Variant::Color(gdcore::math::Color::WHITE),
            ))
            .property(PropertyInfo::new("shadow_enabled", Variant::Bool(false)).with_hint(42))
            .property(PropertyInfo::new("shadow_bias", Variant::Float(0.1)))
            .property(PropertyInfo::new("shadow_blur", Variant::Float(1.0)))
            .property(PropertyInfo::new("shadow_normal_bias", Variant::Float(2.0)))
            .property(PropertyInfo::new("light_negative", Variant::Bool(false)))
            .property(PropertyInfo::new("light_specular", Variant::Float(0.5)))
            .property(PropertyInfo::new("light_bake_mode", Variant::Int(2))),
    );

    // -- DirectionalLight3D --
    register_class(
        ClassRegistration::new("DirectionalLight3D")
            .parent("Light3D")
            .property(PropertyInfo::new(
                "directional_shadow_mode",
                Variant::Int(2),
            ))
            .property(PropertyInfo::new(
                "directional_shadow_max_distance",
                Variant::Float(100.0),
            )),
    );

    // -- OmniLight3D --
    register_class(
        ClassRegistration::new("OmniLight3D")
            .parent("Light3D")
            .property(PropertyInfo::new("omni_range", Variant::Float(5.0)))
            .property(PropertyInfo::new("omni_attenuation", Variant::Float(1.0)))
            .property(PropertyInfo::new("omni_shadow_mode", Variant::Int(1))),
    );

    // -- SpotLight3D --
    register_class(
        ClassRegistration::new("SpotLight3D")
            .parent("Light3D")
            .property(PropertyInfo::new("spot_range", Variant::Float(5.0)))
            .property(PropertyInfo::new("spot_attenuation", Variant::Float(1.0)))
            .property(PropertyInfo::new("spot_angle", Variant::Float(45.0)))
            .property(PropertyInfo::new(
                "spot_angle_attenuation",
                Variant::Float(1.0),
            )),
    );

    // -- Physics 3D bodies --
    register_class(
        ClassRegistration::new("CollisionObject3D")
            .parent("Node3D")
            .abstract_class()
            .property(PropertyInfo::new("collision_layer", Variant::Int(1)))
            .property(PropertyInfo::new("collision_mask", Variant::Int(1))),
    );

    register_class(
        ClassRegistration::new("PhysicsBody3D")
            .parent("CollisionObject3D")
            .abstract_class(),
    );

    register_class(ClassRegistration::new("StaticBody3D").parent("PhysicsBody3D"));

    register_class(
        ClassRegistration::new("RigidBody3D")
            .parent("PhysicsBody3D")
            .property(PropertyInfo::new("mass", Variant::Float(1.0)))
            .property(PropertyInfo::new("gravity_scale", Variant::Float(1.0)))
            .property(PropertyInfo::new("freeze", Variant::Bool(false))),
    );

    register_class(
        ClassRegistration::new("CharacterBody3D")
            .parent("PhysicsBody3D")
            .property(PropertyInfo::new(
                "velocity",
                Variant::Vector3(Vector3::ZERO),
            ))
            .property(PropertyInfo::new(
                "floor_max_angle",
                Variant::Float(0.785398),
            )),
    );

    register_class(
        ClassRegistration::new("Area3D")
            .parent("CollisionObject3D")
            .property(PropertyInfo::new("monitoring", Variant::Bool(true)))
            .property(PropertyInfo::new("monitorable", Variant::Bool(true))),
    );

    register_class(
        ClassRegistration::new("CollisionShape3D")
            .parent("Node3D")
            .property(PropertyInfo::new("disabled", Variant::Bool(false))),
    );

    register_joint_3d_classes();

    // -- Shape3D resource hierarchy (pat-nzgdu.96) --
    // The shapes a CollisionShape3D.shape references. Shape3D is the Resource
    // base carrying `margin` (Godot default 0.04); every concrete shape derives
    // from it. Registering them lets ClassDB resolve `[sub_resource type="…"]`
    // shape resources in a .tscn to their proper class + parent chain.
    register_class(
        ClassRegistration::new("Shape3D")
            .parent("Resource")
            .property(PropertyInfo::new("margin", Variant::Float(0.04))),
    );
    register_class(
        ClassRegistration::new("SphereShape3D")
            .parent("Shape3D")
            .property(PropertyInfo::new("radius", Variant::Float(0.5))),
    );
    register_class(ClassRegistration::new("BoxShape3D").parent("Shape3D"));
    register_class(
        ClassRegistration::new("CapsuleShape3D")
            .parent("Shape3D")
            .property(PropertyInfo::new("radius", Variant::Float(0.5)))
            .property(PropertyInfo::new("height", Variant::Float(2.0))),
    );
    register_class(
        ClassRegistration::new("CylinderShape3D")
            .parent("Shape3D")
            .property(PropertyInfo::new("radius", Variant::Float(0.5)))
            .property(PropertyInfo::new("height", Variant::Float(2.0))),
    );
    register_class(ClassRegistration::new("ConvexPolygonShape3D").parent("Shape3D"));
    register_class(ClassRegistration::new("ConcavePolygonShape3D").parent("Shape3D"));
    register_class(ClassRegistration::new("WorldBoundaryShape3D").parent("Shape3D"));
    register_class(ClassRegistration::new("HeightMapShape3D").parent("Shape3D"));
    register_class(
        ClassRegistration::new("SeparationRayShape3D")
            .parent("Shape3D")
            .property(PropertyInfo::new("length", Variant::Float(1.0)))
            .property(PropertyInfo::new("slide_on_slope", Variant::Bool(false))),
    );

    // -- RayCast3D / ShapeCast3D (scene query nodes over PhysicsServer3D) --
    register_class(
        ClassRegistration::new("RayCast3D")
            .parent("Node3D")
            .property(PropertyInfo::new("enabled", Variant::Bool(true)))
            .property(PropertyInfo::new(
                "target_position",
                Variant::Vector3(Vector3::new(0.0, -1.0, 0.0)),
            ))
            .property(PropertyInfo::new(
                "collision_mask",
                Variant::Int(0xFFFF_FFFF),
            ))
            .property(PropertyInfo::new("exclude_parent", Variant::Bool(true)))
            .property(PropertyInfo::new(
                "collide_with_bodies",
                Variant::Bool(true),
            ))
            .property(PropertyInfo::new(
                "collide_with_areas",
                Variant::Bool(false),
            ))
            .property(PropertyInfo::new("hit_back_faces", Variant::Bool(true)))
            .property(PropertyInfo::new("hit_from_inside", Variant::Bool(false))),
    );
    register_class(
        ClassRegistration::new("ShapeCast3D")
            .parent("Node3D")
            .property(PropertyInfo::new("enabled", Variant::Bool(true)))
            .property(PropertyInfo::new(
                "target_position",
                Variant::Vector3(Vector3::new(0.0, -1.0, 0.0)),
            ))
            .property(PropertyInfo::new(
                "collision_mask",
                Variant::Int(0xFFFF_FFFF),
            ))
            .property(PropertyInfo::new("exclude_parent", Variant::Bool(true)))
            .property(PropertyInfo::new(
                "collide_with_bodies",
                Variant::Bool(true),
            ))
            .property(PropertyInfo::new(
                "collide_with_areas",
                Variant::Bool(false),
            ))
            .property(PropertyInfo::new("max_results", Variant::Int(32))),
    );
    register_class(
        ClassRegistration::new("SpringArm3D")
            .parent("Node3D")
            .property(PropertyInfo::new("spring_length", Variant::Float(1.0)))
            .property(PropertyInfo::new("margin", Variant::Float(0.01)))
            .property(PropertyInfo::new(
                "collision_mask",
                Variant::Int(0xFFFF_FFFF),
            )),
    );

    // -- Environment / World --
    register_class(ClassRegistration::new("WorldEnvironment").parent("Node3D"));
    register_class(ClassRegistration::new("Marker3D").parent("Node3D"));

    // -- XR (pat-p0s82.29): high-use runtime class registered with defaults so
    // ClassDB recognizes it; full XR session handling is a separate concern. --
    register_class(
        ClassRegistration::new("XROrigin3D")
            .parent("Node3D")
            .property(PropertyInfo::new("world_scale", Variant::Float(1.0)))
            .property(PropertyInfo::new("current", Variant::Bool(false))),
    );

    // XR node + server family (pat-p0s82.23). XRServer is the Object singleton
    // fed by the mock pose backend (gdplatform::xr); the spatial nodes place XR
    // tracking data into the scene. XROrigin3D is registered just above.
    register_class(ClassRegistration::new("XRServer").parent("Object"));
    register_class(
        ClassRegistration::new("XRNode3D")
            .parent("Node3D")
            .abstract_class()
            .property(PropertyInfo::new("tracker", Variant::String(String::new()))),
    );
    register_class(ClassRegistration::new("XRController3D").parent("XRNode3D"));
    register_class(ClassRegistration::new("XRAnchor3D").parent("XRNode3D"));
    register_class(ClassRegistration::new("XRCamera3D").parent("Camera3D"));

    // -- Additional 3D types --
    // NavigationRegion3D carries the NavigationMesh the runtime uploads to the
    // navigation map. Defaults diffed against doc/classes/NavigationRegion3D.xml
    // — the doc XML is the authoritative source for property defaults
    // (pat-0q2kx.26). `navigation_mesh` defaults to None, so it is registered as
    // Nil rather than an invented empty resource.
    register_class(
        ClassRegistration::new("NavigationRegion3D")
            .parent("Node3D")
            .property(PropertyInfo::new("navigation_mesh", Variant::Nil))
            .property(PropertyInfo::new("enabled", Variant::Bool(true)))
            .property(PropertyInfo::new("navigation_layers", Variant::Int(1)))
            .property(PropertyInfo::new("enter_cost", Variant::Float(0.0)))
            .property(PropertyInfo::new("travel_cost", Variant::Float(1.0)))
            .property(PropertyInfo::new(
                "use_edge_connections",
                Variant::Bool(true),
            )),
    );
    // NavigationAgent3D — scene-tree pathfinding agent (pat-rzs4o).
    register_class(
        ClassRegistration::new("NavigationAgent3D")
            .parent("Node")
            .property(PropertyInfo::new(
                "target_position",
                Variant::Vector3(Vector3::ZERO),
            ))
            .property(PropertyInfo::new(
                "path_desired_distance",
                Variant::Float(1.0),
            ))
            .property(PropertyInfo::new("avoidance_enabled", Variant::Bool(false)))
            // pat-nzgdu.10 — full NavigationAgent3D property surface (Godot parity).
            .property(PropertyInfo::new(
                "target_desired_distance",
                Variant::Float(10.0),
            ))
            .property(PropertyInfo::new("path_height_offset", Variant::Float(0.0)))
            .property(PropertyInfo::new("path_max_distance", Variant::Float(5.0)))
            .property(PropertyInfo::new("navigation_layers", Variant::Int(1)))
            .property(PropertyInfo::new("pathfinding_algorithm", Variant::Int(0)))
            .property(PropertyInfo::new("path_postprocessing", Variant::Int(0)))
            .property(PropertyInfo::new("path_metadata_flags", Variant::Int(7)))
            .property(PropertyInfo::new("simplify_path", Variant::Bool(false)))
            .property(PropertyInfo::new("simplify_epsilon", Variant::Float(0.0)))
            .property(PropertyInfo::new(
                "path_return_max_length",
                Variant::Float(0.0),
            ))
            .property(PropertyInfo::new(
                "path_return_max_radius",
                Variant::Float(0.0),
            ))
            .property(PropertyInfo::new(
                "path_search_max_polygons",
                Variant::Int(4096),
            ))
            .property(PropertyInfo::new(
                "path_search_max_distance",
                Variant::Float(0.0),
            ))
            .property(PropertyInfo::new(
                "velocity",
                Variant::Vector3(Vector3::ZERO),
            ))
            .property(PropertyInfo::new("height", Variant::Float(1.0)))
            .property(PropertyInfo::new("radius", Variant::Float(0.5)))
            .property(PropertyInfo::new("neighbor_distance", Variant::Float(50.0)))
            .property(PropertyInfo::new("max_neighbors", Variant::Int(10)))
            .property(PropertyInfo::new(
                "time_horizon_agents",
                Variant::Float(1.0),
            ))
            .property(PropertyInfo::new(
                "time_horizon_obstacles",
                Variant::Float(0.0),
            ))
            .property(PropertyInfo::new("max_speed", Variant::Float(10.0)))
            .property(PropertyInfo::new("use_3d_avoidance", Variant::Bool(false)))
            .property(PropertyInfo::new("keep_y_velocity", Variant::Bool(true)))
            .property(PropertyInfo::new("avoidance_layers", Variant::Int(1)))
            .property(PropertyInfo::new("avoidance_mask", Variant::Int(1)))
            .property(PropertyInfo::new("avoidance_priority", Variant::Float(1.0)))
            .property(PropertyInfo::new("debug_enabled", Variant::Bool(false)))
            .property(PropertyInfo::new("debug_use_custom", Variant::Bool(false)))
            .property(PropertyInfo::new(
                "debug_path_custom_color",
                Variant::Color(gdcore::math::Color::new(1.0, 1.0, 1.0, 1.0)),
            ))
            .property(PropertyInfo::new(
                "debug_path_custom_point_size",
                Variant::Float(4.0),
            ))
            // The avoidance solve hands the agent its collision-free velocity
            // through this signal every physics tick; a game connects to it and
            // feeds the result to `move_and_slide` (pat-0q2kx.97).
            .signal(SignalInfo::new("velocity_computed").arg("safe_velocity"))
            .signal(SignalInfo::new("path_changed"))
            .signal(SignalInfo::new("target_reached"))
            .signal(SignalInfo::new("navigation_finished"))
            .signal(SignalInfo::new("waypoint_reached").arg("details"))
            .signal(SignalInfo::new("link_reached").arg("details")),
    );
    // NavigationObstacle3D — the dynamic avoidance obstacle (pat-0q2kx.97).
    //
    // Godot's obstacle contributes a moving radius(+height) cylinder that
    // agents steer around, plus a static vertex outline for navmesh baking.
    // Patina models the cylinder, which is the half that participates in the
    // RVO solve; `vertices` is registered so a `.tscn` carrying one round-trips
    // rather than losing the property.
    register_class(
        ClassRegistration::new("NavigationObstacle3D")
            .parent("Node3D")
            .property(PropertyInfo::new("radius", Variant::Float(0.0)))
            .property(PropertyInfo::new("height", Variant::Float(1.0)))
            .property(PropertyInfo::new(
                "velocity",
                Variant::Vector3(Vector3::ZERO),
            ))
            .property(PropertyInfo::new("avoidance_enabled", Variant::Bool(true)))
            .property(PropertyInfo::new("avoidance_layers", Variant::Int(1)))
            .property(PropertyInfo::new(
                "affect_navigation_mesh",
                Variant::Bool(false),
            ))
            .property(PropertyInfo::new(
                "carve_navigation_mesh",
                Variant::Bool(false),
            ))
            .property(PropertyInfo::new("vertices", Variant::Array(Vec::new()))),
    );
    // Vehicle / soft body scene types (physics sim lives in gdphysics2d; pat-n1gh1).
    register_class(
        ClassRegistration::new("VehicleBody3D")
            .parent("RigidBody3D")
            .property(PropertyInfo::new("engine_force", Variant::Float(0.0)))
            .property(PropertyInfo::new("brake", Variant::Float(0.0)))
            .property(PropertyInfo::new("steering", Variant::Float(0.0))),
    );
    register_class(
        // The full 14-property surface Godot exposes (pat-nzgdu.127). Names and
        // types are diffed against `extension_api.json`; the defaults are
        // Godot's own documented values, which the dump does not carry.
        //
        // ⚠️ These are the NODE's parameters and they are NOT the sim's. The
        // suspension model in `gdphysics2d::vehicle3d` is a different
        // parameterisation: it takes one `suspension_damping` where Godot has
        // the `damping_compression`/`damping_relaxation` pair, and its
        // `suspension_stiffness` is force-per-unit-compression (default 5000.0)
        // where Godot's is its own unit (default 5.88). Copying these values
        // straight into the sim would under-spring the chassis by ~3 orders of
        // magnitude and drop the car through the floor — the bridge has to map
        // semantics, not fields. See the bead notes.
        ClassRegistration::new("VehicleWheel3D")
            .parent("Node3D")
            .property(PropertyInfo::new("engine_force", Variant::Float(0.0)))
            .property(PropertyInfo::new("brake", Variant::Float(0.0)))
            .property(PropertyInfo::new("steering", Variant::Float(0.0)))
            .property(PropertyInfo::new("use_as_traction", Variant::Bool(false)))
            .property(PropertyInfo::new("use_as_steering", Variant::Bool(false)))
            // How much this wheel's suspension resists chassis roll in a turn.
            .property(PropertyInfo::new(
                "wheel_roll_influence",
                Variant::Float(0.1),
            ))
            .property(PropertyInfo::new("wheel_radius", Variant::Float(0.5)))
            // Distance from the wheel's attachment point to its rest position.
            .property(PropertyInfo::new("wheel_rest_length", Variant::Float(0.15)))
            // Tire grip. Lower slides, higher grips; 10.5 is Godot's default.
            .property(PropertyInfo::new(
                "wheel_friction_slip",
                Variant::Float(10.5),
            ))
            .property(PropertyInfo::new("suspension_travel", Variant::Float(0.2)))
            .property(PropertyInfo::new(
                "suspension_stiffness",
                Variant::Float(5.88),
            ))
            .property(PropertyInfo::new(
                "suspension_max_force",
                Variant::Float(6000.0),
            ))
            // Godot splits damping in two: resisting compression vs extension.
            .property(PropertyInfo::new(
                "damping_compression",
                Variant::Float(0.83),
            ))
            .property(PropertyInfo::new(
                "damping_relaxation",
                Variant::Float(0.88),
            )),
    );
    register_class(
        ClassRegistration::new("SoftBody3D")
            .parent("MeshInstance3D")
            .property(PropertyInfo::new("simulation_precision", Variant::Int(5)))
            .property(PropertyInfo::new("total_mass", Variant::Float(1.0)))
            .property(PropertyInfo::new("linear_stiffness", Variant::Float(0.5)))
            .property(PropertyInfo::new(
                "pressure_coefficient",
                Variant::Float(0.0),
            ))
            .property(PropertyInfo::new(
                "damping_coefficient",
                Variant::Float(0.01),
            )),
    );
    register_class(ClassRegistration::new("Skeleton3D").parent("Node3D"));
    register_class(ClassRegistration::new("BoneAttachment3D").parent("Node3D"));

    // -- Skeleton modifier stack (pat-nzgdu.151, pat-nzgdu.182) ------------
    // Godot: a SkeletonModifier3D is a Node3D child of a Skeleton3D whose
    // _process_modification runs during the skeleton's post-pose update, so
    // modifiers layer on top of AnimationPlayer/Tree output.
    //
    // NOT abstract: `extension_api.json` reports is_instantiable=true (contrast
    // PhysicsBody3D/CollisionObject3D, which report false). Godot lets you add a
    // bare SkeletonModifier3D to a scene; it simply modifies nothing.
    //
    // The four axis/direction enums live on the *base*, so every modifier
    // inherits one shared vocabulary rather than redeclaring its own.
    register_class(
        ClassRegistration::new("SkeletonModifier3D")
            .parent("Node3D")
            .property(PropertyInfo::new("active", Variant::Bool(true)))
            .property(PropertyInfo::new("influence", Variant::Float(1.0)))
            .method(MethodInfo::new("get_skeleton", 0))
            .method(MethodInfo::new("set_active", 1))
            .method(MethodInfo::new("is_active", 0))
            .method(MethodInfo::new("set_influence", 1))
            .method(MethodInfo::new("get_influence", 0))
            // The virtuals a subclass overrides. `_process_modification` is the
            // pose-update hook itself — the seam the constraint/twist solvers
            // will run from once they exist (see the bead notes).
            .method(MethodInfo::new("_process_modification", 0))
            .method(MethodInfo::new("_process_modification_with_delta", 1))
            .method(MethodInfo::new("_skeleton_changed", 2))
            .method(MethodInfo::new("_validate_bone_names", 0))
            .signal(SignalInfo::new("modification_processed"))
            .enumeration(
                EnumInfo::new("BoneAxis")
                    .constant("BONE_AXIS_PLUS_X", 0)
                    .constant("BONE_AXIS_MINUS_X", 1)
                    .constant("BONE_AXIS_PLUS_Y", 2)
                    .constant("BONE_AXIS_MINUS_Y", 3)
                    .constant("BONE_AXIS_PLUS_Z", 4)
                    .constant("BONE_AXIS_MINUS_Z", 5),
            )
            // BoneDirection mirrors BoneAxis but adds FROM_PARENT (6) — "point
            // the way the parent bone points" has no fixed-axis equivalent.
            .enumeration(
                EnumInfo::new("BoneDirection")
                    .constant("BONE_DIRECTION_PLUS_X", 0)
                    .constant("BONE_DIRECTION_MINUS_X", 1)
                    .constant("BONE_DIRECTION_PLUS_Y", 2)
                    .constant("BONE_DIRECTION_MINUS_Y", 3)
                    .constant("BONE_DIRECTION_PLUS_Z", 4)
                    .constant("BONE_DIRECTION_MINUS_Z", 5)
                    .constant("BONE_DIRECTION_FROM_PARENT", 6),
            )
            // NOTE the off-by-one against BoneAxis: SecondaryDirection reserves 0
            // for NONE, so PLUS_X is 1 here but 0 there. Copying BoneAxis's
            // numbering would silently mean "no secondary direction".
            .enumeration(
                EnumInfo::new("SecondaryDirection")
                    .constant("SECONDARY_DIRECTION_NONE", 0)
                    .constant("SECONDARY_DIRECTION_PLUS_X", 1)
                    .constant("SECONDARY_DIRECTION_MINUS_X", 2)
                    .constant("SECONDARY_DIRECTION_PLUS_Y", 3)
                    .constant("SECONDARY_DIRECTION_MINUS_Y", 4)
                    .constant("SECONDARY_DIRECTION_PLUS_Z", 5)
                    .constant("SECONDARY_DIRECTION_MINUS_Z", 6)
                    .constant("SECONDARY_DIRECTION_CUSTOM", 7),
            )
            .enumeration(
                EnumInfo::new("RotationAxis")
                    .constant("ROTATION_AXIS_X", 0)
                    .constant("ROTATION_AXIS_Y", 1)
                    .constant("ROTATION_AXIS_Z", 2)
                    .constant("ROTATION_AXIS_ALL", 3)
                    .constant("ROTATION_AXIS_CUSTOM", 4),
            ),
    );

    // -- Ragdoll (pat-nzgdu.175) -------------------------------------------
    //
    // A ragdoll is a `PhysicalBoneSimulator3D` (itself a SkeletonModifier3D)
    // holding one `PhysicalBone3D` rigid body per skeleton bone, joint-connected
    // to its parent bone. `physical_bones_start_simulation()` hands pose control
    // to the bodies; each physics frame their transforms are written back to the
    // Skeleton3D bone poses.
    //
    // Schemas diffed against Godot 4.6.1's `extension_api.json`; the *defaults*
    // are Godot's documented values (the dump carries no property defaults).
    //
    // Class surface only — no bodies are created and nothing is stepped. See the
    // bead notes: the sim needs the joints work plus a MainLoop physics hook.
    register_class(
        ClassRegistration::new("PhysicalBone3D")
            .parent("PhysicsBody3D")
            // Which joint ties this bone to its parent. NONE (0) is Godot's
            // default — a bone is unconstrained until the ragdoll is set up.
            .property(PropertyInfo::new("joint_type", Variant::Int(0)))
            .property(PropertyInfo::new(
                "joint_offset",
                Variant::Transform3D(gdcore::math3d::Transform3D::IDENTITY),
            ))
            .property(PropertyInfo::new(
                "joint_rotation",
                Variant::Vector3(gdcore::math::Vector3::ZERO),
            ))
            .property(PropertyInfo::new(
                "body_offset",
                Variant::Transform3D(gdcore::math3d::Transform3D::IDENTITY),
            ))
            .property(PropertyInfo::new("mass", Variant::Float(1.0)))
            .property(PropertyInfo::new("friction", Variant::Float(1.0)))
            .property(PropertyInfo::new("bounce", Variant::Float(0.0)))
            .property(PropertyInfo::new("gravity_scale", Variant::Float(1.0)))
            .property(PropertyInfo::new("custom_integrator", Variant::Bool(false)))
            .property(PropertyInfo::new("linear_damp_mode", Variant::Int(0)))
            .property(PropertyInfo::new("linear_damp", Variant::Float(0.0)))
            .property(PropertyInfo::new("angular_damp_mode", Variant::Int(0)))
            .property(PropertyInfo::new("angular_damp", Variant::Float(0.0)))
            .property(PropertyInfo::new(
                "linear_velocity",
                Variant::Vector3(gdcore::math::Vector3::ZERO),
            ))
            .property(PropertyInfo::new(
                "angular_velocity",
                Variant::Vector3(gdcore::math::Vector3::ZERO),
            ))
            .property(PropertyInfo::new("can_sleep", Variant::Bool(true)))
            .enumeration(
                EnumInfo::new("DampMode")
                    .constant("DAMP_MODE_COMBINE", 0)
                    .constant("DAMP_MODE_REPLACE", 1),
            )
            .enumeration(
                EnumInfo::new("JointType")
                    .constant("JOINT_TYPE_NONE", 0)
                    .constant("JOINT_TYPE_PIN", 1)
                    .constant("JOINT_TYPE_CONE", 2)
                    .constant("JOINT_TYPE_HINGE", 3)
                    .constant("JOINT_TYPE_SLIDER", 4)
                    .constant("JOINT_TYPE_6DOF", 5),
            ),
    );
    // Godot gives the simulator no properties of its own — it is driven entirely
    // by physical_bones_start_simulation()/stop_simulation(), and inherits
    // active/influence from SkeletonModifier3D.
    register_class(ClassRegistration::new("PhysicalBoneSimulator3D").parent("SkeletonModifier3D"));

    // -- Bone constraints (pat-nzgdu.182) ----------------------------------
    //
    // BoneConstraint3D holds a *list* of constraint settings, each binding an
    // "apply" bone to a "reference" bone-or-node. Its whole surface is therefore
    // indexed methods — set_amount(index, amount), get_apply_bone(index) — and
    // Godot generates the `settings/N/...` properties at runtime from
    // setting_count via _get_property_list. That is why the static property list
    // is EMPTY here: registering `amount`/`apply_bone` as plain properties would
    // invent a schema Godot does not have.
    //
    // It is a concrete class in its own right *and* the base for AimModifier3D /
    // CopyTransformModifier3D / ConvertTransformModifier3D.
    register_class(
        ClassRegistration::new("BoneConstraint3D")
            .parent("SkeletonModifier3D")
            // A setting's reference is either a skeleton bone or an arbitrary
            // node — which is why reference_bone and reference_node coexist.
            .enumeration(
                EnumInfo::new("ReferenceType")
                    .constant("REFERENCE_TYPE_BONE", 0)
                    .constant("REFERENCE_TYPE_NODE", 1),
            )
            .method(MethodInfo::new("set_setting_count", 1))
            .method(MethodInfo::new("get_setting_count", 0))
            // Singular — contrast BoneTwistDisperser3D::clear_settings. Godot's
            // own inconsistency; "normalising" either name breaks parity.
            .method(MethodInfo::new("clear_setting", 0))
            // Every accessor below is indexed by the setting: the setters take
            // (index, value) and the getters take (index).
            .method(MethodInfo::new("set_amount", 2))
            .method(MethodInfo::new("get_amount", 1))
            .method(MethodInfo::new("set_apply_bone_name", 2))
            .method(MethodInfo::new("get_apply_bone_name", 1))
            .method(MethodInfo::new("set_apply_bone", 2))
            .method(MethodInfo::new("get_apply_bone", 1))
            .method(MethodInfo::new("set_reference_type", 2))
            .method(MethodInfo::new("get_reference_type", 1))
            .method(MethodInfo::new("set_reference_bone_name", 2))
            .method(MethodInfo::new("get_reference_bone_name", 1))
            .method(MethodInfo::new("set_reference_bone", 2))
            .method(MethodInfo::new("get_reference_bone", 1))
            .method(MethodInfo::new("set_reference_node", 2))
            .method(MethodInfo::new("get_reference_node", 1)),
    );

    // BoneTwistDisperser3D spreads a bone's roll across the chain between a root
    // and an end bone, so a wrist twisting 90° does not "candy-wrapper" the
    // forearm — each intermediate joint takes a share.
    //
    // Unlike BoneConstraint3D it does expose two static properties; everything
    // per-setting (root/end bone, twist source, damping curve) stays indexed.
    register_class(
        ClassRegistration::new("BoneTwistDisperser3D")
            .parent("SkeletonModifier3D")
            // Godot's default is TRUE (doc XML) — the disperser recomputes bone
            // axes as the chain moves. Defaulting this to false would freeze the
            // axes and read as "the twist is distributed wrongly".
            .property(PropertyInfo::new("mutable_bone_axes", Variant::Bool(true)))
            .property(PropertyInfo::new("setting_count", Variant::Int(0)))
            .enumeration(
                EnumInfo::new("DisperseMode")
                    .constant("DISPERSE_MODE_EVEN", 0)
                    .constant("DISPERSE_MODE_WEIGHTED", 1)
                    .constant("DISPERSE_MODE_CUSTOM", 2),
            )
            .method(MethodInfo::new("set_setting_count", 1))
            .method(MethodInfo::new("get_setting_count", 0))
            // Plural here, singular on BoneConstraint3D. See above.
            .method(MethodInfo::new("clear_settings", 0))
            .method(MethodInfo::new("set_mutable_bone_axes", 1))
            // Reads as a question, not a getter — Godot's bool getter idiom.
            .method(MethodInfo::new("are_bone_axes_mutable", 0))
            .method(MethodInfo::new("set_root_bone_name", 2))
            .method(MethodInfo::new("get_root_bone_name", 1))
            .method(MethodInfo::new("set_root_bone", 2))
            .method(MethodInfo::new("get_root_bone", 1))
            .method(MethodInfo::new("set_end_bone_name", 2))
            .method(MethodInfo::new("get_end_bone_name", 1))
            .method(MethodInfo::new("set_end_bone", 2))
            .method(MethodInfo::new("get_end_bone", 1))
            // Read-only: the reference bone is derived from the chain, not set.
            .method(MethodInfo::new("get_reference_bone_name", 1))
            .method(MethodInfo::new("get_reference_bone", 1))
            .method(MethodInfo::new("set_extend_end_bone", 2))
            .method(MethodInfo::new("is_end_bone_extended", 1))
            .method(MethodInfo::new("set_end_bone_direction", 2))
            .method(MethodInfo::new("get_end_bone_direction", 1))
            .method(MethodInfo::new("set_twist_from_rest", 2))
            .method(MethodInfo::new("is_twist_from_rest", 1))
            .method(MethodInfo::new("set_twist_from", 2))
            .method(MethodInfo::new("get_twist_from", 1))
            .method(MethodInfo::new("set_disperse_mode", 2))
            .method(MethodInfo::new("get_disperse_mode", 1))
            .method(MethodInfo::new("set_weight_position", 2))
            .method(MethodInfo::new("get_weight_position", 1))
            .method(MethodInfo::new("set_damping_curve", 2))
            .method(MethodInfo::new("get_damping_curve", 1))
            // Joints are addressed by (setting_index, joint_index) — hence one
            // more argument than the per-setting accessors above.
            .method(MethodInfo::new("get_joint_count", 1))
            .method(MethodInfo::new("get_joint_bone_name", 2))
            .method(MethodInfo::new("get_joint_bone", 2))
            .method(MethodInfo::new("get_joint_twist_amount", 2))
            .method(MethodInfo::new("set_joint_twist_amount", 3)),
    );

    // -- IK solver chain (pat-nzgdu.183) -----------------------------------
    //
    // Godot's real hierarchy is deeper than "an IK modifier":
    //   SkeletonModifier3D <- IKModifier3D <- ChainIK3D <- IterateIK3D <- FABRIK3D
    // with CCDIK3D and JacobianIK3D as FABRIK3D's siblings. Only the leaves are
    // instantiable; ChainIK3D and IterateIK3D are abstract bases that exist to
    // share the chain definition and the iteration budget.
    //
    // ChainIK3D holds *which bones* — root/end, and the derived joint list. Like
    // BoneConstraint3D its surface is indexed methods rather than properties,
    // because one modifier can drive several chains.
    register_class(
        ClassRegistration::new("ChainIK3D")
            .parent("IKModifier3D")
            .abstract_class()
            .method(MethodInfo::new("set_root_bone_name", 2))
            .method(MethodInfo::new("get_root_bone_name", 1))
            .method(MethodInfo::new("set_root_bone", 2))
            .method(MethodInfo::new("get_root_bone", 1))
            .method(MethodInfo::new("set_end_bone_name", 2))
            .method(MethodInfo::new("get_end_bone_name", 1))
            .method(MethodInfo::new("set_end_bone", 2))
            .method(MethodInfo::new("get_end_bone", 1))
            .method(MethodInfo::new("set_extend_end_bone", 2))
            .method(MethodInfo::new("is_end_bone_extended", 1))
            .method(MethodInfo::new("set_end_bone_direction", 2))
            .method(MethodInfo::new("get_end_bone_direction", 1))
            .method(MethodInfo::new("set_end_bone_length", 2))
            .method(MethodInfo::new("get_end_bone_length", 1))
            // Joints are derived from the root→end chain, so they are read-only
            // and addressed by (setting_index, joint_index).
            .method(MethodInfo::new("get_joint_count", 1))
            .method(MethodInfo::new("get_joint_bone_name", 2))
            .method(MethodInfo::new("get_joint_bone", 2)),
    );

    // IterateIK3D adds the iteration budget shared by every iterative solver.
    register_class(
        ClassRegistration::new("IterateIK3D")
            .parent("ChainIK3D")
            .abstract_class()
            // Godot's defaults (doc XML). Note max_iterations is 4, NOT a big
            // number — these solvers run every frame and converge over frames.
            .property(PropertyInfo::new("max_iterations", Variant::Int(4)))
            // The distance at which the end effector counts as "arrived".
            .property(PropertyInfo::new("min_distance", Variant::Float(0.001)))
            // 0.034906585 rad == exactly 2 degrees — the per-frame rotation cap
            // that keeps a solve from snapping.
            .property(PropertyInfo::new(
                "angular_delta_limit",
                Variant::Float(0.034_906_585),
            ))
            .property(PropertyInfo::new("deterministic", Variant::Bool(false)))
            .property(PropertyInfo::new("setting_count", Variant::Int(0)))
            .method(MethodInfo::new("set_max_iterations", 1))
            .method(MethodInfo::new("get_max_iterations", 0))
            .method(MethodInfo::new("set_min_distance", 1))
            .method(MethodInfo::new("get_min_distance", 0))
            .method(MethodInfo::new("set_angular_delta_limit", 1))
            .method(MethodInfo::new("get_angular_delta_limit", 0))
            .method(MethodInfo::new("set_deterministic", 1))
            // Bool getter reads as a question, not `get_`.
            .method(MethodInfo::new("is_deterministic", 0))
            // The chain aims at a NODE, not a bare position — per setting.
            .method(MethodInfo::new("set_target_node", 2))
            .method(MethodInfo::new("get_target_node", 1))
            // Per-joint limits, addressed by (setting_index, joint_index).
            .method(MethodInfo::new("set_joint_rotation_axis", 3))
            .method(MethodInfo::new("get_joint_rotation_axis", 2))
            .method(MethodInfo::new("set_joint_rotation_axis_vector", 3))
            .method(MethodInfo::new("get_joint_rotation_axis_vector", 2))
            .method(MethodInfo::new("set_joint_limitation", 3))
            .method(MethodInfo::new("get_joint_limitation", 2))
            .method(MethodInfo::new("set_joint_limitation_right_axis", 3))
            .method(MethodInfo::new("get_joint_limitation_right_axis", 2))
            .method(MethodInfo::new("set_joint_limitation_right_axis_vector", 3))
            .method(MethodInfo::new("get_joint_limitation_right_axis_vector", 2))
            .method(MethodInfo::new("set_joint_limitation_rotation_offset", 3))
            .method(MethodInfo::new("get_joint_limitation_rotation_offset", 2)),
    );

    // FABRIK3D declares NOTHING of its own — it is purely the solver choice.
    // Every knob (chain, target, iterations, limits) is inherited. Giving it
    // properties would invent a schema Godot does not have; the difference
    // between FABRIK3D and its siblings CCDIK3D/JacobianIK3D is only the
    // algorithm that runs during the modifier pass.
    register_class(ClassRegistration::new("FABRIK3D").parent("IterateIK3D"));
    // The siblings the comment above describes, now registered too
    // (pat-0q2kx.156). Same reasoning: no properties of their own.
    register_class(ClassRegistration::new("CCDIK3D").parent("IterateIK3D"));
    register_class(ClassRegistration::new("JacobianIK3D").parent("IterateIK3D"));

    register_class(
        ClassRegistration::new("IKModifier3D")
            .parent("SkeletonModifier3D")
            .property(PropertyInfo::new(
                "target_position",
                Variant::Vector3(Vector3::ZERO),
            ))
            .property(PropertyInfo::new("root_bone", Variant::Int(-1)))
            .property(PropertyInfo::new("tip_bone", Variant::Int(-1)))
            .property(PropertyInfo::new("iterations", Variant::Int(10))),
    );
    // Animation resources + the Node <- AnimationMixer <- Player/Tree chain
    // (pat-0q2kx.1). `Animation` and `AnimationLibrary` were absent entirely, so
    // `Animation.new()` was not script-constructible and a `.tscn`
    // `[sub_resource type="Animation"]` had no class to resolve; the two mixers
    // hung directly off `Node`, collapsing Godot's real hierarchy
    // (upstream/godot/doc/classes/AnimationPlayer.xml inherits AnimationMixer).
    register_class(
        ClassRegistration::new("Animation")
            .parent("Resource")
            .property(PropertyInfo::new("length", Variant::Float(1.0)))
            .property(PropertyInfo::new("loop_mode", Variant::Int(0)))
            .property(PropertyInfo::new("step", Variant::Float(0.033_333_335)))
            .property(PropertyInfo::new("capture_included", Variant::Bool(false))),
    );
    register_class(ClassRegistration::new("AnimationLibrary").parent("Resource"));
    register_class(
        ClassRegistration::new("AnimationMixer")
            .parent("Node")
            .abstract_class()
            .property(PropertyInfo::new("active", Variant::Bool(true)))
            .property(PropertyInfo::new("deterministic", Variant::Bool(false)))
            .property(PropertyInfo::new("reset_on_save", Variant::Bool(true)))
            .property(PropertyInfo::new("root_motion_local", Variant::Bool(false)))
            .property(PropertyInfo::new(
                "root_motion_track",
                Variant::NodePath(gdcore::node_path::NodePath::new("")),
            ))
            .property(PropertyInfo::new(
                "root_node",
                Variant::NodePath(gdcore::node_path::NodePath::new("..")),
            ))
            .property(PropertyInfo::new("audio_max_polyphony", Variant::Int(32)))
            .property(PropertyInfo::new("callback_mode_process", Variant::Int(1)))
            .property(PropertyInfo::new("callback_mode_method", Variant::Int(0)))
            .property(PropertyInfo::new("callback_mode_discrete", Variant::Int(1)))
            .signal(SignalInfo::new("animation_started").arg("anim_name"))
            .signal(SignalInfo::new("animation_finished").arg("anim_name"))
            .signal(
                SignalInfo::new("animation_changed")
                    .arg("old_name")
                    .arg("new_name"),
            )
            .signal(SignalInfo::new("animation_libraries_updated"))
            .signal(SignalInfo::new("animation_list_changed"))
            .signal(SignalInfo::new("caches_cleared"))
            .signal(SignalInfo::new("mixer_applied"))
            .signal(SignalInfo::new("mixer_updated")),
    );
    register_class(
        ClassRegistration::new("AnimationPlayer")
            .parent("AnimationMixer")
            .property(PropertyInfo::new(
                "autoplay",
                Variant::StringName(gdcore::string_name::StringName::new("")),
            ))
            .property(PropertyInfo::new(
                "current_animation",
                Variant::StringName(gdcore::string_name::StringName::new("")),
            ))
            .property(PropertyInfo::new("speed_scale", Variant::Float(1.0)))
            .property(PropertyInfo::new(
                "playback_default_blend_time",
                Variant::Float(0.0),
            ))
            .property(PropertyInfo::new(
                "playback_auto_capture",
                Variant::Bool(true),
            ))
            .property(PropertyInfo::new(
                "movie_quit_on_finish",
                Variant::Bool(false),
            ))
            .signal(SignalInfo::new("current_animation_changed").arg("name"))
            .signal(
                SignalInfo::new("animation_changed")
                    .arg("old_name")
                    .arg("new_name"),
            ),
    );
    register_class(ClassRegistration::new("AnimationTree").parent("AnimationMixer"));

    // -- Tween + the Tweener family (pat-0q2kx.4) ---------------------------
    //
    // The tween engine (gdscene/src/tween.rs) and its five tweener kinds have
    // existed and been tested for a long time, but ClassDB knew none of them:
    // `ClassDB.class_exists("Tween")` was false and the object a script gets
    // back from `create_tween()` was not introspectable.
    //
    // Schema diffed against `fixtures/editor_oracle/godot_extension_api.json`.
    // ⚠️ `Tweener` is the one ABSTRACT member — a script never constructs one;
    // it receives them from the `tween_*` factory methods. Note also that
    // `Tween` is a `RefCounted`, NOT a Resource: a tween is a live playback
    // object, never saved.
    if !class_exists("Tween") {
        register_class(
            ClassRegistration::new("Tween")
                .parent("RefCounted")
                .method(MethodInfo::new("tween_property", 4))
                .method(MethodInfo::new("tween_interval", 1))
                .method(MethodInfo::new("tween_callback", 1))
                .method(MethodInfo::new("tween_method", 4))
                .method(MethodInfo::new("tween_subtween", 1))
                .method(MethodInfo::new("custom_step", 1))
                .method(MethodInfo::new("stop", 0))
                .method(MethodInfo::new("pause", 0))
                .method(MethodInfo::new("play", 0))
                .method(MethodInfo::new("kill", 0))
                .method(MethodInfo::new("get_total_elapsed_time", 0))
                .method(MethodInfo::new("is_running", 0))
                .method(MethodInfo::new("is_valid", 0))
                .method(MethodInfo::new("bind_node", 1))
                .method(MethodInfo::new("set_process_mode", 1))
                .method(MethodInfo::new("set_pause_mode", 1))
                .method(MethodInfo::new("set_ignore_time_scale", 1))
                .method(MethodInfo::new("set_parallel", 1))
                .method(MethodInfo::new("set_loops", 1))
                .method(MethodInfo::new("get_loops_left", 0))
                .method(MethodInfo::new("set_speed_scale", 1))
                .method(MethodInfo::new("set_trans", 1))
                .method(MethodInfo::new("set_ease", 1))
                .method(MethodInfo::new("parallel", 0))
                .method(MethodInfo::new("chain", 0))
                .method(MethodInfo::new("interpolate_value", 6))
                .signal(SignalInfo::new("step_finished").arg("idx"))
                .signal(SignalInfo::new("loop_finished").arg("loop_count"))
                .signal(SignalInfo::new("finished"))
                .enumeration(
                    EnumInfo::new("TweenProcessMode")
                        .constant("TWEEN_PROCESS_PHYSICS", 0)
                        .constant("TWEEN_PROCESS_IDLE", 1),
                )
                .enumeration(
                    EnumInfo::new("TweenPauseMode")
                        .constant("TWEEN_PAUSE_BOUND", 0)
                        .constant("TWEEN_PAUSE_STOP", 1)
                        .constant("TWEEN_PAUSE_PROCESS", 2),
                )
                // ⚠️ These are consecutive with NO gaps, and QUINT/QUART sit
                // between SINE and QUAD rather than after CUBIC — copying the
                // "obvious" order gets TRANS_QUAD and TRANS_CUBIC wrong.
                .enumeration(
                    EnumInfo::new("TransitionType")
                        .constant("TRANS_LINEAR", 0)
                        .constant("TRANS_SINE", 1)
                        .constant("TRANS_QUINT", 2)
                        .constant("TRANS_QUART", 3)
                        .constant("TRANS_QUAD", 4)
                        .constant("TRANS_EXPO", 5)
                        .constant("TRANS_ELASTIC", 6)
                        .constant("TRANS_CUBIC", 7)
                        .constant("TRANS_CIRC", 8)
                        .constant("TRANS_BOUNCE", 9)
                        .constant("TRANS_BACK", 10)
                        .constant("TRANS_SPRING", 11),
                )
                .enumeration(
                    EnumInfo::new("EaseType")
                        .constant("EASE_IN", 0)
                        .constant("EASE_OUT", 1)
                        .constant("EASE_IN_OUT", 2)
                        .constant("EASE_OUT_IN", 3),
                ),
        );
    }
    if !class_exists("Tweener") {
        register_class(
            ClassRegistration::new("Tweener")
                .parent("RefCounted")
                .abstract_class()
                .signal(SignalInfo::new("finished")),
        );
    }
    // ⚠️ Each concrete tweener binds a DIFFERENT setter subset — they are not
    // uniform. Only PropertyTweener has `from`/`from_current`/`as_relative`/
    // `set_custom_interpolator`; IntervalTweener declares nothing of its own;
    // MethodTweener has trans/ease but Callback/Subtween have only `set_delay`.
    if !class_exists("PropertyTweener") {
        register_class(
            ClassRegistration::new("PropertyTweener")
                .parent("Tweener")
                .method(MethodInfo::new("from", 1))
                .method(MethodInfo::new("from_current", 0))
                .method(MethodInfo::new("as_relative", 0))
                .method(MethodInfo::new("set_trans", 1))
                .method(MethodInfo::new("set_ease", 1))
                .method(MethodInfo::new("set_custom_interpolator", 1))
                .method(MethodInfo::new("set_delay", 1)),
        );
    }
    if !class_exists("IntervalTweener") {
        register_class(ClassRegistration::new("IntervalTweener").parent("Tweener"));
    }
    if !class_exists("CallbackTweener") {
        register_class(
            ClassRegistration::new("CallbackTweener")
                .parent("Tweener")
                .method(MethodInfo::new("set_delay", 1)),
        );
    }
    if !class_exists("MethodTweener") {
        register_class(
            ClassRegistration::new("MethodTweener")
                .parent("Tweener")
                .method(MethodInfo::new("set_delay", 1))
                .method(MethodInfo::new("set_trans", 1))
                .method(MethodInfo::new("set_ease", 1)),
        );
    }
    if !class_exists("SubtweenTweener") {
        register_class(
            ClassRegistration::new("SubtweenTweener")
                .parent("Tweener")
                .method(MethodInfo::new("set_delay", 1)),
        );
    }

    // AnimationNode* resource hierarchy (pat-nzgdu.95): the graph-node resources
    // an AnimationTree's tree_root is built from. Registering them lets ClassDB
    // resolve `[sub_resource type="AnimationNodeBlendTree"]` graphs from .tscn/.tres.
    register_class(
        ClassRegistration::new("AnimationNode")
            .parent("Resource")
            .abstract_class(),
    );
    register_class(
        ClassRegistration::new("AnimationRootNode")
            .parent("AnimationNode")
            .abstract_class(),
    );
    register_class(ClassRegistration::new("AnimationNodeAnimation").parent("AnimationRootNode"));
    register_class(ClassRegistration::new("AnimationNodeBlendTree").parent("AnimationRootNode"));
    register_class(ClassRegistration::new("AnimationNodeStateMachine").parent("AnimationRootNode"));
    register_class(ClassRegistration::new("AnimationNodeBlendSpace1D").parent("AnimationRootNode"));
    register_class(ClassRegistration::new("AnimationNodeBlendSpace2D").parent("AnimationRootNode"));
    register_class(
        ClassRegistration::new("AnimationNodeSync")
            .parent("AnimationNode")
            .abstract_class(),
    );
    register_class(ClassRegistration::new("AnimationNodeAdd2").parent("AnimationNodeSync"));
    register_class(ClassRegistration::new("AnimationNodeAdd3").parent("AnimationNodeSync"));
    register_class(ClassRegistration::new("AnimationNodeSub2").parent("AnimationNodeSync"));
    register_class(ClassRegistration::new("AnimationNodeBlend2").parent("AnimationNodeSync"));
    register_class(ClassRegistration::new("AnimationNodeBlend3").parent("AnimationNodeSync"));
    register_class(ClassRegistration::new("AnimationNodeOneShot").parent("AnimationNodeSync"));
    register_class(ClassRegistration::new("AnimationNodeTransition").parent("AnimationNodeSync"));
    register_class(ClassRegistration::new("AnimationNodeTimeScale").parent("AnimationNode"));
    register_class(ClassRegistration::new("AnimationNodeTimeSeek").parent("AnimationNode"));
    register_class(ClassRegistration::new("AnimationNodeOutput").parent("AnimationNode"));
    register_class(
        ClassRegistration::new("AnimationNodeStateMachineTransition").parent("Resource"),
    );
    register_class(ClassRegistration::new("AnimationNodeStateMachinePlayback").parent("Resource"));

    register_class(ClassRegistration::new("GPUParticles3D").parent("GeometryInstance3D"));

    // -- GPU particle attractors (pat-nzgdu.144) ---------------------------
    //
    // An attractor applies a per-particle force each frame to particles inside
    // its volume: `strength` scales it (negative repels), `attenuation` shapes
    // the falloff across the volume, `directionality` biases between a radial
    // pull toward the centre and a uniform push along the attractor's -Z, and
    // `cull_mask` selects which emitters it touches. Schemas are diffed against
    // `extension_api.json`.
    //
    // The base is abstract — Godot only instantiates the three shapes, which is
    // what makes the shape property (`size`/`radius`) the volume test.
    register_class(
        ClassRegistration::new("GPUParticlesAttractor3D")
            .parent("VisualInstance3D")
            .abstract_class()
            .property(PropertyInfo::new("strength", Variant::Float(1.0)))
            .property(PropertyInfo::new("attenuation", Variant::Float(1.0)))
            .property(PropertyInfo::new("directionality", Variant::Float(0.0)))
            // A 20-bit render layer mask; every layer on by default.
            .property(PropertyInfo::new("cull_mask", Variant::Int(0xFFFFF))),
    );
    register_class(
        ClassRegistration::new("GPUParticlesAttractorBox3D")
            .parent("GPUParticlesAttractor3D")
            .property(PropertyInfo::new(
                "size",
                Variant::Vector3(gdcore::math::Vector3::new(2.0, 2.0, 2.0)),
            )),
    );
    register_class(
        ClassRegistration::new("GPUParticlesAttractorSphere3D")
            .parent("GPUParticlesAttractor3D")
            .property(PropertyInfo::new("radius", Variant::Float(1.0))),
    );
    register_class(
        ClassRegistration::new("GPUParticlesAttractorVectorField3D")
            .parent("GPUParticlesAttractor3D")
            .property(PropertyInfo::new(
                "size",
                Variant::Vector3(gdcore::math::Vector3::new(2.0, 2.0, 2.0)),
            ))
            // A Texture3D sampled for the force direction; Nil until assigned.
            .property(PropertyInfo::new("texture", Variant::Nil)),
    );
    register_class(ClassRegistration::new("CPUParticles3D").parent("GeometryInstance3D"));
    register_class(ClassRegistration::new("Decal").parent("VisualInstance3D"));
    register_class(ClassRegistration::new("ReflectionProbe").parent("VisualInstance3D"));
    register_class(ClassRegistration::new("VoxelGI").parent("VisualInstance3D"));

    // -- Fog (pat-nzgdu.208) -------------------------------------------------
    //
    // `Material` itself is already registered (abstract) further down this same
    // pass; `inheritance_chain` resolves parents by name at query time, so the
    // fog family reads together here regardless of registration order.
    //
    // ⚠️ FogVolume was registered bare and parented to `Node3D`. Godot has
    // `FogVolume <- VisualInstance3D` (doc/classes/FogVolume.xml), and the
    // difference is load-bearing: `VisualInstance3D` is what carries `layers` /
    // AABB and marks a node as participating in rendering, so a script doing
    // `is_parent_class("FogVolume", "VisualInstance3D")` — or anything walking
    // visual instances — silently missed it.
    if !class_exists("FogVolume") {
        register_class(
            ClassRegistration::new("FogVolume")
                .parent("VisualInstance3D")
                // The Material driving the fog; Nil until assigned.
                .property(PropertyInfo::new("material", Variant::Nil))
                // ⚠️ shape defaults to 3 = BOX, NOT 0/Ellipsoid:
                // `fog_volume.cpp:139` sets FOG_VOLUME_SHAPE_BOX in the
                // constructor. gdserver3d's `FogVolume::default()` agrees — it
                // sets `FogVolumeShape::Box` explicitly rather than taking the
                // enum's zero value (which IS Ellipsoid, a different question).
                // gdscene's `the_node_default_shape_is_box_on_both_sides` pins
                // the two together.
                .property(PropertyInfo::new("shape", Variant::Int(3)))
                .property(PropertyInfo::new(
                    "size",
                    Variant::Vector3(Vector3::new(2.0, 2.0, 2.0)),
                ))
                .method(MethodInfo::new("set_material", 1))
                .method(MethodInfo::new("get_material", 0))
                .method(MethodInfo::new("set_shape", 1))
                .method(MethodInfo::new("get_shape", 0))
                .method(MethodInfo::new("set_size", 1))
                .method(MethodInfo::new("get_size", 0)),
        );
    }
    // The material a FogVolume points at. Every property mirrors
    // `gdserver3d::fog_volume::FogMaterial`, whose defaults already match
    // Godot's `FogMaterial::FogMaterial()` (fog_material.cpp:170-179)
    // field-for-field — this is the ClassDB half that was missing.
    if !class_exists("FogMaterial") {
        register_class(
            ClassRegistration::new("FogMaterial")
                .parent("Material")
                .property(PropertyInfo::new("density", Variant::Float(1.0)))
                .property(PropertyInfo::new(
                    "albedo",
                    Variant::Color(gdcore::math::Color::new(1.0, 1.0, 1.0, 1.0)),
                ))
                // ⚠️ emission is opaque BLACK (0,0,0,1), not transparent
                // (0,0,0,0): fog emits no light by default, but alpha 0 here
                // would be a different colour entirely.
                .property(PropertyInfo::new(
                    "emission",
                    Variant::Color(gdcore::math::Color::new(0.0, 0.0, 0.0, 1.0)),
                ))
                .property(PropertyInfo::new("height_falloff", Variant::Float(0.0)))
                .property(PropertyInfo::new("edge_fade", Variant::Float(0.1)))
                // A Texture3D scaling density; null until assigned.
                .property(PropertyInfo::new("density_texture", Variant::Nil))
                .method(MethodInfo::new("set_density", 1))
                .method(MethodInfo::new("get_density", 0))
                .method(MethodInfo::new("set_albedo", 1))
                .method(MethodInfo::new("get_albedo", 0))
                .method(MethodInfo::new("set_emission", 1))
                .method(MethodInfo::new("get_emission", 0))
                .method(MethodInfo::new("set_height_falloff", 1))
                .method(MethodInfo::new("get_height_falloff", 0))
                .method(MethodInfo::new("set_edge_fade", 1))
                .method(MethodInfo::new("get_edge_fade", 0))
                .method(MethodInfo::new("set_density_texture", 1))
                .method(MethodInfo::new("get_density_texture", 0)),
        );
    }
    // VoxelGIData (pat-nzgdu.216): the baked GI data resource referenced by
    // VoxelGI.data. Bake execution is deferred; registering the resource surface
    // + config properties lets baked-GI .tscn/.res scenes load without dropping
    // the reference (the generic sub_resource loader handles the rest).
    if !class_exists("VoxelGIData") {
        register_class(
            ClassRegistration::new("VoxelGIData")
                .parent("Resource")
                .property(PropertyInfo::new("dynamic_range", Variant::Float(2.0)))
                .property(PropertyInfo::new("energy", Variant::Float(1.0)))
                .property(PropertyInfo::new("bias", Variant::Float(1.5)))
                .property(PropertyInfo::new("normal_bias", Variant::Float(0.0)))
                .property(PropertyInfo::new("propagation", Variant::Float(0.7)))
                .property(PropertyInfo::new("interior", Variant::Bool(false)))
                .method(MethodInfo::new("allocate", 7))
                .method(MethodInfo::new("get_bounds", 0))
                .method(MethodInfo::new("get_octree_size", 0))
                .method(MethodInfo::new("get_octree_cells", 0))
                .method(MethodInfo::new("get_data_cells", 0))
                .method(MethodInfo::new("get_to_cell_xform", 0))
                .method(MethodInfo::new("get_level_counts", 0)),
        );
    }
    register_class(ClassRegistration::new("LightmapGI").parent("VisualInstance3D"));
    register_class(
        ClassRegistration::new("VisualInstance3D")
            .parent("Node3D")
            .abstract_class()
            .property(PropertyInfo::new("layers", Variant::Int(1))),
    );
    // -- Occluders (pat-nzgdu.207) ------------------------------------------
    //
    // The occlusion-culling shapes. gdserver3d/src/occluder.rs already implements
    // all five (and the `occludes()` cull test, with 21 tests) — this is the
    // ClassDB half that was missing.
    //
    // ⚠️ The base is a RESOURCE and is ABSTRACT: an occluder shape is a saved
    // asset shared between instances, not a node. The NODE is
    // OccluderInstance3D, a VisualInstance3D that *holds* one — the same
    // shape/instance split as Mesh vs MeshInstance3D. Conflating them is the
    // trap this family invites.
    if !class_exists("Occluder3D") {
        register_class(
            ClassRegistration::new("Occluder3D")
                .parent("Resource")
                .abstract_class()
                // The baked triangle soup every shape lowers to.
                .method(MethodInfo::new("get_vertices", 0))
                .method(MethodInfo::new("get_indices", 0)),
        );
    }
    if !class_exists("BoxOccluder3D") {
        register_class(
            ClassRegistration::new("BoxOccluder3D")
                .parent("Occluder3D")
                .property(PropertyInfo::new(
                    "size",
                    Variant::Vector3(gdcore::math::Vector3::new(1.0, 1.0, 1.0)),
                ))
                .method(MethodInfo::new("set_size", 1))
                .method(MethodInfo::new("get_size", 0)),
        );
    }
    if !class_exists("SphereOccluder3D") {
        register_class(
            ClassRegistration::new("SphereOccluder3D")
                .parent("Occluder3D")
                .property(PropertyInfo::new("radius", Variant::Float(1.0)))
                .method(MethodInfo::new("set_radius", 1))
                .method(MethodInfo::new("get_radius", 0)),
        );
    }
    // ⚠️ A Vector2 size — it is a flat quad, unlike BoxOccluder3D's Vector3.
    if !class_exists("QuadOccluder3D") {
        register_class(
            ClassRegistration::new("QuadOccluder3D")
                .parent("Occluder3D")
                .property(PropertyInfo::new(
                    "size",
                    Variant::Vector2(gdcore::math::Vector2::new(1.0, 1.0)),
                ))
                .method(MethodInfo::new("set_size", 1))
                .method(MethodInfo::new("get_size", 0)),
        );
    }
    if !class_exists("PolygonOccluder3D") {
        register_class(
            ClassRegistration::new("PolygonOccluder3D")
                .parent("Occluder3D")
                // 2D vertices in the local XY plane.
                .property(PropertyInfo::new(
                    "polygon",
                    Variant::PackedVector2Array(Vec::new()),
                ))
                .method(MethodInfo::new("set_polygon", 1))
                .method(MethodInfo::new("get_polygon", 0)),
        );
    }
    // ⚠️ The only one with TWO properties, and the only one whose setter is
    // plural: `set_arrays(vertices, indices)` sets both at once, alongside the
    // individual setters. An arbitrary triangle mesh needs both halves.
    if !class_exists("ArrayOccluder3D") {
        register_class(
            ClassRegistration::new("ArrayOccluder3D")
                .parent("Occluder3D")
                .property(PropertyInfo::new(
                    "vertices",
                    Variant::PackedVector3Array(Vec::new()),
                ))
                .property(PropertyInfo::new(
                    "indices",
                    Variant::PackedInt32Array(Vec::new()),
                ))
                .method(MethodInfo::new("set_arrays", 2))
                .method(MethodInfo::new("set_vertices", 1))
                .method(MethodInfo::new("set_indices", 1)),
        );
    }
    // The node that puts an occluder in the world.
    if !class_exists("OccluderInstance3D") {
        register_class(
            ClassRegistration::new("OccluderInstance3D")
                .parent("VisualInstance3D")
                .property(PropertyInfo::new("occluder", Variant::Nil))
                // ⚠️ 4294967295 = all 32 layers. A default of 0 or 1 would make
                // baking silently miss geometry.
                .property(PropertyInfo::new("bake_mask", Variant::Int(4_294_967_295)))
                .property(PropertyInfo::new(
                    "bake_simplification_distance",
                    Variant::Float(0.1),
                ))
                .method(MethodInfo::new("set_occluder", 1))
                .method(MethodInfo::new("get_occluder", 0))
                .method(MethodInfo::new("set_bake_mask", 1))
                .method(MethodInfo::new("get_bake_mask", 0))
                .method(MethodInfo::new("set_bake_mask_value", 2))
                .method(MethodInfo::new("get_bake_mask_value", 1))
                .method(MethodInfo::new("set_bake_simplification_distance", 1))
                .method(MethodInfo::new("get_bake_simplification_distance", 0)),
        );
    }

    // -- RootMotionView (pat-nzgdu.184) ------------------------------------
    //
    // An editor-only gizmo: a grid that scrolls under the character by the root
    // motion an AnimationMixer has accumulated, so you can see the motion an
    // animation is *driving* rather than guessing from a sliding mesh. It is a
    // VisualInstance3D because it draws, not a Node3D that merely has a
    // transform.
    //
    // NOTE the bead's description also lists a `use_custom_color` property.
    // Godot 4.6.1 has NO such property — RootMotionView exposes exactly the five
    // below. Registering it would invent schema.
    register_class(
        ClassRegistration::new("RootMotionView")
            .parent("VisualInstance3D")
            // Points at the AnimationMixer whose root motion drives the grid.
            // Empty by default: the gizmo tracks nothing until wired up.
            .property(PropertyInfo::new(
                "animation_path",
                Variant::NodePath(gdcore::node_path::NodePath::new("")),
            ))
            .property(PropertyInfo::new("cell_size", Variant::Float(1.0)))
            .property(PropertyInfo::new("radius", Variant::Float(10.0)))
            // Godot's default is a pale blue, not white — the grid has to read
            // as a gizmo against the scene rather than as geometry.
            .property(PropertyInfo::new(
                "color",
                Variant::Color(gdcore::math::Color::new(0.5, 0.5, 1.0, 1.0)),
            ))
            // True by default: the grid stays flat on XZ, so vertical root
            // motion (a jump) does not carry the ground plane up with it.
            .property(PropertyInfo::new("zero_y", Variant::Bool(true)))
            .method(MethodInfo::new("set_animation_path", 1))
            .method(MethodInfo::new("get_animation_path", 0))
            .method(MethodInfo::new("set_color", 1))
            .method(MethodInfo::new("get_color", 0))
            .method(MethodInfo::new("set_cell_size", 1))
            .method(MethodInfo::new("get_cell_size", 0))
            .method(MethodInfo::new("set_radius", 1))
            .method(MethodInfo::new("get_radius", 0))
            .method(MethodInfo::new("set_zero_y", 1))
            // NOT `is_zero_y` — Godot spells this getter `get_zero_y` despite it
            // being a bool, breaking its own `is_*` idiom.
            .method(MethodInfo::new("get_zero_y", 0)),
    );

    // -- GeometryInstance3D (abstract base for drawable 3D instances:
    //    MeshInstance3D, MultiMeshInstance3D, GPUParticles3D, …). Carries the
    //    material_override/overlay, transparency, cast_shadow, GI, cull, LOD,
    //    visibility-range and custom-AABB surface. --
    register_class(
        ClassRegistration::new("GeometryInstance3D")
            .parent("VisualInstance3D")
            .abstract_class()
            .property(PropertyInfo::new("material_override", Variant::Nil))
            .property(PropertyInfo::new("material_overlay", Variant::Nil))
            .property(PropertyInfo::new("transparency", Variant::Float(0.0)))
            .property(PropertyInfo::new("cast_shadow", Variant::Int(1)))
            .property(PropertyInfo::new("extra_cull_margin", Variant::Float(0.0)))
            .property(PropertyInfo::new("lod_bias", Variant::Float(1.0)))
            .property(PropertyInfo::new("gi_mode", Variant::Int(1)))
            .property(PropertyInfo::new(
                "ignore_occlusion_culling",
                Variant::Bool(false),
            ))
            .property(PropertyInfo::new(
                "visibility_range_begin",
                Variant::Float(0.0),
            ))
            .property(PropertyInfo::new(
                "visibility_range_begin_margin",
                Variant::Float(0.0),
            ))
            .property(PropertyInfo::new(
                "visibility_range_end",
                Variant::Float(0.0),
            ))
            .property(PropertyInfo::new(
                "visibility_range_end_margin",
                Variant::Float(0.0),
            ))
            .property(PropertyInfo::new(
                "visibility_range_fade_mode",
                Variant::Int(0),
            ))
            .property(PropertyInfo::new("custom_aabb", Variant::Nil)),
    );
    register_class(
        ClassRegistration::new("CSGShape3D")
            .parent("Node3D")
            .abstract_class(),
    );
    register_class(ClassRegistration::new("CSGBox3D").parent("CSGShape3D"));
    register_class(ClassRegistration::new("CSGSphere3D").parent("CSGShape3D"));
    register_class(ClassRegistration::new("CSGCylinder3D").parent("CSGShape3D"));
    register_class(ClassRegistration::new("CSGCombiner3D").parent("CSGShape3D"));
    // VisualScript is deprecated but inherits Script → Resource in Godot's hierarchy.
    //
    // `Resource` is a `RefCounted` (core/io/resource.h: `class Resource : public
    // RefCounted`), so `RefCounted` must exist or the chain from every resource
    // type — Material, Mesh, Texture, … — stops dead at `Resource` and never
    // reaches `Object` (pat-nzgdu.98). This function bootstraps neither Object's
    // ref-counted half nor any other `register_*_classes`, so it guards here.
    if !class_exists("RefCounted") {
        register_class(ClassRegistration::new("RefCounted").parent("Object"));
    }
    if !class_exists("Resource") {
        register_class(ClassRegistration::new("Resource").parent("RefCounted"));
    }
    register_class(ClassRegistration::new("VisualScript").parent("Resource"));

    // Material / Shader hierarchy (Godot: Material → Resource; ShaderMaterial →
    // Material; BaseMaterial3D → Material; StandardMaterial3D → BaseMaterial3D).
    // ShaderMaterial exposes the `shader` property plus set/get_shader and the
    // per-parameter set/get_shader_parameter methods that GDScript calls.
    // Guarded (pat-nzgdu.208): `register()` mints a fresh ClassId on every call
    // and overwrites `classes[name]`, leaving a stale `by_id` alias behind — so
    // an unguarded registration is not idempotent, and `register_3d_classes()`
    // is called more than once in practice. `Material` is virtual in Godot:
    // `GDREGISTER_VIRTUAL_CLASS(Material)` (register_scene_types.cpp:439).
    if !class_exists("Material") {
        register_class(
            ClassRegistration::new("Material")
                .parent("Resource")
                .abstract_class(),
        );
    }
    // ProceduralSkyMaterial → Material (pat-nzgdu.161): the sky shader material
    // renders (gdserver3d::sky) but was invisible to ClassDB. Register it with
    // its properties + defaults (mirroring `ProceduralSkyMaterial::default()`)
    // so it is instantiable and introspectable like any Material resource.
    register_class(
        ClassRegistration::new("ProceduralSkyMaterial")
            .parent("Material")
            .property(PropertyInfo::new(
                "sky_top_color",
                Variant::Color(gdcore::math::Color::new(0.385, 0.454, 0.55, 1.0)),
            ))
            .property(PropertyInfo::new(
                "sky_horizon_color",
                Variant::Color(gdcore::math::Color::new(0.6463, 0.6558, 0.6708, 1.0)),
            ))
            .property(PropertyInfo::new("sky_curve", Variant::Float(0.15)))
            .property(PropertyInfo::new(
                "sky_energy_multiplier",
                Variant::Float(1.0),
            ))
            .property(PropertyInfo::new(
                "ground_bottom_color",
                Variant::Color(gdcore::math::Color::new(0.2, 0.169, 0.133, 1.0)),
            ))
            .property(PropertyInfo::new(
                "ground_horizon_color",
                Variant::Color(gdcore::math::Color::new(0.6463, 0.6558, 0.6708, 1.0)),
            ))
            .property(PropertyInfo::new("ground_curve", Variant::Float(0.02)))
            .property(PropertyInfo::new(
                "ground_energy_multiplier",
                Variant::Float(1.0),
            ))
            .property(PropertyInfo::new("sun_angle_max", Variant::Float(30.0)))
            .property(PropertyInfo::new("sun_curve", Variant::Float(0.15)))
            .property(PropertyInfo::new(
                "sky_cover_modulate",
                Variant::Color(gdcore::math::Color::new(1.0, 1.0, 1.0, 1.0)),
            )),
    );
    // PrimitiveMesh family (pat-nzgdu.160): these generate geometry
    // (gdserver3d::primitive_mesh) but were absent from ClassDB. Register the
    // Mesh -> PrimitiveMesh -> <shape> hierarchy so they are creatable and
    // introspectable. CapsuleMesh carries Godot's defaults.
    if !class_exists("Mesh") {
        register_class(
            ClassRegistration::new("Mesh")
                .parent("Resource")
                .abstract_class(),
        );
    }

    // -- Placeholder mesh/material (pat-nzgdu.221) ---------------------------
    //
    // The other two of the eight placeholders. They live here rather than beside
    // the placeholder textures because their parents do: `Material` and `Mesh`
    // are registered in this pass, and parenting to a class another pass owns
    // severs the chain when this one runs alone (pat-nzgdu.98).
    //
    // ⚠️ `PlaceholderMesh` binds `set_aabb` and NOT `get_aabb` — the getter is
    // `Mesh`'s. And its member is an `aabb`, not a size or a surface count: the
    // bead's "surface-count getter" does not exist on it.
    if !class_exists("PlaceholderMesh") {
        register_class(
            ClassRegistration::new("PlaceholderMesh")
                .parent("Mesh")
                .property(PropertyInfo::new(
                    "aabb",
                    Variant::Aabb(gdcore::math3d::Aabb::new(Vector3::ZERO, Vector3::ZERO)),
                ))
                .method(MethodInfo::new("set_aabb", 1)),
        );
    }
    // ⚠️ Binds NOTHING — no methods, no properties. A placeholder material is a
    // pure marker: it draws nothing and has nothing to configure.
    if !class_exists("PlaceholderMaterial") {
        register_class(ClassRegistration::new("PlaceholderMaterial").parent("Material"));
    }

    register_class(
        ClassRegistration::new("PrimitiveMesh")
            .parent("Mesh")
            .abstract_class(),
    );
    register_class(
        ClassRegistration::new("CapsuleMesh")
            .parent("PrimitiveMesh")
            .property(PropertyInfo::new("radius", Variant::Float(0.5)))
            .property(PropertyInfo::new("height", Variant::Float(2.0)))
            .property(PropertyInfo::new("radial_segments", Variant::Int(64)))
            .property(PropertyInfo::new("rings", Variant::Int(8))),
    );
    register_class(
        ClassRegistration::new("BoxMesh")
            .parent("PrimitiveMesh")
            // BoxMesh.size default matches gdserver3d `BoxMesh::default()` so
            // instantiate('BoxMesh') exposes the size resource property (pat-nzgdu.205).
            .property(PropertyInfo::new(
                "size",
                Variant::Vector3(Vector3::new(1.0, 1.0, 1.0)),
            )),
    );
    register_class(ClassRegistration::new("SphereMesh").parent("PrimitiveMesh"));
    register_class(ClassRegistration::new("CylinderMesh").parent("PrimitiveMesh"));
    register_class(ClassRegistration::new("PlaneMesh").parent("PrimitiveMesh"));
    register_class(ClassRegistration::new("PrismMesh").parent("PrimitiveMesh"));
    register_class(ClassRegistration::new("ArrayMesh").parent("Mesh"));
    register_class(
        ClassRegistration::new("Shader")
            .parent("Resource")
            .method(MethodInfo::new("get_mode", 0))
            .method(MethodInfo::new("set_code", 1))
            .method(MethodInfo::new("get_code", 0))
            .method(MethodInfo::new("set_default_texture_parameter", 2))
            .method(MethodInfo::new("get_default_texture_parameter", 1)),
    );
    register_class(
        ClassRegistration::new("ShaderMaterial")
            .parent("Material")
            .method(MethodInfo::new("set_shader", 1))
            .method(MethodInfo::new("get_shader", 0))
            .method(MethodInfo::new("set_shader_parameter", 2))
            .method(MethodInfo::new("get_shader_parameter", 1)),
    );
    // A deferred sub-scene stand-in (pat-0q2kx.139). `instance_placeholder=`
    // nodes loaded as plain `Node`s because this class did not exist, so
    // `create_instance()` had nothing to dispatch on.
    if !class_exists("InstancePlaceholder") {
        register_class(
            ClassRegistration::new("InstancePlaceholder")
                .parent("Node")
                .method(MethodInfo::new("get_stored_values", 0))
                .method(MethodInfo::new("get_instance_path", 0))
                .method(MethodInfo::new("create_instance", 2)),
        );
    }
    // The 2D counterpart (pat-0q2kx.131). Only 3D materials were registered, so
    // `CanvasItemMaterial` — the resource that decides how a 2D item composites
    // — could not be instantiated or inspected at all. Defaults are Godot's:
    // BLEND_MODE_MIX (0), LIGHT_MODE_NORMAL (0), particles animation off.
    register_class(
        ClassRegistration::new("CanvasItemMaterial")
            .parent("Material")
            .property(PropertyInfo::new("blend_mode", Variant::Int(0)))
            .property(PropertyInfo::new("light_mode", Variant::Int(0)))
            .property(PropertyInfo::new(
                "particles_animation",
                Variant::Bool(false),
            ))
            .property(PropertyInfo::new(
                "particles_anim_h_frames",
                Variant::Int(1),
            ))
            .property(PropertyInfo::new(
                "particles_anim_v_frames",
                Variant::Int(1),
            ))
            .property(PropertyInfo::new(
                "particles_anim_loop",
                Variant::Bool(false),
            )),
    );
    // ShaderInclude → Resource (reusable `.gdshaderinc` fragments); Godot's
    // ShaderGlobalsOverride is a Node that overrides project shader globals.
    register_class(ClassRegistration::new("ShaderInclude").parent("Resource"));
    register_class(ClassRegistration::new("ShaderGlobalsOverride").parent("Node"));

    // -- VisualShader node-graph family (pat-nzgdu.140) --------------------
    // VisualShader IS a Shader: its graph compiles to shader source
    // (gdserver2d::material::VisualShader::get_code), so it is usable anywhere
    // a text Shader is. Godot ships 111 VisualShaderNode* classes; this is the
    // core input/output/constant/op/func/parameter/texture set the graph
    // compiler can actually emit code for — the fuller set is additive.
    register_class(
        ClassRegistration::new("VisualShader")
            .parent("Shader")
            .method(MethodInfo::new("add_node", 3))
            .method(MethodInfo::new("remove_node", 2))
            .method(MethodInfo::new("get_node", 2))
            .method(MethodInfo::new("get_node_list", 1))
            .method(MethodInfo::new("connect_nodes", 5))
            .method(MethodInfo::new("disconnect_nodes", 5))
            .method(MethodInfo::new("is_node_connection", 5))
            .method(MethodInfo::new("set_mode", 1))
            .enumeration(
                EnumInfo::new("Type")
                    .constant("TYPE_VERTEX", 0)
                    .constant("TYPE_FRAGMENT", 1)
                    .constant("TYPE_LIGHT", 2)
                    .constant("TYPE_START", 3)
                    .constant("TYPE_PROCESS", 4)
                    .constant("TYPE_COLLIDE", 5)
                    .constant("TYPE_START_CUSTOM", 6)
                    .constant("TYPE_PROCESS_CUSTOM", 7)
                    .constant("TYPE_SKY", 8)
                    .constant("TYPE_FOG", 9)
                    .constant("TYPE_MAX", 10),
            ),
    );
    // The graph node base. Every VisualShaderNode* derives from it.
    register_class(
        ClassRegistration::new("VisualShaderNode")
            .parent("Resource")
            .abstract_class()
            // ⚠️ -1, not 0 (pat-nzgdu.227): both are "none" sentinels. Port 0 and
            // frame 0 are real, so a 0 default would silently mean "preview port
            // 0" / "linked to frame 0".
            .property(PropertyInfo::new(
                "output_port_for_preview",
                Variant::Int(-1),
            ))
            .property(PropertyInfo::new(
                "linked_parent_graph_frame",
                Variant::Int(-1),
            ))
            .property(PropertyInfo::new(
                "default_input_values",
                Variant::Array(Vec::new()),
            ))
            .property(PropertyInfo::new(
                "expanded_output_ports",
                Variant::Array(Vec::new()),
            ))
            .method(MethodInfo::new("get_default_input_port", 1))
            .method(MethodInfo::new("set_output_port_for_preview", 1))
            .method(MethodInfo::new("get_output_port_for_preview", 0))
            // ⚠️ THREE arguments (port, index, value) — this was registered with
            // two (pat-nzgdu.227).
            .method(MethodInfo::new("set_input_port_default_value", 3))
            .method(MethodInfo::new("get_input_port_default_value", 1))
            .method(MethodInfo::new("remove_input_port_default_value", 1))
            .method(MethodInfo::new("clear_default_input_values", 0))
            .method(MethodInfo::new("set_default_input_values", 1))
            .method(MethodInfo::new("get_default_input_values", 0))
            .method(MethodInfo::new("set_frame", 1))
            .method(MethodInfo::new("get_frame", 0))
            // ⚠️ THE VALUES WERE WRONG (pat-nzgdu.227). VECTOR_3D onward were
            // 5/7/8/9/10/12 where Godot has 4/5/6/7/8/9 — the enum is
            // consecutive, with no gaps. A script reading
            // `PORT_TYPE_BOOLEAN` got 8 where Godot says 6, so every port-type
            // comparison past VECTOR_2D silently disagreed with the engine.
            .enumeration(
                EnumInfo::new("PortType")
                    .constant("PORT_TYPE_SCALAR", 0)
                    .constant("PORT_TYPE_SCALAR_INT", 1)
                    .constant("PORT_TYPE_SCALAR_UINT", 2)
                    .constant("PORT_TYPE_VECTOR_2D", 3)
                    .constant("PORT_TYPE_VECTOR_3D", 4)
                    .constant("PORT_TYPE_VECTOR_4D", 5)
                    .constant("PORT_TYPE_BOOLEAN", 6)
                    .constant("PORT_TYPE_TRANSFORM", 7)
                    .constant("PORT_TYPE_SAMPLER", 8)
                    .constant("PORT_TYPE_MAX", 9),
            ),
    );
    // Graph entry/exit.
    register_class(ClassRegistration::new("VisualShaderNodeInput").parent("VisualShaderNode"));
    register_class(ClassRegistration::new("VisualShaderNodeOutput").parent("VisualShaderNode"));

    // -- Particle graph nodes (pat-nzgdu.178) ------------------------------
    //
    // The `shader_type particles` vocabulary: emitters (box/sphere/ring/mesh),
    // cone velocity, accelerator, randomness, multiply-by-axis-angle, emit, and
    // the particle output. Generated from `extension_api.json`.
    //
    // Two shapes worth noting: `VisualShaderNodeParticleEmitter` is an abstract
    // base the four emitters share (it carries `mode_2d`), and
    // `VisualShaderNodeParticleOutput` derives **VisualShaderNodeOutput** — not
    // `VisualShaderNode` like its siblings — because it *is* the graph's output
    // node for the particles shader type.
    register_class(
        ClassRegistration::new("VisualShaderNodeParticleAccelerator")
            .parent("VisualShaderNode")
            .property(PropertyInfo::new("mode", Variant::Int(0)))
            .enumeration(
                EnumInfo::new("Mode")
                    .constant("MODE_LINEAR", 0)
                    .constant("MODE_RADIAL", 1)
                    .constant("MODE_TANGENTIAL", 2)
                    .constant("MODE_MAX", 3),
            ),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeParticleBoxEmitter")
            .parent("VisualShaderNodeParticleEmitter"),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeParticleConeVelocity").parent("VisualShaderNode"),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeParticleEmit")
            .parent("VisualShaderNode")
            .property(PropertyInfo::new("flags", Variant::Int(0)))
            .enumeration(
                EnumInfo::new("EmitFlags")
                    .constant("EMIT_FLAG_POSITION", 1)
                    .constant("EMIT_FLAG_ROT_SCALE", 2)
                    .constant("EMIT_FLAG_VELOCITY", 4)
                    .constant("EMIT_FLAG_COLOR", 8)
                    .constant("EMIT_FLAG_CUSTOM", 16),
            ),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeParticleEmitter")
            .parent("VisualShaderNode")
            .abstract_class()
            .property(PropertyInfo::new("mode_2d", Variant::Bool(false))),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeParticleMeshEmitter")
            .parent("VisualShaderNodeParticleEmitter")
            .property(PropertyInfo::new("mesh", Variant::Nil))
            .property(PropertyInfo::new("use_all_surfaces", Variant::Bool(false)))
            .property(PropertyInfo::new("surface_index", Variant::Int(0))),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeParticleMultiplyByAxisAngle")
            .parent("VisualShaderNode")
            .property(PropertyInfo::new("degrees_mode", Variant::Bool(false))),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeParticleOutput").parent("VisualShaderNodeOutput"),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeParticleRandomness")
            .parent("VisualShaderNode")
            .property(PropertyInfo::new("op_type", Variant::Int(0)))
            .enumeration(
                EnumInfo::new("OpType")
                    .constant("OP_TYPE_SCALAR", 0)
                    .constant("OP_TYPE_VECTOR_2D", 1)
                    .constant("OP_TYPE_VECTOR_3D", 2)
                    .constant("OP_TYPE_VECTOR_4D", 3)
                    .constant("OP_TYPE_MAX", 4),
            ),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeParticleRingEmitter")
            .parent("VisualShaderNodeParticleEmitter"),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeParticleSphereEmitter")
            .parent("VisualShaderNodeParticleEmitter"),
    );
    // Constants: the graph compiler emits these as literals.
    register_class(
        ClassRegistration::new("VisualShaderNodeConstant")
            .parent("VisualShaderNode")
            .abstract_class(),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeFloatConstant")
            .parent("VisualShaderNodeConstant")
            .property(PropertyInfo::new("constant", Variant::Float(0.0))),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeIntConstant")
            .parent("VisualShaderNodeConstant")
            .property(PropertyInfo::new("constant", Variant::Int(0))),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeBooleanConstant")
            .parent("VisualShaderNodeConstant")
            .property(PropertyInfo::new("constant", Variant::Bool(false))),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeColorConstant")
            .parent("VisualShaderNodeConstant")
            .property(PropertyInfo::new(
                "constant",
                Variant::Color(gdcore::math::Color::WHITE),
            )),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeVec3Constant")
            .parent("VisualShaderNodeConstant")
            .property(PropertyInfo::new(
                "constant",
                Variant::Vector3(gdcore::math::Vector3::ZERO),
            )),
    );
    // Math operators and functions (pat-nzgdu.169).
    //
    // The workhorse nodes of every graph. Schemas and every enum value are
    // generated from Godot's `extension_api.json`, not recalled — these carry
    // 171 constants and a transposed one would silently mis-compile a graph.
    //
    // Note `VectorOp`/`VectorFunc`/`FaceForward` derive `VisualShaderNodeVectorBase`
    // (which carries the vec2/vec3/vec4 `op_type`), NOT `VisualShaderNode`
    // directly — the earlier registration got that wrong and the API diff caught it.
    //
    // Selector enums only: `generate_code()` does not yet consult them — see the
    // bead notes (gdserver2d::material's graph compiler has no operator/function
    // node kinds at all).
    register_class(
        ClassRegistration::new("VisualShaderNodeVectorBase")
            .parent("VisualShaderNode")
            .abstract_class()
            .property(PropertyInfo::new("op_type", Variant::Int(0)))
            // The accessor pair (pat-nzgdu.227). The property and enum were
            // registered without them, so `op_type` was introspectable but had no
            // methods — the same "properties but no methods" gap pat-nzgdu.217
            // found on GLTFDocument.
            .method(MethodInfo::new("set_op_type", 1))
            .method(MethodInfo::new("get_op_type", 0))
            .enumeration(
                EnumInfo::new("OpType")
                    .constant("OP_TYPE_VECTOR_2D", 0)
                    .constant("OP_TYPE_VECTOR_3D", 1)
                    .constant("OP_TYPE_VECTOR_4D", 2)
                    .constant("OP_TYPE_MAX", 3),
            ),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeFloatOp")
            .parent("VisualShaderNode")
            .property(PropertyInfo::new("operator", Variant::Int(0)))
            .enumeration(
                EnumInfo::new("Operator")
                    .constant("OP_ADD", 0)
                    .constant("OP_SUB", 1)
                    .constant("OP_MUL", 2)
                    .constant("OP_DIV", 3)
                    .constant("OP_MOD", 4)
                    .constant("OP_POW", 5)
                    .constant("OP_MAX", 6)
                    .constant("OP_MIN", 7)
                    .constant("OP_ATAN2", 8)
                    .constant("OP_STEP", 9)
                    .constant("OP_ENUM_SIZE", 10),
            ),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeIntOp")
            .parent("VisualShaderNode")
            .property(PropertyInfo::new("operator", Variant::Int(0)))
            .enumeration(
                EnumInfo::new("Operator")
                    .constant("OP_ADD", 0)
                    .constant("OP_SUB", 1)
                    .constant("OP_MUL", 2)
                    .constant("OP_DIV", 3)
                    .constant("OP_MOD", 4)
                    .constant("OP_MAX", 5)
                    .constant("OP_MIN", 6)
                    .constant("OP_BITWISE_AND", 7)
                    .constant("OP_BITWISE_OR", 8)
                    .constant("OP_BITWISE_XOR", 9)
                    .constant("OP_BITWISE_LEFT_SHIFT", 10)
                    .constant("OP_BITWISE_RIGHT_SHIFT", 11)
                    .constant("OP_ENUM_SIZE", 12),
            ),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeColorOp")
            .parent("VisualShaderNode")
            .property(PropertyInfo::new("operator", Variant::Int(0)))
            .enumeration(
                EnumInfo::new("Operator")
                    .constant("OP_SCREEN", 0)
                    .constant("OP_DIFFERENCE", 1)
                    .constant("OP_DARKEN", 2)
                    .constant("OP_LIGHTEN", 3)
                    .constant("OP_OVERLAY", 4)
                    .constant("OP_DODGE", 5)
                    .constant("OP_BURN", 6)
                    .constant("OP_SOFT_LIGHT", 7)
                    .constant("OP_HARD_LIGHT", 8)
                    .constant("OP_MAX", 9),
            ),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeVectorOp")
            .parent("VisualShaderNodeVectorBase")
            .property(PropertyInfo::new("operator", Variant::Int(0)))
            // The accessor pair (pat-nzgdu.227) — see VisualShaderNodeVectorBase.
            .method(MethodInfo::new("set_operator", 1))
            .method(MethodInfo::new("get_operator", 0))
            .enumeration(
                EnumInfo::new("Operator")
                    .constant("OP_ADD", 0)
                    .constant("OP_SUB", 1)
                    .constant("OP_MUL", 2)
                    .constant("OP_DIV", 3)
                    .constant("OP_MOD", 4)
                    .constant("OP_POW", 5)
                    .constant("OP_MAX", 6)
                    .constant("OP_MIN", 7)
                    .constant("OP_CROSS", 8)
                    .constant("OP_ATAN2", 9)
                    .constant("OP_REFLECT", 10)
                    .constant("OP_STEP", 11)
                    .constant("OP_ENUM_SIZE", 12),
            ),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeFloatFunc")
            .parent("VisualShaderNode")
            .property(PropertyInfo::new("function", Variant::Int(0)))
            .enumeration(
                EnumInfo::new("Function")
                    .constant("FUNC_SIN", 0)
                    .constant("FUNC_COS", 1)
                    .constant("FUNC_TAN", 2)
                    .constant("FUNC_ASIN", 3)
                    .constant("FUNC_ACOS", 4)
                    .constant("FUNC_ATAN", 5)
                    .constant("FUNC_SINH", 6)
                    .constant("FUNC_COSH", 7)
                    .constant("FUNC_TANH", 8)
                    .constant("FUNC_LOG", 9)
                    .constant("FUNC_EXP", 10)
                    .constant("FUNC_SQRT", 11)
                    .constant("FUNC_ABS", 12)
                    .constant("FUNC_SIGN", 13)
                    .constant("FUNC_FLOOR", 14)
                    .constant("FUNC_ROUND", 15)
                    .constant("FUNC_CEIL", 16)
                    .constant("FUNC_FRACT", 17)
                    .constant("FUNC_SATURATE", 18)
                    .constant("FUNC_NEGATE", 19)
                    .constant("FUNC_ACOSH", 20)
                    .constant("FUNC_ASINH", 21)
                    .constant("FUNC_ATANH", 22)
                    .constant("FUNC_DEGREES", 23)
                    .constant("FUNC_EXP2", 24)
                    .constant("FUNC_INVERSE_SQRT", 25)
                    .constant("FUNC_LOG2", 26)
                    .constant("FUNC_RADIANS", 27)
                    .constant("FUNC_RECIPROCAL", 28)
                    .constant("FUNC_ROUNDEVEN", 29)
                    .constant("FUNC_TRUNC", 30)
                    .constant("FUNC_ONEMINUS", 31)
                    .constant("FUNC_MAX", 32),
            ),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeIntFunc")
            .parent("VisualShaderNode")
            .property(PropertyInfo::new("function", Variant::Int(0)))
            .enumeration(
                EnumInfo::new("Function")
                    .constant("FUNC_ABS", 0)
                    .constant("FUNC_NEGATE", 1)
                    .constant("FUNC_SIGN", 2)
                    .constant("FUNC_BITWISE_NOT", 3)
                    .constant("FUNC_MAX", 4),
            ),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeVectorFunc")
            .parent("VisualShaderNodeVectorBase")
            .property(PropertyInfo::new("function", Variant::Int(0)))
            .enumeration(
                EnumInfo::new("Function")
                    .constant("FUNC_NORMALIZE", 0)
                    .constant("FUNC_SATURATE", 1)
                    .constant("FUNC_NEGATE", 2)
                    .constant("FUNC_RECIPROCAL", 3)
                    .constant("FUNC_ABS", 4)
                    .constant("FUNC_ACOS", 5)
                    .constant("FUNC_ACOSH", 6)
                    .constant("FUNC_ASIN", 7)
                    .constant("FUNC_ASINH", 8)
                    .constant("FUNC_ATAN", 9)
                    .constant("FUNC_ATANH", 10)
                    .constant("FUNC_CEIL", 11)
                    .constant("FUNC_COS", 12)
                    .constant("FUNC_COSH", 13)
                    .constant("FUNC_DEGREES", 14)
                    .constant("FUNC_EXP", 15)
                    .constant("FUNC_EXP2", 16)
                    .constant("FUNC_FLOOR", 17)
                    .constant("FUNC_FRACT", 18)
                    .constant("FUNC_INVERSE_SQRT", 19)
                    .constant("FUNC_LOG", 20)
                    .constant("FUNC_LOG2", 21)
                    .constant("FUNC_RADIANS", 22)
                    .constant("FUNC_ROUND", 23)
                    .constant("FUNC_ROUNDEVEN", 24)
                    .constant("FUNC_SIGN", 25)
                    .constant("FUNC_SIN", 26)
                    .constant("FUNC_SINH", 27)
                    .constant("FUNC_SQRT", 28)
                    .constant("FUNC_TAN", 29)
                    .constant("FUNC_TANH", 30)
                    .constant("FUNC_TRUNC", 31)
                    .constant("FUNC_ONEMINUS", 32)
                    .constant("FUNC_MAX", 33),
            ),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeMix")
            .parent("VisualShaderNode")
            .property(PropertyInfo::new("op_type", Variant::Int(0)))
            .enumeration(
                EnumInfo::new("OpType")
                    .constant("OP_TYPE_SCALAR", 0)
                    .constant("OP_TYPE_VECTOR_2D", 1)
                    .constant("OP_TYPE_VECTOR_2D_SCALAR", 2)
                    .constant("OP_TYPE_VECTOR_3D", 3)
                    .constant("OP_TYPE_VECTOR_3D_SCALAR", 4)
                    .constant("OP_TYPE_VECTOR_4D", 5)
                    .constant("OP_TYPE_VECTOR_4D_SCALAR", 6)
                    .constant("OP_TYPE_MAX", 7),
            ),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeMultiplyAdd")
            .parent("VisualShaderNode")
            .property(PropertyInfo::new("op_type", Variant::Int(0)))
            .enumeration(
                EnumInfo::new("OpType")
                    .constant("OP_TYPE_SCALAR", 0)
                    .constant("OP_TYPE_VECTOR_2D", 1)
                    .constant("OP_TYPE_VECTOR_3D", 2)
                    .constant("OP_TYPE_VECTOR_4D", 3)
                    .constant("OP_TYPE_MAX", 4),
            ),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeCompare")
            .parent("VisualShaderNode")
            .property(PropertyInfo::new("type", Variant::Int(0)))
            .property(PropertyInfo::new("function", Variant::Int(0)))
            .property(PropertyInfo::new("condition", Variant::Int(0)))
            .enumeration(
                EnumInfo::new("ComparisonType")
                    .constant("CTYPE_SCALAR", 0)
                    .constant("CTYPE_SCALAR_INT", 1)
                    .constant("CTYPE_SCALAR_UINT", 2)
                    .constant("CTYPE_VECTOR_2D", 3)
                    .constant("CTYPE_VECTOR_3D", 4)
                    .constant("CTYPE_VECTOR_4D", 5)
                    .constant("CTYPE_BOOLEAN", 6)
                    .constant("CTYPE_TRANSFORM", 7)
                    .constant("CTYPE_MAX", 8),
            )
            .enumeration(
                EnumInfo::new("Function")
                    .constant("FUNC_EQUAL", 0)
                    .constant("FUNC_NOT_EQUAL", 1)
                    .constant("FUNC_GREATER_THAN", 2)
                    .constant("FUNC_GREATER_THAN_EQUAL", 3)
                    .constant("FUNC_LESS_THAN", 4)
                    .constant("FUNC_LESS_THAN_EQUAL", 5)
                    .constant("FUNC_MAX", 6),
            )
            .enumeration(
                EnumInfo::new("Condition")
                    .constant("COND_ALL", 0)
                    .constant("COND_ANY", 1)
                    .constant("COND_MAX", 2),
            ),
    );
    register_class(ClassRegistration::new("VisualShaderNodeIf").parent("VisualShaderNode"));
    register_class(
        ClassRegistration::new("VisualShaderNodeIs")
            .parent("VisualShaderNode")
            .property(PropertyInfo::new("function", Variant::Int(0)))
            .enumeration(
                EnumInfo::new("Function")
                    .constant("FUNC_IS_INF", 0)
                    .constant("FUNC_IS_NAN", 1)
                    .constant("FUNC_MAX", 2),
            ),
    );
    register_class(ClassRegistration::new("VisualShaderNodeFresnel").parent("VisualShaderNode"));
    register_class(ClassRegistration::new("VisualShaderNodeDotProduct").parent("VisualShaderNode"));
    register_class(
        ClassRegistration::new("VisualShaderNodeDeterminant").parent("VisualShaderNode"),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeOuterProduct").parent("VisualShaderNode"),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeDerivativeFunc")
            .parent("VisualShaderNode")
            .property(PropertyInfo::new("op_type", Variant::Int(0)))
            .property(PropertyInfo::new("function", Variant::Int(0)))
            .property(PropertyInfo::new("precision", Variant::Int(0)))
            .enumeration(
                EnumInfo::new("OpType")
                    .constant("OP_TYPE_SCALAR", 0)
                    .constant("OP_TYPE_VECTOR_2D", 1)
                    .constant("OP_TYPE_VECTOR_3D", 2)
                    .constant("OP_TYPE_VECTOR_4D", 3)
                    .constant("OP_TYPE_MAX", 4),
            )
            .enumeration(
                EnumInfo::new("Function")
                    .constant("FUNC_SUM", 0)
                    .constant("FUNC_X", 1)
                    .constant("FUNC_Y", 2)
                    .constant("FUNC_MAX", 3),
            )
            .enumeration(
                EnumInfo::new("Precision")
                    .constant("PRECISION_NONE", 0)
                    .constant("PRECISION_COARSE", 1)
                    .constant("PRECISION_FINE", 2)
                    .constant("PRECISION_MAX", 3),
            ),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeFaceForward").parent("VisualShaderNodeVectorBase"),
    );
    // Parameters (uniforms) and textures (pat-nzgdu.168).
    //
    // Schemas below are diffed against Godot's `extension_api.json` rather than
    // recalled — property names, types, and every enum value match 4.x exactly.
    // A `VisualShaderNode*Parameter` becomes a `uniform` declaration in the
    // generated shader; the node's `parameter_name` is the uniform's name.
    register_class(
        ClassRegistration::new("VisualShaderNodeParameter")
            .parent("VisualShaderNode")
            .abstract_class()
            // Godot types this StringName; we carry it as a String.
            .property(PropertyInfo::new(
                "parameter_name",
                Variant::String(String::new()),
            ))
            .property(PropertyInfo::new("qualifier", Variant::Int(0)))
            .property(PropertyInfo::new("instance_index", Variant::Int(0)))
            .enumeration(
                EnumInfo::new("Qualifier")
                    .constant("QUAL_NONE", 0)
                    .constant("QUAL_GLOBAL", 1)
                    .constant("QUAL_INSTANCE", 2)
                    .constant("QUAL_INSTANCE_INDEX", 3)
                    .constant("QUAL_MAX", 4),
            ),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeFloatParameter")
            .parent("VisualShaderNodeParameter")
            // HINT_RANGE emits `: hint_range(min, max)`; HINT_RANGE_STEP adds step.
            .property(PropertyInfo::new("hint", Variant::Int(0)))
            .property(PropertyInfo::new("min", Variant::Float(0.0)))
            .property(PropertyInfo::new("max", Variant::Float(1.0)))
            .property(PropertyInfo::new("step", Variant::Float(0.1)))
            .property(PropertyInfo::new(
                "default_value_enabled",
                Variant::Bool(false),
            ))
            .property(PropertyInfo::new("default_value", Variant::Float(0.0)))
            .enumeration(
                EnumInfo::new("Hint")
                    .constant("HINT_NONE", 0)
                    .constant("HINT_RANGE", 1)
                    .constant("HINT_RANGE_STEP", 2)
                    .constant("HINT_MAX", 3),
            ),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeIntParameter")
            .parent("VisualShaderNodeParameter")
            .property(PropertyInfo::new("hint", Variant::Int(0)))
            .property(PropertyInfo::new("min", Variant::Int(0)))
            .property(PropertyInfo::new("max", Variant::Int(100)))
            .property(PropertyInfo::new("step", Variant::Int(1)))
            // Only meaningful under HINT_ENUM — the int parameter's named values.
            .property(PropertyInfo::new(
                "enum_names",
                Variant::PackedStringArray(Vec::new()),
            ))
            .property(PropertyInfo::new(
                "default_value_enabled",
                Variant::Bool(false),
            ))
            .property(PropertyInfo::new("default_value", Variant::Int(0)))
            .enumeration(
                // Note: the int hint has HINT_ENUM, which the float one does not.
                EnumInfo::new("Hint")
                    .constant("HINT_NONE", 0)
                    .constant("HINT_RANGE", 1)
                    .constant("HINT_RANGE_STEP", 2)
                    .constant("HINT_ENUM", 3)
                    .constant("HINT_MAX", 4),
            ),
    );
    register_class(
        ClassRegistration::new("VisualShaderNodeColorParameter")
            .parent("VisualShaderNodeParameter")
            .property(PropertyInfo::new(
                "default_value_enabled",
                Variant::Bool(false),
            ))
            .property(PropertyInfo::new(
                "default_value",
                Variant::Color(gdcore::math::Color::WHITE),
            )),
    );
    // The abstract base for every texture-valued uniform (2D/2DArray/3D/Cubemap).
    register_class(
        ClassRegistration::new("VisualShaderNodeTextureParameter")
            .parent("VisualShaderNodeParameter")
            .abstract_class()
            .property(PropertyInfo::new("texture_type", Variant::Int(0)))
            .property(PropertyInfo::new("color_default", Variant::Int(0)))
            .property(PropertyInfo::new("texture_filter", Variant::Int(0)))
            .property(PropertyInfo::new("texture_repeat", Variant::Int(0)))
            .property(PropertyInfo::new("texture_source", Variant::Int(0)))
            .enumeration(
                EnumInfo::new("TextureType")
                    .constant("TYPE_DATA", 0)
                    .constant("TYPE_COLOR", 1)
                    .constant("TYPE_NORMAL_MAP", 2)
                    .constant("TYPE_ANISOTROPY", 3)
                    .constant("TYPE_MAX", 4),
            )
            .enumeration(
                EnumInfo::new("ColorDefault")
                    .constant("COLOR_DEFAULT_WHITE", 0)
                    .constant("COLOR_DEFAULT_BLACK", 1)
                    .constant("COLOR_DEFAULT_TRANSPARENT", 2)
                    .constant("COLOR_DEFAULT_MAX", 3),
            )
            .enumeration(
                EnumInfo::new("TextureFilter")
                    .constant("FILTER_DEFAULT", 0)
                    .constant("FILTER_NEAREST", 1)
                    .constant("FILTER_LINEAR", 2)
                    .constant("FILTER_NEAREST_MIPMAP", 3)
                    .constant("FILTER_LINEAR_MIPMAP", 4)
                    .constant("FILTER_NEAREST_MIPMAP_ANISOTROPIC", 5)
                    .constant("FILTER_LINEAR_MIPMAP_ANISOTROPIC", 6)
                    .constant("FILTER_MAX", 7),
            )
            .enumeration(
                EnumInfo::new("TextureRepeat")
                    .constant("REPEAT_DEFAULT", 0)
                    .constant("REPEAT_ENABLED", 1)
                    .constant("REPEAT_DISABLED", 2)
                    .constant("REPEAT_MAX", 3),
            )
            .enumeration(
                EnumInfo::new("TextureSource")
                    .constant("SOURCE_NONE", 0)
                    .constant("SOURCE_SCREEN", 1)
                    .constant("SOURCE_DEPTH", 2)
                    .constant("SOURCE_NORMAL_ROUGHNESS", 3)
                    .constant("SOURCE_MAX", 4),
            ),
    );
    // Godot gives CubemapParameter no properties of its own — the whole schema
    // is inherited from VisualShaderNodeTextureParameter.
    register_class(
        ClassRegistration::new("VisualShaderNodeCubemapParameter")
            .parent("VisualShaderNodeTextureParameter"),
    );

    // -- Cubemap / curve sampling nodes (pat-nzgdu.225) ----------------------
    //
    // ⚠️ `VisualShaderNodeCubemap` is NOT a `VisualShaderNodeCubemapParameter`
    // (registered just above) and does not derive it: a *parameter* declares a
    // uniform the material exposes; this *samples* a cubemap the node owns. Same
    // noun, unrelated jobs — they only share `VisualShaderNode`.
    if !class_exists("VisualShaderNodeCubemap") {
        register_class(
            ClassRegistration::new("VisualShaderNodeCubemap")
                .parent("VisualShaderNode")
                .property(PropertyInfo::new("source", Variant::Int(0)))
                // A Cubemap/CompressedCubemap/PlaceholderCubemap/TextureCubemapRD
                // — Nil until assigned.
                .property(PropertyInfo::new("cube_map", Variant::Nil))
                .property(PropertyInfo::new("texture_type", Variant::Int(0)))
                .method(MethodInfo::new("set_source", 1))
                .method(MethodInfo::new("get_source", 0))
                .method(MethodInfo::new("set_cube_map", 1))
                .method(MethodInfo::new("get_cube_map", 0))
                .method(MethodInfo::new("set_texture_type", 1))
                .method(MethodInfo::new("get_texture_type", 0))
                // ⚠️ There is NO `SOURCE_SCREEN`. The bead says the source enum is
                // "TEXTURE/PORT/SCREEN"; Godot has TEXTURE, PORT and a `_MAX`
                // sentinel. Registering a SCREEN would invent a value scripts
                // could pass and nothing could honour.
                .enumeration(
                    EnumInfo::new("Source")
                        .constant("SOURCE_TEXTURE", 0)
                        .constant("SOURCE_PORT", 1)
                        .constant("SOURCE_MAX", 2),
                )
                // ⚠️ TYPE_NORMAL_MAP, not "NORMAL" (the bead's spelling).
                .enumeration(
                    EnumInfo::new("TextureType")
                        .constant("TYPE_DATA", 0)
                        .constant("TYPE_COLOR", 1)
                        .constant("TYPE_NORMAL_MAP", 2)
                        .constant("TYPE_MAX", 3),
                ),
        );
    }
    // ⚠️ ABSTRACT — `GDREGISTER_ABSTRACT_CLASS(VisualShaderNodeResizableBase)`.
    // It exists to give resizable nodes their editor `size`; you never place one.
    if !class_exists("VisualShaderNodeResizableBase") {
        register_class(
            ClassRegistration::new("VisualShaderNodeResizableBase")
                .parent("VisualShaderNode")
                .abstract_class()
                .property(PropertyInfo::new(
                    "size",
                    Variant::Vector2(gdcore::math::Vector2::new(0.0, 0.0)),
                ))
                .method(MethodInfo::new("set_size", 1))
                .method(MethodInfo::new("get_size", 0)),
        );
    }
    // ⚠️ These two derive the RESIZABLE base, not VisualShaderNode directly —
    // they draw a curve preview in the editor, which is what the size is for.
    // Each binds only its own `texture` accessor pair.
    if !class_exists("VisualShaderNodeCurveTexture") {
        register_class(
            ClassRegistration::new("VisualShaderNodeCurveTexture")
                .parent("VisualShaderNodeResizableBase")
                .property(PropertyInfo::new("texture", Variant::Nil))
                .method(MethodInfo::new("set_texture", 1))
                .method(MethodInfo::new("get_texture", 0)),
        );
    }
    if !class_exists("VisualShaderNodeCurveXYZTexture") {
        register_class(
            ClassRegistration::new("VisualShaderNodeCurveXYZTexture")
                .parent("VisualShaderNodeResizableBase")
                .property(PropertyInfo::new("texture", Variant::Nil))
                .method(MethodInfo::new("set_texture", 1))
                .method(MethodInfo::new("get_texture", 0)),
        );
    }

    // -- Scene-effect nodes (pat-nzgdu.226) ----------------------------------
    //
    // Spatial-only helpers that read shader built-ins (DEPTH_TEXTURE, VERTEX,
    // INV_PROJECTION_MATRIX) and emit a fixed snippet.
    //
    // ⚠️ All three bind **nothing** — zero methods, zero properties, zero enums.
    // Their inputs are graph *ports* (DistanceFade's min/max, ProximityFade's
    // distance), which are a VisualShader concept, not ClassDB properties. The
    // bead's "min/max distance ports" wording is right, and reading it as
    // properties would invent members Godot does not have.
    if !class_exists("VisualShaderNodeLinearSceneDepth") {
        register_class(
            ClassRegistration::new("VisualShaderNodeLinearSceneDepth").parent("VisualShaderNode"),
        );
    }
    if !class_exists("VisualShaderNodeDistanceFade") {
        register_class(
            ClassRegistration::new("VisualShaderNodeDistanceFade").parent("VisualShaderNode"),
        );
    }
    if !class_exists("VisualShaderNodeProximityFade") {
        register_class(
            ClassRegistration::new("VisualShaderNodeProximityFade").parent("VisualShaderNode"),
        );
    }

    // -- Vector family + depth position (pat-nzgdu.227) ----------------------
    //
    // ⚠️ All three bind **nothing** — zero methods, zero properties, zero enums.
    //
    // The bead says these "carry an operator/type enum (... VectorLen: length;
    // VectorRefract: refract)". They do not: `VisualShaderNodeVectorOp` is the
    // only node in this family with an operator, and Len/Refract each ARE the
    // operation — there is nothing to choose. Their one configurable member,
    // `op_type` (2D/3D/4D), is `VisualShaderNodeVectorBase`'s and is inherited.
    // Registering a "length"/"refract" enum would invent members Godot lacks.
    if !class_exists("VisualShaderNodeVectorLen") {
        register_class(
            ClassRegistration::new("VisualShaderNodeVectorLen")
                .parent("VisualShaderNodeVectorBase"),
        );
    }
    if !class_exists("VisualShaderNodeVectorRefract") {
        register_class(
            ClassRegistration::new("VisualShaderNodeVectorRefract")
                .parent("VisualShaderNodeVectorBase"),
        );
    }
    // ⚠️ NOT a vector node despite arriving in this bead: it reconstructs a world
    // position from DEPTH_TEXTURE, so it hangs off `VisualShaderNode` directly —
    // it has no `op_type` and is not a `VisualShaderNodeVectorBase`. It is the
    // sibling of pat-nzgdu.226's scene-effect nodes.
    if !class_exists("VisualShaderNodeWorldPositionFromDepth") {
        register_class(
            ClassRegistration::new("VisualShaderNodeWorldPositionFromDepth")
                .parent("VisualShaderNode"),
        );
    }
    // Reads back an existing named parameter rather than declaring a new one, so
    // it derives VisualShaderNode directly — NOT VisualShaderNodeParameter.
    register_class(
        ClassRegistration::new("VisualShaderNodeParameterRef")
            .parent("VisualShaderNode")
            .property(PropertyInfo::new(
                "parameter_name",
                Variant::String(String::new()),
            ))
            .property(PropertyInfo::new("param_type", Variant::Int(0))),
    );
    register_class(ClassRegistration::new("VisualShaderNodeTexture").parent("VisualShaderNode"));
    register_class(
        ClassRegistration::new("VisualShaderNodeExpression")
            .parent("VisualShaderNode")
            .property(PropertyInfo::new(
                "expression",
                Variant::String(String::new()),
            )),
    );
    register_class(
        ClassRegistration::new("BaseMaterial3D")
            .parent("Material")
            .abstract_class(),
    );
    register_class(ClassRegistration::new("StandardMaterial3D").parent("BaseMaterial3D"));
    register_class(ClassRegistration::new("ORMMaterial3D").parent("BaseMaterial3D"));

    // ParticleProcessMaterial (pat-nzgdu.93): the resource driving GPU particles.
    // Registered with its high-use property surface + the four enum defaults so
    // ClassDB resolves `process_material = SubResource("ParticleProcessMaterial_…")`
    // in a .tscn. (The .tscn/.tres value parse + simulator wiring are follow-ups.)
    register_class(
        ClassRegistration::new("ParticleProcessMaterial")
            .parent("Material")
            // Emission (EmissionShape enum: 0 = POINT default).
            .property(PropertyInfo::new("emission_shape", Variant::Int(0)))
            .property(PropertyInfo::new(
                "emission_sphere_radius",
                Variant::Float(1.0),
            ))
            .property(PropertyInfo::new(
                "emission_box_extents",
                Variant::Vector3(Vector3::new(1.0, 1.0, 1.0)),
            ))
            // Direction / spread.
            .property(PropertyInfo::new(
                "direction",
                Variant::Vector3(Vector3::new(1.0, 0.0, 0.0)),
            ))
            .property(PropertyInfo::new("spread", Variant::Float(45.0)))
            .property(PropertyInfo::new("flatness", Variant::Float(0.0)))
            // Gravity + linear/angular motion.
            .property(PropertyInfo::new(
                "gravity",
                Variant::Vector3(Vector3::new(0.0, -9.8, 0.0)),
            ))
            .property(PropertyInfo::new(
                "initial_velocity_min",
                Variant::Float(0.0),
            ))
            .property(PropertyInfo::new(
                "initial_velocity_max",
                Variant::Float(0.0),
            ))
            .property(PropertyInfo::new(
                "angular_velocity_min",
                Variant::Float(0.0),
            ))
            .property(PropertyInfo::new(
                "angular_velocity_max",
                Variant::Float(0.0),
            ))
            .property(PropertyInfo::new("damping_min", Variant::Float(0.0)))
            .property(PropertyInfo::new("damping_max", Variant::Float(0.0)))
            .property(PropertyInfo::new("scale_min", Variant::Float(1.0)))
            .property(PropertyInfo::new("scale_max", Variant::Float(1.0)))
            // Flag / grouping enums (ParticleFlags, SubEmitterMode, CollisionMode).
            .property(PropertyInfo::new(
                "particle_flag_align_y",
                Variant::Bool(false),
            ))
            .property(PropertyInfo::new("sub_emitter_mode", Variant::Int(0)))
            .property(PropertyInfo::new("collision_mode", Variant::Int(0)))
            .property(PropertyInfo::new(
                "lifetime_randomness",
                Variant::Float(0.0),
            )),
    );
}

/// Registers the standard Godot 2D class hierarchy in the ClassDB.
///
/// This includes the CanvasItem → Node2D chain, common 2D nodes (Sprite2D,
/// AnimatedSprite2D, etc.), Control UI classes, and 2D physics bodies.
pub fn register_2d_classes() {
    use gdcore::math::Vector2;

    if !class_exists("Object") {
        register_class(object_class());
    }
    if !class_exists("Node") {
        register_class(node_class());
    }
    if !class_exists("Resource") {
        register_class(ClassRegistration::new("Resource").parent("RefCounted"));
    }

    // -- StyleBox resource family (pat-nzgdu.90). Registering these lets ClassDB
    // resolve `[sub_resource type="StyleBoxFlat"]` (etc.) in a .tscn/.tres theme
    // to their proper class + parent chain, matching Godot's Resource hierarchy.
    // -- Font resource family (pat-0q2kx.146) --
    //
    // Godot imports a .ttf/.otf as a `FontFile`, and `Font` is the abstract base
    // carrying the `fallbacks` chain (scene/resources/font.cpp). None of these
    // were in ClassDB, so `ClassDB.instantiate("FontFile")` failed and a
    // `theme_override_fonts/font` slot had no class to resolve against. Names
    // and defaults from doc/classes/<Name>.xml.
    if !class_exists("Font") {
        register_class(
            ClassRegistration::new("Font")
                .parent("Resource")
                .abstract_class()
                .property(PropertyInfo::new("fallbacks", Variant::Nil)),
        );
        register_class(
            ClassRegistration::new("FontFile")
                .parent("Font")
                .property(PropertyInfo::new(
                    "allow_system_fallback",
                    Variant::Bool(true),
                ))
                .property(PropertyInfo::new("antialiasing", Variant::Int(1)))
                .property(PropertyInfo::new(
                    "data",
                    Variant::PackedByteArray(Vec::new()),
                ))
                .property(PropertyInfo::new(
                    "disable_embedded_bitmaps",
                    Variant::Bool(true),
                ))
                .property(PropertyInfo::new("fixed_size", Variant::Int(0)))
                .property(PropertyInfo::new("fixed_size_scale_mode", Variant::Int(0)))
                .property(PropertyInfo::new(
                    "font_name",
                    Variant::String(String::new()),
                ))
                .property(PropertyInfo::new("font_stretch", Variant::Int(100)))
                .property(PropertyInfo::new("font_style", Variant::Int(0)))
                .property(PropertyInfo::new("font_weight", Variant::Int(400)))
                .property(PropertyInfo::new("force_autohinter", Variant::Bool(false)))
                .property(PropertyInfo::new("generate_mipmaps", Variant::Bool(false)))
                .property(PropertyInfo::new("hinting", Variant::Int(1)))
                .property(PropertyInfo::new(
                    "keep_rounding_remainders",
                    Variant::Bool(true),
                ))
                .property(PropertyInfo::new(
                    "modulate_color_glyphs",
                    Variant::Bool(false),
                ))
                .property(PropertyInfo::new("msdf_pixel_range", Variant::Int(16)))
                .property(PropertyInfo::new("msdf_size", Variant::Int(48)))
                .property(PropertyInfo::new(
                    "multichannel_signed_distance_field",
                    Variant::Bool(false),
                ))
                .property(PropertyInfo::new(
                    "opentype_feature_overrides",
                    Variant::Dictionary(Default::default()),
                ))
                .property(PropertyInfo::new("oversampling", Variant::Float(0.0)))
                .property(PropertyInfo::new(
                    "style_name",
                    Variant::String(String::new()),
                ))
                .property(PropertyInfo::new("subpixel_positioning", Variant::Int(1))),
        );
        register_class(
            ClassRegistration::new("FontVariation")
                .parent("Font")
                .property(PropertyInfo::new("base_font", Variant::Nil))
                .property(PropertyInfo::new("baseline_offset", Variant::Float(0.0)))
                .property(PropertyInfo::new(
                    "opentype_features",
                    Variant::Dictionary(Default::default()),
                ))
                .property(PropertyInfo::new("spacing_bottom", Variant::Int(0)))
                .property(PropertyInfo::new("spacing_glyph", Variant::Int(0)))
                .property(PropertyInfo::new("spacing_space", Variant::Int(0)))
                .property(PropertyInfo::new("spacing_top", Variant::Int(0)))
                .property(PropertyInfo::new("variation_embolden", Variant::Float(0.0)))
                .property(PropertyInfo::new("variation_face_index", Variant::Int(0)))
                .property(PropertyInfo::new(
                    "variation_opentype",
                    Variant::Dictionary(Default::default()),
                ))
                .property(PropertyInfo::new("variation_transform", Variant::Nil)),
        );
        register_class(
            ClassRegistration::new("SystemFont")
                .parent("Font")
                .property(PropertyInfo::new(
                    "allow_system_fallback",
                    Variant::Bool(true),
                ))
                .property(PropertyInfo::new("antialiasing", Variant::Int(1)))
                .property(PropertyInfo::new(
                    "disable_embedded_bitmaps",
                    Variant::Bool(true),
                ))
                .property(PropertyInfo::new("font_italic", Variant::Bool(false)))
                .property(PropertyInfo::new(
                    "font_names",
                    Variant::PackedStringArray(Vec::new()),
                ))
                .property(PropertyInfo::new("font_stretch", Variant::Int(100)))
                .property(PropertyInfo::new("font_weight", Variant::Int(400)))
                .property(PropertyInfo::new("force_autohinter", Variant::Bool(false)))
                .property(PropertyInfo::new("generate_mipmaps", Variant::Bool(false)))
                .property(PropertyInfo::new("hinting", Variant::Int(1)))
                .property(PropertyInfo::new(
                    "keep_rounding_remainders",
                    Variant::Bool(true),
                ))
                .property(PropertyInfo::new(
                    "modulate_color_glyphs",
                    Variant::Bool(false),
                ))
                .property(PropertyInfo::new("msdf_pixel_range", Variant::Int(16)))
                .property(PropertyInfo::new("msdf_size", Variant::Int(48)))
                .property(PropertyInfo::new(
                    "multichannel_signed_distance_field",
                    Variant::Bool(false),
                ))
                .property(PropertyInfo::new("oversampling", Variant::Float(0.0)))
                .property(PropertyInfo::new("subpixel_positioning", Variant::Int(1))),
        );
        register_class(
            ClassRegistration::new("LabelSettings")
                .parent("Resource")
                .property(PropertyInfo::new("font", Variant::Nil))
                .property(PropertyInfo::new(
                    "font_color",
                    Variant::Color(gdcore::math::Color::new(1.0, 1.0, 1.0, 1.0)),
                ))
                .property(PropertyInfo::new("font_size", Variant::Int(16)))
                .property(PropertyInfo::new("line_spacing", Variant::Float(3.0)))
                .property(PropertyInfo::new(
                    "outline_color",
                    Variant::Color(gdcore::math::Color::new(1.0, 1.0, 1.0, 1.0)),
                ))
                .property(PropertyInfo::new("outline_size", Variant::Int(0)))
                .property(PropertyInfo::new("paragraph_spacing", Variant::Float(0.0)))
                .property(PropertyInfo::new(
                    "shadow_color",
                    Variant::Color(gdcore::math::Color::new(0.0, 0.0, 0.0, 0.0)),
                ))
                .property(PropertyInfo::new(
                    "shadow_offset",
                    Variant::Vector2(gdcore::math::Vector2::new(1.0, 1.0)),
                ))
                .property(PropertyInfo::new("shadow_size", Variant::Int(1)))
                .property(PropertyInfo::new("stacked_outline_count", Variant::Int(0)))
                .property(PropertyInfo::new("stacked_shadow_count", Variant::Int(0))),
        );
    }
    // pat-0q2kx.148: the StyleBox family was registered as bare class names with
    // no properties, so the inspector had nothing to show and a `.tscn`
    // round-trip had no defaults to compare against. Names and defaults are
    // Godot 4.6.1's (`scene/resources/style_box*.cpp`).
    //
    // `content_margin_*` sits on the **base** class: every subtype carries
    // padding, which is what makes `StyleBoxEmpty` Godot's standard
    // pad-without-painting resource. Its default is -1 ("unset, derive").
    if !class_exists("StyleBox") {
        let content_margins = |reg: ClassRegistration| -> ClassRegistration {
            reg.property(PropertyInfo::new(
                "content_margin_left",
                Variant::Float(-1.0),
            ))
            .property(PropertyInfo::new(
                "content_margin_top",
                Variant::Float(-1.0),
            ))
            .property(PropertyInfo::new(
                "content_margin_right",
                Variant::Float(-1.0),
            ))
            .property(PropertyInfo::new(
                "content_margin_bottom",
                Variant::Float(-1.0),
            ))
        };
        // ONLY the base carries these. `get_property_list` walks the whole
        // inheritance chain and concatenates without deduping, so applying
        // `content_margins` to a subtype as well would report all four twice —
        // 8 properties on `StyleBoxEmpty`, and a duplicated row per margin in
        // the inspector. Subtypes inherit them.
        register_class(content_margins(
            ClassRegistration::new("StyleBox").parent("Resource"),
        ));
        register_class(
            ClassRegistration::new("StyleBoxFlat")
                .parent("StyleBox")
                .property(PropertyInfo::new(
                    "bg_color",
                    Variant::Color(gdcore::math::Color::new(0.6, 0.6, 0.6, 1.0)),
                ))
                .property(PropertyInfo::new("draw_center", Variant::Bool(true)))
                .property(PropertyInfo::new("skew", Variant::Vector2(Vector2::ZERO)))
                .property(PropertyInfo::new("border_width_left", Variant::Int(0)))
                .property(PropertyInfo::new("border_width_top", Variant::Int(0)))
                .property(PropertyInfo::new("border_width_right", Variant::Int(0)))
                .property(PropertyInfo::new("border_width_bottom", Variant::Int(0)))
                .property(PropertyInfo::new(
                    "border_color",
                    Variant::Color(gdcore::math::Color::new(0.8, 0.8, 0.8, 1.0)),
                ))
                .property(PropertyInfo::new("border_blend", Variant::Bool(false)))
                .property(PropertyInfo::new("corner_radius_top_left", Variant::Int(0)))
                .property(PropertyInfo::new(
                    "corner_radius_top_right",
                    Variant::Int(0),
                ))
                .property(PropertyInfo::new(
                    "corner_radius_bottom_right",
                    Variant::Int(0),
                ))
                .property(PropertyInfo::new(
                    "corner_radius_bottom_left",
                    Variant::Int(0),
                ))
                .property(PropertyInfo::new("corner_detail", Variant::Int(8)))
                .property(PropertyInfo::new("expand_margin_left", Variant::Float(0.0)))
                .property(PropertyInfo::new("expand_margin_top", Variant::Float(0.0)))
                .property(PropertyInfo::new(
                    "expand_margin_right",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new(
                    "expand_margin_bottom",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new(
                    "shadow_color",
                    Variant::Color(gdcore::math::Color::new(0.0, 0.0, 0.0, 0.6)),
                ))
                .property(PropertyInfo::new("shadow_size", Variant::Int(0)))
                .property(PropertyInfo::new(
                    "shadow_offset",
                    Variant::Vector2(Vector2::ZERO),
                ))
                .property(PropertyInfo::new("anti_aliasing", Variant::Bool(true)))
                .property(PropertyInfo::new("anti_aliasing_size", Variant::Float(1.0))),
        );
        register_class(
            ClassRegistration::new("StyleBoxTexture")
                .parent("StyleBox")
                .property(PropertyInfo::new("texture", Variant::Nil))
                .property(PropertyInfo::new(
                    "texture_margin_left",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new("texture_margin_top", Variant::Float(0.0)))
                .property(PropertyInfo::new(
                    "texture_margin_right",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new(
                    "texture_margin_bottom",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new("expand_margin_left", Variant::Float(0.0)))
                .property(PropertyInfo::new("expand_margin_top", Variant::Float(0.0)))
                .property(PropertyInfo::new(
                    "expand_margin_right",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new(
                    "expand_margin_bottom",
                    Variant::Float(0.0),
                ))
                // Godot AxisStretchMode: STRETCH = 0, TILE = 1, TILE_FIT = 2.
                .property(PropertyInfo::new(
                    "axis_stretch_horizontal",
                    Variant::Int(0),
                ))
                .property(PropertyInfo::new("axis_stretch_vertical", Variant::Int(0)))
                .property(PropertyInfo::new(
                    "region_rect",
                    Variant::Rect2(gdcore::math::Rect2::new(
                        Vector2::ZERO,
                        Vector2::new(0.0, 0.0),
                    )),
                ))
                .property(PropertyInfo::new(
                    "modulate_color",
                    Variant::Color(gdcore::math::Color::WHITE),
                ))
                .property(PropertyInfo::new("draw_center", Variant::Bool(true)))
                .enumeration(
                    EnumInfo::new("AxisStretchMode")
                        .constant("AXIS_STRETCH_MODE_STRETCH", 0)
                        .constant("AXIS_STRETCH_MODE_TILE", 1)
                        .constant("AXIS_STRETCH_MODE_TILE_FIT", 2),
                ),
        );
        register_class(
            ClassRegistration::new("StyleBoxLine")
                .parent("StyleBox")
                .property(PropertyInfo::new(
                    "color",
                    Variant::Color(gdcore::math::Color::new(0.0, 0.0, 0.0, 1.0)),
                ))
                .property(PropertyInfo::new("grow_begin", Variant::Float(1.0)))
                .property(PropertyInfo::new("grow_end", Variant::Float(1.0)))
                .property(PropertyInfo::new("thickness", Variant::Int(1)))
                .property(PropertyInfo::new("vertical", Variant::Bool(false))),
        );
        // StyleBoxEmpty adds nothing of its own — the base content margins ARE
        // its whole purpose, and it inherits them.
        register_class(ClassRegistration::new("StyleBoxEmpty").parent("StyleBox"));
    }

    // -- CanvasItem (base for all 2D & UI nodes) --
    if !class_exists("CanvasItem") {
        register_class(
            ClassRegistration::new("CanvasItem")
                .parent("Node")
                .abstract_class()
                .property(PropertyInfo::new("visible", Variant::Bool(true)))
                .property(PropertyInfo::new(
                    "modulate",
                    Variant::Color(gdcore::math::Color::WHITE),
                ))
                .property(PropertyInfo::new("z_index", Variant::Int(0)))
                // The custom-drawing surface (pat-0q2kx.82). `queue_redraw` was
                // not registered at all, so `has_method("queue_redraw")` — and
                // anything else that answers from ClassDB rather than from the
                // native dispatch table — reported that CanvasItem could not
                // redraw itself.
                .method(MethodInfo::new("queue_redraw", 0))
                .method(MethodInfo::new("is_queued_for_redraw", 0))
                .method(MethodInfo::new("draw_rect", 4))
                .method(MethodInfo::new("draw_line", 4))
                .method(MethodInfo::new("draw_circle", 4))
                .method(MethodInfo::new("draw_texture", 3))
                .method(MethodInfo::new("draw_texture_rect", 4))
                .method(MethodInfo::new("draw_string", 7)),
        );
    }

    // -- Node2D --
    if !class_exists("Node2D") {
        register_class(
            ClassRegistration::new("Node2D")
                .parent("CanvasItem")
                .property(PropertyInfo::new(
                    "position",
                    Variant::Vector2(Vector2::ZERO),
                ))
                .property(PropertyInfo::new("rotation", Variant::Float(0.0)))
                .property(PropertyInfo::new("scale", Variant::Vector2(Vector2::ONE))),
        );
    }

    // -- Common 2D nodes --
    if !class_exists("Sprite2D") {
        // Sprite2D: 12 own properties from doc/classes/Sprite2D.xml.
        register_class(
            ClassRegistration::new("Sprite2D")
                .parent("Node2D")
                .property(PropertyInfo::new("centered", Variant::Bool(true)))
                .property(PropertyInfo::new("flip_h", Variant::Bool(false)))
                .property(PropertyInfo::new("flip_v", Variant::Bool(false)))
                .property(PropertyInfo::new("frame", Variant::Int(0)))
                .property(PropertyInfo::new(
                    "frame_coords",
                    Variant::Vector2i(gdcore::math::Vector2i::new(0, 0)),
                ))
                .property(PropertyInfo::new("hframes", Variant::Int(1)))
                .property(PropertyInfo::new(
                    "offset",
                    Variant::Vector2(Vector2::new(0.0, 0.0)),
                ))
                .property(PropertyInfo::new("region_enabled", Variant::Bool(false)))
                .property(PropertyInfo::new(
                    "region_filter_clip_enabled",
                    Variant::Bool(false),
                ))
                .property(PropertyInfo::new(
                    "region_rect",
                    Variant::Rect2(gdcore::math::Rect2::new(
                        Vector2::new(0.0, 0.0),
                        Vector2::new(0.0, 0.0),
                    )),
                ))
                .property(PropertyInfo::new("texture", Variant::Nil))
                .property(PropertyInfo::new("vframes", Variant::Int(1))),
        );
    }
    // AnimatedSprite2D was registered as a bare name with no properties,
    // signals or methods, so nothing could inspect it, no `.tscn` property
    // round-tripped through a schema, and `has_signal("animation_finished")`
    // answered false for the signal the playback phase emits (pat-0q2kx.100).
    if !class_exists("AnimatedSprite2D") {
        register_class(
            ClassRegistration::new("AnimatedSprite2D")
                .parent("Node2D")
                .property(PropertyInfo::new("sprite_frames", Variant::Nil))
                .property(PropertyInfo::new(
                    "animation",
                    // Godot's default animation name.
                    Variant::String("default".to_string()),
                ))
                .property(PropertyInfo::new(
                    "autoplay",
                    Variant::String(String::new()),
                ))
                .property(PropertyInfo::new("frame", Variant::Int(0)))
                .property(PropertyInfo::new("frame_progress", Variant::Float(0.0)))
                .property(PropertyInfo::new("speed_scale", Variant::Float(1.0)))
                .property(PropertyInfo::new("playing", Variant::Bool(false)))
                .property(PropertyInfo::new("centered", Variant::Bool(true)))
                .property(PropertyInfo::new(
                    "offset",
                    Variant::Vector2(gdcore::math::Vector2::ZERO),
                ))
                .property(PropertyInfo::new("flip_h", Variant::Bool(false)))
                .property(PropertyInfo::new("flip_v", Variant::Bool(false)))
                .signal(SignalInfo::new("sprite_frames_changed"))
                .signal(SignalInfo::new("animation_changed"))
                .signal(SignalInfo::new("frame_changed"))
                .signal(SignalInfo::new("animation_looped"))
                .signal(SignalInfo::new("animation_finished")),
        );
    }
    // The resource those frames live in. Its contents reach a node as the flat
    // `__sf_*` companion keys the `.tscn` parser writes, but the class itself
    // has to exist for the inspector and for `ClassDB.class_exists`.
    if !class_exists("SpriteFrames") {
        register_class(
            ClassRegistration::new("SpriteFrames")
                .parent("Resource")
                .property(PropertyInfo::new("animations", Variant::Array(Vec::new()))),
        );
    }
    if !class_exists("Camera2D") {
        // Camera2D: 28 own properties from doc/classes/Camera2D.xml.
        register_class(
            ClassRegistration::new("Camera2D")
                .parent("Node2D")
                .property(PropertyInfo::new("anchor_mode", Variant::Int(1)))
                .property(PropertyInfo::new("custom_viewport", Variant::Nil))
                .property(PropertyInfo::new("drag_bottom_margin", Variant::Float(0.2)))
                .property(PropertyInfo::new(
                    "drag_horizontal_enabled",
                    Variant::Bool(false),
                ))
                .property(PropertyInfo::new(
                    "drag_horizontal_offset",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new("drag_left_margin", Variant::Float(0.2)))
                .property(PropertyInfo::new("drag_right_margin", Variant::Float(0.2)))
                .property(PropertyInfo::new("drag_top_margin", Variant::Float(0.2)))
                .property(PropertyInfo::new(
                    "drag_vertical_enabled",
                    Variant::Bool(false),
                ))
                .property(PropertyInfo::new(
                    "drag_vertical_offset",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new(
                    "editor_draw_drag_margin",
                    Variant::Bool(false),
                ))
                .property(PropertyInfo::new(
                    "editor_draw_limits",
                    Variant::Bool(false),
                ))
                .property(PropertyInfo::new("editor_draw_screen", Variant::Bool(true)))
                .property(PropertyInfo::new("enabled", Variant::Bool(true)))
                .property(PropertyInfo::new("ignore_rotation", Variant::Bool(true)))
                .property(PropertyInfo::new("limit_bottom", Variant::Int(10000000)))
                .property(PropertyInfo::new("limit_enabled", Variant::Bool(true)))
                .property(PropertyInfo::new("limit_left", Variant::Int(-10000000)))
                .property(PropertyInfo::new("limit_right", Variant::Int(10000000)))
                .property(PropertyInfo::new("limit_smoothed", Variant::Bool(false)))
                .property(PropertyInfo::new("limit_top", Variant::Int(-10000000)))
                .property(PropertyInfo::new(
                    "offset",
                    Variant::Vector2(Vector2::new(0.0, 0.0)),
                ))
                .property(PropertyInfo::new(
                    "position_smoothing_enabled",
                    Variant::Bool(false),
                ))
                .property(PropertyInfo::new(
                    "position_smoothing_speed",
                    Variant::Float(5.0),
                ))
                .property(PropertyInfo::new("process_callback", Variant::Int(1)))
                .property(PropertyInfo::new(
                    "rotation_smoothing_enabled",
                    Variant::Bool(false),
                ))
                .property(PropertyInfo::new(
                    "rotation_smoothing_speed",
                    Variant::Float(5.0),
                ))
                .property(PropertyInfo::new(
                    "zoom",
                    Variant::Vector2(Vector2::new(1.0, 1.0)),
                )),
        );
    }
    // pat-0q2kx.108: TileMapLayer had a bare registration, so an inspector
    // never showed the properties a painted layer actually carries and a
    // `.tscn` round-trip had no defaults to compare against. Names, types and
    // defaults are Godot 4.6.1's (`extension_api.json` + doc/classes/
    // TileMapLayer.xml). `tile_set` is a Resource reference, hence Nil.
    if !class_exists("TileMapLayer") {
        register_class(
            ClassRegistration::new("TileMapLayer")
                .parent("Node2D")
                .property(PropertyInfo::new(
                    "tile_map_data",
                    Variant::PackedByteArray(Vec::new()),
                ))
                .property(PropertyInfo::new("tile_set", Variant::Nil))
                .property(PropertyInfo::new("enabled", Variant::Bool(true)))
                .property(PropertyInfo::new("navigation_enabled", Variant::Bool(true)))
                .property(PropertyInfo::new("collision_enabled", Variant::Bool(true)))
                .property(PropertyInfo::new("occlusion_enabled", Variant::Bool(true)))
                .property(PropertyInfo::new(
                    "use_kinematic_bodies",
                    Variant::Bool(false),
                ))
                .property(PropertyInfo::new(
                    "x_draw_order_reversed",
                    Variant::Bool(false),
                ))
                .property(PropertyInfo::new("y_sort_origin", Variant::Int(0)))
                .property(PropertyInfo::new(
                    "rendering_quadrant_size",
                    Variant::Int(16),
                ))
                .property(PropertyInfo::new("physics_quadrant_size", Variant::Int(16)))
                .property(PropertyInfo::new(
                    "collision_visibility_mode",
                    Variant::Int(0),
                ))
                .property(PropertyInfo::new(
                    "navigation_visibility_mode",
                    Variant::Int(0),
                ))
                .signal(SignalInfo::new("changed")),
        );
    }

    // -- Misc canvas nodes (pat-nzgdu.174) ---------------------------------
    //
    // All seven derive Node2D. Names/types/parents/enums are diffed against
    // Godot 4.6.1's `extension_api.json`; the *defaults* are Godot's documented
    // values (the dump carries no property defaults), which is the one part here
    // not machine-checked.
    //
    // Object-typed properties (Mesh/Texture2D/MultiMesh/BitMap/Shape2D) default
    // to Nil — they hold no value until assigned.

    // An editor-only anchor: draws a cross gizmo, otherwise a plain Node2D.
    if !class_exists("Marker2D") {
        register_class(
            ClassRegistration::new("Marker2D")
                .parent("Node2D")
                .property(PropertyInfo::new("gizmo_extents", Variant::Float(10.0))),
        );
    }
    if !class_exists("MeshInstance2D") {
        register_class(
            ClassRegistration::new("MeshInstance2D")
                .parent("Node2D")
                .property(PropertyInfo::new("mesh", Variant::Nil))
                .property(PropertyInfo::new("texture", Variant::Nil))
                .signal(SignalInfo::new("texture_changed")),
        );
    }
    if !class_exists("MultiMeshInstance2D") {
        register_class(
            ClassRegistration::new("MultiMeshInstance2D")
                .parent("Node2D")
                .property(PropertyInfo::new("multimesh", Variant::Nil))
                .property(PropertyInfo::new("texture", Variant::Nil))
                .signal(SignalInfo::new("texture_changed")),
        );
    }
    // Composites its children as one group so blending is correct across them.
    if !class_exists("CanvasGroup") {
        register_class(
            ClassRegistration::new("CanvasGroup")
                .parent("Node2D")
                .property(PropertyInfo::new("fit_margin", Variant::Float(10.0)))
                .property(PropertyInfo::new("clear_margin", Variant::Float(10.0)))
                .property(PropertyInfo::new("use_mipmaps", Variant::Bool(false))),
        );
    }
    // Copies the framebuffer so a shader can read SCREEN_TEXTURE.
    if !class_exists("BackBufferCopy") {
        register_class(
            ClassRegistration::new("BackBufferCopy")
                .parent("Node2D")
                // Godot defaults to COPY_MODE_RECT (1), not disabled — a fresh
                // node copies its rect rather than doing nothing.
                .property(PropertyInfo::new("copy_mode", Variant::Int(1)))
                .property(PropertyInfo::new(
                    "rect",
                    Variant::Rect2(gdcore::math::Rect2::new(
                        gdcore::math::Vector2::new(-100.0, -100.0),
                        gdcore::math::Vector2::new(200.0, 200.0),
                    )),
                ))
                .enumeration(
                    EnumInfo::new("CopyMode")
                        .constant("COPY_MODE_DISABLED", 0)
                        .constant("COPY_MODE_RECT", 1)
                        .constant("COPY_MODE_VIEWPORT", 2),
                ),
        );
    }
    if !class_exists("TouchScreenButton") {
        register_class(
            ClassRegistration::new("TouchScreenButton")
                .parent("Node2D")
                .property(PropertyInfo::new("texture_normal", Variant::Nil))
                .property(PropertyInfo::new("texture_pressed", Variant::Nil))
                .property(PropertyInfo::new("bitmask", Variant::Nil))
                .property(PropertyInfo::new("shape", Variant::Nil))
                .property(PropertyInfo::new("shape_centered", Variant::Bool(true)))
                .property(PropertyInfo::new("shape_visible", Variant::Bool(true)))
                .property(PropertyInfo::new("passby_press", Variant::Bool(false)))
                // Godot types this StringName; carried as a String here.
                .property(PropertyInfo::new("action", Variant::String(String::new())))
                .property(PropertyInfo::new("visibility_mode", Variant::Int(0)))
                .enumeration(
                    EnumInfo::new("VisibilityMode")
                        .constant("VISIBILITY_ALWAYS", 0)
                        .constant("VISIBILITY_TOUCHSCREEN_ONLY", 1),
                )
                .signal(SignalInfo::new("pressed"))
                .signal(SignalInfo::new("released")),
        );
    }
    // Overrides where 2D audio is heard from. Godot gives it no properties —
    // it is driven entirely by `make_current()`/`clear_current()`, and asks with
    // `is_current()`; doc/classes/AudioListener2D.xml declares zero `<member>`s
    // and exactly those three methods.
    //
    // The runtime flag those methods flip is a plain node property the audio
    // bridge reads (`audio_server_bridge.rs:303`), not a declared class
    // property. Declaring it here made ClassDB advertise a `current` property
    // Godot does not have — caught by
    // `misc_canvas_nodes_register_and_wire::audio_listener_2d_has_no_properties_of_its_own`
    // (pat-0q2kx.106). Removing the declaration is safe because that read is
    // `matches!(…, Variant::Bool(true))`, so an unset flag is already false.
    if !class_exists("AudioListener2D") {
        register_class(
            ClassRegistration::new("AudioListener2D")
                .parent("Node2D")
                .method(MethodInfo::new("make_current", 0))
                .method(MethodInfo::new("clear_current", 0))
                .method(MethodInfo::new("is_current", 0)),
        );
    }
    if !class_exists("GPUParticles2D") {
        // GPUParticles2D: 26 own properties from doc/classes/GPUParticles2D.xml.
        register_class(
            ClassRegistration::new("GPUParticles2D")
                .parent("Node2D")
                .property(PropertyInfo::new("amount", Variant::Int(8)))
                .property(PropertyInfo::new("amount_ratio", Variant::Float(1.0)))
                .property(PropertyInfo::new(
                    "collision_base_size",
                    Variant::Float(1.0),
                ))
                .property(PropertyInfo::new("draw_order", Variant::Int(1)))
                .property(PropertyInfo::new("emitting", Variant::Bool(true)))
                .property(PropertyInfo::new("explosiveness", Variant::Float(0.0)))
                .property(PropertyInfo::new("fixed_fps", Variant::Int(30)))
                .property(PropertyInfo::new("fract_delta", Variant::Bool(true)))
                .property(PropertyInfo::new("interp_to_end", Variant::Float(0.0)))
                .property(PropertyInfo::new("interpolate", Variant::Bool(true)))
                .property(PropertyInfo::new("lifetime", Variant::Float(1.0)))
                .property(PropertyInfo::new("local_coords", Variant::Bool(false)))
                .property(PropertyInfo::new("one_shot", Variant::Bool(false)))
                .property(PropertyInfo::new("preprocess", Variant::Float(0.0)))
                .property(PropertyInfo::new("process_material", Variant::Nil))
                .property(PropertyInfo::new("randomness", Variant::Float(0.0)))
                .property(PropertyInfo::new("seed", Variant::Int(0)))
                .property(PropertyInfo::new("speed_scale", Variant::Float(1.0)))
                .property(PropertyInfo::new(
                    "sub_emitter",
                    Variant::NodePath(gdcore::node_path::NodePath::new("")),
                ))
                .property(PropertyInfo::new("texture", Variant::Nil))
                .property(PropertyInfo::new("trail_enabled", Variant::Bool(false)))
                .property(PropertyInfo::new("trail_lifetime", Variant::Float(0.3)))
                .property(PropertyInfo::new(
                    "trail_section_subdivisions",
                    Variant::Int(4),
                ))
                .property(PropertyInfo::new("trail_sections", Variant::Int(8)))
                .property(PropertyInfo::new("use_fixed_seed", Variant::Bool(false)))
                .property(PropertyInfo::new(
                    "visibility_rect",
                    Variant::Rect2(gdcore::math::Rect2::new(
                        Vector2::new(-100.0, -100.0),
                        Vector2::new(200.0, 200.0),
                    )),
                )),
        );
    }
    if !class_exists("CPUParticles2D") {
        // CPUParticles2D: 70 own properties from doc/classes/CPUParticles2D.xml.
        register_class(
            ClassRegistration::new("CPUParticles2D")
                .parent("Node2D")
                .property(PropertyInfo::new("amount", Variant::Int(8)))
                .property(PropertyInfo::new("angle_curve", Variant::Nil))
                .property(PropertyInfo::new("angle_max", Variant::Float(0.0)))
                .property(PropertyInfo::new("angle_min", Variant::Float(0.0)))
                .property(PropertyInfo::new("angular_velocity_curve", Variant::Nil))
                .property(PropertyInfo::new(
                    "angular_velocity_max",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new(
                    "angular_velocity_min",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new("anim_offset_curve", Variant::Nil))
                .property(PropertyInfo::new("anim_offset_max", Variant::Float(0.0)))
                .property(PropertyInfo::new("anim_offset_min", Variant::Float(0.0)))
                .property(PropertyInfo::new("anim_speed_curve", Variant::Nil))
                .property(PropertyInfo::new("anim_speed_max", Variant::Float(0.0)))
                .property(PropertyInfo::new("anim_speed_min", Variant::Float(0.0)))
                .property(PropertyInfo::new(
                    "color",
                    Variant::Color(gdcore::math::Color::new(1.0, 1.0, 1.0, 1.0)),
                ))
                .property(PropertyInfo::new("color_initial_ramp", Variant::Nil))
                .property(PropertyInfo::new("color_ramp", Variant::Nil))
                .property(PropertyInfo::new("damping_curve", Variant::Nil))
                .property(PropertyInfo::new("damping_max", Variant::Float(0.0)))
                .property(PropertyInfo::new("damping_min", Variant::Float(0.0)))
                .property(PropertyInfo::new(
                    "direction",
                    Variant::Vector2(Vector2::new(1.0, 0.0)),
                ))
                .property(PropertyInfo::new("draw_order", Variant::Int(0)))
                .property(PropertyInfo::new(
                    "emission_colors",
                    Variant::PackedColorArray(Vec::new()),
                ))
                .property(PropertyInfo::new(
                    "emission_normals",
                    Variant::PackedVector2Array(Vec::new()),
                ))
                .property(PropertyInfo::new(
                    "emission_points",
                    Variant::PackedVector2Array(Vec::new()),
                ))
                .property(PropertyInfo::new(
                    "emission_rect_extents",
                    Variant::Vector2(Vector2::new(0.0, 0.0)),
                ))
                .property(PropertyInfo::new(
                    "emission_ring_inner_radius",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new(
                    "emission_ring_radius",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new("emission_shape", Variant::Int(0)))
                .property(PropertyInfo::new(
                    "emission_sphere_radius",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new("emitting", Variant::Bool(true)))
                .property(PropertyInfo::new("explosiveness", Variant::Float(0.0)))
                .property(PropertyInfo::new("fixed_fps", Variant::Int(0)))
                .property(PropertyInfo::new("fract_delta", Variant::Bool(true)))
                .property(PropertyInfo::new(
                    "gravity",
                    Variant::Vector2(Vector2::new(0.0, 980.0)),
                ))
                .property(PropertyInfo::new("hue_variation_curve", Variant::Nil))
                .property(PropertyInfo::new("hue_variation_max", Variant::Float(0.0)))
                .property(PropertyInfo::new("hue_variation_min", Variant::Float(0.0)))
                .property(PropertyInfo::new(
                    "initial_velocity_max",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new(
                    "initial_velocity_min",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new("lifetime", Variant::Float(1.0)))
                .property(PropertyInfo::new(
                    "lifetime_randomness",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new("linear_accel_curve", Variant::Nil))
                .property(PropertyInfo::new("linear_accel_max", Variant::Float(0.0)))
                .property(PropertyInfo::new("linear_accel_min", Variant::Float(0.0)))
                .property(PropertyInfo::new("local_coords", Variant::Bool(false)))
                .property(PropertyInfo::new("one_shot", Variant::Bool(false)))
                .property(PropertyInfo::new("orbit_velocity_curve", Variant::Nil))
                .property(PropertyInfo::new("orbit_velocity_max", Variant::Float(0.0)))
                .property(PropertyInfo::new("orbit_velocity_min", Variant::Float(0.0)))
                .property(PropertyInfo::new(
                    "particle_flag_align_y",
                    Variant::Bool(false),
                ))
                .property(PropertyInfo::new(
                    "physics_interpolation_mode",
                    Variant::Int(2),
                ))
                .property(PropertyInfo::new("preprocess", Variant::Float(0.0)))
                .property(PropertyInfo::new("radial_accel_curve", Variant::Nil))
                .property(PropertyInfo::new("radial_accel_max", Variant::Float(0.0)))
                .property(PropertyInfo::new("radial_accel_min", Variant::Float(0.0)))
                .property(PropertyInfo::new("randomness", Variant::Float(0.0)))
                .property(PropertyInfo::new("scale_amount_curve", Variant::Nil))
                .property(PropertyInfo::new("scale_amount_max", Variant::Float(1.0)))
                .property(PropertyInfo::new("scale_amount_min", Variant::Float(1.0)))
                .property(PropertyInfo::new("scale_curve_x", Variant::Nil))
                .property(PropertyInfo::new("scale_curve_y", Variant::Nil))
                .property(PropertyInfo::new("seed", Variant::Int(0)))
                .property(PropertyInfo::new("speed_scale", Variant::Float(1.0)))
                .property(PropertyInfo::new("split_scale", Variant::Bool(false)))
                .property(PropertyInfo::new("spread", Variant::Float(45.0)))
                .property(PropertyInfo::new("tangential_accel_curve", Variant::Nil))
                .property(PropertyInfo::new(
                    "tangential_accel_max",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new(
                    "tangential_accel_min",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new("texture", Variant::Nil))
                .property(PropertyInfo::new("use_fixed_seed", Variant::Bool(false))),
        );
    }
    if !class_exists("Line2D") {
        // Line2D: 14 own properties from doc/classes/Line2D.xml.
        register_class(
            ClassRegistration::new("Line2D")
                .parent("Node2D")
                .property(PropertyInfo::new("antialiased", Variant::Bool(false)))
                .property(PropertyInfo::new("begin_cap_mode", Variant::Int(0)))
                .property(PropertyInfo::new("closed", Variant::Bool(false)))
                .property(PropertyInfo::new(
                    "default_color",
                    Variant::Color(gdcore::math::Color::new(1.0, 1.0, 1.0, 1.0)),
                ))
                .property(PropertyInfo::new("end_cap_mode", Variant::Int(0)))
                .property(PropertyInfo::new("gradient", Variant::Nil))
                .property(PropertyInfo::new("joint_mode", Variant::Int(0)))
                .property(PropertyInfo::new(
                    "points",
                    Variant::PackedVector2Array(Vec::new()),
                ))
                .property(PropertyInfo::new("round_precision", Variant::Int(8)))
                .property(PropertyInfo::new("sharp_limit", Variant::Float(2.0)))
                .property(PropertyInfo::new("texture", Variant::Nil))
                .property(PropertyInfo::new("texture_mode", Variant::Int(0)))
                .property(PropertyInfo::new("width", Variant::Float(10.0)))
                .property(PropertyInfo::new("width_curve", Variant::Nil)),
        );
    }
    if !class_exists("Polygon2D") {
        // Polygon2D: 15 own properties from doc/classes/Polygon2D.xml.
        register_class(
            ClassRegistration::new("Polygon2D")
                .parent("Node2D")
                .property(PropertyInfo::new("antialiased", Variant::Bool(false)))
                .property(PropertyInfo::new(
                    "color",
                    Variant::Color(gdcore::math::Color::new(1.0, 1.0, 1.0, 1.0)),
                ))
                .property(PropertyInfo::new("internal_vertex_count", Variant::Int(0)))
                .property(PropertyInfo::new("invert_border", Variant::Float(100.0)))
                .property(PropertyInfo::new("invert_enabled", Variant::Bool(false)))
                .property(PropertyInfo::new(
                    "offset",
                    Variant::Vector2(Vector2::new(0.0, 0.0)),
                ))
                .property(PropertyInfo::new(
                    "polygon",
                    Variant::PackedVector2Array(Vec::new()),
                ))
                .property(PropertyInfo::new("polygons", Variant::Array(Vec::new())))
                .property(PropertyInfo::new(
                    "skeleton",
                    Variant::NodePath(gdcore::node_path::NodePath::new("")),
                ))
                .property(PropertyInfo::new("texture", Variant::Nil))
                .property(PropertyInfo::new(
                    "texture_offset",
                    Variant::Vector2(Vector2::new(0.0, 0.0)),
                ))
                .property(PropertyInfo::new("texture_rotation", Variant::Float(0.0)))
                .property(PropertyInfo::new(
                    "texture_scale",
                    Variant::Vector2(Vector2::new(1.0, 1.0)),
                ))
                .property(PropertyInfo::new(
                    "uv",
                    Variant::PackedVector2Array(Vec::new()),
                ))
                .property(PropertyInfo::new(
                    "vertex_colors",
                    Variant::PackedColorArray(Vec::new()),
                )),
        );
    }
    // Skeleton2D / Bone2D — 2D skeletal deform for Polygon2D skinning
    // (pat-u3l8c.100). Both derive from Node2D in Godot's hierarchy.
    if !class_exists("Skeleton2D") {
        // Godot documents no own properties for Skeleton2D; its surface is
        // entirely inherited (pat-0q2kx.127).
        register_class(ClassRegistration::new("Skeleton2D").parent("Node2D"));
    }
    if !class_exists("Bone2D") {
        // Bone2D: 1 own properties from doc/classes/Bone2D.xml.
        register_class(ClassRegistration::new("Bone2D").parent("Node2D").property(
            PropertyInfo::new(
                "rest",
                Variant::Transform2D(gdcore::math::Transform2D::IDENTITY),
            ),
        ));
    }
    if !class_exists("Path2D") {
        // Path2D: 1 own properties from doc/classes/Path2D.xml.
        register_class(
            ClassRegistration::new("Path2D")
                .parent("Node2D")
                .property(PropertyInfo::new("curve", Variant::Nil)),
        );
    }
    if !class_exists("PathFollow2D") {
        // PathFollow2D: 7 own properties from doc/classes/PathFollow2D.xml.
        register_class(
            ClassRegistration::new("PathFollow2D")
                .parent("Node2D")
                .property(PropertyInfo::new("cubic_interp", Variant::Bool(true)))
                .property(PropertyInfo::new("h_offset", Variant::Float(0.0)))
                .property(PropertyInfo::new("loop", Variant::Bool(true)))
                .property(PropertyInfo::new("progress", Variant::Float(0.0)))
                .property(PropertyInfo::new("progress_ratio", Variant::Float(0.0)))
                .property(PropertyInfo::new("rotates", Variant::Bool(true)))
                .property(PropertyInfo::new("v_offset", Variant::Float(0.0))),
        );
    }
    // 2D lights + occluder (pat-p0s82.6)
    if !class_exists("Path3D") {
        register_class(ClassRegistration::new("Path3D").parent("Node3D"));
    }
    if !class_exists("PathFollow3D") {
        register_class(ClassRegistration::new("PathFollow3D").parent("Node3D"));
    }
    // GridMap (pat-nzgdu.193) — was registered bare (`.parent("Node3D")` and
    // nothing else), so the inspector had nothing to show and `set_cell_item`
    // was invisible to ClassDB even though gdscene/src/grid_map.rs implements it.
    if !class_exists("GridMap") {
        register_class(
            ClassRegistration::new("GridMap")
                .parent("Node3D")
                // The palette of meshes a cell can hold — what the editor tool
                // shows and what `set_cell_item`'s id indexes into.
                .property(PropertyInfo::new("mesh_library", Variant::Nil))
                .property(PropertyInfo::new("physics_material", Variant::Nil))
                // ⚠️ Godot's default is Vector3(2, 2, 2), NOT ones. A 1×1×1
                // default would misplace every painted cell by half a cell and
                // read as a broken grid rather than a wrong constant.
                .property(PropertyInfo::new(
                    "cell_size",
                    Variant::Vector3(gdcore::math::Vector3::new(2.0, 2.0, 2.0)),
                ))
                .property(PropertyInfo::new("cell_octant_size", Variant::Int(8)))
                // Cells are centred on all three axes by default.
                .property(PropertyInfo::new("cell_center_x", Variant::Bool(true)))
                .property(PropertyInfo::new("cell_center_y", Variant::Bool(true)))
                .property(PropertyInfo::new("cell_center_z", Variant::Bool(true)))
                .property(PropertyInfo::new("cell_scale", Variant::Float(1.0)))
                .property(PropertyInfo::new("collision_layer", Variant::Int(1)))
                .property(PropertyInfo::new("collision_mask", Variant::Int(1)))
                .property(PropertyInfo::new("collision_priority", Variant::Float(1.0)))
                .property(PropertyInfo::new("bake_navigation", Variant::Bool(false)))
                // The cell API the editor tool drives. `set_cell_item` takes 2
                // required args (position, item); orientation defaults.
                .method(MethodInfo::new("set_cell_item", 2))
                .method(MethodInfo::new("get_cell_item", 1))
                .method(MethodInfo::new("get_cell_item_orientation", 1))
                .method(MethodInfo::new("get_cell_item_basis", 1))
                // Viewport click -> cell, and back. Without these a paint tool
                // cannot turn a ray hit into a cell coordinate.
                .method(MethodInfo::new("local_to_map", 1))
                .method(MethodInfo::new("map_to_local", 1))
                .method(MethodInfo::new("get_used_cells", 0))
                .method(MethodInfo::new("get_used_cells_by_item", 1))
                .method(MethodInfo::new("clear", 0))
                .method(MethodInfo::new("clear_baked_meshes", 0))
                .method(MethodInfo::new("set_mesh_library", 1))
                .method(MethodInfo::new("get_mesh_library", 0))
                .method(MethodInfo::new("set_cell_size", 1))
                .method(MethodInfo::new("get_cell_size", 0))
                .method(MethodInfo::new("set_cell_scale", 1))
                .method(MethodInfo::new("get_cell_scale", 0))
                .signal(SignalInfo::new("cell_size_changed"))
                .signal(SignalInfo::new("changed")),
        );
    }
    // The mesh palette itself (pat-nzgdu.193). Declares NO properties — its
    // whole surface is the indexed item API, which is what a palette dock reads.
    if !class_exists("MeshLibrary") {
        register_class(
            ClassRegistration::new("MeshLibrary")
                .parent("Resource")
                .method(MethodInfo::new("create_item", 1))
                .method(MethodInfo::new("remove_item", 1))
                .method(MethodInfo::new("clear", 0))
                .method(MethodInfo::new("get_item_list", 0))
                .method(MethodInfo::new("find_item_by_name", 1))
                // Godot hands out ids; a palette needs this to add one.
                .method(MethodInfo::new("get_last_unused_item_id", 0))
                .method(MethodInfo::new("set_item_name", 2))
                .method(MethodInfo::new("get_item_name", 1))
                .method(MethodInfo::new("set_item_mesh", 2))
                .method(MethodInfo::new("get_item_mesh", 1))
                .method(MethodInfo::new("set_item_mesh_transform", 2))
                .method(MethodInfo::new("get_item_mesh_transform", 1))
                .method(MethodInfo::new("set_item_mesh_cast_shadow", 2))
                .method(MethodInfo::new("get_item_mesh_cast_shadow", 1))
                .method(MethodInfo::new("set_item_navigation_mesh", 2))
                .method(MethodInfo::new("get_item_navigation_mesh", 1))
                .method(MethodInfo::new("set_item_navigation_mesh_transform", 2))
                .method(MethodInfo::new("get_item_navigation_mesh_transform", 1))
                .method(MethodInfo::new("set_item_navigation_layers", 2))
                .method(MethodInfo::new("get_item_navigation_layers", 1))
                .method(MethodInfo::new("set_item_shapes", 2))
                .method(MethodInfo::new("get_item_shapes", 1))
                // The thumbnail the palette dock draws.
                .method(MethodInfo::new("set_item_preview", 2))
                .method(MethodInfo::new("get_item_preview", 1)),
        );
    }
    // NavigationLink2D/3D — off-mesh connections between two points on the
    // navigation map (pat-0q2kx.98). Defaults from
    // doc/classes/NavigationLink{2D,3D}.xml. Both were registered as bare
    // classes, so a `.tscn` link's authored `start_position`/`enabled`/… had no
    // schema to land on and the runtime gather had nothing to read.
    if !class_exists("NavigationLink3D") {
        register_class(
            ClassRegistration::new("NavigationLink3D")
                .parent("Node3D")
                .property(PropertyInfo::new("enabled", Variant::Bool(true)))
                .property(PropertyInfo::new("bidirectional", Variant::Bool(true)))
                .property(PropertyInfo::new(
                    "start_position",
                    Variant::Vector3(gdcore::math::Vector3::ZERO),
                ))
                .property(PropertyInfo::new(
                    "end_position",
                    Variant::Vector3(gdcore::math::Vector3::ZERO),
                ))
                .property(PropertyInfo::new("navigation_layers", Variant::Int(1)))
                .property(PropertyInfo::new("enter_cost", Variant::Float(0.0)))
                .property(PropertyInfo::new("travel_cost", Variant::Float(1.0))),
        );
    }
    if !class_exists("NavigationLink2D") {
        register_class(
            ClassRegistration::new("NavigationLink2D")
                .parent("Node2D")
                .property(PropertyInfo::new("enabled", Variant::Bool(true)))
                .property(PropertyInfo::new("bidirectional", Variant::Bool(true)))
                .property(PropertyInfo::new(
                    "start_position",
                    Variant::Vector2(gdcore::math::Vector2::ZERO),
                ))
                .property(PropertyInfo::new(
                    "end_position",
                    Variant::Vector2(gdcore::math::Vector2::ZERO),
                ))
                .property(PropertyInfo::new("navigation_layers", Variant::Int(1)))
                .property(PropertyInfo::new("enter_cost", Variant::Float(0.0)))
                .property(PropertyInfo::new("travel_cost", Variant::Float(1.0))),
        );
    }
    if !class_exists("RemoteTransform3D") {
        register_class(ClassRegistration::new("RemoteTransform3D").parent("Node3D"));
    }
    if !class_exists("RemoteTransform2D") {
        // RemoteTransform2D: 5 own properties from doc/classes/RemoteTransform2D.xml.
        register_class(
            ClassRegistration::new("RemoteTransform2D")
                .parent("Node2D")
                .property(PropertyInfo::new(
                    "remote_path",
                    Variant::NodePath(gdcore::node_path::NodePath::new("")),
                ))
                .property(PropertyInfo::new("update_position", Variant::Bool(true)))
                .property(PropertyInfo::new("update_rotation", Variant::Bool(true)))
                .property(PropertyInfo::new("update_scale", Variant::Bool(true)))
                .property(PropertyInfo::new(
                    "use_global_coordinates",
                    Variant::Bool(true),
                )),
        );
    }
    // These two 3D nodes are registered from the 2D table, so the bases they
    // derive from must exist here too — `inheritance_chain` resolves parents BY
    // NAME at query time and stops dead at the first unregistered link, so
    // pointing them at a base this entry point never registers would sever
    // `is_parent_class(Label3D, ..)` for anyone who calls only
    // `register_2d_classes()` (pat-0q2kx.63).
    //
    // Bare bootstraps: `register_3d_classes` registers these two unguarded with
    // their full property sets, and `register_class` is last-wins, so calling
    // 2D-then-3D still ends up with the complete definitions.
    if !class_exists("VisualInstance3D") {
        register_class(
            ClassRegistration::new("VisualInstance3D")
                .parent("Node3D")
                .abstract_class(),
        );
    }
    if !class_exists("GeometryInstance3D") {
        register_class(
            ClassRegistration::new("GeometryInstance3D")
                .parent("VisualInstance3D")
                .abstract_class(),
        );
    }
    if !class_exists("VisibleOnScreenNotifier3D") {
        register_class(
            ClassRegistration::new("VisibleOnScreenNotifier3D").parent("VisualInstance3D"),
        );
    }
    // pat-0q2kx.109: Label3D was a bare parent-only registration with no
    // properties, so the inspector showed nothing to edit and a `.tscn`
    // `text = "..."` had no declared property to land on.
    //
    // ⚠️ Defaults come from doc/classes/Label3D.xml, and several are NOT the
    // zero value: `pixel_size` is 0.005, `font_size` 16, `modulate` white, and
    // `alpha_cut` 0 = DISABLED. Registering them as zero/black would make every
    // fresh Label3D invisible.
    if !class_exists("Label3D") {
        register_class(
            ClassRegistration::new("Label3D")
                .parent("GeometryInstance3D")
                .property(PropertyInfo::new("text", Variant::String(String::new())))
                .property(PropertyInfo::new("font_size", Variant::Int(16)))
                .property(PropertyInfo::new("pixel_size", Variant::Float(0.005)))
                .property(PropertyInfo::new(
                    "modulate",
                    Variant::Color(gdcore::math::Color::WHITE),
                ))
                .property(PropertyInfo::new(
                    "outline_modulate",
                    Variant::Color(gdcore::math::Color::new(0.0, 0.0, 0.0, 1.0)),
                ))
                .property(PropertyInfo::new("outline_size", Variant::Int(12)))
                .property(PropertyInfo::new("billboard", Variant::Int(0)))
                .property(PropertyInfo::new("alpha_cut", Variant::Int(0)))
                .property(PropertyInfo::new("horizontal_alignment", Variant::Int(1)))
                .property(PropertyInfo::new("vertical_alignment", Variant::Int(1)))
                .property(PropertyInfo::new("autowrap_mode", Variant::Int(0)))
                .property(PropertyInfo::new("width", Variant::Float(500.0)))
                .property(PropertyInfo::new("double_sided", Variant::Bool(true)))
                .property(PropertyInfo::new("no_depth_test", Variant::Bool(false)))
                .property(PropertyInfo::new("fixed_size", Variant::Bool(false))),
        );
    }
    if !class_exists("RichTextLabel") {
        register_class(ClassRegistration::new("RichTextLabel").parent("Control"));
    }

    // -- RichTextEffect / CharFXTransform (pat-nzgdu.157) -------------------
    // Godot's custom BBCode fx pair: a `RichTextEffect` resource declares a
    // `bbcode` tag and implements `_process_custom_fx(char_fx)`, which
    // RichTextLabel calls once per covered glyph with a `CharFXTransform`
    // carrying that glyph's mutable offset/color plus its index and the
    // effect's elapsed time.
    //
    // NOTE: this is the *reflection* surface only. The built-in effects are
    // evaluated natively (`gdscene::char_fx`); `_process_custom_fx` is never
    // dispatched, because RichTextLabel has no `custom_effects` property, the
    // render pass holds `&SceneTree` (a script call needs `&mut`), and
    // CharFXTransform has no Variant encoding to cross the call boundary. See
    // `gdscene::char_fx`'s module docs for the full list — it is its own bead.
    //
    // `RefCounted` is not bootstrapped anywhere else in this function, and an
    // unregistered base silently severs the whole chain from `Object`
    // (pat-nzgdu.98), so register it before anything parents to it.
    if !class_exists("RefCounted") {
        register_class(ClassRegistration::new("RefCounted").parent("Object"));
    }
    if !class_exists("RichTextEffect") {
        register_class(
            ClassRegistration::new("RichTextEffect")
                .parent("Resource")
                // Reads the `bbcode` tag the effect answers to. On a scripted
                // effect Godot reads the script's own `bbcode` member.
                .method(MethodInfo::new("get_bbcode", 0))
                // The virtual a script overrides; returns false to hide the glyph.
                .method(MethodInfo::new("_process_custom_fx", 1)),
        );
    }
    if !class_exists("CharFXTransform") {
        register_class(
            ClassRegistration::new("CharFXTransform")
                .parent("RefCounted")
                // The glyph range this effect covers, as [start, end).
                .property(PropertyInfo::new("range", Variant::Nil))
                // Seconds since the label started animating this effect.
                .property(PropertyInfo::new("elapsed_time", Variant::Float(0.0)))
                // Cleared by an effect to skip drawing the glyph.
                .property(PropertyInfo::new("visible", Variant::Bool(true)))
                .property(PropertyInfo::new("outline", Variant::Bool(false)))
                // The mutable per-glyph pixel offset — what `[wave]` writes.
                .property(PropertyInfo::new("offset", Variant::Nil))
                .property(PropertyInfo::new("color", Variant::Nil))
                // `[ghost speed=2]` arrives here as {"speed": 2}.
                .property(PropertyInfo::new("env", Variant::Nil))
                .property(PropertyInfo::new("glyph_index", Variant::Int(0)))
                .property(PropertyInfo::new("glyph_count", Variant::Int(0)))
                // Index within the effect's own range (what effects animate on).
                .property(PropertyInfo::new("relative_index", Variant::Int(0)))
                .property(PropertyInfo::new("absolute_index", Variant::Int(0))),
        );
    }
    if !class_exists("ScrollContainer") {
        register_class(ClassRegistration::new("ScrollContainer").parent("Container"));
    }
    if !class_exists("TabContainer") {
        register_class(
            ClassRegistration::new("TabContainer")
                .parent("Container")
                .property(PropertyInfo::new("current_tab", Variant::Int(0)))
                .property(PropertyInfo::new("tabs_visible", Variant::Bool(true)))
                .signal(SignalInfo::new("tab_changed").arg("tab"))
                .signal(SignalInfo::new("tab_selected").arg("tab"))
                .signal(SignalInfo::new("tab_clicked").arg("tab")),
        );
    }
    // The item-view Controls (pat-0q2kx.85). Their item state lives in these
    // properties — `gdscene::item_views` is the model/script/click bridge that
    // reads and writes them — and `has_signal` has to answer for the signals
    // a click emits even with nothing connected yet.
    if !class_exists("ItemList") {
        register_class(
            ClassRegistration::new("ItemList")
                .parent("Control")
                .property(PropertyInfo::new("items", Variant::Array(Vec::new())))
                // Godot `SELECT_SINGLE = 0`.
                .property(PropertyInfo::new("select_mode", Variant::Int(0)))
                .property(PropertyInfo::new("allow_reselect", Variant::Bool(false)))
                .signal(SignalInfo::new("item_selected").arg("index"))
                .signal(SignalInfo::new("item_activated").arg("index"))
                .signal(
                    SignalInfo::new("item_clicked")
                        .arg("index")
                        .arg("at_position")
                        .arg("mouse_button_index"),
                )
                .signal(
                    SignalInfo::new("multi_selected")
                        .arg("index")
                        .arg("selected"),
                )
                .signal(
                    SignalInfo::new("empty_clicked")
                        .arg("at_position")
                        .arg("mouse_button_index"),
                ),
        );
    }
    if !class_exists("TabBar") {
        register_class(
            ClassRegistration::new("TabBar")
                .parent("Control")
                .property(PropertyInfo::new("tabs", Variant::Array(Vec::new())))
                .property(PropertyInfo::new("tab_count", Variant::Int(0)))
                // Godot's TabBar starts with no tabs, hence no current one.
                .property(PropertyInfo::new("current_tab", Variant::Int(-1)))
                .signal(SignalInfo::new("tab_changed").arg("tab"))
                .signal(SignalInfo::new("tab_selected").arg("tab"))
                .signal(SignalInfo::new("tab_clicked").arg("tab")),
        );
    }
    // Godot's GUI intermediates. Patina parented the leaves straight onto
    // Control/Container and never registered these, so `is_class("BaseButton")`
    // was false for a Button — which is exactly why `control.rs` carries a
    // hardcoded BUTTON_CLASSES fallback list (pat-0q2kx.62).
    //
    // ⚠️ These MUST be registered in the same entry point as the leaves below.
    // `inheritance_chain` resolves parents by name at query time and stops at
    // the first unregistered name, so re-parenting without the base present
    // would sever Button from Control and Object entirely — worse than the gap
    // being fixed, and invisible to `cargo check`.
    if !class_exists("BaseButton") {
        register_class(
            ClassRegistration::new("BaseButton")
                .parent("Control")
                .abstract_class()
                .property(PropertyInfo::new("disabled", Variant::Bool(false)))
                .property(PropertyInfo::new("toggle_mode", Variant::Bool(false)))
                .property(PropertyInfo::new("button_pressed", Variant::Bool(false)))
                // ACTION_MODE_BUTTON_RELEASE = 1 is Godot's default: a click
                // fires on release, not on press.
                .property(PropertyInfo::new("action_mode", Variant::Int(1)))
                .property(PropertyInfo::new("button_group", Variant::Nil))
                .signal(SignalInfo::new("pressed"))
                .signal(SignalInfo::new("button_down"))
                .signal(SignalInfo::new("button_up"))
                .signal(SignalInfo::new("toggled").arg("toggled_on")),
        );
    }
    if !class_exists("Range") {
        register_class(
            ClassRegistration::new("Range")
                .parent("Control")
                .abstract_class()
                .property(PropertyInfo::new("min_value", Variant::Float(0.0)))
                .property(PropertyInfo::new("max_value", Variant::Float(100.0)))
                .property(PropertyInfo::new("step", Variant::Float(0.01)))
                .property(PropertyInfo::new("page", Variant::Float(0.0)))
                .property(PropertyInfo::new("value", Variant::Float(0.0)))
                .property(PropertyInfo::new("ratio", Variant::Float(0.0)))
                .property(PropertyInfo::new("exp_edit", Variant::Bool(false)))
                .property(PropertyInfo::new("rounded", Variant::Bool(false)))
                .property(PropertyInfo::new("allow_greater", Variant::Bool(false)))
                .property(PropertyInfo::new("allow_lesser", Variant::Bool(false)))
                .signal(SignalInfo::new("value_changed").arg("value"))
                .signal(SignalInfo::new("changed")),
        );
    }
    if !class_exists("BoxContainer") {
        register_class(
            ClassRegistration::new("BoxContainer")
                .parent("Container")
                .abstract_class()
                // ALIGNMENT_BEGIN = 0
                .property(PropertyInfo::new("alignment", Variant::Int(0))),
        );
    }
    if !class_exists("ProgressBar") {
        register_class(ClassRegistration::new("ProgressBar").parent("Range"));
    }
    if !class_exists("CheckBox") {
        register_class(ClassRegistration::new("CheckBox").parent("Button"));
    }
    if !class_exists("OptionButton") {
        register_class(
            ClassRegistration::new("OptionButton")
                .parent("Button")
                .property(PropertyInfo::new("items", Variant::Array(Vec::new())))
                // Godot's `NONE_SELECTED = -1`.
                .property(PropertyInfo::new("selected", Variant::Int(-1)))
                .signal(SignalInfo::new("item_selected").arg("index"))
                .signal(SignalInfo::new("item_focused").arg("index")),
        );
    }
    if !class_exists("TextureRect") {
        register_class(ClassRegistration::new("TextureRect").parent("Control"));
    }
    // pat-p0s82.29: VideoStreamPlayer is a high-use Control node; register the
    // class + defaults for ClassDB parity (the video decode/playback path is a
    // separate media bead, pat-p0s82.28).
    if !class_exists("VideoStreamPlayer") {
        register_class(
            ClassRegistration::new("VideoStreamPlayer")
                .parent("Control")
                .property(PropertyInfo::new("autoplay", Variant::Bool(false)))
                .property(PropertyInfo::new("paused", Variant::Bool(false)))
                .property(PropertyInfo::new("volume_db", Variant::Float(0.0)))
                .property(PropertyInfo::new("expand", Variant::Bool(true))),
        );
    }
    if !class_exists("GridContainer") {
        register_class(ClassRegistration::new("GridContainer").parent("Container"));
    }
    if !class_exists("MarginContainer") {
        register_class(ClassRegistration::new("MarginContainer").parent("Container"));
    }
    // pat-nzgdu.109: the GraphEdit family. Hierarchy matches Godot 4.6:
    // GraphElement : Container, GraphNode/GraphFrame : GraphElement,
    // GraphEdit : Control. Behaviour lives in gdscene::graph_edit; drawing and
    // box-select/drag interaction are follow-up beads.
    if !class_exists("GraphElement") {
        register_class(
            ClassRegistration::new("GraphElement")
                .parent("Container")
                .property(PropertyInfo::new(
                    "position_offset",
                    Variant::Vector2(Vector2::ZERO),
                ))
                .property(PropertyInfo::new("resizable", Variant::Bool(false)))
                .property(PropertyInfo::new("draggable", Variant::Bool(true)))
                .property(PropertyInfo::new("selectable", Variant::Bool(true)))
                .property(PropertyInfo::new("selected", Variant::Bool(false))),
        );
    }
    if !class_exists("GraphNode") {
        register_class(
            ClassRegistration::new("GraphNode")
                .parent("GraphElement")
                .property(PropertyInfo::new("title", Variant::String(String::new()))),
        );
    }
    if !class_exists("GraphFrame") {
        register_class(
            ClassRegistration::new("GraphFrame")
                .parent("GraphElement")
                .property(PropertyInfo::new("title", Variant::String(String::new())))
                .property(PropertyInfo::new("autoshrink_enabled", Variant::Bool(true)))
                .property(PropertyInfo::new(
                    "tint_color_enabled",
                    Variant::Bool(false),
                ))
                .property(PropertyInfo::new(
                    "tint_color",
                    Variant::Color(gdcore::math::Color::new(1.0, 1.0, 1.0, 0.3)),
                )),
        );
    }
    if !class_exists("GraphEdit") {
        register_class(
            ClassRegistration::new("GraphEdit")
                .parent("Control")
                .property(PropertyInfo::new("zoom", Variant::Float(1.0)))
                .property(PropertyInfo::new("show_grid", Variant::Bool(true)))
                .property(PropertyInfo::new("minimap_enabled", Variant::Bool(true)))
                .property(PropertyInfo::new("right_disconnects", Variant::Bool(false))),
        );
    }
    // Every audio effect derives from `AudioEffect`, not straight from
    // `Resource` (`servers/audio/audio_effect.h`; extension_api has
    // `AudioEffect <- Resource`). Patina skipped the base entirely, so anything
    // filtering by "inherits AudioEffect" — the AudioServer's effect-slot type
    // check, the editor's audio-bus effect picker — matched nothing
    // (pat-0q2kx.61).
    //
    // ⚠️ Registering the base HERE is load-bearing, not tidiness.
    // `inheritance_chain` resolves parents by name at query time and stops dead
    // at the first name missing from the map, so re-parenting the three effects
    // without registering `AudioEffect` in this same entry point would sever
    // them from `Resource` and `Object` — turning a missing link into a broken
    // chain, which is strictly worse. `register_2d_classes` is the ONLY
    // function that registers these three (verified structurally), and it
    // already bootstraps Object/RefCounted/Resource above.
    if !class_exists("AudioEffect") {
        register_class(
            ClassRegistration::new("AudioEffect")
                .parent("Resource")
                .abstract_class(),
        );
    }
    // Godot's `AudioEffectInstance` is the per-playback companion and derives
    // from RefCounted, NOT from AudioEffect (audio_effect.h) — registering it
    // under AudioEffect would make the effect picker offer it as an insert.
    if !class_exists("AudioEffectInstance") {
        register_class(
            ClassRegistration::new("AudioEffectInstance")
                .parent("RefCounted")
                .abstract_class(),
        );
    }
    if !class_exists("AudioEffectAmplify") {
        register_class(ClassRegistration::new("AudioEffectAmplify").parent("AudioEffect"));
    }
    if !class_exists("AudioEffectReverb") {
        register_class(ClassRegistration::new("AudioEffectReverb").parent("AudioEffect"));
    }
    if !class_exists("AudioEffectEQ") {
        register_class(ClassRegistration::new("AudioEffectEQ").parent("AudioEffect"));
    }

    if !class_exists("Light2D") {
        // Light2D: 15 own properties from doc/classes/Light2D.xml.
        register_class(
            ClassRegistration::new("Light2D")
                .parent("Node2D")
                .property(PropertyInfo::new("blend_mode", Variant::Int(0)))
                .property(PropertyInfo::new(
                    "color",
                    Variant::Color(gdcore::math::Color::new(1.0, 1.0, 1.0, 1.0)),
                ))
                .property(PropertyInfo::new("editor_only", Variant::Bool(false)))
                .property(PropertyInfo::new("enabled", Variant::Bool(true)))
                .property(PropertyInfo::new("energy", Variant::Float(1.0)))
                .property(PropertyInfo::new("range_item_cull_mask", Variant::Int(1)))
                .property(PropertyInfo::new("range_layer_max", Variant::Int(0)))
                .property(PropertyInfo::new("range_layer_min", Variant::Int(0)))
                .property(PropertyInfo::new("range_z_max", Variant::Int(1024)))
                .property(PropertyInfo::new("range_z_min", Variant::Int(-1024)))
                .property(PropertyInfo::new(
                    "shadow_color",
                    Variant::Color(gdcore::math::Color::new(0.0, 0.0, 0.0, 0.0)),
                ))
                .property(PropertyInfo::new("shadow_enabled", Variant::Bool(false)))
                .property(PropertyInfo::new("shadow_filter", Variant::Int(0)))
                .property(PropertyInfo::new(
                    "shadow_filter_smooth",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new("shadow_item_cull_mask", Variant::Int(1))),
        );
    }
    if !class_exists("PointLight2D") {
        // PointLight2D: 4 own properties from doc/classes/PointLight2D.xml.
        register_class(
            ClassRegistration::new("PointLight2D")
                .parent("Light2D")
                .property(PropertyInfo::new("height", Variant::Float(0.0)))
                .property(PropertyInfo::new(
                    "offset",
                    Variant::Vector2(Vector2::new(0.0, 0.0)),
                ))
                .property(PropertyInfo::new("texture", Variant::Nil))
                .property(PropertyInfo::new("texture_scale", Variant::Float(1.0))),
        );
    }
    if !class_exists("DirectionalLight2D") {
        // DirectionalLight2D: 2 own properties from doc/classes/DirectionalLight2D.xml.
        register_class(
            ClassRegistration::new("DirectionalLight2D")
                .parent("Light2D")
                .property(PropertyInfo::new("height", Variant::Float(0.0)))
                .property(PropertyInfo::new("max_distance", Variant::Float(10000.0))),
        );
    }
    if !class_exists("LightOccluder2D") {
        // LightOccluder2D: 3 own properties from doc/classes/LightOccluder2D.xml.
        register_class(
            ClassRegistration::new("LightOccluder2D")
                .parent("Node2D")
                .property(PropertyInfo::new("occluder", Variant::Nil))
                .property(PropertyInfo::new("occluder_light_mask", Variant::Int(1)))
                .property(PropertyInfo::new("sdf_collision", Variant::Bool(true))),
        );
    }
    if !class_exists("NavigationRegion2D") {
        // Mirrors NavigationRegion3D above; defaults from
        // doc/classes/NavigationRegion2D.xml (pat-0q2kx.26). The 2D resource slot
        // is `navigation_polygon`, not `navigation_mesh`.
        register_class(
            ClassRegistration::new("NavigationRegion2D")
                .parent("Node2D")
                .property(PropertyInfo::new("navigation_polygon", Variant::Nil))
                .property(PropertyInfo::new("enabled", Variant::Bool(true)))
                .property(PropertyInfo::new("navigation_layers", Variant::Int(1)))
                .property(PropertyInfo::new("enter_cost", Variant::Float(0.0)))
                .property(PropertyInfo::new("travel_cost", Variant::Float(1.0)))
                .property(PropertyInfo::new(
                    "use_edge_connections",
                    Variant::Bool(true),
                )),
        );
    }
    // NavigationAgent2D — scene-tree pathfinding agent (pat-rzs4o).
    if !class_exists("NavigationAgent2D") {
        register_class(
            ClassRegistration::new("NavigationAgent2D")
                .parent("Node")
                .property(PropertyInfo::new(
                    "target_position",
                    Variant::Vector2(gdcore::math::Vector2::ZERO),
                ))
                .property(PropertyInfo::new(
                    "path_desired_distance",
                    Variant::Float(4.0),
                ))
                .property(PropertyInfo::new("avoidance_enabled", Variant::Bool(false)))
                // pat-nzgdu.10 — full NavigationAgent2D property surface (Godot parity).
                .property(PropertyInfo::new(
                    "target_desired_distance",
                    Variant::Float(10.0),
                ))
                .property(PropertyInfo::new(
                    "path_max_distance",
                    Variant::Float(100.0),
                ))
                .property(PropertyInfo::new("navigation_layers", Variant::Int(1)))
                .property(PropertyInfo::new("pathfinding_algorithm", Variant::Int(0)))
                .property(PropertyInfo::new("path_postprocessing", Variant::Int(0)))
                .property(PropertyInfo::new("path_metadata_flags", Variant::Int(7)))
                .property(PropertyInfo::new("simplify_path", Variant::Bool(false)))
                .property(PropertyInfo::new("simplify_epsilon", Variant::Float(0.0)))
                .property(PropertyInfo::new(
                    "path_return_max_length",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new(
                    "path_return_max_radius",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new(
                    "path_search_max_polygons",
                    Variant::Int(4096),
                ))
                .property(PropertyInfo::new(
                    "path_search_max_distance",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new(
                    "velocity",
                    Variant::Vector2(gdcore::math::Vector2::ZERO),
                ))
                // pat-nzgdu.116 — the avoidance solve's output: the
                // collision-free velocity published each physics tick and
                // handed to `velocity_computed`.
                .property(PropertyInfo::new(
                    "safe_velocity",
                    Variant::Vector2(gdcore::math::Vector2::ZERO),
                ))
                .property(PropertyInfo::new("radius", Variant::Float(10.0)))
                .property(PropertyInfo::new(
                    "neighbor_distance",
                    Variant::Float(500.0),
                ))
                .property(PropertyInfo::new("max_neighbors", Variant::Int(10)))
                .property(PropertyInfo::new(
                    "time_horizon_agents",
                    Variant::Float(1.0),
                ))
                .property(PropertyInfo::new(
                    "time_horizon_obstacles",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new("max_speed", Variant::Float(100.0)))
                .property(PropertyInfo::new("avoidance_layers", Variant::Int(1)))
                .property(PropertyInfo::new("avoidance_mask", Variant::Int(1)))
                .property(PropertyInfo::new("avoidance_priority", Variant::Float(1.0)))
                .property(PropertyInfo::new("debug_enabled", Variant::Bool(false)))
                .property(PropertyInfo::new("debug_use_custom", Variant::Bool(false)))
                .property(PropertyInfo::new(
                    "debug_path_custom_color",
                    Variant::Color(gdcore::math::Color::new(1.0, 1.0, 1.0, 1.0)),
                ))
                .property(PropertyInfo::new(
                    "debug_path_custom_point_size",
                    Variant::Float(4.0),
                ))
                .property(PropertyInfo::new(
                    "debug_path_custom_line_width",
                    Variant::Float(-1.0),
                ))
                // Same avoidance contract as the 3D agent (pat-0q2kx.97): the
                // solve publishes the collision-free velocity through this
                // signal every physics tick.
                .signal(SignalInfo::new("velocity_computed").arg("safe_velocity"))
                .signal(SignalInfo::new("path_changed"))
                .signal(SignalInfo::new("target_reached"))
                .signal(SignalInfo::new("navigation_finished"))
                .signal(SignalInfo::new("waypoint_reached").arg("details"))
                .signal(SignalInfo::new("link_reached").arg("details")),
        );
    }
    // NavigationObstacle2D — the 2D dynamic avoidance obstacle (pat-0q2kx.97).
    //
    // `gdscene::navigation::NavigationObstacle2D` has driven the 2D ORCA solve
    // since pat-nzgdu.116, but the class was never registered, so a `.tscn`
    // could not carry one and the inspector had no schema for it.
    if !class_exists("NavigationObstacle2D") {
        register_class(
            ClassRegistration::new("NavigationObstacle2D")
                .parent("Node2D")
                .property(PropertyInfo::new("radius", Variant::Float(0.0)))
                .property(PropertyInfo::new(
                    "velocity",
                    Variant::Vector2(gdcore::math::Vector2::ZERO),
                ))
                .property(PropertyInfo::new("avoidance_enabled", Variant::Bool(true)))
                .property(PropertyInfo::new("avoidance_layers", Variant::Int(1)))
                .property(PropertyInfo::new(
                    "affect_navigation_mesh",
                    Variant::Bool(false),
                ))
                .property(PropertyInfo::new(
                    "carve_navigation_mesh",
                    Variant::Bool(false),
                ))
                .property(PropertyInfo::new("vertices", Variant::Array(Vec::new()))),
        );
    }

    // -- Control (UI) hierarchy --
    if !class_exists("Control") {
        register_class(
            ClassRegistration::new("Control")
                .parent("CanvasItem")
                .property(PropertyInfo::new("anchor_left", Variant::Float(0.0)))
                .property(PropertyInfo::new("anchor_top", Variant::Float(0.0)))
                .property(PropertyInfo::new("anchor_right", Variant::Float(0.0)))
                .property(PropertyInfo::new("anchor_bottom", Variant::Float(0.0)))
                // `accept_event()` is how a `_gui_input` handler stops an event
                // from bubbling to ancestor Controls (pat-0q2kx.82).
                .method(MethodInfo::new("accept_event", 0)),
        );
    }
    if !class_exists("Button") {
        register_class(ClassRegistration::new("Button").parent("BaseButton"));
    }
    if !class_exists("Label") {
        register_class(ClassRegistration::new("Label").parent("Control"));
    }
    // pat-0q2kx.87 — the editing surface. `LineEdit` was registered as a bare
    // Control: no `text`, no caret, and no `text_changed`/`text_submitted`, so
    // `has_signal("text_submitted")` answered false and a `connect` in a script
    // had nothing to bind to.
    //
    // ⚠️ `editable` defaults to **true** and `max_length` to **0 = unlimited**.
    // Registering either as the zero value would make every fresh LineEdit
    // read-only or zero-length.
    if !class_exists("LineEdit") {
        register_class(
            ClassRegistration::new("LineEdit")
                .parent("Control")
                .property(PropertyInfo::new("text", Variant::String(String::new())))
                .property(PropertyInfo::new(
                    "placeholder_text",
                    Variant::String(String::new()),
                ))
                .property(PropertyInfo::new("max_length", Variant::Int(0)))
                .property(PropertyInfo::new("editable", Variant::Bool(true)))
                .property(PropertyInfo::new("secret", Variant::Bool(false)))
                .property(PropertyInfo::new(
                    "secret_character",
                    Variant::String("•".to_string()),
                ))
                .property(PropertyInfo::new("caret_column", Variant::Int(0)))
                .method(MethodInfo::new("select_all", 0))
                .method(MethodInfo::new("deselect", 0))
                .method(MethodInfo::new("has_selection", 0))
                .method(MethodInfo::new("get_selected_text", 0))
                .method(MethodInfo::new("insert_text_at_caret", 1))
                .signal(SignalInfo::new("text_changed").arg("new_text"))
                .signal(SignalInfo::new("text_submitted").arg("new_text")),
        );
    }
    if !class_exists("TextEdit") {
        register_class(
            ClassRegistration::new("TextEdit")
                .parent("Control")
                .property(PropertyInfo::new("text", Variant::String(String::new())))
                .property(PropertyInfo::new("editable", Variant::Bool(true)))
                .property(PropertyInfo::new("caret_column", Variant::Int(0)))
                .method(MethodInfo::new("select_all", 0))
                .method(MethodInfo::new("deselect", 0))
                .method(MethodInfo::new("has_selection", 0))
                .method(MethodInfo::new("get_selected_text", 0))
                .method(MethodInfo::new("insert_text_at_caret", 1))
                .signal(SignalInfo::new("text_changed")),
        );
    }
    // `CodeEdit` extends TextEdit in Godot. The `CodeHighlighter` resource was
    // registered but its only consumer was not, so `ClassDB.class_exists`
    // answered false for the node a highlighted script editor hangs off.
    if !class_exists("CodeEdit") {
        register_class(ClassRegistration::new("CodeEdit").parent("TextEdit"));
    }
    if !class_exists("Panel") {
        register_class(ClassRegistration::new("Panel").parent("Control"));
    }
    if !class_exists("ColorRect") {
        register_class(ClassRegistration::new("ColorRect").parent("Control"));
    }
    if !class_exists("Container") {
        register_class(ClassRegistration::new("Container").parent("Control"));
    }
    if !class_exists("HBoxContainer") {
        register_class(ClassRegistration::new("HBoxContainer").parent("BoxContainer"));
    }
    if !class_exists("VBoxContainer") {
        register_class(ClassRegistration::new("VBoxContainer").parent("BoxContainer"));
    }
    // Split + SubViewport container family (pat-nzgdu.158).
    if !class_exists("SplitContainer") {
        register_class(ClassRegistration::new("SplitContainer").parent("Container"));
    }
    if !class_exists("HSplitContainer") {
        register_class(ClassRegistration::new("HSplitContainer").parent("SplitContainer"));
    }
    if !class_exists("VSplitContainer") {
        register_class(ClassRegistration::new("VSplitContainer").parent("SplitContainer"));
    }
    if !class_exists("SubViewportContainer") {
        register_class(ClassRegistration::new("SubViewportContainer").parent("Container"));
    }

    // -- Physics 2D bodies --
    if !class_exists("CollisionObject2D") {
        register_class(
            ClassRegistration::new("CollisionObject2D")
                .parent("Node2D")
                .abstract_class()
                .property(PropertyInfo::new("collision_layer", Variant::Int(1)))
                .property(PropertyInfo::new("collision_mask", Variant::Int(1))),
        );
    }
    if !class_exists("PhysicsBody2D") {
        register_class(
            ClassRegistration::new("PhysicsBody2D")
                .parent("CollisionObject2D")
                .abstract_class(),
        );
    }
    if !class_exists("StaticBody2D") {
        register_class(ClassRegistration::new("StaticBody2D").parent("PhysicsBody2D"));
    }

    // -- Joint2D family (pat-0q2kx.114) --
    //
    // `gdphysics2d::joint` solves all three joint kinds and `world.add_joint`
    // registers them, but no scene path built one and none of the classes was in
    // ClassDB. Parents and defaults from doc/classes/<Name>.xml. Note
    // `disable_collision` defaults to **true** in Godot: jointed bodies ignore
    // each other unless the author opts back in.
    if !class_exists("Joint2D") {
        register_class(
            ClassRegistration::new("Joint2D")
                .parent("Node2D")
                .abstract_class()
                .property(PropertyInfo::new(
                    "node_a",
                    Variant::NodePath(gdcore::node_path::NodePath::empty()),
                ))
                .property(PropertyInfo::new(
                    "node_b",
                    Variant::NodePath(gdcore::node_path::NodePath::empty()),
                ))
                .property(PropertyInfo::new("bias", Variant::Float(0.0)))
                .property(PropertyInfo::new("disable_collision", Variant::Bool(true))),
        );
    }
    if !class_exists("PinJoint2D") {
        register_class(
            ClassRegistration::new("PinJoint2D")
                .parent("Joint2D")
                .property(PropertyInfo::new("softness", Variant::Float(0.0)))
                .property(PropertyInfo::new(
                    "angular_limit_enabled",
                    Variant::Bool(false),
                ))
                .property(PropertyInfo::new(
                    "angular_limit_lower",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new(
                    "angular_limit_upper",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new("motor_enabled", Variant::Bool(false)))
                .property(PropertyInfo::new(
                    "motor_target_velocity",
                    Variant::Float(0.0),
                )),
        );
    }
    if !class_exists("GrooveJoint2D") {
        register_class(
            ClassRegistration::new("GrooveJoint2D")
                .parent("Joint2D")
                .property(PropertyInfo::new("length", Variant::Float(50.0)))
                .property(PropertyInfo::new("initial_offset", Variant::Float(25.0))),
        );
    }
    if !class_exists("DampedSpringJoint2D") {
        register_class(
            ClassRegistration::new("DampedSpringJoint2D")
                .parent("Joint2D")
                .property(PropertyInfo::new("length", Variant::Float(50.0)))
                .property(PropertyInfo::new("rest_length", Variant::Float(0.0)))
                .property(PropertyInfo::new("stiffness", Variant::Float(20.0)))
                .property(PropertyInfo::new("damping", Variant::Float(1.0))),
        );
    }
    if !class_exists("RigidBody2D") {
        register_class(
            ClassRegistration::new("RigidBody2D")
                .parent("PhysicsBody2D")
                .property(PropertyInfo::new("mass", Variant::Float(1.0)))
                .property(PropertyInfo::new("gravity_scale", Variant::Float(1.0)))
                .property(PropertyInfo::new("freeze", Variant::Bool(false))),
        );
    }
    if !class_exists("CharacterBody2D") {
        register_class(
            ClassRegistration::new("CharacterBody2D")
                .parent("PhysicsBody2D")
                .property(PropertyInfo::new(
                    "velocity",
                    Variant::Vector2(Vector2::ZERO),
                ))
                .property(PropertyInfo::new(
                    "floor_max_angle",
                    Variant::Float(0.785398),
                )),
        );
    }
    if !class_exists("Area2D") {
        register_class(
            ClassRegistration::new("Area2D")
                .parent("CollisionObject2D")
                .property(PropertyInfo::new("monitoring", Variant::Bool(true)))
                .property(PropertyInfo::new("monitorable", Variant::Bool(true)))
                // The angular-damp override (pat-0q2kx.112), the rotational
                // twin of the linear one Godot exposes alongside it.
                .property(PropertyInfo::new(
                    "angular_damp_space_override",
                    Variant::Int(0),
                ))
                .property(PropertyInfo::new("angular_damp", Variant::Float(1.0))),
        );
    }
    if !class_exists("CollisionShape2D") {
        register_class(
            ClassRegistration::new("CollisionShape2D")
                .parent("Node2D")
                .property(PropertyInfo::new("disabled", Variant::Bool(false))),
        );
    }
    // Timer node (pat-u3l8c.3). Countdown runs in SceneTree::process_timer_nodes.
    if !class_exists("Timer") {
        register_class(
            ClassRegistration::new("Timer")
                .parent("Node")
                .property(PropertyInfo::new("wait_time", Variant::Float(1.0)))
                .property(PropertyInfo::new("one_shot", Variant::Bool(false)))
                .property(PropertyInfo::new("autostart", Variant::Bool(false)))
                .property(PropertyInfo::new("paused", Variant::Bool(false)))
                // TIMER_PROCESS_IDLE
                .property(PropertyInfo::new("process_callback", Variant::Int(1))),
        );
    }

    // Theme resources (pat-nzgdu.89): Theme is a Resource; ThemeDB is the
    // Object singleton exposing the project + default themes to scripts.
    if !class_exists("Theme") {
        register_class(ClassRegistration::new("Theme").parent("Resource"));
    }
    if !class_exists("ThemeDB") {
        register_class(ClassRegistration::new("ThemeDB").parent("Object"));
    }

    // -- Missing 2D nodes and resources (pat-0q2kx.106) ---------------------
    //
    // Seventeen Godot 4.6 2D classes had no ClassDB entry at all. That is not
    // cosmetic: `Node::is_class` resolves through ClassDB (gdscene/src/node.rs),
    // so an unregistered node answers `false` to `is_class("CanvasItem")` and
    // silently drops out of every canvas walk — `collect_canvas_items` skips it,
    // and the Create-Node dialog never offers it.
    //
    // Parents are the `inherits` attribute from each class's
    // doc/classes/<Name>.xml; property defaults are that file's `default=`.

    // `CanvasLayer` is registered here rather than assumed: it is
    // ParallaxBackground's parent and appears nowhere else in this file, so
    // without it that chain would resolve to nothing.
    if !class_exists("CanvasLayer") {
        register_class(
            ClassRegistration::new("CanvasLayer")
                .parent("Node")
                .property(PropertyInfo::new("layer", Variant::Int(1)))
                .property(PropertyInfo::new("visible", Variant::Bool(true)))
                .property(PropertyInfo::new("offset", Variant::Vector2(Vector2::ZERO)))
                .property(PropertyInfo::new("rotation", Variant::Float(0.0)))
                .property(PropertyInfo::new("scale", Variant::Vector2(Vector2::ONE)))
                .property(PropertyInfo::new(
                    "follow_viewport_enabled",
                    Variant::Bool(false),
                )),
        );
    }

    if !class_exists("RayCast2D") {
        register_class(
            ClassRegistration::new("RayCast2D")
                .parent("Node2D")
                .property(PropertyInfo::new("enabled", Variant::Bool(true)))
                .property(PropertyInfo::new("exclude_parent", Variant::Bool(true)))
                .property(PropertyInfo::new(
                    "target_position",
                    Variant::Vector2(Vector2::new(0.0, 50.0)),
                ))
                .property(PropertyInfo::new("collision_mask", Variant::Int(1)))
                .property(PropertyInfo::new(
                    "collide_with_areas",
                    Variant::Bool(false),
                ))
                .property(PropertyInfo::new(
                    "collide_with_bodies",
                    Variant::Bool(true),
                ))
                .property(PropertyInfo::new("hit_from_inside", Variant::Bool(false))),
        );
    }
    if !class_exists("ShapeCast2D") {
        register_class(
            ClassRegistration::new("ShapeCast2D")
                .parent("Node2D")
                .property(PropertyInfo::new("enabled", Variant::Bool(true)))
                .property(PropertyInfo::new("exclude_parent", Variant::Bool(true)))
                .property(PropertyInfo::new(
                    "target_position",
                    Variant::Vector2(Vector2::new(0.0, 50.0)),
                ))
                .property(PropertyInfo::new("collision_mask", Variant::Int(1)))
                .property(PropertyInfo::new(
                    "collide_with_areas",
                    Variant::Bool(false),
                ))
                .property(PropertyInfo::new(
                    "collide_with_bodies",
                    Variant::Bool(true),
                ))
                .property(PropertyInfo::new("margin", Variant::Float(0.0)))
                .property(PropertyInfo::new("max_results", Variant::Int(32))),
        );
    }
    if !class_exists("CanvasModulate") {
        register_class(
            ClassRegistration::new("CanvasModulate")
                .parent("Node2D")
                .property(PropertyInfo::new(
                    "color",
                    Variant::Color(gdcore::math::Color::WHITE),
                )),
        );
    }
    if !class_exists("VisibleOnScreenNotifier2D") {
        register_class(
            ClassRegistration::new("VisibleOnScreenNotifier2D")
                .parent("Node2D")
                .property(PropertyInfo::new(
                    "rect",
                    Variant::Rect2(gdcore::math::Rect2::new(
                        Vector2::new(-10.0, -10.0),
                        Vector2::new(20.0, 20.0),
                    )),
                ))
                .property(PropertyInfo::new("show_rect", Variant::Bool(true)))
                .signal(SignalInfo::new("screen_entered"))
                .signal(SignalInfo::new("screen_exited")),
        );
    }
    if !class_exists("VisibleOnScreenEnabler2D") {
        register_class(
            ClassRegistration::new("VisibleOnScreenEnabler2D")
                .parent("VisibleOnScreenNotifier2D")
                .property(PropertyInfo::new("enable_mode", Variant::Int(0)))
                // Godot's default is `NodePath("..")`: the node it enables is
                // its own parent (doc/classes/VisibleOnScreenEnabler2D.xml).
                .property(PropertyInfo::new(
                    "enable_node_path",
                    Variant::NodePath(gdcore::node_path::NodePath::new("..")),
                ))
                .enumeration(
                    EnumInfo::new("EnableMode")
                        .constant("ENABLE_MODE_INHERIT", 0)
                        .constant("ENABLE_MODE_ALWAYS", 1)
                        .constant("ENABLE_MODE_WHEN_PAUSED", 2),
                ),
        );
    }
    if !class_exists("Parallax2D") {
        register_class(
            ClassRegistration::new("Parallax2D")
                .parent("Node2D")
                .property(PropertyInfo::new(
                    "scroll_scale",
                    Variant::Vector2(Vector2::ONE),
                ))
                .property(PropertyInfo::new(
                    "scroll_offset",
                    Variant::Vector2(Vector2::ZERO),
                ))
                .property(PropertyInfo::new(
                    "repeat_size",
                    Variant::Vector2(Vector2::ZERO),
                ))
                .property(PropertyInfo::new("repeat_times", Variant::Int(1)))
                .property(PropertyInfo::new(
                    "autoscroll",
                    Variant::Vector2(Vector2::ZERO),
                ))
                .property(PropertyInfo::new(
                    "limit_begin",
                    Variant::Vector2(Vector2::new(-1.0e7, -1.0e7)),
                ))
                .property(PropertyInfo::new(
                    "limit_end",
                    Variant::Vector2(Vector2::new(1.0e7, 1.0e7)),
                ))
                .property(PropertyInfo::new("follow_viewport", Variant::Bool(true)))
                .property(PropertyInfo::new(
                    "ignore_camera_scroll",
                    Variant::Bool(false),
                )),
        );
    }
    if !class_exists("ParallaxBackground") {
        register_class(
            ClassRegistration::new("ParallaxBackground")
                .parent("CanvasLayer")
                .property(PropertyInfo::new(
                    "scroll_offset",
                    Variant::Vector2(Vector2::ZERO),
                ))
                .property(PropertyInfo::new(
                    "scroll_base_offset",
                    Variant::Vector2(Vector2::ZERO),
                ))
                .property(PropertyInfo::new(
                    "scroll_base_scale",
                    Variant::Vector2(Vector2::ONE),
                ))
                .property(PropertyInfo::new(
                    "scroll_ignore_camera_zoom",
                    Variant::Bool(false),
                )),
        );
    }
    if !class_exists("ParallaxLayer") {
        register_class(
            ClassRegistration::new("ParallaxLayer")
                .parent("Node2D")
                .property(PropertyInfo::new(
                    "motion_scale",
                    Variant::Vector2(Vector2::ONE),
                ))
                .property(PropertyInfo::new(
                    "motion_offset",
                    Variant::Vector2(Vector2::ZERO),
                ))
                .property(PropertyInfo::new(
                    "motion_mirroring",
                    Variant::Vector2(Vector2::ZERO),
                )),
        );
    }
    if !class_exists("CollisionPolygon2D") {
        register_class(
            ClassRegistration::new("CollisionPolygon2D")
                .parent("Node2D")
                .property(PropertyInfo::new("build_mode", Variant::Int(0)))
                .property(PropertyInfo::new("disabled", Variant::Bool(false)))
                .property(PropertyInfo::new("one_way_collision", Variant::Bool(false)))
                .property(PropertyInfo::new(
                    "one_way_collision_margin",
                    Variant::Float(1.0),
                ))
                .property(PropertyInfo::new(
                    "polygon",
                    Variant::PackedVector2Array(Vec::new()),
                ))
                .enumeration(
                    EnumInfo::new("BuildMode")
                        .constant("BUILD_SOLIDS", 0)
                        .constant("BUILD_SEGMENTS", 1),
                ),
        );
    }
    if !class_exists("PhysicalBone2D") {
        register_class(
            ClassRegistration::new("PhysicalBone2D")
                .parent("RigidBody2D")
                .property(PropertyInfo::new(
                    "bone2d_nodepath",
                    Variant::NodePath(gdcore::node_path::NodePath::empty()),
                ))
                .property(PropertyInfo::new("bone2d_index", Variant::Int(-1)))
                .property(PropertyInfo::new(
                    "auto_configure_joint",
                    Variant::Bool(true),
                ))
                .property(PropertyInfo::new("simulate_physics", Variant::Bool(false)))
                .property(PropertyInfo::new(
                    "follow_bone_when_simulating",
                    Variant::Bool(false),
                )),
        );
    }
    // The pre-4.3 monolithic TileMap node. Still instantiable in 4.6 (deprecated
    // in favour of TileMapLayer), and a `.tscn` from an older project names it.
    if !class_exists("TileMap") {
        register_class(
            ClassRegistration::new("TileMap")
                .parent("Node2D")
                .property(PropertyInfo::new(
                    "rendering_quadrant_size",
                    Variant::Int(16),
                ))
                .property(PropertyInfo::new(
                    "collision_animatable",
                    Variant::Bool(false),
                ))
                .property(PropertyInfo::new(
                    "collision_visibility_mode",
                    Variant::Int(0),
                ))
                .property(PropertyInfo::new(
                    "navigation_visibility_mode",
                    Variant::Int(0),
                ))
                .property(PropertyInfo::new("tile_set", Variant::Nil))
                .signal(SignalInfo::new("changed")),
        );
    }

    // Resources. `TileData` inherits Object, not Resource — the one exception in
    // this family, per doc/classes/TileData.xml.
    if !class_exists("Curve2D") {
        register_class(
            ClassRegistration::new("Curve2D")
                .parent("Resource")
                .property(PropertyInfo::new("bake_interval", Variant::Float(5.0)))
                .property(PropertyInfo::new("point_count", Variant::Int(0))),
        );
    }
    if !class_exists("SpriteFrames") {
        register_class(ClassRegistration::new("SpriteFrames").parent("Resource"));
    }
    if !class_exists("OccluderPolygon2D") {
        register_class(
            ClassRegistration::new("OccluderPolygon2D")
                .parent("Resource")
                .property(PropertyInfo::new("closed", Variant::Bool(true)))
                .property(PropertyInfo::new("cull_mode", Variant::Int(0)))
                .property(PropertyInfo::new(
                    "polygon",
                    Variant::PackedVector2Array(Vec::new()),
                ))
                .enumeration(
                    EnumInfo::new("CullMode")
                        .constant("CULL_DISABLED", 0)
                        .constant("CULL_CLOCKWISE", 1)
                        .constant("CULL_COUNTER_CLOCKWISE", 2),
                ),
        );
    }
    if !class_exists("TileSet") {
        register_class(
            ClassRegistration::new("TileSet")
                .parent("Resource")
                .property(PropertyInfo::new("tile_shape", Variant::Int(0)))
                .property(PropertyInfo::new("tile_layout", Variant::Int(0)))
                .property(PropertyInfo::new("tile_offset_axis", Variant::Int(0)))
                .property(PropertyInfo::new(
                    "tile_size",
                    Variant::Vector2(Vector2::new(16.0, 16.0)),
                ))
                .property(PropertyInfo::new("uv_clipping", Variant::Bool(false))),
        );
    }
    if !class_exists("TileSetSource") {
        register_class(ClassRegistration::new("TileSetSource").parent("Resource"));
    }
    if !class_exists("TileSetAtlasSource") {
        register_class(
            ClassRegistration::new("TileSetAtlasSource")
                .parent("TileSetSource")
                .property(PropertyInfo::new("texture", Variant::Nil))
                .property(PropertyInfo::new(
                    "texture_region_size",
                    Variant::Vector2(Vector2::new(16.0, 16.0)),
                ))
                .property(PropertyInfo::new(
                    "margins",
                    Variant::Vector2(Vector2::ZERO),
                ))
                .property(PropertyInfo::new(
                    "separation",
                    Variant::Vector2(Vector2::ZERO),
                ))
                .property(PropertyInfo::new(
                    "use_texture_padding",
                    Variant::Bool(true),
                )),
        );
    }
    if !class_exists("TileSetScenesCollectionSource") {
        register_class(
            ClassRegistration::new("TileSetScenesCollectionSource").parent("TileSetSource"),
        );
    }
    if !class_exists("TileData") {
        register_class(
            ClassRegistration::new("TileData")
                .parent("Object")
                .property(PropertyInfo::new("flip_h", Variant::Bool(false)))
                .property(PropertyInfo::new("flip_v", Variant::Bool(false)))
                .property(PropertyInfo::new("transpose", Variant::Bool(false)))
                .property(PropertyInfo::new(
                    "texture_origin",
                    Variant::Vector2(Vector2::ZERO),
                ))
                .property(PropertyInfo::new(
                    "modulate",
                    Variant::Color(gdcore::math::Color::WHITE),
                ))
                .property(PropertyInfo::new("z_index", Variant::Int(0)))
                .property(PropertyInfo::new("y_sort_origin", Variant::Int(0)))
                .signal(SignalInfo::new("changed")),
        );
    }
    if !class_exists("TileMapPattern") {
        register_class(ClassRegistration::new("TileMapPattern").parent("Resource"));
    }

    // Viewport / SubViewport / ViewportTexture (pat-zx775).
    register_viewport_classes();
}

/// Registers Viewport, SubViewport, Texture2D, and ViewportTexture in ClassDB.
///
/// Hierarchy (Godot 4 slice):
/// * `Viewport : Node` (abstract base; owns render-target contract)
/// * `SubViewport : Viewport` — off-screen render target with size/update/clear modes
/// * `Texture2D : Resource` — base 2D texture type
/// * `ViewportTexture : Texture2D` — samples a SubViewport color buffer
///
/// Defaults match Godot 4's `scene/main/viewport.h`:
/// * `size` = (512, 512)
/// * `render_target_update_mode` = `UPDATE_WHEN_VISIBLE` (2)
/// * `render_target_clear_mode` = `CLEAR_MODE_ALWAYS` (0)
/// * `transparent_bg` = false
///
/// Idempotent — each class is only registered if not already present.
/// Registers just the `Object → Node → Viewport → Window` chain the SceneTree
/// root's class depends on (pat-nzgdu.87).
///
/// ⚠️ `SceneTree::new()` calls this, and it MUST keep doing so. The root node's
/// class is `Window`, and `Node::is_class` answers anything other than an exact
/// match by walking the ClassDB — so without this chain registered,
/// `root.is_class("Node")` would return **false**. `register_viewport_classes()`
/// cannot cover it: nothing calls that automatically, only individual tests, so
/// the overwhelming majority of scene trees would come up with a root that is
/// not a Node. Registration is idempotent and guarded by `class_exists`, so the
/// repeated calls cost one map lookup each.
pub fn register_root_window_chain() {
    use gdcore::math::Vector2;

    if !class_exists("Object") {
        register_class(object_class());
    }
    if !class_exists("Node") {
        register_class(node_class());
    }
    if !class_exists("Viewport") {
        register_class(
            ClassRegistration::new("Viewport")
                .parent("Node")
                .abstract_class()
                .property(PropertyInfo::new("transparent_bg", Variant::Bool(false)))
                .method(MethodInfo::new("get_texture", 0)),
        );
    }
    if !class_exists("Window") {
        register_class(
            ClassRegistration::new("Window")
                .parent("Viewport")
                .property(PropertyInfo::new("title", Variant::String(String::new())))
                .property(PropertyInfo::new(
                    "size",
                    Variant::Vector2(Vector2::new(100.0, 100.0)),
                ))
                .property(PropertyInfo::new("visible", Variant::Bool(true)))
                .property(PropertyInfo::new("content_scale_mode", Variant::Int(0)))
                .property(PropertyInfo::new("content_scale_aspect", Variant::Int(0)))
                .property(PropertyInfo::new("content_scale_stretch", Variant::Int(0)))
                .property(PropertyInfo::new(
                    "content_scale_factor",
                    Variant::Float(1.0),
                ))
                .property(PropertyInfo::new(
                    "content_scale_size",
                    Variant::Vector2(Vector2::ZERO),
                ))
                .method(MethodInfo::new("get_window", 0))
                .method(MethodInfo::new("get_viewport", 0)),
        );
    }
}

pub fn register_viewport_classes() {
    use gdcore::math::Vector2;

    if !class_exists("Object") {
        register_class(object_class());
    }
    if !class_exists("Node") {
        register_class(node_class());
    }
    // `Resource` is a `RefCounted`, so the ref-counted base has to be present
    // before any resource chain can reach `Object` (pat-nzgdu.98). This path
    // only picks `RefCounted` up transitively, via the
    // `register_rendering_device_classes()` call far below — guard it here so
    // the base cannot depend on statement order inside this function.
    if !class_exists("RefCounted") {
        register_class(ClassRegistration::new("RefCounted").parent("Object"));
    }
    if !class_exists("Resource") {
        register_class(ClassRegistration::new("Resource").parent("RefCounted"));
    }

    // Viewport is abstract in Godot (Window / SubViewport are concrete).
    if !class_exists("Viewport") {
        register_class(
            ClassRegistration::new("Viewport")
                .parent("Node")
                .abstract_class()
                .property(PropertyInfo::new("transparent_bg", Variant::Bool(false)))
                .method(MethodInfo::new("get_texture", 0)),
        );
    }

    // SubViewport — default size 512×512, update when visible, clear always.
    // Window — the OTHER concrete Viewport, and the class of the SceneTree
    // root (pat-nzgdu.87). Its content_scale_* properties are the stretch
    // pipeline: they map the project's base resolution onto the real window.
    // Defaults come from `Window`'s own property defaults, not from the
    // `display/window/stretch/*` project settings that seed them at boot.
    if !class_exists("Window") {
        register_class(
            ClassRegistration::new("Window")
                .parent("Viewport")
                .property(PropertyInfo::new("title", Variant::String(String::new())))
                .property(PropertyInfo::new(
                    "size",
                    Variant::Vector2(Vector2::new(100.0, 100.0)),
                ))
                .property(PropertyInfo::new("visible", Variant::Bool(true)))
                // CONTENT_SCALE_MODE_DISABLED = 0
                .property(PropertyInfo::new("content_scale_mode", Variant::Int(0)))
                // ⚠️ CONTENT_SCALE_ASPECT_IGNORE = 0 is the *class* default.
                // The project setting `display/window/stretch/aspect` defaults
                // to "keep" instead — do not copy that value here, or a Window
                // created in code would silently letterbox.
                .property(PropertyInfo::new("content_scale_aspect", Variant::Int(0)))
                // CONTENT_SCALE_STRETCH_FRACTIONAL = 0
                .property(PropertyInfo::new("content_scale_stretch", Variant::Int(0)))
                .property(PropertyInfo::new(
                    "content_scale_factor",
                    Variant::Float(1.0),
                ))
                .property(PropertyInfo::new(
                    "content_scale_size",
                    Variant::Vector2(Vector2::ZERO),
                ))
                .method(MethodInfo::new("get_window", 0))
                .method(MethodInfo::new("get_viewport", 0)),
        );
    }

    // `size` is Vector2i in Godot; Patina stores it as Vector2 until Vector2i
    // lands in Variant (integer components 512.0, 512.0).
    if !class_exists("SubViewport") {
        register_class(
            ClassRegistration::new("SubViewport")
                .parent("Viewport")
                .property(PropertyInfo::new(
                    "size",
                    Variant::Vector2(Vector2::new(512.0, 512.0)),
                ))
                .property(PropertyInfo::new(
                    "size_2d_override",
                    Variant::Vector2(Vector2::ZERO),
                ))
                .property(PropertyInfo::new(
                    "size_2d_override_stretch",
                    Variant::Bool(false),
                ))
                // UPDATE_DISABLED=0, ONCE=1, WHEN_VISIBLE=2, WHEN_PARENT_VISIBLE=3, ALWAYS=4
                .property(PropertyInfo::new(
                    "render_target_update_mode",
                    Variant::Int(2), // UPDATE_WHEN_VISIBLE
                ))
                // CLEAR_MODE_ALWAYS=0, NEVER=1, ONCE=2
                .property(PropertyInfo::new(
                    "render_target_clear_mode",
                    Variant::Int(0), // CLEAR_MODE_ALWAYS
                ))
                .property(PropertyInfo::new("transparent_bg", Variant::Bool(false)))
                .method(MethodInfo::new("get_texture", 0))
                .method(MethodInfo::new("set_size", 1))
                .method(MethodInfo::new("get_size", 0)),
        );
    }

    // Texture is Godot's abstract root for every texture resource; Texture2D,
    // TextureLayered and Texture3D all descend from it (pat-nzgdu.214).
    if !class_exists("Texture") {
        register_class(
            // ⚠️ NOT abstract (pat-nzgdu.215). `GDREGISTER_VIRTUAL_CLASS(Texture)`
            // — and VIRTUAL is not ABSTRACT. `register_class<T>(true)` still
            // installs a creation func, and Godot's `ClassDB::is_abstract`
            // returns false the moment one exists (`class_db.cpp:843`); only
            // `GDREGISTER_ABSTRACT_CLASS` (no creation func) is abstract. The
            // dump agrees: `is_instantiable=true`.
            ClassRegistration::new("Texture").parent("Resource"),
        );
    }

    if !class_exists("Texture2D") {
        register_class(
            ClassRegistration::new("Texture2D")
                .parent("Texture")
                .method(MethodInfo::new("get_width", 0))
                .method(MethodInfo::new("get_height", 0))
                .method(MethodInfo::new("get_size", 0)),
        );
    }

    // -- ExternalTexture (pat-nzgdu.209) -------------------------------------
    //
    // A texture backed by a platform external buffer — "as defined by
    // OES_EGL_image_external (GLES) or VK_ANDROID_external_memory_android_-
    // hardware_buffer (Vulkan)" (scene/resources/external_texture.h). It takes
    // an opaque uint64 handle; it has nothing to do with video or cameras
    // (those are CameraFeed / CameraServer / VideoStreamPlayer).
    //
    // ⚠️ Only THREE methods are bound here (external_texture.cpp:35-41).
    // `get_size` is NOT one of them — ExternalTexture overrides the virtual but
    // the binding is inherited from Texture2D, so re-registering it here would
    // invent a duplicate.
    if !class_exists("ExternalTexture") {
        register_class(
            ClassRegistration::new("ExternalTexture")
                .parent("Texture2D")
                // ⚠️ 256x256, NOT zero: `Size2 size = Size2(256, 256)` is a
                // member initialiser (external_texture.h) — the ctor body is
                // empty, so it is easy to miss and assume 0.
                .property(PropertyInfo::new(
                    "size",
                    Variant::Vector2(gdcore::math::Vector2::new(256.0, 256.0)),
                ))
                .method(MethodInfo::new("set_size", 1))
                .method(MethodInfo::new("get_external_texture_id", 0))
                .method(MethodInfo::new("set_external_buffer_id", 1)),
        );
    }

    if !class_exists("ViewportTexture") {
        register_class(
            ClassRegistration::new("ViewportTexture")
                .parent("Texture2D")
                .property(PropertyInfo::new(
                    "viewport_path",
                    Variant::NodePath(gdcore::NodePath::new("")),
                ))
                .method(MethodInfo::new("get_width", 0))
                .method(MethodInfo::new("get_height", 0))
                .method(MethodInfo::new("get_size", 0)),
        );
    }

    // Layered + volume texture family (pat-nzgdu.214). Only the class surface is
    // registered here: layered/3D pixel storage and shader sampling are deferred,
    // so a .tscn/.tres referencing these types loads without dropping the
    // reference (the generic sub_resource loader handles the rest).
    if !class_exists("TextureLayered") {
        register_class(
            // ⚠️ NOT abstract (pat-nzgdu.215): `GDREGISTER_VIRTUAL_CLASS(TextureLayered)`
            // (register_scene_types.cpp:1057). See `Texture` above — virtual
            // classes keep a creation func, so Godot's `is_abstract` is false
            // and a script may extend AND construct one.
            ClassRegistration::new("TextureLayered")
                .parent("Texture")
                .method(MethodInfo::new("get_format", 0))
                .method(MethodInfo::new("get_layered_type", 0))
                .method(MethodInfo::new("get_width", 0))
                .method(MethodInfo::new("get_height", 0))
                .method(MethodInfo::new("get_layers", 0))
                .method(MethodInfo::new("has_mipmaps", 0))
                .method(MethodInfo::new("get_layer_data", 1))
                // The seven `_`-prefixed virtuals a script/GDExtension overrides
                // to supply the layer data. Being VIRTUAL rather than abstract is
                // exactly what these are for — without them the class is
                // introspectable but not implementable.
                .method(MethodInfo::new("_get_format", 0))
                .method(MethodInfo::new("_get_layered_type", 0))
                .method(MethodInfo::new("_get_width", 0))
                .method(MethodInfo::new("_get_height", 0))
                .method(MethodInfo::new("_get_layers", 0))
                .method(MethodInfo::new("_has_mipmaps", 0))
                .method(MethodInfo::new("_get_layer_data", 1)),
        );
    }

    // -- The concrete layered resources (pat-nzgdu.215) ----------------------
    //
    // ⚠️ Cubemap and CubemapArray are `ImageTextureLayered`s, NOT `TextureLayered`s
    // directly — a cubemap is six image layers, so it inherits the image-backed
    // base alongside Texture2DArray. Parenting them straight to TextureLayered
    // would skip the layer storage they actually use.
    if !class_exists("Cubemap") {
        register_class(
            ClassRegistration::new("Cubemap")
                .parent("ImageTextureLayered")
                .method(MethodInfo::new("create_placeholder", 0)),
        );
    }
    if !class_exists("CubemapArray") {
        register_class(
            ClassRegistration::new("CubemapArray")
                .parent("ImageTextureLayered")
                .method(MethodInfo::new("create_placeholder", 0)),
        );
    }

    if !class_exists("ImageTextureLayered") {
        register_class(
            ClassRegistration::new("ImageTextureLayered")
                .parent("TextureLayered")
                .abstract_class()
                .method(MethodInfo::new("create_from_images", 1))
                .method(MethodInfo::new("update_layer", 2)),
        );
    }

    if !class_exists("Texture2DArray") {
        register_class(
            ClassRegistration::new("Texture2DArray")
                .parent("ImageTextureLayered")
                .method(MethodInfo::new("create_placeholder", 0)),
        );
    }

    // Texture3D is abstract in Godot; ImageTexture3D is the concrete volume type.
    if !class_exists("Texture3D") {
        register_class(
            // ⚠️ NOT abstract (pat-nzgdu.215): `GDREGISTER_VIRTUAL_CLASS(Texture3D)`.
            // See `Texture` above.
            ClassRegistration::new("Texture3D")
                .parent("Texture")
                .method(MethodInfo::new("get_format", 0))
                .method(MethodInfo::new("get_width", 0))
                .method(MethodInfo::new("get_height", 0))
                .method(MethodInfo::new("get_depth", 0))
                .method(MethodInfo::new("has_mipmaps", 0))
                .method(MethodInfo::new("get_data", 0))
                .method(MethodInfo::new("create_placeholder", 0))
                // The six virtuals a script overrides to supply the volume data
                // — the point of being VIRTUAL rather than abstract.
                .method(MethodInfo::new("_get_format", 0))
                .method(MethodInfo::new("_get_width", 0))
                .method(MethodInfo::new("_get_height", 0))
                .method(MethodInfo::new("_get_depth", 0))
                .method(MethodInfo::new("_has_mipmaps", 0))
                .method(MethodInfo::new("_get_data", 0)),
        );
    }

    // -- Curve textures (pat-nzgdu.225) --------------------------------------
    //
    // A baked `Curve` sampled as a 1D lookup texture. They live here, not beside
    // the VisualShader nodes that consume them, because their parent does:
    // `Texture2D` is registered in this pass, and parenting to a class another
    // pass owns severs the chain when this one runs alone (pat-nzgdu.98).
    //
    // ⚠️ `set_width` is bound but `get_width` is NOT — the getter is Texture2D's.
    // Same asymmetry as the placeholders below.
    //
    // ⚠️ `width` defaults to **256**, not 0: these bake a curve into a texture of
    // that many samples, and a zero-width lookup samples nothing.
    if !class_exists("CurveTexture") {
        register_class(
            ClassRegistration::new("CurveTexture")
                .parent("Texture2D")
                .property(PropertyInfo::new("width", Variant::Int(256)))
                .property(PropertyInfo::new("texture_mode", Variant::Int(0)))
                // The baked `Curve` resource; Nil until assigned. ⚠️ `Curve`
                // itself is NOT registered — see the bead notes.
                .property(PropertyInfo::new("curve", Variant::Nil))
                .method(MethodInfo::new("set_width", 1))
                .method(MethodInfo::new("set_curve", 1))
                .method(MethodInfo::new("get_curve", 0))
                .method(MethodInfo::new("set_texture_mode", 1))
                .method(MethodInfo::new("get_texture_mode", 0))
                .enumeration(
                    EnumInfo::new("TextureMode")
                        .constant("TEXTURE_MODE_RGB", 0)
                        .constant("TEXTURE_MODE_RED", 1),
                ),
        );
    }
    // ⚠️ Three separate curves, one per axis — NOT one curve and a mode. And it
    // has no `texture_mode` at all, unlike its sibling above.
    if !class_exists("CurveXYZTexture") {
        register_class(
            ClassRegistration::new("CurveXYZTexture")
                .parent("Texture2D")
                .property(PropertyInfo::new("width", Variant::Int(256)))
                .property(PropertyInfo::new("curve_x", Variant::Nil))
                .property(PropertyInfo::new("curve_y", Variant::Nil))
                .property(PropertyInfo::new("curve_z", Variant::Nil))
                .method(MethodInfo::new("set_width", 1))
                .method(MethodInfo::new("set_curve_x", 1))
                .method(MethodInfo::new("get_curve_x", 0))
                .method(MethodInfo::new("set_curve_y", 1))
                .method(MethodInfo::new("get_curve_y", 0))
                .method(MethodInfo::new("set_curve_z", 1))
                .method(MethodInfo::new("get_curve_z", 0)),
        );
    }

    // -- Placeholder textures (pat-nzgdu.221) --------------------------------
    //
    // What Godot substitutes when a texture is stripped from an export or not
    // yet imported: it carries dimensions and draws nothing.
    //
    // ⚠️ Only the SETTERS are bound. `PlaceholderTexture2D` binds `set_size`
    // and **not** `get_size` — the getter is `Texture2D`'s, overridden in C++ but
    // bound on the base. The bead asks to "give each a get_size/get_width/
    // get_height ... getter"; adding one here would invent a duplicate ClassDB
    // entry. The getters are reachable by inheritance, which is the point.
    if !class_exists("PlaceholderTexture2D") {
        register_class(
            ClassRegistration::new("PlaceholderTexture2D")
                .parent("Texture2D")
                // ⚠️ Vector2 (float), unlike its layered/3D cousins below which
                // are Vector2i/Vector3i — this one IS representable.
                .property(PropertyInfo::new(
                    "size",
                    Variant::Vector2(gdcore::math::Vector2::new(1.0, 1.0)),
                ))
                .method(MethodInfo::new("set_size", 1)),
        );
    }
    // ⚠️ ABSTRACT — `GDREGISTER_ABSTRACT_CLASS(PlaceholderTextureLayered)`. Its
    // three concrete subtypes below are what a loader substitutes.
    if !class_exists("PlaceholderTextureLayered") {
        register_class(
            ClassRegistration::new("PlaceholderTextureLayered")
                .parent("TextureLayered")
                .abstract_class()
                // ⚠️ `size` is a **Vector2i**, which `Variant` cannot represent
                // (`gdvariant::binary` names it among the types the enum lacks).
                // Registered as Nil so the member exists for introspection
                // without a Vector2 standing in for a Vector2i and making
                // `typeof()` lie — same call as pat-nzgdu.213/.220.
                .property(PropertyInfo::new("size", Variant::Nil))
                // ⚠️ layers defaults to 1, not 0: a zero-layer placeholder is not
                // a texture at all.
                .property(PropertyInfo::new("layers", Variant::Int(1)))
                // ⚠️ Asymmetric: `set_layers` is bound but `get_layers` is NOT —
                // it comes from TextureLayered.
                .method(MethodInfo::new("set_size", 1))
                .method(MethodInfo::new("get_size", 0))
                .method(MethodInfo::new("set_layers", 1)),
        );
    }
    // ⚠️ All three bind NOTHING — no methods, no properties. Their whole surface
    // is PlaceholderTextureLayered's. They exist to name the layered kind.
    if !class_exists("PlaceholderTexture2DArray") {
        register_class(
            ClassRegistration::new("PlaceholderTexture2DArray").parent("PlaceholderTextureLayered"),
        );
    }
    if !class_exists("PlaceholderCubemap") {
        register_class(
            ClassRegistration::new("PlaceholderCubemap").parent("PlaceholderTextureLayered"),
        );
    }
    if !class_exists("PlaceholderCubemapArray") {
        register_class(
            ClassRegistration::new("PlaceholderCubemapArray").parent("PlaceholderTextureLayered"),
        );
    }
    if !class_exists("PlaceholderTexture3D") {
        register_class(
            ClassRegistration::new("PlaceholderTexture3D")
                .parent("Texture3D")
                // ⚠️ Vector3i — also not representable; see above.
                .property(PropertyInfo::new("size", Variant::Nil))
                .method(MethodInfo::new("set_size", 1))
                .method(MethodInfo::new("get_size", 0)),
        );
    }

    if !class_exists("ImageTexture3D") {
        register_class(
            ClassRegistration::new("ImageTexture3D")
                .parent("Texture3D")
                .method(MethodInfo::new("create", 5))
                .method(MethodInfo::new("update", 1)),
        );
    }

    // RenderingDevice-backed wrappers. The `texture_rd_rid` property is deferred:
    // Variant has no RID variant yet, so only the accessor surface is registered.
    if !class_exists("Texture2DRD") {
        register_class(
            ClassRegistration::new("Texture2DRD")
                .parent("Texture2D")
                .method(MethodInfo::new("set_texture_rd_rid", 1))
                .method(MethodInfo::new("get_texture_rd_rid", 0)),
        );
    }

    if !class_exists("TextureLayeredRD") {
        register_class(
            ClassRegistration::new("TextureLayeredRD")
                .parent("TextureLayered")
                .abstract_class()
                .method(MethodInfo::new("set_texture_rd_rid", 1))
                .method(MethodInfo::new("get_texture_rd_rid", 0)),
        );
    }

    if !class_exists("Texture2DArrayRD") {
        register_class(ClassRegistration::new("Texture2DArrayRD").parent("TextureLayeredRD"));
    }

    if !class_exists("TextureCubemapRD") {
        register_class(ClassRegistration::new("TextureCubemapRD").parent("TextureLayeredRD"));
    }

    if !class_exists("TextureCubemapArrayRD") {
        register_class(ClassRegistration::new("TextureCubemapArrayRD").parent("TextureLayeredRD"));
    }

    if !class_exists("Texture3DRD") {
        register_class(
            ClassRegistration::new("Texture3DRD")
                .parent("Texture3D")
                .method(MethodInfo::new("set_texture_rd_rid", 1))
                .method(MethodInfo::new("get_texture_rd_rid", 0)),
        );
    }

    register_rendering_device_classes();
    register_camera_server_classes();
}

/// Registers `CameraServer` + `CameraFeed` + `CameraTexture` (pat-nzgdu.179).
///
/// Web parity is the point here. Godot's `modules/camera` implements drivers for
/// android/linux/macos/windows and **nothing for web**, so on a web export the
/// singleton exists and simply reports zero feeds rather than erroring. This is
/// the class surface for that: `CameraServer.get_feed_count()` answering 0 is
/// correct behaviour, not a stub.
///
/// The interpreter already dispatches `CameraServer` as a singleton and mints
/// `CameraFeed` handles (`interpreter.rs`, tag 0xCA) — this is the ClassDB half
/// that was missing, so reflection and the editor inspector can see the types.
///
/// Schemas diffed against `extension_api.json` (Godot 4.6.1.stable).
fn register_camera_server_classes() {
    if !class_exists("Object") {
        register_class(object_class());
    }
    if !class_exists("RefCounted") {
        register_class(ClassRegistration::new("RefCounted").parent("Object"));
    }

    // The global singleton. Godot exposes it via Engine, never `new()`.
    if !class_exists("CameraServer") {
        register_class(
            ClassRegistration::new("CameraServer")
                .parent("Object")
                .property(PropertyInfo::new("monitoring_feeds", Variant::Bool(false)))
                // NOTE the duplicate values — this is Godot's real enum, not a
                // typo: FeedImage indexes *within* a feed's datatype, so the
                // first plane of RGBA/YCbCr/Y all share 0 and only the second
                // CbCr plane is 1. Verified against the API dump; do not
                // "renumber" it.
                .enumeration(
                    EnumInfo::new("FeedImage")
                        .constant("FEED_RGBA_IMAGE", 0)
                        .constant("FEED_YCBCR_IMAGE", 0)
                        .constant("FEED_Y_IMAGE", 0)
                        .constant("FEED_CBCR_IMAGE", 1),
                )
                .signal(SignalInfo::new("camera_feed_added"))
                .signal(SignalInfo::new("camera_feed_removed"))
                .signal(SignalInfo::new("camera_feeds_updated")),
        );
    }

    // One camera. RefCounted, not a Resource — it is a live device handle.
    if !class_exists("CameraFeed") {
        register_class(
            ClassRegistration::new("CameraFeed")
                .parent("RefCounted")
                // Godot exposes id/name/position as methods, not properties —
                // only these three are the property surface.
                .property(PropertyInfo::new("feed_is_active", Variant::Bool(false)))
                .property(PropertyInfo::new(
                    "feed_transform",
                    Variant::Transform2D(gdcore::math::Transform2D::IDENTITY),
                ))
                .property(PropertyInfo::new("formats", Variant::Array(Vec::new())))
                .enumeration(
                    EnumInfo::new("FeedDataType")
                        .constant("FEED_NOIMAGE", 0)
                        .constant("FEED_RGB", 1)
                        .constant("FEED_YCBCR", 2)
                        .constant("FEED_YCBCR_SEP", 3)
                        .constant("FEED_EXTERNAL", 4),
                )
                .enumeration(
                    EnumInfo::new("FeedPosition")
                        .constant("FEED_UNSPECIFIED", 0)
                        .constant("FEED_FRONT", 1)
                        .constant("FEED_BACK", 2),
                )
                .signal(SignalInfo::new("frame_changed"))
                .signal(SignalInfo::new("format_changed")),
        );
    }

    // A Texture2D whose pixels come from a feed — what a TextureRect binds to.
    if !class_exists("CameraTexture") {
        register_class(
            ClassRegistration::new("CameraTexture")
                .parent("Texture2D")
                .property(PropertyInfo::new("camera_feed_id", Variant::Int(0)))
                .property(PropertyInfo::new("which_feed", Variant::Int(0)))
                .property(PropertyInfo::new("camera_is_active", Variant::Bool(false))),
        );
    }
}

/// Registers `RenderingDevice` and the RD\* parameter/spec classes
/// (pat-nzgdu.162).
///
/// These are the plain-data objects a script fills in and hands to
/// `RenderingDevice`'s methods — `RDTextureFormat` describes a texture to
/// allocate, `RDUniform` binds a resource to a set slot, `RDShaderSource`
/// carries GLSL to compile, and so on. Registering them makes
/// `ClassDB.class_exists("RDUniform")` and `class_get_property_list` answer, so
/// scripts and the editor's inspector can see the schema.
///
/// **Scope, deliberate:** this is the *schema* half of the bead. The device
/// itself is not implemented and is not reachable from script yet — see the
/// bead notes. In short: there is no GLSL→SPIRV compiler anywhere in the
/// workspace (no shaderc/naga), so `shader_compile_spirv_from_source` cannot be
/// written; `gdrender3d::wgpu_pipeline` is render-only with no compute
/// pipeline; and all of wgpu sits behind the non-default `gpu` feature. Method
/// surfaces are therefore left off `RenderingDevice` rather than advertising
/// ~100 methods that cannot run — the RD\* classes carry properties, which is
/// what they are.
fn register_rendering_device_classes() {
    if !class_exists("Object") {
        register_class(object_class());
    }
    if !class_exists("Resource") {
        register_class(ClassRegistration::new("Resource").parent("RefCounted"));
    }
    // `RefCounted` is not registered anywhere else in this module's viewport
    // path, and an unregistered base silently severs the chain from `Object`
    // (pat-nzgdu.98), so bootstrap it before the RD\* classes parent to it.
    if !class_exists("RefCounted") {
        register_class(ClassRegistration::new("RefCounted").parent("Object"));
    }

    // The device. Godot exposes it via `RenderingServer.get_rendering_device()`,
    // never `new()`. Registered so `class_exists`/inheritance answer; its method
    // surface lands with the implementation beads.
    if !class_exists("RenderingDevice") {
        register_class(ClassRegistration::new("RenderingDevice").parent("Object"));
    }

    // GLSL (or HLSL) source handed to `shader_compile_spirv_from_source`.
    if !class_exists("RDShaderSource") {
        register_class(
            ClassRegistration::new("RDShaderSource")
                .parent("RefCounted")
                .property(PropertyInfo::new(
                    "source_vertex",
                    Variant::String(String::new()),
                ))
                .property(PropertyInfo::new(
                    "source_fragment",
                    Variant::String(String::new()),
                ))
                .property(PropertyInfo::new(
                    "source_tesselation_control",
                    Variant::String(String::new()),
                ))
                .property(PropertyInfo::new(
                    "source_tesselation_evaluation",
                    Variant::String(String::new()),
                ))
                .property(PropertyInfo::new(
                    "source_compute",
                    Variant::String(String::new()),
                ))
                // ShaderLanguage: GLSL = 0, HLSL = 1.
                .property(PropertyInfo::new("language", Variant::Int(0))),
        );
    }

    // The compiled result: per-stage SPIR-V bytecode, or a per-stage error.
    if !class_exists("RDShaderSPIRV") {
        register_class(
            ClassRegistration::new("RDShaderSPIRV")
                .parent("Resource")
                .property(PropertyInfo::new(
                    "bytecode_vertex",
                    Variant::PackedByteArray(Vec::new()),
                ))
                .property(PropertyInfo::new(
                    "bytecode_fragment",
                    Variant::PackedByteArray(Vec::new()),
                ))
                .property(PropertyInfo::new(
                    "bytecode_tesselation_control",
                    Variant::PackedByteArray(Vec::new()),
                ))
                .property(PropertyInfo::new(
                    "bytecode_tesselation_evaluation",
                    Variant::PackedByteArray(Vec::new()),
                ))
                .property(PropertyInfo::new(
                    "bytecode_compute",
                    Variant::PackedByteArray(Vec::new()),
                ))
                .property(PropertyInfo::new(
                    "compile_error_vertex",
                    Variant::String(String::new()),
                ))
                .property(PropertyInfo::new(
                    "compile_error_fragment",
                    Variant::String(String::new()),
                ))
                .property(PropertyInfo::new(
                    "compile_error_tesselation_control",
                    Variant::String(String::new()),
                ))
                .property(PropertyInfo::new(
                    "compile_error_tesselation_evaluation",
                    Variant::String(String::new()),
                ))
                .property(PropertyInfo::new(
                    "compile_error_compute",
                    Variant::String(String::new()),
                )),
        );
    }

    // A compiled `.glsl` import: named SPIR-V variants plus a base error.
    if !class_exists("RDShaderFile") {
        register_class(
            ClassRegistration::new("RDShaderFile")
                .parent("Resource")
                .property(PropertyInfo::new(
                    "base_error",
                    Variant::String(String::new()),
                )),
        );
    }

    // Binds resources (buffers/textures/samplers) to one shader set slot.
    if !class_exists("RDUniform") {
        register_class(
            ClassRegistration::new("RDUniform")
                .parent("RefCounted")
                // UniformType: SAMPLER = 0.
                .property(PropertyInfo::new("uniform_type", Variant::Int(0)))
                .property(PropertyInfo::new("binding", Variant::Int(0))),
        );
    }

    // Describes a texture to allocate — the argument to `texture_create`.
    if !class_exists("RDTextureFormat") {
        register_class(
            ClassRegistration::new("RDTextureFormat")
                .parent("RefCounted")
                .property(PropertyInfo::new("format", Variant::Int(0)))
                .property(PropertyInfo::new("width", Variant::Int(1)))
                .property(PropertyInfo::new("height", Variant::Int(1)))
                .property(PropertyInfo::new("depth", Variant::Int(1)))
                .property(PropertyInfo::new("array_layers", Variant::Int(1)))
                .property(PropertyInfo::new("mipmaps", Variant::Int(1)))
                // TextureType: 2D = 1.
                .property(PropertyInfo::new("texture_type", Variant::Int(1)))
                // TextureSamples: 1x = 0.
                .property(PropertyInfo::new("samples", Variant::Int(0)))
                .property(PropertyInfo::new("usage_bits", Variant::Int(0)))
                .property(PropertyInfo::new("is_resolve_buffer", Variant::Bool(false)))
                // Godot 4.4+: lets the driver throw the contents away between
                // frames instead of storing them.
                .property(PropertyInfo::new("is_discardable", Variant::Bool(false))),
        );
    }

    // Reinterprets a texture's format/channel order at bind time.
    if !class_exists("RDTextureView") {
        register_class(
            ClassRegistration::new("RDTextureView")
                .parent("RefCounted")
                .property(PropertyInfo::new("format_override", Variant::Int(0)))
                .property(PropertyInfo::new("swizzle_r", Variant::Int(0)))
                .property(PropertyInfo::new("swizzle_g", Variant::Int(0)))
                .property(PropertyInfo::new("swizzle_b", Variant::Int(0)))
                .property(PropertyInfo::new("swizzle_a", Variant::Int(0))),
        );
    }

    // Filtering/wrap/compare state for `sampler_create`.
    if !class_exists("RDSamplerState") {
        register_class(
            ClassRegistration::new("RDSamplerState")
                .parent("RefCounted")
                .property(PropertyInfo::new("mag_filter", Variant::Int(0)))
                .property(PropertyInfo::new("min_filter", Variant::Int(0)))
                .property(PropertyInfo::new("mip_filter", Variant::Int(0)))
                .property(PropertyInfo::new("repeat_u", Variant::Int(0)))
                .property(PropertyInfo::new("repeat_v", Variant::Int(0)))
                .property(PropertyInfo::new("repeat_w", Variant::Int(0)))
                .property(PropertyInfo::new("lod_bias", Variant::Float(0.0)))
                .property(PropertyInfo::new("use_anisotropy", Variant::Bool(false)))
                .property(PropertyInfo::new("anisotropy_max", Variant::Float(1.0)))
                .property(PropertyInfo::new("enable_compare", Variant::Bool(false)))
                .property(PropertyInfo::new("compare_op", Variant::Int(0)))
                .property(PropertyInfo::new("min_lod", Variant::Float(0.0)))
                .property(PropertyInfo::new("max_lod", Variant::Float(1e20)))
                .property(PropertyInfo::new("border_color", Variant::Int(0)))
                .property(PropertyInfo::new("unnormalized_uvw", Variant::Bool(false))),
        );
    }

    // One vertex-buffer attribute in a vertex format.
    if !class_exists("RDVertexAttribute") {
        register_class(
            ClassRegistration::new("RDVertexAttribute")
                .parent("RefCounted")
                // The vertex-buffer slot — distinct from `location`, which is
                // the attribute's location in the shader.
                .property(PropertyInfo::new("binding", Variant::Int(0)))
                .property(PropertyInfo::new("location", Variant::Int(0)))
                .property(PropertyInfo::new("offset", Variant::Int(0)))
                .property(PropertyInfo::new("format", Variant::Int(0)))
                .property(PropertyInfo::new("stride", Variant::Int(0)))
                .property(PropertyInfo::new("frequency", Variant::Int(0))),
        );
    }

    // Overrides a SPIR-V specialization constant when building a pipeline.
    if !class_exists("RDPipelineSpecializationConstant") {
        register_class(
            ClassRegistration::new("RDPipelineSpecializationConstant")
                .parent("RefCounted")
                .property(PropertyInfo::new("value", Variant::Nil))
                .property(PropertyInfo::new("constant_id", Variant::Int(0))),
        );
    }

    // -- FramebufferCacheRD (pat-nzgdu.210) ----------------------------------
    //
    // ⚠️ Note the name shape: the RD\* classes above are *parameters* handed to
    // the device, but this is a `*RD` **helper** — a different family. It hangs
    // off `Object`, not `RefCounted`, and it is not something you fill in and
    // pass; it hands back a cached framebuffer RID.
    //
    // ⚠️ `get_cache_multipass` is STATIC (`qualifiers="static"`), called as
    // `FramebufferCacheRD.get_cache_multipass(...)` on the class itself.
    // `MethodInfo` cannot express staticness, so it registers like any other
    // method — the arity is right and introspection answers, but ClassDB cannot
    // tell a caller it must not be invoked on an instance.
    if !class_exists("FramebufferCacheRD") {
        register_class(
            ClassRegistration::new("FramebufferCacheRD")
                .parent("Object")
                // (textures: RID[], passes: RDFramebufferPass[], views: int)
                .method(MethodInfo::new("get_cache_multipass", 3)),
        );
    }
    // `get_cache_multipass`'s second parameter type. Registered here because a
    // registered method whose parameter class does not exist is unusable: a
    // script cannot build the argument at all, so the method resolves and is
    // still uncallable.
    if !class_exists("RDFramebufferPass") {
        register_class(
            ClassRegistration::new("RDFramebufferPass")
                .parent("RefCounted")
                .property(PropertyInfo::new(
                    "color_attachments",
                    Variant::PackedInt32Array(Vec::new()),
                ))
                .property(PropertyInfo::new(
                    "input_attachments",
                    Variant::PackedInt32Array(Vec::new()),
                ))
                .property(PropertyInfo::new(
                    "resolve_attachments",
                    Variant::PackedInt32Array(Vec::new()),
                ))
                .property(PropertyInfo::new(
                    "preserve_attachments",
                    Variant::PackedInt32Array(Vec::new()),
                ))
                // ⚠️ -1, not 0: this is ATTACHMENT_UNUSED, registered below.
                // 0 is a valid attachment index, so a 0 default would silently
                // mean "attachment 0" — a real depth attachment nobody asked
                // for — rather than "none".
                .property(PropertyInfo::new("depth_attachment", Variant::Int(-1)))
                // Godot binds an accessor pair per member (10 methods). The RD\*
                // classes registered above declare properties only and so answer
                // `class_has_method("RDVertexAttribute", "set_binding")` with
                // `false` where Godot says `true` — a gap across that whole
                // family (pat-nzgdu.162), not a convention. Following the ground
                // truth here rather than copying it.
                .method(MethodInfo::new("set_color_attachments", 1))
                .method(MethodInfo::new("get_color_attachments", 0))
                .method(MethodInfo::new("set_input_attachments", 1))
                .method(MethodInfo::new("get_input_attachments", 0))
                .method(MethodInfo::new("set_resolve_attachments", 1))
                .method(MethodInfo::new("get_resolve_attachments", 0))
                .method(MethodInfo::new("set_preserve_attachments", 1))
                .method(MethodInfo::new("get_preserve_attachments", 0))
                .method(MethodInfo::new("set_depth_attachment", 1))
                .method(MethodInfo::new("get_depth_attachment", 0))
                .constant("ATTACHMENT_UNUSED", -1),
        );
    }

    // -- UniformSetCacheRD (pat-nzgdu.215) -----------------------------------
    //
    // FramebufferCacheRD's sibling: the same `*RD` **helper** shape (an `Object`,
    // not a `RefCounted` parameter object) handing back a cached RID from a
    // static method. See pat-nzgdu.210 for why `get_cache`'s staticness cannot
    // be expressed here — `MethodInfo` is a name plus an arity.
    if !class_exists("UniformSetCacheRD") {
        register_class(
            ClassRegistration::new("UniformSetCacheRD")
                .parent("Object")
                // static: (shader, set, uniforms)
                .method(MethodInfo::new("get_cache", 3)),
        );
    }

    register_compositor_render_data_classes();
}

/// Registers the per-frame render-data objects a `CompositorEffect` callback
/// receives (pat-nzgdu.213).
///
/// Ten classes in three families, each with the same shape: an **abstract base**
/// carrying the public getters, an **`*Extension`** whose `_`-prefixed virtuals
/// a GDExtension overrides, and an **`*RD`** concrete implementation.
///
/// ⚠️ `RenderDataRD` and `RenderSceneDataRD` bind **nothing at all** — their
/// entire surface is inherited from their base. That is not an omission: adding
/// methods to them would invent a ClassDB surface Godot does not have.
///
/// **Registration only, deliberately.** These objects are handed to a
/// `CompositorEffect._render_callback` over a live `RenderingDevice`, and Patina
/// has neither: `CompositorEffect` is unregistered, there is no RD (pat-nzgdu.162),
/// and the buffers hand back **`RID`s**, which `Variant` cannot represent at all
/// (pat-nzgdu.210). Registering the surface is what makes `class_exists` and
/// `class_get_method_list` answer as Godot does; the frame data lands with the RD
/// implementation.
///
/// The method surface was **generated** from `extension_api.json` rather than
/// transcribed — 73 methods with arities from 0 to 10 is exactly where hand-copying
/// invents errors (the pat-nzgdu.204 lesson).
fn register_compositor_render_data_classes() {
    if !class_exists("Object") {
        register_class(object_class());
    }
    if !class_exists("RefCounted") {
        register_class(ClassRegistration::new("RefCounted").parent("Object"));
    }

    // -- RenderData: what the effect callback is handed ----------------------
    if !class_exists("RenderData") {
        register_class(
            ClassRegistration::new("RenderData")
                .parent("Object")
                .abstract_class()
                .method(MethodInfo::new("get_render_scene_buffers", 0))
                .method(MethodInfo::new("get_render_scene_data", 0))
                .method(MethodInfo::new("get_environment", 0))
                .method(MethodInfo::new("get_camera_attributes", 0)),
        );
    }
    if !class_exists("RenderDataExtension") {
        register_class(
            ClassRegistration::new("RenderDataExtension")
                .parent("RenderData")
                .method(MethodInfo::new("_get_render_scene_buffers", 0))
                .method(MethodInfo::new("_get_render_scene_data", 0))
                .method(MethodInfo::new("_get_environment", 0))
                .method(MethodInfo::new("_get_camera_attributes", 0)),
        );
    }
    // ⚠️ Binds nothing — the whole surface is RenderData's.
    if !class_exists("RenderDataRD") {
        register_class(ClassRegistration::new("RenderDataRD").parent("RenderData"));
    }

    // -- RenderSceneData: camera/view matrices -------------------------------
    if !class_exists("RenderSceneData") {
        register_class(
            ClassRegistration::new("RenderSceneData")
                .parent("Object")
                .abstract_class()
                .method(MethodInfo::new("get_cam_transform", 0))
                .method(MethodInfo::new("get_cam_projection", 0))
                .method(MethodInfo::new("get_view_count", 0))
                .method(MethodInfo::new("get_view_eye_offset", 1))
                .method(MethodInfo::new("get_view_projection", 1))
                .method(MethodInfo::new("get_uniform_buffer", 0)),
        );
    }
    if !class_exists("RenderSceneDataExtension") {
        register_class(
            ClassRegistration::new("RenderSceneDataExtension")
                .parent("RenderSceneData")
                .method(MethodInfo::new("_get_cam_transform", 0))
                .method(MethodInfo::new("_get_cam_projection", 0))
                .method(MethodInfo::new("_get_view_count", 0))
                .method(MethodInfo::new("_get_view_eye_offset", 1))
                .method(MethodInfo::new("_get_view_projection", 1))
                .method(MethodInfo::new("_get_uniform_buffer", 0)),
        );
    }
    // ⚠️ Binds nothing — the whole surface is RenderSceneData's.
    if !class_exists("RenderSceneDataRD") {
        register_class(ClassRegistration::new("RenderSceneDataRD").parent("RenderSceneData"));
    }

    // -- RenderSceneBuffers: the color/depth/velocity textures ---------------
    //
    // ⚠️ RefCounted, NOT Object — unlike the two families above. The buffers are
    // shared per-viewport and outlive a single callback; RenderData does not.
    if !class_exists("RenderSceneBuffers") {
        register_class(
            ClassRegistration::new("RenderSceneBuffers")
                .parent("RefCounted")
                .abstract_class()
                .method(MethodInfo::new("configure", 1)),
        );
    }
    // ⚠️ NOT a 1:1 mirror of its base, unlike the *Extension classes above: the
    // base binds one method, this binds FIVE virtuals. The four `_set_*` hooks
    // have no public counterpart at all — assuming the mirror pattern here would
    // drop them.
    if !class_exists("RenderSceneBuffersExtension") {
        register_class(
            ClassRegistration::new("RenderSceneBuffersExtension")
                .parent("RenderSceneBuffers")
                .method(MethodInfo::new("_configure", 1))
                .method(MethodInfo::new("_set_fsr_sharpness", 1))
                .method(MethodInfo::new("_set_texture_mipmap_bias", 1))
                .method(MethodInfo::new("_set_anisotropic_filtering_level", 1))
                .method(MethodInfo::new("_set_use_debanding", 1)),
        );
    }
    // The one class with a real surface: 27 methods over the frame's textures.
    // ⚠️ `get_color_texture`/`get_depth_texture` take an argument (the MSAA
    // flag) — they are NOT the zero-arg getters their names suggest.
    if !class_exists("RenderSceneBuffersRD") {
        register_class(
            ClassRegistration::new("RenderSceneBuffersRD")
                .parent("RenderSceneBuffers")
                .method(MethodInfo::new("has_texture", 2))
                .method(MethodInfo::new("create_texture", 10))
                .method(MethodInfo::new("create_texture_from_format", 5))
                .method(MethodInfo::new("create_texture_view", 4))
                .method(MethodInfo::new("get_texture", 2))
                .method(MethodInfo::new("get_texture_format", 2))
                .method(MethodInfo::new("get_texture_slice", 6))
                .method(MethodInfo::new("get_texture_slice_view", 7))
                .method(MethodInfo::new("get_texture_slice_size", 3))
                .method(MethodInfo::new("clear_context", 1))
                .method(MethodInfo::new("get_color_texture", 1))
                .method(MethodInfo::new("get_color_layer", 2))
                .method(MethodInfo::new("get_depth_texture", 1))
                .method(MethodInfo::new("get_depth_layer", 2))
                .method(MethodInfo::new("get_velocity_texture", 1))
                .method(MethodInfo::new("get_velocity_layer", 2))
                .method(MethodInfo::new("get_render_target", 0))
                .method(MethodInfo::new("get_view_count", 0))
                .method(MethodInfo::new("get_internal_size", 0))
                .method(MethodInfo::new("get_target_size", 0))
                .method(MethodInfo::new("get_scaling_3d_mode", 0))
                .method(MethodInfo::new("get_fsr_sharpness", 0))
                .method(MethodInfo::new("get_msaa_3d", 0))
                .method(MethodInfo::new("get_texture_samples", 0))
                .method(MethodInfo::new("get_screen_space_aa", 0))
                .method(MethodInfo::new("get_use_taa", 0))
                .method(MethodInfo::new("get_use_debanding", 0)),
        );
    }

    // -- RenderSceneBuffersConfiguration: how the buffers get allocated ------
    //
    // ⚠️ Property TYPES here come from the doc XML, NOT from
    // `extension_api.json`, because the dump is **internally inconsistent**: it
    // reports `fsr_sharpness` and `texture_mipmap_bias` as `bool` while its own
    // `set_fsr_sharpness(float)` signature — and the XML's
    // `type="float" default="0.0"` — say otherwise. The XML wins.
    //
    // ⚠️ Three of the ten members have NO representable default: `render_target`
    // is an `RID` and `internal_size`/`target_size` are `Vector2i`, and
    // `Variant` carries neither (`gdvariant::binary` names both among the types
    // the enum lacks). They register as `Nil` — the property exists so
    // introspection matches Godot, but the value is absent rather than a
    // `Vector2` standing in for a `Vector2i`, which would make `typeof()` lie.
    if !class_exists("RenderSceneBuffersConfiguration") {
        register_class(
            ClassRegistration::new("RenderSceneBuffersConfiguration")
                .parent("RefCounted")
                .property(PropertyInfo::new("render_target", Variant::Nil))
                .property(PropertyInfo::new("internal_size", Variant::Nil))
                .property(PropertyInfo::new("target_size", Variant::Nil))
                // ⚠️ 1, not 0 — a zero view count would allocate nothing.
                .property(PropertyInfo::new("view_count", Variant::Int(1)))
                // ⚠️ 255, not 0: SCALING_3D_MODE_MAX is the "unset" sentinel
                // here, so 0 (BILINEAR) would silently pick a real mode.
                .property(PropertyInfo::new("scaling_3d_mode", Variant::Int(255)))
                .property(PropertyInfo::new("msaa_3d", Variant::Int(0)))
                .property(PropertyInfo::new("screen_space_aa", Variant::Int(0)))
                .property(PropertyInfo::new("fsr_sharpness", Variant::Float(0.0)))
                .property(PropertyInfo::new(
                    "texture_mipmap_bias",
                    Variant::Float(0.0),
                ))
                // ⚠️ 2, not 0 — ANISOTROPY_4X is Godot's default.
                .property(PropertyInfo::new(
                    "anisotropic_filtering_level",
                    Variant::Int(2),
                ))
                .method(MethodInfo::new("get_render_target", 0))
                .method(MethodInfo::new("set_render_target", 1))
                .method(MethodInfo::new("get_internal_size", 0))
                .method(MethodInfo::new("set_internal_size", 1))
                .method(MethodInfo::new("get_target_size", 0))
                .method(MethodInfo::new("set_target_size", 1))
                .method(MethodInfo::new("get_view_count", 0))
                .method(MethodInfo::new("set_view_count", 1))
                .method(MethodInfo::new("get_scaling_3d_mode", 0))
                .method(MethodInfo::new("set_scaling_3d_mode", 1))
                .method(MethodInfo::new("get_msaa_3d", 0))
                .method(MethodInfo::new("set_msaa_3d", 1))
                .method(MethodInfo::new("get_screen_space_aa", 0))
                .method(MethodInfo::new("set_screen_space_aa", 1))
                .method(MethodInfo::new("get_fsr_sharpness", 0))
                .method(MethodInfo::new("set_fsr_sharpness", 1))
                .method(MethodInfo::new("get_texture_mipmap_bias", 0))
                .method(MethodInfo::new("set_texture_mipmap_bias", 1))
                .method(MethodInfo::new("get_anisotropic_filtering_level", 0))
                .method(MethodInfo::new("set_anisotropic_filtering_level", 1)),
        );
    }
}

/// Registers the networking node/class hierarchy in the ClassDB.
///
/// Adds Godot's HTTP and WebSocket types so GDScript referencing
/// `HTTPRequest`, `HTTPClient`, or `WebSocketPeer` resolves against ClassDB:
///
/// Registers audio player node classes matching Godot's scene/audio hierarchy.
///
/// * `AudioStreamPlayer : Node`
/// * `AudioStreamPlayer2D : Node2D`
/// * `AudioStreamPlayer3D : Node3D`
///
/// Idempotent — each class is only registered if not already present.
pub fn register_audio_classes() {
    if !class_exists("Object") {
        register_class(object_class());
    }
    if !class_exists("Node") {
        register_class(node_class());
    }
    if !class_exists("RefCounted") {
        register_class(ClassRegistration::new("RefCounted").parent("Object"));
    }

    // -- AudioStreamPlayback runtime-instance family (pat-nzgdu.153) -------
    // Godot separates the *resource* (AudioStream) from the *runtime instance*
    // it hands out: AudioStream.instantiate_playback() returns an
    // AudioStreamPlayback (a RefCounted, not a Resource) that the mixer pulls
    // frames from. AudioStreamPlaybackResampled sits between the base and the
    // decoders that need sample-rate conversion (Ogg, the generator); the
    // container playbacks derive the base directly, since they mix
    // sub-playbacks that have already been resampled.
    if !class_exists("AudioStreamPlayback") {
        register_class(
            ClassRegistration::new("AudioStreamPlayback")
                .parent("RefCounted")
                .abstract_class()
                .method(MethodInfo::new("start", 0))
                .method(MethodInfo::new("stop", 0))
                .method(MethodInfo::new("is_playing", 0))
                .method(MethodInfo::new("seek", 0))
                .method(MethodInfo::new("get_playback_position", 0))
                .method(MethodInfo::new("mix", 3))
                .method(MethodInfo::new("get_loop_count", 0)),
        );
    }

    // -- Sample pool (pat-nzgdu.187) ---------------------------------------
    //
    // ⚠️ BOTH register with NOTHING but a parent, and that is EXACT. Godot's
    // `AudioSample` and `AudioSamplePlayback` are plain data structs carrying
    // `GDCLASS(..., RefCounted)` and **no `_bind_methods()`**
    // (`servers/audio/audio_stream.h:43-74`). They are GDREGISTER_CLASS'd
    // (`servers/register_server_types.cpp:171-172`), so ClassDB knows the name
    // and parent — but every field is a public C++ member, NOT a bound property,
    // and `AudioSample::LoopMode` has no BIND_ENUM_CONSTANT/VARIANT_ENUM_CAST,
    // so it is not a ClassDB enum either.
    //
    // The bead describes AudioSample as "wraps an AudioStream + LoopMode": true
    // of the C++ struct, but none of it is visible to ClassDB or GDScript.
    // Registering `stream`/`loop_mode`/`pitch_scale` as properties would invent
    // a schema Godot does not have. These are opaque handles by design — the
    // engine hands one to the audio driver, and only the driver looks inside.
    register_class(ClassRegistration::new("AudioSample").parent("RefCounted"));
    register_class(ClassRegistration::new("AudioSamplePlayback").parent("RefCounted"));
    if !class_exists("AudioStreamPlaybackResampled") {
        register_class(
            ClassRegistration::new("AudioStreamPlaybackResampled")
                .parent("AudioStreamPlayback")
                .abstract_class()
                .method(MethodInfo::new("begin_resample", 0))
                .method(MethodInfo::new("get_stream_sampling_rate", 0)),
        );
    }
    // Decoder playbacks — these resample.
    if !class_exists("AudioStreamPlaybackOggVorbis") {
        register_class(
            ClassRegistration::new("AudioStreamPlaybackOggVorbis")
                .parent("AudioStreamPlaybackResampled"),
        );
    }
    if !class_exists("AudioStreamGeneratorPlayback") {
        register_class(
            ClassRegistration::new("AudioStreamGeneratorPlayback")
                .parent("AudioStreamPlaybackResampled")
                .method(MethodInfo::new("push_frame", 1))
                .method(MethodInfo::new("can_push_buffer", 1))
                .method(MethodInfo::new("push_buffer", 1))
                .method(MethodInfo::new("get_frames_available", 0))
                .method(MethodInfo::new("get_skips", 0))
                .method(MethodInfo::new("clear_buffer", 0)),
        );
    }
    // Container playbacks — these mix already-resampled sub-playbacks.
    if !class_exists("AudioStreamPlaybackPolyphonic") {
        register_class(
            ClassRegistration::new("AudioStreamPlaybackPolyphonic")
                .parent("AudioStreamPlayback")
                .method(MethodInfo::new("play_stream", 1))
                .method(MethodInfo::new("set_stream_volume", 2))
                .method(MethodInfo::new("set_stream_pitch_scale", 2))
                .method(MethodInfo::new("is_stream_playing", 1))
                .method(MethodInfo::new("stop_stream", 1)),
        );
    }
    if !class_exists("AudioStreamPlaybackInteractive") {
        register_class(
            ClassRegistration::new("AudioStreamPlaybackInteractive")
                .parent("AudioStreamPlayback")
                .method(MethodInfo::new("switch_to_clip_by_name", 1))
                .method(MethodInfo::new("switch_to_clip", 1))
                .method(MethodInfo::new("get_current_clip_index", 0)),
        );
    }
    if !class_exists("AudioStreamPlaybackPlaylist") {
        register_class(
            ClassRegistration::new("AudioStreamPlaybackPlaylist").parent("AudioStreamPlayback"),
        );
    }
    if !class_exists("AudioStreamPlaybackSynchronized") {
        register_class(
            ClassRegistration::new("AudioStreamPlaybackSynchronized").parent("AudioStreamPlayback"),
        );
    }

    // Ensure 2D/3D bases exist for the positional players.
    if !class_exists("Node2D") {
        register_2d_classes();
    }
    if !class_exists("Node3D") {
        register_3d_classes();
    }

    if !class_exists("AudioStreamPlayer") {
        register_class(
            ClassRegistration::new("AudioStreamPlayer")
                .parent("Node")
                .property(PropertyInfo::new("stream", Variant::String(String::new())))
                .property(PropertyInfo::new("volume_db", Variant::Float(0.0)))
                .property(PropertyInfo::new("pitch_scale", Variant::Float(1.0)))
                .property(PropertyInfo::new("autoplay", Variant::Bool(false)))
                .property(PropertyInfo::new("bus", Variant::String("Master".into())))
                .property(PropertyInfo::new("playing", Variant::Bool(false)))
                .method(MethodInfo::new("play", 1))
                .method(MethodInfo::new("stop", 0))
                .method(MethodInfo::new("is_playing", 0)),
        );
    }
    if !class_exists("AudioStreamPlayer2D") {
        register_class(
            ClassRegistration::new("AudioStreamPlayer2D")
                .parent("Node2D")
                .property(PropertyInfo::new("stream", Variant::String(String::new())))
                .property(PropertyInfo::new("volume_db", Variant::Float(0.0)))
                .property(PropertyInfo::new("pitch_scale", Variant::Float(1.0)))
                .property(PropertyInfo::new("autoplay", Variant::Bool(false)))
                .property(PropertyInfo::new("bus", Variant::String("Master".into())))
                .property(PropertyInfo::new("playing", Variant::Bool(false)))
                .property(PropertyInfo::new("max_distance", Variant::Float(2000.0)))
                // Falloff exponent and pan width — the audio bridge mirrors both
                // onto the spatial calculation (pat-0q2kx.57).
                .property(PropertyInfo::new("attenuation", Variant::Float(1.0)))
                .property(PropertyInfo::new("panning_strength", Variant::Float(1.0)))
                .method(MethodInfo::new("play", 1))
                .method(MethodInfo::new("stop", 0))
                .method(MethodInfo::new("is_playing", 0)),
        );
    }
    if !class_exists("AudioStreamPlayer3D") {
        register_class(
            ClassRegistration::new("AudioStreamPlayer3D")
                .parent("Node3D")
                .property(PropertyInfo::new("stream", Variant::String(String::new())))
                .property(PropertyInfo::new("volume_db", Variant::Float(0.0)))
                .property(PropertyInfo::new("pitch_scale", Variant::Float(1.0)))
                .property(PropertyInfo::new("autoplay", Variant::Bool(false)))
                .property(PropertyInfo::new("bus", Variant::String("Master".into())))
                .property(PropertyInfo::new("playing", Variant::Bool(false)))
                .property(PropertyInfo::new("max_distance", Variant::Float(0.0)))
                .property(PropertyInfo::new("unit_size", Variant::Float(10.0)))
                // The rest of Godot's spatial surface, all mirrored per frame by
                // the audio bridge (pat-0q2kx.57). `attenuation_model` is the
                // `AudioStreamPlayer3D.AttenuationModel` enum: 0 inverse,
                // 1 inverse-square, 2 logarithmic, 3 disabled.
                .property(PropertyInfo::new("attenuation_model", Variant::Int(0)))
                .property(PropertyInfo::new("max_db", Variant::Float(3.0)))
                .property(PropertyInfo::new("panning_strength", Variant::Float(1.0)))
                .method(MethodInfo::new("play", 1))
                .method(MethodInfo::new("stop", 0))
                .method(MethodInfo::new("is_playing", 0)),
        );
    }

    // -- The AudioStream *resource* side (pat-nzgdu.188) -----------------------
    //
    // ⚠️ Every `AudioStream*Playback*` class above was already registered — but
    // not one of the *streams* they play. Godot splits the resource
    // (`AudioStream`, holding the data) from the runtime instance
    // (`AudioStreamPlayback`, pulling frames from it); Patina had only ever
    // registered the second half, so an `AudioStreamOggVorbis` could be played
    // but never named.
    //
    // `Resource` needs its own guard: this function ensures Object/Node/RefCounted
    // but never Resource, and every class below descends from it.
    if !class_exists("Resource") {
        register_class(ClassRegistration::new("Resource").parent("RefCounted"));
    }
    if !class_exists("AudioStream") {
        // GDREGISTER_CLASS in servers/register_server_types.cpp:166 — concrete,
        // not abstract, despite being a base (cf. AudioStreamPlayback above).
        register_class(
            ClassRegistration::new("AudioStream")
                .parent("Resource")
                .method(MethodInfo::new("get_length", 0))
                .method(MethodInfo::new("is_monophonic", 0))
                .method(MethodInfo::new("instantiate_playback", 0))
                .method(MethodInfo::new("can_be_sampled", 0))
                .method(MethodInfo::new("generate_sample", 0))
                .method(MethodInfo::new("is_meta_stream", 0)),
        );
    }
    if !class_exists("OggPacketSequence") {
        register_class(
            ClassRegistration::new("OggPacketSequence")
                .parent("Resource")
                // ⚠️ `packet_data` is Variant::ARRAY — an array of *pages*, each
                // itself an array of packets (C++ `Vector<Vector<PackedByteArray>>`,
                // bound as `TypedArray<Array>`). The api dump types this
                // `typedarray::PackedByteArray`, i.e. a flat array of byte arrays,
                // which loses the page grouping the granule positions index against.
                // ADD_PROPERTY at modules/ogg/ogg_packet_sequence.cpp:131 is the truth.
                .property(PropertyInfo::new("packet_data", Variant::Array(Vec::new())))
                // ⚠️ Named `granule_positions`, but its accessors are
                // `{set,get}_packet_granule_positions` — and the values are one per
                // PAGE, not per packet, despite that name (`page_granule_positions`
                // in the header).
                .property(PropertyInfo::new(
                    "granule_positions",
                    Variant::PackedInt64Array(Vec::new()),
                ))
                .property(PropertyInfo::new("sampling_rate", Variant::Float(0.0)))
                .method(MethodInfo::new("set_packet_data", 1))
                .method(MethodInfo::new("get_packet_data", 0))
                .method(MethodInfo::new("set_packet_granule_positions", 1))
                .method(MethodInfo::new("get_packet_granule_positions", 0))
                .method(MethodInfo::new("set_sampling_rate", 1))
                .method(MethodInfo::new("get_sampling_rate", 0))
                .method(MethodInfo::new("get_length", 0)),
        );
    }
    if !class_exists("OggPacketSequencePlayback") {
        // Binds nothing — Godot hands it out only as an opaque handle.
        // `OggPacketSequence::instantiate_playback()` exists in C++ but is
        // deliberately NOT in _bind_methods, so it must not be registered.
        register_class(ClassRegistration::new("OggPacketSequencePlayback").parent("RefCounted"));
    }
    if !class_exists("AudioStreamOggVorbis") {
        register_class(
            ClassRegistration::new("AudioStreamOggVorbis")
                .parent("AudioStream")
                .property(PropertyInfo::new("packet_sequence", Variant::Nil))
                .property(PropertyInfo::new("bpm", Variant::Float(0.0)))
                .property(PropertyInfo::new("beat_count", Variant::Int(0)))
                // ⚠️ 4, not 0 — a bar is four beats unless told otherwise.
                .property(PropertyInfo::new("bar_beats", Variant::Int(4)))
                .property(PropertyInfo::new(
                    "tags",
                    Variant::Dictionary(gdvariant::Dictionary::new()),
                ))
                .property(PropertyInfo::new("loop", Variant::Bool(false)))
                .property(PropertyInfo::new("loop_offset", Variant::Float(0.0)))
                .method(MethodInfo::new("load_from_buffer", 1))
                .method(MethodInfo::new("load_from_file", 1))
                .method(MethodInfo::new("set_packet_sequence", 1))
                .method(MethodInfo::new("get_packet_sequence", 0))
                .method(MethodInfo::new("set_loop", 1))
                .method(MethodInfo::new("has_loop", 0))
                .method(MethodInfo::new("set_loop_offset", 1))
                .method(MethodInfo::new("get_loop_offset", 0))
                .method(MethodInfo::new("set_bpm", 1))
                .method(MethodInfo::new("get_bpm", 0))
                .method(MethodInfo::new("set_beat_count", 1))
                .method(MethodInfo::new("get_beat_count", 0))
                .method(MethodInfo::new("set_bar_beats", 1))
                .method(MethodInfo::new("get_bar_beats", 0))
                .method(MethodInfo::new("set_tags", 1))
                .method(MethodInfo::new("get_tags", 0)),
        );
    }
}

/// Registers core IO utility classes matching Godot's `FileAccess` / `DirAccess`.
///
/// * `FileAccess : RefCounted` (core/io/file_access)
/// * `DirAccess : RefCounted` (core/io/dir_access)
/// * `ZIPReader : RefCounted` (core/io/zip_reader)
/// * `ZIPPacker : RefCounted` (core/io/zip_packer)
///
/// Idempotent — each class is only registered if not already present.
pub fn register_io_classes() {
    if !class_exists("Object") {
        register_class(object_class());
    }
    if !class_exists("RefCounted") {
        register_class(ClassRegistration::new("RefCounted").parent("Object"));
    }

    // -- Script / ScriptLanguage + the GDExtension hooks (pat-nzgdu.204) ---
    //
    // SCOPE: the class surface only. Native-plugin loading is not feasible on a
    // wasm target, so nothing implements these virtuals — see
    // prd/UPSTREAM_GAP_MATRIX.md § "2026-07-17 Scope Decisions". Registering them
    // keeps ClassDB introspection matching Godot and stops a .tscn/.tres that
    // references them from silently failing.
    //
    // NOTE the bead names only the two *Extension classes, but BOTH their parents
    // were missing too — a child over an absent parent has a silently severed
    // chain (pat-nzgdu.98), so Script and ScriptLanguage are registered here as
    // well.
    if !class_exists("Script") {
        register_class(
            ClassRegistration::new("Script")
                // A Resource: scripts are saved assets.
                .parent("Resource")
                .abstract_class()
                .property(PropertyInfo::new(
                    "source_code",
                    Variant::String(String::new()),
                ))
                .method(MethodInfo::new("can_instantiate", 0))
                .method(MethodInfo::new("instance_has", 1))
                .method(MethodInfo::new("has_source_code", 0))
                .method(MethodInfo::new("get_source_code", 0))
                .method(MethodInfo::new("set_source_code", 1))
                .method(MethodInfo::new("reload", 0))
                .method(MethodInfo::new("get_base_script", 0))
                .method(MethodInfo::new("get_instance_base_type", 0))
                .method(MethodInfo::new("get_global_name", 0))
                .method(MethodInfo::new("has_script_signal", 1))
                .method(MethodInfo::new("get_script_property_list", 0))
                .method(MethodInfo::new("get_script_method_list", 0))
                .method(MethodInfo::new("get_script_signal_list", 0))
                .method(MethodInfo::new("get_script_constant_map", 0))
                .method(MethodInfo::new("get_property_default_value", 1))
                .method(MethodInfo::new("is_tool", 0))
                .method(MethodInfo::new("is_abstract", 0))
                .method(MethodInfo::new("get_rpc_config", 0)),
        );
    }
    // A pure marker base: Godot binds NOTHING on it — no properties, no methods.
    // The whole language surface lives on ScriptLanguageExtension's virtuals.
    if !class_exists("ScriptLanguage") {
        register_class(
            ClassRegistration::new("ScriptLanguage")
                .parent("Object")
                .abstract_class(),
        );
    }
    // ⚠️ Both Extensions are PURE VIRTUAL SURFACES (zero non-virtual methods) and
    // are INSTANTIABLE in Godot — unlike their abstract parents. That asymmetry is
    // the point: you construct the Extension and override its virtuals from a
    // GDExtension. Without a native plugin providing them, an instance is inert —
    // which is exactly Godot's behaviour too, not a Patina limitation.
    if !class_exists("ScriptExtension") {
        register_class(
            ClassRegistration::new("ScriptExtension")
                .parent("Script")
                .method(MethodInfo::new("_editor_can_reload_from_file", 0))
                .method(MethodInfo::new("_placeholder_erased", 1))
                .method(MethodInfo::new("_can_instantiate", 0))
                .method(MethodInfo::new("_get_base_script", 0))
                .method(MethodInfo::new("_get_global_name", 0))
                .method(MethodInfo::new("_inherits_script", 1))
                .method(MethodInfo::new("_get_instance_base_type", 0))
                .method(MethodInfo::new("_instance_create", 1))
                .method(MethodInfo::new("_placeholder_instance_create", 1))
                .method(MethodInfo::new("_instance_has", 1))
                .method(MethodInfo::new("_has_source_code", 0))
                .method(MethodInfo::new("_get_source_code", 0))
                .method(MethodInfo::new("_set_source_code", 1))
                .method(MethodInfo::new("_reload", 1))
                .method(MethodInfo::new("_get_doc_class_name", 0))
                .method(MethodInfo::new("_get_documentation", 0))
                .method(MethodInfo::new("_get_class_icon_path", 0))
                .method(MethodInfo::new("_has_method", 1))
                .method(MethodInfo::new("_has_static_method", 1))
                .method(MethodInfo::new("_get_script_method_argument_count", 1))
                .method(MethodInfo::new("_get_method_info", 1))
                .method(MethodInfo::new("_is_tool", 0))
                .method(MethodInfo::new("_is_valid", 0))
                .method(MethodInfo::new("_is_abstract", 0))
                .method(MethodInfo::new("_get_language", 0))
                .method(MethodInfo::new("_has_script_signal", 1))
                .method(MethodInfo::new("_get_script_signal_list", 0))
                .method(MethodInfo::new("_has_property_default_value", 1))
                .method(MethodInfo::new("_get_property_default_value", 1))
                .method(MethodInfo::new("_update_exports", 0))
                .method(MethodInfo::new("_get_script_method_list", 0))
                .method(MethodInfo::new("_get_script_property_list", 0))
                .method(MethodInfo::new("_get_member_line", 1))
                .method(MethodInfo::new("_get_constants", 0))
                .method(MethodInfo::new("_get_members", 0))
                .method(MethodInfo::new("_is_placeholder_fallback_enabled", 0))
                .method(MethodInfo::new("_get_rpc_config", 0)),
        );
    }
    if !class_exists("ScriptLanguageExtension") {
        register_class(
            ClassRegistration::new("ScriptLanguageExtension")
                .parent("ScriptLanguage")
                .method(MethodInfo::new("_get_name", 0))
                .method(MethodInfo::new("_init", 0))
                .method(MethodInfo::new("_get_type", 0))
                .method(MethodInfo::new("_get_extension", 0))
                .method(MethodInfo::new("_finish", 0))
                .method(MethodInfo::new("_get_reserved_words", 0))
                .method(MethodInfo::new("_is_control_flow_keyword", 1))
                .method(MethodInfo::new("_get_comment_delimiters", 0))
                .method(MethodInfo::new("_get_doc_comment_delimiters", 0))
                .method(MethodInfo::new("_get_string_delimiters", 0))
                .method(MethodInfo::new("_make_template", 3))
                .method(MethodInfo::new("_get_built_in_templates", 1))
                .method(MethodInfo::new("_is_using_templates", 0))
                .method(MethodInfo::new("_validate", 6))
                .method(MethodInfo::new("_validate_path", 1))
                .method(MethodInfo::new("_create_script", 0))
                .method(MethodInfo::new("_has_named_classes", 0))
                .method(MethodInfo::new("_supports_builtin_mode", 0))
                .method(MethodInfo::new("_supports_documentation", 0))
                .method(MethodInfo::new("_can_inherit_from_file", 0))
                .method(MethodInfo::new("_find_function", 2))
                .method(MethodInfo::new("_make_function", 3))
                .method(MethodInfo::new("_can_make_function", 0))
                .method(MethodInfo::new("_open_in_external_editor", 3))
                .method(MethodInfo::new("_overrides_external_editor", 0))
                .method(MethodInfo::new("_preferred_file_name_casing", 0))
                .method(MethodInfo::new("_complete_code", 3))
                .method(MethodInfo::new("_lookup_code", 4))
                .method(MethodInfo::new("_auto_indent_code", 3))
                .method(MethodInfo::new("_add_global_constant", 2))
                .method(MethodInfo::new("_add_named_global_constant", 2))
                .method(MethodInfo::new("_remove_named_global_constant", 1))
                .method(MethodInfo::new("_thread_enter", 0))
                .method(MethodInfo::new("_thread_exit", 0))
                .method(MethodInfo::new("_debug_get_error", 0))
                .method(MethodInfo::new("_debug_get_stack_level_count", 0))
                .method(MethodInfo::new("_debug_get_stack_level_line", 1))
                .method(MethodInfo::new("_debug_get_stack_level_function", 1))
                .method(MethodInfo::new("_debug_get_stack_level_source", 1))
                .method(MethodInfo::new("_debug_get_stack_level_locals", 3))
                .method(MethodInfo::new("_debug_get_stack_level_members", 3))
                .method(MethodInfo::new("_debug_get_stack_level_instance", 1))
                .method(MethodInfo::new("_debug_get_globals", 2))
                .method(MethodInfo::new("_debug_parse_stack_level_expression", 4))
                .method(MethodInfo::new("_debug_get_current_stack_info", 0))
                .method(MethodInfo::new("_reload_all_scripts", 0))
                .method(MethodInfo::new("_reload_scripts", 2))
                .method(MethodInfo::new("_reload_tool_script", 2))
                .method(MethodInfo::new("_get_recognized_extensions", 0))
                .method(MethodInfo::new("_get_public_functions", 0))
                .method(MethodInfo::new("_get_public_constants", 0))
                .method(MethodInfo::new("_get_public_annotations", 0))
                .method(MethodInfo::new("_profiling_start", 0))
                .method(MethodInfo::new("_profiling_stop", 0))
                .method(MethodInfo::new("_profiling_set_save_native_calls", 1))
                .method(MethodInfo::new("_profiling_get_accumulated_data", 2))
                .method(MethodInfo::new("_profiling_get_frame_data", 2))
                .method(MethodInfo::new("_frame", 0))
                .method(MethodInfo::new("_handles_global_class_type", 1))
                .method(MethodInfo::new("_get_global_class_name", 1)),
        );
    }

    // -- GodotInstance (pat-nzgdu.203) — SCOPE DECISION, NOT A GAP ---------
    //
    // OUT_OF_SCOPE for the browser target, decided 2026-07-17. Cited in
    // prd/UPSTREAM_GAP_MATRIX.md § "2026-07-17 Scope Decisions". Do NOT re-open
    // this as MISSING, and do NOT build a host-embedding runtime here.
    //
    // GodotInstance is a *host embedding* handle: one per process, created by the
    // C API `libgodot_create_godot_instance`, driven by start()/iteration() from
    // the host application's loop (`core/extension/godot_instance.h:36-57`).
    // Upstream ships that entry point for linuxbsd and windows ONLY — there is no
    // `platform/web/libgodot_*`. Godot does not embed itself in a page, because in
    // the browser the page IS the host. There is no analog to port.
    //
    // ⚠️ Registered anyway, and ABSTRACT — matching
    // `GDREGISTER_ABSTRACT_CLASS(GodotInstance)`
    // (`core/register_core_types.cpp:290`) and `is_instantiable=false` in the API
    // dump. So `class_exists` agrees with upstream and a defensive script probing
    // for the class takes the same branch it would in Godot. The methods are
    // declared for introspection; nothing implements them, and nothing needs to:
    // a script cannot construct one in Godot either — the C API does.
    if !class_exists("GodotInstance") {
        register_class(
            ClassRegistration::new("GodotInstance")
                .parent("Object")
                .abstract_class()
                // The host loop's surface. Declared, never implemented here.
                .method(MethodInfo::new("start", 0))
                .method(MethodInfo::new("is_started", 0))
                .method(MethodInfo::new("iteration", 0))
                .method(MethodInfo::new("focus_in", 0))
                .method(MethodInfo::new("focus_out", 0))
                .method(MethodInfo::new("pause", 0))
                .method(MethodInfo::new("resume", 0)),
        );
    }

    // -- EncodedObjectAsID (pat-nzgdu.189) ---------------------------------
    //
    // What `bytes_to_var` hands back for an object that was encoded as a bare
    // instance id rather than as a full object. It carries the id and nothing
    // else, and that minimalism IS the security property: decoding an id can
    // never construct anything, which is why Godot takes this branch even when
    // full-object decoding is refused — its own comment on it reads
    // "This _is_ allowed." (`core/io/marshalls.cpp:699-715`). Only the
    // full-object branch is gated behind `p_allow_objects`.
    // Guarded like every sibling here: `register_class` does NOT dedupe — it
    // mints a fresh ClassId and overwrites the name, leaving a stale by_id
    // alias and leaking next_id on each of the ~20 calls this fn gets per suite.
    if !class_exists("EncodedObjectAsID") {
        register_class(
            ClassRegistration::new("EncodedObjectAsID")
                .parent("RefCounted")
                // Godot's documented default. NOTE the decoder can never yield
                // this value: a null id decodes to NIL, not to an object with
                // id 0 (`marshalls.cpp:707-708`).
                .property(PropertyInfo::new("object_id", Variant::Int(0)))
                .method(MethodInfo::new("set_object_id", 1))
                .method(MethodInfo::new("get_object_id", 0)),
        );
    }

    // -- UndoRedo (pat-nzgdu.155) ------------------------------------------
    // Godot's UndoRedo is a plain Object (not a Resource/RefCounted): scripts
    // and EditorPlugins construct one and record paired do/undo operations,
    // then commit_action() applies the do-half and pushes it on the history.
    //
    // Only the *property* half of the op API is registered. Godot's
    // add_do_method/add_undo_method/add_do_reference are deliberately absent:
    // the script binding (gdscript-interop) records ops in a free function that
    // has no way back into the interpreter, so it cannot invoke a Callable.
    // Registering them would make class_has_method() promise a method that
    // errors on call, which is worse than reflecting the honest surface.
    if !class_exists("UndoRedo") {
        register_class(
            ClassRegistration::new("UndoRedo")
                .parent("Object")
                .method(MethodInfo::new("create_action", 1))
                .method(MethodInfo::new("commit_action", 0))
                .method(MethodInfo::new("is_committing_action", 0))
                .method(MethodInfo::new("add_do_property", 3))
                .method(MethodInfo::new("add_undo_property", 3))
                .method(MethodInfo::new("undo", 0))
                .method(MethodInfo::new("redo", 0))
                .method(MethodInfo::new("has_undo", 0))
                .method(MethodInfo::new("has_redo", 0))
                .method(MethodInfo::new("get_version", 0))
                .method(MethodInfo::new("clear_history", 0))
                .method(MethodInfo::new("get_action_name", 1))
                .method(MethodInfo::new("get_history_count", 0))
                .method(MethodInfo::new("get_current_action", 0))
                .method(MethodInfo::new("get_current_action_name", 0))
                .signal(SignalInfo::new("version_changed"))
                .enumeration(
                    EnumInfo::new("MergeMode")
                        .constant("MERGE_DISABLE", 0)
                        .constant("MERGE_ENDS", 1)
                        .constant("MERGE_ALL", 2),
                ),
        );
    }

    // -- JavaScriptBridge web-interop singleton (pat-nzgdu.146) ------------
    // Godot's web export exposes JavaScriptBridge as an Object singleton
    // (javascript_bridge_singleton.cpp) whose eval/get_interface/create_object
    // return a JavaScriptObject (a RefCounted wrapper around a JS value).
    // Script dispatch already routes these (interpreter's singleton table,
    // pat-nzgdu.66); this registers the ClassDB surface Godot also exposes, so
    // ClassDB.class_exists("JavaScriptBridge") and class_get_method_list agree
    // with upstream.
    if !class_exists("JavaScriptBridge") {
        register_class(
            ClassRegistration::new("JavaScriptBridge")
                .parent("Object")
                .method(MethodInfo::new("eval", 1))
                .method(MethodInfo::new("get_interface", 1))
                .method(MethodInfo::new("create_callback", 1))
                .method(MethodInfo::new("create_object", 1))
                .method(MethodInfo::new("download_buffer", 2))
                .method(MethodInfo::new("is_js_buffer", 1))
                .method(MethodInfo::new("js_buffer_to_packed_byte_array", 1))
                .method(MethodInfo::new("force_fs_sync", 0))
                .method(MethodInfo::new("pwa_needs_update", 0))
                .method(MethodInfo::new("pwa_update", 0))
                .signal(SignalInfo::new("pwa_update_available")),
        );
    }
    if !class_exists("JavaScriptObject") {
        register_class(ClassRegistration::new("JavaScriptObject").parent("RefCounted"));
    }

    // -- ImageFormatLoader (pat-nzgdu.218) -----------------------------------
    //
    // The hook an add-on implements to teach `Image.load` a new file format.
    //
    // ⚠️ Genuinely ABSTRACT, unlike the texture bases: `GDREGISTER_ABSTRACT_CLASS(ImageFormatLoader)`
    // — no creation func, so `is_abstract` is true. (Contrast pat-nzgdu.215,
    // where `Texture`/`TextureLayered` are `GDREGISTER_VIRTUAL_CLASS` and so are
    // NOT abstract. The macro must be read, not guessed from the role.)
    //
    // ⚠️ It binds NO methods at all — the base is a pure marker. Everything
    // scripts touch lives on the Extension below.
    if !class_exists("ImageFormatLoader") {
        register_class(
            ClassRegistration::new("ImageFormatLoader")
                .parent("RefCounted")
                .abstract_class()
                // The `flags` in `_load_image(image, file_access, flags, scale)`.
                // A bitfield, so the values are powers of two and combine.
                .enumeration(
                    EnumInfo::new("LoaderFlags")
                        .constant("FLAG_NONE", 0)
                        .constant("FLAG_FORCE_LINEAR", 1)
                        .constant("FLAG_CONVERT_COLORS", 2),
                ),
        );
    }
    // ⚠️ FOUR methods, not two. The bead names only the `_load_image` /
    // `_get_recognized_extensions` hooks, but `add_format_loader` and
    // `remove_format_loader` are **public, non-virtual** methods — they are how a
    // loader enters and leaves the registry, so a surface with only the virtuals
    // would be overridable and unregisterable.
    if !class_exists("ImageFormatLoaderExtension") {
        register_class(
            ClassRegistration::new("ImageFormatLoaderExtension")
                .parent("ImageFormatLoader")
                // The two overridable hooks.
                .method(MethodInfo::new("_get_recognized_extensions", 0))
                // (image, file_access, flags, scale)
                .method(MethodInfo::new("_load_image", 4))
                // ...and the two public registry calls.
                .method(MethodInfo::new("add_format_loader", 0))
                .method(MethodInfo::new("remove_format_loader", 0)),
        );
    }

    // pat-nzgdu.222: ResourceFormatLoader / ResourceFormatSaver — the
    // script-subclassable base classes a plugin extends to add a custom resource
    // format. Both are GDREGISTER_CLASS (core/register_core_types.cpp:149-150) —
    // concrete and *directly* subclassable, unlike ImageFormatLoader above which
    // splits into an abstract base + an Extension. They bind ONLY virtual methods
    // (the dump reports zero non-virtual on each); a subclass overrides the ones
    // it needs.
    //
    // ⚠️ SCOPE: this registers the class surface so `extends ResourceFormatLoader`
    // resolves. The runtime *dispatch* — `ResourceLoader.add_resource_format_loader`
    // plus a `load()` that invokes a script subclass's `_load` — is a separate,
    // larger gdscript-interop feature (no ResourceLoader singleton/registry exists
    // in the interpreter yet) and is NOT wired here.
    if !class_exists("ResourceFormatLoader") {
        register_class(
            ClassRegistration::new("ResourceFormatLoader")
                .parent("RefCounted")
                .method(MethodInfo::new("_get_recognized_extensions", 0))
                .method(MethodInfo::new("_recognize_path", 2))
                .method(MethodInfo::new("_handles_type", 1))
                .method(MethodInfo::new("_get_resource_type", 1))
                .method(MethodInfo::new("_get_resource_script_class", 1))
                .method(MethodInfo::new("_get_resource_uid", 1))
                .method(MethodInfo::new("_get_dependencies", 2))
                .method(MethodInfo::new("_rename_dependencies", 2))
                .method(MethodInfo::new("_exists", 1))
                .method(MethodInfo::new("_get_classes_used", 1))
                // (path, original_path, use_sub_threads, cache_mode)
                .method(MethodInfo::new("_load", 4)),
        );
    }
    if !class_exists("ResourceFormatSaver") {
        register_class(
            ClassRegistration::new("ResourceFormatSaver")
                .parent("RefCounted")
                // (resource, path, flags)
                .method(MethodInfo::new("_save", 3))
                .method(MethodInfo::new("_set_uid", 2))
                .method(MethodInfo::new("_recognize", 1))
                .method(MethodInfo::new("_get_recognized_extensions", 1))
                .method(MethodInfo::new("_recognize_path", 2)),
        );
    }

    if !class_exists("FileAccess") {
        register_class(
            ClassRegistration::new("FileAccess")
                .parent("RefCounted")
                .method(MethodInfo::new("open", 2))
                .method(MethodInfo::new("get_as_text", 0))
                .method(MethodInfo::new("get_buffer", 1))
                .method(MethodInfo::new("store_string", 1))
                .method(MethodInfo::new("store_buffer", 1))
                .method(MethodInfo::new("get_error", 0))
                .method(MethodInfo::new("close", 0))
                .method(MethodInfo::new("file_exists", 1)),
        );
    }
    if !class_exists("DirAccess") {
        register_class(
            ClassRegistration::new("DirAccess")
                .parent("RefCounted")
                .method(MethodInfo::new("open", 1))
                .method(MethodInfo::new("list_dir_begin", 0))
                .method(MethodInfo::new("get_next", 0))
                .method(MethodInfo::new("current_is_dir", 0))
                .method(MethodInfo::new("list_dir_end", 0))
                .method(MethodInfo::new("make_dir", 1))
                .method(MethodInfo::new("remove", 1))
                .method(MethodInfo::new("rename", 2)),
        );
    }
    // -- PackedDataContainer (pat-nzgdu.220) ---------------------------------
    //
    // A nested Array/Dictionary packed into one compact byte buffer, read back
    // by random access without unpacking the whole thing.
    //
    // ⚠️ `Resource`, not `RefCounted` — unlike ConfigFile/FileAccess below. A
    // packed container is an *asset*: you save it and load it. This module
    // bootstraps `Object` and `RefCounted` but not `Resource`, and an
    // unregistered base silently severs the chain from Object (pat-nzgdu.98), so
    // guard it here rather than relying on `register_gltf_classes` happening to
    // do it first.
    if !class_exists("Resource") {
        register_class(ClassRegistration::new("Resource").parent("RefCounted"));
    }
    if !class_exists("PackedDataContainer") {
        register_class(
            ClassRegistration::new("PackedDataContainer")
                .parent("Resource")
                // ⚠️ Exactly two methods. `__data__` — the packed bytes — is a
                // real property in Godot (`ADD_PROPERTY(... PACKED_BYTE_ARRAY,
                // "__data__" ...)`, packed_data_container.cpp:366) but carries
                // `PROPERTY_USAGE_INTERNAL`, which is why `extension_api.json`
                // reports zero properties for this class. `PropertyInfo` here has
                // no `usage` field, so registering it would advertise an internal
                // member as a public one. Left off deliberately — see the notes.
                .method(MethodInfo::new("pack", 1))
                .method(MethodInfo::new("size", 0)),
        );
    }
    // ⚠️ ABSTRACT and `RefCounted` — a script never constructs one. Indexing a
    // PackedDataContainer hands one back as a *view* into the parent's bytes;
    // `GDREGISTER_ABSTRACT_CLASS(PackedDataContainerRef)` is what stops a script
    // making a view of nothing.
    if !class_exists("PackedDataContainerRef") {
        register_class(
            ClassRegistration::new("PackedDataContainerRef")
                .parent("RefCounted")
                .abstract_class()
                // Just `size` — indexing goes through the Variant `_get`
                // operator, which is not a bound method.
                .method(MethodInfo::new("size", 0)),
        );
    }

    if !class_exists("ConfigFile") {
        register_class(
            ClassRegistration::new("ConfigFile")
                .parent("RefCounted")
                .method(MethodInfo::new("set_value", 3))
                .method(MethodInfo::new("get_value", 3))
                .method(MethodInfo::new("has_section", 1))
                .method(MethodInfo::new("has_section_key", 2))
                .method(MethodInfo::new("get_sections", 0))
                .method(MethodInfo::new("get_section_keys", 1))
                .method(MethodInfo::new("load", 1))
                .method(MethodInfo::new("save", 1))
                .method(MethodInfo::new("clear", 0)),
        );
    }
    if !class_exists("ZIPReader") {
        register_class(
            ClassRegistration::new("ZIPReader")
                .parent("RefCounted")
                .method(MethodInfo::new("open", 1))
                .method(MethodInfo::new("close", 0))
                .method(MethodInfo::new("get_files", 0))
                .method(MethodInfo::new("file_exists", 2))
                .method(MethodInfo::new("read_file", 2)),
        );
    }
    if !class_exists("ZIPPacker") {
        register_class(
            ClassRegistration::new("ZIPPacker")
                .parent("RefCounted")
                // Godot's default: -1 selects the zlib default level.
                .property(PropertyInfo::new("compression_level", Variant::Int(-1)))
                .method(MethodInfo::new("open", 2))
                .method(MethodInfo::new("start_file", 1))
                .method(MethodInfo::new("write_file", 1))
                .method(MethodInfo::new("close_file", 0))
                .method(MethodInfo::new("close", 0)),
        );
    }

    register_gltf_classes();
}

/// Registers Godot's 19 script-facing `GLTF*` classes (pat-nzgdu.164).
///
/// `GLTFDocument` + `GLTFState` are the runtime glTF API; the rest are the data
/// holders a parse fills in (nodes, meshes, accessors, buffer views, skins,
/// animations, ...). Schemas are **generated** from `extension_api.json` and
/// diffed back — 135 properties across the family, which is not a surface anyone
/// should hand-type.
///
/// All 19 are `Resource` except `GLTFObjectModelProperty`, which is `RefCounted`.
///
/// **Scope:** this is the class surface only. The classes are not constructible
/// from script and nothing routes `gdresource`'s existing `gltf_import.rs`
/// parser through `GLTFDocument::append_from_buffer` yet — see the bead notes.
/// Method surfaces are therefore omitted rather than advertising ~350 methods
/// that cannot run (pat-nzgdu.155's rule).
fn register_gltf_classes() {
    // 18 of the 19 parent to Resource, which this module bootstraps Object and
    // RefCounted for but NOT Resource — and an unregistered base silently severs
    // the chain from Object (pat-nzgdu.98).
    if !class_exists("Object") {
        register_class(object_class());
    }
    if !class_exists("RefCounted") {
        register_class(ClassRegistration::new("RefCounted").parent("Object"));
    }
    if !class_exists("Resource") {
        register_class(ClassRegistration::new("Resource").parent("RefCounted"));
    }

    register_class(
        ClassRegistration::new("GLTFAccessor")
            .parent("Resource")
            .property(PropertyInfo::new("buffer_view", Variant::Int(0)))
            .property(PropertyInfo::new("byte_offset", Variant::Int(0)))
            .property(PropertyInfo::new("component_type", Variant::Int(0)))
            .property(PropertyInfo::new("normalized", Variant::Bool(false)))
            .property(PropertyInfo::new("count", Variant::Int(0)))
            .property(PropertyInfo::new("accessor_type", Variant::Int(0)))
            .property(PropertyInfo::new("type", Variant::Int(0)))
            .property(PropertyInfo::new(
                "min",
                Variant::PackedFloat64Array(Vec::new()),
            ))
            .property(PropertyInfo::new(
                "max",
                Variant::PackedFloat64Array(Vec::new()),
            ))
            .property(PropertyInfo::new("sparse_count", Variant::Int(0)))
            .property(PropertyInfo::new(
                "sparse_indices_buffer_view",
                Variant::Int(0),
            ))
            .property(PropertyInfo::new(
                "sparse_indices_byte_offset",
                Variant::Int(0),
            ))
            .property(PropertyInfo::new(
                "sparse_indices_component_type",
                Variant::Int(0),
            ))
            .property(PropertyInfo::new(
                "sparse_values_buffer_view",
                Variant::Int(0),
            ))
            .property(PropertyInfo::new(
                "sparse_values_byte_offset",
                Variant::Int(0),
            )),
    );
    register_class(
        ClassRegistration::new("GLTFAnimation")
            .parent("Resource")
            .property(PropertyInfo::new(
                "original_name",
                Variant::String(String::new()),
            ))
            .property(PropertyInfo::new("loop", Variant::Bool(false))),
    );
    register_class(
        ClassRegistration::new("GLTFBufferView")
            .parent("Resource")
            .property(PropertyInfo::new("buffer", Variant::Int(0)))
            .property(PropertyInfo::new("byte_offset", Variant::Int(0)))
            .property(PropertyInfo::new("byte_length", Variant::Int(0)))
            .property(PropertyInfo::new("byte_stride", Variant::Int(0)))
            .property(PropertyInfo::new("indices", Variant::Bool(false)))
            .property(PropertyInfo::new("vertex_attributes", Variant::Bool(false))),
    );
    register_class(
        ClassRegistration::new("GLTFCamera")
            .parent("Resource")
            .property(PropertyInfo::new("perspective", Variant::Bool(false)))
            .property(PropertyInfo::new("fov", Variant::Float(0.0)))
            .property(PropertyInfo::new("size_mag", Variant::Float(0.0)))
            .property(PropertyInfo::new("depth_far", Variant::Float(0.0)))
            .property(PropertyInfo::new("depth_near", Variant::Float(0.0))),
    );
    register_class(
        ClassRegistration::new("GLTFDocument")
            .parent("Resource")
            .property(PropertyInfo::new(
                "image_format",
                Variant::String(String::new()),
            ))
            .property(PropertyInfo::new("lossy_quality", Variant::Float(0.0)))
            .property(PropertyInfo::new(
                "fallback_image_format",
                Variant::String(String::new()),
            ))
            .property(PropertyInfo::new(
                "fallback_image_quality",
                Variant::Float(0.0),
            ))
            .property(PropertyInfo::new("root_node_mode", Variant::Int(0)))
            .property(PropertyInfo::new("visibility_mode", Variant::Int(0)))
            // The document-import API (pat-nzgdu.217). pat-nzgdu.164 registered
            // this family's properties but no methods, so `append_from_buffer`
            // did not resolve at all — and `FBXDocument` binds NOTHING of its
            // own, inheriting its entire surface from here.
            //
            // ⚠️ `append_from_buffer` takes FOUR arguments (bytes, base_path,
            // state, flags), not the `(bytes, state)` its callers assume; likewise
            // `generate_scene` takes four (state, bake_fps, trimming,
            // remove_immutable_tracks), not one.
            .method(MethodInfo::new("set_image_format", 1))
            .method(MethodInfo::new("get_image_format", 0))
            .method(MethodInfo::new("set_lossy_quality", 1))
            .method(MethodInfo::new("get_lossy_quality", 0))
            .method(MethodInfo::new("set_fallback_image_format", 1))
            .method(MethodInfo::new("get_fallback_image_format", 0))
            .method(MethodInfo::new("set_fallback_image_quality", 1))
            .method(MethodInfo::new("get_fallback_image_quality", 0))
            .method(MethodInfo::new("set_root_node_mode", 1))
            .method(MethodInfo::new("get_root_node_mode", 0))
            .method(MethodInfo::new("set_visibility_mode", 1))
            .method(MethodInfo::new("get_visibility_mode", 0))
            .method(MethodInfo::new("append_from_file", 4))
            .method(MethodInfo::new("append_from_buffer", 4))
            .method(MethodInfo::new("append_from_scene", 3))
            .method(MethodInfo::new("generate_scene", 4))
            .method(MethodInfo::new("generate_buffer", 1))
            .method(MethodInfo::new("write_to_filesystem", 2))
            .method(MethodInfo::new("import_object_model_property", 2))
            .method(MethodInfo::new("export_object_model_property", 4))
            // ⚠️ These two are STATIC in Godot; `MethodInfo` cannot say so
            // (pat-nzgdu.210).
            .method(MethodInfo::new("register_gltf_document_extension", 2))
            .method(MethodInfo::new("unregister_gltf_document_extension", 1))
            .method(MethodInfo::new("get_supported_gltf_extensions", 0)),
    );
    register_class(ClassRegistration::new("GLTFDocumentExtension").parent("Resource"));
    register_class(
        ClassRegistration::new("GLTFDocumentExtensionConvertImporterMesh")
            .parent("GLTFDocumentExtension"),
    );
    register_class(
        ClassRegistration::new("GLTFLight")
            .parent("Resource")
            .property(PropertyInfo::new(
                "color",
                Variant::Color(gdcore::math::Color::WHITE),
            ))
            .property(PropertyInfo::new("intensity", Variant::Float(0.0)))
            .property(PropertyInfo::new(
                "light_type",
                Variant::String(String::new()),
            ))
            .property(PropertyInfo::new("range", Variant::Float(0.0)))
            .property(PropertyInfo::new("inner_cone_angle", Variant::Float(0.0)))
            .property(PropertyInfo::new("outer_cone_angle", Variant::Float(0.0))),
    );
    register_class(
        ClassRegistration::new("GLTFMesh")
            .parent("Resource")
            .property(PropertyInfo::new(
                "original_name",
                Variant::String(String::new()),
            ))
            .property(PropertyInfo::new("mesh", Variant::Nil))
            .property(PropertyInfo::new(
                "blend_weights",
                Variant::PackedFloat32Array(Vec::new()),
            ))
            .property(PropertyInfo::new(
                "instance_materials",
                Variant::Array(Vec::new()),
            )),
    );
    register_class(
        ClassRegistration::new("GLTFNode")
            .parent("Resource")
            .property(PropertyInfo::new(
                "original_name",
                Variant::String(String::new()),
            ))
            .property(PropertyInfo::new("parent", Variant::Int(0)))
            .property(PropertyInfo::new("height", Variant::Int(0)))
            .property(PropertyInfo::new(
                "xform",
                Variant::Transform3D(gdcore::math3d::Transform3D::IDENTITY),
            ))
            .property(PropertyInfo::new("mesh", Variant::Int(0)))
            .property(PropertyInfo::new("camera", Variant::Int(0)))
            .property(PropertyInfo::new("skin", Variant::Int(0)))
            .property(PropertyInfo::new("skeleton", Variant::Int(0)))
            .property(PropertyInfo::new(
                "position",
                Variant::Vector3(gdcore::math::Vector3::ZERO),
            ))
            .property(PropertyInfo::new(
                "rotation",
                Variant::Quaternion(gdcore::math3d::Quaternion::IDENTITY),
            ))
            .property(PropertyInfo::new(
                "scale",
                Variant::Vector3(gdcore::math::Vector3::ZERO),
            ))
            .property(PropertyInfo::new(
                "children",
                Variant::PackedInt32Array(Vec::new()),
            ))
            .property(PropertyInfo::new("light", Variant::Int(0)))
            .property(PropertyInfo::new("visible", Variant::Bool(false))),
    );
    register_class(
        ClassRegistration::new("GLTFObjectModelProperty")
            .parent("RefCounted")
            .property(PropertyInfo::new("gltf_to_godot_expression", Variant::Nil))
            .property(PropertyInfo::new("godot_to_gltf_expression", Variant::Nil))
            .property(PropertyInfo::new("node_paths", Variant::Array(Vec::new())))
            .property(PropertyInfo::new("object_model_type", Variant::Int(0)))
            .property(PropertyInfo::new(
                "json_pointers",
                Variant::PackedStringArray(Vec::new()),
            ))
            .property(PropertyInfo::new("variant_type", Variant::Int(0))),
    );
    register_class(
        ClassRegistration::new("GLTFPhysicsBody")
            .parent("Resource")
            .property(PropertyInfo::new(
                "body_type",
                Variant::String(String::new()),
            ))
            .property(PropertyInfo::new("mass", Variant::Float(0.0)))
            .property(PropertyInfo::new(
                "linear_velocity",
                Variant::Vector3(gdcore::math::Vector3::ZERO),
            ))
            .property(PropertyInfo::new(
                "angular_velocity",
                Variant::Vector3(gdcore::math::Vector3::ZERO),
            ))
            .property(PropertyInfo::new(
                "center_of_mass",
                Variant::Vector3(gdcore::math::Vector3::ZERO),
            ))
            .property(PropertyInfo::new(
                "inertia_diagonal",
                Variant::Vector3(gdcore::math::Vector3::ZERO),
            ))
            .property(PropertyInfo::new(
                "inertia_orientation",
                Variant::Quaternion(gdcore::math3d::Quaternion::IDENTITY),
            ))
            .property(PropertyInfo::new(
                "inertia_tensor",
                Variant::Basis(gdcore::math3d::Basis::IDENTITY),
            )),
    );
    register_class(
        ClassRegistration::new("GLTFPhysicsShape")
            .parent("Resource")
            .property(PropertyInfo::new(
                "shape_type",
                Variant::String(String::new()),
            ))
            .property(PropertyInfo::new(
                "size",
                Variant::Vector3(gdcore::math::Vector3::ZERO),
            ))
            .property(PropertyInfo::new("radius", Variant::Float(0.0)))
            .property(PropertyInfo::new("height", Variant::Float(0.0)))
            .property(PropertyInfo::new("is_trigger", Variant::Bool(false)))
            .property(PropertyInfo::new("mesh_index", Variant::Int(0)))
            .property(PropertyInfo::new("importer_mesh", Variant::Nil)),
    );
    register_class(
        ClassRegistration::new("GLTFSkeleton")
            .parent("Resource")
            .property(PropertyInfo::new(
                "joints",
                Variant::PackedInt32Array(Vec::new()),
            ))
            .property(PropertyInfo::new(
                "roots",
                Variant::PackedInt32Array(Vec::new()),
            ))
            .property(PropertyInfo::new(
                "unique_names",
                Variant::Array(Vec::new()),
            ))
            .property(PropertyInfo::new(
                "godot_bone_node",
                Variant::Dictionary(gdvariant::Dictionary::new()),
            )),
    );
    register_class(
        ClassRegistration::new("GLTFSkin")
            .parent("Resource")
            .property(PropertyInfo::new("skin_root", Variant::Int(0)))
            .property(PropertyInfo::new(
                "joints_original",
                Variant::PackedInt32Array(Vec::new()),
            ))
            .property(PropertyInfo::new(
                "inverse_binds",
                Variant::Array(Vec::new()),
            ))
            .property(PropertyInfo::new(
                "joints",
                Variant::PackedInt32Array(Vec::new()),
            ))
            .property(PropertyInfo::new(
                "non_joints",
                Variant::PackedInt32Array(Vec::new()),
            ))
            .property(PropertyInfo::new(
                "roots",
                Variant::PackedInt32Array(Vec::new()),
            ))
            .property(PropertyInfo::new("skeleton", Variant::Int(0)))
            .property(PropertyInfo::new(
                "joint_i_to_bone_i",
                Variant::Dictionary(gdvariant::Dictionary::new()),
            ))
            .property(PropertyInfo::new(
                "joint_i_to_name",
                Variant::Dictionary(gdvariant::Dictionary::new()),
            ))
            .property(PropertyInfo::new("godot_skin", Variant::Nil)),
    );
    register_class(
        ClassRegistration::new("GLTFSpecGloss")
            .parent("Resource")
            .property(PropertyInfo::new("diffuse_img", Variant::Nil))
            .property(PropertyInfo::new(
                "diffuse_factor",
                Variant::Color(gdcore::math::Color::WHITE),
            ))
            .property(PropertyInfo::new("gloss_factor", Variant::Float(0.0)))
            .property(PropertyInfo::new(
                "specular_factor",
                Variant::Color(gdcore::math::Color::WHITE),
            ))
            .property(PropertyInfo::new("spec_gloss_img", Variant::Nil)),
    );
    register_class(
        ClassRegistration::new("GLTFState")
            .parent("Resource")
            .property(PropertyInfo::new(
                "json",
                Variant::Dictionary(gdvariant::Dictionary::new()),
            ))
            .property(PropertyInfo::new("major_version", Variant::Int(0)))
            .property(PropertyInfo::new("minor_version", Variant::Int(0)))
            .property(PropertyInfo::new(
                "copyright",
                Variant::String(String::new()),
            ))
            .property(PropertyInfo::new(
                "glb_data",
                Variant::PackedByteArray(Vec::new()),
            ))
            .property(PropertyInfo::new(
                "use_named_skin_binds",
                Variant::Bool(false),
            ))
            .property(PropertyInfo::new("nodes", Variant::Array(Vec::new())))
            .property(PropertyInfo::new("buffers", Variant::Array(Vec::new())))
            .property(PropertyInfo::new(
                "buffer_views",
                Variant::Array(Vec::new()),
            ))
            .property(PropertyInfo::new("accessors", Variant::Array(Vec::new())))
            .property(PropertyInfo::new("meshes", Variant::Array(Vec::new())))
            .property(PropertyInfo::new("materials", Variant::Array(Vec::new())))
            .property(PropertyInfo::new(
                "scene_name",
                Variant::String(String::new()),
            ))
            .property(PropertyInfo::new(
                "base_path",
                Variant::String(String::new()),
            ))
            .property(PropertyInfo::new(
                "filename",
                Variant::String(String::new()),
            ))
            .property(PropertyInfo::new(
                "root_nodes",
                Variant::PackedInt32Array(Vec::new()),
            ))
            .property(PropertyInfo::new("textures", Variant::Array(Vec::new())))
            .property(PropertyInfo::new(
                "texture_samplers",
                Variant::Array(Vec::new()),
            ))
            .property(PropertyInfo::new("images", Variant::Array(Vec::new())))
            .property(PropertyInfo::new("skins", Variant::Array(Vec::new())))
            .property(PropertyInfo::new("cameras", Variant::Array(Vec::new())))
            .property(PropertyInfo::new("lights", Variant::Array(Vec::new())))
            .property(PropertyInfo::new(
                "unique_names",
                Variant::Array(Vec::new()),
            ))
            .property(PropertyInfo::new(
                "unique_animation_names",
                Variant::Array(Vec::new()),
            ))
            .property(PropertyInfo::new("skeletons", Variant::Array(Vec::new())))
            .property(PropertyInfo::new("create_animations", Variant::Bool(false)))
            .property(PropertyInfo::new(
                "import_as_skeleton_bones",
                Variant::Bool(false),
            ))
            .property(PropertyInfo::new("animations", Variant::Array(Vec::new())))
            .property(PropertyInfo::new(
                "handle_binary_image_mode",
                Variant::Int(0),
            ))
            .property(PropertyInfo::new("bake_fps", Variant::Float(0.0)))
            .property(PropertyInfo::new("handle_binary_image", Variant::Int(0))),
    );
    register_class(
        ClassRegistration::new("GLTFTexture")
            .parent("Resource")
            .property(PropertyInfo::new("src_image", Variant::Int(0)))
            .property(PropertyInfo::new("sampler", Variant::Int(0))),
    );
    register_class(
        ClassRegistration::new("GLTFTextureSampler")
            .parent("Resource")
            .property(PropertyInfo::new("mag_filter", Variant::Int(0)))
            .property(PropertyInfo::new("min_filter", Variant::Int(0)))
            .property(PropertyInfo::new("wrap_s", Variant::Int(0)))
            .property(PropertyInfo::new("wrap_t", Variant::Int(0))),
    );

    // -- FBX (pat-nzgdu.217) -------------------------------------------------
    //
    // Godot imports FBX by *reusing the glTF model wholesale*: `FBXDocument` and
    // `FBXState` are subclasses of the glTF ones, and an imported FBX lands in
    // the same node/mesh/material/animation structures. That is why these two
    // live here rather than in a family of their own.
    //
    // ⚠️ `FBXDocument` binds NOTHING — zero methods, zero properties. Its entire
    // surface, `append_from_buffer` included, is `GLTFDocument`'s. It exists to
    // *name* the format, and adding methods to it would invent a ClassDB surface
    // Godot does not have.
    if !class_exists("FBXDocument") {
        register_class(ClassRegistration::new("FBXDocument").parent("GLTFDocument"));
    }
    // ⚠️ FBXState adds exactly ONE member over GLTFState — the geometry-helper
    // toggle, which is an FBX-specific concept (transform/pivot helper nodes
    // glTF has no equivalent for). Default `false`: Godot collapses those helpers
    // into transforms unless asked to keep them, so defaulting to `true` would
    // silently produce a deeper node tree than Godot.
    if !class_exists("FBXState") {
        register_class(
            ClassRegistration::new("FBXState")
                .parent("GLTFState")
                .property(PropertyInfo::new(
                    "allow_geometry_helper_nodes",
                    Variant::Bool(false),
                ))
                .method(MethodInfo::new("get_allow_geometry_helper_nodes", 0))
                .method(MethodInfo::new("set_allow_geometry_helper_nodes", 1)),
        );
    }
}

/// * `HTTPRequest : Node` (scene/main/http_request)
/// * `HTTPClient : RefCounted` (core/io/http_client)
/// * `WebSocketPeer : PacketPeer : RefCounted` (modules/websocket)
///
/// Idempotent — each class is only registered if not already present.
pub fn register_networking_classes() {
    // Ensure the base chain exists (Object → RefCounted → PacketPeer, and Node).
    if !class_exists("Object") {
        register_class(object_class());
    }
    if !class_exists("RefCounted") {
        register_class(ClassRegistration::new("RefCounted").parent("Object"));
    }
    if !class_exists("Node") {
        register_class(node_class());
    }
    if !class_exists("PacketPeer") {
        register_class(
            ClassRegistration::new("PacketPeer")
                .parent("RefCounted")
                .abstract_class(),
        );
    }

    // -- HTTPClient : RefCounted --
    if !class_exists("HTTPClient") {
        register_class(
            ClassRegistration::new("HTTPClient")
                .parent("RefCounted")
                .method(MethodInfo::new("connect_to_host", 3))
                .method(MethodInfo::new("request", 4))
                .method(MethodInfo::new("poll", 0))
                .method(MethodInfo::new("get_status", 0))
                .method(MethodInfo::new("has_response", 0))
                .method(MethodInfo::new("get_response_code", 0))
                .method(MethodInfo::new("get_response_headers", 0))
                .method(MethodInfo::new("get_response_body_length", 0))
                .method(MethodInfo::new("read_response_body_chunk", 0))
                .method(MethodInfo::new("close", 0)),
        );
    }

    // -- HTTPRequest : Node --
    if !class_exists("HTTPRequest") {
        register_class(
            ClassRegistration::new("HTTPRequest")
                .parent("Node")
                .property(PropertyInfo::new("timeout", Variant::Float(0.0)))
                .property(PropertyInfo::new("max_redirects", Variant::Int(8)))
                .property(PropertyInfo::new("use_threads", Variant::Bool(false)))
                .property(PropertyInfo::new("accept_gzip", Variant::Bool(true)))
                .property(PropertyInfo::new("body_size_limit", Variant::Int(-1)))
                .property(PropertyInfo::new(
                    "download_file",
                    Variant::String(String::new()),
                ))
                .method(MethodInfo::new("request", 4))
                .method(MethodInfo::new("request_raw", 4))
                .method(MethodInfo::new("cancel_request", 0))
                .method(MethodInfo::new("get_http_client_status", 0))
                .method(MethodInfo::new("set_timeout", 1))
                .method(MethodInfo::new("get_timeout", 0))
                .method(MethodInfo::new("set_max_redirects", 1))
                .method(MethodInfo::new("get_max_redirects", 0)),
        );
    }

    // -- WebSocketPeer : PacketPeer --
    if !class_exists("WebSocketPeer") {
        register_class(
            ClassRegistration::new("WebSocketPeer")
                .parent("PacketPeer")
                .method(MethodInfo::new("connect_to_url", 2))
                .method(MethodInfo::new("accept_stream", 1))
                .method(MethodInfo::new("send", 2))
                .method(MethodInfo::new("send_text", 1))
                .method(MethodInfo::new("get_available_packet_count", 0))
                .method(MethodInfo::new("get_packet", 0))
                .method(MethodInfo::new("was_string_packet", 0))
                .method(MethodInfo::new("poll", 0))
                .method(MethodInfo::new("close", 2))
                .method(MethodInfo::new("get_ready_state", 0))
                .method(MethodInfo::new("get_close_code", 0))
                .method(MethodInfo::new("get_close_reason", 0)),
        );
    }

    // -- Multiplayer peer surface (pat-fg8dk) --
    if !class_exists("MultiplayerPeer") {
        register_class(
            ClassRegistration::new("MultiplayerPeer")
                .parent("PacketPeer")
                .abstract_class()
                .method(MethodInfo::new("get_connection_status", 0))
                .method(MethodInfo::new("get_unique_id", 0))
                .method(MethodInfo::new("poll", 0))
                .method(MethodInfo::new("close", 0)),
        );
    }
    if !class_exists("TcpMultiplayerPeer") {
        register_class(
            ClassRegistration::new("TcpMultiplayerPeer")
                .parent("MultiplayerPeer")
                .method(MethodInfo::new("create_server", 1))
                .method(MethodInfo::new("create_client", 1))
                .method(MethodInfo::new("poll", 0))
                .method(MethodInfo::new("close", 0)),
        );
    }
    // pat-p0s82.29: WebRTCMultiplayerPeer is a high-use MultiplayerPeer;
    // register the class + core methods for ClassDB parity (native WebRTC
    // transport is a separate bead, pat-p0s82.22).
    if !class_exists("WebRTCMultiplayerPeer") {
        register_class(
            ClassRegistration::new("WebRTCMultiplayerPeer")
                .parent("MultiplayerPeer")
                .method(MethodInfo::new("create_server", 0))
                .method(MethodInfo::new("create_client", 1))
                .method(MethodInfo::new("add_peer", 3))
                .method(MethodInfo::new("poll", 0))
                .method(MethodInfo::new("close", 0)),
        );
    }
    // ENetMultiplayerPeer (pat-z26jd): ClassDB-compatible alias that delegates
    // to TcpMultiplayerPeer. Channel + transfer-mode metadata are preserved in
    // packet headers; transport is TCP-reliable (not native ENet UDP).
    if !class_exists("ENetMultiplayerPeer") {
        register_class(
            ClassRegistration::new("ENetMultiplayerPeer")
                .parent("MultiplayerPeer")
                .method(MethodInfo::new("create_server", 1))
                .method(MethodInfo::new("create_client", 1))
                .method(MethodInfo::new("poll", 0))
                .method(MethodInfo::new("close", 0)),
        );
    }
    // pat-0q2kx.94: `MultiplayerAPI` had exactly two methods and no signals, so
    // `multiplayer.peer_connected.connect(...)` — the single most common
    // multiplayer line in any Godot project — had nothing to bind to.
    if !class_exists("MultiplayerAPI") {
        register_class(
            ClassRegistration::new("MultiplayerAPI")
                .parent("RefCounted")
                .method(MethodInfo::new("set_multiplayer_peer", 1))
                .method(MethodInfo::new("is_server", 0))
                .method(MethodInfo::new("get_unique_id", 0))
                .method(MethodInfo::new("get_peers", 0))
                .method(MethodInfo::new("get_remote_sender_id", 0))
                .method(MethodInfo::new("has_multiplayer_peer", 0))
                .method(MethodInfo::new("poll", 0))
                .method(MethodInfo::new("rpc", 4))
                .method(MethodInfo::new("object_configuration_add", 2))
                .method(MethodInfo::new("object_configuration_remove", 2))
                .signal(SignalInfo::new("peer_connected").arg("id"))
                .signal(SignalInfo::new("peer_disconnected").arg("id"))
                .signal(SignalInfo::new("connected_to_server"))
                .signal(SignalInfo::new("connection_failed"))
                .signal(SignalInfo::new("server_disconnected")),
        );
    }
    // `SceneMultiplayer` is the default MultiplayerAPI in Godot and adds the
    // authentication handshake, raw `send_bytes`/`peer_packet`, and relay
    // controls on top. It was absent entirely (grep = 0 hits).
    if !class_exists("SceneMultiplayer") {
        register_class(
            ClassRegistration::new("SceneMultiplayer")
                .parent("MultiplayerAPI")
                .property(PropertyInfo::new(
                    "refuse_new_connections",
                    Variant::Bool(false),
                ))
                // ⚠️ Godot's defaults: relay ON, decoding OFF (decoding objects
                // off the wire is an RCE risk, which is why it is opt-in), and a
                // 3-second auth timeout.
                .property(PropertyInfo::new("server_relay", Variant::Bool(true)))
                .property(PropertyInfo::new(
                    "allow_object_decoding",
                    Variant::Bool(false),
                ))
                .property(PropertyInfo::new("auth_timeout", Variant::Float(3.0)))
                .method(MethodInfo::new("send_bytes", 4))
                .method(MethodInfo::new("set_auth_callback", 1))
                .method(MethodInfo::new("get_auth_callback", 0))
                .method(MethodInfo::new("send_auth", 2))
                .method(MethodInfo::new("complete_auth", 1))
                .method(MethodInfo::new("get_authenticating_peers", 0))
                .method(MethodInfo::new("disconnect_peer", 1))
                .method(MethodInfo::new("clear", 0))
                .signal(SignalInfo::new("peer_packet").arg("id").arg("packet"))
                .signal(SignalInfo::new("peer_authenticating").arg("id"))
                .signal(SignalInfo::new("peer_authentication_failed").arg("id")),
        );
    }
    // The always-server loopback peer a single-player build runs with, so the
    // same `multiplayer.is_server()` branches work with no sockets.
    if !class_exists("OfflineMultiplayerPeer") {
        register_class(
            ClassRegistration::new("OfflineMultiplayerPeer")
                .parent("MultiplayerPeer")
                .method(MethodInfo::new("get_connection_status", 0))
                .method(MethodInfo::new("get_unique_id", 0))
                .method(MethodInfo::new("poll", 0))
                .method(MethodInfo::new("close", 0)),
        );
    }

    // -- Extension bases (pat-nzgdu.199) ------------------------------------
    //
    // The subclass-me bases: you override the virtuals to implement a custom
    // transport (Peer) or an entirely custom multiplayer stack (API).
    //
    // ⚠️ Both are PURE VIRTUAL SURFACES — zero non-virtual methods between them.
    // Every entry below is a hook the subclass provides, not an API the engine
    // offers. That inverts the usual reading of a registration: nothing here is
    // callable *on* the class; it is all callable *by* the engine.
    //
    // ⚠️ And note the direction of the hierarchy: the Extension is the CHILD of
    // the abstract base it extends, not a sibling — so a subclass of
    // MultiplayerPeerExtension IS-A MultiplayerPeer and can be handed straight
    // to `multiplayer_peer`.
    if !class_exists("MultiplayerPeerExtension") {
        register_class(
            ClassRegistration::new("MultiplayerPeerExtension")
                .parent("MultiplayerPeer")
                // The transport itself.
                .method(MethodInfo::new("_get_packet", 2))
                .method(MethodInfo::new("_put_packet", 2))
                .method(MethodInfo::new("_get_available_packet_count", 0))
                .method(MethodInfo::new("_get_max_packet_size", 0))
                // The script-facing variants Godot calls when the override comes
                // from a script rather than GDExtension — note they exist
                // ALONGSIDE the raw pointer ones above, not instead of them.
                .method(MethodInfo::new("_get_packet_script", 0))
                .method(MethodInfo::new("_put_packet_script", 1))
                .method(MethodInfo::new("_get_packet_channel", 0))
                .method(MethodInfo::new("_get_packet_mode", 0))
                .method(MethodInfo::new("_set_transfer_channel", 1))
                .method(MethodInfo::new("_get_transfer_channel", 0))
                .method(MethodInfo::new("_set_transfer_mode", 1))
                .method(MethodInfo::new("_get_transfer_mode", 0))
                .method(MethodInfo::new("_set_target_peer", 1))
                .method(MethodInfo::new("_get_packet_peer", 0))
                .method(MethodInfo::new("_is_server", 0))
                // The pump the SceneTree drives each frame.
                .method(MethodInfo::new("_poll", 0))
                .method(MethodInfo::new("_close", 0))
                .method(MethodInfo::new("_disconnect_peer", 2))
                .method(MethodInfo::new("_get_unique_id", 0))
                .method(MethodInfo::new("_set_refuse_new_connections", 1))
                .method(MethodInfo::new("_is_refusing_new_connections", 0))
                .method(MethodInfo::new("_is_server_relay_supported", 0))
                .method(MethodInfo::new("_get_connection_status", 0)),
        );
    }
    if !class_exists("MultiplayerAPIExtension") {
        register_class(
            ClassRegistration::new("MultiplayerAPIExtension")
                .parent("MultiplayerAPI")
                .method(MethodInfo::new("_poll", 0))
                .method(MethodInfo::new("_set_multiplayer_peer", 1))
                .method(MethodInfo::new("_get_multiplayer_peer", 0))
                .method(MethodInfo::new("_get_unique_id", 0))
                .method(MethodInfo::new("_get_peer_ids", 0))
                // (method, peer_id, object, args) — the whole RPC path a custom
                // stack has to reimplement.
                .method(MethodInfo::new("_rpc", 4))
                .method(MethodInfo::new("_get_remote_sender_id", 0))
                // How MultiplayerSpawner/Synchronizer register themselves with a
                // custom API.
                .method(MethodInfo::new("_object_configuration_add", 2))
                .method(MethodInfo::new("_object_configuration_remove", 2)),
        );
    }

    // -- MultiplayerSpawner / MultiplayerSynchronizer (pat-ikq04) --
    // Upstream: modules/multiplayer/multiplayer_spawner.* / multiplayer_synchronizer.*
    if !class_exists("MultiplayerSpawner") {
        register_class(
            ClassRegistration::new("MultiplayerSpawner")
                .parent("Node")
                .property(PropertyInfo::new(
                    "spawn_path",
                    Variant::String(String::new()),
                ))
                .property(PropertyInfo::new("spawn_limit", Variant::Int(0)))
                .method(MethodInfo::new("add_spawnable_scene", 1))
                .method(MethodInfo::new("get_spawnable_scene_count", 0))
                .method(MethodInfo::new("get_spawnable_scene", 1))
                .method(MethodInfo::new("clear_spawnable_scenes", 0))
                .method(MethodInfo::new("spawn", 1))
                .method(MethodInfo::new("get_spawn_path", 0))
                .method(MethodInfo::new("set_spawn_path", 1))
                .method(MethodInfo::new("get_spawn_limit", 0))
                .method(MethodInfo::new("set_spawn_limit", 1)),
        );
    }
    if !class_exists("MultiplayerSynchronizer") {
        register_class(
            ClassRegistration::new("MultiplayerSynchronizer")
                .parent("Node")
                .property(PropertyInfo::new(
                    "root_path",
                    Variant::String(String::from("..")),
                ))
                .property(PropertyInfo::new(
                    "replication_interval",
                    Variant::Float(0.0),
                ))
                .method(MethodInfo::new("get_root_path", 0))
                .method(MethodInfo::new("set_root_path", 1))
                .method(MethodInfo::new("get_replication_interval", 0))
                .method(MethodInfo::new("set_replication_interval", 1))
                .method(MethodInfo::new("get_net_id", 0))
                .method(MethodInfo::new("set_net_id", 1)),
        );
    }

    // -- PacketPeerUDP / StreamPeerTCP (pat-72jn7) --
    if !class_exists("PacketPeerUDP") {
        register_class(
            ClassRegistration::new("PacketPeerUDP")
                .parent("PacketPeer")
                .method(MethodInfo::new("bind", 2))
                .method(MethodInfo::new("set_dest_address", 2))
                .method(MethodInfo::new("put_packet", 1))
                .method(MethodInfo::new("get_packet", 0))
                .method(MethodInfo::new("get_available_packet_count", 0))
                .method(MethodInfo::new("close", 0)),
        );
    }
    if !class_exists("StreamPeer") {
        register_class(
            ClassRegistration::new("StreamPeer")
                .parent("RefCounted")
                .abstract_class()
                .method(MethodInfo::new("put_data", 1))
                .method(MethodInfo::new("get_data", 1))
                .method(MethodInfo::new("get_available_bytes", 0)),
        );
    }
    if !class_exists("StreamPeerTCP") {
        register_class(
            ClassRegistration::new("StreamPeerTCP")
                .parent("StreamPeer")
                .method(MethodInfo::new("connect_to_host", 2))
                .method(MethodInfo::new("get_status", 0))
                .method(MethodInfo::new("poll", 0))
                .method(MethodInfo::new("put_data", 1))
                .method(MethodInfo::new("get_data", 1))
                .method(MethodInfo::new("get_available_bytes", 0))
                .method(MethodInfo::new("disconnect_from_host", 0)),
        );
    }
}

/// Registers all core, 2D, and 3D classes for the editor.
///
/// Convenience function that ensures the full class hierarchy is available.
/// Safe to call multiple times — each class is only registered if not already present.
/// Registers Godot's script-callable pathfinding graph classes: `AStar2D`,
/// `AStar3D`, and `AStarGrid2D` (all `RefCounted`). The runtime implementations
/// live in `gdscene::astar`; this records their ClassDB metadata so scripts and
/// the editor can discover the types and their methods.
pub fn register_navigation_classes() {
    if !class_exists("Object") {
        register_class(object_class());
    }
    if !class_exists("RefCounted") {
        register_class(ClassRegistration::new("RefCounted").parent("Object"));
    }

    // AStar2D and AStar3D share the same graph API (differing only in the
    // dimensionality of point positions), so register both with one method set.
    for name in ["AStar2D", "AStar3D"] {
        if !class_exists(name) {
            register_class(
                ClassRegistration::new(name)
                    .parent("RefCounted")
                    .method(MethodInfo::new("add_point", 3))
                    .method(MethodInfo::new("has_point", 1))
                    .method(MethodInfo::new("remove_point", 1))
                    .method(MethodInfo::new("get_point_position", 1))
                    .method(MethodInfo::new("set_point_position", 2))
                    .method(MethodInfo::new("get_point_weight_scale", 1))
                    .method(MethodInfo::new("set_point_weight_scale", 2))
                    .method(MethodInfo::new("set_point_disabled", 2))
                    .method(MethodInfo::new("is_point_disabled", 1))
                    .method(MethodInfo::new("get_point_count", 0))
                    .method(MethodInfo::new("get_point_ids", 0))
                    .method(MethodInfo::new("connect_points", 3))
                    .method(MethodInfo::new("disconnect_points", 3))
                    .method(MethodInfo::new("are_points_connected", 3))
                    .method(MethodInfo::new("get_point_connections", 1))
                    .method(MethodInfo::new("get_id_path", 3))
                    .method(MethodInfo::new("get_point_path", 3))
                    .method(MethodInfo::new("get_closest_point", 2))
                    .method(MethodInfo::new("clear", 0)),
            );
        }
    }

    if !class_exists("AStarGrid2D") {
        register_class(
            ClassRegistration::new("AStarGrid2D")
                .parent("RefCounted")
                .method(MethodInfo::new("set_region", 1))
                .method(MethodInfo::new("get_region", 0))
                .method(MethodInfo::new("set_cell_size", 1))
                .method(MethodInfo::new("set_offset", 1))
                .method(MethodInfo::new("update", 0))
                .method(MethodInfo::new("is_in_bounds", 2))
                .method(MethodInfo::new("is_in_boundsv", 1))
                .method(MethodInfo::new("set_diagonal_mode", 1))
                .method(MethodInfo::new("get_diagonal_mode", 0))
                .method(MethodInfo::new("set_default_compute_heuristic", 1))
                .method(MethodInfo::new("set_default_estimate_heuristic", 1))
                .method(MethodInfo::new("set_jumping_enabled", 1))
                .method(MethodInfo::new("is_jumping_enabled", 0))
                .method(MethodInfo::new("set_point_solid", 2))
                .method(MethodInfo::new("is_point_solid", 1))
                .method(MethodInfo::new("set_point_weight_scale", 2))
                .method(MethodInfo::new("get_point_weight_scale", 1))
                .method(MethodInfo::new("get_point_position", 1))
                .method(MethodInfo::new("get_id_path", 3))
                .method(MethodInfo::new("get_point_path", 3)),
        );
    }

    register_navigation_resource_classes();
}

/// Registers the navigation *resources* — the data a region is baked from and
/// pathfound over (pat-nzgdu.114).
///
/// Schemas are generated from Godot's `extension_api.json`, not recalled. Two
/// asymmetries between the 2D and 3D pair are easy to get wrong and are real:
/// `NavigationMesh`'s parse properties are `geometry_`-prefixed
/// (`geometry_parsed_geometry_type`, ...) while `NavigationPolygon`'s are not
/// (`parsed_geometry_type`, ...); and although both expose a
/// `sample_partition_type`, their `SamplePartitionType` enums are *different
/// value sets* — 3D partitions a voxel field (WATERSHED/MONOTONE/LAYERS) while
/// 2D partitions an outline (CONVEX_PARTITION/TRIANGULATE).
///
/// This is the schema only. Baking, source-geometry parsing, and `.tscn`
/// round-trip are follow-up; see the bead notes.
fn register_navigation_resource_classes() {
    // These all parent to Resource, which `register_navigation_classes` does not
    // bootstrap (it does Object and RefCounted) — and an unregistered base
    // silently severs the chain from Object (pat-nzgdu.98).
    if !class_exists("Object") {
        register_class(object_class());
    }
    // `Resource` hangs off `RefCounted`, so re-guard that too rather than
    // relying on the caller having done it (pat-nzgdu.98).
    if !class_exists("RefCounted") {
        register_class(ClassRegistration::new("RefCounted").parent("Object"));
    }
    if !class_exists("Resource") {
        register_class(ClassRegistration::new("Resource").parent("RefCounted"));
    }

    if !class_exists("NavigationPolygon") {
        register_class(
            ClassRegistration::new("NavigationPolygon")
                .parent("Resource")
                .property(PropertyInfo::new(
                    "vertices",
                    Variant::PackedVector2Array(Vec::new()),
                ))
                .property(PropertyInfo::new("polygons", Variant::Array(Vec::new())))
                .property(PropertyInfo::new("outlines", Variant::Array(Vec::new())))
                .property(PropertyInfo::new("sample_partition_type", Variant::Int(0)))
                .property(PropertyInfo::new("parsed_geometry_type", Variant::Int(0)))
                .property(PropertyInfo::new("parsed_collision_mask", Variant::Int(0)))
                .property(PropertyInfo::new("source_geometry_mode", Variant::Int(0)))
                .property(PropertyInfo::new(
                    "source_geometry_group_name",
                    Variant::String(String::new()),
                ))
                .property(PropertyInfo::new("cell_size", Variant::Float(0.0)))
                .property(PropertyInfo::new("border_size", Variant::Float(0.0)))
                .property(PropertyInfo::new("agent_radius", Variant::Float(0.0)))
                .property(PropertyInfo::new(
                    "baking_rect",
                    Variant::Rect2(gdcore::math::Rect2::new(
                        gdcore::math::Vector2::ZERO,
                        gdcore::math::Vector2::ZERO,
                    )),
                ))
                .property(PropertyInfo::new(
                    "baking_rect_offset",
                    Variant::Vector2(gdcore::math::Vector2::ZERO),
                ))
                .enumeration(
                    EnumInfo::new("SamplePartitionType")
                        .constant("SAMPLE_PARTITION_CONVEX_PARTITION", 0)
                        .constant("SAMPLE_PARTITION_TRIANGULATE", 1)
                        .constant("SAMPLE_PARTITION_MAX", 2),
                )
                .enumeration(
                    EnumInfo::new("ParsedGeometryType")
                        .constant("PARSED_GEOMETRY_MESH_INSTANCES", 0)
                        .constant("PARSED_GEOMETRY_STATIC_COLLIDERS", 1)
                        .constant("PARSED_GEOMETRY_BOTH", 2)
                        .constant("PARSED_GEOMETRY_MAX", 3),
                )
                .enumeration(
                    EnumInfo::new("SourceGeometryMode")
                        .constant("SOURCE_GEOMETRY_ROOT_NODE_CHILDREN", 0)
                        .constant("SOURCE_GEOMETRY_GROUPS_WITH_CHILDREN", 1)
                        .constant("SOURCE_GEOMETRY_GROUPS_EXPLICIT", 2)
                        .constant("SOURCE_GEOMETRY_MAX", 3),
                ),
        );
    }
    if !class_exists("NavigationMesh") {
        register_class(
            ClassRegistration::new("NavigationMesh")
                .parent("Resource")
                .property(PropertyInfo::new(
                    "vertices",
                    Variant::PackedVector3Array(Vec::new()),
                ))
                .property(PropertyInfo::new("polygons", Variant::Array(Vec::new())))
                .property(PropertyInfo::new("sample_partition_type", Variant::Int(0)))
                .property(PropertyInfo::new(
                    "geometry_parsed_geometry_type",
                    Variant::Int(0),
                ))
                .property(PropertyInfo::new(
                    "geometry_collision_mask",
                    Variant::Int(0),
                ))
                .property(PropertyInfo::new(
                    "geometry_source_geometry_mode",
                    Variant::Int(0),
                ))
                .property(PropertyInfo::new(
                    "geometry_source_group_name",
                    Variant::String(String::new()),
                ))
                .property(PropertyInfo::new("cell_size", Variant::Float(0.0)))
                .property(PropertyInfo::new("cell_height", Variant::Float(0.0)))
                .property(PropertyInfo::new("border_size", Variant::Float(0.0)))
                .property(PropertyInfo::new("agent_height", Variant::Float(0.0)))
                .property(PropertyInfo::new("agent_radius", Variant::Float(0.0)))
                .property(PropertyInfo::new("agent_max_climb", Variant::Float(0.0)))
                .property(PropertyInfo::new("agent_max_slope", Variant::Float(0.0)))
                .property(PropertyInfo::new("region_min_size", Variant::Float(0.0)))
                .property(PropertyInfo::new("region_merge_size", Variant::Float(0.0)))
                .property(PropertyInfo::new("edge_max_length", Variant::Float(0.0)))
                .property(PropertyInfo::new("edge_max_error", Variant::Float(0.0)))
                .property(PropertyInfo::new(
                    "vertices_per_polygon",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new(
                    "detail_sample_distance",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new(
                    "detail_sample_max_error",
                    Variant::Float(0.0),
                ))
                .property(PropertyInfo::new(
                    "filter_low_hanging_obstacles",
                    Variant::Bool(false),
                ))
                .property(PropertyInfo::new(
                    "filter_ledge_spans",
                    Variant::Bool(false),
                ))
                .property(PropertyInfo::new(
                    "filter_walkable_low_height_spans",
                    Variant::Bool(false),
                ))
                .property(PropertyInfo::new(
                    "filter_baking_aabb",
                    Variant::Aabb(gdcore::math3d::Aabb::new(
                        gdcore::math::Vector3::ZERO,
                        gdcore::math::Vector3::ZERO,
                    )),
                ))
                .property(PropertyInfo::new(
                    "filter_baking_aabb_offset",
                    Variant::Vector3(gdcore::math::Vector3::ZERO),
                ))
                .enumeration(
                    EnumInfo::new("SamplePartitionType")
                        .constant("SAMPLE_PARTITION_WATERSHED", 0)
                        .constant("SAMPLE_PARTITION_MONOTONE", 1)
                        .constant("SAMPLE_PARTITION_LAYERS", 2)
                        .constant("SAMPLE_PARTITION_MAX", 3),
                )
                .enumeration(
                    EnumInfo::new("ParsedGeometryType")
                        .constant("PARSED_GEOMETRY_MESH_INSTANCES", 0)
                        .constant("PARSED_GEOMETRY_STATIC_COLLIDERS", 1)
                        .constant("PARSED_GEOMETRY_BOTH", 2)
                        .constant("PARSED_GEOMETRY_MAX", 3),
                )
                .enumeration(
                    EnumInfo::new("SourceGeometryMode")
                        .constant("SOURCE_GEOMETRY_ROOT_NODE_CHILDREN", 0)
                        .constant("SOURCE_GEOMETRY_GROUPS_WITH_CHILDREN", 1)
                        .constant("SOURCE_GEOMETRY_GROUPS_EXPLICIT", 2)
                        .constant("SOURCE_GEOMETRY_MAX", 3),
                ),
        );
    }
    if !class_exists("NavigationMeshSourceGeometryData2D") {
        register_class(
            ClassRegistration::new("NavigationMeshSourceGeometryData2D")
                .parent("Resource")
                .property(PropertyInfo::new(
                    "traversable_outlines",
                    Variant::Array(Vec::new()),
                ))
                .property(PropertyInfo::new(
                    "obstruction_outlines",
                    Variant::Array(Vec::new()),
                ))
                .property(PropertyInfo::new(
                    "projected_obstructions",
                    Variant::Array(Vec::new()),
                )),
        );
    }
    if !class_exists("NavigationMeshSourceGeometryData3D") {
        register_class(
            ClassRegistration::new("NavigationMeshSourceGeometryData3D")
                .parent("Resource")
                .property(PropertyInfo::new(
                    "vertices",
                    Variant::PackedVector3Array(Vec::new()),
                ))
                .property(PropertyInfo::new(
                    "indices",
                    Variant::PackedInt32Array(Vec::new()),
                ))
                .property(PropertyInfo::new(
                    "projected_obstructions",
                    Variant::Array(Vec::new()),
                )),
        );
    }
}

pub fn register_editor_classes() {
    register_2d_classes();
    register_3d_classes();
    register_audio_classes();
    register_io_classes();
    register_networking_classes();
    register_navigation_classes();

    // -- SceneState (pat-nzgdu.167): PackedScene.get_state() introspection --
    if !class_exists("SceneState") {
        register_class(ClassRegistration::new("SceneState").parent("RefCounted"));
    }

    // -- Separators (pat-nzgdu.197) -----------------------------------------
    //
    // A separator is a line in a container. All three declare NOTHING of their
    // own: the base is abstract, and the two concrete ones differ ONLY in
    // orientation — which is the class, not a property. Giving them a
    // `vertical` bool would invent schema (contrast ScrollBar, where Godot does
    // split H/V by class for the same reason).
    if !class_exists("Separator") {
        register_class(
            ClassRegistration::new("Separator")
                .parent("Control")
                .abstract_class(),
        );
    }
    for sep in ["HSeparator", "VSeparator"] {
        if !class_exists(sep) {
            register_class(ClassRegistration::new(sep).parent("Separator"));
        }
    }

    // -- Script editor (pat-nzgdu.195) --------------------------------------
    //
    // PanelContainer was missing, which is what kept ScriptEditor out: a child
    // whose parent is unregistered has a silently severed chain (pat-nzgdu.98).
    // It declares nothing of its own in Godot — a Container that paints a panel.
    if !class_exists("PanelContainer") {
        register_class(ClassRegistration::new("PanelContainer").parent("Container"));
    }
    // ⚠️ ABSTRACT (is_instantiable=false), which the bead does not mention. A
    // script never constructs the script editor — it gets the editor's one via
    // EditorInterface.get_script_editor().
    if !class_exists("ScriptEditor") {
        register_class(
            ClassRegistration::new("ScriptEditor")
                .parent("PanelContainer")
                .abstract_class()
                // What a plugin drives it with.
                .method(MethodInfo::new("get_current_editor", 0))
                .method(MethodInfo::new("get_open_script_editors", 0))
                .method(MethodInfo::new("get_current_script", 0))
                .method(MethodInfo::new("get_open_scripts", 0))
                .method(MethodInfo::new("get_breakpoints", 0))
                .method(MethodInfo::new("goto_line", 1))
                // (base_path, base_name) — 2 required.
                .method(MethodInfo::new("open_script_create_dialog", 2))
                // A plugin can add its own highlighter to every open script —
                // this is the hook pat-nzgdu.190's SyntaxHighlighter family
                // plugs into.
                .method(MethodInfo::new("register_syntax_highlighter", 1))
                .method(MethodInfo::new("unregister_syntax_highlighter", 1))
                .method(MethodInfo::new("goto_help", 1))
                .method(MethodInfo::new("update_docs_from_script", 1))
                .method(MethodInfo::new("clear_docs_from_script", 1))
                .signal(SignalInfo::new("editor_script_changed"))
                .signal(SignalInfo::new("script_close")),
        );
    }
    // ⚠️ Also ABSTRACT. One open script's editor pane — a plugin implements this
    // to provide an editor for its own language; it does not instantiate one.
    //
    // Note the shape: 2 methods but TEN signals. It is almost entirely a
    // notifier — the editor listens to a pane, not the other way round.
    if !class_exists("ScriptEditorBase") {
        register_class(
            ClassRegistration::new("ScriptEditorBase")
                .parent("VBoxContainer")
                .abstract_class()
                .method(MethodInfo::new("get_base_editor", 0))
                .method(MethodInfo::new("add_syntax_highlighter", 1))
                .signal(SignalInfo::new("name_changed"))
                .signal(SignalInfo::new("edited_script_changed"))
                .signal(SignalInfo::new("request_help"))
                .signal(SignalInfo::new("request_open_script_at_line"))
                .signal(SignalInfo::new("request_save_history"))
                .signal(SignalInfo::new("request_save_previous_state"))
                .signal(SignalInfo::new("go_to_help"))
                .signal(SignalInfo::new("search_in_files_requested"))
                .signal(SignalInfo::new("replace_in_files_requested"))
                .signal(SignalInfo::new("go_to_method")),
        );
    }

    // -- 3D gizmos (pat-nzgdu.194) ------------------------------------------
    //
    // ⚠️ `Node3DGizmo` declares NOTHING — no properties, no methods, no
    // virtuals — and is ABSTRACT. It exists only so `Node3D` can hold a gizmo
    // reference without the scene tree depending on the editor. The whole
    // surface the bead describes (`add_lines`, `add_handles`, `_redraw`, the
    // handle-drag callbacks) lives on **EditorNode3DGizmo**, its concrete
    // subclass. Registering that surface here would attribute it to the wrong
    // class and let a script call methods Godot's Node3DGizmo does not have.
    if !class_exists("Node3DGizmo") {
        register_class(
            ClassRegistration::new("Node3DGizmo")
                .parent("RefCounted")
                .abstract_class(),
        );
    }
    // The concrete gizmo: what a plugin builds during `_redraw`.
    if !class_exists("EditorNode3DGizmo") {
        register_class(
            ClassRegistration::new("EditorNode3DGizmo")
                .parent("Node3DGizmo")
                // Geometry a gizmo draws.
                .method(MethodInfo::new("add_lines", 2))
                .method(MethodInfo::new("add_mesh", 1))
                .method(MethodInfo::new("add_unscaled_billboard", 1))
                // The draggable dots. (handles, material, ids) — 3 required.
                .method(MethodInfo::new("add_handles", 3))
                // Picking geometry: what a click can hit, distinct from what is
                // drawn.
                .method(MethodInfo::new("add_collision_segments", 1))
                .method(MethodInfo::new("add_collision_triangles", 1))
                .method(MethodInfo::new("set_node_3d", 1))
                .method(MethodInfo::new("get_node_3d", 0))
                .method(MethodInfo::new("get_plugin", 0))
                .method(MethodInfo::new("clear", 0))
                .method(MethodInfo::new("set_hidden", 1))
                .method(MethodInfo::new("is_subgizmo_selected", 1))
                .method(MethodInfo::new("get_subgizmo_selection", 0))
                // The virtuals a custom gizmo overrides. ⚠️ These take
                // (handle_id, secondary); the PLUGIN's equivalents take an extra
                // leading `gizmo` argument — see below.
                .method(MethodInfo::new("_redraw", 0))
                .method(MethodInfo::new("_get_handle_name", 2))
                .method(MethodInfo::new("_is_handle_highlighted", 2))
                .method(MethodInfo::new("_get_handle_value", 2))
                .method(MethodInfo::new("_begin_handle_action", 2))
                .method(MethodInfo::new("_set_handle", 4))
                .method(MethodInfo::new("_commit_handle", 4))
                .method(MethodInfo::new("_subgizmos_intersect_ray", 2))
                .method(MethodInfo::new("_subgizmos_intersect_frustum", 2))
                .method(MethodInfo::new("_set_subgizmo_transform", 2))
                .method(MethodInfo::new("_get_subgizmo_transform", 1))
                .method(MethodInfo::new("_commit_subgizmos", 3)),
        );
    }
    // The registration path: a plugin says which nodes it handles and mints a
    // gizmo for each. ⚠️ A Resource, not a RefCounted — the editor keeps it.
    if !class_exists("EditorNode3DGizmoPlugin") {
        register_class(
            ClassRegistration::new("EditorNode3DGizmoPlugin")
                .parent("Resource")
                .method(MethodInfo::new("create_material", 2))
                .method(MethodInfo::new("create_icon_material", 2))
                .method(MethodInfo::new("create_handle_material", 1))
                .method(MethodInfo::new("add_material", 2))
                .method(MethodInfo::new("get_material", 1))
                // The two that make a plugin a plugin: which nodes it claims,
                // and the gizmo it builds for one.
                .method(MethodInfo::new("_has_gizmo", 1))
                .method(MethodInfo::new("_create_gizmo", 1))
                .method(MethodInfo::new("_get_gizmo_name", 0))
                // Priority breaks ties when two plugins claim the same node.
                .method(MethodInfo::new("_get_priority", 0))
                .method(MethodInfo::new("_can_be_hidden", 0))
                .method(MethodInfo::new("_is_selectable_when_hidden", 0))
                // ⚠️ Every hook below takes the GIZMO as its first argument, so
                // each is exactly one arity higher than EditorNode3DGizmo's
                // same-named virtual. A plugin serves many gizmos; a gizmo
                // serves one node.
                .method(MethodInfo::new("_redraw", 1))
                .method(MethodInfo::new("_get_handle_name", 3))
                .method(MethodInfo::new("_is_handle_highlighted", 3))
                .method(MethodInfo::new("_get_handle_value", 3))
                .method(MethodInfo::new("_begin_handle_action", 3))
                .method(MethodInfo::new("_set_handle", 5))
                .method(MethodInfo::new("_commit_handle", 5))
                .method(MethodInfo::new("_subgizmos_intersect_ray", 3))
                .method(MethodInfo::new("_subgizmos_intersect_frustum", 3))
                .method(MethodInfo::new("_get_subgizmo_transform", 2))
                .method(MethodInfo::new("_set_subgizmo_transform", 3))
                .method(MethodInfo::new("_commit_subgizmos", 4)),
        );
    }

    // -- GridMap editor tool (pat-nzgdu.193) --------------------------------
    //
    // ⚠️ This is NOT just a marker class. Its bound methods ARE the tool's API:
    // `get_current_grid_map` is which GridMap is selected,
    // `get_selected_palette_item` is which mesh the palette has highlighted, and
    // `get_selected_cells` is the paint selection. A plugin drives the tool
    // through these — so registering them is the contract the served editor's
    // endpoints have to satisfy, not decoration.
    if !class_exists("GridMapEditorPlugin") {
        register_class(
            ClassRegistration::new("GridMapEditorPlugin")
                .parent("EditorPlugin")
                .method(MethodInfo::new("get_current_grid_map", 0))
                .method(MethodInfo::new("set_selection", 2))
                .method(MethodInfo::new("clear_selection", 0))
                .method(MethodInfo::new("get_selection", 0))
                .method(MethodInfo::new("has_selection", 0))
                .method(MethodInfo::new("get_selected_cells", 0))
                .method(MethodInfo::new("set_selected_palette_item", 1))
                .method(MethodInfo::new("get_selected_palette_item", 0)),
        );
    }

    // -- Scene import plugins (pat-nzgdu.192) -------------------------------
    //
    // The scriptable scene-import pipeline. A format importer says which
    // extensions it handles and turns a file into a scene; a post-import hook
    // then mutates the result before it is saved.
    //
    // ⚠️ These are all `RefCounted`, NOT `ResourceImporter`. `EditorImportPlugin`
    // (already registered) derives ResourceImporter and imports *any* resource;
    // this family plugs into the **scene** importer specifically and shares no
    // base with it beyond RefCounted. Deriving them from ResourceImporter would
    // be a plausible-looking mistake.
    if !class_exists("EditorSceneFormatImporter") {
        register_class(
            ClassRegistration::new("EditorSceneFormatImporter")
                .parent("RefCounted")
                // Called from inside _get_import_options to declare options.
                .method(MethodInfo::new("add_import_option", 2))
                .method(MethodInfo::new("add_import_option_advanced", 3))
                // The two that make an importer an importer: which files it
                // claims, and how it turns one into a scene.
                .method(MethodInfo::new("_get_extensions", 0))
                .method(MethodInfo::new("_import_scene", 3))
                .method(MethodInfo::new("_get_import_options", 1))
                .method(MethodInfo::new("_get_option_visibility", 3)),
        );
    }
    // The concrete importers. Every one declares NOTHING of its own — the
    // format is the only difference, and it lives in `_get_extensions`.
    for importer in [
        "EditorSceneFormatImporterGLTF",
        // Blend and FBX2GLTF exist in Godot but shell out to Blender /
        // FBX2glTF. Registering them keeps ClassDB honest; making them *work*
        // needs a subprocess, which the web build has no answer for — see the
        // bead notes.
        "EditorSceneFormatImporterBlend",
        "EditorSceneFormatImporterFBX2GLTF",
        // UFBX decodes FBX in-process, so unlike FBX2GLTF it is portable.
        "EditorSceneFormatImporterUFBX",
    ] {
        if !class_exists(importer) {
            register_class(ClassRegistration::new(importer).parent("EditorSceneFormatImporter"));
        }
    }
    if !class_exists("EditorScenePostImport") {
        register_class(
            ClassRegistration::new("EditorScenePostImport")
                .parent("RefCounted")
                // Its whole surface: the hook, plus where the scene came from.
                .method(MethodInfo::new("_post_import", 1))
                .method(MethodInfo::new("get_source_file", 0)),
        );
    }
    // The richer sibling: EditorScenePostImport mutates the finished scene,
    // while EditorScenePostImportPlugin can also add import options and touch
    // each internal resource as it is built.
    if !class_exists("EditorScenePostImportPlugin") {
        register_class(
            ClassRegistration::new("EditorScenePostImportPlugin")
                .parent("RefCounted")
                .method(MethodInfo::new("get_option_value", 1))
                .method(MethodInfo::new("add_import_option", 2))
                .method(MethodInfo::new("add_import_option_advanced", 3))
                .method(MethodInfo::new("_get_internal_import_options", 1))
                .method(MethodInfo::new("_get_internal_option_visibility", 3))
                .method(MethodInfo::new(
                    "_get_internal_option_update_view_required",
                    2,
                ))
                .method(MethodInfo::new("_internal_process", 4))
                .method(MethodInfo::new("_get_import_options", 1))
                .method(MethodInfo::new("_get_option_visibility", 3))
                .method(MethodInfo::new("_pre_process", 1))
                .method(MethodInfo::new("_post_process", 1))
                // Which kind of thing `_internal_process` is being handed.
                .enumeration(
                    EnumInfo::new("InternalImportCategory")
                        .constant("INTERNAL_IMPORT_CATEGORY_NODE", 0)
                        .constant("INTERNAL_IMPORT_CATEGORY_MESH_3D_NODE", 1)
                        .constant("INTERNAL_IMPORT_CATEGORY_MESH", 2)
                        .constant("INTERNAL_IMPORT_CATEGORY_MATERIAL", 3)
                        .constant("INTERNAL_IMPORT_CATEGORY_ANIMATION", 4)
                        .constant("INTERNAL_IMPORT_CATEGORY_ANIMATION_NODE", 5)
                        .constant("INTERNAL_IMPORT_CATEGORY_SKELETON_3D_NODE", 6)
                        // A count sentinel, not a category — Godot's _MAX idiom.
                        .constant("INTERNAL_IMPORT_CATEGORY_MAX", 7),
                ),
        );
    }

    // -- Editor plugin scripting API (pat-nzgdu.191) ------------------------
    //
    // The classes a @tool EditorPlugin script reaches for. Registered here so
    // ClassDB reports them; the script binding over the existing backends
    // (editor_settings_store.rs / filesystem.rs / editor_compat.rs) is follow-up
    // — see the bead notes.
    //
    // Schemas diffed against 4.6.1. NOTE the bead's description names several
    // methods Godot does not have: `EditorSettings.get_setting_list` and
    // `property_get_revert`, and `FileSystemDock.get_current_path`. None exist;
    // they are not registered.
    if !class_exists("EditorSettings") {
        register_class(
            ClassRegistration::new("EditorSettings")
                // A Resource, not an Object: the editor saves it.
                .parent("Resource")
                .method(MethodInfo::new("has_setting", 1))
                .method(MethodInfo::new("set_setting", 2))
                .method(MethodInfo::new("get_setting", 1))
                .method(MethodInfo::new("erase", 1))
                .method(MethodInfo::new("set_initial_value", 3))
                .method(MethodInfo::new("add_property_info", 1))
                .method(MethodInfo::new("set_project_metadata", 3))
                .method(MethodInfo::new("get_project_metadata", 2))
                .method(MethodInfo::new("set_favorites", 1))
                .method(MethodInfo::new("get_favorites", 0))
                .method(MethodInfo::new("set_recent_dirs", 1))
                .method(MethodInfo::new("get_recent_dirs", 0))
                .method(MethodInfo::new("set_builtin_action_override", 2))
                .method(MethodInfo::new("add_shortcut", 2))
                .method(MethodInfo::new("remove_shortcut", 1))
                .method(MethodInfo::new("is_shortcut", 2))
                .method(MethodInfo::new("has_shortcut", 1))
                .method(MethodInfo::new("get_shortcut", 1))
                .method(MethodInfo::new("get_shortcut_list", 0))
                .method(MethodInfo::new("check_changed_settings_in_group", 1))
                .method(MethodInfo::new("get_changed_settings", 0))
                .method(MethodInfo::new("mark_setting_changed", 1))
                .signal(SignalInfo::new("settings_changed")),
        );
    }
    if !class_exists("EditorSelection") {
        register_class(
            ClassRegistration::new("EditorSelection")
                .parent("Object")
                .method(MethodInfo::new("clear", 0))
                // `add_node`/`remove_node`, NOT `add`/`remove` — the bead's
                // shorthand would be the wrong names.
                .method(MethodInfo::new("add_node", 1))
                .method(MethodInfo::new("remove_node", 1))
                .method(MethodInfo::new("get_selected_nodes", 0))
                // Top-selected = the selection minus nodes whose parent is also
                // selected; distinct from get_selected_nodes.
                .method(MethodInfo::new("get_top_selected_nodes", 0))
                .method(MethodInfo::new("get_transformable_selected_nodes", 0))
                .signal(SignalInfo::new("selection_changed")),
        );
    }
    // ⚠️ ABSTRACT in Godot (is_instantiable=false): a script never constructs
    // one — it gets the editor's via EditorPlugin.get_undo_redo(). The bead does
    // not mention this.
    if !class_exists("EditorUndoRedoManager") {
        register_class(
            ClassRegistration::new("EditorUndoRedoManager")
                .parent("Object")
                .abstract_class()
                .method(MethodInfo::new("create_action", 1))
                .method(MethodInfo::new("commit_action", 0))
                .method(MethodInfo::new("is_committing_action", 0))
                .method(MethodInfo::new("force_fixed_history", 0))
                .method(MethodInfo::new("add_do_method", 2))
                .method(MethodInfo::new("add_undo_method", 2))
                .method(MethodInfo::new("add_do_property", 3))
                .method(MethodInfo::new("add_undo_property", 3))
                .method(MethodInfo::new("add_do_reference", 1))
                .method(MethodInfo::new("add_undo_reference", 1))
                // The editor keeps a separate undo history per scene/resource;
                // these are how a plugin finds the right one.
                .method(MethodInfo::new("get_object_history_id", 1))
                .method(MethodInfo::new("get_history_undo_redo", 1))
                .method(MethodInfo::new("clear_history", 0))
                .signal(SignalInfo::new("history_changed"))
                .signal(SignalInfo::new("version_changed")),
        );
    }
    if !class_exists("EditorScript") {
        register_class(
            ClassRegistration::new("EditorScript")
                .parent("RefCounted")
                .method(MethodInfo::new("add_root_node", 1))
                .method(MethodInfo::new("get_scene", 0))
                .method(MethodInfo::new("get_editor_interface", 0))
                // The one-shot entry point a @tool script overrides.
                .method(MethodInfo::new("_run", 0)),
        );
    }
    if !class_exists("EditorVCSInterface") {
        register_class(
            ClassRegistration::new("EditorVCSInterface")
                .parent("Object")
                // Its surface is almost entirely virtuals a VCS plugin
                // overrides; these few are the helpers Godot calls back into.
                .method(MethodInfo::new("create_diff_line", 4))
                .method(MethodInfo::new("create_diff_hunk", 4))
                .method(MethodInfo::new("create_diff_file", 2))
                .method(MethodInfo::new("create_commit", 5))
                .method(MethodInfo::new("create_status_file", 3))
                .method(MethodInfo::new("add_diff_hunks_into_diff_file", 2))
                .method(MethodInfo::new("add_line_diffs_into_diff_hunk", 2))
                .method(MethodInfo::new("popup_error", 1)),
        );
    }
    if !class_exists("EditorResourcePreviewGenerator") {
        register_class(
            ClassRegistration::new("EditorResourcePreviewGenerator")
                .parent("RefCounted")
                .method(MethodInfo::new("request_draw_and_wait", 1))
                .method(MethodInfo::new("_handles", 1))
                .method(MethodInfo::new("_generate", 3))
                .method(MethodInfo::new("_generate_from_path", 3))
                .method(MethodInfo::new("_generate_small_preview_automatically", 0))
                .method(MethodInfo::new("_can_generate_small_preview", 0)),
        );
    }
    if !class_exists("EditorResourceTooltipPlugin") {
        register_class(
            ClassRegistration::new("EditorResourceTooltipPlugin")
                .parent("RefCounted")
                .method(MethodInfo::new("request_thumbnail", 2))
                .method(MethodInfo::new("_handles", 1))
                .method(MethodInfo::new("_make_tooltip_for_path", 3)),
        );
    }
    // Declares no non-virtual methods at all — a parser plugin is pure override.
    if !class_exists("EditorTranslationParserPlugin") {
        register_class(
            ClassRegistration::new("EditorTranslationParserPlugin")
                .parent("RefCounted")
                .method(MethodInfo::new("_parse_file", 1))
                .method(MethodInfo::new("_get_recognized_extensions", 0)),
        );
    }
    // ⚠️ ABSTRACT (is_instantiable=false) — the editor owns the one toaster; a
    // plugin only pushes to it.
    if !class_exists("EditorToaster") {
        register_class(
            ClassRegistration::new("EditorToaster")
                .parent("HBoxContainer")
                .abstract_class()
                .method(MethodInfo::new("push_toast", 1)),
        );
    }

    // -- Syntax highlighting (pat-nzgdu.190) --------------------------------
    //
    // A three-link chain, and every link is concrete:
    //   Resource <- SyntaxHighlighter <- EditorSyntaxHighlighter <- GDScriptSyntaxHighlighter
    //
    // SyntaxHighlighter is the runtime base a TextEdit binds; EditorSyntaxHighlighter
    // adds the editor-plugin surface (name + languages + `_create`, so the script
    // editor can clone one per open file); GDScriptSyntaxHighlighter is the
    // concrete GDScript one.
    if !class_exists("SyntaxHighlighter") {
        register_class(
            ClassRegistration::new("SyntaxHighlighter")
                .parent("Resource")
                // Returns `{ start_column: { "color": Color } }` — a map from a
                // column to the colour that starts there
                // (`scene/resources/syntax_highlighter.cpp:212-213`).
                //
                // ⚠️ It takes only a LINE NUMBER, not text: the highlighter reads
                // the line from the TextEdit it is bound to, and returns an EMPTY
                // dict when it has none (`syntax_highlighter.cpp:41-43`). A
                // highlighter with no editor colours nothing.
                .method(MethodInfo::new("get_line_syntax_highlighting", 1))
                .method(MethodInfo::new("update_cache", 0))
                .method(MethodInfo::new("clear_highlighting_cache", 0))
                .method(MethodInfo::new("get_text_edit", 0))
                // The virtuals a custom highlighter overrides.
                .method(MethodInfo::new("_get_line_syntax_highlighting", 1))
                .method(MethodInfo::new("_clear_highlighting_cache", 0))
                .method(MethodInfo::new("_update_cache", 0)),
        );
    }
    if !class_exists("EditorSyntaxHighlighter") {
        register_class(
            ClassRegistration::new("EditorSyntaxHighlighter")
                .parent("SyntaxHighlighter")
                // ⚠️ EXACTLY three virtuals and nothing else. The bead's
                // description also lists `_get_supported_extensions` — Godot has
                // NO such virtual (`editor/script/script_editor_plugin.h:62-64`
                // declares only these three). Registering it would invent a
                // surface a plugin could override and Godot would never call.
                .method(MethodInfo::new("_get_name", 0))
                .method(MethodInfo::new("_get_supported_languages", 0))
                // `_create` is why this class exists: the script editor clones a
                // registered highlighter per open file rather than sharing one.
                .method(MethodInfo::new("_create", 0)),
        );
    }
    // Declares nothing of its own — it is the GDScript *implementation* of the
    // editor highlighter, and every knob is inherited.
    if !class_exists("GDScriptSyntaxHighlighter") {
        register_class(
            ClassRegistration::new("GDScriptSyntaxHighlighter").parent("EditorSyntaxHighlighter"),
        );
    }
    // CodeHighlighter is the concrete, *runtime* SyntaxHighlighter a TextEdit
    // binds to colour code without an editor (pat-nzgdu.198). Godot registers it
    // in `register_scene_types.cpp:533` (GDREGISTER_CLASS — concrete), next to
    // the base; it lives here beside its base so a single registration call
    // brings up the whole family.
    //
    // ⚠️ Its four scalar colours all default to `Color(0, 0, 0, 1)` — opaque
    // black, not transparent — and its three maps default empty. An empty
    // highlighter colours nothing (every `get_*_color` starts at black until a
    // theme/script fills it in), which is exactly Godot's behaviour.
    if !class_exists("CodeHighlighter") {
        register_class(
            ClassRegistration::new("CodeHighlighter")
                .parent("SyntaxHighlighter")
                .property(PropertyInfo::new(
                    "number_color",
                    Variant::Color(gdcore::math::Color::new(0.0, 0.0, 0.0, 1.0)),
                ))
                .property(PropertyInfo::new(
                    "symbol_color",
                    Variant::Color(gdcore::math::Color::new(0.0, 0.0, 0.0, 1.0)),
                ))
                .property(PropertyInfo::new(
                    "function_color",
                    Variant::Color(gdcore::math::Color::new(0.0, 0.0, 0.0, 1.0)),
                ))
                .property(PropertyInfo::new(
                    "member_variable_color",
                    Variant::Color(gdcore::math::Color::new(0.0, 0.0, 0.0, 1.0)),
                ))
                .property(PropertyInfo::new(
                    "keyword_colors",
                    Variant::Dictionary(gdvariant::Dictionary::new()),
                ))
                .property(PropertyInfo::new(
                    "member_keyword_colors",
                    Variant::Dictionary(gdvariant::Dictionary::new()),
                ))
                .property(PropertyInfo::new(
                    "color_regions",
                    Variant::Dictionary(gdvariant::Dictionary::new()),
                ))
                .method(MethodInfo::new("add_keyword_color", 2))
                .method(MethodInfo::new("remove_keyword_color", 1))
                .method(MethodInfo::new("has_keyword_color", 1))
                .method(MethodInfo::new("get_keyword_color", 1))
                .method(MethodInfo::new("set_keyword_colors", 1))
                .method(MethodInfo::new("clear_keyword_colors", 0))
                .method(MethodInfo::new("get_keyword_colors", 0))
                .method(MethodInfo::new("add_member_keyword_color", 2))
                .method(MethodInfo::new("remove_member_keyword_color", 1))
                .method(MethodInfo::new("has_member_keyword_color", 1))
                .method(MethodInfo::new("get_member_keyword_color", 1))
                .method(MethodInfo::new("set_member_keyword_colors", 1))
                .method(MethodInfo::new("clear_member_keyword_colors", 0))
                .method(MethodInfo::new("get_member_keyword_colors", 0))
                .method(MethodInfo::new("add_color_region", 4))
                .method(MethodInfo::new("remove_color_region", 1))
                .method(MethodInfo::new("has_color_region", 1))
                .method(MethodInfo::new("set_color_regions", 1))
                .method(MethodInfo::new("clear_color_regions", 0))
                .method(MethodInfo::new("get_color_regions", 0))
                .method(MethodInfo::new("set_function_color", 1))
                .method(MethodInfo::new("get_function_color", 0))
                .method(MethodInfo::new("set_number_color", 1))
                .method(MethodInfo::new("get_number_color", 0))
                .method(MethodInfo::new("set_symbol_color", 1))
                .method(MethodInfo::new("get_symbol_color", 0))
                .method(MethodInfo::new("set_member_variable_color", 1))
                .method(MethodInfo::new("get_member_variable_color", 0)),
        );
    }

    // -- MovieWriter (pat-nzgdu.180): abstract offline video-capture base --
    // Scripts subclass it and register via `movie_writer::add_writer`; actual
    // frame/audio writing is deferred (out of web scope). The class surface must
    // exist so script/tool references resolve without error.
    if !class_exists("MovieWriter") {
        register_class(
            ClassRegistration::new("MovieWriter")
                .parent("Object")
                .abstract_class()
                .method(MethodInfo::new("_handles_file", 1))
                .method(MethodInfo::new("_get_audio_mix_rate", 0))
                .method(MethodInfo::new("_get_audio_speaker_mode", 0))
                .method(MethodInfo::new("_write_begin", 3))
                .method(MethodInfo::new("_write_frame", 2))
                .method(MethodInfo::new("_write_end", 0))
                .method(MethodInfo::new("add_writer", 1)),
        );
    }

    // -- EditorPlugin --
    if !class_exists("EditorPlugin") {
        register_class(
            ClassRegistration::new("EditorPlugin")
                .parent("Node")
                .method(MethodInfo::new("get_editor_interface", 0))
                .method(MethodInfo::new("add_control_to_dock", 2))
                .method(MethodInfo::new("remove_control_from_docks", 1))
                .method(MethodInfo::new("add_control_to_bottom_panel", 2))
                .method(MethodInfo::new("remove_control_from_bottom_panel", 1))
                .method(MethodInfo::new("add_custom_type", 4))
                .method(MethodInfo::new("remove_custom_type", 1))
                .method(MethodInfo::new("add_autoload_singleton", 2))
                .method(MethodInfo::new("remove_autoload_singleton", 1)),
        );
    }

    // -- EditorInterface --
    if !class_exists("EditorInterface") {
        register_class(
            ClassRegistration::new("EditorInterface")
                .parent("Node")
                .method(MethodInfo::new("get_editor_settings", 0))
                .method(MethodInfo::new("get_selection", 0))
                .method(MethodInfo::new("get_inspector", 0))
                .method(MethodInfo::new("get_file_system_dock", 0))
                .method(MethodInfo::new("get_edited_scene_root", 0))
                .method(MethodInfo::new("open_scene_from_path", 1))
                .method(MethodInfo::new("save_scene", 0))
                .method(MethodInfo::new("reload_scene_from_disk", 0))
                .method(MethodInfo::new("set_distraction_free_mode", 1))
                .method(MethodInfo::new("is_distraction_free_mode_enabled", 0)),
        );
    }

    register_resource_importer_classes();
}

/// Registers Godot's `ResourceImporter` family (pat-nzgdu.166).
///
/// One abstract base plus the 16 concrete importers the editor instantiates for
/// each source-asset type. `ResourceImporter` itself is `api_type: "core"`; the
/// 16 subclasses are `api_type: "editor"`, i.e. they exist only in an editor
/// build — which is why a game-runtime ClassDB would not carry them and this
/// lives in `register_editor_classes`.
///
/// **Import options are deliberately NOT registered as properties, because Godot
/// does not have them.** The whole family exposes **zero** properties in Godot's
/// `extension_api.json` — every one of the 17. An importer's options are not a
/// ClassDB surface at all: the editor asks the importer for them at runtime
/// (C++ `_get_import_options()`, not exposed to the extension API) and persists
/// the chosen values in the asset's `.import` file. So inventing per-importer
/// `compress/mode`-style properties here would be fiction that
/// `class_get_property_list` would then report as truth.
///
/// The surface really is this small: the base has one virtual
/// (`_get_build_dependencies`) and the `ImportOrder` enum, and the only subclass
/// adding anything is `ResourceImporterOggVorbis` (two static loaders, omitted
/// below — nothing dispatches them yet, and advertising a callable that errors
/// is worse than an honest gap).
fn register_resource_importer_classes() {
    // The base. Abstract, RefCounted — an importer is not a Resource.
    if !class_exists("ResourceImporter") {
        register_class(
            ClassRegistration::new("ResourceImporter")
                .parent("RefCounted")
                .abstract_class()
                // The one virtual Godot exposes: a script-defined importer
                // overrides it to declare what a reimport depends on.
                .method(MethodInfo::new("_get_build_dependencies", 1))
                .enumeration(
                    // Note SCENE is 100, not 1 — scenes import last so their
                    // dependencies (textures, meshes) are ready first.
                    EnumInfo::new("ImportOrder")
                        .constant("IMPORT_ORDER_DEFAULT", 0)
                        .constant("IMPORT_ORDER_SCENE", 100),
                ),
        );
    }

    // The 16 concrete importers. Godot gives each of them nothing of its own —
    // the type identity *is* the surface (the editor picks one per extension).
    for name in [
        "ResourceImporterBMFont",
        "ResourceImporterBitMap",
        "ResourceImporterCSVTranslation",
        "ResourceImporterDynamicFont",
        "ResourceImporterImage",
        "ResourceImporterImageFont",
        "ResourceImporterLayeredTexture",
        "ResourceImporterMP3",
        "ResourceImporterOBJ",
        "ResourceImporterOggVorbis",
        "ResourceImporterSVG",
        "ResourceImporterScene",
        "ResourceImporterShaderFile",
        "ResourceImporterTexture",
        "ResourceImporterTextureAtlas",
        "ResourceImporterWAV",
    ] {
        if !class_exists(name) {
            register_class(ClassRegistration::new(name).parent("ResourceImporter"));
        }
    }

    // `EditorImportPlugin` also derives ResourceImporter, and it is where the
    // bead's "per-type import options" actually live — a script subclasses it and
    // overrides `_get_import_options`/`_get_preset_*`/`_get_option_visibility` to
    // describe its options, then `_import` to produce the resource. Note it still
    // has **zero properties**: even the class built for import options doesn't
    // model them as ClassDB properties, which is the clearest evidence that
    // options are a virtual/`.import`-file surface rather than a schema.
    //
    // Only the virtuals are registered. Godot's one concrete method here,
    // `append_import_external_resource`, is omitted: nothing dispatches it, and a
    // registered method that errors on call is worse than an honest gap
    // (pat-nzgdu.155).
    if !class_exists("EditorImportPlugin") {
        register_class(
            ClassRegistration::new("EditorImportPlugin")
                .parent("ResourceImporter")
                .method(MethodInfo::new("_get_importer_name", 0))
                .method(MethodInfo::new("_get_visible_name", 0))
                .method(MethodInfo::new("_get_preset_count", 0))
                .method(MethodInfo::new("_get_preset_name", 1))
                .method(MethodInfo::new("_get_recognized_extensions", 0))
                .method(MethodInfo::new("_get_import_options", 2))
                .method(MethodInfo::new("_get_save_extension", 0))
                .method(MethodInfo::new("_get_resource_type", 0))
                .method(MethodInfo::new("_get_priority", 0))
                .method(MethodInfo::new("_get_import_order", 0))
                .method(MethodInfo::new("_get_format_version", 0))
                .method(MethodInfo::new("_get_option_visibility", 3))
                .method(MethodInfo::new("_import", 5))
                .method(MethodInfo::new("_can_import_threaded", 0)),
        );
    }
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

    // -- pat-nzgdu.98: signals / integer constants / enums on ClassInfo -------

    #[test]
    fn node_signals_are_registered_and_inherited() {
        let _guard = setup();
        register_2d_classes();

        assert!(class_has_signal("Node", "tree_entered"));
        assert!(class_has_signal("Node", "ready"));
        assert!(!class_has_signal("Node", "__no_such_signal__"));
        assert!(
            !get_signal_list("Node").is_empty(),
            "class_get_signal_list(\"Node\") must be non-empty",
        );

        // Signals inherit down the chain (Node2D -> CanvasItem -> Node).
        assert!(class_has_signal("Node2D", "tree_entered"));
        assert!(class_has_signal("Node2D", "script_changed"), "from Object");

        // Base-first ordering, matching get_property_list/get_method_list.
        let names: Vec<String> = get_signal_list("Node2D")
            .into_iter()
            .map(|s| s.name)
            .collect();
        let object_sig = names.iter().position(|n| n == "script_changed");
        let node_sig = names.iter().position(|n| n == "tree_entered");
        assert!(
            object_sig < node_sig,
            "Object signals must come before Node signals: {names:?}",
        );
    }

    #[test]
    fn signal_arguments_are_recorded() {
        let _guard = setup();
        register_2d_classes();

        let signals = get_signal_list("Node");
        let entered = signals
            .iter()
            .find(|s| s.name == "child_entered_tree")
            .expect("child_entered_tree is registered");
        assert_eq!(entered.arguments, vec!["node".to_string()]);

        let ready = signals.iter().find(|s| s.name == "ready").expect("ready");
        assert!(ready.arguments.is_empty(), "ready carries no arguments");
    }

    #[test]
    fn node_integer_constants_track_the_notification_codes() {
        use crate::notification::{NOTIFICATION_ENTER_TREE, NOTIFICATION_READY};

        let _guard = setup();
        register_2d_classes();

        // Sourced from crate::notification, so ClassDB cannot drift from the
        // codes the engine actually dispatches.
        assert_eq!(
            class_get_integer_constant("Node", "NOTIFICATION_READY"),
            Some(NOTIFICATION_READY.code() as i64),
        );
        assert_eq!(
            class_get_integer_constant("Node", "NOTIFICATION_ENTER_TREE"),
            Some(NOTIFICATION_ENTER_TREE.code() as i64),
        );
        assert!(get_integer_constant_list("Node").contains(&"NOTIFICATION_READY".to_string()));

        // Inherited, and absent constants are None rather than 0.
        assert!(class_get_integer_constant("Node2D", "NOTIFICATION_READY").is_some());
        assert_eq!(class_get_integer_constant("Node", "__nope__"), None);
    }

    #[test]
    fn enums_register_their_constants_as_integer_constants_too() {
        let _guard = setup();
        register_2d_classes();

        assert!(class_has_enum("Node", "ProcessMode"));
        assert!(get_enum_list("Node").contains(&"ProcessMode".to_string()));
        assert_eq!(
            get_enum_constants("Node", "ProcessMode"),
            vec![
                "PROCESS_MODE_INHERIT",
                "PROCESS_MODE_PAUSABLE",
                "PROCESS_MODE_WHEN_PAUSED",
                "PROCESS_MODE_ALWAYS",
                "PROCESS_MODE_DISABLED",
            ],
        );
        // Godot exposes enum constants through class_get_integer_constant too.
        assert_eq!(
            class_get_integer_constant("Node", "PROCESS_MODE_ALWAYS"),
            Some(3),
        );
        assert_eq!(
            class_get_integer_constant("Node", "DUPLICATE_USE_INSTANTIATION"),
            Some(8),
        );

        // Inherited; unknown enums are empty, not a panic.
        assert!(class_has_enum("Node2D", "ProcessMode"));
        assert!(get_enum_constants("Node", "__NoSuchEnum__").is_empty());
        assert!(!class_has_enum("Node", "__NoSuchEnum__"));
    }

    #[test]
    fn node_exposes_its_core_method_surface() {
        let _guard = setup();
        register_2d_classes();

        // The acceptance script asserts exactly this call.
        assert!(class_has_method("Node", "get_parent"));
        assert!(class_has_method("Node", "queue_free"));
        // Inherited from Object.
        assert!(class_has_method("Node", "get_instance_id"));
        assert!(class_has_method("Node2D", "get_parent"));
        assert!(!get_method_list("Node").is_empty());
    }

    #[test]
    fn object_and_node_metadata_do_not_depend_on_registration_order() {
        // Every register_*_classes entry point bootstraps Object/Node behind a
        // class_exists guard, so whichever runs first defines the metadata.
        // Each must produce the same result.
        for (label, register) in [
            ("2d", register_2d_classes as fn()),
            ("3d", register_3d_classes as fn()),
            ("viewport", register_viewport_classes as fn()),
            ("audio", register_audio_classes as fn()),
            ("networking", register_networking_classes as fn()),
        ] {
            let _guard = setup();
            register();
            assert!(
                class_has_signal("Node", "tree_entered"),
                "{label} registered Node without signals",
            );
            assert!(
                class_has_method("Node", "get_parent"),
                "{label} registered Node without methods",
            );
            assert!(
                class_has_signal("Object", "script_changed"),
                "{label} registered Object without signals",
            );
        }
    }

    /// pat-nzgdu.153: the AudioStreamPlayback runtime-instance family, with
    /// AudioStreamPlaybackResampled sitting between the base and the decoder
    /// playbacks that need sample-rate conversion.
    #[test]
    fn audiostream_playback_family_registered() {
        let _guard = setup();
        register_audio_classes();

        // The base is an abstract RefCounted — NOT a Resource: Godot separates
        // the AudioStream resource from the playback instance it hands out.
        assert!(class_exists("AudioStreamPlayback"));
        assert!(is_parent_class("AudioStreamPlayback", "RefCounted"));
        assert!(is_parent_class("AudioStreamPlayback", "Object"));
        assert!(is_abstract("AudioStreamPlayback"));
        assert!(class_has_method("AudioStreamPlayback", "mix"));

        assert!(is_abstract("AudioStreamPlaybackResampled"));
        assert!(is_parent_class(
            "AudioStreamPlaybackResampled",
            "AudioStreamPlayback"
        ));

        // "Resampled between": the decoders derive Resampled, which derives the
        // base — so they inherit the base's mix() through it.
        for name in [
            "AudioStreamPlaybackOggVorbis",
            "AudioStreamGeneratorPlayback",
        ] {
            assert!(class_exists(name), "{name}");
            assert!(
                is_parent_class(name, "AudioStreamPlaybackResampled"),
                "{name} must derive Resampled",
            );
            assert!(is_parent_class(name, "AudioStreamPlayback"), "{name}");
            assert!(class_has_method(name, "mix"), "{name} inherits mix");
            assert!(can_instantiate(name), "{name} is concrete");
        }

        // Container playbacks mix already-resampled children, so they derive the
        // base directly and must NOT sit under Resampled.
        for name in [
            "AudioStreamPlaybackPolyphonic",
            "AudioStreamPlaybackInteractive",
            "AudioStreamPlaybackPlaylist",
            "AudioStreamPlaybackSynchronized",
        ] {
            assert!(class_exists(name), "{name}");
            assert!(is_parent_class(name, "AudioStreamPlayback"), "{name}");
            assert!(
                !is_parent_class(name, "AudioStreamPlaybackResampled"),
                "{name} must NOT derive Resampled",
            );
            assert!(can_instantiate(name), "{name} is concrete");
        }

        // The generator's buffer surface is what scripts drive.
        assert!(class_has_method(
            "AudioStreamGeneratorPlayback",
            "push_buffer"
        ));
        assert!(class_has_method(
            "AudioStreamGeneratorPlayback",
            "get_frames_available"
        ));
        assert!(class_has_method(
            "AudioStreamPlaybackPolyphonic",
            "play_stream"
        ));
    }

    /// pat-nzgdu.146: Godot's web export exposes JavaScriptBridge in ClassDB
    /// (an Object singleton) alongside JavaScriptObject (a RefCounted wrapper
    /// around a JS value). Script dispatch already existed (pat-nzgdu.66); this
    /// is the ClassDB surface upstream also reports.
    #[test]
    fn javascript_bridge_class_surface_registered() {
        let _guard = setup();
        register_io_classes();

        assert!(class_exists("JavaScriptBridge"));
        assert!(is_parent_class("JavaScriptBridge", "Object"));
        for m in [
            "eval",
            "get_interface",
            "create_callback",
            "create_object",
            "download_buffer",
            "is_js_buffer",
            "js_buffer_to_packed_byte_array",
            "force_fs_sync",
            "pwa_needs_update",
            "pwa_update",
        ] {
            assert!(class_has_method("JavaScriptBridge", m), "method {m}");
        }
        // Godot fires this when a PWA update is staged.
        assert!(class_has_signal("JavaScriptBridge", "pwa_update_available"));

        // JavaScriptObject is what eval/get_interface/create_object hand back.
        assert!(class_exists("JavaScriptObject"));
        assert!(is_parent_class("JavaScriptObject", "RefCounted"));
        assert!(is_parent_class("JavaScriptObject", "Object"));
        assert!(can_instantiate("JavaScriptObject"));
    }

    /// pat-nzgdu.140: the VisualShader node-graph family is registered with the
    /// Godot parent chain, so a saved `.tres` VisualShader round-trips through
    /// ClassDB and the graph compiler has classes to instantiate.
    #[test]
    fn visual_shader_class_family_registered() {
        let _guard = setup();
        register_3d_classes();

        // VisualShader IS a Shader — that is what lets it stand in for text
        // shader source anywhere a Shader is accepted.
        assert!(class_exists("VisualShader"));
        assert!(is_parent_class("VisualShader", "Shader"));
        assert!(is_parent_class("VisualShader", "Resource"));
        assert!(is_parent_class("VisualShader", "Object"));
        assert!(can_instantiate("VisualShader"));
        assert!(class_has_method("VisualShader", "connect_nodes"));
        // Inherited from Shader.
        assert!(class_has_method("VisualShader", "get_code"));

        // The node base is abstract; concrete nodes derive from it.
        assert!(class_exists("VisualShaderNode"));
        assert!(is_parent_class("VisualShaderNode", "Resource"));
        assert!(is_abstract("VisualShaderNode"));
        assert!(!can_instantiate("VisualShaderNode"));

        for name in [
            "VisualShaderNodeInput",
            "VisualShaderNodeOutput",
            "VisualShaderNodeFloatConstant",
            "VisualShaderNodeColorConstant",
            "VisualShaderNodeVec3Constant",
            "VisualShaderNodeFloatOp",
            "VisualShaderNodeVectorOp",
            "VisualShaderNodeFloatFunc",
            "VisualShaderNodeMix",
            "VisualShaderNodeTexture",
            "VisualShaderNodeExpression",
            "VisualShaderNodeFloatParameter",
        ] {
            assert!(class_exists(name), "{name} is registered");
            assert!(
                is_parent_class(name, "VisualShaderNode"),
                "{name} derives VisualShaderNode",
            );
            assert!(can_instantiate(name), "{name} is concrete");
        }

        // The two abstract intermediates.
        assert!(is_abstract("VisualShaderNodeConstant"));
        assert!(is_abstract("VisualShaderNodeParameter"));
        assert!(is_parent_class(
            "VisualShaderNodeColorConstant",
            "VisualShaderNodeConstant"
        ));
        assert!(is_parent_class(
            "VisualShaderNodeFloatParameter",
            "VisualShaderNodeParameter"
        ));
    }

    #[test]
    fn visual_shader_node_defaults_and_enums() {
        let _guard = setup();
        register_3d_classes();

        // Constant nodes carry their value as a property default, so
        // instantiate() yields a usable node.
        let color = instantiate("VisualShaderNodeColorConstant").expect("instantiates");
        assert_eq!(
            color.get_property("constant"),
            Variant::Color(gdcore::math::Color::WHITE),
        );
        let f = instantiate("VisualShaderNodeFloatConstant").expect("instantiates");
        assert_eq!(f.get_property("constant"), Variant::Float(0.0));

        // Godot's VisualShader.Type / VisualShaderNode.PortType enums.
        assert!(class_has_enum("VisualShader", "Type"));
        assert_eq!(
            class_get_integer_constant("VisualShader", "TYPE_FRAGMENT"),
            Some(1),
        );
        assert_eq!(
            class_get_integer_constant("VisualShader", "TYPE_SKY"),
            Some(8)
        );
        assert!(class_has_enum("VisualShaderNode", "PortType"));
        // ⚠️ 4, not 5 (pat-nzgdu.227). This asserted 5 — the old, WRONG value.
        // Godot's PortType is consecutive (SCALAR=0 … MAX=9); the registration
        // had invented gaps from VECTOR_3D onward (5/7/8/9/10/12), so every
        // port-type comparison past VECTOR_2D silently disagreed with the
        // engine. This test was locking that bug in.
        assert_eq!(
            class_get_integer_constant("VisualShaderNode", "PORT_TYPE_VECTOR_3D"),
            Some(4),
        );
        assert_eq!(
            class_get_integer_constant("VisualShaderNode", "PORT_TYPE_MAX"),
            Some(9),
        );
        // Enums inherit, so a concrete node sees the base's PortType.
        assert!(class_has_enum("VisualShaderNodeColorConstant", "PortType"));
    }

    #[test]
    fn register_3d_classes_alone_reaches_the_object_root() {
        // Regression (pat-nzgdu.98): register_3d_classes did not bootstrap
        // Object/Node, so on its own it registered Node3D under a Node that did
        // not exist and inheritance_chain stopped at Node3D — severing every 3D
        // class from the root. This is what broke `xr_nodes_classdb_registered`.
        let _guard = setup();
        register_3d_classes();

        assert!(class_exists("Object"), "3D pass must bootstrap Object");
        assert!(class_exists("Node"), "3D pass must bootstrap Node");
        assert!(is_parent_class("Node3D", "Node"));
        assert!(is_parent_class("Node3D", "Object"));
        assert!(is_parent_class("XRServer", "Object"));
        // And the roots carry their metadata here too, not just via the 2D pass.
        assert!(class_has_signal("Node3D", "tree_entered"));
        assert!(class_has_method("Node3D", "get_parent"));
    }

    #[test]
    fn a_plain_registration_has_no_signal_metadata() {
        let _guard = setup();
        register_class(ClassRegistration::new("Object"));
        register_class(ClassRegistration::new("Plain").parent("Object"));

        // Default-empty: existing registrations are unaffected by the new fields.
        assert!(get_signal_list("Plain").is_empty());
        assert!(get_integer_constant_list("Plain").is_empty());
        assert!(get_enum_list("Plain").is_empty());
        assert!(!class_has_signal("Plain", "anything"));
        assert_eq!(class_get_integer_constant("Plain", "anything"), None);

        let info = get_class_info("Plain").expect("registered");
        assert!(info.signals.is_empty());
        assert!(info.integer_constants.is_empty());
        assert!(info.enums.is_empty());
    }

    #[test]
    fn unregistered_class_queries_are_empty_not_panics() {
        let _guard = setup();
        assert!(get_signal_list("__Ghost__").is_empty());
        assert!(get_integer_constant_list("__Ghost__").is_empty());
        assert!(get_enum_list("__Ghost__").is_empty());
        assert!(get_enum_constants("__Ghost__", "X").is_empty());
        assert!(!class_has_signal("__Ghost__", "x"));
        assert!(!class_has_enum("__Ghost__", "x"));
        assert_eq!(class_get_integer_constant("__Ghost__", "x"), None);
    }

    #[test]
    fn derived_constants_shadow_inherited_ones() {
        let _guard = setup();
        register_class(ClassRegistration::new("Base").constant("SHARED", 1));
        register_class(
            ClassRegistration::new("Derived")
                .parent("Base")
                .constant("SHARED", 2),
        );

        // Most-derived definition wins, as in Godot.
        assert_eq!(class_get_integer_constant("Derived", "SHARED"), Some(2));
        assert_eq!(class_get_integer_constant("Base", "SHARED"), Some(1));
    }

    /// pat-nzgdu.216: the VoxelGIData bake-result resource is registered under
    /// Resource, constructs via ClassDB with its config-property defaults, and
    /// exposes the data accessor method surface.
    #[test]
    fn voxelgidata_resource_registered_and_loads() {
        let _guard = setup();
        register_3d_classes();

        assert!(class_exists("VoxelGIData"), "VoxelGIData is registered");
        assert!(
            is_parent_class("VoxelGIData", "Resource"),
            "VoxelGIData derives Resource",
        );

        // Config properties expose their Godot defaults via instantiate.
        let data = instantiate("VoxelGIData").expect("VoxelGIData is instantiable");
        assert_eq!(data.get_property("energy"), Variant::Float(1.0));
        assert_eq!(data.get_property("bias"), Variant::Float(1.5));
        assert_eq!(data.get_property("propagation"), Variant::Float(0.7));
        assert_eq!(data.get_property("dynamic_range"), Variant::Float(2.0));

        // The allocate/get_* data accessors are on the class surface.
        let info = get_class_info("VoxelGIData").expect("VoxelGIData class info");
        let methods: Vec<&str> = info.methods.iter().map(|m| m.name.as_str()).collect();
        assert!(methods.contains(&"allocate"), "allocate() exposed");
        assert!(methods.contains(&"get_bounds"), "get_bounds() exposed");
        assert!(
            methods.contains(&"get_octree_size"),
            "get_octree_size() exposed"
        );
    }

    /// pat-nzgdu.205: the Mesh → PrimitiveMesh → shape hierarchy is registered
    /// with correct parents and abstract bases, and instantiate('BoxMesh')
    /// exposes the `size` resource property at its BoxMesh::default() value.
    #[test]
    fn primitive_mesh_family_registered_in_classdb() {
        use gdcore::math::Vector3;
        let _guard = setup();
        register_3d_classes();

        for cls in [
            "Mesh",
            "PrimitiveMesh",
            "BoxMesh",
            "SphereMesh",
            "CapsuleMesh",
            "CylinderMesh",
            "PlaneMesh",
        ] {
            assert!(class_exists(cls), "{cls} is registered in ClassDB");
        }
        // The two bases are abstract; the concrete primitives derive them.
        assert!(is_abstract("Mesh"), "Mesh is abstract");
        assert!(is_abstract("PrimitiveMesh"), "PrimitiveMesh is abstract");
        assert!(is_parent_class("BoxMesh", "PrimitiveMesh"));
        assert!(is_parent_class("BoxMesh", "Mesh"));
        assert!(is_parent_class("PrimitiveMesh", "Resource"));

        // instantiate('BoxMesh') exposes a `size` property defaulting to
        // Vector3(1,1,1), matching gdserver3d BoxMesh::default().
        let box_mesh = instantiate("BoxMesh").expect("BoxMesh is instantiable");
        assert_eq!(
            box_mesh.get_property("size"),
            Variant::Vector3(Vector3::new(1.0, 1.0, 1.0)),
            "instantiate('BoxMesh') exposes the size default",
        );
    }

    /// pat-nzgdu.95: the AnimationNode* graph-node resource hierarchy is
    /// registered in ClassDB with the correct parent chains, so
    /// `[sub_resource type="AnimationNode*"]` graphs resolve during .tres/.tscn
    /// load.
    #[test]
    fn animation_node_classdb_hierarchy() {
        let _guard = setup();
        register_3d_classes();

        assert!(class_exists("AnimationNode"), "AnimationNode registered");
        assert!(is_parent_class("AnimationNode", "Resource"));
        assert!(class_exists("AnimationRootNode"));
        assert!(is_parent_class("AnimationRootNode", "AnimationNode"));

        // BlendTree / StateMachine / Animation derive AnimationRootNode.
        for cls in [
            "AnimationNodeBlendTree",
            "AnimationNodeStateMachine",
            "AnimationNodeAnimation",
        ] {
            assert!(class_exists(cls), "{cls} registered");
            assert!(
                is_parent_class(cls, "AnimationRootNode"),
                "{cls} derives AnimationRootNode"
            );
        }

        // The blend/mix nodes derive AnimationNodeSync (← AnimationNode).
        assert!(is_parent_class("AnimationNodeSync", "AnimationNode"));
        for cls in [
            "AnimationNodeAdd2",
            "AnimationNodeSub2",
            "AnimationNodeBlend2",
            "AnimationNodeOneShot",
            "AnimationNodeTransition",
        ] {
            assert!(class_exists(cls), "{cls} registered");
            assert!(
                is_parent_class(cls, "AnimationNodeSync"),
                "{cls} derives AnimationNodeSync"
            );
            assert!(
                is_parent_class(cls, "AnimationNode"),
                "{cls} derives AnimationNode transitively"
            );
        }

        // TimeScale/TimeSeek derive AnimationNode directly.
        assert!(is_parent_class("AnimationNodeTimeScale", "AnimationNode"));
        assert!(is_parent_class("AnimationNodeTimeSeek", "AnimationNode"));
    }

    /// pat-p0s82.23: the XR node family + XRServer are registered in ClassDB with
    /// the correct parents (the mock pose backend lives in `gdplatform::xr` and is
    /// covered by its own `mock_session_pose` test).
    #[test]
    fn xr_nodes_classdb_registered() {
        let _guard = setup();
        register_3d_classes();

        assert!(class_exists("XRServer"), "XRServer registered");
        assert!(is_parent_class("XRServer", "Object"));

        assert!(class_exists("XRNode3D"));
        assert!(is_parent_class("XRNode3D", "Node3D"));

        assert!(class_exists("XROrigin3D"));
        assert!(is_parent_class("XROrigin3D", "Node3D"));

        assert!(class_exists("XRCamera3D"));
        assert!(is_parent_class("XRCamera3D", "Camera3D"));

        assert!(class_exists("XRController3D"));
        assert!(is_parent_class("XRController3D", "XRNode3D"));
        assert!(
            is_parent_class("XRController3D", "Node3D"),
            "XRController3D → XRNode3D → Node3D"
        );

        assert!(class_exists("XRAnchor3D"));
        assert!(is_parent_class("XRAnchor3D", "XRNode3D"));
    }

    /// pat-nzgdu.93: ParticleProcessMaterial is registered in ClassDB as a
    /// Material resource with its high-use property surface + Godot defaults, so
    /// a `.tscn` `process_material = SubResource("ParticleProcessMaterial_…")`
    /// resolves to the right class.
    #[test]
    fn particle_process_material_classdb_registered() {
        let _guard = setup();
        register_3d_classes();

        assert!(
            class_exists("ParticleProcessMaterial"),
            "ParticleProcessMaterial must be registered"
        );
        assert!(is_parent_class("ParticleProcessMaterial", "Material"));
        assert!(is_parent_class("ParticleProcessMaterial", "Resource"));

        let info = get_class_info("ParticleProcessMaterial").expect("info");
        let prop = |n: &str| {
            info.properties
                .iter()
                .find(|p| p.name.as_str() == n)
                .map(|p| p.default_value.clone())
        };
        assert_eq!(
            prop("emission_shape"),
            Some(Variant::Int(0)),
            "emission_shape defaults to POINT (0)"
        );
        assert_eq!(
            prop("spread"),
            Some(Variant::Float(45.0)),
            "spread defaults to 45"
        );
        assert!(prop("gravity").is_some(), "gravity property present");
        assert!(prop("initial_velocity_min").is_some());
        assert!(prop("initial_velocity_max").is_some());
        assert!(prop("emission_box_extents").is_some());
    }

    /// pat-nzgdu.96: the Shape3D resource base and its concrete shapes are
    /// registered in ClassDB with the correct parent chain; Shape3D carries
    /// `margin` (Godot default 0.04) and SeparationRayShape3D carries
    /// length/slide_on_slope.
    #[test]
    fn shape3d_resource_classdb_test() {
        let _guard = setup();
        register_3d_classes();

        // Shape3D base derives from Resource.
        assert!(class_exists("Shape3D"), "Shape3D must be in ClassDB");
        assert!(
            is_parent_class("Shape3D", "Resource"),
            "Shape3D derives from Resource"
        );

        // SeparationRayShape3D derives from Shape3D (and thus Resource).
        assert!(class_exists("SeparationRayShape3D"));
        assert!(is_parent_class("SeparationRayShape3D", "Shape3D"));
        assert!(is_parent_class("SeparationRayShape3D", "Resource"));

        // Every concrete shape a CollisionShape3D.shape references derives Shape3D.
        for shape in [
            "SphereShape3D",
            "BoxShape3D",
            "CapsuleShape3D",
            "CylinderShape3D",
            "ConvexPolygonShape3D",
            "ConcavePolygonShape3D",
            "WorldBoundaryShape3D",
            "HeightMapShape3D",
        ] {
            assert!(class_exists(shape), "{shape} must be registered");
            assert!(
                is_parent_class(shape, "Shape3D"),
                "{shape} derives from Shape3D"
            );
        }

        // Shape3D.margin round-trips through ClassDB with Godot's default 0.04.
        let info = get_class_info("Shape3D").expect("Shape3D info");
        let margin = info
            .properties
            .iter()
            .find(|p| p.name.as_str() == "margin")
            .expect("Shape3D exposes a margin property");
        assert_eq!(
            margin.default_value,
            Variant::Float(0.04),
            "Shape3D.margin default is 0.04"
        );

        // SeparationRayShape3D exposes length + slide_on_slope.
        let sr = get_class_info("SeparationRayShape3D").expect("SeparationRayShape3D info");
        assert!(sr.properties.iter().any(|p| p.name.as_str() == "length"));
        assert!(sr
            .properties
            .iter()
            .any(|p| p.name.as_str() == "slide_on_slope"));
    }

    // pat-nzgdu.56: MeshInstance3D and the drawable-instance family inherit
    // GeometryInstance3D → VisualInstance3D, exposing the material / transparency
    // / shadow / GI / visibility-range property surface through ClassDB.
    #[test]
    fn geometry_instance3d_hierarchy_and_properties() {
        let _guard = setup();
        register_3d_classes();

        // GeometryInstance3D exists under VisualInstance3D → Node3D.
        assert!(class_exists("GeometryInstance3D"));
        assert!(is_parent_class("GeometryInstance3D", "VisualInstance3D"));
        assert!(is_parent_class("GeometryInstance3D", "Node3D"));

        // The drawable-instance family now inherits it (and is still a Node3D).
        for cls in ["MeshInstance3D", "MultiMeshInstance3D", "GPUParticles3D"] {
            assert!(
                is_parent_class(cls, "GeometryInstance3D"),
                "{cls} should inherit GeometryInstance3D"
            );
            assert!(
                is_parent_class(cls, "Node3D"),
                "{cls} should still be a Node3D transitively"
            );
        }

        // The VisualInstance3D + GeometryInstance3D property surface is reachable
        // from MeshInstance3D via inheritance.
        let props = get_property_list("MeshInstance3D");
        let names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
        for expected in [
            "layers",            // VisualInstance3D
            "material_override", // GeometryInstance3D
            "material_overlay",
            "transparency",
            "cast_shadow",
            "gi_mode",
            "extra_cull_margin",
            "lod_bias",
            "visibility_range_begin",
            "custom_aabb",
        ] {
            assert!(
                names.contains(&expected),
                "MeshInstance3D missing inherited property {expected}; got {names:?}"
            );
        }
    }

    #[test]
    fn navigation_classes_registered() {
        let _guard = setup();
        register_navigation_classes();

        for cls in ["AStar2D", "AStar3D", "AStarGrid2D"] {
            assert!(class_exists(cls), "{cls} should be registered");
            assert!(
                is_parent_class(cls, "RefCounted"),
                "{cls} should inherit RefCounted"
            );
        }

        // AStar2D/3D core graph API.
        for cls in ["AStar2D", "AStar3D"] {
            for method in [
                "add_point",
                "connect_points",
                "are_points_connected",
                "get_id_path",
                "get_point_path",
                "get_closest_point",
                "set_point_disabled",
                "set_point_weight_scale",
            ] {
                assert!(
                    class_has_method(cls, method),
                    "{cls} should have method {method}"
                );
            }
        }

        // AStarGrid2D grid API.
        for method in [
            "set_region",
            "set_cell_size",
            "set_diagonal_mode",
            "set_default_compute_heuristic",
            "set_point_solid",
            "set_jumping_enabled",
            "get_id_path",
            "get_point_path",
        ] {
            assert!(
                class_has_method("AStarGrid2D", method),
                "AStarGrid2D should have method {method}"
            );
        }
    }

    #[test]
    fn register_and_lookup() {
        let _g = setup();

        let id = register_class(ClassRegistration::new("Object"));

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

        register_class(ClassRegistration::new("Object"));
        register_class(
            ClassRegistration::new("Node")
                .parent("Object")
                .property(PropertyInfo::new("name", Variant::String(String::new())))
                .method(MethodInfo::new("_ready", 0)),
        );
        register_class(ClassRegistration::new("Node2D").parent("Node").property(
            PropertyInfo::new("position", Variant::Vector2(gdcore::math::Vector2::ZERO)),
        ));

        let obj = instantiate("Node2D").expect("should create Node2D");
        assert_eq!(obj.get_class(), "Node2D");
        assert_eq!(obj.get_property("name"), Variant::String(String::new()),);
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

    #[test]
    fn register_same_class_twice_overwrites() {
        let _g = setup();
        let id1 = register_class(ClassRegistration::new("Duplicate"));
        let id2 = register_class(ClassRegistration::new("Duplicate"));
        // Second registration gets a new ID
        assert_ne!(id1, id2);
        // Only one class entry in the map
        assert_eq!(class_count(), 1);
        // The latest ID is the one stored
        let info = get_class_info("Duplicate").unwrap();
        assert_eq!(info.class_id, id2);
    }

    #[test]
    fn lookup_nonexistent_class_returns_none() {
        let _g = setup();
        assert!(get_class_info("DoesNotExist").is_none());
    }

    #[test]
    fn lookup_nonexistent_by_id_returns_none() {
        let _g = setup();
        assert!(get_class_info_by_id(ClassId::new(9999)).is_none());
    }

    #[test]
    fn inheritance_depth_greater_than_two() {
        let _g = setup();
        register_class(ClassRegistration::new("Object"));
        register_class(ClassRegistration::new("Node").parent("Object"));
        register_class(ClassRegistration::new("CanvasItem").parent("Node"));
        register_class(ClassRegistration::new("Node2D").parent("CanvasItem"));
        register_class(ClassRegistration::new("Sprite2D").parent("Node2D"));

        let chain = inheritance_chain("Sprite2D");
        assert_eq!(
            chain,
            vec!["Sprite2D", "Node2D", "CanvasItem", "Node", "Object"]
        );
        assert_eq!(chain.len(), 5);

        assert!(is_parent_class("Sprite2D", "Object"));
        assert!(is_parent_class("Sprite2D", "CanvasItem"));
        assert!(!is_parent_class("Object", "Sprite2D"));
    }

    #[test]
    fn inheritance_chain_of_unregistered_class() {
        let _g = setup();
        let chain = inheritance_chain("Unknown");
        assert!(chain.is_empty());
    }

    #[test]
    fn is_parent_class_self() {
        let _g = setup();
        register_class(ClassRegistration::new("TestSelf"));
        assert!(is_parent_class("TestSelf", "TestSelf"));
    }

    /// pat-rzs4o: NavigationAgent2D/3D must be registered for scene load / create dialog.
    #[test]
    fn navigation_agents_registered_in_default_classdb() {
        let _g = setup();
        register_2d_classes();
        register_3d_classes();
        assert!(
            class_exists("NavigationAgent2D"),
            "NavigationAgent2D must be in ClassDB"
        );
        assert!(
            class_exists("NavigationAgent3D"),
            "NavigationAgent3D must be in ClassDB"
        );
        assert!(is_parent_class("NavigationAgent2D", "Node"));
        assert!(is_parent_class("NavigationAgent3D", "Node"));

        let info2 = get_class_info("NavigationAgent2D").expect("NavigationAgent2D info");
        let props2: Vec<&str> = info2.properties.iter().map(|p| p.name.as_str()).collect();
        assert!(
            props2.contains(&"target_position"),
            "NavigationAgent2D needs target_position: {props2:?}"
        );
        assert!(props2.contains(&"path_desired_distance"));
        assert!(props2.contains(&"avoidance_enabled"));

        let info3 = get_class_info("NavigationAgent3D").expect("NavigationAgent3D info");
        let props3: Vec<&str> = info3.properties.iter().map(|p| p.name.as_str()).collect();
        assert!(props3.contains(&"target_position"));
        assert!(props3.contains(&"path_desired_distance"));
        assert!(props3.contains(&"avoidance_enabled"));

        // pat-nzgdu.10 — the full Godot NavigationAgent property surface.
        // Shared across 2D and 3D.
        for p in [
            "target_desired_distance",
            "path_max_distance",
            "navigation_layers",
            "pathfinding_algorithm",
            "path_postprocessing",
            "path_metadata_flags",
            "simplify_path",
            "simplify_epsilon",
            "radius",
            "neighbor_distance",
            "max_neighbors",
            "time_horizon_agents",
            "time_horizon_obstacles",
            "max_speed",
            "avoidance_layers",
            "avoidance_mask",
            "avoidance_priority",
            "velocity",
        ] {
            assert!(
                class_has_property("NavigationAgent2D", p),
                "NavigationAgent2D missing {p}"
            );
            assert!(
                class_has_property("NavigationAgent3D", p),
                "NavigationAgent3D missing {p}"
            );
        }
        // 3D-only properties.
        assert!(class_has_property(
            "NavigationAgent3D",
            "path_height_offset"
        ));
        assert!(class_has_property("NavigationAgent3D", "height"));
        assert!(class_has_property("NavigationAgent3D", "use_3d_avoidance"));
        assert!(class_has_property("NavigationAgent3D", "keep_y_velocity"));
        // 2D-only property.
        assert!(class_has_property(
            "NavigationAgent2D",
            "debug_path_custom_line_width"
        ));
        // 3D must NOT carry the 2D-only line-width property.
        assert!(!class_has_property(
            "NavigationAgent3D",
            "debug_path_custom_line_width"
        ));
    }

    /// pat-n1gh1: VehicleBody3D / VehicleWheel3D / SoftBody3D ClassDB surface.
    #[test]
    fn vehicle_and_softbody_registered_in_classdb() {
        let _g = setup();
        register_3d_classes();
        assert!(class_exists("VehicleBody3D"));
        assert!(class_exists("VehicleWheel3D"));
        assert!(class_exists("SoftBody3D"));
        assert!(is_parent_class("VehicleBody3D", "RigidBody3D"));
        assert!(is_parent_class("VehicleWheel3D", "Node3D"));
        assert!(is_parent_class("SoftBody3D", "MeshInstance3D"));

        let vb = get_class_info("VehicleBody3D").unwrap();
        let names: Vec<&str> = vb.properties.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"engine_force"));
        assert!(names.contains(&"brake"));
        assert!(names.contains(&"steering"));

        let sb = get_class_info("SoftBody3D").unwrap();
        let snames: Vec<&str> = sb.properties.iter().map(|p| p.name.as_str()).collect();
        assert!(snames.contains(&"linear_stiffness") || snames.contains(&"total_mass"));
    }

    #[test]
    fn instantiate_applies_inherited_defaults() {
        let _g = setup();
        register_class(ClassRegistration::new("Object"));
        register_class(
            ClassRegistration::new("Node")
                .parent("Object")
                .property(PropertyInfo::new("name", Variant::String("".into()))),
        );
        register_class(
            ClassRegistration::new("Node2D")
                .parent("Node")
                .property(PropertyInfo::new(
                    "position",
                    Variant::Vector2(gdcore::math::Vector2::ZERO),
                ))
                .property(PropertyInfo::new(
                    "name",
                    Variant::String("default_2d".into()),
                )),
        );

        let obj = instantiate("Node2D").unwrap();
        // Derived default overrides base default
        assert_eq!(
            obj.get_property("name"),
            Variant::String("default_2d".into())
        );
        assert_eq!(
            obj.get_property("position"),
            Variant::Vector2(gdcore::math::Vector2::ZERO)
        );
    }

    #[test]
    fn class_registration_builder_methods() {
        let _g = setup();
        let id = register_class(
            ClassRegistration::new("FullClass")
                .parent("Object")
                .property(PropertyInfo::new("hp", Variant::Int(100)))
                .method(MethodInfo::new("take_damage", 1))
                .method(MethodInfo::new("heal", 1)),
        );
        let info = get_class_info_by_id(id).unwrap();
        assert_eq!(info.properties.len(), 1);
        assert_eq!(info.methods.len(), 2);
        assert_eq!(info.methods[0].name, "take_damage");
        assert_eq!(info.methods[0].argument_count, 1);
    }

    /// pat-zx775: SubViewport + ViewportTexture must be in ClassDB with Godot defaults.
    #[test]
    fn subviewport_and_viewport_texture_registered() {
        let _g = setup();
        register_viewport_classes();

        assert!(class_exists("Viewport"), "Viewport base must be in ClassDB");
        assert!(
            class_exists("SubViewport"),
            "SubViewport must be in ClassDB"
        );
        assert!(class_exists("Texture2D"), "Texture2D must be in ClassDB");
        assert!(
            class_exists("ViewportTexture"),
            "ViewportTexture must be in ClassDB"
        );

        assert!(is_parent_class("SubViewport", "Viewport"));
        assert!(is_parent_class("SubViewport", "Node"));
        assert!(is_parent_class("ViewportTexture", "Texture2D"));
        assert!(is_parent_class("ViewportTexture", "Resource"));

        let sv = get_class_info("SubViewport").expect("SubViewport info");
        let props: Vec<&str> = sv.properties.iter().map(|p| p.name.as_str()).collect();
        assert!(props.contains(&"size"), "missing size: {props:?}");
        assert!(
            props.contains(&"render_target_update_mode"),
            "missing render_target_update_mode: {props:?}"
        );
        assert!(
            props.contains(&"render_target_clear_mode"),
            "missing render_target_clear_mode: {props:?}"
        );
        assert!(
            props.contains(&"transparent_bg"),
            "missing transparent_bg: {props:?}"
        );

        // Defaults match Godot: size 512×512, update WHEN_VISIBLE (2), clear ALWAYS (0).
        let size = sv
            .properties
            .iter()
            .find(|p| p.name == "size")
            .map(|p| &p.default_value)
            .expect("size prop");
        match size {
            Variant::Vector2(v) => {
                assert!((v.x - 512.0).abs() < 0.01, "size.x default {v:?}");
                assert!((v.y - 512.0).abs() < 0.01, "size.y default {v:?}");
            }
            other => panic!("size default should be Vector2, got {other:?}"),
        }
        let update = sv
            .properties
            .iter()
            .find(|p| p.name == "render_target_update_mode")
            .map(|p| &p.default_value)
            .expect("update mode");
        assert_eq!(update, &Variant::Int(2), "UPDATE_WHEN_VISIBLE default");
        let clear = sv
            .properties
            .iter()
            .find(|p| p.name == "render_target_clear_mode")
            .map(|p| &p.default_value)
            .expect("clear mode");
        assert_eq!(clear, &Variant::Int(0), "CLEAR_MODE_ALWAYS default");

        // Instantiation produces SubViewport with defaults applied.
        let inst = instantiate("SubViewport").expect("instantiate SubViewport");
        match inst.get_property("size") {
            Variant::Vector2(v) => {
                assert!((v.x - 512.0).abs() < 0.01);
                assert!((v.y - 512.0).abs() < 0.01);
            }
            other => panic!("inst size {other:?}"),
        }
        assert_eq!(
            inst.get_property("render_target_update_mode"),
            Variant::Int(2)
        );

        // Also registered via register_2d_classes for editor/runtime convenience.
        clear_for_testing();
        register_2d_classes();
        assert!(class_exists("SubViewport"));
        assert!(class_exists("ViewportTexture"));
    }

    /// pat-nzgdu.214: the abstract Texture base plus the layered (Texture2DArray)
    /// and volume (Texture3D) families are registered under Godot's real
    /// inheritance chain, alongside the RenderingDevice-backed wrappers.
    #[test]
    fn texture_base_layered_volume_classes_registered() {
        let _g = setup();
        register_viewport_classes();

        for name in [
            "Texture",
            "Texture2D",
            "TextureLayered",
            "ImageTextureLayered",
            "Texture2DArray",
            "Texture3D",
            "ImageTexture3D",
            "Texture2DRD",
            "TextureLayeredRD",
            "Texture2DArrayRD",
            "TextureCubemapRD",
            "TextureCubemapArrayRD",
            "Texture3DRD",
        ] {
            assert!(class_exists(name), "{name} must be in ClassDB");
        }

        // Texture2D now hangs off the Texture base rather than Resource directly.
        assert_eq!(
            inheritance_chain("Texture2DArray"),
            vec![
                "Texture2DArray",
                "ImageTextureLayered",
                "TextureLayered",
                "Texture",
                "Resource",
                // Resource is a RefCounted, not a direct Object (pat-0q2kx.7).
                "RefCounted",
                "Object",
            ]
        );
        assert!(is_parent_class("Texture2D", "Texture"));
        assert!(is_parent_class("Texture2D", "Resource"));
        assert!(is_parent_class("ImageTexture3D", "Texture3D"));
        assert!(is_parent_class("ImageTexture3D", "Texture"));
        assert!(is_parent_class("Texture2DArrayRD", "TextureLayeredRD"));
        assert!(is_parent_class("TextureCubemapArrayRD", "TextureLayered"));
        assert!(is_parent_class("Texture2DRD", "Texture2D"));
        assert!(is_parent_class("Texture3DRD", "Texture3D"));

        // Reparenting Texture2D must not break the existing ViewportTexture chain.
        assert!(is_parent_class("ViewportTexture", "Texture2D"));
        assert!(is_parent_class("ViewportTexture", "Resource"));

        // ⚠️ Only the ABSTRACT-registered bases are abstract (pat-nzgdu.215).
        // This loop used to include Texture/TextureLayered/Texture3D on the
        // theory that "Godot marks the bases abstract" — it does not. Those
        // three are `GDREGISTER_VIRTUAL_CLASS`, and VIRTUAL is not ABSTRACT:
        // `register_class<T>(true)` still installs a creation func, and Godot's
        // `ClassDB::is_abstract` returns false as soon as one exists
        // (class_db.cpp:843). The dump agrees (`is_instantiable=true`).
        for name in ["ImageTextureLayered", "TextureLayeredRD"] {
            assert!(
                is_abstract(name),
                "{name} is GDREGISTER_ABSTRACT_CLASS in Godot"
            );
        }
        for name in ["Texture", "TextureLayered", "Texture3D"] {
            assert!(
                !is_abstract(name),
                "{name} is GDREGISTER_VIRTUAL_CLASS — virtual, not abstract, so a \
                 script may extend AND construct it"
            );
        }
        assert!(!is_abstract("Texture2DArray"));
        assert!(!is_abstract("ImageTexture3D"));

        // Method surface resolves through the chain.
        assert!(class_has_method("Texture2DArray", "get_layers"));
        assert!(class_has_method("Texture2DArray", "get_layer_data"));
        assert!(class_has_method("Texture2DArray", "create_from_images"));
        assert!(class_has_method("Texture2DArray", "create_placeholder"));
        assert!(class_has_method("Texture3D", "get_depth"));
        assert!(class_has_method("ImageTexture3D", "get_depth"));
        assert!(class_has_method("ImageTexture3D", "create"));
        assert!(class_has_method("ImageTexture3D", "update"));
        assert!(class_has_method("Texture3DRD", "set_texture_rd_rid"));
        assert!(class_has_method("Texture2DArrayRD", "get_texture_rd_rid"));
    }

    /// pat-u3l8c.43: Shader / ShaderMaterial classes with set/get_shader_parameter.
    #[test]
    fn shader_classes_registered_with_parameter_methods() {
        let _g = setup();
        register_3d_classes();

        assert!(class_exists("Shader"), "Shader must be in ClassDB");
        assert!(
            class_exists("ShaderMaterial"),
            "ShaderMaterial must be in ClassDB"
        );
        assert!(class_exists("Material"), "Material must be in ClassDB");
        assert!(class_exists("StandardMaterial3D"));

        // Godot hierarchy: ShaderMaterial → Material → Resource.
        assert!(is_parent_class("ShaderMaterial", "Material"));
        assert!(is_parent_class("ShaderMaterial", "Resource"));
        assert!(is_parent_class("Shader", "Resource"));
        assert!(is_parent_class("StandardMaterial3D", "BaseMaterial3D"));
        assert!(is_parent_class("StandardMaterial3D", "Material"));

        // Material is abstract; ShaderMaterial is instantiable.
        assert!(is_abstract("Material"));
        assert!(can_instantiate("ShaderMaterial"));

        // ShaderMaterial exposes the shader-parameter methods GDScript calls.
        let methods: Vec<String> = get_method_list("ShaderMaterial")
            .into_iter()
            .map(|m| m.name)
            .collect();
        assert!(
            methods.contains(&"set_shader_parameter".to_string()),
            "ShaderMaterial needs set_shader_parameter: {methods:?}"
        );
        assert!(methods.contains(&"get_shader_parameter".to_string()));
        assert!(methods.contains(&"set_shader".to_string()));
        assert!(methods.contains(&"get_shader".to_string()));

        // Shader exposes get_code / set_code / get_mode.
        let shader_methods: Vec<String> = get_method_list("Shader")
            .into_iter()
            .map(|m| m.name)
            .collect();
        assert!(shader_methods.contains(&"get_code".to_string()));
        assert!(shader_methods.contains(&"get_mode".to_string()));
    }
}
