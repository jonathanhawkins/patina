//! # gdvariant
//!
//! Variant type system, conversion rules, typed value containers, and
//! serialization for the Patina Engine runtime.

#![warn(clippy::all)]

pub mod binary;
pub mod conversion;
pub mod dictionary;
// Godot-identical `Variant::hash()` (pat-0q2kx.230). Lives here rather than in
// the interpreter because it is a Variant operation, and the interpreter's copy
// used Rust's SipHash and matched nothing.
#[cfg(test)]
mod fuzz_variant;
pub mod hash;
pub mod serialize;
// pat-s3rrv.8: Godot's Array/Dictionary are refcounted handles, not values
// ("Arrays are always passed by reference" — doc/classes/Array.xml). These are
// the shared-storage types `Variant::Array`/`Variant::Dictionary` will hold.
pub mod shared;
// pat-0q2kx.225: the project-wide global shader parameter store. Lives here
// because gdserver3d (shader resolve) and gdscript-interop (the RenderingServer
// script surface) must share it, and gdvariant is their only common dependency
// that has Variant.
pub mod shader_globals;
pub mod variant;

// Re-export the most-used types at the crate root.
pub use binary::{decode_variant, encode_variant, VariantDecodeError, VariantEncodeError};
pub use conversion::ConversionError;
pub use dictionary::Dictionary;
pub use shared::{ContainerError, ContainerType, SharedArray, SharedDictionary};
pub use variant::{CallableRef, ResourceRef, Variant, VariantType};
