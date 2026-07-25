//! # gdcore
//!
//! Low-level engine primitives, IDs, allocation helpers, and diagnostics
//! for the Patina Engine runtime.

#![warn(clippy::all)]
#![warn(missing_docs)]

pub mod bench_regression;
pub mod compare3d;
pub mod comparison_tooling;
pub mod content_scale;
pub mod crash_triage;
pub mod crypto;
pub mod dashboard;
pub mod debug_protocol;
pub mod debugger;
pub mod diagnostics;
pub mod display_backend;
pub mod error;
pub mod file_access;
pub mod gdextension_host;
pub mod geometry3d;
pub mod id;
pub mod math;
pub mod math3d;
pub mod memory_profiler;
pub mod nightly_ci;
pub mod node_path;
pub mod perf_comparison;
pub mod project_settings;
pub mod property_testing;
pub mod regex;
pub mod release_notes;
pub mod release_train;
pub mod reproducible_build;
pub mod rng;
pub mod string_name;
pub mod translation;
pub mod xml_parser;

// Re-export commonly used types at the crate root.
pub use error::{EngineError, EngineResult};
pub use file_access::{
    is_dir_access_handle, is_file_access_handle, project_root, resolve_path, set_path_roots,
    user_root, DirAccess, FileAccess, FileMode,
};
pub use id::{ClassId, ObjectId, ResourceUid};
pub use math::{Color, Rect2, Transform2D, Vector2, Vector2i, Vector3, Vector3i};
pub use math3d::{Aabb, Basis, Plane, Quaternion, Transform3D};
pub use node_path::NodePath;
pub use project_settings::{ProjectSettings, SettingsValue};
pub use string_name::StringName;
pub use translation::{parse_csv, parse_po, Translation, TranslationServer};
pub use xml_parser::{XmlNodeType, XmlParser};
