//! # gdscript-interop
//!
//! Scripting interop layer for the Patina Engine runtime.
//!
//! This crate provides the bridge between native engine objects and scripting
//! backends. It defines the `ScriptInstance` trait that every scripting runtime
//! must implement, and `ScriptBridge` which maps engine objects to their
//! attached scripts.

pub mod bindings;
pub mod bridge;

pub use bindings::{
    MethodFlags, MethodInfo, ScriptError, ScriptInstance, ScriptPropertyInfo,
};
pub use bridge::{NativeScript, NativeScriptBuilder, ScriptBridge};
