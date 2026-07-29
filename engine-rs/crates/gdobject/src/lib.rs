//! # gdobject
//!
//! Object model, inheritance metadata, signals, notifications, and
//! reference counting for the Patina Engine runtime.
//!
//! This crate sits between `gdvariant` (the type system) and `gdscene`
//! (the scene tree). It provides:
//!
//! - **Object model** (`object`): `GodotObject` trait, `ObjectBase` struct,
//!   property storage, and `GenericObject` for dynamic instances.
//! - **Signal system** (`signal`): One-to-many observer pattern with
//!   ordered connections, matching Godot's signal semantics.
//! - **Notification dispatch** (`notification`): Godot-compatible
//!   notification constants and inheritance-chain dispatch.
//! - **Class registry** (`class_db`): Global class database for metadata
//!   lookup, inheritance queries, and instance creation.

#![warn(clippy::all)]

pub mod class_db;
pub mod editor_plugin_classes;
pub mod movie_writer;
pub mod notification;
pub mod object;
pub mod ref_counted;
/// Script-reachable `RefCounted` instances addressed by `ObjectId`
/// (pat-0q2kx.163). `ref_counted` supplies the counting semantics; this owns
/// the instances so GDScript can name one.
pub mod refcounted_handle;
pub mod signal;
pub mod weak_ref;

// Re-export the most-used types at the crate root.
pub use class_db::{
    class_count, class_exists, class_get_integer_constant, class_has_enum, class_has_method,
    class_has_signal, clear_for_testing, get_class_info, get_class_info_by_id, get_enum_constants,
    get_enum_list, get_integer_constant_list, get_method_list, get_property_list, get_signal_list,
    inheritance_chain, instantiate, is_parent_class, register_2d_classes, register_3d_classes,
    register_audio_classes, register_class, register_editor_classes, register_io_classes,
    register_navigation_classes, register_networking_classes, register_viewport_classes, ClassInfo,
    ClassRegistration, EnumInfo, MethodInfo, PropertyInfo, SignalInfo,
};
pub use notification::{
    dispatch_notification_chain, Notification, NotificationHandler, NotificationRecord,
    NOTIFICATION_CHILD_ORDER_CHANGED, NOTIFICATION_DRAW, NOTIFICATION_ENTER_TREE,
    NOTIFICATION_EXIT_TREE, NOTIFICATION_INSTANCED, NOTIFICATION_INTERNAL_PHYSICS_PROCESS,
    NOTIFICATION_INTERNAL_PROCESS, NOTIFICATION_MOVED_IN_PARENT, NOTIFICATION_PARENTED,
    NOTIFICATION_PAUSED, NOTIFICATION_PHYSICS_PROCESS, NOTIFICATION_POSTINITIALIZE,
    NOTIFICATION_PREDELETE, NOTIFICATION_PROCESS, NOTIFICATION_READY, NOTIFICATION_UNPARENTED,
    NOTIFICATION_UNPAUSED,
};
pub use object::{GenericObject, GodotObject, ObjectBase};
pub use ref_counted::{RefCounted, RefCountedBase};
pub use signal::{Connection, DeferredCall, Signal, SignalEmitter, SignalStore};
