//! # gdcore
//!
//! Low-level engine primitives, IDs, allocation helpers, and diagnostics
//! for the Patina Engine runtime.

#![warn(clippy::all)]
#![warn(missing_docs)]

pub mod bench_regression;
pub mod compare3d;
pub mod comparison_tooling;
pub mod crash_triage;
pub mod dashboard;
pub mod debug_protocol;
pub mod debugger;
pub mod diagnostics;
pub mod error;
pub mod id;
pub mod math;
pub mod math3d;
pub mod memory_profiler;
pub mod nightly_ci;
pub mod node_path;
pub mod perf_comparison;
pub mod property_testing;
pub mod regex;
pub mod release_notes;
pub mod release_train;
pub mod reproducible_build;
pub mod string_name;

// Re-export commonly used types at the crate root.
pub use error::{EngineError, EngineResult};
pub use id::{ClassId, ObjectId, ResourceUid};
pub use math::{Color, Rect2, Transform2D, Vector2, Vector2i, Vector3};
pub use math3d::{Aabb, Basis, Plane, Quaternion, Transform3D};
pub use node_path::NodePath;
pub use string_name::StringName;
