//! # gdcore
//!
//! Low-level engine primitives, IDs, allocation helpers, and diagnostics
//! for the Patina Engine runtime.

#![warn(clippy::all)]

pub mod diagnostics;
pub mod error;
pub mod id;
pub mod math;
pub mod node_path;
pub mod string_name;

// Re-export commonly used types at the crate root.
pub use error::{EngineError, EngineResult};
pub use id::{ClassId, ObjectId, ResourceUid};
pub use math::{Color, Rect2, Transform2D, Vector2, Vector3};
pub use node_path::NodePath;
pub use string_name::StringName;
