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
    /// Optional property hint value (matches Godot's PropertyHint enum).
    /// 0 means no hint (default).
    pub hint: i32,
}

impl PropertyInfo {
    /// Creates a new property info entry with no hint.
    pub fn new(name: impl Into<String>, default_value: Variant) -> Self {
        Self {
            name: name.into(),
            default_value,
            hint: 0,
        }
    }

    /// Sets the property hint value (builder pattern).
    pub fn with_hint(mut self, hint: i32) -> Self {
        self.hint = hint;
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
pub fn register_3d_classes() {
    use gdcore::math::Vector3;
    use gdcore::math3d::{Basis, Transform3D};

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
            .property(PropertyInfo::new(
                "scale",
                Variant::Vector3(Vector3::ONE),
            )),
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

    // -- MeshInstance3D --
    register_class(
        ClassRegistration::new("MeshInstance3D")
            .parent("Node3D")
            .property(PropertyInfo::new("cast_shadow", Variant::Int(1))),
    );

    // -- MultiMeshInstance3D --
    register_class(ClassRegistration::new("MultiMeshInstance3D").parent("Node3D"));

    // -- Light3D (abstract base for all lights) --
    register_class(
        ClassRegistration::new("Light3D")
            .parent("Node3D")
            .property(PropertyInfo::new("light_energy", Variant::Float(1.0)))
            .property(PropertyInfo::new(
                "light_color",
                Variant::Color(gdcore::math::Color::WHITE),
            ))
            .property(PropertyInfo::new("shadow_enabled", Variant::Bool(false)).with_hint(42))
            .property(PropertyInfo::new("shadow_bias", Variant::Float(0.1)))
            .property(PropertyInfo::new("shadow_blur", Variant::Float(1.0)))
            .property(PropertyInfo::new(
                "shadow_normal_bias",
                Variant::Float(2.0),
            ))
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
            .property(PropertyInfo::new("collision_layer", Variant::Int(1)))
            .property(PropertyInfo::new("collision_mask", Variant::Int(1))),
    );

    register_class(
        ClassRegistration::new("PhysicsBody3D").parent("CollisionObject3D"),
    );

    register_class(
        ClassRegistration::new("StaticBody3D").parent("PhysicsBody3D"),
    );

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
            .property(PropertyInfo::new("floor_max_angle", Variant::Float(0.785398))),
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

    // -- Environment / World --
    register_class(ClassRegistration::new("WorldEnvironment").parent("Node3D"));
    register_class(ClassRegistration::new("Marker3D").parent("Node3D"));

    // -- Additional 3D types --
    register_class(ClassRegistration::new("NavigationRegion3D").parent("Node3D"));
    register_class(ClassRegistration::new("Skeleton3D").parent("Node3D"));
    register_class(ClassRegistration::new("BoneAttachment3D").parent("Node3D"));
    register_class(ClassRegistration::new("AnimationPlayer").parent("Node"));
    register_class(ClassRegistration::new("AnimationTree").parent("Node"));
    register_class(ClassRegistration::new("GPUParticles3D").parent("Node3D"));
    register_class(ClassRegistration::new("CPUParticles3D").parent("Node3D"));
    register_class(ClassRegistration::new("Decal").parent("Node3D"));
    register_class(ClassRegistration::new("FogVolume").parent("Node3D"));
    register_class(ClassRegistration::new("ReflectionProbe").parent("Node3D"));
    register_class(ClassRegistration::new("VoxelGI").parent("Node3D"));
    register_class(ClassRegistration::new("LightmapGI").parent("Node3D"));
    register_class(ClassRegistration::new("VisualInstance3D").parent("Node3D"));
    register_class(ClassRegistration::new("CSGShape3D").parent("Node3D"));
    register_class(ClassRegistration::new("CSGBox3D").parent("CSGShape3D"));
    register_class(ClassRegistration::new("CSGSphere3D").parent("CSGShape3D"));
    register_class(ClassRegistration::new("CSGCylinder3D").parent("CSGShape3D"));
    register_class(ClassRegistration::new("CSGCombiner3D").parent("CSGShape3D"));
    // VisualScript is deprecated but inherits Script → Resource in Godot's hierarchy.
    if !class_exists("Resource") {
        register_class(ClassRegistration::new("Resource").parent("Object"));
    }
    register_class(ClassRegistration::new("VisualScript").parent("Resource"));
}

/// Registers the standard Godot 2D class hierarchy in the ClassDB.
///
/// This includes the CanvasItem → Node2D chain, common 2D nodes (Sprite2D,
/// AnimatedSprite2D, etc.), Control UI classes, and 2D physics bodies.
pub fn register_2d_classes() {
    use gdcore::math::Vector2;

    if !class_exists("Object") {
        register_class(ClassRegistration::new("Object"));
    }
    if !class_exists("Node") {
        register_class(ClassRegistration::new("Node").parent("Object"));
    }
    if !class_exists("Resource") {
        register_class(ClassRegistration::new("Resource").parent("Object"));
    }

    // -- CanvasItem (base for all 2D & UI nodes) --
    if !class_exists("CanvasItem") {
        register_class(
            ClassRegistration::new("CanvasItem")
                .parent("Node")
                .property(PropertyInfo::new("visible", Variant::Bool(true)))
                .property(PropertyInfo::new("modulate", Variant::Color(gdcore::math::Color::WHITE)))
                .property(PropertyInfo::new("z_index", Variant::Int(0))),
        );
    }

    // -- Node2D --
    if !class_exists("Node2D") {
        register_class(
            ClassRegistration::new("Node2D")
                .parent("CanvasItem")
                .property(PropertyInfo::new("position", Variant::Vector2(Vector2::ZERO)))
                .property(PropertyInfo::new("rotation", Variant::Float(0.0)))
                .property(PropertyInfo::new("scale", Variant::Vector2(Vector2::ONE))),
        );
    }

    // -- Common 2D nodes --
    if !class_exists("Sprite2D") {
        register_class(ClassRegistration::new("Sprite2D").parent("Node2D"));
    }
    if !class_exists("AnimatedSprite2D") {
        register_class(ClassRegistration::new("AnimatedSprite2D").parent("Node2D"));
    }
    if !class_exists("Camera2D") {
        register_class(ClassRegistration::new("Camera2D").parent("Node2D"));
    }
    if !class_exists("TileMapLayer") {
        register_class(ClassRegistration::new("TileMapLayer").parent("Node2D"));
    }
    if !class_exists("GPUParticles2D") {
        register_class(ClassRegistration::new("GPUParticles2D").parent("Node2D"));
    }
    if !class_exists("CPUParticles2D") {
        register_class(ClassRegistration::new("CPUParticles2D").parent("Node2D"));
    }
    if !class_exists("Line2D") {
        register_class(ClassRegistration::new("Line2D").parent("Node2D"));
    }
    if !class_exists("Path2D") {
        register_class(ClassRegistration::new("Path2D").parent("Node2D"));
    }
    if !class_exists("PathFollow2D") {
        register_class(ClassRegistration::new("PathFollow2D").parent("Node2D"));
    }
    if !class_exists("NavigationRegion2D") {
        register_class(ClassRegistration::new("NavigationRegion2D").parent("Node2D"));
    }

    // -- Control (UI) hierarchy --
    if !class_exists("Control") {
        register_class(
            ClassRegistration::new("Control")
                .parent("CanvasItem")
                .property(PropertyInfo::new("anchor_left", Variant::Float(0.0)))
                .property(PropertyInfo::new("anchor_top", Variant::Float(0.0)))
                .property(PropertyInfo::new("anchor_right", Variant::Float(0.0)))
                .property(PropertyInfo::new("anchor_bottom", Variant::Float(0.0))),
        );
    }
    if !class_exists("Button") {
        register_class(ClassRegistration::new("Button").parent("Control"));
    }
    if !class_exists("Label") {
        register_class(ClassRegistration::new("Label").parent("Control"));
    }
    if !class_exists("LineEdit") {
        register_class(ClassRegistration::new("LineEdit").parent("Control"));
    }
    if !class_exists("TextEdit") {
        register_class(ClassRegistration::new("TextEdit").parent("Control"));
    }
    if !class_exists("Panel") {
        register_class(ClassRegistration::new("Panel").parent("Control"));
    }
    if !class_exists("Container") {
        register_class(ClassRegistration::new("Container").parent("Control"));
    }
    if !class_exists("HBoxContainer") {
        register_class(ClassRegistration::new("HBoxContainer").parent("Container"));
    }
    if !class_exists("VBoxContainer") {
        register_class(ClassRegistration::new("VBoxContainer").parent("Container"));
    }

    // -- Physics 2D bodies --
    if !class_exists("CollisionObject2D") {
        register_class(
            ClassRegistration::new("CollisionObject2D")
                .parent("Node2D")
                .property(PropertyInfo::new("collision_layer", Variant::Int(1)))
                .property(PropertyInfo::new("collision_mask", Variant::Int(1))),
        );
    }
    if !class_exists("PhysicsBody2D") {
        register_class(ClassRegistration::new("PhysicsBody2D").parent("CollisionObject2D"));
    }
    if !class_exists("StaticBody2D") {
        register_class(ClassRegistration::new("StaticBody2D").parent("PhysicsBody2D"));
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
                .property(PropertyInfo::new("velocity", Variant::Vector2(Vector2::ZERO)))
                .property(PropertyInfo::new("floor_max_angle", Variant::Float(0.785398))),
        );
    }
    if !class_exists("Area2D") {
        register_class(
            ClassRegistration::new("Area2D")
                .parent("CollisionObject2D")
                .property(PropertyInfo::new("monitoring", Variant::Bool(true)))
                .property(PropertyInfo::new("monitorable", Variant::Bool(true))),
        );
    }
    if !class_exists("CollisionShape2D") {
        register_class(
            ClassRegistration::new("CollisionShape2D")
                .parent("Node2D")
                .property(PropertyInfo::new("disabled", Variant::Bool(false))),
        );
    }
}

/// Registers all core, 2D, and 3D classes for the editor.
///
/// Convenience function that ensures the full class hierarchy is available.
/// Safe to call multiple times — each class is only registered if not already present.
pub fn register_editor_classes() {
    register_2d_classes();
    register_3d_classes();

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
}
