//! # gdscript-interop
//!
//! Scripting interop layer for the Patina Engine runtime.
//!
//! This crate provides the bridge between native engine objects and scripting
//! backends. It defines the `ScriptInstance` trait that every scripting runtime
//! must implement, and `ScriptBridge` which maps engine objects to their
//! attached scripts.

pub mod autoload_registry;
pub mod bindings;
pub mod bridge;
pub mod coroutine;
pub mod fuzz_gdscript;
pub mod global_constants;
pub mod interpreter;
pub mod parser;
pub mod tokenizer;
pub mod type_checker;
/// Script-facing dispatch for the affine/math builtin Variant types.
pub mod variant_math;
pub mod visual_script;

pub use coroutine::{AwaitTarget, CoroutineHost, CoroutineId, SuspendedCoroutine};

pub use bindings::{
    MethodFlags, MethodInfo, SceneAccess, ScriptError, ScriptInstance, ScriptPropertyInfo,
};
pub use bridge::{NativeScript, NativeScriptBuilder, ScriptBridge};
pub use interpreter::{
    engine_max_fps, engine_max_physics_steps_per_frame, engine_physics_frames,
    engine_physics_jitter_fix, engine_physics_ticks_per_second,
    engine_physics_ticks_per_second_generation, engine_process_frames, engine_time_scale,
    publish_engine_physics_ticks_per_second, reset_engine_singleton_state, set_engine_editor_hint,
    set_engine_frame_counters, set_engine_frames_per_second, set_engine_in_physics_frame,
    set_engine_max_fps, set_engine_max_physics_steps_per_frame, set_engine_physics_jitter_fix,
    set_engine_physics_ticks_per_second, set_engine_time_scale, RuntimeError, RuntimeErrorKind,
    ScriptWarning, SourceLocation, StackFrame,
};
pub use parser::{
    Annotation, AssignOp, BinOp, Expr, FuncParam, MatchArm, MatchPattern, ParseError, Parser, Stmt,
    UnaryOp,
};
pub use tokenizer::{tokenize, LexError, Token, TokenSpan};
pub use visual_script::VisualScriptStub;
