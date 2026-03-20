//! # gdvariant
//!
//! Variant type system, conversion rules, typed value containers, and
//! serialization for the Patina Engine runtime.

#![warn(clippy::all)]

pub mod conversion;
pub mod serialize;
pub mod variant;

// Re-export the most-used types at the crate root.
pub use conversion::ConversionError;
pub use variant::{CallableRef, ResourceRef, Variant, VariantType};
