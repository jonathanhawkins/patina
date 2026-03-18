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
pub mod parser;
pub mod tokenizer;

pub use bindings::{MethodFlags, MethodInfo, ScriptError, ScriptInstance, ScriptPropertyInfo};
pub use bridge::{NativeScript, NativeScriptBuilder, ScriptBridge};
pub use parser::{AssignOp, BinOp, Expr, ParseError, Parser, Stmt, UnaryOp};
pub use tokenizer::{tokenize, LexError, Token, TokenSpan};
