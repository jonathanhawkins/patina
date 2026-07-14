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

    // -- RayCast3D / ShapeCast3D (scene query nodes over PhysicsServer3D) --
    register_class(
        ClassRegistration::new("RayCast3D")
            .parent("Node3D")
            .property(PropertyInfo::new("enabled", Variant::Bool(true)))
            .property(PropertyInfo::new(
                "target_position",
                Variant::Vector3(Vector3::new(0.0, -1.0, 0.0)),
            ))
            .property(PropertyInfo::new("collision_mask", Variant::Int(0xFFFF_FFFF)))
            .property(PropertyInfo::new("exclude_parent", Variant::Bool(true)))
            .property(PropertyInfo::new("collide_with_bodies", Variant::Bool(true)))
            .property(PropertyInfo::new("collide_with_areas", Variant::Bool(false))),
    );
    register_class(
        ClassRegistration::new("ShapeCast3D")
            .parent("Node3D")
            .property(PropertyInfo::new("enabled", Variant::Bool(true)))
            .property(PropertyInfo::new(
                "target_position",
                Variant::Vector3(Vector3::new(0.0, -1.0, 0.0)),
            ))
            .property(PropertyInfo::new("collision_mask", Variant::Int(0xFFFF_FFFF)))
            .property(PropertyInfo::new("exclude_parent", Variant::Bool(true)))
            .property(PropertyInfo::new("collide_with_bodies", Variant::Bool(true)))
            .property(PropertyInfo::new("collide_with_areas", Variant::Bool(false)))
            .property(PropertyInfo::new("max_results", Variant::Int(32))),
    );
    register_class(
        ClassRegistration::new("SpringArm3D")
            .parent("Node3D")
            .property(PropertyInfo::new("spring_length", Variant::Float(1.0)))
            .property(PropertyInfo::new("margin", Variant::Float(0.01)))
            .property(PropertyInfo::new("collision_mask", Variant::Int(0xFFFF_FFFF))),
    );

    // -- Environment / World --
    register_class(ClassRegistration::new("WorldEnvironment").parent("Node3D"));
    register_class(ClassRegistration::new("Marker3D").parent("Node3D"));

    // -- Additional 3D types --
    register_class(ClassRegistration::new("NavigationRegion3D").parent("Node3D"));
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
            .property(PropertyInfo::new(
                "avoidance_enabled",
                Variant::Bool(false),
            ))
            // pat-nzgdu.10 — full NavigationAgent3D property surface (Godot parity).
            .property(PropertyInfo::new("target_desired_distance", Variant::Float(10.0)))
            .property(PropertyInfo::new("path_height_offset", Variant::Float(0.0)))
            .property(PropertyInfo::new("path_max_distance", Variant::Float(5.0)))
            .property(PropertyInfo::new("navigation_layers", Variant::Int(1)))
            .property(PropertyInfo::new("pathfinding_algorithm", Variant::Int(0)))
            .property(PropertyInfo::new("path_postprocessing", Variant::Int(0)))
            .property(PropertyInfo::new("path_metadata_flags", Variant::Int(7)))
            .property(PropertyInfo::new("simplify_path", Variant::Bool(false)))
            .property(PropertyInfo::new("simplify_epsilon", Variant::Float(0.0)))
            .property(PropertyInfo::new("path_return_max_length", Variant::Float(0.0)))
            .property(PropertyInfo::new("path_return_max_radius", Variant::Float(0.0)))
            .property(PropertyInfo::new("path_search_max_polygons", Variant::Int(4096)))
            .property(PropertyInfo::new("path_search_max_distance", Variant::Float(0.0)))
            .property(PropertyInfo::new("velocity", Variant::Vector3(Vector3::ZERO)))
            .property(PropertyInfo::new("height", Variant::Float(1.0)))
            .property(PropertyInfo::new("radius", Variant::Float(0.5)))
            .property(PropertyInfo::new("neighbor_distance", Variant::Float(50.0)))
            .property(PropertyInfo::new("max_neighbors", Variant::Int(10)))
            .property(PropertyInfo::new("time_horizon_agents", Variant::Float(1.0)))
            .property(PropertyInfo::new("time_horizon_obstacles", Variant::Float(0.0)))
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
            .property(PropertyInfo::new("debug_path_custom_point_size", Variant::Float(4.0))),
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
        ClassRegistration::new("VehicleWheel3D")
            .parent("Node3D")
            .property(PropertyInfo::new("engine_force", Variant::Float(0.0)))
            .property(PropertyInfo::new("brake", Variant::Float(0.0)))
            .property(PropertyInfo::new("steering", Variant::Float(0.0)))
            .property(PropertyInfo::new("use_as_traction", Variant::Bool(false)))
            .property(PropertyInfo::new("use_as_steering", Variant::Bool(false))),
    );
    register_class(
        ClassRegistration::new("SoftBody3D")
            .parent("MeshInstance3D")
            .property(PropertyInfo::new("simulation_precision", Variant::Int(5)))
            .property(PropertyInfo::new("total_mass", Variant::Float(1.0)))
            .property(PropertyInfo::new("linear_stiffness", Variant::Float(0.5)))
            .property(PropertyInfo::new("pressure_coefficient", Variant::Float(0.0)))
            .property(PropertyInfo::new("damping_coefficient", Variant::Float(0.01))),
    );
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
    register_class(
        ClassRegistration::new("VisualInstance3D")
            .parent("Node3D")
            .abstract_class(),
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
    if !class_exists("Resource") {
        register_class(ClassRegistration::new("Resource").parent("Object"));
    }
    register_class(ClassRegistration::new("VisualScript").parent("Resource"));

    // Material / Shader hierarchy (Godot: Material → Resource; ShaderMaterial →
    // Material; BaseMaterial3D → Material; StandardMaterial3D → BaseMaterial3D).
    // ShaderMaterial exposes the `shader` property plus set/get_shader and the
    // per-parameter set/get_shader_parameter methods that GDScript calls.
    register_class(
        ClassRegistration::new("Material")
            .parent("Resource")
            .abstract_class(),
    );
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
    register_class(
        ClassRegistration::new("BaseMaterial3D")
            .parent("Material")
            .abstract_class(),
    );
    register_class(ClassRegistration::new("StandardMaterial3D").parent("BaseMaterial3D"));
    register_class(ClassRegistration::new("ORMMaterial3D").parent("BaseMaterial3D"));
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
                .abstract_class()
                .property(PropertyInfo::new("visible", Variant::Bool(true)))
                .property(PropertyInfo::new(
                    "modulate",
                    Variant::Color(gdcore::math::Color::WHITE),
                ))
                .property(PropertyInfo::new("z_index", Variant::Int(0))),
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
    if !class_exists("Polygon2D") {
        register_class(ClassRegistration::new("Polygon2D").parent("Node2D"));
    }
    if !class_exists("Path2D") {
        register_class(ClassRegistration::new("Path2D").parent("Node2D"));
    }
    if !class_exists("PathFollow2D") {
        register_class(ClassRegistration::new("PathFollow2D").parent("Node2D"));
    }
    // 2D lights + occluder (pat-p0s82.6)
    if !class_exists("Path3D") {
        register_class(ClassRegistration::new("Path3D").parent("Node3D"));
    }
    if !class_exists("PathFollow3D") {
        register_class(ClassRegistration::new("PathFollow3D").parent("Node3D"));
    }
    if !class_exists("GridMap") {
        register_class(ClassRegistration::new("GridMap").parent("Node3D"));
    }
    if !class_exists("NavigationLink3D") {
        register_class(ClassRegistration::new("NavigationLink3D").parent("Node3D"));
    }
    if !class_exists("NavigationLink2D") {
        register_class(ClassRegistration::new("NavigationLink2D").parent("Node2D"));
    }
    if !class_exists("RemoteTransform3D") {
        register_class(ClassRegistration::new("RemoteTransform3D").parent("Node3D"));
    }
    if !class_exists("RemoteTransform2D") {
        register_class(ClassRegistration::new("RemoteTransform2D").parent("Node2D"));
    }
    if !class_exists("VisibleOnScreenNotifier3D") {
        register_class(ClassRegistration::new("VisibleOnScreenNotifier3D").parent("Node3D"));
    }
    if !class_exists("Label3D") {
        register_class(ClassRegistration::new("Label3D").parent("Node3D"));
    }
    if !class_exists("RichTextLabel") {
        register_class(ClassRegistration::new("RichTextLabel").parent("Control"));
    }
    if !class_exists("ScrollContainer") {
        register_class(ClassRegistration::new("ScrollContainer").parent("Container"));
    }
    if !class_exists("TabContainer") {
        register_class(ClassRegistration::new("TabContainer").parent("Container"));
    }
    if !class_exists("ProgressBar") {
        register_class(ClassRegistration::new("ProgressBar").parent("Control"));
    }
    if !class_exists("CheckBox") {
        register_class(ClassRegistration::new("CheckBox").parent("Button"));
    }
    if !class_exists("OptionButton") {
        register_class(ClassRegistration::new("OptionButton").parent("Button"));
    }
    if !class_exists("TextureRect") {
        register_class(ClassRegistration::new("TextureRect").parent("Control"));
    }
    if !class_exists("GridContainer") {
        register_class(ClassRegistration::new("GridContainer").parent("Container"));
    }
    if !class_exists("MarginContainer") {
        register_class(ClassRegistration::new("MarginContainer").parent("Container"));
    }
    if !class_exists("AudioEffectAmplify") {
        register_class(ClassRegistration::new("AudioEffectAmplify").parent("Resource"));
    }
    if !class_exists("AudioEffectReverb") {
        register_class(ClassRegistration::new("AudioEffectReverb").parent("Resource"));
    }
    if !class_exists("AudioEffectEQ") {
        register_class(ClassRegistration::new("AudioEffectEQ").parent("Resource"));
    }
    
    if !class_exists("Light2D") {
        register_class(ClassRegistration::new("Light2D").parent("Node2D"));
    }
    if !class_exists("PointLight2D") {
        register_class(ClassRegistration::new("PointLight2D").parent("Light2D"));
    }
    if !class_exists("DirectionalLight2D") {
        register_class(ClassRegistration::new("DirectionalLight2D").parent("Light2D"));
    }
    if !class_exists("LightOccluder2D") {
        register_class(ClassRegistration::new("LightOccluder2D").parent("Node2D"));
    }
    if !class_exists("NavigationRegion2D") {
        register_class(ClassRegistration::new("NavigationRegion2D").parent("Node2D"));
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
                .property(PropertyInfo::new(
                    "avoidance_enabled",
                    Variant::Bool(false),
                ))
                // pat-nzgdu.10 — full NavigationAgent2D property surface (Godot parity).
                .property(PropertyInfo::new("target_desired_distance", Variant::Float(10.0)))
                .property(PropertyInfo::new("path_max_distance", Variant::Float(100.0)))
                .property(PropertyInfo::new("navigation_layers", Variant::Int(1)))
                .property(PropertyInfo::new("pathfinding_algorithm", Variant::Int(0)))
                .property(PropertyInfo::new("path_postprocessing", Variant::Int(0)))
                .property(PropertyInfo::new("path_metadata_flags", Variant::Int(7)))
                .property(PropertyInfo::new("simplify_path", Variant::Bool(false)))
                .property(PropertyInfo::new("simplify_epsilon", Variant::Float(0.0)))
                .property(PropertyInfo::new("path_return_max_length", Variant::Float(0.0)))
                .property(PropertyInfo::new("path_return_max_radius", Variant::Float(0.0)))
                .property(PropertyInfo::new("path_search_max_polygons", Variant::Int(4096)))
                .property(PropertyInfo::new("path_search_max_distance", Variant::Float(0.0)))
                .property(PropertyInfo::new(
                    "velocity",
                    Variant::Vector2(gdcore::math::Vector2::ZERO),
                ))
                .property(PropertyInfo::new("radius", Variant::Float(10.0)))
                .property(PropertyInfo::new("neighbor_distance", Variant::Float(500.0)))
                .property(PropertyInfo::new("max_neighbors", Variant::Int(10)))
                .property(PropertyInfo::new("time_horizon_agents", Variant::Float(1.0)))
                .property(PropertyInfo::new("time_horizon_obstacles", Variant::Float(0.0)))
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
                .property(PropertyInfo::new("debug_path_custom_point_size", Variant::Float(4.0)))
                .property(PropertyInfo::new("debug_path_custom_line_width", Variant::Float(-1.0))),
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
    if !class_exists("ColorRect") {
        register_class(ClassRegistration::new("ColorRect").parent("Control"));
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
pub fn register_viewport_classes() {
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

    if !class_exists("Texture2D") {
        register_class(
            ClassRegistration::new("Texture2D")
                .parent("Resource")
                .method(MethodInfo::new("get_width", 0))
                .method(MethodInfo::new("get_height", 0))
                .method(MethodInfo::new("get_size", 0)),
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
        register_class(ClassRegistration::new("Object"));
    }
    if !class_exists("Node") {
        register_class(ClassRegistration::new("Node").parent("Object"));
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
                .method(MethodInfo::new("play", 1))
                .method(MethodInfo::new("stop", 0))
                .method(MethodInfo::new("is_playing", 0)),
        );
    }
}

/// Registers core IO utility classes matching Godot's `FileAccess` / `DirAccess`.
///
/// * `FileAccess : RefCounted` (core/io/file_access)
/// * `DirAccess : RefCounted` (core/io/dir_access)
///
/// Idempotent — each class is only registered if not already present.
pub fn register_io_classes() {
    if !class_exists("Object") {
        register_class(ClassRegistration::new("Object"));
    }
    if !class_exists("RefCounted") {
        register_class(ClassRegistration::new("RefCounted").parent("Object"));
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
}

/// * `HTTPRequest : Node` (scene/main/http_request)
/// * `HTTPClient : RefCounted` (core/io/http_client)
/// * `WebSocketPeer : PacketPeer : RefCounted` (modules/websocket)
///
/// Idempotent — each class is only registered if not already present.
pub fn register_networking_classes() {
    // Ensure the base chain exists (Object → RefCounted → PacketPeer, and Node).
    if !class_exists("Object") {
        register_class(ClassRegistration::new("Object"));
    }
    if !class_exists("RefCounted") {
        register_class(ClassRegistration::new("RefCounted").parent("Object"));
    }
    if !class_exists("Node") {
        register_class(ClassRegistration::new("Node").parent("Object"));
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
    if !class_exists("MultiplayerAPI") {
        register_class(
            ClassRegistration::new("MultiplayerAPI")
                .parent("RefCounted")
                .method(MethodInfo::new("set_multiplayer_peer", 1))
                .method(MethodInfo::new("is_server", 0)),
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
        register_class(ClassRegistration::new("Object"));
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
}

pub fn register_editor_classes() {
    register_2d_classes();
    register_3d_classes();
    register_audio_classes();
    register_io_classes();
    register_networking_classes();
    register_navigation_classes();

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
        assert!(class_has_property("NavigationAgent3D", "path_height_offset"));
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
        assert!(class_exists("SubViewport"), "SubViewport must be in ClassDB");
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
