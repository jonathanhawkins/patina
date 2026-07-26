//! GDScript tree-walk interpreter.
//!
//! Evaluates a parsed GDScript AST, maintaining an environment of scoped
//! variables and a registry of user-defined functions. Built-in functions
//! (print, str, int, float, len, range, typeof) are provided out of the box.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use gdvariant::{CallableRef, Dictionary, ResourceRef, Variant};

use crate::bindings::{
    MethodFlags, MethodInfo, SceneAccess, ScriptError, ScriptInstance, ScriptPropertyInfo,
};
use crate::parser::{
    Annotation, AssignOp, BinOp, Expr, FuncParam, MatchPattern, Parser, Stmt, UnaryOp,
};
use crate::tokenizer::tokenize;

/// Maximum call-stack depth before we bail out.
const MAX_RECURSION_DEPTH: usize = 64;

/// Remaining native stack (bytes) below which script calls grow the stack
/// instead of risking overflow — debug-build interpreter frames are large
/// enough that `MAX_RECURSION_DEPTH` script frames can exceed the default
/// thread stack.
const STACK_RED_ZONE: usize = 128 * 1024;

/// How much native stack to allocate when growing.
const STACK_GROWTH: usize = 4 * 1024 * 1024;

/// Accumulates a `Vector2` delta onto a node property used as a script→physics
/// bridge (pat-nzgdu.14). RigidBody2D force/impulse methods have no direct
/// handle to the `PhysicsServer`, so they accumulate onto `_pending_*` /
/// `constant_*` node properties that `PhysicsServer::sync_to_physics` drains
/// into the world body each physics tick.
fn accumulate_vec2_property(
    access: &mut dyn SceneAccess,
    id: u64,
    prop: &str,
    delta: gdcore::math::Vector2,
) {
    let cur = match access.get_node_property(id, prop) {
        Variant::Vector2(v) => v,
        _ => gdcore::math::Vector2::ZERO,
    };
    access.set_node_property(id, prop, Variant::Vector2(cur + delta));
}

/// Accumulates a scalar delta onto a float node property (see
/// [`accumulate_vec2_property`]).
fn accumulate_float_property(access: &mut dyn SceneAccess, id: u64, prop: &str, delta: f64) {
    let cur = match access.get_node_property(id, prop) {
        Variant::Float(f) => f,
        Variant::Int(i) => i as f64,
        _ => 0.0,
    };
    access.set_node_property(id, prop, Variant::Float(cur + delta));
}

/// Extracts a `Vector2` method argument (defaulting to zero).
fn vec2_arg(args: &[Variant], idx: usize) -> gdcore::math::Vector2 {
    match args.get(idx) {
        Some(Variant::Vector2(v)) => *v,
        _ => gdcore::math::Vector2::ZERO,
    }
}

/// Extracts a scalar method argument (Float or Int, defaulting to zero).
fn float_arg(args: &[Variant], idx: usize) -> f64 {
    match args.get(idx) {
        Some(Variant::Float(f)) => *f,
        Some(Variant::Int(i)) => *i as f64,
        _ => 0.0,
    }
}

/// Resolves a Godot resource path to a filesystem path for loading.
///
/// Falls back to a project-root-relative path when no `res://` root is
/// configured, matching how the scene accessor resolves `res://` when it
/// loads a `.tscn` from disk.
fn resource_fs_path(path: &str) -> std::path::PathBuf {
    gdcore::file_access::resolve_path(path)
        .unwrap_or_else(|_| std::path::PathBuf::from(path.strip_prefix("res://").unwrap_or(path)))
}

/// Parses a text resource (`.tres`/`.res`) into a [`ResourceRef`] carrying the
/// class name and properties declared in the file.
///
/// Returns `None` when the file is missing or does not parse, leaving the
/// caller to fall back to a path-only placeholder.
fn load_text_resource(path: &str) -> Option<ResourceRef> {
    let source = std::fs::read_to_string(resource_fs_path(path)).ok()?;
    let resource = gdresource::TresLoader::new()
        .parse_str(&source, path)
        .ok()?;
    Some(ResourceRef {
        path: path.to_string(),
        class_name: resource.class_name.clone(),
        properties: resource
            .properties()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    })
}

/// Source location for error reporting.
#[derive(Debug, Clone)]
pub struct SourceLocation {
    pub line: usize,
    pub column: usize,
    pub source_line: String,
}

impl fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}, column {}", self.line, self.column)?;
        if !self.source_line.is_empty() {
            write!(f, "\n  | {}", self.source_line)?;
            if self.column > 0 {
                write!(f, "\n  | {}^", " ".repeat(self.column - 1))?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct StackFrame {
    pub function_name: String,
    pub source_location: Option<SourceLocation>,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum RuntimeErrorKind {
    #[error("undefined variable: '{0}'")]
    UndefinedVariable(String),
    #[error("type error: {0}")]
    TypeError(String),
    #[error("division by zero")]
    DivisionByZero,
    #[error("undefined function: '{0}'")]
    UndefinedFunction(String),
    #[error("index out of bounds: {index} (length {length})")]
    IndexOutOfBounds { index: i64, length: usize },
    #[error("maximum recursion depth exceeded ({0})")]
    MaxRecursionDepth(usize),
    #[error("parse error: {0}")]
    ParseError(String),
    #[error("lex error: {0}")]
    LexError(String),
    #[error("assertion failed: {0}")]
    AssertionFailed(String),
}

#[derive(Debug, Clone)]
pub struct RuntimeError {
    pub kind: RuntimeErrorKind,
    pub source_location: Option<SourceLocation>,
    pub call_stack: Vec<StackFrame>,
}

impl RuntimeError {
    pub fn new(kind: RuntimeErrorKind) -> Self {
        Self {
            kind,
            source_location: None,
            call_stack: Vec::new(),
        }
    }
    pub fn with_location(mut self, loc: SourceLocation) -> Self {
        self.source_location = Some(loc);
        self
    }
    pub fn with_call_stack(mut self, stack: Vec<StackFrame>) -> Self {
        self.call_stack = stack;
        self
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)?;
        if let Some(ref loc) = self.source_location {
            write!(f, " at {loc}")?;
        }
        if !self.call_stack.is_empty() {
            write!(f, "\nCall stack:")?;
            for frame in self.call_stack.iter().rev() {
                write!(f, "\n  in {}", frame.function_name)?;
                if let Some(ref loc) = frame.source_location {
                    write!(f, " (line {})", loc.line)?;
                }
            }
        }
        Ok(())
    }
}

impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.kind)
    }
}

#[derive(Debug, Clone)]
pub enum ScriptWarning {
    UnusedVariable {
        name: String,
        location: SourceLocation,
    },
    ShadowedVariable {
        name: String,
        location: SourceLocation,
    },
    UnreachableCode {
        location: SourceLocation,
    },
}

impl fmt::Display for ScriptWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScriptWarning::UnusedVariable { name, location } => {
                write!(f, "warning: unused variable '{name}' at {location}")
            }
            ScriptWarning::ShadowedVariable { name, location } => {
                write!(f, "warning: variable '{name}' shadows outer at {location}")
            }
            ScriptWarning::UnreachableCode { location } => {
                write!(f, "warning: unreachable code at {location}")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------

/// A stack of lexical scopes mapping names to values.
#[derive(Debug, Clone)]
struct Environment {
    scopes: Vec<HashMap<String, Variant>>,
    /// Parallel to `scopes`: names declared with `const` in each scope, which
    /// may not be reassigned.
    consts: Vec<std::collections::HashSet<String>>,
}

impl Environment {
    fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            consts: vec![std::collections::HashSet::new()],
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
        self.consts.push(std::collections::HashSet::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
        self.consts.pop();
    }

    /// Define a new variable in the current (innermost) scope.
    fn define(&mut self, name: String, value: Variant) {
        self.scopes.last_mut().unwrap().insert(name, value);
    }

    /// Define a constant in the current scope; reassigning it later is an error.
    fn define_const(&mut self, name: String, value: Variant) {
        self.consts.last_mut().unwrap().insert(name.clone());
        self.scopes.last_mut().unwrap().insert(name, value);
    }

    /// Look up a variable by walking scopes from innermost to outermost.
    fn get(&self, name: &str) -> Result<Variant, RuntimeError> {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(name) {
                return Ok(v.clone());
            }
        }
        Err(RuntimeError::new(RuntimeErrorKind::UndefinedVariable(
            name.to_string(),
        )))
    }

    /// Set an existing variable. Searches scopes from inner to outer.
    fn set(&mut self, name: &str, value: Variant) -> Result<(), RuntimeError> {
        for i in (0..self.scopes.len()).rev() {
            if self.scopes[i].contains_key(name) {
                if self.consts[i].contains(name) {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                        "cannot assign to constant '{name}'"
                    ))));
                }
                self.scopes[i].insert(name.to_string(), value);
                return Ok(());
            }
        }
        Err(RuntimeError::new(RuntimeErrorKind::UndefinedVariable(
            name.to_string(),
        )))
    }

    /// Returns true if the name exists in a scope strictly inner to the outermost
    /// scope that also contains it. Used to detect when a local `var` shadows an
    /// instance property that was defined in an outer scope.
    fn is_shadowed_in_inner_scope(&self, name: &str) -> bool {
        let mut found_outer = false;
        // Walk from outer to inner; if we find it twice, the inner one shadows.
        for scope in &self.scopes {
            if scope.contains_key(name) {
                if found_outer {
                    return true;
                }
                found_outer = true;
            }
        }
        false
    }

    #[allow(dead_code)]
    fn exists_in_outer_scope(&self, name: &str) -> bool {
        if self.scopes.len() < 2 {
            return false;
        }
        for scope in self.scopes[..self.scopes.len() - 1].iter().rev() {
            if scope.contains_key(name) {
                return true;
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// Control flow signals
// ---------------------------------------------------------------------------

/// Internal signal for non-local control flow inside loops / functions.
enum ControlFlow {
    Return(Option<Variant>),
    Break,
    Continue,
    /// Suspend the current function at an `await` (pat-p0s82.7).
    Suspend(crate::coroutine::AwaitTarget),
}

// ---------------------------------------------------------------------------
// Interpreter
// ---------------------------------------------------------------------------

/// The result of running a GDScript program.
#[derive(Debug, Clone)]
pub struct InterpreterResult {
    /// Lines produced by `print()` calls.
    pub output: Vec<String>,
    /// The value returned by the last top-level `return` (if any).
    pub return_value: Option<Variant>,
}

/// A tree-walk interpreter for GDScript.
pub struct Interpreter {
    environment: Environment,
    function_registry: HashMap<String, FuncDef>,
    output: Vec<String>,
    /// Continuation frames captured while a suspension unwinds, innermost
    /// first. Drained by `park_coroutine` into the parked frame (pat-0q2kx.78).
    pending_continuation: Vec<crate::coroutine::ResumeFrame>,
    /// Set when the most recent call parked a coroutine instead of returning.
    ///
    /// A suspending callee returns `Nil` like any other, so this is the only
    /// signal the caller has that it should await rather than carry on
    /// (pat-0q2kx.78).
    last_call_suspended: Option<crate::coroutine::CoroutineId>,
    call_depth: usize,
    /// The current class instance when executing inside a class method.
    self_instance: Option<ClassInstance>,
    /// Registry of known class definitions (for super lookup).
    class_registry: HashMap<String, ClassDef>,
    source_lines: Vec<String>,
    current_line: usize,
    current_col: usize,
    current_function: Option<String>,
    call_stack_frames: Vec<StackFrame>,
    warnings: Vec<ScriptWarning>,
    /// Runtime error/warning channel populated by `push_error`, `push_warning`,
    /// and `printerr` (pat-u3l8c.20). Distinct from `output` (the `print`
    /// stream) so tests and tooling can observe the error stream, including the
    /// source location appended by `push_error`/`push_warning`.
    errors: Vec<String>,
    /// Scene-tree access for `get_node`, `emit_signal`, etc.
    pub(crate) scene_access: Option<Box<dyn SceneAccess>>,
    /// The raw NodeId of the node this script is attached to.
    pub(crate) current_node_id: Option<u64>,
    /// Suspended / ready coroutines for `await` (pat-p0s82.7).
    pub coroutine_host: crate::coroutine::CoroutineHost,
    /// The local name that a suspending `var v = await sig` wants bound to the
    /// resumed value. Set when the `await` statement suspends, consumed by
    /// `park_coroutine` as it parks the frame, and bound in `resume_one` on
    /// resume. `None` for a bare `await` whose result is discarded (pat-nzgdu.6).
    pending_await_bind: Option<String>,
}

impl fmt::Debug for Interpreter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Interpreter")
            .field("call_depth", &self.call_depth)
            .field("scene_access", &self.scene_access.is_some())
            .field("current_node_id", &self.current_node_id)
            .finish()
    }
}

impl Clone for Interpreter {
    fn clone(&self) -> Self {
        Self {
            environment: self.environment.clone(),
            function_registry: self.function_registry.clone(),
            output: self.output.clone(),
            call_depth: self.call_depth,
            self_instance: self.self_instance.clone(),
            class_registry: self.class_registry.clone(),
            source_lines: self.source_lines.clone(),
            current_line: self.current_line,
            current_col: self.current_col,
            current_function: self.current_function.clone(),
            call_stack_frames: self.call_stack_frames.clone(),
            warnings: self.warnings.clone(),
            errors: self.errors.clone(),
            scene_access: None,
            current_node_id: self.current_node_id,
            coroutine_host: crate::coroutine::CoroutineHost::new(),
            pending_await_bind: None,
            // Both are strictly in-flight suspension bookkeeping, valid only
            // between a Suspend unwinding and the matching `park_coroutine`.
            // A clone starts with a fresh (empty) CoroutineHost above, so
            // carrying half-captured frames into it would describe a suspension
            // the clone has no record of (pat-0q2kx.78).
            pending_continuation: Vec::new(),
            last_call_suspended: None,
        }
    }
}

/// A stored user-defined function.
#[derive(Debug, Clone)]
pub struct FuncDef {
    /// Parameter names.
    pub params: Vec<String>,
    /// Default value expressions for each parameter (None = required).
    pub defaults: Vec<Option<Expr>>,
    /// Function body statements.
    pub body: Vec<Stmt>,
    /// Whether this is a static function.
    pub is_static: bool,
    /// Annotations declared on the function, e.g. `@rpc("any_peer", ...)`.
    ///
    /// ⚠️ The parser has always attached these to `Stmt::FuncDef`, but `FuncDef`
    /// dropped them on the floor when the `ClassDef` was built — which made
    /// registering a script's RPCs structurally impossible, not merely unwired
    /// (pat-0q2kx.79).
    pub annotations: Vec<Annotation>,
}

impl FuncDef {
    /// Constructs a `FuncDef` from parsed [`FuncParam`]s.
    pub fn from_params(params: &[FuncParam], body: &[Stmt], is_static: bool) -> Self {
        Self::from_params_annotated(params, body, is_static, &[])
    }

    /// [`from_params`](Self::from_params) that also keeps the function's
    /// annotations.
    pub fn from_params_annotated(
        params: &[FuncParam],
        body: &[Stmt],
        is_static: bool,
        annotations: &[Annotation],
    ) -> Self {
        Self {
            params: params.iter().map(|p| p.name.clone()).collect(),
            defaults: params.iter().map(|p| p.default.clone()).collect(),
            body: body.to_vec(),
            is_static,
            annotations: annotations.to_vec(),
        }
    }

    /// The `@rpc(...)` arguments if this function is an RPC, else `None`.
    ///
    /// A bare `@rpc` yields an empty argument list, which is Godot's
    /// `authority`/`call_remote`/`unreliable`/channel-0 default — distinct from
    /// "not an RPC at all", hence `Option<Vec<_>>` rather than `Vec<_>`.
    pub fn rpc_annotation(&self) -> Option<&[String]> {
        self.annotations
            .iter()
            .find(|a| a.name == "rpc")
            .map(|a| a.args.as_slice())
    }

    /// Returns the minimum number of required arguments (those without defaults).
    pub fn min_args(&self) -> usize {
        self.defaults.iter().take_while(|d| d.is_none()).count()
    }
}

/// Information about an exported variable.
#[derive(Debug, Clone)]
pub struct ExportInfo {
    /// Variable name.
    pub name: String,
    /// Optional type hint.
    pub type_hint: Option<String>,
    /// Godot `PROPERTY_HINT_*` code derived from the `@export*` annotation
    /// (0 = `PROPERTY_HINT_NONE`). Drives the inspector editor for this property.
    pub hint: i64,
    /// Hint string paired with [`hint`](Self::hint), e.g. `"0,100,5"` for
    /// `@export_range(0,100,5)` or `"Idle,Run"` for `@export_enum("Idle","Run")`.
    pub hint_string: String,
}

// Godot 4 `PropertyHint` codes (core/object/object.h) used by the inspector.
/// No hint (`@export`).
pub const PROPERTY_HINT_NONE: i64 = 0;
/// Numeric range (`@export_range(min,max,step,...)`).
pub const PROPERTY_HINT_RANGE: i64 = 1;
/// Enumerated choices (`@export_enum("A","B")`).
pub const PROPERTY_HINT_ENUM: i64 = 2;
/// Bit flags (`@export_flags("A","B")`).
pub const PROPERTY_HINT_FLAGS: i64 = 6;
/// File path with optional filter (`@export_file("*.png")`).
pub const PROPERTY_HINT_FILE: i64 = 13;
/// Directory path (`@export_dir`).
pub const PROPERTY_HINT_DIR: i64 = 14;
/// Multiline text (`@export_multiline`).
pub const PROPERTY_HINT_MULTILINE_TEXT: i64 = 18;
/// Placeholder text (`@export_placeholder("text")`).
pub const PROPERTY_HINT_PLACEHOLDER_TEXT: i64 = 20;
/// Color without alpha (`@export_color_no_alpha`).
pub const PROPERTY_HINT_COLOR_NO_ALPHA: i64 = 21;
/// NodePath restricted to node types (`@export_node_path("Node2D")`).
pub const PROPERTY_HINT_NODE_PATH_VALID_TYPES: i64 = 24;

/// Maps an `@export*` annotation to its Godot `(PROPERTY_HINT_*, hint_string)`
/// pair. The `hint_string` is the annotation's arguments joined with commas,
/// matching Godot's inspector encoding (e.g. `@export_range(0,100,5)` ->
/// `(PROPERTY_HINT_RANGE, "0,100,5")`). Returns `None` for non-export
/// annotations so callers can `find_map` the first export in a list.
pub fn export_hint_from_annotation(ann: &Annotation) -> Option<(i64, String)> {
    let joined = || ann.args.join(",");
    let hint = match ann.name.as_str() {
        "export" => (PROPERTY_HINT_NONE, String::new()),
        "export_range" => (PROPERTY_HINT_RANGE, joined()),
        "export_enum" => (PROPERTY_HINT_ENUM, joined()),
        "export_flags" => (PROPERTY_HINT_FLAGS, joined()),
        "export_file" | "export_global_file" => (PROPERTY_HINT_FILE, joined()),
        "export_dir" | "export_global_dir" => (PROPERTY_HINT_DIR, String::new()),
        "export_multiline" => (PROPERTY_HINT_MULTILINE_TEXT, String::new()),
        "export_placeholder" => (PROPERTY_HINT_PLACEHOLDER_TEXT, joined()),
        "export_color_no_alpha" => (PROPERTY_HINT_COLOR_NO_ALPHA, String::new()),
        "export_node_path" => (PROPERTY_HINT_NODE_PATH_VALID_TYPES, joined()),
        _ => return None,
    };
    Some(hint)
}

/// Instance variable declaration with default.
#[derive(Debug, Clone)]
pub struct VarDecl {
    /// Variable name.
    pub name: String,
    /// Optional type hint.
    pub type_hint: Option<String>,
    /// Default value expression.
    pub default: Option<Expr>,
    /// Annotations on this variable.
    pub annotations: Vec<Annotation>,
    /// Optional setter function name.
    pub setter: Option<String>,
    /// Optional getter function name.
    pub getter: Option<String>,
}

/// A GDScript class definition parsed from source.
#[derive(Debug, Clone)]
pub struct ClassDef {
    /// The class name (from `class_name`).
    pub name: Option<String>,
    /// Parent class name (from `extends`).
    pub parent_class: Option<String>,
    /// Declared signals.
    pub signals: Vec<String>,
    /// Declared enums: name → { variant_name → value }.
    pub enums: HashMap<String, HashMap<String, i64>>,
    /// Methods defined in the class.
    pub methods: HashMap<String, FuncDef>,
    /// Instance variable declarations.
    pub instance_vars: Vec<VarDecl>,
    /// Exported variables.
    pub exports: Vec<ExportInfo>,
    /// Property setter function names: property_name → setter_func_name.
    pub setters: HashMap<String, String>,
    /// Property getter function names: property_name → getter_func_name.
    pub getters: HashMap<String, String>,
    /// Whether this script has `@tool` annotation.
    pub is_tool: bool,
    /// Inner class definitions: name → ClassDef.
    pub inner_classes: HashMap<String, ClassDef>,
    /// Class-level `const` declarations, in declaration order.
    pub constants: Vec<(String, Expr)>,
}

/// A live instance of a class.
#[derive(Debug, Clone)]
pub struct ClassInstance {
    /// The class definition this instance was created from.
    pub class_def: ClassDef,
    /// Instance variable values.
    pub properties: HashMap<String, Variant>,
}

impl Interpreter {
    /// Drains everything `print()` has emitted since the last call.
    ///
    /// The buffer itself is private, and `InterpreterResult::output` only
    /// carries what a single top-level run produced — a host that keeps ONE
    /// interpreter alive across many `call_instance_method` invocations (the
    /// editor running a `@tool` script, pat-0q2kx.76) has no other way to reach
    /// the lines those calls printed.
    pub fn take_output(&mut self) -> Vec<String> {
        std::mem::take(&mut self.output)
    }

    /// Creates a new interpreter with an empty environment.
    pub fn new() -> Self {
        Self {
            environment: Environment::new(),
            function_registry: HashMap::new(),
            output: Vec::new(),
            pending_continuation: Vec::new(),
            last_call_suspended: None,
            call_depth: 0,
            self_instance: None,
            class_registry: HashMap::new(),

            source_lines: Vec::new(),
            current_line: 0,
            current_col: 0,
            current_function: None,
            call_stack_frames: Vec::new(),
            warnings: Vec::new(),
            errors: Vec::new(),
            scene_access: None,
            current_node_id: None,
            coroutine_host: crate::coroutine::CoroutineHost::new(),
            pending_await_bind: None,
        }
    }

    /// Sets scene-tree access for the interpreter.
    pub fn set_scene_access(&mut self, access: Box<dyn SceneAccess>, node_id: u64) {
        self.scene_access = Some(access);
        self.current_node_id = Some(node_id);
    }

    /// Clears scene-tree access after a method call.
    pub fn clear_scene_access(&mut self) {
        self.scene_access = None;
    }

    pub fn warnings(&self) -> &[ScriptWarning] {
        &self.warnings
    }

    /// The runtime error/warning channel (`push_error`, `push_warning`,
    /// `printerr` output), in emission order. Each `push_error`/`push_warning`
    /// entry carries the appended source location (`... at: <fn>:<line>`).
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    fn make_location(&self) -> SourceLocation {
        let line_text = if self.current_line > 0 && self.current_line <= self.source_lines.len() {
            self.source_lines[self.current_line - 1].to_string()
        } else {
            String::new()
        };
        SourceLocation {
            line: self.current_line,
            column: self.current_col,
            source_line: line_text,
        }
    }

    #[allow(dead_code)]
    fn make_error(&self, kind: RuntimeErrorKind) -> RuntimeError {
        RuntimeError {
            kind,
            source_location: Some(self.make_location()),
            call_stack: self.call_stack_frames.clone(),
        }
    }

    fn check_unreachable_code(&mut self, stmts: &[Stmt]) {
        let mut found_return = false;
        for stmt in stmts {
            if found_return {
                self.warnings.push(ScriptWarning::UnreachableCode {
                    location: self.make_location(),
                });
                break;
            }
            if matches!(stmt, Stmt::Return(_)) {
                found_return = true;
            }
        }
    }

    #[allow(dead_code)]
    fn check_shadowed_variable(&mut self, name: &str) {
        if self.environment.exists_in_outer_scope(name) {
            self.warnings.push(ScriptWarning::ShadowedVariable {
                name: name.to_string(),
                location: self.make_location(),
            });
        }
    }

    /// Tokenizes, parses, and executes a GDScript source string.
    pub fn run(&mut self, source: &str) -> Result<InterpreterResult, RuntimeError> {
        self.source_lines = source.lines().map(|l| l.to_string()).collect();
        let tokens = tokenize(source)
            .map_err(|e| RuntimeError::new(RuntimeErrorKind::LexError(e.to_string())))?;
        let mut parser = Parser::new(tokens, source);
        let stmts = parser
            .parse_script()
            .map_err(|e| RuntimeError::new(RuntimeErrorKind::ParseError(e.to_string())))?;
        self.check_unreachable_code(&stmts);
        let mut last_return = None;
        for stmt in &stmts {
            if let Some(ControlFlow::Return(v)) = self.exec_stmt(stmt)? {
                last_return = v;
                break;
            }
        }
        Ok(InterpreterResult {
            output: self.output.clone(),
            return_value: last_return,
        })
    }

    // -----------------------------------------------------------------------
    // Statement execution
    // -----------------------------------------------------------------------

    fn exec_stmt(&mut self, stmt: &Stmt) -> Result<Option<ControlFlow>, RuntimeError> {
        match stmt {
            Stmt::VarDecl { name, value, .. } => {
                let v = match value {
                    Some(expr) => self.eval_expr(expr)?,
                    None => Variant::Nil,
                };
                self.environment.define(name.clone(), v);
                Ok(None)
            }

            Stmt::ConstDecl { name, value, .. } => {
                let v = self.eval_expr(value)?;
                self.environment.define_const(name.clone(), v);
                Ok(None)
            }

            Stmt::Assignment { target, op, value } => {
                let rhs = self.eval_expr(value)?;
                self.exec_assignment(target, op, rhs)?;
                Ok(None)
            }

            Stmt::If {
                condition,
                body,
                elif_branches,
                else_body,
            } => {
                let cond = self.eval_expr(condition)?;
                if cond.is_truthy() {
                    return self.exec_block(body);
                }
                for (elif_cond, elif_body) in elif_branches {
                    let c = self.eval_expr(elif_cond)?;
                    if c.is_truthy() {
                        return self.exec_block(elif_body);
                    }
                }
                if let Some(eb) = else_body {
                    return self.exec_block(eb);
                }
                Ok(None)
            }

            Stmt::While { condition, body } => {
                loop {
                    let cond = self.eval_expr(condition)?;
                    if !cond.is_truthy() {
                        break;
                    }
                    if let Some(cf) = self.exec_block(body)? {
                        match cf {
                            ControlFlow::Break => break,
                            ControlFlow::Continue => continue,
                            ControlFlow::Return(_) => return Ok(Some(cf)),
                            // A suspension owes the REST OF THE LOOP, not just
                            // the rest of this iteration: record the loop itself
                            // so resume re-tests the condition and carries on
                            // (pat-0q2kx.78).
                            ControlFlow::Suspend(_) => {
                                self.pending_continuation.push(
                                    crate::coroutine::ResumeFrame::While {
                                        condition: condition.clone(),
                                        body: body.clone(),
                                    },
                                );
                                return Ok(Some(cf));
                            }
                        }
                    }
                }
                Ok(None)
            }

            Stmt::For {
                var,
                iterable,
                body,
            } => {
                let iter_val = self.eval_expr(iterable)?;
                let items = match iter_val {
                    Variant::Array(a) => a,
                    // Godot iterates an int as range(n): 0, 1, ..., n-1
                    // (empty when n <= 0). Mirrors `for i in 5:`.
                    Variant::Int(n) => (0..n).map(Variant::Int).collect(),
                    other => {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                            "cannot iterate over {}",
                            other.variant_type()
                        ))));
                    }
                };
                for (idx, item) in items.iter().enumerate() {
                    self.environment.define(var.clone(), item.clone());
                    if let Some(cf) = self.exec_block(body)? {
                        match cf {
                            ControlFlow::Break => break,
                            ControlFlow::Continue => continue,
                            ControlFlow::Return(_) => return Ok(Some(cf)),
                            // The items after this one were never reached; carry
                            // them so resume finishes the iteration. This is the
                            // "spawns ONE enemy" bug (pat-0q2kx.78).
                            ControlFlow::Suspend(_) => {
                                self.pending_continuation.push(
                                    crate::coroutine::ResumeFrame::For {
                                        var: var.clone(),
                                        body: body.clone(),
                                        pending_items: items[idx + 1..].to_vec(),
                                    },
                                );
                                return Ok(Some(cf));
                            }
                        }
                    }
                }
                Ok(None)
            }

            Stmt::Return(expr) => {
                let v = match expr {
                    Some(e) => Some(self.eval_expr(e)?),
                    None => None,
                };
                Ok(Some(ControlFlow::Return(v)))
            }

            Stmt::FuncDef {
                name,
                params,
                body,
                is_static,
                annotations,
                ..
            } => {
                self.function_registry.insert(
                    name.clone(),
                    FuncDef::from_params_annotated(params, body, *is_static, annotations),
                );
                Ok(None)
            }

            Stmt::ExprStmt(expr) => {
                self.eval_expr(expr)?;
                Ok(None)
            }

            Stmt::Pass => Ok(None),

            Stmt::Break => Ok(Some(ControlFlow::Break)),

            Stmt::Continue => Ok(Some(ControlFlow::Continue)),

            Stmt::Match { value, arms } => {
                let val = self.eval_expr(value)?;
                for arm in arms {
                    for pattern in &arm.patterns {
                        if let Some(bindings) = self.match_pattern(pattern, &val)? {
                            // Bind in a fresh scope so the guard and body can
                            // see the pattern binds.
                            self.environment.push_scope();
                            for (n, v) in bindings {
                                self.environment.define(n, v);
                            }
                            let guard_ok = match &arm.guard {
                                Some(g) => self.eval_expr(g)?.is_truthy(),
                                None => true,
                            };
                            if guard_ok {
                                let r = self.exec_block_no_scope(&arm.body);
                                self.environment.pop_scope();
                                return r;
                            }
                            // Guard failed: discard binds and try the next
                            // pattern / arm.
                            self.environment.pop_scope();
                        }
                    }
                }
                Ok(None)
            }

            Stmt::Await { expr, bind } => {
                // Clear first: only a suspension raised by THIS expression
                // should chain, not a stale one from an earlier call.
                self.last_call_suspended = None;
                let awaited = self.eval_expr(expr)?;
                // `await helper()` where helper itself awaited: the call already
                // parked a coroutine and handed back `Nil`, so park this frame on
                // the callee and take its return value on completion.
                if let Some(id) = self.last_call_suspended.take() {
                    self.pending_await_bind = bind.clone();
                    return Ok(Some(ControlFlow::Suspend(
                        crate::coroutine::AwaitTarget::Coroutine { id: id.0 },
                    )));
                }
                if let Some(target) = crate::coroutine::AwaitTarget::from_variant(&awaited) {
                    // Real suspension — record the optional binding target so the
                    // parked frame binds the resumed value on resume, then bubble
                    // the suspension to the calling function frame (pat-nzgdu.6).
                    self.pending_await_bind = bind.clone();
                    return Ok(Some(ControlFlow::Suspend(target)));
                }
                // Non-awaitable value: Godot treats await on a non-signal as
                // immediate; bind Nil if a target was requested (pat-nzgdu.6).
                if let Some(name) = bind {
                    self.environment.define(name.clone(), Variant::Nil);
                }
                Ok(None)
            }

            // Class-level statements are no-ops during normal execution;
            // they are processed by `run_class()`.
            Stmt::Extends { .. }
            | Stmt::ClassNameDecl { .. }
            | Stmt::SignalDecl { .. }
            | Stmt::EnumDecl { .. }
            | Stmt::InnerClass { .. }
            | Stmt::AnnotationStmt { .. } => Ok(None),
        }
    }

    fn exec_block(&mut self, stmts: &[Stmt]) -> Result<Option<ControlFlow>, RuntimeError> {
        self.environment.push_scope();
        let mut result = None;
        for (i, stmt) in stmts.iter().enumerate() {
            if let Some(cf) = self.exec_stmt(stmt)? {
                // ⚠️ Return and Suspend both leave the block early, but they are
                // not the same: a Return discards the rest, while a Suspend owes
                // it. Record what is left so `resume_one` can finish this block
                // rather than abandoning it (pat-0q2kx.78).
                if matches!(cf, ControlFlow::Suspend(_)) {
                    let remaining = stmts[i + 1..].to_vec();
                    if !remaining.is_empty() {
                        self.pending_continuation
                            .push(crate::coroutine::ResumeFrame::Block { remaining });
                    }
                }
                result = Some(cf);
                break;
            }
        }
        self.environment.pop_scope();
        Ok(result)
    }

    fn exec_block_no_scope(&mut self, stmts: &[Stmt]) -> Result<Option<ControlFlow>, RuntimeError> {
        for stmt in stmts {
            if let Some(cf) = self.exec_stmt(stmt)? {
                return Ok(Some(cf));
            }
        }
        Ok(None)
    }

    /// Snapshot all currently visible bindings for coroutine resume.
    fn snapshot_locals(&self) -> HashMap<String, Variant> {
        let mut map = HashMap::new();
        for scope in &self.environment.scopes {
            for (k, v) in scope {
                map.insert(k.clone(), v.clone());
            }
        }
        map
    }

    /// Register a suspended coroutine with remaining statements.
    fn park_coroutine(
        &mut self,
        remaining: Vec<Stmt>,
        wait: crate::coroutine::AwaitTarget,
        function_name: &str,
        is_instance_method: bool,
    ) {
        let locals = self.snapshot_locals();
        let instance_props = self
            .self_instance
            .as_ref()
            .map(|i| i.properties.clone())
            .unwrap_or_default();
        // Take the `var v = await ...` binding target recorded by the suspending
        // await statement so the resumed frame binds the value (pat-nzgdu.6).
        let bind_target = self.pending_await_bind.take();
        // Frames were pushed innermost-first as the suspension unwound, which is
        // exactly the order resume needs to replay them.
        let frames = std::mem::take(&mut self.pending_continuation);
        let id = self
            .coroutine_host
            .suspend(crate::coroutine::SuspendedCoroutine {
                id: crate::coroutine::CoroutineId(0),
                remaining,
                frames,
                locals,
                instance_props,
                function_name: function_name.to_string(),
                is_instance_method,
                wait,
                bind_target,
                resume_value: Variant::Nil,
                prior_output: self.output.clone(),
            });
        // Remember it so an awaiting CALLER can park on this coroutine rather
        // than treating the callee's `Nil` as a finished result.
        self.last_call_suspended = Some(id);
    }

    /// Advance timer-based awaits by `delta` seconds, then resume any ready
    /// coroutines. Returns print-lines emitted by resumed bodies this tick.
    pub fn poll_coroutines(&mut self, delta: f32) -> Result<Vec<String>, RuntimeError> {
        // SceneTree timers (pat-z35r2.3): fire timeout signals then resume awaiters.
        if let Some(ref mut access) = self.scene_access {
            let fired = access.process_timers(delta as f64);
            for sig in fired {
                // Timer timeouts carry no argument (Godot `timeout` is void).
                self.coroutine_host.notify_signal(&sig, Variant::Nil);
            }
        }
        self.coroutine_host.poll(delta);
        self.resume_ready_coroutines()
    }

    /// Advance frame-based awaits by one frame, then resume ready coroutines.
    pub fn tick_coroutine_frames(&mut self) -> Result<Vec<String>, RuntimeError> {
        self.coroutine_host.tick_frame();
        self.resume_ready_coroutines()
    }

    /// Wake coroutines waiting on `signal_name` and resume them, binding the
    /// signal's first emitted argument (`args[0]`, or `Nil`) as the awaited value
    /// so `var v = await sig` sees it (pat-nzgdu.6).
    pub fn notify_await_signal(
        &mut self,
        signal_name: &str,
        args: &[Variant],
    ) -> Result<Vec<String>, RuntimeError> {
        self.notify_await_signal_from(None, signal_name, args)
    }

    /// [`notify_await_signal`](Self::notify_await_signal) with the emitting
    /// object's raw `ObjectId`, so an `await` bound to one object is not
    /// resumed by a same-named signal from another (pat-0q2kx.9).
    pub fn notify_await_signal_from(
        &mut self,
        source: Option<u64>,
        signal_name: &str,
        args: &[Variant],
    ) -> Result<Vec<String>, RuntimeError> {
        let value = args.first().cloned().unwrap_or(Variant::Nil);
        self.coroutine_host
            .notify_signal_from(source, signal_name, value);
        self.resume_ready_coroutines()
    }

    /// Resume this class instance's frame-based coroutines for one frame,
    /// threading `instance` in as `self` so resumed bodies read/mutate instance
    /// properties, then writing the mutations back. A coroutine resumed off the
    /// SceneTree tick must see the same `self` a normal method call would;
    /// without this the resumed `self.x = ...` writes and declared-signal
    /// lookups are lost because `self_instance` is torn down after the original
    /// call returns (pat-nzgdu.6).
    pub fn tick_instance_coroutine_frames(
        &mut self,
        instance: &mut ClassInstance,
    ) -> Result<Vec<String>, RuntimeError> {
        if self.coroutine_host.suspended_count() == 0 {
            return Ok(Vec::new());
        }
        self.resume_with_instance_self(instance, |i| i.tick_coroutine_frames())
    }

    /// Wake this class instance's coroutines parked on `signal_name`, threading
    /// `instance` in as `self` (see [`tick_instance_coroutine_frames`]) and
    /// binding the emitted argument as the awaited value (pat-nzgdu.6).
    pub fn notify_instance_await_signal(
        &mut self,
        instance: &mut ClassInstance,
        signal_name: &str,
        args: &[Variant],
    ) -> Result<Vec<String>, RuntimeError> {
        if self.coroutine_host.suspended_count() == 0 {
            return Ok(Vec::new());
        }
        self.resume_with_instance_self(instance, |i| i.notify_await_signal(signal_name, args))
    }

    /// [`notify_instance_await_signal`](Self::notify_instance_await_signal)
    /// carrying the emitting object's raw `ObjectId` (pat-0q2kx.9).
    pub fn notify_instance_await_signal_from(
        &mut self,
        instance: &mut ClassInstance,
        source: Option<u64>,
        signal_name: &str,
        args: &[Variant],
    ) -> Result<Vec<String>, RuntimeError> {
        if self.coroutine_host.suspended_count() == 0 {
            return Ok(Vec::new());
        }
        self.resume_with_instance_self(instance, |i| {
            i.notify_await_signal_from(source, signal_name, args)
        })
    }

    /// Installs `instance` as `self_instance` (registering its methods so a
    /// resumed body can call siblings), runs `f`, writes any property mutations
    /// back to `instance`, and restores the prior `self`. Mirrors the self
    /// setup/writeback in `call_instance_method` (pat-nzgdu.6).
    fn resume_with_instance_self<R>(
        &mut self,
        instance: &mut ClassInstance,
        f: impl FnOnce(&mut Self) -> Result<R, RuntimeError>,
    ) -> Result<R, RuntimeError> {
        let prev_self = self.self_instance.take();
        self.self_instance = Some(instance.clone());
        for (name, func_def) in &instance.class_def.methods {
            self.function_registry
                .insert(name.clone(), func_def.clone());
        }
        let result = f(self);
        if let Some(ref updated) = self.self_instance {
            instance.properties = updated.properties.clone();
        }
        self.self_instance = prev_self;
        result
    }

    /// How many coroutines are currently suspended (not ready).
    pub fn suspended_coroutine_count(&self) -> usize {
        self.coroutine_host.suspended_count()
    }

    fn resume_ready_coroutines(&mut self) -> Result<Vec<String>, RuntimeError> {
        let mut emitted = Vec::new();
        // Loop in case a resume immediately re-awaits zero-time work.
        loop {
            let ready = self.coroutine_host.take_ready();
            if ready.is_empty() {
                break;
            }
            for co in ready {
                let before = self.output.len();
                self.resume_one(co)?;
                emitted.extend(self.output[before..].iter().cloned());
            }
        }
        Ok(emitted)
    }

    fn resume_one(
        &mut self,
        co: crate::coroutine::SuspendedCoroutine,
    ) -> Result<Variant, RuntimeError> {
        // Restore local environment
        self.environment.push_scope();
        for (k, v) in &co.locals {
            self.environment.define(k.clone(), v.clone());
        }
        if co.is_instance_method {
            if let Some(ref mut inst) = self.self_instance {
                inst.properties = co.instance_props.clone();
            } else {
                // Reconstruct a minimal self shell so property writes work.
                // Class methods already re-bind self in call_instance_method;
                // free-standing resume uses properties map only.
            }
        }

        // Bind the resumed value (the emitted signal argument) to the target of
        // `var v = await sig` before running the remaining statements (pat-nzgdu.6).
        if let Some(ref name) = co.bind_target {
            self.environment
                .define(name.clone(), co.resume_value.clone());
        }

        // Finish the enclosing constructs first, innermost outward: the block
        // the await sat in, then the loop that block belonged to, and so on,
        // before the function body's own remaining statements (pat-0q2kx.78).
        //
        // ⚠️ A frame that suspends again owes everything OUTSIDE it too, so the
        // not-yet-replayed frames are appended to whatever the new suspension
        // pushed. Dropping them would lose an outer loop on the second await.
        for fi in 0..co.frames.len() {
            match self.run_resume_frame(&co.frames[fi])? {
                Some(ControlFlow::Return(v)) => {
                    let value = v.unwrap_or(Variant::Nil);
                    self.environment.pop_scope();
                    // Anything awaiting THIS coroutine now has its result.
                    self.coroutine_host.notify_coroutine_done(co.id, value.clone());
                    return Ok(value);
                }
                Some(ControlFlow::Suspend(target)) => {
                    self.pending_continuation
                        .extend(co.frames[fi + 1..].iter().cloned());
                    self.park_coroutine(
                        co.remaining.clone(),
                        target,
                        &co.function_name,
                        co.is_instance_method,
                    );
                    self.environment.pop_scope();
                    return Ok(Variant::Nil);
                }
                _ => {}
            }
        }

        let mut return_val = Variant::Nil;
        let mut i = 0;
        while i < co.remaining.len() {
            match self.exec_stmt(&co.remaining[i])? {
                Some(ControlFlow::Return(v)) => {
                    return_val = v.unwrap_or(Variant::Nil);
                    break;
                }
                Some(ControlFlow::Suspend(target)) => {
                    let rest = co.remaining[i + 1..].to_vec();
                    self.park_coroutine(rest, target, &co.function_name, co.is_instance_method);
                    self.environment.pop_scope();
                    return Ok(Variant::Nil);
                }
                Some(ControlFlow::Break) | Some(ControlFlow::Continue) => {
                    // Not meaningful at top-level of a resumed function body.
                }
                None => {}
            }
            i += 1;
        }
        self.environment.pop_scope();
        // The body ran to the end (or hit a `return`): wake any caller that
        // parked on this coroutine with `await helper()` (pat-0q2kx.78).
        self.coroutine_host
            .notify_coroutine_done(co.id, return_val.clone());
        Ok(return_val)
    }

    /// Replays one [`ResumeFrame`](crate::coroutine::ResumeFrame), i.e. finishes
    /// the construct it stands for.
    ///
    /// Returns the control flow that escaped it, so the caller can re-park on a
    /// further suspension or unwind on a `return`.
    fn run_resume_frame(
        &mut self,
        frame: &crate::coroutine::ResumeFrame,
    ) -> Result<Option<ControlFlow>, RuntimeError> {
        use crate::coroutine::ResumeFrame;
        match frame {
            // The tail of the block the await sat in. Executed WITHOUT a fresh
            // scope: its bindings were snapshotted into the resumed environment,
            // and pushing a scope here would hide later writes from them.
            //
            // ⚠️ Tracks its own index rather than delegating to
            // `exec_block_no_scope`: a second await inside this tail owes the
            // statements after it, and that helper records nothing.
            ResumeFrame::Block { remaining } => {
                for (i, stmt) in remaining.iter().enumerate() {
                    if let Some(cf) = self.exec_stmt(stmt)? {
                        if matches!(cf, ControlFlow::Suspend(_)) {
                            let rest = remaining[i + 1..].to_vec();
                            if !rest.is_empty() {
                                self.pending_continuation
                                    .push(ResumeFrame::Block { remaining: rest });
                            }
                        }
                        return Ok(Some(cf));
                    }
                }
                Ok(None)
            }

            // Continue iterating the items the loop never reached. The current
            // iteration's tail is a `Block` frame beneath this one and has
            // already run by the time we get here.
            ResumeFrame::For {
                var,
                body,
                pending_items,
            } => {
                for (idx, item) in pending_items.iter().enumerate() {
                    self.environment.define(var.clone(), item.clone());
                    if let Some(cf) = self.exec_block(body)? {
                        match cf {
                            ControlFlow::Break => break,
                            ControlFlow::Continue => continue,
                            ControlFlow::Return(_) => return Ok(Some(cf)),
                            ControlFlow::Suspend(_) => {
                                // Re-park the REST of this loop, exactly as the
                                // original `Stmt::For` arm does.
                                self.pending_continuation.push(ResumeFrame::For {
                                    var: var.clone(),
                                    body: body.clone(),
                                    pending_items: pending_items[idx + 1..].to_vec(),
                                });
                                return Ok(Some(cf));
                            }
                        }
                    }
                }
                Ok(None)
            }

            // Re-test the condition and keep looping.
            ResumeFrame::While { condition, body } => {
                loop {
                    let cond = self.eval_expr(condition)?;
                    if !cond.is_truthy() {
                        break;
                    }
                    if let Some(cf) = self.exec_block(body)? {
                        match cf {
                            ControlFlow::Break => break,
                            ControlFlow::Continue => continue,
                            ControlFlow::Return(_) => return Ok(Some(cf)),
                            ControlFlow::Suspend(_) => {
                                self.pending_continuation.push(ResumeFrame::While {
                                    condition: condition.clone(),
                                    body: body.clone(),
                                });
                                return Ok(Some(cf));
                            }
                        }
                    }
                }
                Ok(None)
            }
        }
    }
    /// Attempts to match `value` against `pattern`. Returns `Ok(Some(binds))`
    /// with any `var` bindings when the pattern matches, `Ok(None)` when it
    /// does not, and propagates evaluation errors from constant-reference or
    /// dictionary-key expressions.
    fn match_pattern(
        &mut self,
        pattern: &MatchPattern,
        value: &Variant,
    ) -> Result<Option<Vec<(String, Variant)>>, RuntimeError> {
        match pattern {
            // `..` outside an array/dict pattern (handled inline there) matches
            // anything.
            MatchPattern::Wildcard | MatchPattern::Rest => Ok(Some(vec![])),
            MatchPattern::Binding(name) => Ok(Some(vec![(name.clone(), value.clone())])),
            MatchPattern::Literal(lit) => Ok(if variant_eq(lit, value) {
                Some(vec![])
            } else {
                None
            }),
            // Bare identifier / expression: evaluate and compare by value.
            MatchPattern::ConstantRef(expr) => {
                let expected = self.eval_expr(expr)?;
                Ok(if variant_eq(&expected, value) {
                    Some(vec![])
                } else {
                    None
                })
            }
            MatchPattern::Array(patterns) => {
                let Variant::Array(arr) = value else {
                    return Ok(None);
                };
                let has_rest = matches!(patterns.last(), Some(MatchPattern::Rest));
                if has_rest {
                    // Match all fixed sub-patterns (everything before the rest)
                    // against a prefix; extra elements are allowed.
                    let fixed = &patterns[..patterns.len() - 1];
                    if arr.len() < fixed.len() {
                        return Ok(None);
                    }
                    let mut bindings = vec![];
                    for (pat, val) in fixed.iter().zip(arr.iter()) {
                        match self.match_pattern(pat, val)? {
                            Some(b) => bindings.extend(b),
                            None => return Ok(None),
                        }
                    }
                    Ok(Some(bindings))
                } else {
                    if arr.len() != patterns.len() {
                        return Ok(None);
                    }
                    let mut bindings = vec![];
                    for (pat, val) in patterns.iter().zip(arr.iter()) {
                        match self.match_pattern(pat, val)? {
                            Some(b) => bindings.extend(b),
                            None => return Ok(None),
                        }
                    }
                    Ok(Some(bindings))
                }
            }
            MatchPattern::Dictionary { entries, rest } => {
                let Variant::Dictionary(dict) = value else {
                    return Ok(None);
                };
                // Without a `..` rest, the key set must match exactly.
                if !rest && dict.len() != entries.len() {
                    return Ok(None);
                }
                let mut bindings = vec![];
                for (key_expr, sub) in entries {
                    let key = self.eval_expr(key_expr)?;
                    let Some(entry_val) = dict.get(&key) else {
                        return Ok(None);
                    };
                    if let Some(sub_pat) = sub {
                        match self.match_pattern(sub_pat, &entry_val.clone())? {
                            Some(b) => bindings.extend(b),
                            None => return Ok(None),
                        }
                    }
                }
                Ok(Some(bindings))
            }
        }
    }
    /// Applies a compound-assignment operator to the current value and the
    /// right-hand side, returning the value to store. For a plain `=` this is
    /// just `rhs`.
    fn apply_assign_op(
        &self,
        op: &AssignOp,
        cur: &Variant,
        rhs: &Variant,
    ) -> Result<Variant, RuntimeError> {
        let binop = match op {
            AssignOp::Assign => return Ok(rhs.clone()),
            AssignOp::AddAssign => BinOp::Add,
            AssignOp::SubAssign => BinOp::Sub,
            AssignOp::MulAssign => BinOp::Mul,
            AssignOp::DivAssign => BinOp::Div,
            AssignOp::ModAssign => BinOp::Mod,
            AssignOp::PowAssign => BinOp::Pow,
            AssignOp::BitAndAssign => BinOp::BitAnd,
            AssignOp::BitOrAssign => BinOp::BitOr,
            AssignOp::BitXorAssign => BinOp::BitXor,
            AssignOp::ShlAssign => BinOp::Shl,
            AssignOp::ShrAssign => BinOp::Shr,
        };
        self.eval_binary_op(&binop, cur, rhs)
    }

    /// Writes `value` into the place described by `target`, supporting nested
    /// index/member chains via read-modify-write. This is the general lvalue
    /// write path used by indexed assignment and mutating container methods so
    /// that `self.grid[i] = x`, `arr[i][j] = x`, and `self.items.append(x)` all
    /// persist to the underlying variable / instance property.
    fn write_place(&mut self, target: &Expr, value: Variant) -> Result<(), RuntimeError> {
        match target {
            Expr::Index { object, index } => {
                let idx = self.eval_expr(index)?;
                let mut container = self.eval_expr(object)?;
                set_index(&mut container, &idx, value)?;
                self.write_place(object, container)
            }
            // Idents, `self.member`, node properties and dictionary members are
            // handled by the existing plain-assignment logic. This also serves
            // as the recursion base for the index chain above.
            _ => self.exec_assignment(target, &AssignOp::Assign, value),
        }
    }

    fn exec_assignment(
        &mut self,
        target: &Expr,
        op: &AssignOp,
        rhs: Variant,
    ) -> Result<(), RuntimeError> {
        match target {
            Expr::Ident(name) => {
                // Check if this is an instance property (bare `count = x` in a class method),
                // but NOT if a local `var` shadows the instance variable.
                let is_instance_prop = self
                    .self_instance
                    .as_ref()
                    .map(|inst| inst.properties.contains_key(name.as_str()))
                    .unwrap_or(false)
                    && !self.environment.is_shadowed_in_inner_scope(name);
                let is_node_prop = !is_instance_prop
                    && !self.environment.is_shadowed_in_inner_scope(name)
                    && if let (Some(ref access), Some(node_id)) =
                        (&self.scene_access, self.current_node_id)
                    {
                        !access.get_node_property(node_id, name).is_nil()
                    } else {
                        false
                    };

                let final_val = if matches!(op, AssignOp::Assign) {
                    rhs
                } else {
                    let cur = if is_instance_prop {
                        self.self_instance
                            .as_ref()
                            .unwrap()
                            .properties
                            .get(name.as_str())
                            .cloned()
                            .unwrap_or(Variant::Nil)
                    } else if is_node_prop {
                        self.scene_access
                            .as_ref()
                            .unwrap()
                            .get_node_property(self.current_node_id.unwrap(), name)
                    } else {
                        self.environment.get(name)?
                    };
                    self.apply_assign_op(op, &cur, &rhs)?
                };

                if is_instance_prop {
                    // Check for setter function (avoid recursion if we're inside the setter)
                    if let Some(setter_name) = self
                        .self_instance
                        .as_ref()
                        .and_then(|inst| inst.class_def.setters.get(name).cloned())
                    {
                        if self.current_function.as_deref() != Some(&setter_name) {
                            return self.call_user_func(&setter_name, &[final_val]).map(|_| ());
                        }
                    }
                    // Write to instance properties (synced back on method return)
                    self.self_instance
                        .as_mut()
                        .unwrap()
                        .properties
                        .insert(name.clone(), final_val.clone());
                    // Also update environment so subsequent reads in this call see the new value
                    let _ = self.environment.set(name, final_val);
                    Ok(())
                } else if is_node_prop {
                    if let (Some(ref mut access), Some(node_id)) =
                        (&mut self.scene_access, self.current_node_id)
                    {
                        access.set_node_property(node_id, name, final_val.clone());
                    }
                    let _ = self.environment.set(name, final_val);
                    Ok(())
                } else {
                    self.environment.set(name, final_val)
                }
            }
            Expr::Index { object, index } => {
                let idx = self.eval_expr(index)?;
                // Read the container by evaluating its (possibly nested) base
                // expression, mutate the element, then write the whole container
                // back to its place. Supports `arr[i][j] = x`, `self.grid[i] = x`.
                let mut container = self.eval_expr(object)?;
                let final_val = if matches!(op, AssignOp::Assign) {
                    rhs
                } else {
                    let cur = index_into(&container, &idx)?;
                    self.apply_assign_op(op, &cur, &rhs)?
                };
                set_index(&mut container, &idx, final_val)?;
                self.write_place(object, container)
            }
            Expr::MemberAccess { object, member } => {
                if self.try_exec_value_member_assignment(object, member, op, rhs.clone())? {
                    return Ok(());
                }
                // Handle compound member assignment: obj.prop.field = value
                // e.g. self.position.x = 5.0, node.position.y += 10.0
                if let Expr::MemberAccess {
                    object: inner_obj,
                    member: prop,
                } = object.as_ref()
                {
                    return self.exec_compound_member_assignment(inner_obj, prop, member, op, rhs);
                }
                // Handle self.member = value
                if matches!(object.as_ref(), Expr::SelfRef) {
                    if self.self_instance.is_none() {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "'self' used outside of a class instance".into(),
                        )));
                    }
                    // Read current value: try instance properties first, then scene_access
                    let get_current = |s: &Self, member: &str| -> Variant {
                        if let Some(inst) = s.self_instance.as_ref() {
                            if let Some(v) = inst.properties.get(member) {
                                return v.clone();
                            }
                        }
                        if let Some(ref access) = s.scene_access {
                            if let Some(node_id) = s.current_node_id {
                                return access.get_node_property(node_id, member);
                            }
                        }
                        Variant::Nil
                    };
                    let final_val = if matches!(op, AssignOp::Assign) {
                        rhs
                    } else {
                        let cur = get_current(self, member);
                        self.apply_assign_op(op, &cur, &rhs)?
                    };
                    // Check for setter function (avoid recursion if inside setter)
                    if let Some(setter_name) = self
                        .self_instance
                        .as_ref()
                        .and_then(|inst| inst.class_def.setters.get(member).cloned())
                    {
                        if self.current_function.as_deref() != Some(&setter_name) {
                            return self.call_user_func(&setter_name, &[final_val]).map(|_| ());
                        }
                    }
                    // Write to instance properties
                    self.self_instance
                        .as_mut()
                        .unwrap()
                        .properties
                        .insert(member.clone(), final_val.clone());
                    // Keep any environment binding in sync so bare-name reads
                    // in the same call observe the write (mirrors the Ident arm).
                    let _ = self.environment.set(member, final_val.clone());
                    // Also write through scene_access for Node properties (position, rotation, etc.)
                    if let Some(ref mut access) = self.scene_access {
                        if let Some(node_id) = self.current_node_id {
                            access.set_node_property(node_id, member, final_val);
                        }
                    }
                    return Ok(());
                }
                // Handle bare node/instance property member writes:
                // `position.x += 1`, `velocity.y = 2`, etc.
                if let Expr::Ident(name) = object.as_ref() {
                    let is_instance_prop = self
                        .self_instance
                        .as_ref()
                        .map(|inst| inst.properties.contains_key(name.as_str()))
                        .unwrap_or(false);
                    let is_node_prop = if let (Some(ref access), Some(node_id)) =
                        (&self.scene_access, self.current_node_id)
                    {
                        !access.get_node_property(node_id, name).is_nil()
                    } else {
                        false
                    };
                    if is_instance_prop || is_node_prop {
                        return self.exec_compound_member_assignment(
                            &Expr::SelfRef,
                            name,
                            member,
                            op,
                            rhs,
                        );
                    }
                }
                // Check if the object evaluates to an ObjectId
                let obj_val = self.eval_expr(object)?;
                if let Variant::ObjectId(oid) = &obj_val {
                    let final_val = if matches!(op, AssignOp::Assign) {
                        rhs
                    } else {
                        let cur = if let Some(ref access) = self.scene_access {
                            access.get_node_property(oid.raw(), member)
                        } else {
                            Variant::Nil
                        };
                        self.apply_assign_op(op, &cur, &rhs)?
                    };
                    if let Some(ref mut access) = self.scene_access {
                        access.set_node_property(oid.raw(), member, final_val);
                    }
                    return Ok(());
                }
                // Generic member write on a container value (e.g. a dictionary
                // `d.key = v`), including nested/indexed bases via write_place.
                let mut container = self.eval_expr(object)?;
                let final_val = if matches!(op, AssignOp::Assign) {
                    rhs
                } else {
                    let cur = index_into(&container, &Variant::String(member.clone()))?;
                    self.apply_assign_op(op, &cur, &rhs)?
                };
                set_index(&mut container, &Variant::String(member.clone()), final_val)?;
                self.write_place(object, container)
            }
            _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                "invalid assignment target".into(),
            ))),
        }
    }

    /// Handle read-modify-write on value-type members where the target object
    /// expression itself resolves to a container value such as Vector2, Vector3,
    /// or Color.
    ///
    /// Examples:
    /// - `position.x += 1`
    /// - `self.position.y = 2`
    /// - `node.position.x = 3`
    fn try_exec_value_member_assignment(
        &mut self,
        object: &Expr,
        field: &str,
        op: &AssignOp,
        rhs: Variant,
    ) -> Result<bool, RuntimeError> {
        let intermediate = match self.eval_expr(object) {
            Ok(value) => value,
            Err(_) => return Ok(false),
        };

        let current_field = match &intermediate {
            Variant::Vector2(v) => match field {
                "x" => Some(Variant::Float(v.x as f64)),
                "y" => Some(Variant::Float(v.y as f64)),
                _ => None,
            },
            Variant::Vector3(v) => match field {
                "x" => Some(Variant::Float(v.x as f64)),
                "y" => Some(Variant::Float(v.y as f64)),
                "z" => Some(Variant::Float(v.z as f64)),
                _ => None,
            },
            Variant::Color(c) => match field {
                "r" => Some(Variant::Float(c.r as f64)),
                "g" => Some(Variant::Float(c.g as f64)),
                "b" => Some(Variant::Float(c.b as f64)),
                "a" => Some(Variant::Float(c.a as f64)),
                _ => None,
            },
            _ => None,
        };

        let Some(current_field) = current_field else {
            // Members of the affine builtins are whole values, not floats
            // (`t.origin` is a Vector2, `t.basis` a Basis), so they need the
            // general read-modify-write below rather than the float path
            // (pat-0q2kx.50).
            if let Some(current) = crate::variant_math::member_get(&intermediate, field) {
                let final_field = self.apply_assign_op(op, &current, &rhs)?;
                match crate::variant_math::member_set(&intermediate, field, &final_field) {
                    Some(updated) => {
                        self.exec_assignment(object, &AssignOp::Assign, updated?)?;
                        return Ok(true);
                    }
                    None => return Ok(false),
                }
            }
            return Ok(false);
        };

        let final_field = self.apply_assign_op(op, &current_field, &rhs)?;
        let new_float = to_float(&final_field)? as f32;

        let modified = match intermediate {
            Variant::Vector2(v) => match field {
                "x" => Variant::Vector2(gdcore::math::Vector2::new(new_float, v.y)),
                "y" => Variant::Vector2(gdcore::math::Vector2::new(v.x, new_float)),
                _ => unreachable!(),
            },
            Variant::Vector3(v) => match field {
                "x" => Variant::Vector3(gdcore::math::Vector3::new(new_float, v.y, v.z)),
                "y" => Variant::Vector3(gdcore::math::Vector3::new(v.x, new_float, v.z)),
                "z" => Variant::Vector3(gdcore::math::Vector3::new(v.x, v.y, new_float)),
                _ => unreachable!(),
            },
            Variant::Color(c) => match field {
                "r" => Variant::Color(gdcore::math::Color::new(new_float, c.g, c.b, c.a)),
                "g" => Variant::Color(gdcore::math::Color::new(c.r, new_float, c.b, c.a)),
                "b" => Variant::Color(gdcore::math::Color::new(c.r, c.g, new_float, c.a)),
                "a" => Variant::Color(gdcore::math::Color::new(c.r, c.g, c.b, new_float)),
                _ => unreachable!(),
            },
            _ => unreachable!(),
        };

        self.exec_assignment(object, &AssignOp::Assign, modified)?;
        Ok(true)
    }

    /// Handle compound member assignment: `inner_obj.prop.field = rhs`
    ///
    /// Implements the read-modify-write pattern for value-type members:
    /// 1. Read `inner_obj.prop` → get intermediate (e.g. Vector2)
    /// 2. Modify the `field` component (e.g. x) with `rhs`
    /// 3. Write back `inner_obj.prop = modified`
    fn exec_compound_member_assignment(
        &mut self,
        inner_obj: &Expr,
        prop: &str,
        field: &str,
        op: &AssignOp,
        rhs: Variant,
    ) -> Result<(), RuntimeError> {
        // Step 1: Read the intermediate value (e.g. self.position → Vector2)
        let intermediate = self.eval_expr(&Expr::MemberAccess {
            object: Box::new(inner_obj.clone()),
            member: prop.to_string(),
        })?;

        // Step 2: Get the current field value for compound ops
        let current_field = match &intermediate {
            Variant::Vector2(v) => match field {
                "x" => Ok(Variant::Float(v.x as f64)),
                "y" => Ok(Variant::Float(v.y as f64)),
                _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                    "Vector2 has no member '{field}'"
                )))),
            },
            Variant::Vector3(v) => match field {
                "x" => Ok(Variant::Float(v.x as f64)),
                "y" => Ok(Variant::Float(v.y as f64)),
                "z" => Ok(Variant::Float(v.z as f64)),
                _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                    "Vector3 has no member '{field}'"
                )))),
            },
            Variant::Color(c) => match field {
                "r" => Ok(Variant::Float(c.r as f64)),
                "g" => Ok(Variant::Float(c.g as f64)),
                "b" => Ok(Variant::Float(c.b as f64)),
                "a" => Ok(Variant::Float(c.a as f64)),
                _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                    "Color has no member '{field}'"
                )))),
            },
            other => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                "cannot assign member '{field}' on {}",
                other.variant_type()
            )))),
        }?;

        // Step 3: Compute final value based on assignment operator
        let final_val = self.apply_assign_op(op, &current_field, &rhs)?;

        let new_float = to_float(&final_val)? as f32;

        // Step 4: Create modified intermediate with the new field value
        let modified = match intermediate {
            Variant::Vector2(v) => match field {
                "x" => Ok(Variant::Vector2(gdcore::math::Vector2::new(new_float, v.y))),
                "y" => Ok(Variant::Vector2(gdcore::math::Vector2::new(v.x, new_float))),
                _ => unreachable!(),
            },
            Variant::Vector3(v) => match field {
                "x" => Ok(Variant::Vector3(gdcore::math::Vector3::new(
                    new_float, v.y, v.z,
                ))),
                "y" => Ok(Variant::Vector3(gdcore::math::Vector3::new(
                    v.x, new_float, v.z,
                ))),
                "z" => Ok(Variant::Vector3(gdcore::math::Vector3::new(
                    v.x, v.y, new_float,
                ))),
                _ => unreachable!(),
            },
            Variant::Color(c) => match field {
                "r" => Ok(Variant::Color(gdcore::math::Color::new(
                    new_float, c.g, c.b, c.a,
                ))),
                "g" => Ok(Variant::Color(gdcore::math::Color::new(
                    c.r, new_float, c.b, c.a,
                ))),
                "b" => Ok(Variant::Color(gdcore::math::Color::new(
                    c.r, c.g, new_float, c.a,
                ))),
                "a" => Ok(Variant::Color(gdcore::math::Color::new(
                    c.r, c.g, c.b, new_float,
                ))),
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }?;

        // Step 5: Write back the modified intermediate to the original target
        self.exec_assignment(
            &Expr::MemberAccess {
                object: Box::new(inner_obj.clone()),
                member: prop.to_string(),
            },
            &AssignOp::Assign,
            modified,
        )
    }

    // -----------------------------------------------------------------------
    // Expression evaluation
    // -----------------------------------------------------------------------

    fn eval_expr(&mut self, expr: &Expr) -> Result<Variant, RuntimeError> {
        match expr {
            Expr::Literal(v) => Ok(v.clone()),

            Expr::Ident(name) => match name.as_str() {
                "PI" => Ok(Variant::Float(std::f64::consts::PI)),
                "TAU" => Ok(Variant::Float(std::f64::consts::TAU)),
                "INF" => Ok(Variant::Float(f64::INFINITY)),
                "NAN" => Ok(Variant::Float(f64::NAN)),
                _ => {
                    if let Ok(value) = self.environment.get(name) {
                        return Ok(value);
                    }
                    if let Some(ref inst) = self.self_instance {
                        if let Some(value) = inst.properties.get(name) {
                            // Check for getter — only call if not already
                            // inside the getter (avoids infinite recursion).
                            if let Some(getter_name) = inst.class_def.getters.get(name).cloned() {
                                if self.current_function.as_deref() != Some(&getter_name) {
                                    return self.call_user_func(&getter_name, &[]);
                                }
                            }
                            return Ok(value.clone());
                        }
                    }
                    if let (Some(ref access), Some(node_id)) =
                        (&self.scene_access, self.current_node_id)
                    {
                        let value = access.get_node_property(node_id, name);
                        if !value.is_nil() {
                            return Ok(value);
                        }
                    }
                    // A bareword naming a signal declared on this script yields a
                    // signal awaitable, so `await my_signal` suspends until that
                    // signal fires — Godot resolves `await sig` the same way. A
                    // same-named local shadows it (checked above). (pat-nzgdu.6)
                    if let Some(ref inst) = self.self_instance {
                        if inst.class_def.signals.iter().any(|s| s == name) {
                            // In Godot a bareword signal IS a `Signal` value
                            // bound to `self` — `my_signal.emit(x)` and
                            // `await my_signal` are both just uses of it. When
                            // we know which node we are, bind it; otherwise
                            // fall back to the legacy name-only awaitable
                            // (pat-0q2kx.9).
                            return Ok(match self.current_node_id {
                                Some(id) => Variant::Signal {
                                    object_id: id,
                                    name: name.to_string(),
                                },
                                None => crate::coroutine::AwaitTarget::signal_variant(name),
                            });
                        }
                    }
                    // A bareword naming a method declared on this script is a
                    // `Callable` bound to it — that is how Godot 4 writes
                    // `sig.connect(_on_pressed)` with no `Callable(...)`
                    // wrapper. Resolved after locals / members / node
                    // properties / signals so any of those shadow it, matching
                    // Godot's order. Only plain identifiers reach here; a real
                    // call `foo()` is an `Expr::Call` handled elsewhere, so this
                    // cannot turn a call into a value (pat-0q2kx.9).
                    if let Some(ref inst) = self.self_instance {
                        if inst.class_def.methods.contains_key(name) {
                            return Ok(Variant::Callable(Box::new(CallableRef::Method {
                                // 0 means "the calling object" for a Callable;
                                // `callable_target` resolves it when connecting.
                                target_id: self.current_node_id.unwrap_or(0),
                                method: name.to_string(),
                            })));
                        }
                    }
                    // @GlobalScope named constants (KEY_*/OK/ERR_*/TYPE_*/...) are
                    // barewords in GDScript. Resolve them last, after user locals,
                    // members, `const`s, and node properties — matching Godot, where
                    // a local of the same name shadows the global constant. (pat-nzgdu.7)
                    if let Some(value) = crate::global_constants::global_constant(name) {
                        return Ok(value);
                    }
                    // Autoload singletons (pat-nzgdu.70). Godot binds every
                    // `[autoload]` name declared in project.godot as a global
                    // resolving to the node at `/root/<Name>`, so `Global.score`
                    // needs no get_node. Resolved here — last, and only for names
                    // that would otherwise be a hard error — so locals, members,
                    // node properties and global constants all shadow an autoload,
                    // matching Godot's resolution order. The node is looked up
                    // through this interpreter's own scene access, so each
                    // SceneTree in the process resolves to its own node.
                    if crate::autoload_registry::is_autoload(name) {
                        let path = format!("/root/{name}");
                        if let (Some(ref access), Some(node_id)) =
                            (&self.scene_access, self.current_node_id)
                        {
                            if let Some(target) = access.get_node(node_id, &path) {
                                let oid = gdcore::id::ObjectId::from_raw(target);
                                return Ok(Variant::ObjectId(oid));
                            }
                        }
                    }
                    Err(RuntimeError::new(RuntimeErrorKind::UndefinedVariable(
                        name.clone(),
                    )))
                }
            },

            Expr::BinaryOp { left, op, right } => {
                let lhs = self.eval_expr(left)?;
                // Short-circuit for logical operators
                match op {
                    BinOp::And => {
                        if !lhs.is_truthy() {
                            return Ok(lhs);
                        }
                        return self.eval_expr(right);
                    }
                    BinOp::Or => {
                        if lhs.is_truthy() {
                            return Ok(lhs);
                        }
                        return self.eval_expr(right);
                    }
                    _ => {}
                }
                let rhs = self.eval_expr(right)?;
                self.eval_binary_op(op, &lhs, &rhs)
            }

            Expr::UnaryOp { op, expr } => {
                let val = self.eval_expr(expr)?;
                match op {
                    UnaryOp::Neg => match val {
                        Variant::Int(i) => Ok(Variant::Int(-i)),
                        Variant::Float(f) => Ok(Variant::Float(-f)),
                        Variant::Vector2(v) => Ok(Variant::Vector2(-v)),
                        Variant::Vector3(v) => Ok(Variant::Vector3(-v)),
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                            "cannot negate {}",
                            val.variant_type()
                        )))),
                    },
                    UnaryOp::Not => Ok(Variant::Bool(!val.is_truthy())),
                    UnaryOp::BitNot => match val {
                        Variant::Int(i) => Ok(Variant::Int(!i)),
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                            "cannot apply bitwise '~' to {}",
                            val.variant_type()
                        )))),
                    },
                }
            }

            Expr::Call { callee, args } => {
                let evaluated_args: Vec<Variant> = args
                    .iter()
                    .map(|a| self.eval_expr(a))
                    .collect::<Result<_, _>>()?;

                // Check for built-in first, then method calls, then user funcs
                match callee.as_ref() {
                    Expr::Ident(name) => {
                        if let Some(result) = self.try_builtin(name, &evaluated_args)? {
                            return Ok(result);
                        }
                        // Check if the name resolves to a Callable variable
                        if let Ok(Variant::Callable(ref callable)) = self.environment.get(name) {
                            let callable = callable.clone();
                            return self.invoke_callable(&callable, &evaluated_args);
                        }
                        self.call_self_func_or_native(name, &evaluated_args)
                    }
                    Expr::MemberAccess { object, member } => {
                        // Handle self.method() — dispatch to class methods
                        // First try builtins (add_child, queue_free, etc.), then user funcs
                        if matches!(object.as_ref(), Expr::SelfRef) {
                            if let Some(result) = self.try_builtin(member, &evaluated_args)? {
                                return Ok(result);
                            }
                            return self.call_self_func_or_native(member, &evaluated_args);
                        }
                        // Handle Input singleton: Input.is_action_pressed(), etc.
                        if matches!(object.as_ref(), Expr::Ident(n) if n == "Input") {
                            return self.call_input_method(member, &evaluated_args);
                        }
                        // FileAccess / DirAccess static methods (Godot core/io).
                        if matches!(object.as_ref(), Expr::Ident(n) if n == "FileAccess") {
                            return call_file_access_static(member, &evaluated_args);
                        }
                        if matches!(object.as_ref(), Expr::Ident(n) if n == "DirAccess") {
                            return call_dir_access_static(member, &evaluated_args);
                        }
                        // Godot singletons: OS / Engine / Time / JSON.
                        if matches!(object.as_ref(), Expr::Ident(n) if n == "OS") {
                            return call_os_static(member, &evaluated_args);
                        }
                        if matches!(object.as_ref(), Expr::Ident(n) if n == "Engine") {
                            return call_engine_static(member, &evaluated_args);
                        }
                        if matches!(object.as_ref(), Expr::Ident(n) if n == "Time") {
                            return call_time_static(member, &evaluated_args);
                        }
                        if matches!(object.as_ref(), Expr::Ident(n) if n == "JSON") {
                            return call_json_static(member, &evaluated_args);
                        }
                        // ClassDB reflection singleton (pat-u3l8c.21).
                        if matches!(object.as_ref(), Expr::Ident(n) if n == "ClassDB") {
                            return call_classdb_static(member, &evaluated_args);
                        }
                        // ProjectSettings singleton (pat-u3l8c.21).
                        if matches!(object.as_ref(), Expr::Ident(n) if n == "ProjectSettings") {
                            return call_projectsettings_static(member, &evaluated_args);
                        }
                        // DisplayServer singleton (pat-nzgdu.67) — headless
                        // dispatch; the web surface is wired in gdplatform (wasm32).
                        if matches!(object.as_ref(), Expr::Ident(n) if n == "DisplayServer") {
                            return call_displayserver_static(member, &evaluated_args);
                        }
                        // JavaScriptBridge singleton (pat-nzgdu.66) — headless
                        // recorder; the web-sys binding is wired in gdplatform (wasm32).
                        if matches!(object.as_ref(), Expr::Ident(n) if n == "JavaScriptBridge") {
                            return call_javascriptbridge_static(member, &evaluated_args);
                        }
                        // Marshalls + ResourceUID IO singletons (pat-nzgdu.79).
                        if matches!(object.as_ref(), Expr::Ident(n) if n == "Marshalls") {
                            return call_marshalls_static(member, &evaluated_args);
                        }
                        if matches!(object.as_ref(), Expr::Ident(n) if n == "ResourceUID") {
                            return call_resourceuid_static(member, &evaluated_args);
                        }
                        // TranslationServer singleton (pat-nzgdu.92).
                        if matches!(object.as_ref(), Expr::Ident(n) if n == "TranslationServer") {
                            return call_translationserver_static(member, &evaluated_args);
                        }
                        // CameraServer singleton (pat-nzgdu.179) — reports 0 feeds
                        // until a script adds one (web parity: Godot ships no web
                        // camera driver).
                        if matches!(object.as_ref(), Expr::Ident(n) if n == "CameraServer") {
                            return call_cameraserver_static(member, &evaluated_args);
                        }
                        // EngineDebugger singleton (pat-nzgdu.176) — backed by
                        // the real gdcore::debugger. `is_active()` is false
                        // until a session attaches, matching Godot.
                        if matches!(object.as_ref(), Expr::Ident(n) if n == "EngineDebugger") {
                            return call_enginedebugger_static(member, &evaluated_args);
                        }
                        // FramebufferCacheRD (pat-nzgdu.210) — a static helper
                        // over a RenderingDevice that does not exist here, so it
                        // answers with the documented unsupported value rather
                        // than an undefined-variable error.
                        if matches!(object.as_ref(), Expr::Ident(n) if n == "FramebufferCacheRD") {
                            return call_framebuffercacherd_static(member, &evaluated_args);
                        }
                        // Handle ClassName.new() — runtime node creation / RefCounted helpers
                        if member == "new" {
                            if let Expr::Ident(class_name) = object.as_ref() {
                                if class_name == "ConfigFile" {
                                    return Ok(Variant::ObjectId(
                                        gdresource::ConfigFile::new_handle(),
                                    ));
                                }
                                // CameraFeed is a RefCounted-style handle, not a
                                // node: it needs no scene access (pat-nzgdu.179).
                                if class_name == "CameraFeed" {
                                    return Ok(Variant::ObjectId(new_camera_feed_handle()));
                                }
                                // UndoRedo is an Object-derived handle, not a node
                                // (pat-nzgdu.155).
                                if class_name == "UndoRedo" {
                                    return Ok(Variant::ObjectId(new_undo_redo_handle()));
                                }
                                if is_node_class_name(class_name) {
                                    if let Some(ref mut access) = self.scene_access {
                                        if let Some(raw_id) =
                                            access.create_node(class_name, class_name)
                                        {
                                            return Ok(Variant::ObjectId(
                                                gdcore::id::ObjectId::from_raw(raw_id),
                                            ));
                                        }
                                    }
                                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                        format!("{class_name}.new() requires scene access"),
                                    )));
                                }
                            }
                        }
                        // `get_tree().create_timer(t)` / `get_tree().<method>()`:
                        // the SceneTree's methods dispatch to the same builtins as
                        // the bare calls, so `await get_tree().create_timer(t).timeout`
                        // (the ubiquitous Godot idiom) resolves (pat-nzgdu.6).
                        if matches!(object.as_ref(), Expr::Call { callee, .. }
                            if matches!(callee.as_ref(), Expr::Ident(n) if n == "get_tree"))
                        {
                            // SceneTree's own control surface first — change
                            // scene, quit, pause, group broadcasts (pat-0q2kx.47).
                            // These are SceneTree methods in Godot, not Node
                            // ones, so this form is the only place they resolve.
                            if let Some(result) =
                                self.try_scene_tree_method(member, &evaluated_args)?
                            {
                                return Ok(result);
                            }
                            if let Some(result) = self.try_builtin(member, &evaluated_args)? {
                                return Ok(result);
                            }
                        }
                        let obj = self.eval_expr(object)?;
                        self.call_method_on(&obj, member, &evaluated_args, object)
                    }
                    // super() — call parent class method with same name
                    Expr::SuperRef => self.call_super(&evaluated_args),
                    _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "not callable".into(),
                    ))),
                }
            }

            Expr::MemberAccess { object, member } => {
                // Handle self.member: try instance properties, then scene_access for Node props
                if matches!(object.as_ref(), Expr::SelfRef) {
                    if let Some(ref inst) = self.self_instance {
                        // Check for getter
                        if let Some(getter_name) = inst.class_def.getters.get(member).cloned() {
                            if self.current_function.as_deref() != Some(&getter_name) {
                                return self.call_user_func(&getter_name, &[]);
                            }
                        }
                        if let Some(val) = inst.properties.get(member) {
                            return Ok(val.clone());
                        }
                        // Fallback: property may be on the Node (e.g. position, rotation)
                        if let (Some(ref access), Some(node_id)) =
                            (&self.scene_access, self.current_node_id)
                        {
                            let val = access.get_node_property(node_id, member);
                            if !val.is_nil() {
                                return Ok(val);
                            }
                        }
                        return Err(RuntimeError::new(RuntimeErrorKind::UndefinedVariable(
                            member.clone(),
                        )));
                    } else {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "'self' used outside of a class instance".into(),
                        )));
                    }
                }
                // Handle static type member access (Vector2.ZERO, etc.)
                if let Expr::Ident(type_name) = object.as_ref() {
                    if let Some(v) = try_static_member(type_name, member) {
                        return Ok(v);
                    }
                }
                let obj = self.eval_expr(object)?;
                // ObjectId property access via scene_access
                if let Variant::ObjectId(oid) = &obj {
                    if let Some(ref access) = self.scene_access {
                        // `$Other.my_signal` is a `Signal` value in Godot, not a
                        // property. Checked before the property read because a
                        // signal has no property entry, so this used to return
                        // Nil and `$B.pressed.connect(cb)` was impossible
                        // (pat-0q2kx.9).
                        if access.node_has_signal(oid.raw(), member) {
                            return Ok(Variant::Signal {
                                object_id: oid.raw(),
                                name: member.clone(),
                            });
                        }
                        let val = access.get_node_property(oid.raw(), member);
                        return Ok(val);
                    }
                }
                match &obj {
                    Variant::Vector2(v) => match member.as_str() {
                        "x" => Ok(Variant::Float(v.x as f64)),
                        "y" => Ok(Variant::Float(v.y as f64)),
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                            "Vector2 has no member '{member}'"
                        )))),
                    },
                    Variant::Vector3(v) => match member.as_str() {
                        "x" => Ok(Variant::Float(v.x as f64)),
                        "y" => Ok(Variant::Float(v.y as f64)),
                        "z" => Ok(Variant::Float(v.z as f64)),
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                            "Vector3 has no member '{member}'"
                        )))),
                    },
                    Variant::Color(c) => match member.as_str() {
                        "r" => Ok(Variant::Float(c.r as f64)),
                        "g" => Ok(Variant::Float(c.g as f64)),
                        "b" => Ok(Variant::Float(c.b as f64)),
                        "a" => Ok(Variant::Float(c.a as f64)),
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                            "Color has no member '{member}'"
                        )))),
                    },
                    Variant::Dictionary(d) => {
                        // Try dictionary key first
                        if let Some(val) = d.get(member) {
                            Ok(val.clone())
                        } else if let (Some(ref access), Some(node_id)) =
                            (&self.scene_access, self.current_node_id)
                        {
                            // Fallback: self is Dictionary but property may be on the Node
                            let val = access.get_node_property(node_id, member);
                            if !val.is_nil() {
                                Ok(val)
                            } else {
                                Err(RuntimeError::new(RuntimeErrorKind::UndefinedVariable(
                                    member.clone(),
                                )))
                            }
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::UndefinedVariable(
                                member.clone(),
                            )))
                        }
                    }
                    // Rect2/Transform2D/Transform3D/Basis/Quaternion/AABB/Plane
                    // members (`t.origin`, `q.w`, `r.size`) — pat-0q2kx.50.
                    other => match crate::variant_math::member_get(other, member) {
                        Some(v) => Ok(v),
                        None => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                            "cannot access member '{member}' on {}",
                            obj.variant_type()
                        )))),
                    },
                }
            }

            Expr::Index { object, index } => {
                let obj = self.eval_expr(object)?;
                let idx = self.eval_expr(index)?;
                index_into(&obj, &idx)
            }

            Expr::ArrayLiteral(elems) => {
                let values: Vec<Variant> = elems
                    .iter()
                    .map(|e| self.eval_expr(e))
                    .collect::<Result<_, _>>()?;
                Ok(Variant::Array(values))
            }

            Expr::DictLiteral(entries) => {
                let mut map = Dictionary::new();
                for (k, v) in entries {
                    let key = self.eval_expr(k)?;
                    let val = self.eval_expr(v)?;
                    // Keys keep their evaluated Variant type (Godot dictionaries
                    // are Variant-keyed, e.g. `{1: "a", Vector2(): "b"}`).
                    map.insert(key, val);
                }
                Ok(Variant::Dictionary(map))
            }

            Expr::SelfRef => {
                // If we have scene_access + current_node_id, return ObjectId
                // so member access goes through scene_access (reads Node properties).
                if let Some(node_id) = self.current_node_id {
                    if self.scene_access.is_some() {
                        return Ok(Variant::ObjectId(gdcore::id::ObjectId::from_raw(node_id)));
                    }
                }
                // Fallback: return ClassInstance properties as Dictionary
                if let Some(ref inst) = self.self_instance {
                    Ok(Variant::Dictionary(Dictionary::from(
                        inst.properties.clone(),
                    )))
                } else {
                    Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "'self' used outside of a class instance".into(),
                    )))
                }
            }

            Expr::SuperRef => {
                // super is only meaningful as a call target; return Nil as marker
                Ok(Variant::Nil)
            }
            Expr::Ternary {
                value,
                condition,
                else_value,
            } => {
                let cond = self.eval_expr(condition)?;
                if cond.is_truthy() {
                    self.eval_expr(value)
                } else {
                    self.eval_expr(else_value)
                }
            }

            Expr::GetNode(path) => {
                if let (Some(ref access), Some(node_id)) =
                    (&self.scene_access, self.current_node_id)
                {
                    match access.get_node(node_id, path) {
                        Some(target_id) => {
                            Ok(Variant::ObjectId(gdcore::id::ObjectId::from_raw(target_id)))
                        }
                        None => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                            "node not found: {path}"
                        )))),
                    }
                } else {
                    Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "get_node() requires scene access".into(),
                    )))
                }
            }
            Expr::Lambda { params, body } => {
                // Store the body as Arc<Vec<Stmt>> so it can be recovered later.
                let body_arc: Arc<dyn std::any::Any + Send + Sync> = Arc::new(body.clone());
                Ok(Variant::Callable(Box::new(CallableRef::Lambda {
                    params: params.iter().map(|p| p.name.clone()).collect(),
                    body: body_arc,
                })))
            }

            Expr::TypeTest { value, type_name } => {
                let v = self.eval_expr(value)?;
                Ok(Variant::Bool(self.value_is_type(&v, type_name)))
            }

            Expr::Cast { value, type_name } => {
                let v = self.eval_expr(value)?;
                Ok(self.cast_value(v, type_name))
            }
        }
    }

    /// Set of built-in *value* type names understood by `is`/`as` (as opposed
    /// to native/script class names, which reference objects).
    fn is_builtin_type_name(type_name: &str) -> bool {
        matches!(
            type_name,
            "bool"
                | "int"
                | "float"
                | "String"
                | "StringName"
                | "NodePath"
                | "Array"
                | "Dictionary"
                | "Vector2"
                | "Vector3"
                | "Color"
                | "Rect2"
                | "Callable"
        ) || crate::variant_math::CONSTRUCTOR_NAMES.contains(&type_name)
    }

    /// Implements the `is` operator: does `v` have type `type_name`?
    fn value_is_type(&self, v: &Variant, type_name: &str) -> bool {
        match type_name {
            "int" => matches!(v, Variant::Int(_)),
            "float" => matches!(v, Variant::Float(_)),
            "bool" => matches!(v, Variant::Bool(_)),
            "String" => matches!(v, Variant::String(_)),
            "Array" => matches!(v, Variant::Array(_)),
            "Dictionary" => matches!(v, Variant::Dictionary(_)),
            "Vector2" => matches!(v, Variant::Vector2(_)),
            "Vector3" => matches!(v, Variant::Vector3(_)),
            "Color" => matches!(v, Variant::Color(_)),
            "Callable" => matches!(v, Variant::Callable(_)),
            "Rect2" => matches!(v, Variant::Rect2(_)),
            "Transform2D" => matches!(v, Variant::Transform2D(_)),
            "Transform3D" => matches!(v, Variant::Transform3D(_)),
            "Quaternion" => matches!(v, Variant::Quaternion(_)),
            "Basis" => matches!(v, Variant::Basis(_)),
            "AABB" => matches!(v, Variant::Aabb(_)),
            "Plane" => matches!(v, Variant::Plane(_)),
            // Objects match `Object` and any class the resource reports.
            "Object" => matches!(v, Variant::ObjectId(_) | Variant::Resource(_)),
            other => {
                if let Variant::Resource(r) = v {
                    r.class_name.as_str() == other
                } else {
                    // Native/script class checks on live nodes aren't tracked
                    // as Variant types; fall back to a type-name string match.
                    format!("{}", v.variant_type()).as_str() == other
                }
            }
        }
    }

    /// Implements the `as` operator with Godot semantics: numeric int<->float
    /// coercion, no-op for matching types, and `null` for failed object casts.
    fn cast_value(&self, v: Variant, type_name: &str) -> Variant {
        if self.value_is_type(&v, type_name) {
            return v;
        }
        match type_name {
            "int" => match &v {
                Variant::Float(f) => Variant::Int(*f as i64),
                Variant::Bool(b) => Variant::Int(*b as i64),
                _ => Variant::Nil,
            },
            "float" => match &v {
                Variant::Int(i) => Variant::Float(*i as f64),
                Variant::Bool(b) => Variant::Float(*b as i64 as f64),
                _ => Variant::Nil,
            },
            _ => {
                if Self::is_builtin_type_name(type_name) {
                    // Mismatched value-type cast: keep the value (lenient),
                    // mirroring how Godot leaves incompatible value casts.
                    v
                } else {
                    // Object/class cast that didn't match → null.
                    Variant::Nil
                }
            }
        }
    }

    fn eval_binary_op(
        &self,
        op: &BinOp,
        lhs: &Variant,
        rhs: &Variant,
    ) -> Result<Variant, RuntimeError> {
        match op {
            BinOp::Add => self.binary_add(lhs, rhs),
            BinOp::Sub => self.binary_sub(lhs, rhs),
            BinOp::Mul => self.binary_mul(lhs, rhs),
            BinOp::Div => self.binary_div(lhs, rhs),
            BinOp::Mod => self.binary_mod(lhs, rhs),
            BinOp::Pow => self.binary_pow(lhs, rhs),
            BinOp::BitAnd => self.binary_bitwise(lhs, rhs, "&", |a, b| a & b),
            BinOp::BitOr => self.binary_bitwise(lhs, rhs, "|", |a, b| a | b),
            BinOp::BitXor => self.binary_bitwise(lhs, rhs, "^", |a, b| a ^ b),
            BinOp::Shl => self.binary_bitwise(lhs, rhs, "<<", |a, b| a.wrapping_shl(b as u32)),
            BinOp::Shr => self.binary_bitwise(lhs, rhs, ">>", |a, b| a.wrapping_shr(b as u32)),
            BinOp::Eq => Ok(Variant::Bool(variant_eq(lhs, rhs))),
            BinOp::Ne => Ok(Variant::Bool(!variant_eq(lhs, rhs))),
            BinOp::Lt => self.binary_cmp(lhs, rhs, |o| o.is_lt()),
            BinOp::Gt => self.binary_cmp(lhs, rhs, |o| o.is_gt()),
            BinOp::Le => self.binary_cmp(lhs, rhs, |o| o.is_le()),
            BinOp::Ge => self.binary_cmp(lhs, rhs, |o| o.is_ge()),
            BinOp::In => match rhs {
                Variant::Array(a) => Ok(Variant::Bool(a.iter().any(|v| variant_eq(v, lhs)))),
                Variant::Dictionary(d) => Ok(Variant::Bool(d.contains_key(lhs))),
                _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                    "cannot use 'in' with {}",
                    rhs.variant_type()
                )))),
            },
            // And/Or handled via short-circuit in eval_expr
            BinOp::And | BinOp::Or => unreachable!(),
        }
    }

    // -- Arithmetic helpers -------------------------------------------------

    fn binary_add(&self, lhs: &Variant, rhs: &Variant) -> Result<Variant, RuntimeError> {
        match (lhs, rhs) {
            (Variant::Int(a), Variant::Int(b)) => Ok(Variant::Int(a + b)),
            (Variant::Float(a), Variant::Float(b)) => Ok(Variant::Float(a + b)),
            (Variant::Int(a), Variant::Float(b)) => Ok(Variant::Float(*a as f64 + b)),
            (Variant::Float(a), Variant::Int(b)) => Ok(Variant::Float(a + *b as f64)),
            (Variant::String(a), Variant::String(b)) => Ok(Variant::String(format!("{a}{b}"))),
            (Variant::Vector2(a), Variant::Vector2(b)) => Ok(Variant::Vector2(*a + *b)),
            (Variant::Vector3(a), Variant::Vector3(b)) => Ok(Variant::Vector3(*a + *b)),
            _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                "cannot add {} and {}",
                lhs.variant_type(),
                rhs.variant_type()
            )))),
        }
    }

    fn binary_sub(&self, lhs: &Variant, rhs: &Variant) -> Result<Variant, RuntimeError> {
        match (lhs, rhs) {
            (Variant::Int(a), Variant::Int(b)) => Ok(Variant::Int(a - b)),
            (Variant::Float(a), Variant::Float(b)) => Ok(Variant::Float(a - b)),
            (Variant::Int(a), Variant::Float(b)) => Ok(Variant::Float(*a as f64 - b)),
            (Variant::Float(a), Variant::Int(b)) => Ok(Variant::Float(a - *b as f64)),
            (Variant::Vector2(a), Variant::Vector2(b)) => Ok(Variant::Vector2(*a - *b)),
            (Variant::Vector3(a), Variant::Vector3(b)) => Ok(Variant::Vector3(*a - *b)),
            _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                "cannot subtract {} from {}",
                rhs.variant_type(),
                lhs.variant_type()
            )))),
        }
    }

    fn binary_mul(&self, lhs: &Variant, rhs: &Variant) -> Result<Variant, RuntimeError> {
        match (lhs, rhs) {
            (Variant::Int(a), Variant::Int(b)) => Ok(Variant::Int(a * b)),
            (Variant::Float(a), Variant::Float(b)) => Ok(Variant::Float(a * b)),
            (Variant::Int(a), Variant::Float(b)) => Ok(Variant::Float(*a as f64 * b)),
            (Variant::Float(a), Variant::Int(b)) => Ok(Variant::Float(a * *b as f64)),
            // Vector2 * scalar and scalar * Vector2
            (Variant::Vector2(v), Variant::Float(s)) => Ok(Variant::Vector2(*v * *s as f32)),
            (Variant::Vector2(v), Variant::Int(s)) => Ok(Variant::Vector2(*v * *s as f32)),
            (Variant::Float(s), Variant::Vector2(v)) => Ok(Variant::Vector2(*v * *s as f32)),
            (Variant::Int(s), Variant::Vector2(v)) => Ok(Variant::Vector2(*v * *s as f32)),
            // Vector3 * scalar and scalar * Vector3
            (Variant::Vector3(v), Variant::Float(s)) => Ok(Variant::Vector3(*v * *s as f32)),
            (Variant::Vector3(v), Variant::Int(s)) => Ok(Variant::Vector3(*v * *s as f32)),
            (Variant::Float(s), Variant::Vector3(v)) => Ok(Variant::Vector3(*v * *s as f32)),
            (Variant::Int(s), Variant::Vector3(v)) => Ok(Variant::Vector3(*v * *s as f32)),
            // Transform/Basis/Quaternion products: `t * point`, `a * b`
            // composition, `q * v` rotation (pat-0q2kx.50).
            _ => match crate::variant_math::mul(lhs, rhs) {
                Some(v) => Ok(v),
                None => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                    "cannot multiply {} and {}",
                    lhs.variant_type(),
                    rhs.variant_type()
                )))),
            },
        }
    }

    fn binary_div(&self, lhs: &Variant, rhs: &Variant) -> Result<Variant, RuntimeError> {
        match (lhs, rhs) {
            (Variant::Int(a), Variant::Int(b)) => {
                if *b == 0 {
                    return Err(RuntimeError::new(RuntimeErrorKind::DivisionByZero));
                }
                Ok(Variant::Int(a / b))
            }
            (Variant::Float(a), Variant::Float(b)) => {
                if *b == 0.0 {
                    return Err(RuntimeError::new(RuntimeErrorKind::DivisionByZero));
                }
                Ok(Variant::Float(a / b))
            }
            (Variant::Int(a), Variant::Float(b)) => {
                if *b == 0.0 {
                    return Err(RuntimeError::new(RuntimeErrorKind::DivisionByZero));
                }
                Ok(Variant::Float(*a as f64 / b))
            }
            (Variant::Float(a), Variant::Int(b)) => {
                if *b == 0 {
                    return Err(RuntimeError::new(RuntimeErrorKind::DivisionByZero));
                }
                Ok(Variant::Float(a / *b as f64))
            }
            // Vector2 / scalar
            (Variant::Vector2(v), Variant::Float(s)) => {
                if *s == 0.0 {
                    return Err(RuntimeError::new(RuntimeErrorKind::DivisionByZero));
                }
                Ok(Variant::Vector2(*v / *s as f32))
            }
            (Variant::Vector2(v), Variant::Int(s)) => {
                if *s == 0 {
                    return Err(RuntimeError::new(RuntimeErrorKind::DivisionByZero));
                }
                Ok(Variant::Vector2(*v / *s as f32))
            }
            // Vector3 / scalar
            (Variant::Vector3(v), Variant::Float(s)) => {
                if *s == 0.0 {
                    return Err(RuntimeError::new(RuntimeErrorKind::DivisionByZero));
                }
                Ok(Variant::Vector3(*v / *s as f32))
            }
            (Variant::Vector3(v), Variant::Int(s)) => {
                if *s == 0 {
                    return Err(RuntimeError::new(RuntimeErrorKind::DivisionByZero));
                }
                Ok(Variant::Vector3(*v / *s as f32))
            }
            _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                "cannot divide {} by {}",
                lhs.variant_type(),
                rhs.variant_type()
            )))),
        }
    }

    fn binary_mod(&self, lhs: &Variant, rhs: &Variant) -> Result<Variant, RuntimeError> {
        if let Variant::String(fmt) = lhs {
            let values: Vec<Variant> = match rhs {
                Variant::Array(a) => a.clone(),
                other => vec![other.clone()],
            };
            return Ok(Variant::String(string_format(fmt, &values)));
        }
        match (lhs, rhs) {
            (Variant::Int(a), Variant::Int(b)) => {
                if *b == 0 {
                    return Err(RuntimeError::new(RuntimeErrorKind::DivisionByZero));
                }
                Ok(Variant::Int(a % b))
            }
            (Variant::Float(a), Variant::Float(b)) => {
                if *b == 0.0 {
                    return Err(RuntimeError::new(RuntimeErrorKind::DivisionByZero));
                }
                Ok(Variant::Float(a % b))
            }
            (Variant::Int(a), Variant::Float(b)) => {
                if *b == 0.0 {
                    return Err(RuntimeError::new(RuntimeErrorKind::DivisionByZero));
                }
                Ok(Variant::Float(*a as f64 % b))
            }
            (Variant::Float(a), Variant::Int(b)) => {
                if *b == 0 {
                    return Err(RuntimeError::new(RuntimeErrorKind::DivisionByZero));
                }
                Ok(Variant::Float(a % *b as f64))
            }
            _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                "cannot modulo {} by {}",
                lhs.variant_type(),
                rhs.variant_type()
            )))),
        }
    }

    fn binary_pow(&self, lhs: &Variant, rhs: &Variant) -> Result<Variant, RuntimeError> {
        match (lhs, rhs) {
            // int ** int → int for non-negative exponents, else float.
            (Variant::Int(a), Variant::Int(b)) => {
                if *b >= 0 {
                    match u32::try_from(*b).ok().and_then(|e| a.checked_pow(e)) {
                        Some(r) => Ok(Variant::Int(r)),
                        None => Ok(Variant::Float((*a as f64).powf(*b as f64))),
                    }
                } else {
                    Ok(Variant::Float((*a as f64).powf(*b as f64)))
                }
            }
            // Any float operand → float.
            (Variant::Int(_) | Variant::Float(_), Variant::Int(_) | Variant::Float(_)) => {
                Ok(Variant::Float(to_float(lhs)?.powf(to_float(rhs)?)))
            }
            _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                "cannot raise {} to {}",
                lhs.variant_type(),
                rhs.variant_type()
            )))),
        }
    }

    fn binary_bitwise(
        &self,
        lhs: &Variant,
        rhs: &Variant,
        sym: &str,
        op: fn(i64, i64) -> i64,
    ) -> Result<Variant, RuntimeError> {
        match (lhs, rhs) {
            (Variant::Int(a), Variant::Int(b)) => Ok(Variant::Int(op(*a, *b))),
            _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                "bitwise '{sym}' requires int operands, got {} and {}",
                lhs.variant_type(),
                rhs.variant_type()
            )))),
        }
    }

    fn binary_cmp(
        &self,
        lhs: &Variant,
        rhs: &Variant,
        pred: fn(std::cmp::Ordering) -> bool,
    ) -> Result<Variant, RuntimeError> {
        let ord = match (lhs, rhs) {
            (Variant::Int(a), Variant::Int(b)) => a.cmp(b),
            (Variant::Float(a), Variant::Float(b)) => {
                a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
            }
            (Variant::Int(a), Variant::Float(b)) => (*a as f64)
                .partial_cmp(b)
                .unwrap_or(std::cmp::Ordering::Equal),
            (Variant::Float(a), Variant::Int(b)) => a
                .partial_cmp(&(*b as f64))
                .unwrap_or(std::cmp::Ordering::Equal),
            (Variant::String(a), Variant::String(b)) => a.cmp(b),
            _ => {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                    "cannot compare {} and {}",
                    lhs.variant_type(),
                    rhs.variant_type()
                ))));
            }
        };
        Ok(Variant::Bool(pred(ord)))
    }

    // -----------------------------------------------------------------------
    // Function calls
    // -----------------------------------------------------------------------

    /// A string argument to a SceneTree builtin (scene path, group name, method
    /// name, property name).
    fn scene_tree_arg_string(
        name: &str,
        args: &[Variant],
        index: usize,
    ) -> Result<String, RuntimeError> {
        match args.get(index) {
            Some(Variant::String(s)) => Ok(s.clone()),
            Some(Variant::StringName(s)) => Ok(s.as_str().to_string()),
            Some(Variant::NodePath(p)) => Ok(p.to_string()),
            other => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                "{name}() expects a string for argument {index}, got {other:?}"
            )))),
        }
    }

    /// An integer argument to a SceneTree builtin, defaulting when absent —
    /// Godot's `quit(exit_code = 0)` and the group-call flag word.
    fn scene_tree_arg_int(
        name: &str,
        args: &[Variant],
        index: usize,
        default: i64,
    ) -> Result<i64, RuntimeError> {
        match args.get(index) {
            None | Some(Variant::Nil) => Ok(default),
            Some(Variant::Int(i)) => Ok(*i),
            Some(Variant::Float(f)) => Ok(*f as i64),
            Some(Variant::Bool(b)) => Ok(i64::from(*b)),
            other => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                "{name}() expects an int for argument {index}, got {other:?}"
            )))),
        }
    }

    /// `SceneTree`'s control surface, as reached by `get_tree().<method>()`
    /// (pat-0q2kx.47).
    ///
    /// Every method here already had a working, unit-tested implementation on
    /// `SceneTree`; none of them was reachable from a script, so a scene could
    /// not change, quit, pause, or broadcast to a group — `get_tree().quit()`
    /// died on `UndefinedFunction`.
    ///
    /// Deliberately **not** part of [`try_builtin`]. That runs ahead of user
    /// functions for a bare call, so exposing names like `quit` or `has_group`
    /// there would shadow a script's own `func quit()`. These live only on
    /// `SceneTree` in Godot, and only on this dispatch path here.
    ///
    /// `Ok(None)` means "not a SceneTree method", so the caller falls through.
    fn try_scene_tree_method(
        &mut self,
        name: &str,
        args: &[Variant],
    ) -> Result<Option<Variant>, RuntimeError> {
        // Godot's Error enum: OK, and ERR_CANT_OPEN for a scene that will not
        // load. The idiomatic call discards it, but returning the real code
        // keeps `if get_tree().change_scene_to_file(p) != OK:` meaningful.
        const OK: i64 = 0;
        const ERR_CANT_OPEN: i64 = 19;

        let result = match name {
            "change_scene_to_file" => {
                Self::require_args("change_scene_to_file", args, 1)?;
                let path = Self::scene_tree_arg_string("change_scene_to_file", args, 0)?;
                let ok = match self.scene_access {
                    Some(ref mut access) => access.change_scene_to_file(&path),
                    None => false,
                };
                Variant::Int(if ok { OK } else { ERR_CANT_OPEN })
            }
            "reload_current_scene" => {
                let ok = match self.scene_access {
                    Some(ref mut access) => access.reload_current_scene(),
                    None => false,
                };
                Variant::Int(if ok { OK } else { ERR_CANT_OPEN })
            }
            "quit" => {
                let code = Self::scene_tree_arg_int("quit", args, 0, 0)?;
                if let Some(ref mut access) = self.scene_access {
                    access.quit(code);
                }
                Variant::Nil
            }
            "set_pause" => {
                Self::require_args("set_pause", args, 1)?;
                let paused = args[0].is_truthy();
                if let Some(ref mut access) = self.scene_access {
                    access.set_tree_paused(paused);
                }
                Variant::Nil
            }
            "is_paused" => Variant::Bool(
                self.scene_access
                    .as_ref()
                    .map(|a| a.is_tree_paused())
                    .unwrap_or(false),
            ),
            "get_root" => self
                .scene_access
                .as_ref()
                .and_then(|a| a.tree_root())
                .map(|raw| Variant::ObjectId(gdcore::id::ObjectId::from_raw(raw)))
                .unwrap_or(Variant::Nil),
            "get_node_count" => Variant::Int(
                self.scene_access
                    .as_ref()
                    .map(|a| a.tree_node_count())
                    .unwrap_or(0),
            ),
            "get_nodes_in_group" => {
                Self::require_args("get_nodes_in_group", args, 1)?;
                let group = Self::scene_tree_arg_string("get_nodes_in_group", args, 0)?;
                let ids = self
                    .scene_access
                    .as_ref()
                    .map(|a| a.nodes_in_group(&group))
                    .unwrap_or_default();
                Variant::Array(
                    ids.into_iter()
                        .map(|raw| Variant::ObjectId(gdcore::id::ObjectId::from_raw(raw)))
                        .collect(),
                )
            }
            "get_first_node_in_group" => {
                Self::require_args("get_first_node_in_group", args, 1)?;
                let group = Self::scene_tree_arg_string("get_first_node_in_group", args, 0)?;
                self.scene_access
                    .as_ref()
                    .and_then(|a| a.first_node_in_group(&group))
                    .map(|raw| Variant::ObjectId(gdcore::id::ObjectId::from_raw(raw)))
                    .unwrap_or(Variant::Nil)
            }
            "has_group" => {
                Self::require_args("has_group", args, 1)?;
                let group = Self::scene_tree_arg_string("has_group", args, 0)?;
                Variant::Bool(
                    self.scene_access
                        .as_ref()
                        .map(|a| a.tree_has_group(&group))
                        .unwrap_or(false),
                )
            }
            // `call_group(group, method, ...)` and
            // `call_group_flags(flags, group, method, ...)` differ only by the
            // leading flag word, and both forward any trailing arguments.
            "call_group" | "call_group_flags" => {
                let flagged = name == "call_group_flags";
                let base = usize::from(flagged);
                if args.len() < base + 2 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                        "{name}() takes at least {} argument(s), got {}",
                        base + 2,
                        args.len()
                    ))));
                }
                let flags = if flagged {
                    Self::scene_tree_arg_int(name, args, 0, 0)?
                } else {
                    0
                };
                let group = Self::scene_tree_arg_string(name, args, base)?;
                let method = Self::scene_tree_arg_string(name, args, base + 1)?;
                let forwarded = args[base + 2..].to_vec();
                if let Some(ref mut access) = self.scene_access {
                    access.call_group_flags(flags, &group, &method, &forwarded);
                }
                Variant::Nil
            }
            "set_group" => {
                Self::require_args("set_group", args, 3)?;
                let group = Self::scene_tree_arg_string("set_group", args, 0)?;
                let property = Self::scene_tree_arg_string("set_group", args, 1)?;
                let value = args[2].clone();
                if let Some(ref mut access) = self.scene_access {
                    access.set_group(&group, &property, value);
                }
                Variant::Nil
            }
            "notify_group" => {
                Self::require_args("notify_group", args, 2)?;
                let group = Self::scene_tree_arg_string("notify_group", args, 0)?;
                let what = Self::scene_tree_arg_int("notify_group", args, 1, 0)?;
                if let Some(ref mut access) = self.scene_access {
                    access.notify_group(&group, what);
                }
                Variant::Nil
            }
            _ => return Ok(None),
        };
        Ok(Some(result))
    }

    fn try_builtin(
        &mut self,
        name: &str,
        args: &[Variant],
    ) -> Result<Option<Variant>, RuntimeError> {
        match name {
            "print" => {
                let line: Vec<String> = args.iter().map(|a| format!("{a}")).collect();
                self.output.push(line.join(" "));
                Ok(Some(Variant::Nil))
            }
            // Awaitable factories (pat-p0s82.7 / pat-z35r2.3).
            // Godot: `await get_tree().create_timer(t).timeout`
            // Patina: `await create_timer(t)` or `await create_timer(t).timeout`
            "create_timer" | "wait_seconds" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "create_timer() takes exactly 1 argument".into(),
                    )));
                }
                let secs = match &args[0] {
                    Variant::Float(f) => *f,
                    Variant::Int(i) => *i as f64,
                    other => {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                            "create_timer() expects number, got {}",
                            other.variant_type()
                        ))))
                    }
                };
                // Prefer SceneTree-backed timer when scene access is available.
                if let Some(ref mut access) = self.scene_access {
                    if let Some((id, signal_name)) = access.create_timer(secs) {
                        let mut d = Dictionary::new();
                        d.insert("__await_kind", Variant::String("signal".into()));
                        d.insert("name", Variant::String(signal_name.clone()));
                        d.insert("timer_id", Variant::Int(id as i64));
                        // Property `.timeout` is itself awaitable (Godot idiom).
                        d.insert(
                            "timeout",
                            crate::coroutine::AwaitTarget::signal_variant(&signal_name),
                        );
                        d.insert("time_left", Variant::Float(secs));
                        return Ok(Some(Variant::Dictionary(d)));
                    }
                }
                Ok(Some(crate::coroutine::AwaitTarget::timer_variant(secs)))
            }
            // Godot: `Node.create_tween()` returns an already-running Tween
            // bound to the tree. The handle is a `Variant::ObjectId`, so the
            // fluent chain `create_tween().tween_property(...)` dispatches
            // through `call_method_on`'s tween arm (pat-0q2kx.4). Before this,
            // `create_tween` hit the unknown-function path — scripts could not
            // make a tween at all, and `SceneTree::create_tween` had only test
            // callers.
            "create_tween" => {
                let node_id = match self.current_node_id {
                    Some(id) => id,
                    // Godot returns null for a node outside the tree; a bare
                    // interpreter with no owning node is the same situation.
                    None => return Ok(Some(Variant::Nil)),
                };
                if let Some(ref mut access) = self.scene_access {
                    if let Some(raw) = access.create_tween(node_id) {
                        return Ok(Some(Variant::ObjectId(gdcore::id::ObjectId::from_raw(raw))));
                    }
                }
                Ok(Some(Variant::Nil))
            }
            "wait_signal" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "wait_signal() takes exactly 1 argument".into(),
                    )));
                }
                let name = match &args[0] {
                    Variant::String(s) => s.clone(),
                    other => {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                            "wait_signal() expects string, got {}",
                            other.variant_type()
                        ))))
                    }
                };
                Ok(Some(crate::coroutine::AwaitTarget::signal_variant(&name)))
            }
            "wait_frames" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "wait_frames() takes exactly 1 argument".into(),
                    )));
                }
                let n = match &args[0] {
                    Variant::Int(i) => *i,
                    Variant::Float(f) => *f as i64,
                    other => {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                            "wait_frames() expects number, got {}",
                            other.variant_type()
                        ))))
                    }
                };
                Ok(Some(crate::coroutine::AwaitTarget::frames_variant(n)))
            }
            "str" => {
                // Variadic (Godot `str(...)`): concatenate the string form of
                // every argument with no separator. `str()` yields "".
                let joined: String = args.iter().map(|a| format!("{a}")).collect();
                Ok(Some(Variant::String(joined)))
            }
            // i18n builtins (pat-nzgdu.92). `tr(message[, context])` returns the
            // active-locale translation of `message`, or `message` unchanged when
            // there is no catalog entry (Godot's fallback). `tr_n` is the
            // plural-aware form.
            "tr" => {
                let key = match args.first() {
                    Some(Variant::String(s)) => s.clone(),
                    Some(Variant::StringName(sn)) => sn.as_str().to_string(),
                    _ => {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "tr() takes a message string".into(),
                        )))
                    }
                };
                Ok(Some(Variant::String(tr_lookup(&key))))
            }
            "tr_n" => {
                // tr_n(message, plural_message, n[, context]).
                if args.len() < 3 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "tr_n() takes (message, plural_message, n)".into(),
                    )));
                }
                let key = match &args[0] {
                    Variant::String(s) => s.clone(),
                    Variant::StringName(sn) => sn.as_str().to_string(),
                    _ => {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "tr_n() message must be a string".into(),
                        )))
                    }
                };
                let plural = match &args[1] {
                    Variant::String(s) => s.clone(),
                    Variant::StringName(sn) => sn.as_str().to_string(),
                    _ => {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "tr_n() plural_message must be a string".into(),
                        )))
                    }
                };
                let n = to_int(&args[2])?;
                Ok(Some(Variant::String(tr_lookup_plural(&key, &plural, n))))
            }
            "int" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "int() takes exactly 1 argument".into(),
                    )));
                }
                match &args[0] {
                    Variant::Int(i) => Ok(Some(Variant::Int(*i))),
                    Variant::Float(f) => Ok(Some(Variant::Int(*f as i64))),
                    Variant::Bool(b) => Ok(Some(Variant::Int(if *b { 1 } else { 0 }))),
                    Variant::String(s) => {
                        let i: i64 = s.parse().map_err(|_| {
                            RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                                "cannot convert '{s}' to int"
                            )))
                        })?;
                        Ok(Some(Variant::Int(i)))
                    }
                    other => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                        "cannot convert {} to int",
                        other.variant_type()
                    )))),
                }
            }
            "float" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "float() takes exactly 1 argument".into(),
                    )));
                }
                match &args[0] {
                    Variant::Float(f) => Ok(Some(Variant::Float(*f))),
                    Variant::Int(i) => Ok(Some(Variant::Float(*i as f64))),
                    Variant::Bool(b) => Ok(Some(Variant::Float(if *b { 1.0 } else { 0.0 }))),
                    Variant::String(s) => {
                        let f: f64 = s.parse().map_err(|_| {
                            RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                                "cannot convert '{s}' to float"
                            )))
                        })?;
                        Ok(Some(Variant::Float(f)))
                    }
                    other => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                        "cannot convert {} to float",
                        other.variant_type()
                    )))),
                }
            }
            "len" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "len() takes exactly 1 argument".into(),
                    )));
                }
                match &args[0] {
                    // Character count, not byte count (multibyte-safe).
                    Variant::String(s) => Ok(Some(Variant::Int(s.chars().count() as i64))),
                    Variant::Array(a) => Ok(Some(Variant::Int(a.len() as i64))),
                    Variant::Dictionary(d) => Ok(Some(Variant::Int(d.len() as i64))),
                    other => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                        "len() not supported for {}",
                        other.variant_type()
                    )))),
                }
            }
            "range" => match args.len() {
                1 => match &args[0] {
                    Variant::Int(n) => {
                        let arr: Vec<Variant> = (0..*n).map(Variant::Int).collect();
                        Ok(Some(Variant::Array(arr)))
                    }
                    _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "range() argument must be int".into(),
                    ))),
                },
                2 => match (&args[0], &args[1]) {
                    (Variant::Int(start), Variant::Int(end)) => {
                        let arr: Vec<Variant> = (*start..*end).map(Variant::Int).collect();
                        Ok(Some(Variant::Array(arr)))
                    }
                    _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "range() arguments must be int".into(),
                    ))),
                },
                3 => match (&args[0], &args[1], &args[2]) {
                    (Variant::Int(start), Variant::Int(end), Variant::Int(step)) => {
                        if *step == 0 {
                            return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "range() step cannot be zero".into(),
                            )));
                        }
                        let mut arr = Vec::new();
                        let mut i = *start;
                        if *step > 0 {
                            while i < *end {
                                arr.push(Variant::Int(i));
                                i += step;
                            }
                        } else {
                            while i > *end {
                                arr.push(Variant::Int(i));
                                i += step;
                            }
                        }
                        Ok(Some(Variant::Array(arr)))
                    }
                    _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "range() arguments must be int".into(),
                    ))),
                },
                _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "range() takes 1, 2, or 3 arguments".into(),
                ))),
            },
            "typeof" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "typeof() takes exactly 1 argument".into(),
                    )));
                }
                // Godot `typeof()` returns the `Variant.Type` integer (comparable
                // against the `TYPE_*` @GlobalScope constants). (pat-nzgdu.7)
                Ok(Some(Variant::Int(
                    crate::global_constants::variant_type_code(&args[0]),
                )))
            }
            "abs" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "abs() takes exactly 1 argument".into(),
                    )));
                }
                match &args[0] {
                    Variant::Int(i) => Ok(Some(Variant::Int(i.abs()))),
                    Variant::Float(f) => Ok(Some(Variant::Float(f.abs()))),
                    // Componentwise on vectors (Godot `Vector2.abs()` etc.).
                    Variant::Vector2(v) => Ok(Some(Variant::Vector2(gdcore::math::Vector2::new(
                        v.x.abs(),
                        v.y.abs(),
                    )))),
                    Variant::Vector3(v) => Ok(Some(Variant::Vector3(gdcore::math::Vector3::new(
                        v.x.abs(),
                        v.y.abs(),
                        v.z.abs(),
                    )))),
                    other => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                        "abs() not supported for {}",
                        other.variant_type()
                    )))),
                }
            }
            // Godot exposes this as `Viewport.set_input_as_handled()`; until a
            // Viewport node exists it is reachable as a bare call from an input
            // virtual, marking the in-flight event handled so later propagation
            // stages are skipped. A no-op when the host dispatches no input.
            "set_input_as_handled" => {
                if !args.is_empty() {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "set_input_as_handled() takes no arguments".into(),
                    )));
                }
                if let Some(ref mut access) = self.scene_access {
                    access.set_input_as_handled();
                }
                Ok(Some(Variant::Nil))
            }
            "preload" | "load" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "preload/load takes 1 argument".into(),
                    )));
                }
                match &args[0] {
                    Variant::String(path) => {
                        // Text resources are parsed off disk so the returned
                        // Resource carries the file's real class name and
                        // properties (`load("x.tres").get("prop")`). An
                        // unreadable or malformed file falls through to the
                        // path-only placeholder below.
                        if path.ends_with(".tres") || path.ends_with(".res") {
                            if let Some(loaded) = load_text_resource(path) {
                                return Ok(Some(Variant::Resource(Box::new(loaded))));
                            }
                        }
                        // Return a Resource variant with the path and inferred class name
                        let class_name = if path.ends_with(".tres") || path.ends_with(".res") {
                            "Resource"
                        } else if path.ends_with(".tscn") || path.ends_with(".scn") {
                            "PackedScene"
                        } else if path.ends_with(".png")
                            || path.ends_with(".jpg")
                            || path.ends_with(".svg")
                        {
                            "Texture2D"
                        } else if path.ends_with(".wav") || path.ends_with(".ogg") {
                            "AudioStream"
                        } else if path.ends_with(".ttf") || path.ends_with(".otf") {
                            "Font"
                        } else if path.ends_with(".gd") {
                            "GDScript"
                        } else if path.ends_with(".gdshader") || path.ends_with(".gdshaderinc") {
                            "Shader"
                        } else {
                            "Resource"
                        };
                        Ok(Some(Variant::Resource(Box::new(ResourceRef {
                            path: path.clone(),
                            class_name: class_name.to_string(),
                            properties: HashMap::new(),
                        }))))
                    }
                    // If already a Resource, pass through
                    Variant::Resource(_) => Ok(Some(args[0].clone())),
                    _ => Ok(Some(args[0].clone())),
                }
            }
            "min" => {
                // Godot `min()` is variadic (2+ args). Fold pairwise using the
                // existing 2-arg semantics so int/float typing is preserved
                // exactly as before for the common 2-arg case.
                if args.len() < 2 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "min() takes at least 2 arguments".into(),
                    )));
                }
                let mut acc = args[0].clone();
                for a in &args[1..] {
                    acc = float_or_int(to_float(&acc)?.min(to_float(a)?), &acc, a);
                }
                Ok(Some(acc))
            }
            "max" => {
                if args.len() < 2 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "max() takes at least 2 arguments".into(),
                    )));
                }
                let mut acc = args[0].clone();
                for a in &args[1..] {
                    acc = float_or_int(to_float(&acc)?.max(to_float(a)?), &acc, a);
                }
                Ok(Some(acc))
            }
            "clamp" => {
                if args.len() != 3 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "clamp() takes 3 arguments".into(),
                    )));
                }
                // Componentwise on vectors (Godot `Vector2.clamp(min, max)`);
                // `.max(lo).min(hi)` avoids f32::clamp's lo>hi panic.
                match (&args[0], &args[1], &args[2]) {
                    (Variant::Vector2(v), Variant::Vector2(lo), Variant::Vector2(hi)) => {
                        Ok(Some(Variant::Vector2(gdcore::math::Vector2::new(
                            v.x.max(lo.x).min(hi.x),
                            v.y.max(lo.y).min(hi.y),
                        ))))
                    }
                    (Variant::Vector3(v), Variant::Vector3(lo), Variant::Vector3(hi)) => {
                        Ok(Some(Variant::Vector3(gdcore::math::Vector3::new(
                            v.x.max(lo.x).min(hi.x),
                            v.y.max(lo.y).min(hi.y),
                            v.z.max(lo.z).min(hi.z),
                        ))))
                    }
                    _ => {
                        let v = to_float(&args[0])?;
                        let lo = to_float(&args[1])?;
                        let hi = to_float(&args[2])?;
                        Ok(Some(float_or_int(v.max(lo).min(hi), &args[0], &args[1])))
                    }
                }
            }
            "lerp" => {
                if args.len() != 3 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "lerp() takes 3 arguments".into(),
                    )));
                }
                let a = to_float(&args[0])?;
                let b = to_float(&args[1])?;
                let t = to_float(&args[2])?;
                Ok(Some(Variant::Float(a + (b - a) * t)))
            }
            "sign" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "sign() takes 1 argument".into(),
                    )));
                }
                // Componentwise on vectors (Godot `Vector2.sign()`).
                let fsign = |x: f32| {
                    if x > 0.0 {
                        1.0
                    } else if x < 0.0 {
                        -1.0
                    } else {
                        0.0
                    }
                };
                match &args[0] {
                    Variant::Vector2(v) => Ok(Some(Variant::Vector2(gdcore::math::Vector2::new(
                        fsign(v.x),
                        fsign(v.y),
                    )))),
                    Variant::Vector3(v) => Ok(Some(Variant::Vector3(gdcore::math::Vector3::new(
                        fsign(v.x),
                        fsign(v.y),
                        fsign(v.z),
                    )))),
                    _ => {
                        let v = to_float(&args[0])?;
                        let s = if v > 0.0 {
                            1.0
                        } else if v < 0.0 {
                            -1.0
                        } else {
                            0.0
                        };
                        Ok(Some(float_or_int(s, &args[0], &args[0])))
                    }
                }
            }
            "floor" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "floor() takes 1 argument".into(),
                    )));
                }
                Ok(Some(Variant::Float(to_float(&args[0])?.floor())))
            }
            "ceil" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "ceil() takes 1 argument".into(),
                    )));
                }
                Ok(Some(Variant::Float(to_float(&args[0])?.ceil())))
            }
            "round" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "round() takes 1 argument".into(),
                    )));
                }
                Ok(Some(Variant::Float(to_float(&args[0])?.round())))
            }
            // Typed integer/float math variants (pat-nzgdu.39). Godot's `*i`
            // family forces an int return, `*f` forces a float; they are
            // pervasive in real 4.x scripts, so their absence throws
            // UndefinedFunction on common code.
            "floori" => {
                Self::require_args("floori", args, 1)?;
                Ok(Some(Variant::Int(to_float(&args[0])?.floor() as i64)))
            }
            "floorf" => {
                Self::require_args("floorf", args, 1)?;
                Ok(Some(Variant::Float(to_float(&args[0])?.floor())))
            }
            "ceili" => {
                Self::require_args("ceili", args, 1)?;
                Ok(Some(Variant::Int(to_float(&args[0])?.ceil() as i64)))
            }
            "ceilf" => {
                Self::require_args("ceilf", args, 1)?;
                Ok(Some(Variant::Float(to_float(&args[0])?.ceil())))
            }
            "roundi" => {
                Self::require_args("roundi", args, 1)?;
                Ok(Some(Variant::Int(to_float(&args[0])?.round() as i64)))
            }
            "roundf" => {
                Self::require_args("roundf", args, 1)?;
                Ok(Some(Variant::Float(to_float(&args[0])?.round())))
            }
            "absi" => {
                Self::require_args("absi", args, 1)?;
                Ok(Some(Variant::Int(to_int(&args[0])?.abs())))
            }
            "absf" => {
                Self::require_args("absf", args, 1)?;
                Ok(Some(Variant::Float(to_float(&args[0])?.abs())))
            }
            "signi" => {
                Self::require_args("signi", args, 1)?;
                let v = to_float(&args[0])?;
                Ok(Some(Variant::Int(if v > 0.0 {
                    1
                } else if v < 0.0 {
                    -1
                } else {
                    0
                })))
            }
            "signf" => {
                Self::require_args("signf", args, 1)?;
                let v = to_float(&args[0])?;
                Ok(Some(Variant::Float(if v > 0.0 {
                    1.0
                } else if v < 0.0 {
                    -1.0
                } else {
                    0.0
                })))
            }
            "snappedi" => {
                Self::require_args("snappedi", args, 2)?;
                let value = to_float(&args[0])?;
                let step = to_float(&args[1])?;
                let r = if step != 0.0 {
                    (value / step + 0.5).floor() * step
                } else {
                    value
                };
                Ok(Some(Variant::Int(r as i64)))
            }
            "snappedf" => {
                Self::require_args("snappedf", args, 2)?;
                let value = to_float(&args[0])?;
                let step = to_float(&args[1])?;
                let r = if step != 0.0 {
                    (value / step + 0.5).floor() * step
                } else {
                    value
                };
                Ok(Some(Variant::Float(r)))
            }
            "maxi" => {
                Self::require_args("maxi", args, 2)?;
                Ok(Some(Variant::Int(to_int(&args[0])?.max(to_int(&args[1])?))))
            }
            "mini" => {
                Self::require_args("mini", args, 2)?;
                Ok(Some(Variant::Int(to_int(&args[0])?.min(to_int(&args[1])?))))
            }
            "maxf" => {
                Self::require_args("maxf", args, 2)?;
                Ok(Some(Variant::Float(
                    to_float(&args[0])?.max(to_float(&args[1])?),
                )))
            }
            "minf" => {
                Self::require_args("minf", args, 2)?;
                Ok(Some(Variant::Float(
                    to_float(&args[0])?.min(to_float(&args[1])?),
                )))
            }
            "clampi" => {
                Self::require_args("clampi", args, 3)?;
                let v = to_int(&args[0])?;
                let lo = to_int(&args[1])?;
                let hi = to_int(&args[2])?;
                Ok(Some(Variant::Int(v.max(lo).min(hi))))
            }
            "clampf" => {
                Self::require_args("clampf", args, 3)?;
                let v = to_float(&args[0])?;
                let lo = to_float(&args[1])?;
                let hi = to_float(&args[2])?;
                Ok(Some(Variant::Float(v.max(lo).min(hi))))
            }
            "lerpf" => {
                Self::require_args("lerpf", args, 3)?;
                let a = to_float(&args[0])?;
                let b = to_float(&args[1])?;
                let t = to_float(&args[2])?;
                Ok(Some(Variant::Float(a + (b - a) * t)))
            }
            // Variant-generic `wrap(value, min, max)`: integer operands wrap as
            // wrapi, otherwise as wrapf (wrapi/wrapf themselves already exist).
            "wrap" => {
                Self::require_args("wrap", args, 3)?;
                let all_int = matches!(&args[0], Variant::Int(_))
                    && matches!(&args[1], Variant::Int(_))
                    && matches!(&args[2], Variant::Int(_));
                if all_int {
                    let value = to_int(&args[0])?;
                    let min = to_int(&args[1])?;
                    let max = to_int(&args[2])?;
                    let range = max - min;
                    let result = if range == 0 {
                        min
                    } else {
                        min + (((value - min) % range) + range) % range
                    };
                    Ok(Some(Variant::Int(result)))
                } else {
                    let value = to_float(&args[0])?;
                    let min = to_float(&args[1])?;
                    let max = to_float(&args[2])?;
                    let range = max - min;
                    let result = if range.abs() < 1e-5 {
                        min
                    } else {
                        let r = value - (range * ((value - min) / range).floor());
                        if is_equal_approx(r, max) {
                            min
                        } else {
                            r
                        }
                    };
                    Ok(Some(Variant::Float(result)))
                }
            }
            "sqrt" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "sqrt() takes 1 argument".into(),
                    )));
                }
                Ok(Some(Variant::Float(to_float(&args[0])?.sqrt())))
            }
            "pow" => {
                if args.len() != 2 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "pow() takes 2 arguments".into(),
                    )));
                }
                Ok(Some(Variant::Float(
                    to_float(&args[0])?.powf(to_float(&args[1])?),
                )))
            }
            "sin" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "sin() takes 1 argument".into(),
                    )));
                }
                Ok(Some(Variant::Float(to_float(&args[0])?.sin())))
            }
            "cos" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "cos() takes 1 argument".into(),
                    )));
                }
                Ok(Some(Variant::Float(to_float(&args[0])?.cos())))
            }
            "randi" => Ok(Some(Variant::Int(deterministic_randi()))),
            "randf" => Ok(Some(Variant::Float(
                (deterministic_randi() as f64) / (i64::MAX as f64),
            ))),
            "randi_range" => {
                if args.len() != 2 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "randi_range() takes 2 arguments".into(),
                    )));
                }
                match (&args[0], &args[1]) {
                    (Variant::Int(lo), Variant::Int(hi)) => {
                        if hi < lo {
                            return Ok(Some(Variant::Int(*lo)));
                        }
                        let r = (hi - lo + 1) as u64;
                        Ok(Some(Variant::Int(
                            lo + ((deterministic_randi().unsigned_abs()) % r) as i64,
                        )))
                    }
                    _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "randi_range() arguments must be int".into(),
                    ))),
                }
            }
            "randf_range" => {
                if args.len() != 2 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "randf_range() takes 2 arguments".into(),
                    )));
                }
                let lo = to_float(&args[0])?;
                let hi = to_float(&args[1])?;
                let t = (deterministic_randi() as f64) / (i64::MAX as f64);
                Ok(Some(Variant::Float(lo + (hi - lo) * t.abs())))
            }
            "get_node" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "get_node() takes 1 argument".into(),
                    )));
                }
                let path = match &args[0] {
                    Variant::String(s) => s.clone(),
                    _ => {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "get_node() argument must be a string".into(),
                        )))
                    }
                };
                if let (Some(ref access), Some(node_id)) =
                    (&self.scene_access, self.current_node_id)
                {
                    match access.get_node(node_id, &path) {
                        Some(target_id) => Ok(Some(Variant::ObjectId(
                            gdcore::id::ObjectId::from_raw(target_id),
                        ))),
                        None => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                            "node not found: {path}"
                        )))),
                    }
                } else {
                    Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "get_node() requires scene access".into(),
                    )))
                }
            }
            "get_parent" => {
                if let (Some(ref access), Some(node_id)) =
                    (&self.scene_access, self.current_node_id)
                {
                    match access.get_parent(node_id) {
                        Some(parent_id) => Ok(Some(Variant::ObjectId(
                            gdcore::id::ObjectId::from_raw(parent_id),
                        ))),
                        None => Ok(Some(Variant::Nil)),
                    }
                } else {
                    Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "get_parent() requires scene access".into(),
                    )))
                }
            }
            "get_children" => {
                if let (Some(ref access), Some(node_id)) =
                    (&self.scene_access, self.current_node_id)
                {
                    let children = access.get_children(node_id);
                    let arr: Vec<Variant> = children
                        .iter()
                        .map(|id| Variant::ObjectId(gdcore::id::ObjectId::from_raw(*id)))
                        .collect();
                    Ok(Some(Variant::Array(arr)))
                } else {
                    Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "get_children() requires scene access".into(),
                    )))
                }
            }
            "get_child_count" => {
                if let (Some(ref access), Some(node_id)) =
                    (&self.scene_access, self.current_node_id)
                {
                    let children = access.get_children(node_id);
                    Ok(Some(Variant::Int(children.len() as i64)))
                } else {
                    Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "get_child_count() requires scene access".into(),
                    )))
                }
            }
            "emit_signal" => {
                if args.is_empty() {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "emit_signal() needs at least a signal name".into(),
                    )));
                }
                let sig_name = match &args[0] {
                    Variant::String(s) => s.clone(),
                    _ => {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "emit_signal() first arg must be string".into(),
                        )))
                    }
                };
                if let (Some(ref mut access), Some(node_id)) =
                    (&mut self.scene_access, self.current_node_id)
                {
                    access.emit_signal(node_id, &sig_name, &args[1..]);
                    Ok(Some(Variant::Nil))
                } else {
                    Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "emit_signal() requires scene access".into(),
                    )))
                }
            }
            "add_child" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "add_child() takes 1 argument".into(),
                    )));
                }
                let child_id = match &args[0] {
                    Variant::ObjectId(oid) => oid.raw(),
                    _ => {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "add_child() argument must be a node reference".into(),
                        )))
                    }
                };
                if let (Some(ref mut access), Some(node_id)) =
                    (&mut self.scene_access, self.current_node_id)
                {
                    access.add_child(node_id, child_id);
                    Ok(Some(Variant::Nil))
                } else {
                    Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "add_child() requires scene access".into(),
                    )))
                }
            }
            // Godot `Node.rpc(method, ...)` / `Node.rpc_id(peer, method, ...)`:
            // send through the tree's MultiplayerAPI. Neither existed in the
            // interpreter at all, so a script could never reach the (working)
            // networking layer (pat-0q2kx.79).
            "rpc" | "rpc_id" => {
                let is_targeted = name == "rpc_id";
                let mut rest = args.iter();
                let peer = if is_targeted {
                    match rest.next() {
                        Some(Variant::Int(p)) => *p,
                        _ => {
                            return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "rpc_id() takes (peer_id, method, ...)".into(),
                            )))
                        }
                    }
                } else {
                    0
                };
                let method = match rest.next() {
                    Some(Variant::String(m)) => m.clone(),
                    Some(Variant::StringName(m)) => m.as_str().to_string(),
                    _ => {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            format!("{name}() requires a method name"),
                        )))
                    }
                };
                let call_args: Vec<Variant> = rest.cloned().collect();
                // Same shape as the `queue_free` arm below: a tuple `if let`
                // over the two disjoint fields.
                if let (Some(ref mut access), Some(node_id)) =
                    (&mut self.scene_access, self.current_node_id)
                {
                    let sent = if is_targeted {
                        access.node_rpc_id(node_id, peer, &method, &call_args)
                    } else {
                        access.node_rpc(node_id, &method, &call_args)
                    };
                    match sent {
                        // Godot errors on an RPC the node never configured; so
                        // does the transport, and that error is surfaced rather
                        // than swallowed.
                        Err(e) => Err(RuntimeError::new(RuntimeErrorKind::TypeError(e))),
                        Ok(()) => Ok(Some(Variant::Nil)),
                    }
                } else {
                    Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                        "{name}() requires scene access"
                    ))))
                }
            }
            "queue_free" => {
                if let (Some(ref mut access), Some(node_id)) =
                    (&mut self.scene_access, self.current_node_id)
                {
                    access.queue_free(node_id);
                    Ok(Some(Variant::Nil))
                } else {
                    Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "queue_free() requires scene access".into(),
                    )))
                }
            }
            "call_deferred" => {
                // Godot: call_deferred(method: StringName, ...) queues a method call.
                if args.is_empty() {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "call_deferred() requires a method name".into(),
                    )));
                }
                let method = match &args[0] {
                    Variant::String(s) => s.clone(),
                    other => {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                            "call_deferred() method name must be a String, got {other:?}"
                        ))))
                    }
                };
                let rest = &args[1..];
                if let (Some(ref mut access), Some(node_id)) =
                    (&mut self.scene_access, self.current_node_id)
                {
                    access.call_deferred(node_id, &method, rest);
                    Ok(Some(Variant::Nil))
                } else {
                    Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "call_deferred() requires scene access".into(),
                    )))
                }
            }
            "deg_to_rad" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "deg_to_rad() takes 1 argument".into(),
                    )));
                }
                Ok(Some(Variant::Float(to_float(&args[0])?.to_radians())))
            }
            "rad_to_deg" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "rad_to_deg() takes 1 argument".into(),
                    )));
                }
                Ok(Some(Variant::Float(to_float(&args[0])?.to_degrees())))
            }
            // The zero-argument forms are Godot's default constructors
            // (`Vector2()` is `ZERO`, `Color()` is opaque black). They matter
            // beyond convenience: nested calls like `AABB(Vector3(), size)`
            // read that way in real scripts (pat-0q2kx.50).
            "Vector2" => {
                if args.is_empty() {
                    return Ok(Some(Variant::Vector2(gdcore::math::Vector2::ZERO)));
                }
                if args.len() != 2 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "Vector2() takes 0 or 2 arguments".into(),
                    )));
                }
                let x = to_float(&args[0])? as f32;
                let y = to_float(&args[1])? as f32;
                Ok(Some(Variant::Vector2(gdcore::math::Vector2::new(x, y))))
            }
            "Vector3" => {
                if args.is_empty() {
                    return Ok(Some(Variant::Vector3(gdcore::math::Vector3::ZERO)));
                }
                if args.len() != 3 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "Vector3() takes 0 or 3 arguments".into(),
                    )));
                }
                let x = to_float(&args[0])? as f32;
                let y = to_float(&args[1])? as f32;
                let z = to_float(&args[2])? as f32;
                Ok(Some(Variant::Vector3(gdcore::math::Vector3::new(x, y, z))))
            }
            // Rect2/Transform2D/Transform3D/Quaternion/Basis/AABB/Plane: every
            // one of these had a `Variant` arm and a tested `gdcore` type, but
            // no way for a script to build one (pat-0q2kx.50).
            "Rect2" | "Transform2D" | "Transform3D" | "Quaternion" | "Basis" | "AABB" | "Plane" => {
                crate::variant_math::construct(name, args).map(Some)
            }
            "Color" => match args.len() {
                0 => Ok(Some(Variant::Color(gdcore::math::Color::new(
                    0.0, 0.0, 0.0, 1.0,
                )))),
                3 => {
                    let r = to_float(&args[0])? as f32;
                    let g = to_float(&args[1])? as f32;
                    let b = to_float(&args[2])? as f32;
                    Ok(Some(Variant::Color(gdcore::math::Color::rgb(r, g, b))))
                }
                4 => {
                    let r = to_float(&args[0])? as f32;
                    let g = to_float(&args[1])? as f32;
                    let b = to_float(&args[2])? as f32;
                    let a = to_float(&args[3])? as f32;
                    Ok(Some(Variant::Color(gdcore::math::Color::new(r, g, b, a))))
                }
                _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "Color() takes 0, 3 or 4 arguments".into(),
                ))),
            },
            "PackedByteArray" => Ok(Some(build_packed_array(
                Variant::PackedByteArray(Vec::new()),
                args,
            )?)),
            "PackedInt32Array" => Ok(Some(build_packed_array(
                Variant::PackedInt32Array(Vec::new()),
                args,
            )?)),
            "PackedInt64Array" => Ok(Some(build_packed_array(
                Variant::PackedInt64Array(Vec::new()),
                args,
            )?)),
            "PackedFloat32Array" => Ok(Some(build_packed_array(
                Variant::PackedFloat32Array(Vec::new()),
                args,
            )?)),
            "PackedFloat64Array" => Ok(Some(build_packed_array(
                Variant::PackedFloat64Array(Vec::new()),
                args,
            )?)),
            "PackedStringArray" => Ok(Some(build_packed_array(
                Variant::PackedStringArray(Vec::new()),
                args,
            )?)),
            "PackedVector2Array" => Ok(Some(build_packed_array(
                Variant::PackedVector2Array(Vec::new()),
                args,
            )?)),
            "PackedVector3Array" => Ok(Some(build_packed_array(
                Variant::PackedVector3Array(Vec::new()),
                args,
            )?)),
            "PackedColorArray" => Ok(Some(build_packed_array(
                Variant::PackedColorArray(Vec::new()),
                args,
            )?)),
            "move_toward" => {
                if args.len() != 3 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "move_toward() takes 3 arguments".into(),
                    )));
                }
                let from = to_float(&args[0])?;
                let to = to_float(&args[1])?;
                let delta = to_float(&args[2])?;
                let result = if (to - from).abs() <= delta {
                    to
                } else if to > from {
                    from + delta
                } else {
                    from - delta
                };
                Ok(Some(Variant::Float(result)))
            }
            "Callable" => {
                if args.len() == 2 {
                    // Callable(object, "method_name")
                    let target_id = match &args[0] {
                        Variant::ObjectId(oid) => oid.raw(),
                        Variant::Nil => 0, // self
                        _ => {
                            return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "Callable() first argument must be an object or null".into(),
                            )));
                        }
                    };
                    let method = match &args[1] {
                        Variant::String(s) => s.clone(),
                        _ => {
                            return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "Callable() second argument must be a string method name".into(),
                            )));
                        }
                    };
                    Ok(Some(Variant::Callable(Box::new(CallableRef::Method {
                        target_id,
                        method,
                    }))))
                } else if args.is_empty() {
                    Ok(Some(Variant::Callable(Box::new(CallableRef::Method {
                        target_id: 0,
                        method: String::new(),
                    }))))
                } else {
                    Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "Callable() takes 0 or 2 arguments".into(),
                    )))
                }
            }
            "is_instance_valid" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "is_instance_valid() takes 1 argument".into(),
                    )));
                }
                // A node reference is valid only while its object is still live
                // in the scene tree. Route object ids through the alive registry
                // (SceneAccess); a freed node reports invalid even though its id
                // is non-nil. Non-object args keep the plain non-nil heuristic.
                let valid = match &args[0] {
                    Variant::ObjectId(oid) => match &self.scene_access {
                        Some(access) => access.is_object_alive(oid.raw()),
                        // No scene to consult -> assume the id is valid (legacy).
                        None => true,
                    },
                    other => !other.is_nil(),
                };
                Ok(Some(Variant::Bool(valid)))
            }
            "assert" => {
                // assert(condition, message = "")
                if args.is_empty() || args.len() > 2 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "assert() takes 1 or 2 arguments".into(),
                    )));
                }
                if !args[0].is_truthy() {
                    let msg = match args.get(1) {
                        Some(Variant::String(s)) => s.clone(),
                        Some(other) => format!("{other}"),
                        None => "assertion failed".to_string(),
                    };
                    return Err(RuntimeError::new(RuntimeErrorKind::AssertionFailed(msg)));
                }
                Ok(Some(Variant::Nil))
            }
            "fmod" => {
                Self::require_args("fmod", args, 2)?;
                Ok(Some(Variant::Float(
                    to_float(&args[0])? % to_float(&args[1])?,
                )))
            }
            "posmod" => {
                Self::require_args("posmod", args, 2)?;
                let a = to_int(&args[0])?;
                let b = to_int(&args[1])?;
                if b == 0 {
                    return Err(RuntimeError::new(RuntimeErrorKind::DivisionByZero));
                }
                let mut value = a % b;
                if (value < 0 && b > 0) || (value > 0 && b < 0) {
                    value += b;
                }
                Ok(Some(Variant::Int(value)))
            }
            "tan" => {
                Self::require_args("tan", args, 1)?;
                Ok(Some(Variant::Float(to_float(&args[0])?.tan())))
            }
            "asin" => {
                Self::require_args("asin", args, 1)?;
                Ok(Some(Variant::Float(to_float(&args[0])?.asin())))
            }
            "acos" => {
                Self::require_args("acos", args, 1)?;
                Ok(Some(Variant::Float(to_float(&args[0])?.acos())))
            }
            "atan" => {
                Self::require_args("atan", args, 1)?;
                Ok(Some(Variant::Float(to_float(&args[0])?.atan())))
            }
            "atan2" => {
                Self::require_args("atan2", args, 2)?;
                Ok(Some(Variant::Float(
                    to_float(&args[0])?.atan2(to_float(&args[1])?),
                )))
            }
            "exp" => {
                Self::require_args("exp", args, 1)?;
                Ok(Some(Variant::Float(to_float(&args[0])?.exp())))
            }
            "log" => {
                Self::require_args("log", args, 1)?;
                Ok(Some(Variant::Float(to_float(&args[0])?.ln())))
            }
            "snapped" => {
                Self::require_args("snapped", args, 2)?;
                let snap = |value: f32, step: f32| {
                    if step != 0.0 {
                        (value / step + 0.5).floor() * step
                    } else {
                        value
                    }
                };
                // Componentwise on vectors (Godot `Vector2.snapped(step)`).
                match (&args[0], &args[1]) {
                    (Variant::Vector2(v), Variant::Vector2(s)) => Ok(Some(Variant::Vector2(
                        gdcore::math::Vector2::new(snap(v.x, s.x), snap(v.y, s.y)),
                    ))),
                    (Variant::Vector3(v), Variant::Vector3(s)) => Ok(Some(Variant::Vector3(
                        gdcore::math::Vector3::new(snap(v.x, s.x), snap(v.y, s.y), snap(v.z, s.z)),
                    ))),
                    _ => {
                        let value = to_float(&args[0])?;
                        let step = to_float(&args[1])?;
                        let result = if step != 0.0 {
                            (value / step + 0.5).floor() * step
                        } else {
                            value
                        };
                        Ok(Some(float_or_int(result, &args[0], &args[1])))
                    }
                }
            }
            "smoothstep" => {
                Self::require_args("smoothstep", args, 3)?;
                let from = to_float(&args[0])?;
                let to = to_float(&args[1])?;
                let s = to_float(&args[2])?;
                Ok(Some(Variant::Float(smoothstep(from, to, s))))
            }
            "remap" => {
                Self::require_args("remap", args, 5)?;
                let value = to_float(&args[0])?;
                let istart = to_float(&args[1])?;
                let istop = to_float(&args[2])?;
                let ostart = to_float(&args[3])?;
                let ostop = to_float(&args[4])?;
                let t = (value - istart) / (istop - istart);
                Ok(Some(Variant::Float(ostart + (ostop - ostart) * t)))
            }
            "inverse_lerp" => {
                Self::require_args("inverse_lerp", args, 3)?;
                let from = to_float(&args[0])?;
                let to = to_float(&args[1])?;
                let value = to_float(&args[2])?;
                Ok(Some(Variant::Float((value - from) / (to - from))))
            }
            "lerp_angle" => {
                Self::require_args("lerp_angle", args, 3)?;
                let from = to_float(&args[0])?;
                let to = to_float(&args[1])?;
                let weight = to_float(&args[2])?;
                Ok(Some(Variant::Float(
                    from + angle_difference(from, to) * weight,
                )))
            }
            // Interpolation + angle helpers (pat-nzgdu.40). Missing arms threw
            // UndefinedFunction on common Godot 4.x scripts.
            "angle_difference" => {
                Self::require_args("angle_difference", args, 2)?;
                Ok(Some(Variant::Float(angle_difference(
                    to_float(&args[0])?,
                    to_float(&args[1])?,
                ))))
            }
            "rotate_toward" => {
                Self::require_args("rotate_toward", args, 3)?;
                Ok(Some(Variant::Float(rotate_toward(
                    to_float(&args[0])?,
                    to_float(&args[1])?,
                    to_float(&args[2])?,
                ))))
            }
            "cubic_interpolate" => {
                Self::require_args("cubic_interpolate", args, 5)?;
                Ok(Some(Variant::Float(cubic_interpolate(
                    to_float(&args[0])?,
                    to_float(&args[1])?,
                    to_float(&args[2])?,
                    to_float(&args[3])?,
                    to_float(&args[4])?,
                ))))
            }
            "cubic_interpolate_angle" => {
                Self::require_args("cubic_interpolate_angle", args, 5)?;
                Ok(Some(Variant::Float(cubic_interpolate_angle(
                    to_float(&args[0])?,
                    to_float(&args[1])?,
                    to_float(&args[2])?,
                    to_float(&args[3])?,
                    to_float(&args[4])?,
                ))))
            }
            "cubic_interpolate_in_time" => {
                Self::require_args("cubic_interpolate_in_time", args, 8)?;
                Ok(Some(Variant::Float(cubic_interpolate_in_time(
                    to_float(&args[0])?,
                    to_float(&args[1])?,
                    to_float(&args[2])?,
                    to_float(&args[3])?,
                    to_float(&args[4])?,
                    to_float(&args[5])?,
                    to_float(&args[6])?,
                    to_float(&args[7])?,
                ))))
            }
            "cubic_interpolate_angle_in_time" => {
                Self::require_args("cubic_interpolate_angle_in_time", args, 8)?;
                Ok(Some(Variant::Float(cubic_interpolate_angle_in_time(
                    to_float(&args[0])?,
                    to_float(&args[1])?,
                    to_float(&args[2])?,
                    to_float(&args[3])?,
                    to_float(&args[4])?,
                    to_float(&args[5])?,
                    to_float(&args[6])?,
                    to_float(&args[7])?,
                ))))
            }
            "bezier_interpolate" => {
                Self::require_args("bezier_interpolate", args, 5)?;
                Ok(Some(Variant::Float(bezier_interpolate(
                    to_float(&args[0])?,
                    to_float(&args[1])?,
                    to_float(&args[2])?,
                    to_float(&args[3])?,
                    to_float(&args[4])?,
                ))))
            }
            "bezier_derivative" => {
                Self::require_args("bezier_derivative", args, 5)?;
                Ok(Some(Variant::Float(bezier_derivative(
                    to_float(&args[0])?,
                    to_float(&args[1])?,
                    to_float(&args[2])?,
                    to_float(&args[3])?,
                    to_float(&args[4])?,
                ))))
            }
            "is_finite" => {
                Self::require_args("is_finite", args, 1)?;
                Ok(Some(Variant::Bool(to_float(&args[0])?.is_finite())))
            }
            "step_decimals" => {
                Self::require_args("step_decimals", args, 1)?;
                Ok(Some(Variant::Int(step_decimals(to_float(&args[0])?))))
            }
            "fposmod" => {
                Self::require_args("fposmod", args, 2)?;
                Ok(Some(Variant::Float(fposmod(
                    to_float(&args[0])?,
                    to_float(&args[1])?,
                ))))
            }
            "sinh" => {
                Self::require_args("sinh", args, 1)?;
                Ok(Some(Variant::Float(to_float(&args[0])?.sinh())))
            }
            "cosh" => {
                Self::require_args("cosh", args, 1)?;
                Ok(Some(Variant::Float(to_float(&args[0])?.cosh())))
            }
            "tanh" => {
                Self::require_args("tanh", args, 1)?;
                Ok(Some(Variant::Float(to_float(&args[0])?.tanh())))
            }
            "asinh" => {
                Self::require_args("asinh", args, 1)?;
                Ok(Some(Variant::Float(to_float(&args[0])?.asinh())))
            }
            "acosh" => {
                Self::require_args("acosh", args, 1)?;
                Ok(Some(Variant::Float(to_float(&args[0])?.acosh())))
            }
            "atanh" => {
                Self::require_args("atanh", args, 1)?;
                Ok(Some(Variant::Float(to_float(&args[0])?.atanh())))
            }
            "wrapi" => {
                Self::require_args("wrapi", args, 3)?;
                let value = to_int(&args[0])?;
                let min = to_int(&args[1])?;
                let max = to_int(&args[2])?;
                let range = max - min;
                let result = if range == 0 {
                    min
                } else {
                    min + (((value - min) % range) + range) % range
                };
                Ok(Some(Variant::Int(result)))
            }
            "wrapf" => {
                Self::require_args("wrapf", args, 3)?;
                let value = to_float(&args[0])?;
                let min = to_float(&args[1])?;
                let max = to_float(&args[2])?;
                let range = max - min;
                let result = if range.abs() < 1e-5 {
                    min
                } else {
                    let r = value - (range * ((value - min) / range).floor());
                    if is_equal_approx(r, max) {
                        min
                    } else {
                        r
                    }
                };
                Ok(Some(Variant::Float(result)))
            }
            "pingpong" => {
                Self::require_args("pingpong", args, 2)?;
                let value = to_float(&args[0])?;
                let length = to_float(&args[1])?;
                let result = if length != 0.0 {
                    let x = (value - length) / (length * 2.0);
                    ((x - x.floor()) * length * 2.0 - length).abs()
                } else {
                    0.0
                };
                Ok(Some(Variant::Float(result)))
            }
            "ease" => {
                Self::require_args("ease", args, 2)?;
                let x = to_float(&args[0])?;
                let curve = to_float(&args[1])?;
                Ok(Some(Variant::Float(ease(x, curve))))
            }
            "is_equal_approx" => {
                Self::require_args("is_equal_approx", args, 2)?;
                Ok(Some(Variant::Bool(is_equal_approx(
                    to_float(&args[0])?,
                    to_float(&args[1])?,
                ))))
            }
            "is_zero_approx" => {
                Self::require_args("is_zero_approx", args, 1)?;
                Ok(Some(Variant::Bool(to_float(&args[0])?.abs() < 1e-5)))
            }
            "is_nan" => {
                Self::require_args("is_nan", args, 1)?;
                Ok(Some(Variant::Bool(to_float(&args[0])?.is_nan())))
            }
            "is_inf" => {
                Self::require_args("is_inf", args, 1)?;
                Ok(Some(Variant::Bool(to_float(&args[0])?.is_infinite())))
            }
            "nearest_po2" => {
                Self::require_args("nearest_po2", args, 1)?;
                let x = to_int(&args[0])?;
                Ok(Some(Variant::Int(nearest_po2(x))))
            }
            "db_to_linear" => {
                Self::require_args("db_to_linear", args, 1)?;
                Ok(Some(Variant::Float(
                    (to_float(&args[0])? * 0.11512925464970228).exp(),
                )))
            }
            "linear_to_db" => {
                Self::require_args("linear_to_db", args, 1)?;
                Ok(Some(Variant::Float(
                    to_float(&args[0])?.ln() * 8.685889638065037,
                )))
            }

            // -- Print / error family (pat-u3l8c.20) --------------------------
            "prints" => {
                let line: Vec<String> = args.iter().map(|a| format!("{a}")).collect();
                self.output.push(line.join(" "));
                Ok(Some(Variant::Nil))
            }
            "printt" => {
                let line: Vec<String> = args.iter().map(|a| format!("{a}")).collect();
                self.output.push(line.join("\t"));
                Ok(Some(Variant::Nil))
            }
            "printerr" => {
                // Concatenate args (no separator) onto the error channel.
                let line: String = args.iter().map(|a| format!("{a}")).collect();
                self.errors.push(line);
                Ok(Some(Variant::Nil))
            }
            "print_rich" => {
                // Concatenate then strip BBCode tags (headless has no console).
                let joined: String = args.iter().map(|a| format!("{a}")).collect();
                self.output.push(strip_bbcode(&joined));
                Ok(Some(Variant::Nil))
            }
            "print_debug" => {
                let joined: String = args.iter().map(|a| format!("{a}")).collect();
                let func = self.current_function.as_deref().unwrap_or("GDScript");
                self.output
                    .push(format!("{joined}\n   At: {func}:{}", self.current_line));
                Ok(Some(Variant::Nil))
            }
            "push_error" => {
                Self::require_args("push_error", args, 1)?;
                let msg = format!("{}", args[0]);
                self.errors.push(format!(
                    "{msg}\n   at: {}:{}",
                    self.current_function.as_deref().unwrap_or("<anonymous>"),
                    self.current_line
                ));
                Ok(Some(Variant::Nil))
            }
            "push_warning" => {
                Self::require_args("push_warning", args, 1)?;
                let msg = format!("{}", args[0]);
                // `ScriptWarning` has no free-form variant, so route warnings to
                // the error channel with a "WARNING: " prefix and source line.
                self.errors.push(format!(
                    "WARNING: {msg}\n   at: {}:{}",
                    self.current_function.as_deref().unwrap_or("<anonymous>"),
                    self.current_line
                ));
                Ok(Some(Variant::Nil))
            }
            "printraw" => {
                // Godot `printraw` writes with no trailing newline. The output
                // buffer is line-based, so concatenate onto the current (last)
                // line instead of starting a new one; consecutive `printraw`
                // calls therefore land on a single line.
                let joined: String = args.iter().map(|a| format!("{a}")).collect();
                if let Some(last) = self.output.last_mut() {
                    last.push_str(&joined);
                } else {
                    self.output.push(joined);
                }
                Ok(Some(Variant::Nil))
            }
            "print_verbose" => {
                // Godot `print_verbose` prints only when OS verbose mode is on.
                // The headless interpreter has no verbose stdout, so this is a
                // no-op (registered so it does not throw UndefinedFunction).
                Ok(Some(Variant::Nil))
            }

            // -- Serialization (pat-u3l8c.20) ---------------------------------
            "var_to_str" => {
                Self::require_args("var_to_str", args, 1)?;
                Ok(Some(Variant::String(variant_to_gd_string(&args[0]))))
            }
            "str_to_var" => {
                Self::require_args("str_to_var", args, 1)?;
                let s = match &args[0] {
                    Variant::String(s) => s.clone(),
                    other => {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                            "str_to_var() expects String, got {}",
                            other.variant_type()
                        ))))
                    }
                };
                Ok(Some(gd_string_to_variant(&s)))
            }
            "var_to_bytes" => {
                Self::require_args("var_to_bytes", args, 1)?;
                let bytes = gdvariant::binary::encode_variant(&args[0]).map_err(|e| {
                    RuntimeError::new(RuntimeErrorKind::TypeError(format!("var_to_bytes: {e:?}")))
                })?;
                Ok(Some(Variant::PackedByteArray(bytes)))
            }
            "bytes_to_var" => {
                Self::require_args("bytes_to_var", args, 1)?;
                let buf = match &args[0] {
                    Variant::PackedByteArray(b) => b,
                    other => {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                            "bytes_to_var() expects PackedByteArray, got {}",
                            other.variant_type()
                        ))))
                    }
                };
                let (v, _used) = gdvariant::binary::decode_variant(buf).map_err(|e| {
                    RuntimeError::new(RuntimeErrorKind::TypeError(format!("bytes_to_var: {e:?}")))
                })?;
                // Godot hands back an inert EncodedObjectAsID here, never a
                // usable object — `bytes_to_var` decodes with objects disallowed
                // (pat-nzgdu.189). Skipping this would turn a decoded id into a
                // live scene reference.
                Ok(Some(wrap_decoded_object_ids(v)))
            }

            // -- Utility (pat-u3l8c.20) ---------------------------------------
            "type_string" => {
                Self::require_args("type_string", args, 1)?;
                let code = to_int(&args[0])?;
                Ok(Some(Variant::String(type_code_to_name(code).to_string())))
            }
            "hash" => {
                Self::require_args("hash", args, 1)?;
                Ok(Some(Variant::Int(variant_hash(&args[0]))))
            }
            "Color8" => {
                Self::require_args_range("Color8", args, 3, 4)?;
                let r = to_int(&args[0])? as f32 / 255.0;
                let g = to_int(&args[1])? as f32 / 255.0;
                let b = to_int(&args[2])? as f32 / 255.0;
                let a = if args.len() == 4 {
                    to_int(&args[3])? as f32 / 255.0
                } else {
                    1.0
                };
                Ok(Some(Variant::Color(gdcore::math::Color::new(r, g, b, a))))
            }
            "char" => {
                Self::require_args("char", args, 1)?;
                let code = to_int(&args[0])?;
                let s = u32::try_from(code)
                    .ok()
                    .and_then(char::from_u32)
                    .map(|c| c.to_string())
                    .unwrap_or_default();
                Ok(Some(Variant::String(s)))
            }
            "ord" => {
                Self::require_args("ord", args, 1)?;
                let s = match &args[0] {
                    Variant::String(s) => s,
                    other => {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                            "ord() expects a single-character String, got {}",
                            other.variant_type()
                        ))))
                    }
                };
                let mut chars = s.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => Ok(Some(Variant::Int(c as i64))),
                    _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "ord() expects a string of exactly one character".into(),
                    ))),
                }
            }
            "is_instance_of" => {
                Self::require_args("is_instance_of", args, 2)?;
                // Second arg is either a Variant.Type code (Int) or a type/class
                // name (String/StringName); reuse the `is` operator's logic.
                let result = match &args[1] {
                    Variant::Int(code) => {
                        crate::global_constants::variant_type_code(&args[0]) == *code
                    }
                    Variant::String(type_name) => self.value_is_type(&args[0], type_name),
                    Variant::StringName(sn) => self.value_is_type(&args[0], sn.as_str()),
                    _ => false,
                };
                Ok(Some(Variant::Bool(result)))
            }
            // Object/weakref infrastructure consults the live-object registry
            // through `SceneAccess` when a scene is present (pat-nzgdu.32).
            // `weakref` returns the object id as a re-queryable handle;
            // `instance_from_id` resolves an id back to a live object (or Nil).
            // Without scene access both degrade gracefully (passthrough / Nil).
            "weakref" => {
                Self::require_args("weakref", args, 1)?;
                // A WeakRef is a non-owning handle: return the object id
                // unchanged so `is_instance_valid(w)` and `w.get_ref()` re-query
                // the live registry and observe `Nil` once the referent is freed.
                Ok(Some(args[0].clone()))
            }
            "instance_from_id" => {
                Self::require_args("instance_from_id", args, 1)?;
                let raw = match &args[0] {
                    Variant::ObjectId(oid) => oid.raw(),
                    other => to_int(other)? as u64,
                };
                let resolved = self
                    .scene_access
                    .as_ref()
                    .and_then(|access| access.instance_from_id(raw));
                Ok(Some(match resolved {
                    Some(id) => Variant::ObjectId(gdcore::id::ObjectId::from_raw(id)),
                    None => Variant::Nil,
                }))
            }

            // -- Object meta + dynamic property access (pat-nzgdu.30).
            // Bare `set_meta/get_meta/has_meta/remove_meta/get_meta_list` and the
            // dynamic `set/get/set_deferred` accessors operate on the implicit
            // `self` node (Godot `Object` API). Meta uses the Node's separate meta
            // bag; `set`/`get` reuse the node-property accessors; `set_deferred`
            // routes a `set` through the end-of-frame deferred queue. A call whose
            // signature does not match (or without scene access) returns `Ok(None)`
            // so a user-defined function of the same name still resolves.
            "set_meta" => {
                if let (Some(Variant::String(key)), Some(value)) = (args.first(), args.get(1)) {
                    if let (Some(ref mut access), Some(node_id)) =
                        (&mut self.scene_access, self.current_node_id)
                    {
                        access.set_meta(node_id, key, value.clone());
                        return Ok(Some(Variant::Nil));
                    }
                }
                Ok(None)
            }
            "get_meta" => {
                if let Some(Variant::String(key)) = args.first() {
                    if let (Some(ref access), Some(node_id)) =
                        (&self.scene_access, self.current_node_id)
                    {
                        let default = args.get(1).cloned().unwrap_or(Variant::Nil);
                        return Ok(Some(access.get_meta(node_id, key, default)));
                    }
                }
                Ok(None)
            }
            "has_meta" => {
                if let Some(Variant::String(key)) = args.first() {
                    if let (Some(ref access), Some(node_id)) =
                        (&self.scene_access, self.current_node_id)
                    {
                        return Ok(Some(Variant::Bool(access.has_meta(node_id, key))));
                    }
                }
                Ok(None)
            }
            "remove_meta" => {
                if let Some(Variant::String(key)) = args.first() {
                    if let (Some(ref mut access), Some(node_id)) =
                        (&mut self.scene_access, self.current_node_id)
                    {
                        access.remove_meta(node_id, key);
                        return Ok(Some(Variant::Nil));
                    }
                }
                Ok(None)
            }
            "get_meta_list" => {
                if let (Some(ref access), Some(node_id)) =
                    (&self.scene_access, self.current_node_id)
                {
                    let names = access.get_meta_list(node_id);
                    return Ok(Some(Variant::Array(
                        names.into_iter().map(Variant::String).collect(),
                    )));
                }
                Ok(None)
            }
            "set" => {
                if let (Some(Variant::String(prop)), Some(value)) = (args.first(), args.get(1)) {
                    if let (Some(ref mut access), Some(node_id)) =
                        (&mut self.scene_access, self.current_node_id)
                    {
                        access.set_node_property(node_id, prop, value.clone());
                        return Ok(Some(Variant::Nil));
                    }
                }
                Ok(None)
            }
            "get" => {
                if args.len() == 1 {
                    if let Some(Variant::String(prop)) = args.first() {
                        if let (Some(ref access), Some(node_id)) =
                            (&self.scene_access, self.current_node_id)
                        {
                            return Ok(Some(access.get_node_property(node_id, prop)));
                        }
                    }
                }
                Ok(None)
            }
            "set_deferred" => {
                if let (Some(Variant::String(prop)), Some(value)) = (args.first(), args.get(1)) {
                    if let (Some(ref mut access), Some(node_id)) =
                        (&mut self.scene_access, self.current_node_id)
                    {
                        access.call_deferred(
                            node_id,
                            "set",
                            &[Variant::String(prop.clone()), value.clone()],
                        );
                        return Ok(Some(Variant::Nil));
                    }
                }
                Ok(None)
            }

            // Variant identity (pat-nzgdu.107). Godot `is_same` requires EXACT
            // type + value equality (unlike `==`, which coerces int/float).
            // Reference types (Array/Dictionary/Object) use identity in Godot;
            // Patina's value-based Variants can't model that, so those fall back
            // to structural equality here.
            "is_same" => {
                if args.len() != 2 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "is_same() takes exactly 2 arguments".into(),
                    )));
                }
                let same = crate::global_constants::variant_type_code(&args[0])
                    == crate::global_constants::variant_type_code(&args[1])
                    && args[0] == args[1];
                Ok(Some(Variant::Bool(same)))
            }

            _ => Ok(None), // Not a built-in
        }
    }

    /// Validates that a built-in received exactly `n` arguments.
    fn require_args(name: &str, args: &[Variant], n: usize) -> Result<(), RuntimeError> {
        if args.len() != n {
            return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                "{name}() takes {n} argument(s), got {}",
                args.len()
            ))));
        }
        Ok(())
    }

    /// Validates that a built-in received between `min` and `max` arguments.
    fn require_args_range(
        name: &str,
        args: &[Variant],
        min: usize,
        max: usize,
    ) -> Result<(), RuntimeError> {
        if args.len() < min || args.len() > max {
            return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                "{name}() takes {min} to {max} arguments, got {}",
                args.len()
            ))));
        }
        Ok(())
    }

    fn call_user_func(&mut self, name: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
        stacker::maybe_grow(STACK_RED_ZONE, STACK_GROWTH, || {
            self.call_user_func_impl(name, args)
        })
    }

    fn call_user_func_impl(
        &mut self,
        name: &str,
        args: &[Variant],
    ) -> Result<Variant, RuntimeError> {
        self.call_stack_frames.push(StackFrame {
            function_name: name.to_string(),
            source_location: Some(self.make_location()),
        });
        let prev_function = self.current_function.take();
        self.current_function = Some(name.to_string());

        let func = self.function_registry.get(name).cloned().ok_or_else(|| {
            RuntimeError::new(RuntimeErrorKind::UndefinedFunction(name.to_string()))
        })?;

        if args.len() < func.min_args() || args.len() > func.params.len() {
            return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                "{}() takes {} arguments, got {}",
                name,
                func.params.len(),
                args.len()
            ))));
        }

        if self.call_depth >= MAX_RECURSION_DEPTH {
            return Err(RuntimeError::new(RuntimeErrorKind::MaxRecursionDepth(
                MAX_RECURSION_DEPTH,
            )));
        }
        self.call_depth += 1;

        self.environment.push_scope();
        for (i, param) in func.params.iter().enumerate() {
            let val = if i < args.len() {
                args[i].clone()
            } else if let Some(Some(default_expr)) = func.defaults.get(i) {
                self.eval_expr(default_expr)?
            } else {
                Variant::Nil
            };
            self.environment.define(param.clone(), val);
        }

        let mut return_val = Variant::Nil;
        let body = func.body.clone();
        let mut i = 0;
        while i < body.len() {
            match self.exec_stmt(&body[i])? {
                Some(ControlFlow::Return(v)) => {
                    return_val = v.unwrap_or(Variant::Nil);
                    break;
                }
                Some(ControlFlow::Suspend(target)) => {
                    let remaining = body[i + 1..].to_vec();
                    self.park_coroutine(remaining, target, name, false);
                    self.environment.pop_scope();
                    self.call_depth -= 1;
                    self.current_function = prev_function;
                    self.call_stack_frames.pop();
                    return Ok(Variant::Nil);
                }
                Some(ControlFlow::Break) | Some(ControlFlow::Continue) => {}
                None => {}
            }
            i += 1;
        }

        self.environment.pop_scope();
        self.call_depth -= 1;
        self.current_function = prev_function;
        self.call_stack_frames.pop();
        Ok(return_val)
    }

    /// Dispatch an `Input.method()` call through the scene_access input API.
    fn call_input_method(
        &mut self,
        method: &str,
        args: &[Variant],
    ) -> Result<Variant, RuntimeError> {
        match method {
            "is_action_pressed" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "Input.is_action_pressed() takes 1 argument".into(),
                    )));
                }
                let action = match &args[0] {
                    Variant::String(s) => s.as_str(),
                    _ => {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "Input.is_action_pressed() argument must be a string".into(),
                        )))
                    }
                };
                if let Some(ref access) = self.scene_access {
                    Ok(Variant::Bool(access.is_input_action_pressed(action)))
                } else {
                    Ok(Variant::Bool(false))
                }
            }
            "is_action_just_pressed" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "Input.is_action_just_pressed() takes 1 argument".into(),
                    )));
                }
                let action = match &args[0] {
                    Variant::String(s) => s.as_str(),
                    _ => {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "Input.is_action_just_pressed() argument must be a string".into(),
                        )))
                    }
                };
                if let Some(ref access) = self.scene_access {
                    Ok(Variant::Bool(access.is_input_action_just_pressed(action)))
                } else {
                    Ok(Variant::Bool(false))
                }
            }
            "is_key_pressed" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "Input.is_key_pressed() takes 1 argument".into(),
                    )));
                }
                let key = match &args[0] {
                    Variant::String(s) => s.as_str(),
                    _ => {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "Input.is_key_pressed() argument must be a string".into(),
                        )))
                    }
                };
                if let Some(ref access) = self.scene_access {
                    Ok(Variant::Bool(access.is_input_key_pressed(key)))
                } else {
                    Ok(Variant::Bool(false))
                }
            }
            "get_global_mouse_position" => {
                if !args.is_empty() {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "Input.get_global_mouse_position() takes 0 arguments".into(),
                    )));
                }
                if let Some(ref access) = self.scene_access {
                    let (x, y) = access.get_global_mouse_position();
                    Ok(Variant::Vector2(gdcore::math::Vector2::new(x, y)))
                } else {
                    Ok(Variant::Vector2(gdcore::math::Vector2::new(0.0, 0.0)))
                }
            }
            "is_mouse_button_pressed" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "Input.is_mouse_button_pressed() takes 1 argument".into(),
                    )));
                }
                let button_index = match &args[0] {
                    Variant::Int(i) => *i,
                    _ => {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "Input.is_mouse_button_pressed() argument must be an integer".into(),
                        )))
                    }
                };
                if let Some(ref access) = self.scene_access {
                    Ok(Variant::Bool(access.is_mouse_button_pressed(button_index)))
                } else {
                    Ok(Variant::Bool(false))
                }
            }
            "get_vector" => {
                if args.len() != 4 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "Input.get_vector() takes 4 arguments".into(),
                    )));
                }
                let strs: Vec<&str> = args
                    .iter()
                    .map(|a| match a {
                        Variant::String(s) => Ok(s.as_str()),
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "Input.get_vector() arguments must be strings".into(),
                        ))),
                    })
                    .collect::<Result<_, _>>()?;
                if let Some(ref access) = self.scene_access {
                    let (x, y) = access.get_input_vector(strs[0], strs[1], strs[2], strs[3]);
                    Ok(Variant::Vector2(gdcore::math::Vector2::new(x, y)))
                } else {
                    Ok(Variant::Vector2(gdcore::math::Vector2::new(0.0, 0.0)))
                }
            }
            // pat-nzgdu.9 — analog/axis/just-released polling.
            "get_axis" => {
                if args.len() != 2 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "Input.get_axis() takes 2 arguments".into(),
                    )));
                }
                let neg = Self::input_action_arg("get_axis", &args[0])?;
                let pos = Self::input_action_arg("get_axis", &args[1])?;
                if let Some(ref access) = self.scene_access {
                    Ok(Variant::Float(access.get_input_axis(neg, pos) as f64))
                } else {
                    Ok(Variant::Float(0.0))
                }
            }
            "get_action_strength" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "Input.get_action_strength() takes 1 argument".into(),
                    )));
                }
                let action = Self::input_action_arg("get_action_strength", &args[0])?;
                if let Some(ref access) = self.scene_access {
                    Ok(Variant::Float(access.get_action_strength(action) as f64))
                } else {
                    Ok(Variant::Float(0.0))
                }
            }
            "is_action_just_released" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "Input.is_action_just_released() takes 1 argument".into(),
                    )));
                }
                let action = Self::input_action_arg("is_action_just_released", &args[0])?;
                if let Some(ref access) = self.scene_access {
                    Ok(Variant::Bool(access.is_input_action_just_released(action)))
                } else {
                    Ok(Variant::Bool(false))
                }
            }
            // pat-u3l8c.22 — mouse mode, programmatic actions, joypad & vibration.
            "set_mouse_mode" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "Input.set_mouse_mode() takes 1 argument".into(),
                    )));
                }
                let Variant::Int(mode) = &args[0] else {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "Input.set_mouse_mode() argument must be an int".into(),
                    )));
                };
                let mode = *mode;
                if let Some(access) = self.scene_access.as_mut() {
                    access.set_mouse_mode(mode);
                }
                Ok(Variant::Nil)
            }
            "get_mouse_mode" => {
                let mode = self
                    .scene_access
                    .as_ref()
                    .map(|a| a.get_mouse_mode())
                    .unwrap_or(0);
                Ok(Variant::Int(mode))
            }
            "warp_mouse" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "Input.warp_mouse() takes 1 argument".into(),
                    )));
                }
                let Variant::Vector2(pos) = &args[0] else {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "Input.warp_mouse() argument must be a Vector2".into(),
                    )));
                };
                let (px, py) = (pos.x, pos.y);
                if let Some(access) = self.scene_access.as_mut() {
                    access.warp_mouse(px, py);
                }
                Ok(Variant::Nil)
            }
            "action_press" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "Input.action_press() takes 1 or 2 arguments".into(),
                    )));
                }
                let action = Self::input_action_arg("action_press", &args[0])?.to_string();
                let strength = match args.get(1) {
                    None => 1.0_f32,
                    Some(Variant::Float(f)) => *f as f32,
                    Some(Variant::Int(i)) => *i as f32,
                    Some(_) => {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "Input.action_press() strength must be a number".into(),
                        )))
                    }
                };
                if let Some(access) = self.scene_access.as_mut() {
                    access.action_press(&action, strength);
                }
                Ok(Variant::Nil)
            }
            "action_release" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "Input.action_release() takes 1 argument".into(),
                    )));
                }
                let action = Self::input_action_arg("action_release", &args[0])?.to_string();
                if let Some(access) = self.scene_access.as_mut() {
                    access.action_release(&action);
                }
                Ok(Variant::Nil)
            }
            "get_connected_joypads" => {
                let ids = self
                    .scene_access
                    .as_ref()
                    .map(|a| a.get_connected_joypads())
                    .unwrap_or_default();
                Ok(Variant::Array(ids.into_iter().map(Variant::Int).collect()))
            }
            "start_joy_vibration" => {
                if args.len() < 3 || args.len() > 4 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "Input.start_joy_vibration() takes 3 or 4 arguments".into(),
                    )));
                }
                let Variant::Int(device) = &args[0] else {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "Input.start_joy_vibration() device must be an int".into(),
                    )));
                };
                let device = *device;
                let num = |a: &Variant| -> Result<f32, RuntimeError> {
                    match a {
                        Variant::Float(f) => Ok(*f as f32),
                        Variant::Int(i) => Ok(*i as f32),
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "Input.start_joy_vibration() magnitudes must be numbers".into(),
                        ))),
                    }
                };
                let weak = num(&args[1])?;
                let strong = num(&args[2])?;
                let duration = match args.get(3) {
                    Some(v) => num(v)?,
                    None => 0.0_f32,
                };
                if let Some(access) = self.scene_access.as_mut() {
                    access.start_joy_vibration(device, weak, strong, duration);
                }
                Ok(Variant::Nil)
            }
            "stop_joy_vibration" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "Input.stop_joy_vibration() takes 1 argument".into(),
                    )));
                }
                let Variant::Int(device) = &args[0] else {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "Input.stop_joy_vibration() device must be an int".into(),
                    )));
                };
                let device = *device;
                if let Some(access) = self.scene_access.as_mut() {
                    access.stop_joy_vibration(device);
                }
                Ok(Variant::Nil)
            }
            other => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
                format!("Input.{other}"),
            ))),
        }
    }

    /// Extracts a string action-name argument for an `Input.*` method.
    fn input_action_arg<'a>(method: &str, arg: &'a Variant) -> Result<&'a str, RuntimeError> {
        match arg {
            Variant::String(s) => Ok(s.as_str()),
            _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                "Input.{method}() argument must be a string"
            )))),
        }
    }

    /// Call a method on a value (e.g. array.push_back, array.size, etc.)
    /// Clones the array, applies `f` (which mutates it and yields a return
    /// value), then writes the whole array back to its place. This is how every
    /// mutating Array method (`append`, `sort`, `insert`, …) persists through
    /// the general lvalue write path, so `self.items.append(x)` and
    /// `grid[i].append(x)` work.
    fn array_mutate<F>(
        &mut self,
        arr: &[Variant],
        object_expr: &Expr,
        f: F,
    ) -> Result<Variant, RuntimeError>
    where
        F: FnOnce(&mut Vec<Variant>) -> Variant,
    {
        let mut items = arr.to_vec();
        let ret = f(&mut items);
        self.write_place(object_expr, Variant::Array(items))?;
        Ok(ret)
    }

    fn call_method_on(
        &mut self,
        obj: &Variant,
        method: &str,
        args: &[Variant],
        object_expr: &Expr,
    ) -> Result<Variant, RuntimeError> {
        // FileAccess / DirAccess / ConfigFile handle methods.
        if let Variant::ObjectId(oid) = obj {
            if gdcore::is_file_access_handle(*oid) {
                return call_file_access_instance(*oid, method, args);
            }
            if gdcore::is_dir_access_handle(*oid) {
                return call_dir_access_instance(*oid, method, args);
            }
            if gdresource::is_config_file_handle(*oid) {
                return call_config_file_instance(*oid, method, args);
            }
            if is_camera_feed_handle(*oid) {
                return call_camera_feed_instance(*oid, method, args);
            }
            if is_undo_redo_handle(*oid) {
                // UndoRedo applies its recorded property writes through the
                // scene, so it is the one handle family that needs access.
                let access = self.scene_access.as_deref_mut();
                return call_undo_redo_instance(*oid, method, args, access);
            }
            // MUST precede the Variant::ObjectId arm below, which resolves ids
            // against the live scene. An EncodedObjectAsID reports its id and
            // resolves to nothing — that is the whole point (pat-nzgdu.189).
            if is_encoded_object_as_id_handle(*oid) {
                return call_encoded_object_as_id_instance(*oid, method, args);
            }
            // Tween handles (pat-0q2kx.4). Unlike the families above, a tween id
            // carries no tag bit — it is drawn from the same `ObjectId` counter
            // as nodes — so only the host can say whether this id names a live
            // tween. `None` means "not a tween", and we fall through to the
            // node arms below. This MUST run before them: `Tween` shares the
            // method names `play`/`stop`/`pause`/`kill`/`set_loops` with
            // AudioStreamPlayer and AnimationPlayer, which would otherwise
            // swallow the call and set `playing` on a nonexistent node.
            if let Some(ref mut access) = self.scene_access {
                if let Some(result) = access.tween_call(oid.raw(), method, args) {
                    return Ok(result);
                }
            }
        }
        match obj {
            // -- Godot's `Signal` builtin (pat-0q2kx.9) ----------------------
            //
            // `$B.pressed.connect(cb)` / `sig.emit(x)` / `sig.is_connected(cb)`.
            // The value carries its own object, so every call routes to *that*
            // object's SignalStore rather than matching by name globally.
            Variant::Signal { object_id, name } => {
                let source = *object_id;
                // Captured before the mutable `scene_access` borrow below.
                let self_node = self.current_node_id;
                match method {
                    "get_name" => return Ok(Variant::String(name.clone())),
                    "get_object" => {
                        return Ok(Variant::ObjectId(gdcore::id::ObjectId::from_raw(source)))
                    }
                    "get_object_id" => return Ok(Variant::Int(source as i64)),
                    // Godot: a Signal is null when it is bound to nothing.
                    "is_null" => return Ok(Variant::Bool(source == 0 || name.is_empty())),
                    _ => {}
                }

                let access = match self.scene_access.as_deref_mut() {
                    Some(a) => a,
                    None => {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "Signal methods require scene access".into(),
                        )))
                    }
                };
                match method {
                    "emit" => {
                        access.emit_signal(source, name, args);
                        Ok(Variant::Nil)
                    }
                    "connect" | "disconnect" | "is_connected" => {
                        // Godot 4 takes a Callable. `Callable(self, "m")` has
                        // target_id 0 meaning "the calling object", so resolve
                        // that against the current node.
                        let (target, target_method) = match args.first() {
                            Some(v) => match callable_target(v, self_node) {
                                Some(t) => t,
                                None => {
                                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                        format!("Signal.{method}() expects a Callable"),
                                    )))
                                }
                            },
                            None => {
                                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                    format!("Signal.{method}() requires a Callable"),
                                )))
                            }
                        };
                        match method {
                            "connect" => {
                                // Optional Godot ConnectFlags second argument.
                                let flags = match args.get(1) {
                                    Some(Variant::Int(i)) => *i,
                                    _ => 0,
                                };
                                access.connect_signal_flags(
                                    source,
                                    name,
                                    target,
                                    &target_method,
                                    flags,
                                    &[],
                                );
                                // Godot returns an Error code; OK == 0.
                                Ok(Variant::Int(0))
                            }
                            "disconnect" => {
                                access.disconnect_signal(source, name, target, &target_method);
                                Ok(Variant::Nil)
                            }
                            _ => Ok(Variant::Bool(access.is_signal_connected(
                                source,
                                name,
                                target,
                                &target_method,
                            ))),
                        }
                    }
                    "get_connections" => {
                        let conns: Vec<Variant> = access
                            .signal_connections(source, name)
                            .into_iter()
                            .map(|(t, m)| {
                                let mut d = Dictionary::new();
                                d.insert(
                                    "callable",
                                    Variant::Callable(Box::new(CallableRef::Method {
                                        target_id: t,
                                        method: m,
                                    })),
                                );
                                d.insert(
                                    "signal",
                                    Variant::Signal {
                                        object_id: source,
                                        name: name.clone(),
                                    },
                                );
                                Variant::Dictionary(d)
                            })
                            .collect();
                        Ok(Variant::Array(conns))
                    }
                    "has_connections" => Ok(Variant::Bool(
                        !access.signal_connections(source, name).is_empty(),
                    )),
                    other => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
                        format!("Signal.{other}"),
                    ))),
                }
            }
            Variant::Array(arr) => match method {
                "size" | "length" => Ok(Variant::Int(arr.len() as i64)),
                "is_empty" => Ok(Variant::Bool(arr.is_empty())),
                "push_back" | "append" => {
                    Self::require_args(method, args, 1)?;
                    self.array_mutate(arr, object_expr, |a| {
                        a.push(args[0].clone());
                        Variant::Nil
                    })
                }
                "push_front" => {
                    Self::require_args("push_front", args, 1)?;
                    self.array_mutate(arr, object_expr, |a| {
                        a.insert(0, args[0].clone());
                        Variant::Nil
                    })
                }
                "pop_back" => {
                    self.array_mutate(arr, object_expr, |a| a.pop().unwrap_or(Variant::Nil))
                }
                "pop_front" => self.array_mutate(arr, object_expr, |a| {
                    if a.is_empty() {
                        Variant::Nil
                    } else {
                        a.remove(0)
                    }
                }),
                "pop_at" => {
                    Self::require_args("pop_at", args, 1)?;
                    let i = to_int(&args[0])?;
                    self.array_mutate(arr, object_expr, |a| {
                        let idx = if i < 0 { i + a.len() as i64 } else { i };
                        if idx >= 0 && (idx as usize) < a.len() {
                            a.remove(idx as usize)
                        } else {
                            Variant::Nil
                        }
                    })
                }
                "remove_at" => {
                    Self::require_args("remove_at", args, 1)?;
                    let idx = to_int(&args[0])?;
                    self.array_mutate(arr, object_expr, |a| {
                        if idx >= 0 && (idx as usize) < a.len() {
                            a.remove(idx as usize);
                        }
                        Variant::Nil
                    })
                }
                "front" => Ok(arr.first().cloned().unwrap_or(Variant::Nil)),
                "back" => Ok(arr.last().cloned().unwrap_or(Variant::Nil)),
                "clear" => self.array_mutate(arr, object_expr, |a| {
                    a.clear();
                    Variant::Nil
                }),
                "sort" => self.array_mutate(arr, object_expr, |a| {
                    a.sort_by(variant_cmp);
                    Variant::Nil
                }),
                "reverse" => self.array_mutate(arr, object_expr, |a| {
                    a.reverse();
                    Variant::Nil
                }),
                "find" => {
                    Self::require_args("find", args, 1)?;
                    Ok(Variant::Int(
                        arr.iter()
                            .position(|v| variant_eq(v, &args[0]))
                            .map(|idx| idx as i64)
                            .unwrap_or(-1),
                    ))
                }
                "rfind" => {
                    Self::require_args("rfind", args, 1)?;
                    Ok(Variant::Int(
                        arr.iter()
                            .rposition(|v| variant_eq(v, &args[0]))
                            .map(|idx| idx as i64)
                            .unwrap_or(-1),
                    ))
                }
                "count" => {
                    Self::require_args("count", args, 1)?;
                    Ok(Variant::Int(
                        arr.iter().filter(|v| variant_eq(v, &args[0])).count() as i64,
                    ))
                }
                "has" => {
                    Self::require_args("has", args, 1)?;
                    Ok(Variant::Bool(arr.iter().any(|v| variant_eq(v, &args[0]))))
                }
                "erase" => {
                    Self::require_args("erase", args, 1)?;
                    self.array_mutate(arr, object_expr, |a| {
                        if let Some(pos) = a.iter().position(|v| variant_eq(v, &args[0])) {
                            a.remove(pos);
                        }
                        Variant::Nil
                    })
                }
                "insert" => {
                    Self::require_args("insert", args, 2)?;
                    let idx = to_int(&args[0])?.max(0) as usize;
                    self.array_mutate(arr, object_expr, |a| {
                        if idx <= a.len() {
                            a.insert(idx, args[1].clone());
                        }
                        Variant::Nil
                    })
                }
                "append_array" => {
                    Self::require_args("append_array", args, 1)?;
                    let Variant::Array(other) = &args[0] else {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "append_array() argument must be an Array".into(),
                        )));
                    };
                    self.array_mutate(arr, object_expr, |a| {
                        a.extend(other.iter().cloned());
                        Variant::Nil
                    })
                }
                "resize" => {
                    Self::require_args("resize", args, 1)?;
                    let n = to_int(&args[0])?.max(0) as usize;
                    self.array_mutate(arr, object_expr, |a| {
                        a.resize(n, Variant::Nil);
                        Variant::Int(0)
                    })
                }
                "fill" => {
                    Self::require_args("fill", args, 1)?;
                    self.array_mutate(arr, object_expr, |a| {
                        for e in a.iter_mut() {
                            *e = args[0].clone();
                        }
                        Variant::Nil
                    })
                }
                "duplicate" => {
                    // Value semantics make shallow and deep copies identical.
                    Ok(Variant::Array(arr.clone()))
                }
                "max" => {
                    let mut best: Option<Variant> = None;
                    for v in arr {
                        best = Some(match best {
                            None => v.clone(),
                            Some(b) => {
                                if variant_cmp(v, &b) == std::cmp::Ordering::Greater {
                                    v.clone()
                                } else {
                                    b
                                }
                            }
                        });
                    }
                    Ok(best.unwrap_or(Variant::Nil))
                }
                "min" => {
                    let mut best: Option<Variant> = None;
                    for v in arr {
                        best = Some(match best {
                            None => v.clone(),
                            Some(b) => {
                                if variant_cmp(v, &b) == std::cmp::Ordering::Less {
                                    v.clone()
                                } else {
                                    b
                                }
                            }
                        });
                    }
                    Ok(best.unwrap_or(Variant::Nil))
                }
                "bsearch" => {
                    Self::require_args("bsearch", args, 1)?;
                    let mut lo = 0usize;
                    let mut hi = arr.len();
                    while lo < hi {
                        let mid = (lo + hi) / 2;
                        if variant_cmp(&arr[mid], &args[0]) == std::cmp::Ordering::Less {
                            lo = mid + 1;
                        } else {
                            hi = mid;
                        }
                    }
                    Ok(Variant::Int(lo as i64))
                }
                "pick_random" => {
                    if arr.is_empty() {
                        Ok(Variant::Nil)
                    } else {
                        let idx = (deterministic_randi().unsigned_abs() as usize) % arr.len();
                        Ok(arr[idx].clone())
                    }
                }
                "shuffle" => self.array_mutate(arr, object_expr, |a| {
                    let n = a.len();
                    if n > 1 {
                        for i in (1..n).rev() {
                            let j = (deterministic_randi().unsigned_abs() as usize) % (i + 1);
                            a.swap(i, j);
                        }
                    }
                    Variant::Nil
                }),
                "slice" => {
                    if args.is_empty() || args.len() > 4 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "slice() takes 1 to 4 arguments".into(),
                        )));
                    }
                    let begin = to_int(&args[0])?;
                    let end = if args.len() >= 2 {
                        to_int(&args[1])?
                    } else {
                        i64::MAX
                    };
                    let step = if args.len() >= 3 {
                        to_int(&args[2])?
                    } else {
                        1
                    };
                    Ok(Variant::Array(array_slice(arr, begin, end, step)?))
                }
                "map" => {
                    Self::require_args("map", args, 1)?;
                    let callable = extract_callable(&args[0])?;
                    let mut out = Vec::with_capacity(arr.len());
                    for elem in arr {
                        out.push(self.invoke_callable(&callable, &[elem.clone()])?);
                    }
                    Ok(Variant::Array(out))
                }
                "filter" => {
                    Self::require_args("filter", args, 1)?;
                    let callable = extract_callable(&args[0])?;
                    let mut out = Vec::new();
                    for elem in arr {
                        if self
                            .invoke_callable(&callable, &[elem.clone()])?
                            .is_truthy()
                        {
                            out.push(elem.clone());
                        }
                    }
                    Ok(Variant::Array(out))
                }
                "any" => {
                    Self::require_args("any", args, 1)?;
                    let callable = extract_callable(&args[0])?;
                    for elem in arr {
                        if self
                            .invoke_callable(&callable, &[elem.clone()])?
                            .is_truthy()
                        {
                            return Ok(Variant::Bool(true));
                        }
                    }
                    Ok(Variant::Bool(false))
                }
                "all" => {
                    Self::require_args("all", args, 1)?;
                    let callable = extract_callable(&args[0])?;
                    for elem in arr {
                        if !self
                            .invoke_callable(&callable, &[elem.clone()])?
                            .is_truthy()
                        {
                            return Ok(Variant::Bool(false));
                        }
                    }
                    Ok(Variant::Bool(true))
                }
                "reduce" => {
                    if args.is_empty() || args.len() > 2 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "reduce() takes 1 or 2 arguments".into(),
                        )));
                    }
                    let callable = extract_callable(&args[0])?;
                    let mut ret = if args.len() == 2 {
                        args[1].clone()
                    } else {
                        Variant::Nil
                    };
                    let mut start = 0;
                    if matches!(ret, Variant::Nil) && !arr.is_empty() {
                        ret = arr[0].clone();
                        start = 1;
                    }
                    for elem in &arr[start..] {
                        ret = self.invoke_callable(&callable, &[ret, elem.clone()])?;
                    }
                    Ok(ret)
                }
                "sort_custom" => {
                    Self::require_args("sort_custom", args, 1)?;
                    let callable = extract_callable(&args[0])?;
                    let mut items = arr.to_vec();
                    // Stable insertion sort; comparator returns true when the
                    // first argument should sort before the second (Godot).
                    for i in 1..items.len() {
                        let mut j = i;
                        while j > 0 {
                            let before = self
                                .invoke_callable(
                                    &callable,
                                    &[items[j].clone(), items[j - 1].clone()],
                                )?
                                .is_truthy();
                            if before {
                                items.swap(j, j - 1);
                                j -= 1;
                            } else {
                                break;
                            }
                        }
                    }
                    self.write_place(object_expr, Variant::Array(items))?;
                    Ok(Variant::Nil)
                }
                _ => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
                    format!("Array.{method}"),
                ))),
            },
            Variant::String(s) => match method {
                // Character count, not byte count (multibyte-safe).
                "length" => Ok(Variant::Int(s.chars().count() as i64)),
                "is_empty" => Ok(Variant::Bool(s.is_empty())),
                "to_upper" => Ok(Variant::String(s.to_uppercase())),
                "to_lower" => Ok(Variant::String(s.to_lowercase())),
                "begins_with" => {
                    Self::require_args("begins_with", args, 1)?;
                    Ok(Variant::Bool(s.starts_with(&arg_str(&args[0]))))
                }
                "ends_with" => {
                    Self::require_args("ends_with", args, 1)?;
                    Ok(Variant::Bool(s.ends_with(&arg_str(&args[0]))))
                }
                "contains" => {
                    Self::require_args("contains", args, 1)?;
                    Ok(Variant::Bool(s.contains(&arg_str(&args[0]))))
                }
                "containsn" => {
                    Self::require_args("containsn", args, 1)?;
                    Ok(Variant::Bool(
                        s.to_lowercase().contains(&arg_str(&args[0]).to_lowercase()),
                    ))
                }
                "split" => {
                    if args.is_empty() {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "split() takes at least 1 argument".into(),
                        )));
                    }
                    let d = arg_str(&args[0]);
                    let parts: Vec<Variant> = if d.is_empty() {
                        s.chars().map(|c| Variant::String(c.to_string())).collect()
                    } else {
                        s.split(d.as_str())
                            .map(|pp| Variant::String(pp.to_string()))
                            .collect()
                    };
                    Ok(Variant::Array(parts))
                }
                "rsplit" => {
                    if args.is_empty() {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "rsplit() takes at least 1 argument".into(),
                        )));
                    }
                    let d = arg_str(&args[0]);
                    // Without a maxsplit limit, rsplit produces the same
                    // (left-to-right) segments as split.
                    let parts: Vec<Variant> = if d.is_empty() {
                        vec![Variant::String(s.clone())]
                    } else {
                        s.split(d.as_str())
                            .map(|pp| Variant::String(pp.to_string()))
                            .collect()
                    };
                    Ok(Variant::Array(parts))
                }
                "join" => {
                    Self::require_args("join", args, 1)?;
                    match &args[0] {
                        Variant::Array(a) => {
                            let parts: Vec<String> = a.iter().map(|v| format!("{v}")).collect();
                            Ok(Variant::String(parts.join(s)))
                        }
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "join() arg must be array".into(),
                        ))),
                    }
                }
                "replace" => {
                    Self::require_args("replace", args, 2)?;
                    Ok(Variant::String(
                        s.replace(&arg_str(&args[0]), &arg_str(&args[1])),
                    ))
                }
                "find" => {
                    if args.is_empty() || args.len() > 2 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "find() takes 1 or 2 arguments".into(),
                        )));
                    }
                    let needle: Vec<char> = arg_str(&args[0]).chars().collect();
                    let from = args
                        .get(1)
                        .map(|v| to_int(v).unwrap_or(0).max(0) as usize)
                        .unwrap_or(0);
                    let chars: Vec<char> = s.chars().collect();
                    Ok(Variant::Int(str_find(&chars, &needle, from)))
                }
                "findn" => {
                    Self::require_args_range("findn", args, 1, 2)?;
                    let needle: Vec<char> = arg_str(&args[0]).to_lowercase().chars().collect();
                    let from = args
                        .get(1)
                        .map(|v| to_int(v).unwrap_or(0).max(0) as usize)
                        .unwrap_or(0);
                    let chars: Vec<char> = s.to_lowercase().chars().collect();
                    Ok(Variant::Int(str_find(&chars, &needle, from)))
                }
                "rfind" => {
                    Self::require_args_range("rfind", args, 1, 2)?;
                    let needle: Vec<char> = arg_str(&args[0]).chars().collect();
                    let chars: Vec<char> = s.chars().collect();
                    let from = match args.get(1).map(|v| to_int(v).unwrap_or(-1)) {
                        Some(f) if f >= 0 => Some(f as usize),
                        _ => None,
                    };
                    Ok(Variant::Int(str_rfind(&chars, &needle, from)))
                }
                "rfindn" => {
                    Self::require_args_range("rfindn", args, 1, 2)?;
                    let needle: Vec<char> = arg_str(&args[0]).to_lowercase().chars().collect();
                    let chars: Vec<char> = s.to_lowercase().chars().collect();
                    let from = match args.get(1).map(|v| to_int(v).unwrap_or(-1)) {
                        Some(f) if f >= 0 => Some(f as usize),
                        _ => None,
                    };
                    Ok(Variant::Int(str_rfind(&chars, &needle, from)))
                }
                "count" => {
                    Self::require_args_range("count", args, 1, 3)?;
                    let needle: Vec<char> = arg_str(&args[0]).chars().collect();
                    let chars: Vec<char> = s.chars().collect();
                    let from = args
                        .get(1)
                        .map(|v| to_int(v).unwrap_or(0).max(0) as usize)
                        .unwrap_or(0);
                    let to = args
                        .get(2)
                        .map(|v| to_int(v).unwrap_or(0).max(0) as usize)
                        .unwrap_or(0);
                    Ok(Variant::Int(str_count(&chars, &needle, from, to)))
                }
                "countn" => {
                    Self::require_args_range("countn", args, 1, 3)?;
                    let needle: Vec<char> = arg_str(&args[0]).to_lowercase().chars().collect();
                    let chars: Vec<char> = s.to_lowercase().chars().collect();
                    let from = args
                        .get(1)
                        .map(|v| to_int(v).unwrap_or(0).max(0) as usize)
                        .unwrap_or(0);
                    let to = args
                        .get(2)
                        .map(|v| to_int(v).unwrap_or(0).max(0) as usize)
                        .unwrap_or(0);
                    Ok(Variant::Int(str_count(&chars, &needle, from, to)))
                }
                "substr" => {
                    Self::require_args_range("substr", args, 1, 2)?;
                    let from = to_int(&args[0])?.max(0) as usize;
                    let len = args.get(1).map(|v| to_int(v).unwrap_or(-1)).unwrap_or(-1);
                    let take = if len < 0 { usize::MAX } else { len as usize };
                    Ok(Variant::String(s.chars().skip(from).take(take).collect()))
                }
                "left" => {
                    Self::require_args("left", args, 1)?;
                    Ok(Variant::String(str_left(s, to_int(&args[0])?)))
                }
                "right" => {
                    Self::require_args("right", args, 1)?;
                    Ok(Variant::String(str_right(s, to_int(&args[0])?)))
                }
                "repeat" => {
                    Self::require_args("repeat", args, 1)?;
                    let n = to_int(&args[0])?;
                    Ok(Variant::String(if n <= 0 {
                        String::new()
                    } else {
                        s.repeat(n as usize)
                    }))
                }
                "strip_edges" => {
                    let left = args.first().map(|v| v.is_truthy()).unwrap_or(true);
                    let right = args.get(1).map(|v| v.is_truthy()).unwrap_or(true);
                    let mut r = s.as_str();
                    if left {
                        r = r.trim_start();
                    }
                    if right {
                        r = r.trim_end();
                    }
                    Ok(Variant::String(r.to_string()))
                }
                "strip_escapes" => Ok(Variant::String(
                    s.chars().filter(|c| (*c as u32) >= 32).collect(),
                )),
                "lstrip" => {
                    Self::require_args("lstrip", args, 1)?;
                    let set: Vec<char> = arg_str(&args[0]).chars().collect();
                    Ok(Variant::String(
                        s.trim_start_matches(|c| set.contains(&c)).to_string(),
                    ))
                }
                "rstrip" => {
                    Self::require_args("rstrip", args, 1)?;
                    let set: Vec<char> = arg_str(&args[0]).chars().collect();
                    Ok(Variant::String(
                        s.trim_end_matches(|c| set.contains(&c)).to_string(),
                    ))
                }
                "trim_prefix" => {
                    Self::require_args("trim_prefix", args, 1)?;
                    let p = arg_str(&args[0]);
                    Ok(Variant::String(
                        s.strip_prefix(&p).unwrap_or(s.as_str()).to_string(),
                    ))
                }
                "trim_suffix" => {
                    Self::require_args("trim_suffix", args, 1)?;
                    let p = arg_str(&args[0]);
                    Ok(Variant::String(
                        s.strip_suffix(&p).unwrap_or(s.as_str()).to_string(),
                    ))
                }
                "to_int" => Ok(Variant::Int(str_to_int(s))),
                "to_float" => Ok(Variant::Float(str_to_float(s))),
                "is_valid_int" => Ok(Variant::Bool(str_is_valid_int(s))),
                "is_valid_float" => Ok(Variant::Bool(str_is_valid_float(s))),
                "hex_to_int" => {
                    let mut t = s.trim();
                    let mut sign = 1i64;
                    if let Some(r) = t.strip_prefix('-') {
                        sign = -1;
                        t = r;
                    } else if let Some(r) = t.strip_prefix('+') {
                        t = r;
                    }
                    let t = t
                        .strip_prefix("0x")
                        .or_else(|| t.strip_prefix("0X"))
                        .unwrap_or(t);
                    Ok(Variant::Int(sign * i64::from_str_radix(t, 16).unwrap_or(0)))
                }
                "bin_to_int" => {
                    let mut t = s.trim();
                    let mut sign = 1i64;
                    if let Some(r) = t.strip_prefix('-') {
                        sign = -1;
                        t = r;
                    } else if let Some(r) = t.strip_prefix('+') {
                        t = r;
                    }
                    let t = t
                        .strip_prefix("0b")
                        .or_else(|| t.strip_prefix("0B"))
                        .unwrap_or(t);
                    Ok(Variant::Int(sign * i64::from_str_radix(t, 2).unwrap_or(0)))
                }
                "format" => {
                    Self::require_args_range("format", args, 1, 2)?;
                    Ok(Variant::String(gd_format(s, &args[0])))
                }
                "lpad" => {
                    Self::require_args_range("lpad", args, 1, 2)?;
                    let min = to_int(&args[0])?;
                    let pad = args
                        .get(1)
                        .map(arg_str)
                        .and_then(|p| p.chars().next())
                        .unwrap_or(' ');
                    let cur = s.chars().count() as i64;
                    let mut out = String::new();
                    for _ in 0..(min - cur).max(0) {
                        out.push(pad);
                    }
                    out.push_str(s);
                    Ok(Variant::String(out))
                }
                "rpad" => {
                    Self::require_args_range("rpad", args, 1, 2)?;
                    let min = to_int(&args[0])?;
                    let pad = args
                        .get(1)
                        .map(arg_str)
                        .and_then(|p| p.chars().next())
                        .unwrap_or(' ');
                    let cur = s.chars().count() as i64;
                    let mut out = s.clone();
                    for _ in 0..(min - cur).max(0) {
                        out.push(pad);
                    }
                    Ok(Variant::String(out))
                }
                "pad_zeros" => {
                    Self::require_args("pad_zeros", args, 1)?;
                    let digits = to_int(&args[0])?;
                    let (sign, rest) = if let Some(r) = s.strip_prefix('-') {
                        ("-", r)
                    } else if let Some(r) = s.strip_prefix('+') {
                        ("+", r)
                    } else {
                        ("", s.as_str())
                    };
                    let (int_part, frac_part) = match rest.find('.') {
                        Some(p) => (&rest[..p], &rest[p..]),
                        None => (rest, ""),
                    };
                    let cur = int_part.chars().count() as i64;
                    let mut ip = String::new();
                    for _ in 0..(digits - cur).max(0) {
                        ip.push('0');
                    }
                    ip.push_str(int_part);
                    Ok(Variant::String(format!("{sign}{ip}{frac_part}")))
                }
                "pad_decimals" => {
                    Self::require_args("pad_decimals", args, 1)?;
                    let digits = to_int(&args[0])?.max(0) as usize;
                    let out = match s.find('.') {
                        Some(p) => {
                            let int_part = &s[..p];
                            let frac = &s[p + 1..];
                            if digits == 0 {
                                int_part.to_string()
                            } else {
                                let mut f: String = frac.chars().take(digits).collect();
                                while f.chars().count() < digits {
                                    f.push('0');
                                }
                                format!("{int_part}.{f}")
                            }
                        }
                        None => {
                            if digits == 0 {
                                s.clone()
                            } else {
                                format!("{s}.{}", "0".repeat(digits))
                            }
                        }
                    };
                    Ok(Variant::String(out))
                }
                "capitalize" => Ok(Variant::String(gd_capitalize(s))),
                "to_snake_case" => Ok(Variant::String(gd_to_snake_case(s))),
                "to_camel_case" => Ok(Variant::String(gd_to_camel_case(s))),
                "to_pascal_case" => Ok(Variant::String(gd_to_pascal_case(s))),
                "dedent" => Ok(Variant::String(gd_dedent(s))),
                "indent" => {
                    Self::require_args("indent", args, 1)?;
                    Ok(Variant::String(gd_indent(s, &arg_str(&args[0]))))
                }
                "get_extension" => Ok(Variant::String(path_get_extension(s))),
                "get_basename" => Ok(Variant::String(path_get_basename(s))),
                "get_file" => Ok(Variant::String(path_get_file(s))),
                "get_base_dir" => Ok(Variant::String(path_get_base_dir(s))),
                "path_join" => {
                    Self::require_args("path_join", args, 1)?;
                    Ok(Variant::String(path_join(s, &arg_str(&args[0]))))
                }
                "simplify_path" => Ok(Variant::String(gd_simplify_path(s))),
                "is_absolute_path" => Ok(Variant::Bool(path_is_absolute(s))),
                "is_relative_path" => Ok(Variant::Bool(!path_is_absolute(s))),
                "xml_escape" => {
                    let quotes = args.first().map(|v| v.is_truthy()).unwrap_or(false);
                    Ok(Variant::String(gd_xml_escape(s, quotes)))
                }
                "xml_unescape" => Ok(Variant::String(gd_xml_unescape(s))),
                "c_escape" => Ok(Variant::String(gd_c_escape(s))),
                "c_unescape" => Ok(Variant::String(gd_c_unescape(s))),
                "json_escape" => Ok(Variant::String(gd_json_escape(s))),
                "uri_encode" => Ok(Variant::String(gd_uri_encode(s))),
                "uri_decode" => Ok(Variant::String(gd_uri_decode(s))),
                "hash" => Ok(Variant::Int(str_hash(s))),
                "sha256_text" => Ok(Variant::String(gdcore::crypto::sha256_hex(s.as_bytes()))),
                "similarity" => {
                    Self::require_args("similarity", args, 1)?;
                    Ok(Variant::Float(str_similarity(s, &arg_str(&args[0]))))
                }
                "casecmp_to" => {
                    Self::require_args("casecmp_to", args, 1)?;
                    Ok(Variant::Int(ordering_to_int(
                        s.as_str().cmp(arg_str(&args[0]).as_str()),
                    )))
                }
                "nocasecmp_to" => {
                    Self::require_args("nocasecmp_to", args, 1)?;
                    Ok(Variant::Int(ordering_to_int(
                        s.to_lowercase().cmp(&arg_str(&args[0]).to_lowercase()),
                    )))
                }
                "naturalnocasecmp_to" => {
                    Self::require_args("naturalnocasecmp_to", args, 1)?;
                    Ok(Variant::Int(ordering_to_int(natural_cmp(
                        s,
                        &arg_str(&args[0]),
                        true,
                    ))))
                }
                _ => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
                    format!("String.{method}"),
                ))),
            },
            Variant::Dictionary(d) => match method {
                "size" => Ok(Variant::Int(d.len() as i64)),
                "is_empty" => Ok(Variant::Bool(d.is_empty())),
                "has" => {
                    if args.len() != 1 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "has() takes 1 argument".into(),
                        )));
                    }
                    Ok(Variant::Bool(d.contains_key(&args[0])))
                }
                "has_all" => {
                    if args.len() != 1 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "has_all() takes 1 argument".into(),
                        )));
                    }
                    match &args[0] {
                        Variant::Array(keys) => Ok(Variant::Bool(d.has_all(keys))),
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "has_all() arg must be an array".into(),
                        ))),
                    }
                }
                "keys" => Ok(Variant::Array(d.keys().cloned().collect())),
                "values" => Ok(Variant::Array(d.values().cloned().collect())),
                "find_key" => {
                    if args.len() != 1 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "find_key() takes 1 argument".into(),
                        )));
                    }
                    Ok(d.find_key(&args[0]))
                }
                "duplicate" => {
                    let deep = matches!(args.first(), Some(Variant::Bool(true)));
                    Ok(Variant::Dictionary(d.duplicate(deep)))
                }
                "erase" => {
                    if args.len() != 1 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "erase() takes 1 argument".into(),
                        )));
                    }
                    let mut c = self.eval_expr(object_expr)?;
                    let existed = if let Variant::Dictionary(ref mut dm) = c {
                        dm.remove(&args[0]).is_some()
                    } else {
                        false
                    };
                    self.write_place(object_expr, c)?;
                    Ok(Variant::Bool(existed))
                }
                "clear" => {
                    let mut c = self.eval_expr(object_expr)?;
                    if let Variant::Dictionary(ref mut dm) = c {
                        dm.clear();
                    }
                    self.write_place(object_expr, c)?;
                    Ok(Variant::Nil)
                }
                "merge" => {
                    if args.is_empty() || args.len() > 2 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "merge() takes 1-2 arguments".into(),
                        )));
                    }
                    match &args[0] {
                        Variant::Dictionary(other) => {
                            let overwrite = matches!(args.get(1), Some(Variant::Bool(true)));
                            let mut c = self.eval_expr(object_expr)?;
                            if let Variant::Dictionary(ref mut dm) = c {
                                dm.merge(other, overwrite);
                            }
                            self.write_place(object_expr, c)?;
                            Ok(Variant::Nil)
                        }
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "merge() arg must be dictionary".into(),
                        ))),
                    }
                }
                "merged" => {
                    if args.is_empty() || args.len() > 2 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "merged() takes 1-2 arguments".into(),
                        )));
                    }
                    match &args[0] {
                        Variant::Dictionary(other) => {
                            let overwrite = matches!(args.get(1), Some(Variant::Bool(true)));
                            Ok(Variant::Dictionary(d.merged(other, overwrite)))
                        }
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "merged() arg must be dictionary".into(),
                        ))),
                    }
                }
                "get" => {
                    if args.is_empty() || args.len() > 2 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "get() takes 1-2 arguments".into(),
                        )));
                    }
                    let default = if args.len() == 2 {
                        args[1].clone()
                    } else {
                        Variant::Nil
                    };
                    Ok(d.get(&args[0]).cloned().unwrap_or(default))
                }
                "get_or_add" => {
                    if args.is_empty() || args.len() > 2 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "get_or_add() takes 1-2 arguments".into(),
                        )));
                    }
                    let default = if args.len() == 2 {
                        args[1].clone()
                    } else {
                        Variant::Nil
                    };
                    let mut c = self.eval_expr(object_expr)?;
                    let result = if let Variant::Dictionary(ref mut dm) = c {
                        dm.get_or_add(args[0].clone(), default)
                    } else {
                        Variant::Nil
                    };
                    self.write_place(object_expr, c)?;
                    Ok(result)
                }
                _ => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
                    format!("Dictionary.{method}"),
                ))),
            },
            Variant::Vector2(v) => match method {
                "length" => Ok(Variant::Float(v.length() as f64)),
                "length_squared" => Ok(Variant::Float(v.length_squared() as f64)),
                "normalized" => Ok(Variant::Vector2(v.normalized())),
                "dot" => {
                    if args.len() != 1 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "dot() takes 1 argument".into(),
                        )));
                    }
                    match &args[0] {
                        Variant::Vector2(other) => Ok(Variant::Float(v.dot(*other) as f64)),
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "dot() argument must be Vector2".into(),
                        ))),
                    }
                }
                "distance_to" => {
                    if args.len() != 1 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "distance_to() takes 1 argument".into(),
                        )));
                    }
                    match &args[0] {
                        Variant::Vector2(other) => Ok(Variant::Float(v.distance_to(*other) as f64)),
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "distance_to() argument must be Vector2".into(),
                        ))),
                    }
                }
                "lerp" => {
                    if args.len() != 2 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "lerp() takes 2 arguments".into(),
                        )));
                    }
                    match (&args[0], &args[1]) {
                        (Variant::Vector2(to), t) => {
                            let t = to_float(t)? as f32;
                            Ok(Variant::Vector2(v.lerp(*to, t)))
                        }
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "lerp() first argument must be Vector2".into(),
                        ))),
                    }
                }
                "angle" => Ok(Variant::Float(v.angle() as f64)),
                "cross" => {
                    if args.len() != 1 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "cross() takes 1 argument".into(),
                        )));
                    }
                    match &args[0] {
                        Variant::Vector2(other) => Ok(Variant::Float(v.cross(*other) as f64)),
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "cross() argument must be Vector2".into(),
                        ))),
                    }
                }
                _ => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
                    format!("Vector2.{method}"),
                ))),
            },
            Variant::Vector3(v) => match method {
                "length" => Ok(Variant::Float(v.length() as f64)),
                "length_squared" => Ok(Variant::Float(v.length_squared() as f64)),
                "normalized" => Ok(Variant::Vector3(v.normalized())),
                "dot" => {
                    if args.len() != 1 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "dot() takes 1 argument".into(),
                        )));
                    }
                    match &args[0] {
                        Variant::Vector3(other) => Ok(Variant::Float(v.dot(*other) as f64)),
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "dot() argument must be Vector3".into(),
                        ))),
                    }
                }
                "cross" => {
                    if args.len() != 1 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "cross() takes 1 argument".into(),
                        )));
                    }
                    match &args[0] {
                        Variant::Vector3(other) => Ok(Variant::Vector3(v.cross(*other))),
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "cross() argument must be Vector3".into(),
                        ))),
                    }
                }
                "distance_to" => {
                    if args.len() != 1 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "distance_to() takes 1 argument".into(),
                        )));
                    }
                    match &args[0] {
                        Variant::Vector3(other) => Ok(Variant::Float(v.distance_to(*other) as f64)),
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "distance_to() argument must be Vector3".into(),
                        ))),
                    }
                }
                "lerp" => {
                    if args.len() != 2 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "lerp() takes 2 arguments".into(),
                        )));
                    }
                    match (&args[0], &args[1]) {
                        (Variant::Vector3(to), t) => {
                            let t = to_float(t)? as f32;
                            Ok(Variant::Vector3(v.lerp(*to, t)))
                        }
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "lerp() first argument must be Vector3".into(),
                        ))),
                    }
                }
                _ => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
                    format!("Vector3.{method}"),
                ))),
            },
            // The affine builtins delegate to `variant_math`, which forwards to
            // the already-tested `gdcore` implementations (pat-0q2kx.50).
            Variant::Rect2(_)
            | Variant::Transform2D(_)
            | Variant::Transform3D(_)
            | Variant::Basis(_)
            | Variant::Quaternion(_)
            | Variant::Aabb(_)
            | Variant::Plane(_) => match crate::variant_math::call_method(obj, method, args) {
                Some(result) => result,
                None => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
                    format!("{}.{method}", obj.variant_type()),
                ))),
            },
            Variant::ObjectId(oid) => {
                let id = oid.raw();
                match method {
                    "get_children" => {
                        if let Some(ref access) = self.scene_access {
                            let children = access.get_children(id);
                            let arr: Vec<Variant> = children
                                .iter()
                                .map(|c| Variant::ObjectId(gdcore::id::ObjectId::from_raw(*c)))
                                .collect();
                            Ok(Variant::Array(arr))
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    "get_child_count" => {
                        if let Some(ref access) = self.scene_access {
                            let children = access.get_children(id);
                            Ok(Variant::Int(children.len() as i64))
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    "get_parent" => {
                        if let Some(ref access) = self.scene_access {
                            match access.get_parent(id) {
                                Some(pid) => {
                                    Ok(Variant::ObjectId(gdcore::id::ObjectId::from_raw(pid)))
                                }
                                None => Ok(Variant::Nil),
                            }
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    "get_node" => {
                        if args.len() != 1 {
                            return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "get_node() takes 1 argument".into(),
                            )));
                        }
                        let path = match &args[0] {
                            Variant::String(s) => s.clone(),
                            _ => {
                                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                    "get_node() arg must be string".into(),
                                )))
                            }
                        };
                        if let Some(ref access) = self.scene_access {
                            match access.get_node(id, &path) {
                                Some(tid) => {
                                    Ok(Variant::ObjectId(gdcore::id::ObjectId::from_raw(tid)))
                                }
                                None => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                    format!("node not found: {path}"),
                                ))),
                            }
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    "connect" => {
                        if args.len() < 2 {
                            return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "connect() needs a signal and a target".into(),
                            )));
                        }
                        let signal = match &args[0] {
                            Variant::String(s) => s.clone(),
                            Variant::StringName(s) => s.as_str().to_string(),
                            _ => {
                                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                    "connect() signal must be string".into(),
                                )))
                            }
                        };
                        let self_node = self.current_node_id;
                        // Two accepted shapes (pat-0q2kx.9):
                        //   Godot 4  — connect(signal, Callable[, flags])
                        //   legacy   — connect(signal, ObjectId, method[, flags])
                        // Godot 4 removed the 3-arg form, but Patina scripts
                        // predate first-class Callables, so both are honoured
                        // and the flags argument shifts accordingly.
                        let (target_id, method_name, flags_index) = match &args[1] {
                            Variant::Callable(_) => match callable_target(&args[1], self_node) {
                                Some((t, m)) => (t, m, 2),
                                None => {
                                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                        "connect() Callable has no bindable target/method".into(),
                                    )))
                                }
                            },
                            Variant::ObjectId(tid) => {
                                let m = match args.get(2) {
                                    Some(Variant::String(s)) => s.clone(),
                                    Some(Variant::StringName(s)) => s.as_str().to_string(),
                                    _ => {
                                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                            "connect() method must be string".into(),
                                        )))
                                    }
                                };
                                (tid.raw(), m, 3)
                            }
                            _ => {
                                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                    "connect() target must be a Callable or ObjectId".into(),
                                )))
                            }
                        };
                        // Godot ConnectFlags bitmask (DEFERRED=1, ONE_SHOT=4, …).
                        let flags = match args.get(flags_index) {
                            Some(Variant::Int(f)) => *f,
                            _ => 0,
                        };
                        if let Some(ref mut access) = self.scene_access {
                            access.connect_signal_flags(
                                id,
                                &signal,
                                target_id,
                                &method_name,
                                flags,
                                &[],
                            );
                            Ok(Variant::Nil)
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    "disconnect" => {
                        if args.len() < 3 {
                            return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "disconnect() needs signal, target, method".into(),
                            )));
                        }
                        let signal = match &args[0] {
                            Variant::String(s) => s.clone(),
                            _ => {
                                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                    "disconnect() signal must be string".into(),
                                )))
                            }
                        };
                        let target_id = match &args[1] {
                            Variant::ObjectId(tid) => tid.raw(),
                            _ => {
                                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                    "disconnect() target must be ObjectId".into(),
                                )))
                            }
                        };
                        let method_name = match &args[2] {
                            Variant::String(s) => s.clone(),
                            _ => {
                                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                    "disconnect() method must be string".into(),
                                )))
                            }
                        };
                        if let Some(ref mut access) = self.scene_access {
                            let removed =
                                access.disconnect_signal(id, &signal, target_id, &method_name);
                            Ok(Variant::Bool(removed))
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    "is_connected" => {
                        if args.len() < 3 {
                            return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "is_connected() needs signal, target, method".into(),
                            )));
                        }
                        let signal = match &args[0] {
                            Variant::String(s) => s.clone(),
                            _ => {
                                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                    "is_connected() signal must be string".into(),
                                )))
                            }
                        };
                        let target_id = match &args[1] {
                            Variant::ObjectId(tid) => tid.raw(),
                            _ => {
                                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                    "is_connected() target must be ObjectId".into(),
                                )))
                            }
                        };
                        let method_name = match &args[2] {
                            Variant::String(s) => s.clone(),
                            _ => {
                                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                    "is_connected() method must be string".into(),
                                )))
                            }
                        };
                        if let Some(ref access) = self.scene_access {
                            Ok(Variant::Bool(access.is_signal_connected(
                                id,
                                &signal,
                                target_id,
                                &method_name,
                            )))
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    "get_name" => {
                        if let Some(ref access) = self.scene_access {
                            match access.get_node_name(id) {
                                Some(name) => Ok(Variant::String(name)),
                                None => Ok(Variant::Nil),
                            }
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    "add_child" => {
                        if args.len() != 1 {
                            return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "add_child() takes 1 argument".into(),
                            )));
                        }
                        let child_id = match &args[0] {
                            Variant::ObjectId(oid) => oid.raw(),
                            _ => {
                                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                    "add_child() argument must be a node reference".into(),
                                )))
                            }
                        };
                        if let Some(ref mut access) = self.scene_access {
                            access.add_child(id, child_id);
                            Ok(Variant::Nil)
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    "queue_free" => {
                        if let Some(ref mut access) = self.scene_access {
                            access.queue_free(id);
                            Ok(Variant::Nil)
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    "free" => {
                        // Godot `Object.free()`: immediate destruction (unlike
                        // `queue_free`, the object is gone the moment this
                        // returns, so `is_instance_valid` reports false at once).
                        if let Some(ref mut access) = self.scene_access {
                            access.free_object(id);
                            Ok(Variant::Nil)
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    "get_ref" => {
                        // `WeakRef.get_ref()`: the live object reference, or Nil
                        // once the referent has been freed.
                        let alive = self
                            .scene_access
                            .as_ref()
                            .map(|access| access.is_object_alive(id))
                            .unwrap_or(true);
                        Ok(if alive {
                            Variant::ObjectId(gdcore::id::ObjectId::from_raw(id))
                        } else {
                            Variant::Nil
                        })
                    }
                    "call_deferred" => {
                        if args.is_empty() {
                            return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "call_deferred() requires a method name".into(),
                            )));
                        }
                        let method = match &args[0] {
                            Variant::String(s) => s.clone(),
                            other => {
                                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                    format!(
                                    "call_deferred() method name must be a String, got {other:?}"
                                ),
                                )))
                            }
                        };
                        if let Some(ref mut access) = self.scene_access {
                            access.call_deferred(id, &method, &args[1..]);
                            Ok(Variant::Nil)
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    "get_class" => {
                        if let Some(ref access) = self.scene_access {
                            match access.get_class(id) {
                                Some(cls) => Ok(Variant::String(cls)),
                                None => Ok(Variant::Nil),
                            }
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    // -- Object introspection (pat-0q2kx.64) --------------
                    //
                    // Godot's save systems, serializers and plugin code walk
                    // `get_property_list()` expecting Array[Dictionary] with
                    // name/class_name/type/hint/hint_string/usage
                    // (`core/object/object.cpp`). Patina exposed none of it, so a
                    // script could not enumerate — let alone revert — a property.
                    "get_property_list" => {
                        let class = self
                            .scene_access
                            .as_ref()
                            .and_then(|a| a.get_class(id))
                            .unwrap_or_default();
                        let props = gdobject::class_db::get_property_list(&class)
                            .into_iter()
                            .map(|p| {
                                let mut d = gdvariant::Dictionary::new();
                                d.insert("name", Variant::String(p.name));
                                // Godot fills `class_name` only for object-typed
                                // properties; the hint string carries the class
                                // there, so mirroring it keeps the dict honest
                                // rather than inventing a name for every entry.
                                d.insert("class_name", Variant::String(p.hint_string.clone()));
                                d.insert(
                                    "type",
                                    Variant::Int(crate::global_constants::variant_type_code(
                                        &p.default_value,
                                    )),
                                );
                                d.insert("hint", Variant::Int(p.hint as i64));
                                d.insert("hint_string", Variant::String(p.hint_string));
                                d.insert("usage", Variant::Int(p.usage as i64));
                                Variant::Dictionary(d)
                            })
                            .collect();
                        Ok(Variant::Array(props))
                    }
                    "get_method_list" => {
                        let class = self
                            .scene_access
                            .as_ref()
                            .and_then(|a| a.get_class(id))
                            .unwrap_or_default();
                        let methods = gdobject::class_db::get_method_list(&class)
                            .into_iter()
                            .map(|m| {
                                let mut d = gdvariant::Dictionary::new();
                                d.insert("name", Variant::String(m.name));
                                d.insert("args", Variant::Int(m.argument_count as i64));
                                Variant::Dictionary(d)
                            })
                            .collect();
                        Ok(Variant::Array(methods))
                    }
                    "get_signal_list" => {
                        let class = self
                            .scene_access
                            .as_ref()
                            .and_then(|a| a.get_class(id))
                            .unwrap_or_default();
                        let signals = gdobject::class_db::get_signal_list(&class)
                            .into_iter()
                            .map(|sig| {
                                let mut d = gdvariant::Dictionary::new();
                                d.insert("name", Variant::String(sig.name));
                                let args = sig
                                    .arguments
                                    .into_iter()
                                    .map(Variant::String)
                                    .collect::<Vec<_>>();
                                d.insert("args", Variant::Array(args));
                                Variant::Dictionary(d)
                            })
                            .collect();
                        Ok(Variant::Array(signals))
                    }
                    "property_can_revert" => {
                        let class = self
                            .scene_access
                            .as_ref()
                            .and_then(|a| a.get_class(id))
                            .unwrap_or_default();
                        let name = match args.first() {
                            Some(Variant::String(s)) => s.clone(),
                            Some(Variant::StringName(s)) => s.as_str().to_string(),
                            _ => String::new(),
                        };
                        Ok(Variant::Bool(gdobject::class_db::property_can_revert(
                            &class, &name,
                        )))
                    }
                    "get_signal_connection_list" => {
                        let signal_name = match args.first() {
                            Some(Variant::String(s)) => s.clone(),
                            Some(Variant::StringName(s)) => s.as_str().to_string(),
                            _ => String::new(),
                        };
                        let conns = self
                            .scene_access
                            .as_ref()
                            .map(|a| a.signal_connection_list(id, &signal_name))
                            .unwrap_or_default()
                            .into_iter()
                            .map(|(target, method, one_shot)| {
                                let mut d = Dictionary::new();
                                d.insert("signal", Variant::String(signal_name.clone()));
                                d.insert("callable", Variant::String(method));
                                d.insert("target", Variant::Int(target as i64));
                                // Godot reports the connect flags; ONE_SHOT is 1.
                                d.insert("flags", Variant::Int(i64::from(one_shot)));
                                Variant::Dictionary(d)
                            })
                            .collect();
                        Ok(Variant::Array(conns))
                    }
                    "get_incoming_connections" => {
                        let conns = self
                            .scene_access
                            .as_ref()
                            .map(|a| a.incoming_connections(id))
                            .unwrap_or_default()
                            .into_iter()
                            .map(|(source, signal_name, method)| {
                                let mut d = Dictionary::new();
                                d.insert("source", Variant::Int(source as i64));
                                d.insert("signal", Variant::String(signal_name));
                                d.insert("callable", Variant::String(method));
                                Variant::Dictionary(d)
                            })
                            .collect();
                        Ok(Variant::Array(conns))
                    }
                    "has_connections" => {
                        // Godot's `has_connections(signal)` asks about ONE signal;
                        // with no argument this answers "any signal at all", which
                        // is what the tree-wide check is useful for.
                        let per_signal = match args.first() {
                            Some(Variant::String(s)) => Some(s.clone()),
                            Some(Variant::StringName(s)) => Some(s.as_str().to_string()),
                            _ => None,
                        };
                        let connected = match (self.scene_access.as_ref(), per_signal) {
                            (Some(access), Some(name)) => {
                                !access.signal_connection_list(id, &name).is_empty()
                            }
                            (Some(access), None) => access.has_any_connections(id),
                            (None, _) => false,
                        };
                        Ok(Variant::Bool(connected))
                    }
                    "notify_property_list_changed" => {
                        if let Some(ref mut access) = self.scene_access {
                            access.notify_property_list_changed(id);
                        }
                        Ok(Variant::Nil)
                    }
                    "property_get_revert" => {
                        let class = self
                            .scene_access
                            .as_ref()
                            .and_then(|a| a.get_class(id))
                            .unwrap_or_default();
                        let name = match args.first() {
                            Some(Variant::String(s)) => s.clone(),
                            Some(Variant::StringName(s)) => s.as_str().to_string(),
                            _ => String::new(),
                        };
                        Ok(gdobject::class_db::property_get_revert(&class, &name)
                            .unwrap_or(Variant::Nil))
                    }
                    "get_instance_id" => {
                        // Godot `Object.get_instance_id()`: the object's unique
                        // id, usable with `instance_from_id` / `weakref`.
                        Ok(Variant::Int(id as i64))
                    }
                    // -- AudioStreamPlayer(2D/3D) playback control (pat-nzgdu.2) --
                    // Godot scripts drive audio via method calls; route them to the
                    // node properties that `AudioServerBridge::sync_player_node`
                    // polls each frame. `playing` starts/stops the voice,
                    // `stream_paused` pauses it, `seek_request` requests a seek, and
                    // `playback_position` is read back. Before this, these calls hit
                    // the `_` arm below and threw `UndefinedFunction("ObjectId.play")`.
                    "play" => {
                        if let Some(ref mut access) = self.scene_access {
                            // `AnimationPlayer.play("walk")` names a *clip*,
                            // while `AudioStreamPlayer.play(0.5)` names a seek
                            // position — both land here as `ObjectId.play`.
                            // Route to the attached AnimationPlayer first; only
                            // a node without one falls through to the audio
                            // semantics below (pat-0q2kx.1). Before this,
                            // `$AnimationPlayer.play("walk")` silently dropped
                            // the clip name and set `playing = true` instead.
                            let anim_clip = match args.first() {
                                Some(Variant::String(s)) => Some(s.clone()),
                                Some(Variant::StringName(s)) => Some(s.as_str().to_string()),
                                None => Some(String::new()),
                                _ => None,
                            };
                            if let Some(clip) = anim_clip {
                                if access.animation_player_play(id, &clip) {
                                    return Ok(Variant::Nil);
                                }
                            }
                            // Optional `from` position: request a seek first.
                            if let Some(from) = args.first() {
                                let pos = match from {
                                    Variant::Float(f) => *f,
                                    Variant::Int(i) => *i as f64,
                                    _ => 0.0,
                                };
                                if pos > 0.0 {
                                    access.set_node_property(
                                        id,
                                        "seek_request",
                                        Variant::Float(pos),
                                    );
                                }
                            }
                            access.set_node_property(id, "playing", Variant::Bool(true));
                            Ok(Variant::Nil)
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    "stop" => {
                        if let Some(ref mut access) = self.scene_access {
                            if access.animation_player_stop(id) {
                                return Ok(Variant::Nil);
                            }
                            access.set_node_property(id, "playing", Variant::Bool(false));
                            Ok(Variant::Nil)
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    // AnimationPlayer-only playback verbs (pat-0q2kx.52).
                    //
                    // Each arm is guarded on the node actually having an attached
                    // player (`animation_player_is_playing` answers `None` when it
                    // does not), so a script's own `pause()`/`queue()` method on
                    // some other node is NOT shadowed by these builtins.
                    "pause"
                        if self
                            .scene_access
                            .as_ref()
                            .and_then(|a| a.animation_player_is_playing(id))
                            .is_some() =>
                    {
                        if let Some(ref mut access) = self.scene_access {
                            access.animation_player_pause(id);
                        }
                        Ok(Variant::Nil)
                    }
                    "queue"
                        if self
                            .scene_access
                            .as_ref()
                            .and_then(|a| a.animation_player_is_playing(id))
                            .is_some() =>
                    {
                        let clip = match args.first() {
                            Some(Variant::String(s)) => s.clone(),
                            Some(Variant::StringName(s)) => s.as_str().to_string(),
                            _ => String::new(),
                        };
                        if let Some(ref mut access) = self.scene_access {
                            access.animation_player_queue(id, &clip);
                        }
                        Ok(Variant::Nil)
                    }
                    "play_backwards"
                        if self
                            .scene_access
                            .as_ref()
                            .and_then(|a| a.animation_player_is_playing(id))
                            .is_some() =>
                    {
                        let clip = match args.first() {
                            Some(Variant::String(s)) => s.clone(),
                            Some(Variant::StringName(s)) => s.as_str().to_string(),
                            _ => String::new(),
                        };
                        if let Some(ref mut access) = self.scene_access {
                            access.animation_player_play_backwards(id, &clip);
                        }
                        Ok(Variant::Nil)
                    }
                    "get_playing_speed"
                        if self
                            .scene_access
                            .as_ref()
                            .and_then(|a| a.animation_player_is_playing(id))
                            .is_some() =>
                    {
                        let speed = self
                            .scene_access
                            .as_ref()
                            .and_then(|a| a.animation_player_playing_speed(id))
                            .unwrap_or(0.0);
                        Ok(Variant::Float(speed))
                    }
                    "is_playing" => {
                        if let Some(ref access) = self.scene_access {
                            if let Some(playing) = access.animation_player_is_playing(id) {
                                return Ok(Variant::Bool(playing));
                            }
                            Ok(Variant::Bool(matches!(
                                access.get_node_property(id, "playing"),
                                Variant::Bool(true)
                            )))
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    "seek" => {
                        if let Some(ref mut access) = self.scene_access {
                            let pos = match args.first() {
                                Some(Variant::Float(f)) => *f,
                                Some(Variant::Int(i)) => *i as f64,
                                _ => 0.0,
                            };
                            if access.animation_player_seek(id, pos) {
                                return Ok(Variant::Nil);
                            }
                            access.set_node_property(id, "seek_request", Variant::Float(pos));
                            Ok(Variant::Nil)
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    // `AnimationPlayer.get_current_animation()` — the playing
                    // clip, or "" when stopped (pat-0q2kx.1). Answered from the
                    // live player so a script reading it in the same frame as
                    // its own `play()` sees the new clip, not last frame's.
                    "get_current_animation" => {
                        if let Some(ref access) = self.scene_access {
                            if let Some(name) = access.animation_player_current(id) {
                                return Ok(Variant::String(name));
                            }
                            Ok(access.get_node_property(id, "current_animation"))
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    "get_playback_position" => {
                        if let Some(ref access) = self.scene_access {
                            if let Some(pos) = access.animation_player_position(id) {
                                return Ok(Variant::Float(pos));
                            }
                            Ok(match access.get_node_property(id, "playback_position") {
                                v @ Variant::Float(_) => v,
                                Variant::Int(i) => Variant::Float(i as f64),
                                _ => Variant::Float(0.0),
                            })
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    "set_stream_paused" => {
                        if let Some(ref mut access) = self.scene_access {
                            let paused = matches!(args.first(), Some(Variant::Bool(true)));
                            access.set_node_property(id, "stream_paused", Variant::Bool(paused));
                            Ok(Variant::Nil)
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    // -- RayCast3D / ShapeCast3D / SpringArm3D query results
                    // (pat-nzgdu.12). The MainLoop's per-frame 3D query phase
                    // publishes these `_hit_*` node properties; these methods read
                    // them so `$RayCast3D.is_colliding()` works from GDScript.
                    "is_colliding" => {
                        if let Some(ref access) = self.scene_access {
                            Ok(Variant::Bool(matches!(
                                access.get_node_property(id, "_hit_colliding"),
                                Variant::Bool(true)
                            )))
                        } else {
                            Ok(Variant::Bool(false))
                        }
                    }
                    "get_collider" => {
                        if let Some(ref access) = self.scene_access {
                            Ok(access.get_node_property(id, "_hit_collider"))
                        } else {
                            Ok(Variant::Nil)
                        }
                    }
                    "get_collision_point" => {
                        if let Some(ref access) = self.scene_access {
                            Ok(access.get_node_property(id, "_hit_point"))
                        } else {
                            Ok(Variant::Nil)
                        }
                    }
                    "get_collision_normal" => {
                        if let Some(ref access) = self.scene_access {
                            Ok(access.get_node_property(id, "_hit_normal"))
                        } else {
                            Ok(Variant::Nil)
                        }
                    }
                    "get_hit_length" => {
                        if let Some(ref access) = self.scene_access {
                            Ok(access.get_node_property(id, "_hit_length"))
                        } else {
                            Ok(Variant::Nil)
                        }
                    }
                    // The MainLoop already re-runs these queries every physics
                    // frame, so a scripted force update is a no-op.
                    "force_raycast_update" | "force_shapecast_update" => Ok(Variant::Nil),
                    // -- RigidBody2D force/impulse/torque script API (pat-nzgdu.14).
                    // Rigid bodies have no direct handle to the PhysicsServer, so
                    // these methods accumulate onto `_pending_*` / `constant_*`
                    // node properties that `PhysicsServer::sync_to_physics` drains
                    // into the world body each physics tick. A positioned
                    // impulse/force also contributes a torque = cross(offset, vec).
                    "apply_central_impulse" => {
                        if let Some(ref mut access) = self.scene_access {
                            accumulate_vec2_property(
                                &mut **access,
                                id,
                                "_pending_impulse",
                                vec2_arg(args, 0),
                            );
                        }
                        Ok(Variant::Nil)
                    }
                    "apply_impulse" => {
                        if let Some(ref mut access) = self.scene_access {
                            let imp = vec2_arg(args, 0);
                            let pos = vec2_arg(args, 1);
                            accumulate_vec2_property(&mut **access, id, "_pending_impulse", imp);
                            accumulate_float_property(
                                &mut **access,
                                id,
                                "_pending_angular_impulse",
                                (pos.x * imp.y - pos.y * imp.x) as f64,
                            );
                        }
                        Ok(Variant::Nil)
                    }
                    "apply_central_force" => {
                        if let Some(ref mut access) = self.scene_access {
                            accumulate_vec2_property(
                                &mut **access,
                                id,
                                "_pending_force",
                                vec2_arg(args, 0),
                            );
                        }
                        Ok(Variant::Nil)
                    }
                    "apply_force" => {
                        if let Some(ref mut access) = self.scene_access {
                            let f = vec2_arg(args, 0);
                            let pos = vec2_arg(args, 1);
                            accumulate_vec2_property(&mut **access, id, "_pending_force", f);
                            accumulate_float_property(
                                &mut **access,
                                id,
                                "_pending_torque",
                                (pos.x * f.y - pos.y * f.x) as f64,
                            );
                        }
                        Ok(Variant::Nil)
                    }
                    "apply_torque" => {
                        if let Some(ref mut access) = self.scene_access {
                            accumulate_float_property(
                                &mut **access,
                                id,
                                "_pending_torque",
                                float_arg(args, 0),
                            );
                        }
                        Ok(Variant::Nil)
                    }
                    "apply_torque_impulse" => {
                        if let Some(ref mut access) = self.scene_access {
                            accumulate_float_property(
                                &mut **access,
                                id,
                                "_pending_angular_impulse",
                                float_arg(args, 0),
                            );
                        }
                        Ok(Variant::Nil)
                    }
                    "add_constant_force" | "add_constant_central_force" => {
                        if let Some(ref mut access) = self.scene_access {
                            let f = vec2_arg(args, 0);
                            accumulate_vec2_property(&mut **access, id, "constant_force", f);
                            if method == "add_constant_force" {
                                let pos = vec2_arg(args, 1);
                                accumulate_float_property(
                                    &mut **access,
                                    id,
                                    "constant_torque",
                                    (pos.x * f.y - pos.y * f.x) as f64,
                                );
                            }
                        }
                        Ok(Variant::Nil)
                    }
                    "add_constant_torque" => {
                        if let Some(ref mut access) = self.scene_access {
                            accumulate_float_property(
                                &mut **access,
                                id,
                                "constant_torque",
                                float_arg(args, 0),
                            );
                        }
                        Ok(Variant::Nil)
                    }
                    "set_axis_velocity" => {
                        if let Some(ref mut access) = self.scene_access {
                            access.set_node_property(
                                id,
                                "_pending_axis_velocity",
                                Variant::Vector2(vec2_arg(args, 0)),
                            );
                        }
                        Ok(Variant::Nil)
                    }
                    // -- Object meta + dynamic property access (pat-nzgdu.30),
                    // method form (`obj.set_meta(...)`); also the target of the
                    // `set` call queued by `set_deferred`. Mirrors the bare-call
                    // handlers in `try_builtin`.
                    "set_meta" => {
                        let name = match args.first() {
                            Some(Variant::String(s)) => s.clone(),
                            _ => {
                                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                    "set_meta() takes (name, value)".into(),
                                )))
                            }
                        };
                        let value = args.get(1).cloned().unwrap_or(Variant::Nil);
                        if let Some(ref mut access) = self.scene_access {
                            access.set_meta(id, &name, value);
                            Ok(Variant::Nil)
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    "get_meta" => {
                        let name = match args.first() {
                            Some(Variant::String(s)) => s.clone(),
                            _ => {
                                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                    "get_meta() takes (name, [default])".into(),
                                )))
                            }
                        };
                        let default = args.get(1).cloned().unwrap_or(Variant::Nil);
                        if let Some(ref access) = self.scene_access {
                            Ok(access.get_meta(id, &name, default))
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    "has_meta" => {
                        let name = match args.first() {
                            Some(Variant::String(s)) => s.clone(),
                            _ => {
                                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                    "has_meta() takes a name".into(),
                                )))
                            }
                        };
                        if let Some(ref access) = self.scene_access {
                            Ok(Variant::Bool(access.has_meta(id, &name)))
                        } else {
                            Ok(Variant::Bool(false))
                        }
                    }
                    "remove_meta" => {
                        let name = match args.first() {
                            Some(Variant::String(s)) => s.clone(),
                            _ => {
                                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                    "remove_meta() takes a name".into(),
                                )))
                            }
                        };
                        if let Some(ref mut access) = self.scene_access {
                            access.remove_meta(id, &name);
                        }
                        Ok(Variant::Nil)
                    }
                    "get_meta_list" => {
                        if let Some(ref access) = self.scene_access {
                            let names = access.get_meta_list(id);
                            Ok(Variant::Array(
                                names.into_iter().map(Variant::String).collect(),
                            ))
                        } else {
                            Ok(Variant::Array(Vec::new()))
                        }
                    }
                    "set" => {
                        let prop = match args.first() {
                            Some(Variant::String(s)) => s.clone(),
                            _ => {
                                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                    "set() takes (property, value)".into(),
                                )))
                            }
                        };
                        let value = args.get(1).cloned().unwrap_or(Variant::Nil);
                        if let Some(ref mut access) = self.scene_access {
                            access.set_node_property(id, &prop, value);
                            Ok(Variant::Nil)
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    "get" if args.len() == 1 => {
                        let prop = match &args[0] {
                            Variant::String(s) => s.clone(),
                            _ => {
                                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                    "get() takes a property name".into(),
                                )))
                            }
                        };
                        if let Some(ref access) = self.scene_access {
                            Ok(access.get_node_property(id, &prop))
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    "set_deferred" => {
                        let prop = match args.first() {
                            Some(Variant::String(s)) => s.clone(),
                            _ => {
                                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                    "set_deferred() takes (property, value)".into(),
                                )))
                            }
                        };
                        let value = args.get(1).cloned().unwrap_or(Variant::Nil);
                        if let Some(ref mut access) = self.scene_access {
                            access.call_deferred(id, "set", &[Variant::String(prop), value]);
                            Ok(Variant::Nil)
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    "to_string" => {
                        let cls = self
                            .scene_access
                            .as_ref()
                            .and_then(|a| a.get_class(id))
                            .unwrap_or_else(|| "Object".to_string());
                        Ok(Variant::String(format!("<{cls}#{id}>")))
                    }
                    // Godot `Object.call(name, ...)` / `Object.callv(name, args)`
                    // run `name` on the target object (pat-0q2kx.8). `call`
                    // spreads its trailing arguments; `callv` takes them as one
                    // array.
                    "call" | "callv" => {
                        let name = match args.first() {
                            Some(Variant::String(s)) => s.clone(),
                            Some(Variant::StringName(s)) => s.as_str().to_string(),
                            _ => {
                                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                    format!("{method}() takes a method name"),
                                )))
                            }
                        };
                        let call_args: Vec<Variant> = if method == "callv" {
                            match args.get(1) {
                                Some(Variant::Array(items)) => items.clone(),
                                None | Some(Variant::Nil) => Vec::new(),
                                _ => {
                                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                        "callv() takes (method, args_array)".into(),
                                    )))
                                }
                            }
                        } else {
                            args[1..].to_vec()
                        };
                        self.call_object_method(id, &name, &call_args)
                    }
                    // Godot `Object.has_method` reports script and native methods.
                    "has_method" => {
                        let name = match args.first() {
                            Some(Variant::String(s)) => s.clone(),
                            Some(Variant::StringName(s)) => s.as_str().to_string(),
                            _ => {
                                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                    "has_method() takes a method name".into(),
                                )))
                            }
                        };
                        let has = match &self.scene_access {
                            Some(access) => access.node_has_method(id, &name),
                            // No scene access: the only script we can see is the
                            // running one, so answer for it.
                            None => self.function_registry.get(&name).is_some(),
                        };
                        Ok(Variant::Bool(has))
                    }
                    // Godot `Object.get_method_argument_count` — -1 when unknown,
                    // as Godot returns for a method it cannot find.
                    "get_method_argument_count" => {
                        let name = match args.first() {
                            Some(Variant::String(s)) => s.clone(),
                            Some(Variant::StringName(s)) => s.as_str().to_string(),
                            _ => {
                                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                    "get_method_argument_count() takes a method name".into(),
                                )))
                            }
                        };
                        let count = self
                            .scene_access
                            .as_ref()
                            .and_then(|access| access.node_method_argument_count(id, &name))
                            .unwrap_or(-1);
                        Ok(Variant::Int(count))
                    }
                    // An unknown method may still be a script method on the target
                    // object — Godot dispatches those through `Object::call`
                    // (pat-0q2kx.8). Reports "no such method" if it is not.
                    _ => self.call_object_method(id, method, args),
                }
            }
            Variant::Callable(callable_ref) => match method {
                "call" => self.invoke_callable(callable_ref, args),
                "callv" => {
                    // callv takes a single Array argument
                    if args.len() != 1 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "callv() takes 1 Array argument".into(),
                        )));
                    }
                    let call_args = match &args[0] {
                        Variant::Array(a) => a.clone(),
                        _ => {
                            return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "callv() argument must be an Array".into(),
                            )));
                        }
                    };
                    self.invoke_callable(callable_ref, &call_args)
                }
                "is_valid" => {
                    let valid = match &**callable_ref {
                        CallableRef::Method { method, .. } => !method.is_empty(),
                        CallableRef::Lambda { .. } => true,
                        CallableRef::Bound { inner, .. } | CallableRef::Unbound { inner, .. } => {
                            match inner.inner_callable() {
                                CallableRef::Method { method, .. } => !method.is_empty(),
                                _ => true,
                            }
                        }
                    };
                    Ok(Variant::Bool(valid))
                }
                "get_method" => match &**callable_ref {
                    CallableRef::Method { method, .. } => Ok(Variant::String(method.clone())),
                    CallableRef::Lambda { .. } => Ok(Variant::String("<lambda>".into())),
                    CallableRef::Bound { inner, .. } | CallableRef::Unbound { inner, .. } => {
                        match inner.inner_callable() {
                            CallableRef::Method { method, .. } => {
                                Ok(Variant::String(method.clone()))
                            }
                            _ => Ok(Variant::String("<lambda>".into())),
                        }
                    }
                },
                // Callable.bind(...args): returns a new Callable with `args`
                // appended after any call-site arguments (Godot semantics).
                "bind" => Ok(Variant::Callable(Box::new(CallableRef::Bound {
                    inner: Box::new((**callable_ref).clone()),
                    bound_args: args.to_vec(),
                }))),
                // Godot's `bindv(array)` is `bind` with the arguments supplied as
                // one array rather than varargs (pat-0q2kx.67).
                "bindv" => {
                    let bound_args = match args.first() {
                        Some(Variant::Array(a)) => a.clone(),
                        _ => {
                            return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "bindv() takes an Array".into(),
                            )))
                        }
                    };
                    Ok(Variant::Callable(Box::new(CallableRef::Bound {
                        inner: Box::new((**callable_ref).clone()),
                        bound_args,
                    })))
                }
                // The object a standard callable targets. `target_id == 0` is the
                // "calling object" sentinel `Callable(self, "m")` produces, so it
                // resolves against the current node rather than reporting 0.
                "get_object" | "get_object_id" => {
                    let target = match callable_ref.inner_callable() {
                        CallableRef::Method { target_id, .. } => {
                            if *target_id == 0 {
                                self.current_node_id.unwrap_or(0)
                            } else {
                                *target_id
                            }
                        }
                        _ => 0,
                    };
                    if method == "get_object_id" {
                        Ok(Variant::Int(target as i64))
                    } else {
                        Ok(Variant::ObjectId(gdcore::id::ObjectId::from_raw(target)))
                    }
                }
                // Bound arguments accumulate across nested `bind` calls, so the
                // whole chain is walked rather than just the outermost layer.
                "get_bound_arguments" => Ok(Variant::Array(collect_bound_args(callable_ref))),
                "get_bound_arguments_count" => {
                    Ok(Variant::Int(collect_bound_args(callable_ref).len() as i64))
                }
                "get_unbound_arguments_count" => {
                    Ok(Variant::Int(count_unbound_args(callable_ref) as i64))
                }
                "is_null" => {
                    let null = match callable_ref.inner_callable() {
                        CallableRef::Method { target_id, method } => {
                            *target_id == 0 && method.is_empty()
                        }
                        _ => false,
                    };
                    Ok(Variant::Bool(null))
                }
                // Godot: a lambda is "custom", an object+method pair is
                // "standard". Bind/unbind wrappers report their inner kind.
                "is_custom" => Ok(Variant::Bool(matches!(
                    callable_ref.inner_callable(),
                    CallableRef::Lambda { .. }
                ))),
                "is_standard" => Ok(Variant::Bool(matches!(
                    callable_ref.inner_callable(),
                    CallableRef::Method { .. }
                ))),
                "get_argument_count" => {
                    let count = match callable_ref.inner_callable() {
                        CallableRef::Lambda { params, .. } => params.len() as i64,
                        // A method's arity lives in the ClassDB, not the
                        // callable; reporting 0 rather than guessing.
                        CallableRef::Method { .. } => 0,
                        _ => 0,
                    };
                    Ok(Variant::Int(count))
                }
                "hash" => {
                    let target = match callable_ref.inner_callable() {
                        CallableRef::Method { target_id, method } => {
                            format!("{target_id}::{method}")
                        }
                        CallableRef::Lambda { params, .. } => format!("lambda/{}", params.len()),
                        _ => String::from("callable"),
                    };
                    Ok(Variant::Int(str_hash(&target)))
                }
                // Present so a script calling them gets Godot's shape of error
                // rather than "has no method"; the multiplayer lane wires them.
                "rpc" | "rpc_id" => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                    "Callable.{method}() requires multiplayer, which is not configured"
                )))),
                // Callable.unbind(n): returns a new Callable that drops the last
                // `n` call-site arguments before delegating to this callable.
                "unbind" => {
                    if args.len() != 1 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "unbind() takes 1 argument".into(),
                        )));
                    }
                    let n = match &args[0] {
                        Variant::Int(i) if *i >= 0 => *i as usize,
                        _ => {
                            return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "unbind() argument must be a non-negative integer".into(),
                            )));
                        }
                    };
                    Ok(Variant::Callable(Box::new(CallableRef::Unbound {
                        inner: Box::new((**callable_ref).clone()),
                        unbind_count: n,
                    })))
                }
                _ => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
                    format!("Callable.{method}"),
                ))),
            },
            Variant::Resource(res_ref) => match method {
                "get_path" => Ok(Variant::String(res_ref.path.clone())),
                "get_class" => Ok(Variant::String(res_ref.class_name.clone())),
                "get" => {
                    if args.len() != 1 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "Resource.get() takes 1 argument".into(),
                        )));
                    }
                    let key = match &args[0] {
                        Variant::String(s) => s.clone(),
                        _ => {
                            return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "Resource.get() key must be a string".into(),
                            )));
                        }
                    };
                    Ok(res_ref
                        .properties
                        .get(&key)
                        .cloned()
                        .unwrap_or(Variant::Nil))
                }
                // PackedScene.instantiate() / .instance(): build the scene's
                // node subtree via the scene-access hook and return a node
                // reference (same representation `get_node` yields), so
                // `preload("x.tscn").instantiate()` works end to end.
                "instantiate" | "instance" => {
                    if res_ref.class_name != "PackedScene" {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                            "cannot instantiate a {} resource",
                            res_ref.class_name
                        ))));
                    }
                    let path = res_ref.path.clone();
                    match self.scene_access.as_mut() {
                        Some(access) => match access.instantiate_packed_scene(&path) {
                            // Missing / unloadable scene → null (Godot semantics).
                            Some(raw) => Ok(Variant::ObjectId(gdcore::id::ObjectId::from_raw(raw))),
                            None => Ok(Variant::Nil),
                        },
                        None => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "instantiate() requires scene access".into(),
                        ))),
                    }
                }
                "can_instantiate" => Ok(Variant::Bool(res_ref.class_name == "PackedScene")),
                // ShaderMaterial.set_shader_parameter(name, value): stored under
                // the `shader_parameter/<name>` property key so the value round-
                // trips through tscn/tres and is picked up by the 3D render
                // server's ShaderMaterial resolution. Mutates in place by writing
                // the updated resource back to its lvalue.
                "set_shader_parameter" => {
                    if args.len() != 2 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "set_shader_parameter() takes 2 arguments (name, value)".into(),
                        )));
                    }
                    let name = match &args[0] {
                        Variant::String(s) => s.clone(),
                        _ => {
                            return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "set_shader_parameter() name must be a string".into(),
                            )));
                        }
                    };
                    let mut new_ref = res_ref.clone();
                    new_ref
                        .properties
                        .insert(format!("shader_parameter/{name}"), args[1].clone());
                    self.write_place(object_expr, Variant::Resource(new_ref))?;
                    Ok(Variant::Nil)
                }
                "get_shader_parameter" => {
                    if args.len() != 1 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "get_shader_parameter() takes 1 argument (name)".into(),
                        )));
                    }
                    let name = match &args[0] {
                        Variant::String(s) => s.clone(),
                        _ => {
                            return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "get_shader_parameter() name must be a string".into(),
                            )));
                        }
                    };
                    Ok(res_ref
                        .properties
                        .get(&format!("shader_parameter/{name}"))
                        .cloned()
                        .unwrap_or(Variant::Nil))
                }
                // ShaderMaterial.set_shader(shader) / get_shader() — the assigned
                // Shader resource is kept under the `shader` property.
                "set_shader" => {
                    Self::require_args(method, args, 1)?;
                    let mut new_ref = res_ref.clone();
                    new_ref
                        .properties
                        .insert("shader".to_string(), args[0].clone());
                    self.write_place(object_expr, Variant::Resource(new_ref))?;
                    Ok(Variant::Nil)
                }
                "get_shader" => Ok(res_ref
                    .properties
                    .get("shader")
                    .cloned()
                    .unwrap_or(Variant::Nil)),
                // Generic Resource.set(name, value) mirror of the existing get().
                "set" => {
                    if args.len() != 2 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "Resource.set() takes 2 arguments".into(),
                        )));
                    }
                    let key = match &args[0] {
                        Variant::String(s) => s.clone(),
                        _ => {
                            return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "Resource.set() key must be a string".into(),
                            )));
                        }
                    };
                    let mut new_ref = res_ref.clone();
                    new_ref.properties.insert(key, args[1].clone());
                    self.write_place(object_expr, Variant::Resource(new_ref))?;
                    Ok(Variant::Nil)
                }
                // GDScript resource `.new()`: instantiate the script class. The
                // host (scene_access) loads the `.gd`, builds a scripted instance
                // and returns its object id, mirroring PackedScene.instantiate().
                // A missing/unloadable script → Nil (Godot's `new()` semantics).
                "new" => {
                    if res_ref.class_name != "GDScript" {
                        // Non-script resources have no `.new()`; Godot yields null.
                        return Ok(Variant::Nil);
                    }
                    let path = res_ref.path.clone();
                    match self.scene_access.as_mut() {
                        Some(access) => match access.instantiate_script(&path) {
                            Some(raw) => Ok(Variant::ObjectId(gdcore::id::ObjectId::from_raw(raw))),
                            None => Ok(Variant::Nil),
                        },
                        None => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "GDScript.new() requires scene access".into(),
                        ))),
                    }
                }
                _ => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
                    format!("Resource.{method}"),
                ))),
            },
            packed if packed.is_packed_array() => {
                self.call_packed_method(packed, method, args, object_expr)
            }
            _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                "cannot call method on {}",
                obj.variant_type()
            )))),
        }
    }

    /// Dispatches `Packed*Array` methods (`size`, `append`, `resize`, `slice`,
    /// `to_byte_array`, …). Mutating methods use the clone → mutate →
    /// `write_place` pattern so `self.data.append(x)` persists, exactly like
    /// [`array_mutate`].
    fn call_packed_method(
        &mut self,
        obj: &Variant,
        method: &str,
        args: &[Variant],
        object_expr: &Expr,
    ) -> Result<Variant, RuntimeError> {
        let len = obj.packed_len().unwrap_or(0);
        // Resolves a (possibly negative) index against `len`; `None` if OOB.
        let resolve = |i: i64| -> Option<usize> {
            let idx = if i < 0 { i + len as i64 } else { i };
            if idx < 0 || idx as usize >= len {
                None
            } else {
                Some(idx as usize)
            }
        };
        match method {
            "size" => Ok(Variant::Int(len as i64)),
            "is_empty" => Ok(Variant::Bool(len == 0)),
            "get" => {
                Self::require_args("get", args, 1)?;
                let i = to_int(&args[0])?;
                let idx = resolve(i).ok_or_else(|| {
                    RuntimeError::new(RuntimeErrorKind::IndexOutOfBounds {
                        index: i,
                        length: len,
                    })
                })?;
                Ok(obj.packed_get(idx).unwrap())
            }
            "set" => {
                Self::require_args("set", args, 2)?;
                let i = to_int(&args[0])?;
                let idx = resolve(i).ok_or_else(|| {
                    RuntimeError::new(RuntimeErrorKind::IndexOutOfBounds {
                        index: i,
                        length: len,
                    })
                })?;
                let mut updated = obj.clone();
                if !updated.packed_set(idx, &args[1]) {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                        "cannot store {} in {}",
                        args[1].variant_type(),
                        obj.variant_type()
                    ))));
                }
                self.write_place(object_expr, updated)?;
                Ok(Variant::Nil)
            }
            "append" | "push_back" => {
                Self::require_args(method, args, 1)?;
                let mut updated = obj.clone();
                if !updated.packed_push(&args[0]) {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                        "cannot append {} to {}",
                        args[0].variant_type(),
                        obj.variant_type()
                    ))));
                }
                self.write_place(object_expr, updated)?;
                Ok(Variant::Nil)
            }
            "resize" => {
                Self::require_args("resize", args, 1)?;
                let n = to_int(&args[0])?;
                if n < 0 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "resize() size must be >= 0".into(),
                    )));
                }
                let mut updated = obj.clone();
                updated.packed_resize(n as usize);
                self.write_place(object_expr, updated)?;
                Ok(Variant::Nil)
            }
            "clear" => {
                let mut updated = obj.clone();
                updated.packed_resize(0);
                self.write_place(object_expr, updated)?;
                Ok(Variant::Nil)
            }
            "slice" => {
                let begin = if args.is_empty() {
                    0
                } else {
                    to_int(&args[0])?
                };
                let end = if args.len() >= 2 {
                    to_int(&args[1])?
                } else {
                    len as i64
                };
                Ok(obj.packed_slice(begin, end).unwrap())
            }
            "to_byte_array" => obj.packed_to_byte_array().ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                    "{} has no to_byte_array()",
                    obj.variant_type()
                )))
            }),
            "duplicate" => Ok(obj.clone()),
            "has" => {
                Self::require_args("has", args, 1)?;
                let elems = obj.packed_to_array().unwrap();
                Ok(Variant::Bool(elems.iter().any(|e| e == &args[0])))
            }
            // PackedByteArray string-conversion surface (pat-nzgdu.33). These
            // are meaningful only on a byte array; other packed types report
            // "no method" via the catch-all.
            "get_string_from_utf8" => match obj {
                Variant::PackedByteArray(b) => {
                    // Godot decodes UTF-8, substituting the replacement char for
                    // invalid sequences (lossy).
                    Ok(Variant::String(String::from_utf8_lossy(b).into_owned()))
                }
                _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                    "{} has no method 'get_string_from_utf8'",
                    obj.variant_type()
                )))),
            },
            "get_string_from_ascii" => match obj {
                Variant::PackedByteArray(b) => {
                    // Each byte maps directly to a Latin-1 code point.
                    Ok(Variant::String(b.iter().map(|&c| c as char).collect()))
                }
                _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                    "{} has no method 'get_string_from_ascii'",
                    obj.variant_type()
                )))),
            },
            "hex_encode" => match obj {
                Variant::PackedByteArray(b) => {
                    // Lowercase, two hex digits per byte, no separator.
                    let mut s = String::with_capacity(b.len() * 2);
                    for &byte in b {
                        s.push_str(&format!("{byte:02x}"));
                    }
                    Ok(Variant::String(s))
                }
                _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                    "{} has no method 'hex_encode'",
                    obj.variant_type()
                )))),
            },
            // -- the shared ordering/searching/editing surface (pat-0q2kx.68) --
            //
            // Every packed array carries these in Godot. The element-type rules
            // live in `gdvariant`, so each arm is just index/argument handling.
            "sort" => {
                let mut updated = obj.clone();
                updated.packed_sort();
                self.write_place(object_expr, updated)?;
                Ok(Variant::Nil)
            }
            "bsearch" => {
                Self::require_args_range("bsearch", args, 1, 2)?;
                // Godot's second argument defaults to `true` (insert before).
                let before = args.get(1).map(|v| v.is_truthy()).unwrap_or(true);
                let idx = obj.packed_bsearch(&args[0], before).unwrap_or(0);
                Ok(Variant::Int(idx as i64))
            }
            "find" => {
                Self::require_args_range("find", args, 1, 2)?;
                let from = args.get(1).map(|v| to_int(v)).transpose()?.unwrap_or(0);
                let from = from.max(0) as usize;
                Ok(Variant::Int(obj.packed_find(&args[0], from).unwrap_or(-1)))
            }
            "rfind" => {
                Self::require_args_range("rfind", args, 1, 2)?;
                let to = match args.get(1) {
                    Some(v) => {
                        let i = to_int(v)?;
                        if i < 0 {
                            None
                        } else {
                            Some(i as usize)
                        }
                    }
                    None => None,
                };
                Ok(Variant::Int(obj.packed_rfind(&args[0], to).unwrap_or(-1)))
            }
            "count" => {
                Self::require_args("count", args, 1)?;
                Ok(Variant::Int(obj.packed_count(&args[0]).unwrap_or(0) as i64))
            }
            "reverse" => {
                let mut updated = obj.clone();
                updated.packed_reverse();
                self.write_place(object_expr, updated)?;
                Ok(Variant::Nil)
            }
            "fill" => {
                Self::require_args("fill", args, 1)?;
                let mut updated = obj.clone();
                if !updated.packed_fill(&args[0]) {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                        "cannot fill {} with {}",
                        obj.variant_type(),
                        args[0].variant_type()
                    ))));
                }
                self.write_place(object_expr, updated)?;
                Ok(Variant::Nil)
            }
            "insert" => {
                Self::require_args("insert", args, 2)?;
                let at = to_int(&args[0])?.max(0) as usize;
                let mut updated = obj.clone();
                if !updated.packed_insert(at, &args[1]) {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                        "cannot insert {} into {}",
                        args[1].variant_type(),
                        obj.variant_type()
                    ))));
                }
                self.write_place(object_expr, updated)?;
                Ok(Variant::Nil)
            }
            "remove_at" => {
                Self::require_args("remove_at", args, 1)?;
                let at = to_int(&args[0])?;
                let mut updated = obj.clone();
                if at >= 0 {
                    updated.packed_remove_at(at as usize);
                }
                self.write_place(object_expr, updated)?;
                Ok(Variant::Nil)
            }
            "append_array" => {
                Self::require_args("append_array", args, 1)?;
                let mut updated = obj.clone();
                if !updated.packed_append_array(&args[0]) {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                        "cannot append {} to {}",
                        args[0].variant_type(),
                        obj.variant_type()
                    ))));
                }
                self.write_place(object_expr, updated)?;
                Ok(Variant::Nil)
            }
            // Typed reinterpretations of the byte image.
            "to_int32_array" | "to_int64_array" | "to_float32_array" | "to_float64_array" => {
                let kind = method.trim_start_matches("to_").trim_end_matches("_array");
                obj.packed_to_typed_array(kind).ok_or_else(|| {
                    RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                        "{} has no method '{method}'",
                        obj.variant_type()
                    )))
                })
            }
            // -- PackedByteArray codecs (pat-0q2kx.68) ----------------------
            //
            // `decode_double(0)` / `encode_double(0, PI)` and friends: the
            // save-game and networking workhorses. Byte-array only, so a
            // non-byte receiver falls through to the error below.
            m if m.starts_with("decode_") && matches!(obj, Variant::PackedByteArray(_)) => {
                Self::require_args(m, args, 1)?;
                let offset = to_int(&args[0])?.max(0) as usize;
                let kind = m.trim_start_matches("decode_");
                if kind == "var" {
                    let Variant::PackedByteArray(bytes) = obj else {
                        unreachable!("guarded by the match arm")
                    };
                    // Godot returns null for a buffer that does not hold a
                    // variant, rather than raising.
                    return Ok(bytes
                        .get(offset..)
                        .and_then(|b| gdvariant::binary::decode_variant(b).ok())
                        .map(|(v, _)| v)
                        .unwrap_or(Variant::Nil));
                }
                obj.packed_byte_decode(offset, kind).ok_or_else(|| {
                    RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                        "PackedByteArray has no method '{m}' (or the read ran past the end)"
                    )))
                })
            }
            m if m.starts_with("encode_") && matches!(obj, Variant::PackedByteArray(_)) => {
                Self::require_args(m, args, 2)?;
                let offset = to_int(&args[0])?.max(0) as usize;
                let kind = m.trim_start_matches("encode_");
                let mut updated = obj.clone();
                let ok = if kind == "var" {
                    match gdvariant::binary::encode_variant(&args[1]) {
                        Ok(encoded) => {
                            let Variant::PackedByteArray(bytes) = &mut updated else {
                                unreachable!("guarded by the match arm")
                            };
                            let end = offset + encoded.len();
                            if bytes.len() < end {
                                bytes.resize(end, 0);
                            }
                            bytes[offset..end].copy_from_slice(&encoded);
                            true
                        }
                        Err(_) => false,
                    }
                } else {
                    updated.packed_byte_encode(offset, kind, &args[1])
                };
                if !ok {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                        "PackedByteArray has no method '{m}'"
                    ))));
                }
                self.write_place(object_expr, updated)?;
                Ok(Variant::Nil)
            }
            other => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                "{} has no method '{}'",
                obj.variant_type(),
                other
            )))),
        }
    }

    /// Invokes a Callable (lambda or method reference).
    fn invoke_callable(
        &mut self,
        callable: &CallableRef,
        args: &[Variant],
    ) -> Result<Variant, RuntimeError> {
        stacker::maybe_grow(STACK_RED_ZONE, STACK_GROWTH, || {
            self.invoke_callable_impl(callable, args)
        })
    }

    /// Runs `method` on the object `id` and returns its value — Godot's
    /// `Object::call` reaching a scripted object
    /// (upstream/godot/core/object/object.cpp `Object::callv`), which dispatches
    /// onto the *target's* script (pat-0q2kx.8).
    ///
    /// A call whose target is the script already running stays inside this
    /// interpreter, so recursion and local state behave as they do within one
    /// script. Anything else goes through the host onto the target's own script.
    /// A target that does not respond is `UndefinedFunction`, matching Godot's
    /// "nonexistent function" on a valid object; a method that raised yields
    /// `Nil`, as a failed call in Godot has no value.
    /// A bare `foo()` or `self.foo()` call: the running script's own function
    /// first, then the engine's native method for this node's class — Godot's
    /// `Object::callp` order (upstream/godot/core/object/object.cpp).
    ///
    /// Without the second half, a `CharacterBody2D` script calling
    /// `move_and_slide()` with no receiver died on `UndefinedFunction`, since
    /// the interpreter only knew script-defined functions here (pat-0q2kx.16).
    fn call_self_func_or_native(
        &mut self,
        name: &str,
        args: &[Variant],
    ) -> Result<Variant, RuntimeError> {
        match self.call_user_func(name, args) {
            Err(e) if matches!(e.kind, RuntimeErrorKind::UndefinedFunction(_)) => {
                if let Some(id) = self.current_node_id {
                    if let Some(ref mut access) = self.scene_access {
                        if let Some(value) = access.call_node_method(id, name, args) {
                            return Ok(value);
                        }
                    }
                }
                Err(e)
            }
            other => other,
        }
    }

    fn call_object_method(
        &mut self,
        id: u64,
        method: &str,
        args: &[Variant],
    ) -> Result<Variant, RuntimeError> {
        if self.current_node_id == Some(id) {
            return self.call_self_func_or_native(method, args);
        }
        let responds = self
            .scene_access
            .as_ref()
            .map(|access| access.node_has_method(id, method))
            .unwrap_or(false);
        if let Some(ref mut access) = self.scene_access {
            if let Some(value) = access.call_script_method(id, method, args) {
                return Ok(value);
            }
            // Godot's `Object::callp` tries the script instance, then the
            // ClassDB method bind. Native methods run here so `$Body.
            // move_and_slide()` / `other.add_to_group("enemies")` resolve
            // instead of throwing UndefinedFunction (pat-0q2kx.16). Ordered
            // before the `responds` check so a native call returns its real
            // value rather than the Nil placeholder.
            if let Some(value) = access.call_node_method(id, method, args) {
                return Ok(value);
            }
        }
        if responds {
            return Ok(Variant::Nil);
        }
        Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("ObjectId.{method}"),
        )))
    }

    fn invoke_callable_impl(
        &mut self,
        callable: &CallableRef,
        args: &[Variant],
    ) -> Result<Variant, RuntimeError> {
        match callable {
            CallableRef::Method { target_id, method } => {
                // A Callable carries its target object, so `Callable(obj, "m")`
                // must run `m` ON `obj` (core/variant/callable.cpp) — dropping
                // the target ran it on whichever script happened to be calling.
                // `target_id` 0 means "self", as does the running script's own id.
                if *target_id != 0 {
                    return self.call_object_method(*target_id, method, args);
                }
                // Try to call as a user-defined function
                self.call_user_func(method, args)
            }
            CallableRef::Lambda { params, body } => {
                // Downcast the body back to Vec<Stmt>
                let body_stmts: &Vec<Stmt> = body.downcast_ref::<Vec<Stmt>>().ok_or_else(|| {
                    RuntimeError::new(RuntimeErrorKind::TypeError("invalid lambda body".into()))
                })?;
                if self.call_depth >= MAX_RECURSION_DEPTH {
                    return Err(RuntimeError::new(RuntimeErrorKind::MaxRecursionDepth(
                        MAX_RECURSION_DEPTH,
                    )));
                }
                self.call_depth += 1;
                self.environment.push_scope();
                for (i, param) in params.iter().enumerate() {
                    let val = args.get(i).cloned().unwrap_or(Variant::Nil);
                    self.environment.define(param.clone(), val);
                }
                let mut return_val = Variant::Nil;
                for stmt in body_stmts {
                    if let Some(ControlFlow::Return(v)) = self.exec_stmt(stmt)? {
                        return_val = v.unwrap_or(Variant::Nil);
                        break;
                    }
                }
                self.environment.pop_scope();
                self.call_depth -= 1;
                Ok(return_val)
            }
            CallableRef::Bound { inner, bound_args } => {
                let resolved = callable.resolve_args(args);
                self.invoke_callable(inner, &resolved)
            }
            CallableRef::Unbound { inner, .. } => {
                let resolved = callable.resolve_args(args);
                self.invoke_callable(inner, &resolved)
            }
        }
    }

    // -----------------------------------------------------------------------
    // Class system
    // -----------------------------------------------------------------------

    /// Parses a GDScript source as a class definition.
    pub fn run_class(&mut self, source: &str) -> Result<ClassDef, RuntimeError> {
        let tokens = tokenize(source)
            .map_err(|e| RuntimeError::new(RuntimeErrorKind::LexError(e.to_string())))?;
        let mut parser = Parser::new(tokens, source);
        let stmts = parser
            .parse_script()
            .map_err(|e| RuntimeError::new(RuntimeErrorKind::ParseError(e.to_string())))?;

        let mut class_def = ClassDef {
            name: None,
            parent_class: None,
            signals: Vec::new(),
            enums: HashMap::new(),
            methods: HashMap::new(),
            instance_vars: Vec::new(),
            exports: Vec::new(),
            setters: HashMap::new(),
            getters: HashMap::new(),
            is_tool: false,
            inner_classes: HashMap::new(),
            constants: Vec::new(),
        };

        for stmt in &stmts {
            match stmt {
                Stmt::ConstDecl { name, value, .. } => {
                    class_def.constants.push((name.clone(), value.clone()));
                }
                Stmt::Extends { class_name } => {
                    class_def.parent_class = Some(class_name.clone());
                }
                Stmt::ClassNameDecl { name } => {
                    class_def.name = Some(name.clone());
                }
                Stmt::SignalDecl { name, .. } => {
                    class_def.signals.push(name.clone());
                }
                Stmt::EnumDecl { name, variants } => {
                    // Compute each variant's value: an explicit `= N` sets it,
                    // otherwise it is the previous value + 1 (starting at 0),
                    // matching Godot's enum numbering.
                    let mut pairs: Vec<(String, i64)> = Vec::with_capacity(variants.len());
                    let mut next: i64 = 0;
                    for (variant_name, explicit) in variants {
                        let val = match explicit {
                            Some(v) => *v,
                            None => next,
                        };
                        pairs.push((variant_name.clone(), val));
                        next = val.wrapping_add(1);
                    }
                    match name {
                        Some(enum_name) => {
                            let map: HashMap<String, i64> = pairs.into_iter().collect();
                            class_def.enums.insert(enum_name.clone(), map);
                        }
                        None => {
                            // Anonymous enum: its variants become script
                            // constants (Godot semantics).
                            for (variant_name, val) in pairs {
                                class_def
                                    .constants
                                    .push((variant_name, Expr::Literal(Variant::Int(val))));
                            }
                        }
                    }
                }
                Stmt::FuncDef {
                    name,
                    params,
                    body,
                    is_static,
                    annotations,
                    ..
                } => {
                    // Keep the annotations: `@rpc` on a method is what the host
                    // registers with the MultiplayerAPI (pat-0q2kx.79).
                    class_def.methods.insert(
                        name.clone(),
                        FuncDef::from_params_annotated(params, body, *is_static, annotations),
                    );
                }
                Stmt::VarDecl {
                    name,
                    type_hint,
                    value,
                    annotations,
                    setter,
                    getter,
                    // `static var` is stored like an ordinary instance var for
                    // now (it initializes and is readable/writable); full
                    // class-shared-across-instances semantics is a follow-up.
                    is_static: _,
                    setter_body,
                    getter_body,
                } => {
                    let var_decl = VarDecl {
                        name: name.clone(),
                        type_hint: type_hint.clone(),
                        default: value.clone(),
                        annotations: annotations.clone(),
                        setter: setter.clone(),
                        getter: getter.clone(),
                    };
                    if let Some(s) = setter {
                        class_def.setters.insert(name.clone(), s.clone());
                    }
                    if let Some(g) = getter {
                        class_def.getters.insert(name.clone(), g.clone());
                    }
                    // Inline `set(param): <block>` / `get: <block>` bodies
                    // (pat-u3l8c.18): synthesize a hidden method for each and
                    // register it in the setter/getter tables so the existing
                    // property-dispatch machinery invokes it. The `@`-prefixed
                    // name can never collide with a user-defined method.
                    if let Some((param, body)) = setter_body {
                        let fname = format!("@set_{}", name);
                        class_def.methods.insert(
                            fname.clone(),
                            FuncDef {
                                params: vec![param.clone()],
                                defaults: vec![None],
                                body: body.clone(),
                                is_static: false,
                                // A property accessor body is not a declared
                                // function, so it carries no annotations.
                                annotations: Vec::new(),
                            },
                        );
                        class_def.setters.insert(name.clone(), fname);
                    }
                    if let Some(body) = getter_body {
                        let fname = format!("@get_{}", name);
                        class_def.methods.insert(
                            fname.clone(),
                            FuncDef {
                                params: vec![],
                                defaults: vec![],
                                body: body.clone(),
                                is_static: false,
                                // A property accessor body is not a declared
                                // function, so it carries no annotations.
                                annotations: Vec::new(),
                            },
                        );
                        class_def.getters.insert(name.clone(), fname);
                    }
                    if let Some((hint, hint_string)) =
                        annotations.iter().find_map(export_hint_from_annotation)
                    {
                        class_def.exports.push(ExportInfo {
                            name: name.clone(),
                            type_hint: type_hint.clone(),
                            hint,
                            hint_string,
                        });
                    }
                    class_def.instance_vars.push(var_decl);
                }
                Stmt::AnnotationStmt { annotations } => {
                    if annotations.iter().any(|a| a.name == "tool") {
                        class_def.is_tool = true;
                    }
                }
                Stmt::InnerClass { name, body } => {
                    let mut inner = ClassDef {
                        name: Some(name.clone()),
                        parent_class: None,
                        signals: Vec::new(),
                        enums: HashMap::new(),
                        methods: HashMap::new(),
                        instance_vars: Vec::new(),
                        exports: Vec::new(),
                        setters: HashMap::new(),
                        getters: HashMap::new(),
                        is_tool: false,
                        inner_classes: HashMap::new(),
                        constants: Vec::new(),
                    };
                    for inner_stmt in body {
                        match inner_stmt {
                            Stmt::ConstDecl { name, value, .. } => {
                                inner.constants.push((name.clone(), value.clone()));
                            }
                            Stmt::VarDecl {
                                name,
                                type_hint,
                                value,
                                annotations,
                                setter,
                                getter,
                                ..
                            } => {
                                inner.instance_vars.push(VarDecl {
                                    name: name.clone(),
                                    type_hint: type_hint.clone(),
                                    default: value.clone(),
                                    annotations: annotations.clone(),
                                    setter: setter.clone(),
                                    getter: getter.clone(),
                                });
                            }
                            Stmt::FuncDef {
                                name,
                                params,
                                body,
                                is_static,
                                annotations,
                                ..
                            } => {
                                inner.methods.insert(
                                    name.clone(),
                                    FuncDef::from_params_annotated(
                                        params,
                                        body,
                                        *is_static,
                                        annotations,
                                    ),
                                );
                            }
                            Stmt::SignalDecl { name, .. } => {
                                inner.signals.push(name.clone());
                            }
                            _ => {}
                        }
                    }
                    class_def.inner_classes.insert(name.clone(), inner);
                }
                _ => {}
            }
        }

        if let Some(ref name) = class_def.name {
            self.class_registry.insert(name.clone(), class_def.clone());
        }

        Ok(class_def)
    }

    /// Creates a new instance of a class, initializing instance variables.
    pub fn instantiate_class(
        &mut self,
        class_def: &ClassDef,
    ) -> Result<ClassInstance, RuntimeError> {
        let mut properties = HashMap::new();
        for var in &class_def.instance_vars {
            let is_onready = var.annotations.iter().any(|a| a.name == "onready");
            let val = if is_onready {
                // @onready vars are deferred until _ready; start as Nil.
                Variant::Nil
            } else {
                match &var.default {
                    Some(expr) => self.eval_expr(expr)?,
                    None => Variant::Nil,
                }
            };
            properties.insert(var.name.clone(), val);
        }
        Ok(ClassInstance {
            class_def: class_def.clone(),
            properties,
        })
    }

    /// Resolves `@onready` variables by evaluating their default expressions.
    ///
    /// Call this after the `_ready` lifecycle callback fires to populate
    /// deferred variables that were initialized to `Nil`.
    pub fn resolve_onready_vars(
        &mut self,
        instance: &mut ClassInstance,
    ) -> Result<(), RuntimeError> {
        for var in &instance.class_def.instance_vars {
            if var.annotations.iter().any(|a| a.name == "onready") {
                if let Some(expr) = &var.default {
                    let val = self.eval_expr(expr)?;
                    instance.properties.insert(var.name.clone(), val);
                }
            }
        }
        Ok(())
    }

    /// Calls a method on a class instance.
    pub fn call_instance_method(
        &mut self,
        instance: &mut ClassInstance,
        method_name: &str,
        args: &[Variant],
    ) -> Result<Variant, RuntimeError> {
        stacker::maybe_grow(STACK_RED_ZONE, STACK_GROWTH, || {
            self.call_instance_method_impl(instance, method_name, args)
        })
    }

    /// Invokes `setter_name` on `instance` **as a property setter**, so a write to
    /// the property from inside the setter body lands on the backing store instead
    /// of re-entering the setter (pat-0q2kx.17).
    ///
    /// The assignment path recognises "already inside this setter" by comparing
    /// [`current_function`](Self::current_function) against the setter's name, but
    /// [`call_instance_method`](Self::call_instance_method) does not set
    /// `current_function` — so calling a setter through it fires the body once,
    /// then the body's own `self.prop = value` fires it a *second* time before the
    /// guard engages. This wrapper sets the marker for the duration of the call
    /// and restores it afterwards, which is what an engine-side write
    /// (`Object::set`, a `.tscn` property override) needs.
    pub fn call_property_setter(
        &mut self,
        instance: &mut ClassInstance,
        setter_name: &str,
        value: Variant,
    ) -> Result<Variant, RuntimeError> {
        let previous = self.current_function.replace(setter_name.to_string());
        let result = self.call_instance_method(instance, setter_name, &[value]);
        self.current_function = previous;
        result
    }

    fn call_instance_method_impl(
        &mut self,
        instance: &mut ClassInstance,
        method_name: &str,
        args: &[Variant],
    ) -> Result<Variant, RuntimeError> {
        let func = instance
            .class_def
            .methods
            .get(method_name)
            .cloned()
            .ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::UndefinedFunction(method_name.to_string()))
            })?;

        if args.len() < func.min_args() || args.len() > func.params.len() {
            return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                "{}() takes {} arguments, got {}",
                method_name,
                func.params.len(),
                args.len()
            ))));
        }

        if self.call_depth >= MAX_RECURSION_DEPTH {
            return Err(RuntimeError::new(RuntimeErrorKind::MaxRecursionDepth(
                MAX_RECURSION_DEPTH,
            )));
        }
        self.call_depth += 1;

        let prev_self = self.self_instance.take();
        self.self_instance = Some(instance.clone());

        for (name, func_def) in &instance.class_def.methods {
            self.function_registry
                .insert(name.clone(), func_def.clone());
        }

        self.environment.push_scope();
        for (i, param) in func.params.iter().enumerate() {
            let val = if i < args.len() {
                args[i].clone()
            } else if let Some(Some(default_expr)) = func.defaults.get(i) {
                self.eval_expr(default_expr)?
            } else {
                Variant::Nil
            };
            self.environment.define(param.clone(), val);
        }
        for (name, val) in &instance.properties {
            self.environment.define(name.clone(), val.clone());
        }
        // Seed class-level constants so methods can reference them by name.
        let class_constants = instance.class_def.constants.clone();
        for (name, expr) in &class_constants {
            let val = self.eval_expr(expr)?;
            self.environment.define_const(name.clone(), val);
        }

        // Push an inner scope for the method body so that local `var`
        // declarations correctly shadow instance properties / parameters.
        self.environment.push_scope();
        let mut return_val = Variant::Nil;
        let body = func.body.clone();
        let mut i = 0;
        while i < body.len() {
            match self.exec_stmt(&body[i])? {
                Some(ControlFlow::Return(v)) => {
                    return_val = v.unwrap_or(Variant::Nil);
                    break;
                }
                Some(ControlFlow::Suspend(target)) => {
                    let remaining = body[i + 1..].to_vec();
                    self.park_coroutine(remaining, target, method_name, true);
                    // Write back any property mutations before parking.
                    if let Some(ref updated) = self.self_instance {
                        instance.properties = updated.properties.clone();
                    }
                    self.environment.pop_scope();
                    self.environment.pop_scope();
                    self.self_instance = prev_self;
                    self.call_depth -= 1;
                    return Ok(Variant::Nil);
                }
                Some(ControlFlow::Break) | Some(ControlFlow::Continue) => {}
                None => {}
            }
            i += 1;
        }
        self.environment.pop_scope();

        if let Some(ref updated) = self.self_instance {
            instance.properties = updated.properties.clone();
        }

        self.environment.pop_scope();
        self.self_instance = prev_self;
        self.call_depth -= 1;
        Ok(return_val)
    }

    /// Calls the parent class method (for `super()` calls).
    fn call_super(&mut self, args: &[Variant]) -> Result<Variant, RuntimeError> {
        let parent_name = self
            .self_instance
            .as_ref()
            .and_then(|inst| inst.class_def.parent_class.clone())
            .ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::TypeError(
                    "super() called but no parent class".into(),
                ))
            })?;

        let parent_def = self
            .class_registry
            .get(&parent_name)
            .cloned()
            .ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::UndefinedFunction(format!(
                    "parent class '{parent_name}'"
                )))
            })?;

        if let Some(func) = parent_def.methods.get("_init") {
            if args.len() < func.min_args() || args.len() > func.params.len() {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                    "super() takes {} arguments, got {}",
                    func.params.len(),
                    args.len()
                ))));
            }
            self.environment.push_scope();
            for (i, param) in func.params.iter().enumerate() {
                let val = if i < args.len() {
                    args[i].clone()
                } else if let Some(Some(default_expr)) = func.defaults.get(i) {
                    self.eval_expr(default_expr)?
                } else {
                    Variant::Nil
                };
                self.environment.define(param.clone(), val);
            }
            let mut return_val = Variant::Nil;
            for stmt in &func.body {
                if let Some(ControlFlow::Return(v)) = self.exec_stmt(stmt)? {
                    return_val = v.unwrap_or(Variant::Nil);
                    break;
                }
            }
            self.environment.pop_scope();
            Ok(return_val)
        } else {
            Ok(Variant::Nil)
        }
    }
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Builds a `Packed*Array` from a GDScript constructor call. `empty` is the
/// target packed variant (empty). Accepts zero args (empty) or a single `Array`
/// / packed-array whose elements are coerced into the packed element type.
fn build_packed_array(empty: Variant, args: &[Variant]) -> Result<Variant, RuntimeError> {
    let mut out = empty;
    match args.len() {
        0 => Ok(out),
        1 => {
            let elems: Vec<Variant> = match &args[0] {
                Variant::Array(a) => a.clone(),
                v if v.is_packed_array() => v.packed_to_array().unwrap(),
                other => {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                        "cannot construct {} from {}",
                        out.variant_type(),
                        other.variant_type()
                    ))));
                }
            };
            for e in &elems {
                if !out.packed_push(e) {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                        "cannot add {} to {}",
                        e.variant_type(),
                        out.variant_type()
                    ))));
                }
            }
            Ok(out)
        }
        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
            "{} constructor takes 0 or 1 arguments",
            out.variant_type()
        )))),
    }
}

fn index_into(container: &Variant, idx: &Variant) -> Result<Variant, RuntimeError> {
    // Packed arrays index by int, with Python-style negative indices.
    if container.is_packed_array() {
        if let Variant::Int(i) = idx {
            let len = container.packed_len().unwrap();
            let index = if *i < 0 { *i + len as i64 } else { *i };
            if index < 0 || index as usize >= len {
                return Err(RuntimeError::new(RuntimeErrorKind::IndexOutOfBounds {
                    index: *i,
                    length: len,
                }));
            }
            return Ok(container.packed_get(index as usize).unwrap());
        }
        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
            "cannot index {} with {}",
            container.variant_type(),
            idx.variant_type()
        ))));
    }
    match (container, idx) {
        (Variant::Array(a), Variant::Int(i)) => {
            let index = if *i < 0 {
                (a.len() as i64 + i) as usize
            } else {
                *i as usize
            };
            a.get(index)
                .cloned()
                .ok_or(RuntimeError::new(RuntimeErrorKind::IndexOutOfBounds {
                    index: *i,
                    length: a.len(),
                }))
        }
        (Variant::Dictionary(d), key) => d.get(key).cloned().ok_or_else(|| {
            RuntimeError::new(RuntimeErrorKind::UndefinedVariable(key.to_string_lossy()))
        }),
        (Variant::String(s), Variant::Int(i)) => {
            let index = if *i < 0 {
                (s.len() as i64 + i) as usize
            } else {
                *i as usize
            };
            s.chars()
                .nth(index)
                .map(|c| Variant::String(c.to_string()))
                .ok_or(RuntimeError::new(RuntimeErrorKind::IndexOutOfBounds {
                    index: *i,
                    length: s.len(),
                }))
        }
        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
            "cannot index {} with {}",
            container.variant_type(),
            idx.variant_type()
        )))),
    }
}

fn set_index(container: &mut Variant, idx: &Variant, value: Variant) -> Result<(), RuntimeError> {
    // Packed arrays assign by int index, coercing `value` to the element type.
    if container.is_packed_array() {
        if let Variant::Int(i) = idx {
            let len = container.packed_len().unwrap();
            let index = if *i < 0 { *i + len as i64 } else { *i };
            if index < 0 || index as usize >= len {
                return Err(RuntimeError::new(RuntimeErrorKind::IndexOutOfBounds {
                    index: *i,
                    length: len,
                }));
            }
            return if container.packed_set(index as usize, &value) {
                Ok(())
            } else {
                Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                    "cannot store {} in {}",
                    value.variant_type(),
                    container.variant_type()
                ))))
            };
        }
        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
            "packed arrays are indexed by int".into(),
        )));
    }
    match (container, idx) {
        (Variant::Array(a), Variant::Int(i)) => {
            let index = if *i < 0 {
                (a.len() as i64 + i) as usize
            } else {
                *i as usize
            };
            if index >= a.len() {
                return Err(RuntimeError::new(RuntimeErrorKind::IndexOutOfBounds {
                    index: *i,
                    length: a.len(),
                }));
            }
            a[index] = value;
            Ok(())
        }
        (Variant::Dictionary(d), key) => {
            d.insert(key.clone(), value);
            Ok(())
        }
        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
            "invalid index assignment".into(),
        ))),
    }
}

/// Extracts a [`CallableRef`] from a Variant argument (for `map`/`filter`/
/// `reduce`/`sort_custom`/etc.).
fn extract_callable(v: &Variant) -> Result<CallableRef, RuntimeError> {
    match v {
        Variant::Callable(cb) => Ok((**cb).clone()),
        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
            "expected a Callable, got {}",
            v.variant_type()
        )))),
    }
}

/// Godot `Array.slice(begin, end, step)` semantics, including negative indices
/// and negative step. Mirrors `core/variant/array.cpp`.
fn array_slice(
    arr: &[Variant],
    begin: i64,
    end: i64,
    step: i64,
) -> Result<Vec<Variant>, RuntimeError> {
    if step == 0 {
        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
            "slice() step cannot be zero".into(),
        )));
    }
    let s = arr.len() as i64;
    if s == 0 || (begin < -s && step < 0) || (begin >= s && step > 0) {
        return Ok(Vec::new());
    }
    let mut b = begin.clamp(-s, s - 1);
    if b < 0 {
        b += s;
    }
    let mut e = end.clamp(-s - 1, s);
    if e < 0 {
        e += s;
    }
    if (step > 0 && b > e) || (step < 0 && b < e) {
        return Ok(Vec::new());
    }
    let span = e - b;
    let result_size = span / step + if span % step != 0 { 1 } else { 0 };
    let mut out = Vec::new();
    let mut src = b;
    for _ in 0..result_size.max(0) {
        if src >= 0 && (src as usize) < arr.len() {
            out.push(arr[src as usize].clone());
        }
        src += step;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// String method helpers (char-index based, Godot semantics)
// ---------------------------------------------------------------------------

/// Char-based forward substring search from char index `from`. Returns -1 when
/// not found (Godot `String.find`).
fn str_find(chars: &[char], needle: &[char], from: usize) -> i64 {
    let n = chars.len();
    let m = needle.len();
    if m == 0 {
        return from.min(n) as i64;
    }
    if m > n {
        return -1;
    }
    let mut i = from;
    while i + m <= n {
        if chars[i..i + m] == *needle {
            return i as i64;
        }
        i += 1;
    }
    -1
}

/// Char-based backward search. `from` is the highest start index to consider
/// (Godot uses the string length minus needle length when `from < 0`).
fn str_rfind(chars: &[char], needle: &[char], from: Option<usize>) -> i64 {
    let n = chars.len();
    let m = needle.len();
    if m == 0 {
        return n as i64;
    }
    if m > n {
        return -1;
    }
    let start = from.unwrap_or(n - m).min(n - m);
    let mut i = start as i64;
    while i >= 0 {
        let iu = i as usize;
        if chars[iu..iu + m] == *needle {
            return i;
        }
        i -= 1;
    }
    -1
}

/// Number of non-overlapping occurrences of `needle` in `chars[from..end]`.
fn str_count(chars: &[char], needle: &[char], from: usize, to: usize) -> i64 {
    if needle.is_empty() {
        return 0;
    }
    let end = if to == 0 || to > chars.len() {
        chars.len()
    } else {
        to
    };
    let mut cnt = 0i64;
    let mut i = from;
    while i + needle.len() <= end {
        if chars[i..i + needle.len()] == *needle {
            cnt += 1;
            i += needle.len();
        } else {
            i += 1;
        }
    }
    cnt
}

fn str_left(s: &str, mut n: i64) -> String {
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len() as i64;
    if n < 0 {
        n = len + n;
    }
    if n <= 0 {
        return String::new();
    }
    if n >= len {
        return s.to_string();
    }
    chars[..n as usize].iter().collect()
}

fn str_right(s: &str, mut n: i64) -> String {
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len() as i64;
    if n < 0 {
        n = len + n;
    }
    if n <= 0 {
        return String::new();
    }
    if n >= len {
        return s.to_string();
    }
    chars[(len - n) as usize..].iter().collect()
}

/// Parses a leading integer (skips leading whitespace, honors sign, stops at
/// the first non-digit / decimal point). Godot `String.to_int`.
fn str_to_int(s: &str) -> i64 {
    let chars: Vec<char> = s.trim_start().chars().collect();
    let mut i = 0;
    let mut sign = 1i64;
    if i < chars.len() && (chars[i] == '-' || chars[i] == '+') {
        if chars[i] == '-' {
            sign = -1;
        }
        i += 1;
    }
    let mut val: i64 = 0;
    let mut any = false;
    while i < chars.len() && chars[i].is_ascii_digit() {
        val = val
            .wrapping_mul(10)
            .wrapping_add((chars[i] as u8 - b'0') as i64);
        i += 1;
        any = true;
    }
    if any {
        sign * val
    } else {
        0
    }
}

/// Parses a leading float. Godot `String.to_float`.
fn str_to_float(s: &str) -> f64 {
    let chars: Vec<char> = s.trim_start().chars().collect();
    let mut i = 0;
    let mut seen_dot = false;
    let mut seen_e = false;
    if i < chars.len() && (chars[i] == '-' || chars[i] == '+') {
        i += 1;
    }
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() {
            i += 1;
        } else if c == '.' && !seen_dot && !seen_e {
            seen_dot = true;
            i += 1;
        } else if (c == 'e' || c == 'E') && !seen_e && i > 0 {
            seen_e = true;
            i += 1;
            if i < chars.len() && (chars[i] == '-' || chars[i] == '+') {
                i += 1;
            }
        } else {
            break;
        }
    }
    let prefix: String = chars[..i].iter().collect();
    prefix.parse::<f64>().unwrap_or(0.0)
}

fn str_is_valid_int(s: &str) -> bool {
    let mut chars = s.chars();
    let mut c = chars.next();
    if let Some(ch) = c {
        if ch == '-' || ch == '+' {
            c = chars.next();
        }
    }
    match c {
        Some(ch) if ch.is_ascii_digit() => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_digit())
}

fn str_is_valid_float(s: &str) -> bool {
    !s.is_empty() && s.trim().parse::<f64>().is_ok()
}

/// Godot bigram Sørensen–Dice similarity coefficient (char based).
fn str_similarity(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    let ca: Vec<char> = a.chars().collect();
    let cb: Vec<char> = b.chars().collect();
    if ca.len() < 2 || cb.len() < 2 {
        return 0.0;
    }
    let src = ca.len() - 1;
    let tgt = cb.len() - 1;
    let sum = (src + tgt) as f64;
    let mut inter = 0;
    for i in 0..src {
        for j in 0..tgt {
            if ca[i] == cb[j] && ca[i + 1] == cb[j + 1] {
                inter += 1;
                break;
            }
        }
    }
    (2.0 * inter as f64) / sum
}

/// djb2 string hash matching Godot's `String.hash`.
/// Every argument bound onto a callable, outermost `bind` last — Godot appends
/// each `bind` after the previous ones (pat-0q2kx.67).
fn collect_bound_args(callable: &CallableRef) -> Vec<Variant> {
    match callable {
        CallableRef::Bound { inner, bound_args } => {
            let mut acc = collect_bound_args(inner);
            acc.extend(bound_args.iter().cloned());
            acc
        }
        CallableRef::Unbound { inner, .. } => collect_bound_args(inner),
        _ => Vec::new(),
    }
}

/// Total trailing arguments dropped by nested `unbind` layers.
fn count_unbound_args(callable: &CallableRef) -> usize {
    match callable {
        CallableRef::Unbound {
            inner,
            unbind_count,
        } => unbind_count + count_unbound_args(inner),
        CallableRef::Bound { inner, .. } => count_unbound_args(inner),
        _ => 0,
    }
}

fn str_hash(s: &str) -> i64 {
    let mut h: u32 = 5381;
    for c in s.chars() {
        h = (h << 5).wrapping_add(h).wrapping_add(c as u32);
    }
    h as i64
}

/// Splits a compound word/identifier into space-separated lowercased words,
/// matching Godot's `_separate_compound_words`.
fn separate_compound_words(s: &str) -> String {
    let cstr: Vec<char> = s.chars().collect();
    if cstr.is_empty() {
        return String::new();
    }
    let is_upper = |c: char| c.is_uppercase();
    let is_lower = |c: char| c.is_lowercase();
    let is_digit = |c: char| c.is_ascii_digit();
    let mut out = String::new();
    let mut start = 0;
    let mut prev_upper = is_upper(cstr[0]);
    let mut prev_lower = is_lower(cstr[0]);
    let mut prev_digit = is_digit(cstr[0]);
    for i in 1..cstr.len() {
        let curr_upper = is_upper(cstr[i]);
        let curr_lower = is_lower(cstr[i]);
        let curr_digit = is_digit(cstr[i]);
        let next_lower = i + 1 < cstr.len() && is_lower(cstr[i + 1]);
        let cond_a = prev_lower && curr_upper;
        let cond_b = (prev_upper || prev_digit) && curr_upper && next_lower;
        let cond_c = prev_digit && curr_lower && next_lower;
        let cond_d = (prev_upper || prev_lower) && curr_digit;
        if cond_a || cond_b || cond_c || cond_d {
            out.extend(cstr[start..i].iter());
            out.push(' ');
            start = i;
        }
        prev_upper = curr_upper;
        prev_lower = curr_lower;
        prev_digit = curr_digit;
    }
    out.extend(cstr[start..].iter());
    let replaced: String = out
        .chars()
        .map(|c| {
            if c.is_whitespace() || c == '_' || c == '-' {
                ' '
            } else {
                c
            }
        })
        .collect();
    replaced.to_lowercase()
}

/// Upper-cases the first char of `w`, leaving the rest unchanged.
fn title_word(w: &str) -> String {
    let mut ch = w.chars();
    match ch.next() {
        Some(f) => f.to_uppercase().collect::<String>() + ch.as_str(),
        None => String::new(),
    }
}

fn gd_capitalize(s: &str) -> String {
    separate_compound_words(s)
        .split(' ')
        .filter(|w| !w.is_empty())
        .map(title_word)
        .collect::<Vec<_>>()
        .join(" ")
}

fn gd_to_snake_case(s: &str) -> String {
    separate_compound_words(s).replace(' ', "_")
}

fn gd_to_camel_case(s: &str) -> String {
    let words = separate_compound_words(s);
    let mut ret = String::new();
    for (i, w) in words.split(' ').filter(|w| !w.is_empty()).enumerate() {
        if i == 0 {
            ret.push_str(w);
        } else {
            ret.push_str(&title_word(w));
        }
    }
    ret
}

fn gd_to_pascal_case(s: &str) -> String {
    separate_compound_words(s)
        .split(' ')
        .filter(|w| !w.is_empty())
        .map(title_word)
        .collect()
}

fn path_get_file(s: &str) -> String {
    match s.rfind(['/', '\\']) {
        Some(p) => s[p + 1..].to_string(),
        None => s.to_string(),
    }
}

fn path_get_extension(s: &str) -> String {
    let pos = match s.rfind('.') {
        Some(p) => p as isize,
        None => return String::new(),
    };
    let slash = s.rfind(['/', '\\']).map(|x| x as isize).unwrap_or(-1);
    if pos < slash || pos == 0 {
        return String::new();
    }
    s[pos as usize + 1..].to_string()
}

fn path_get_basename(s: &str) -> String {
    let pos = s.rfind('.').map(|x| x as isize).unwrap_or(-1);
    let slash = s.rfind(['/', '\\']).map(|x| x as isize).unwrap_or(-1);
    if pos > slash {
        s[..pos as usize].to_string()
    } else {
        s.to_string()
    }
}

fn path_get_base_dir(s: &str) -> String {
    match s.rfind(['/', '\\']) {
        Some(p) => s[..p].to_string(),
        None => String::new(),
    }
}

fn path_join(a: &str, b: &str) -> String {
    if a.is_empty() {
        b.to_string()
    } else if a.ends_with('/') {
        format!("{a}{b}")
    } else {
        format!("{a}/{b}")
    }
}

fn path_is_absolute(s: &str) -> bool {
    s.starts_with('/')
        || s.starts_with('\\')
        || s.contains("://")
        || (s.len() >= 2 && s.as_bytes()[1] == b':')
}

/// Simplified `String.simplify_path`: resolves `.`/`..` segments while
/// preserving a leading `/` or `xxx://` protocol prefix.
fn gd_simplify_path(s: &str) -> String {
    let s = s.replace('\\', "/");
    let mut prefix = String::new();
    let mut rest = s.clone();
    if let Some(p) = s.find("://") {
        if p > 0 && s[..p].chars().all(|c| c.is_ascii_alphanumeric()) {
            prefix = s[..p + 3].to_string();
            rest = s[p + 3..].to_string();
        }
    }
    if prefix.is_empty() && rest.starts_with('/') {
        prefix = "/".to_string();
        rest = rest[1..].to_string();
    }
    let absolute = !prefix.is_empty();
    let mut out: Vec<&str> = Vec::new();
    for part in rest.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if let Some(last) = out.last() {
                    if *last == ".." {
                        out.push("..");
                    } else {
                        out.pop();
                    }
                } else if !absolute {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    format!("{prefix}{}", out.join("/"))
}

fn gd_xml_escape(s: &str, escape_quotes: bool) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if escape_quotes => out.push_str("&quot;"),
            '\'' if escape_quotes => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn gd_xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn gd_c_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '"' => out.push_str("\\\""),
            '\'' => out.push_str("\\'"),
            '\0' => out.push_str("\\0"),
            _ => out.push(c),
        }
    }
    out
}

fn gd_c_unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some('\'') => out.push('\''),
                Some('0') => out.push('\0'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn gd_json_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            _ => out.push(c),
        }
    }
    out
}

fn gd_uri_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        let c = b as char;
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
            out.push(c);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn gd_uri_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Godot `String.dedent`: removes the common leading whitespace of all lines.
fn gd_dedent(s: &str) -> String {
    let lines: Vec<&str> = s.split('\n').collect();
    let mut common: Option<usize> = None;
    for line in &lines {
        if line.trim().is_empty() {
            continue;
        }
        let ws = line.chars().take_while(|c| *c == ' ' || *c == '\t').count();
        common = Some(common.map_or(ws, |c| c.min(ws)));
    }
    let common = common.unwrap_or(0);
    lines
        .iter()
        .map(|line| {
            if line.chars().take(common).all(|c| c == ' ' || c == '\t') && line.len() >= common {
                line.chars().skip(common).collect::<String>()
            } else {
                line.trim_start().to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Godot `String.indent`: prepends `prefix` to every non-empty line.
fn gd_indent(s: &str, prefix: &str) -> String {
    s.split('\n')
        .map(|line| {
            if line.is_empty() {
                line.to_string()
            } else {
                format!("{prefix}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Godot `String.format` supporting `{0}`/`{1}` and `{}` (Array) and `{name}`
/// (Dictionary) placeholders.
fn gd_format(template: &str, values: &Variant) -> String {
    match values {
        Variant::Array(a) => {
            let mut result = template.to_string();
            for (i, v) in a.iter().enumerate() {
                result = result.replace(&format!("{{{i}}}"), &format!("{v}"));
            }
            let mut out = String::new();
            let mut idx = 0;
            let mut chars = result.chars().peekable();
            while let Some(c) = chars.next() {
                if c == '{' && chars.peek() == Some(&'}') {
                    chars.next();
                    if idx < a.len() {
                        out.push_str(&format!("{}", a[idx]));
                        idx += 1;
                    } else {
                        out.push_str("{}");
                    }
                } else {
                    out.push(c);
                }
            }
            out
        }
        Variant::Dictionary(d) => {
            let mut result = template.to_string();
            for (k, v) in d {
                result = result.replace(&format!("{{{k}}}"), &format!("{v}"));
            }
            result
        }
        other => template.replace("{}", &format!("{other}")),
    }
}

/// Coerces a Variant method argument to a String (Godot accepts StringName /
/// String interchangeably and stringifies most values).
fn arg_str(v: &Variant) -> String {
    match v {
        Variant::String(s) => s.clone(),
        other => format!("{other}"),
    }
}

/// Natural-order comparison (numeric runs compared by value). Used by
/// `naturalnocasecmp_to`.
fn natural_cmp(a: &str, b: &str, case_insensitive: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let a = if case_insensitive {
        a.to_lowercase()
    } else {
        a.to_string()
    };
    let b = if case_insensitive {
        b.to_lowercase()
    } else {
        b.to_string()
    };
    let ac: Vec<char> = a.chars().collect();
    let bc: Vec<char> = b.chars().collect();
    let mut i = 0;
    let mut j = 0;
    while i < ac.len() && j < bc.len() {
        if ac[i].is_ascii_digit() && bc[j].is_ascii_digit() {
            let si = i;
            while i < ac.len() && ac[i].is_ascii_digit() {
                i += 1;
            }
            let sj = j;
            while j < bc.len() && bc[j].is_ascii_digit() {
                j += 1;
            }
            let na: String = ac[si..i].iter().collect();
            let nb: String = bc[sj..j].iter().collect();
            let na_t = na.trim_start_matches('0');
            let nb_t = nb.trim_start_matches('0');
            let ord = na_t.len().cmp(&nb_t.len()).then_with(|| na_t.cmp(nb_t));
            if ord != Ordering::Equal {
                return ord;
            }
        } else {
            let ord = ac[i].cmp(&bc[j]);
            if ord != Ordering::Equal {
                return ord;
            }
            i += 1;
            j += 1;
        }
    }
    ac.len().cmp(&bc.len())
}

fn ordering_to_int(o: std::cmp::Ordering) -> i64 {
    match o {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}
fn variant_cmp(a: &Variant, b: &Variant) -> std::cmp::Ordering {
    match (a, b) {
        (Variant::Int(x), Variant::Int(y)) => x.cmp(y),
        (Variant::Float(x), Variant::Float(y)) => {
            x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)
        }
        (Variant::Int(x), Variant::Float(y)) => (*x as f64)
            .partial_cmp(y)
            .unwrap_or(std::cmp::Ordering::Equal),
        (Variant::Float(x), Variant::Int(y)) => x
            .partial_cmp(&(*y as f64))
            .unwrap_or(std::cmp::Ordering::Equal),
        (Variant::String(x), Variant::String(y)) => x.cmp(y),
        _ => std::cmp::Ordering::Equal,
    }
}
fn to_float(v: &Variant) -> Result<f64, RuntimeError> {
    match v {
        Variant::Float(f) => Ok(*f),
        Variant::Int(ii) => Ok(*ii as f64),
        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
            "expected number, got {}",
            v.variant_type()
        )))),
    }
}
fn float_or_int(val: f64, a: &Variant, b: &Variant) -> Variant {
    if matches!(a, Variant::Int(_)) && matches!(b, Variant::Int(_)) && val.fract() == 0.0 {
        Variant::Int(val as i64)
    } else {
        Variant::Float(val)
    }
}
fn to_int(v: &Variant) -> Result<i64, RuntimeError> {
    match v {
        Variant::Int(i) => Ok(*i),
        Variant::Float(f) => Ok(*f as i64),
        Variant::Bool(b) => Ok(*b as i64),
        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
            "expected int, got {}",
            v.variant_type()
        )))),
    }
}
/// Godot `is_equal_approx` with a magnitude-scaled tolerance (CMP_EPSILON).
fn is_equal_approx(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    let tolerance = (1e-5 * a.abs()).max(1e-5);
    (a - b).abs() < tolerance
}
/// Godot `smoothstep`.
fn smoothstep(from: f64, to: f64, s: f64) -> f64 {
    if is_equal_approx(from, to) {
        return if s <= from { 0.0 } else { 1.0 };
    }
    let t = ((s - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
/// Godot `angle_difference` — shortest signed distance between two angles.
fn angle_difference(from: f64, to: f64) -> f64 {
    let difference = (to - from) % std::f64::consts::TAU;
    (2.0 * difference) % std::f64::consts::TAU - difference
}

/// Godot `rotate_toward`: move `from` toward `to` by at most `delta` radians,
/// taking the shortest path. A negative `delta` moves away, no further than `to`.
fn rotate_toward(from: f64, to: f64, delta: f64) -> f64 {
    let difference = angle_difference(from, to);
    let abs_difference = difference.abs();
    let sign = if difference >= 0.0 { 1.0 } else { -1.0 };
    from + delta.clamp(abs_difference - std::f64::consts::PI, abs_difference) * sign
}

/// Godot `cubic_interpolate` (Catmull-Rom style cubic through `from`/`to` with
/// `pre`/`post` tangent samples).
fn cubic_interpolate(from: f64, to: f64, pre: f64, post: f64, weight: f64) -> f64 {
    0.5 * ((from * 2.0)
        + (-pre + to) * weight
        + (2.0 * pre - 5.0 * from + 4.0 * to - post) * (weight * weight)
        + (-pre + 3.0 * from - 3.0 * to + post) * (weight * weight * weight))
}

/// Godot `cubic_interpolate_in_time` (Barry-Goldman non-uniform time form).
#[allow(clippy::too_many_arguments)]
fn cubic_interpolate_in_time(
    from: f64,
    to: f64,
    pre: f64,
    post: f64,
    weight: f64,
    to_t: f64,
    pre_t: f64,
    post_t: f64,
) -> f64 {
    let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
    let t = lerp(0.0, to_t, weight);
    let a1 = lerp(
        pre,
        from,
        if pre_t == 0.0 {
            0.0
        } else {
            (t - pre_t) / -pre_t
        },
    );
    let a2 = lerp(from, to, if to_t == 0.0 { 0.5 } else { t / to_t });
    let a3 = lerp(
        to,
        post,
        if post_t - to_t == 0.0 {
            1.0
        } else {
            (t - to_t) / (post_t - to_t)
        },
    );
    let b1 = lerp(
        a1,
        a2,
        if to_t - pre_t == 0.0 {
            0.0
        } else {
            (t - pre_t) / (to_t - pre_t)
        },
    );
    let b2 = lerp(a2, a3, if post_t == 0.0 { 1.0 } else { t / post_t });
    lerp(b1, b2, if to_t == 0.0 { 0.5 } else { t / to_t })
}

/// Wraps `pre`/`to`/`post` into the shortest-angle frame of `from` (shared by
/// the `_angle` cubic forms), returning `(from_rot, to_rot, pre_rot, post_rot)`.
fn cubic_angle_frame(from: f64, to: f64, pre: f64, post: f64) -> (f64, f64, f64, f64) {
    let tau = std::f64::consts::TAU;
    let from_rot = from % tau;
    let pre_diff = (pre - from_rot) % tau;
    let pre_rot = from_rot + (2.0 * pre_diff) % tau - pre_diff;
    let to_diff = (to - from_rot) % tau;
    let to_rot = from_rot + (2.0 * to_diff) % tau - to_diff;
    let post_diff = (post - to_rot) % tau;
    let post_rot = to_rot + (2.0 * post_diff) % tau - post_diff;
    (from_rot, to_rot, pre_rot, post_rot)
}

/// Godot `cubic_interpolate_angle`.
fn cubic_interpolate_angle(from: f64, to: f64, pre: f64, post: f64, weight: f64) -> f64 {
    let (from_rot, to_rot, pre_rot, post_rot) = cubic_angle_frame(from, to, pre, post);
    cubic_interpolate(from_rot, to_rot, pre_rot, post_rot, weight)
}

/// Godot `cubic_interpolate_angle_in_time`.
#[allow(clippy::too_many_arguments)]
fn cubic_interpolate_angle_in_time(
    from: f64,
    to: f64,
    pre: f64,
    post: f64,
    weight: f64,
    to_t: f64,
    pre_t: f64,
    post_t: f64,
) -> f64 {
    let (from_rot, to_rot, pre_rot, post_rot) = cubic_angle_frame(from, to, pre, post);
    cubic_interpolate_in_time(
        from_rot, to_rot, pre_rot, post_rot, weight, to_t, pre_t, post_t,
    )
}

/// Godot `bezier_interpolate` (cubic Bézier from `start` to `end`).
fn bezier_interpolate(start: f64, control_1: f64, control_2: f64, end: f64, t: f64) -> f64 {
    let omt = 1.0 - t;
    let omt2 = omt * omt;
    let omt3 = omt2 * omt;
    let t2 = t * t;
    let t3 = t2 * t;
    start * omt3 + control_1 * omt2 * t * 3.0 + control_2 * omt * t2 * 3.0 + end * t3
}

/// Godot `bezier_derivative` (tangent of the cubic Bézier at `t`).
fn bezier_derivative(start: f64, control_1: f64, control_2: f64, end: f64, t: f64) -> f64 {
    let omt = 1.0 - t;
    let omt2 = omt * omt;
    let t2 = t * t;
    (control_1 - start) * 3.0 * omt2
        + (control_2 - control_1) * 6.0 * omt * t
        + (end - control_2) * 3.0 * t2
}

/// Godot `fposmod`: floating modulo whose result carries the sign of `y`.
fn fposmod(x: f64, y: f64) -> f64 {
    let mut value = x % y;
    if (value < 0.0 && y > 0.0) || (value > 0.0 && y < 0.0) {
        value += y;
    }
    value + 0.0
}

/// Godot `step_decimals`: count of significant decimal places in `step`.
fn step_decimals(step: f64) -> i64 {
    const SD: [f64; 10] = [
        0.9999,
        0.09999,
        0.009999,
        0.0009999,
        0.00009999,
        0.000009999,
        0.0000009999,
        0.00000009999,
        0.000000009999,
        0.0000000009999,
    ];
    let abs = step.abs();
    let decs = abs - (abs as i64) as f64;
    for (i, sd) in SD.iter().enumerate() {
        if decs >= *sd {
            return i as i64;
        }
    }
    0
}
/// Godot `ease` curve function.
fn ease(mut x: f64, curve: f64) -> f64 {
    if x < 0.0 {
        x = 0.0;
    } else if x > 1.0 {
        x = 1.0;
    }
    if curve > 0.0 {
        if curve < 1.0 {
            1.0 - (1.0 - x).powf(1.0 / curve)
        } else {
            x.powf(curve)
        }
    } else if curve < 0.0 {
        if x < 0.5 {
            (x * 2.0).powf(-curve) * 0.5
        } else {
            (1.0 - (1.0 - (x - 0.5) * 2.0).powf(-curve)) * 0.5 + 0.5
        }
    } else {
        0.0
    }
}
/// Godot `nearest_po2` — smallest power of two >= `x` (0 maps to 0).
fn nearest_po2(x: i64) -> i64 {
    let mut n = x as u64;
    if n == 0 {
        return 0;
    }
    n -= 1;
    n |= n >> 1;
    n |= n >> 2;
    n |= n >> 4;
    n |= n >> 8;
    n |= n >> 16;
    n |= n >> 32;
    (n + 1) as i64
}

// ---------------------------------------------------------------------------
// pat-u3l8c.20 builtin helpers: BBCode stripping, Godot-text (de)serialization,
// hashing, and Variant.Type name mapping.
// ---------------------------------------------------------------------------

/// Removes BBCode tag spans (`[...]`) from `s`, keeping only the visible text.
/// A simple state machine drops every character from an opening `[` up to and
/// including the matching `]`. Used by the `print_rich` builtin.
fn strip_bbcode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '[' => in_tag = true,
            ']' if in_tag => in_tag = false,
            _ if in_tag => {}
            _ => out.push(c),
        }
    }
    out
}

/// Formats an `f64` the way Godot serializes floats: always with a decimal
/// point so an integral value like `5.0` renders as `"5.0"` (not `"5"`), which
/// keeps `str_to_var` from re-parsing it as an `int`.
fn format_gd_float(f: f64) -> String {
    let s = format!("{f}");
    if s.contains('.')
        || s.contains('e')
        || s.contains('E')
        || s.contains("inf")
        || s.contains("nan")
    {
        s
    } else {
        format!("{s}.0")
    }
}

/// Escapes `"` and `\` so a string can be embedded in `var_to_str` output.
fn gd_escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(c),
        }
    }
    out
}

/// Godot-text serialization of a [`Variant`] (`var_to_str`). Round-trips with
/// [`gd_string_to_variant`] for the supported set; any other variant falls back
/// to its `Display` form.
/// Comma-joins `Display`-able packed-array elements for constructor text.
fn join_gd<T: std::fmt::Display>(items: impl Iterator<Item = T>) -> String {
    items.map(|i| i.to_string()).collect::<Vec<_>>().join(", ")
}

fn variant_to_gd_string(v: &Variant) -> String {
    match v {
        Variant::Nil => "null".to_string(),
        Variant::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        Variant::Int(i) => i.to_string(),
        Variant::Float(f) => format_gd_float(*f),
        Variant::String(s) => format!("\"{}\"", gd_escape_string(s)),
        Variant::Vector2(p) => format!(
            "Vector2({}, {})",
            format_gd_float(p.x as f64),
            format_gd_float(p.y as f64)
        ),
        Variant::Vector3(p) => format!(
            "Vector3({}, {}, {})",
            format_gd_float(p.x as f64),
            format_gd_float(p.y as f64),
            format_gd_float(p.z as f64)
        ),
        Variant::Color(c) => format!(
            "Color({}, {}, {}, {})",
            format_gd_float(c.r as f64),
            format_gd_float(c.g as f64),
            format_gd_float(c.b as f64),
            format_gd_float(c.a as f64)
        ),
        Variant::Array(arr) => {
            let items: Vec<String> = arr.iter().map(variant_to_gd_string).collect();
            format!("[{}]", items.join(", "))
        }
        Variant::Dictionary(d) => {
            if d.is_empty() {
                "{}".to_string()
            } else {
                let items: Vec<String> = d
                    .iter()
                    .map(|(k, val)| {
                        format!("{}: {}", variant_to_gd_string(k), variant_to_gd_string(val))
                    })
                    .collect();
                format!("{{ {} }}", items.join(", "))
            }
        }
        // Constructor text for the remaining types (pat-0q2kx.69). `var_to_str`
        // is NOT `print`: it must emit something `str_to_var` can read back, so
        // these carry their type name and arguments rather than Godot's display
        // shapes (`(1, 2)`, `[X: …]`).
        Variant::Vector2i(v) => format!("Vector2i({}, {})", v.x, v.y),
        Variant::Vector3i(v) => format!("Vector3i({}, {}, {})", v.x, v.y, v.z),
        Variant::Rect2(r) => format!(
            "Rect2({}, {}, {}, {})",
            format_gd_float(r.position.x as f64),
            format_gd_float(r.position.y as f64),
            format_gd_float(r.size.x as f64),
            format_gd_float(r.size.y as f64)
        ),
        Variant::Transform2D(t) => format!(
            "Transform2D({}, {}, {}, {}, {}, {})",
            format_gd_float(t.x.x as f64),
            format_gd_float(t.x.y as f64),
            format_gd_float(t.y.x as f64),
            format_gd_float(t.y.y as f64),
            format_gd_float(t.origin.x as f64),
            format_gd_float(t.origin.y as f64)
        ),
        Variant::Basis(b) => format!(
            "Basis({}, {}, {}, {}, {}, {}, {}, {}, {})",
            format_gd_float(b.x.x as f64),
            format_gd_float(b.x.y as f64),
            format_gd_float(b.x.z as f64),
            format_gd_float(b.y.x as f64),
            format_gd_float(b.y.y as f64),
            format_gd_float(b.y.z as f64),
            format_gd_float(b.z.x as f64),
            format_gd_float(b.z.y as f64),
            format_gd_float(b.z.z as f64)
        ),
        Variant::Quaternion(q) => format!(
            "Quaternion({}, {}, {}, {})",
            format_gd_float(q.x as f64),
            format_gd_float(q.y as f64),
            format_gd_float(q.z as f64),
            format_gd_float(q.w as f64)
        ),
        Variant::Aabb(a) => format!(
            "AABB({}, {}, {}, {}, {}, {})",
            format_gd_float(a.position.x as f64),
            format_gd_float(a.position.y as f64),
            format_gd_float(a.position.z as f64),
            format_gd_float(a.size.x as f64),
            format_gd_float(a.size.y as f64),
            format_gd_float(a.size.z as f64)
        ),
        Variant::Plane(pl) => format!(
            "Plane({}, {}, {}, {})",
            format_gd_float(pl.normal.x as f64),
            format_gd_float(pl.normal.y as f64),
            format_gd_float(pl.normal.z as f64),
            format_gd_float(pl.d as f64)
        ),
        Variant::StringName(sn) => format!("&\"{}\"", gd_escape_string(sn.as_str())),
        Variant::NodePath(np) => format!("NodePath(\"{np}\")"),
        Variant::PackedInt32Array(items) => {
            format!("PackedInt32Array({})", join_gd(items.iter()))
        }
        Variant::PackedInt64Array(items) => {
            format!("PackedInt64Array({})", join_gd(items.iter()))
        }
        Variant::PackedFloat32Array(items) => format!(
            "PackedFloat32Array({})",
            items
                .iter()
                .map(|f| format_gd_float(*f as f64))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Variant::PackedFloat64Array(items) => format!(
            "PackedFloat64Array({})",
            items
                .iter()
                .map(|f| format_gd_float(*f))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Variant::PackedStringArray(items) => format!(
            "PackedStringArray({})",
            items
                .iter()
                .map(|s| format!("\"{}\"", gd_escape_string(s)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        other => format!("{other}"),
    }
}

/// A tiny recursive-descent cursor for [`gd_string_to_variant`].
struct GdTextParser {
    chars: Vec<char>,
    pos: usize,
}

impl GdTextParser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn read_ident(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == '_' {
                s.push(c);
                self.pos += 1;
            } else {
                break;
            }
        }
        s
    }

    fn parse_value(&mut self) -> Option<Variant> {
        self.skip_ws();
        let c = self.peek()?;
        match c {
            '"' => self.parse_string(),
            '[' => self.parse_array(),
            '{' => self.parse_dict(),
            '-' | '+' | '.' => self.parse_number(),
            _ if c.is_ascii_digit() => self.parse_number(),
            _ if c.is_ascii_alphabetic() || c == '_' => self.parse_ident_or_ctor(),
            _ => None,
        }
    }

    fn parse_string(&mut self) -> Option<Variant> {
        self.pos += 1; // consume opening quote
        let mut s = String::new();
        while let Some(c) = self.peek() {
            self.pos += 1;
            match c {
                '"' => return Some(Variant::String(s)),
                '\\' => {
                    let esc = self.peek()?;
                    self.pos += 1;
                    match esc {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        other => s.push(other),
                    }
                }
                _ => s.push(c),
            }
        }
        None // unterminated
    }

    fn parse_number(&mut self) -> Option<Variant> {
        let start = self.pos;
        if matches!(self.peek(), Some('-') | Some('+')) {
            self.pos += 1;
        }
        let mut is_float = false;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.pos += 1;
            } else if c == '.' || c == 'e' || c == 'E' {
                is_float = true;
                self.pos += 1;
                if (c == 'e' || c == 'E') && matches!(self.peek(), Some('-') | Some('+')) {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        if is_float {
            text.parse::<f64>().ok().map(Variant::Float)
        } else {
            text.parse::<i64>().ok().map(Variant::Int)
        }
    }

    fn parse_array(&mut self) -> Option<Variant> {
        self.pos += 1; // consume '['
        let mut arr = Vec::new();
        self.skip_ws();
        if self.peek() == Some(']') {
            self.pos += 1;
            return Some(Variant::Array(arr));
        }
        loop {
            let v = self.parse_value()?;
            arr.push(v);
            self.skip_ws();
            match self.peek()? {
                ',' => self.pos += 1,
                ']' => {
                    self.pos += 1;
                    return Some(Variant::Array(arr));
                }
                _ => return None,
            }
        }
    }

    fn parse_dict(&mut self) -> Option<Variant> {
        self.pos += 1; // consume '{'
        let mut d = Dictionary::new();
        self.skip_ws();
        if self.peek() == Some('}') {
            self.pos += 1;
            return Some(Variant::Dictionary(d));
        }
        loop {
            let key = self.parse_value()?;
            self.skip_ws();
            if self.peek()? != ':' {
                return None;
            }
            self.pos += 1; // consume ':'
            let val = self.parse_value()?;
            d.insert(key, val);
            self.skip_ws();
            match self.peek()? {
                ',' => self.pos += 1,
                '}' => {
                    self.pos += 1;
                    return Some(Variant::Dictionary(d));
                }
                _ => return None,
            }
        }
    }

    /// Reads `n` comma-separated numeric components wrapped in `( ... )`.
    fn parse_ctor_floats(&mut self, n: usize) -> Option<Vec<f32>> {
        self.skip_ws();
        if self.peek()? != '(' {
            return None;
        }
        self.pos += 1;
        let mut vals = Vec::with_capacity(n);
        for i in 0..n {
            let f = match self.parse_value()? {
                Variant::Float(f) => f as f32,
                Variant::Int(x) => x as f32,
                _ => return None,
            };
            vals.push(f);
            self.skip_ws();
            if i + 1 < n {
                if self.peek()? != ',' {
                    return None;
                }
                self.pos += 1;
            }
        }
        self.skip_ws();
        if self.peek()? != ')' {
            return None;
        }
        self.pos += 1;
        Some(vals)
    }

    fn parse_ident_or_ctor(&mut self) -> Option<Variant> {
        let ident = self.read_ident();
        match ident.as_str() {
            "null" => Some(Variant::Nil),
            "true" => Some(Variant::Bool(true)),
            "false" => Some(Variant::Bool(false)),
            "Vector2" => {
                let v = self.parse_ctor_floats(2)?;
                Some(Variant::Vector2(gdcore::math::Vector2::new(v[0], v[1])))
            }
            "Vector3" => {
                let v = self.parse_ctor_floats(3)?;
                Some(Variant::Vector3(gdcore::math::Vector3::new(
                    v[0], v[1], v[2],
                )))
            }
            "Color" => {
                let v = self.parse_ctor_floats(4)?;
                Some(Variant::Color(gdcore::math::Color::new(
                    v[0], v[1], v[2], v[3],
                )))
            }
            _ => None,
        }
    }
}

/// Parses Godot-text (`str_to_var`) back into a [`Variant`], the inverse of
/// [`variant_to_gd_string`] for the supported set (null/bool/int/float/String/
/// Vector2/Vector3/Color/Array/Dictionary). Returns [`Variant::Nil`] on any
/// parse failure.
fn gd_string_to_variant(s: &str) -> Variant {
    let mut p = GdTextParser {
        chars: s.chars().collect(),
        pos: 0,
    };
    // Fall back to `gdresource`'s `.tscn`/`.tres` literal parser, which already
    // reads every constructor form `var_to_str` emits — Quaternion, Plane, AABB,
    // Basis, Transform2D/3D, StringName, typed and packed arrays (pat-0q2kx.69).
    // Reusing it keeps the two directions in step instead of maintaining a
    // second grammar that drifts. `gdscript-interop` already depends on
    // `gdresource`, so no crate move was needed.
    match p.parse_value() {
        Some(v) => v,
        None => gdresource::loader::parse_variant_value(s.trim()).unwrap_or(Variant::Nil),
    }
}

/// Deterministic (within a run) hash of a [`Variant`] for the `hash` builtin.
/// Uses `DefaultHasher`; need not match Godot's exact hash values.
fn variant_hash(v: &Variant) -> i64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;
    let mut h = DefaultHasher::new();
    hash_variant_into(v, &mut h);
    h.finish() as i64
}

/// Feeds a [`Variant`]'s structure into `h`, recursing into arrays/dictionaries.
fn hash_variant_into<H: std::hash::Hasher>(v: &Variant, h: &mut H) {
    use std::hash::Hash;
    // Tag by discriminant so distinct types can't collide trivially.
    std::mem::discriminant(v).hash(h);
    match v {
        Variant::Nil => {}
        Variant::Bool(b) => b.hash(h),
        Variant::Int(i) => i.hash(h),
        Variant::Float(f) => f.to_bits().hash(h),
        Variant::String(s) => s.hash(h),
        Variant::StringName(s) => s.as_str().hash(h),
        Variant::Vector2(p) => {
            p.x.to_bits().hash(h);
            p.y.to_bits().hash(h);
        }
        Variant::Vector3(p) => {
            p.x.to_bits().hash(h);
            p.y.to_bits().hash(h);
            p.z.to_bits().hash(h);
        }
        Variant::Color(c) => {
            c.r.to_bits().hash(h);
            c.g.to_bits().hash(h);
            c.b.to_bits().hash(h);
            c.a.to_bits().hash(h);
        }
        Variant::Array(arr) => {
            (arr.len() as u64).hash(h);
            for e in arr {
                hash_variant_into(e, h);
            }
        }
        Variant::Dictionary(d) => {
            (d.len() as u64).hash(h);
            for (k, val) in d.iter() {
                hash_variant_into(k, h);
                hash_variant_into(val, h);
            }
        }
        other => format!("{other}").hash(h),
    }
}

/// Maps a Godot `Variant.Type` integer code to its type name, matching the
/// `type_string()` builtin. Unknown codes yield `""` (as Godot does).
fn type_code_to_name(code: i64) -> &'static str {
    match code {
        0 => "Nil",
        1 => "bool",
        2 => "int",
        3 => "float",
        4 => "String",
        5 => "Vector2",
        6 => "Vector2i",
        7 => "Rect2",
        8 => "Rect2i",
        9 => "Vector3",
        10 => "Vector3i",
        11 => "Transform2D",
        12 => "Vector4",
        13 => "Vector4i",
        14 => "Plane",
        15 => "Quaternion",
        16 => "AABB",
        17 => "Basis",
        18 => "Transform3D",
        19 => "Projection",
        20 => "Color",
        21 => "StringName",
        22 => "NodePath",
        23 => "RID",
        24 => "Object",
        25 => "Callable",
        26 => "Signal",
        27 => "Dictionary",
        28 => "Array",
        29 => "PackedByteArray",
        30 => "PackedInt32Array",
        31 => "PackedInt64Array",
        32 => "PackedFloat32Array",
        33 => "PackedFloat64Array",
        34 => "PackedStringArray",
        35 => "PackedVector2Array",
        36 => "PackedVector3Array",
        37 => "PackedColorArray",
        _ => "",
    }
}

fn deterministic_randi() -> i64 {
    use std::sync::atomic::{AtomicI64, Ordering};
    static COUNTER: AtomicI64 = AtomicI64::new(42);
    let c = COUNTER.fetch_add(1, Ordering::Relaxed);
    (c.wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407))
        & i64::MAX
}
fn string_format(fmt: &str, values: &[Variant]) -> String {
    let mut result = String::new();
    let mut chars = fmt.chars().peekable();
    let mut idx = 0;
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.peek() {
                Some('%') => {
                    chars.next();
                    result.push('%');
                }
                Some('s') => {
                    chars.next();
                    if idx < values.len() {
                        result.push_str(&format!("{}", values[idx]));
                        idx += 1;
                    }
                }
                Some('d') => {
                    chars.next();
                    if idx < values.len() {
                        match &values[idx] {
                            Variant::Int(ii) => result.push_str(&ii.to_string()),
                            Variant::Float(ff) => result.push_str(&(*ff as i64).to_string()),
                            v => result.push_str(&format!("{v}")),
                        }
                        idx += 1;
                    }
                }
                Some('f') => {
                    chars.next();
                    if idx < values.len() {
                        match &values[idx] {
                            Variant::Float(ff) => result.push_str(&format!("{:.6}", ff)),
                            Variant::Int(ii) => result.push_str(&format!("{:.6}", *ii as f64)),
                            v => result.push_str(&format!("{v}")),
                        }
                        idx += 1;
                    }
                }
                Some('v') => {
                    chars.next();
                    if idx < values.len() {
                        result.push_str(&format!("{}", values[idx]));
                        idx += 1;
                    }
                }
                _ => result.push('%'),
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Resolves static member access on built-in type names (e.g. `Vector2.ZERO`).
fn try_static_member(type_name: &str, member: &str) -> Option<Variant> {
    use gdcore::math::{Color, Vector2, Vector3};
    match type_name {
        "Vector2" => match member {
            "ZERO" => Some(Variant::Vector2(Vector2::ZERO)),
            "ONE" => Some(Variant::Vector2(Vector2::ONE)),
            "UP" => Some(Variant::Vector2(Vector2::UP)),
            "DOWN" => Some(Variant::Vector2(Vector2::DOWN)),
            "LEFT" => Some(Variant::Vector2(Vector2::LEFT)),
            "RIGHT" => Some(Variant::Vector2(Vector2::RIGHT)),
            _ => None,
        },
        "Vector3" => match member {
            "ZERO" => Some(Variant::Vector3(Vector3::ZERO)),
            "ONE" => Some(Variant::Vector3(Vector3::ONE)),
            "UP" => Some(Variant::Vector3(Vector3::UP)),
            "DOWN" => Some(Variant::Vector3(Vector3::DOWN)),
            "FORWARD" => Some(Variant::Vector3(Vector3::FORWARD)),
            _ => None,
        },
        "Color" => match member {
            "WHITE" => Some(Variant::Color(Color::WHITE)),
            "BLACK" => Some(Variant::Color(Color::BLACK)),
            "TRANSPARENT" => Some(Variant::Color(Color::TRANSPARENT)),
            "RED" => Some(Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0))),
            "GREEN" => Some(Variant::Color(Color::new(0.0, 1.0, 0.0, 1.0))),
            "BLUE" => Some(Variant::Color(Color::new(0.0, 0.0, 1.0, 1.0))),
            _ => None,
        },
        // Godot FileAccess.ModeFlags
        "FileAccess" => match member {
            "READ" => Some(Variant::Int(1)),
            "WRITE" => Some(Variant::Int(2)),
            "READ_WRITE" => Some(Variant::Int(3)),
            "WRITE_READ" => Some(Variant::Int(7)),
            _ => None,
        },
        // `Transform2D.IDENTITY`, `Basis.IDENTITY`, `Plane.PLANE_XY`, ...
        // (pat-0q2kx.50), then @GlobalScope enum-qualified access
        // (`Key.KEY_SPACE`, `Error.OK`, ...): the 20 bareword global-enum
        // prefixes delegate their member to the flat @GlobalScope constant
        // table (pat-nzgdu.7).
        _ => crate::variant_math::static_member(type_name, member)
            .or_else(|| crate::global_constants::global_enum_member(type_name, member)),
    }
}

fn engine_err_to_runtime(err: gdcore::EngineError) -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::TypeError(err.to_string()))
}

fn arg_as_string(args: &[Variant], idx: usize, ctx: &str) -> Result<String, RuntimeError> {
    match args.get(idx) {
        Some(Variant::String(s)) => Ok(s.clone()),
        Some(_) => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
            "{ctx}: argument {} must be a string",
            idx + 1
        )))),
        None => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
            "{ctx}: missing argument {}",
            idx + 1
        )))),
    }
}

fn call_file_access_static(method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    use gdcore::{FileAccess, FileMode};
    match method {
        "open" => {
            if args.len() != 2 {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "FileAccess.open() takes 2 arguments".into(),
                )));
            }
            let path = arg_as_string(args, 0, "FileAccess.open")?;
            let mode_flags = match &args[1] {
                Variant::Int(i) => *i as u32,
                Variant::Float(f) => *f as u32,
                _ => {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "FileAccess.open() mode must be an integer".into(),
                    )))
                }
            };
            let mode = FileMode::from_godot(mode_flags).ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                    "FileAccess.open() invalid mode flags {mode_flags}"
                )))
            })?;
            let id = FileAccess::open_handle(&path, mode).map_err(engine_err_to_runtime)?;
            Ok(Variant::ObjectId(id))
        }
        "file_exists" => {
            if args.len() != 1 {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "FileAccess.file_exists() takes 1 argument".into(),
                )));
            }
            let path = arg_as_string(args, 0, "FileAccess.file_exists")?;
            Ok(Variant::Bool(FileAccess::file_exists(&path)))
        }
        "get_file_as_string" => {
            if args.len() != 1 {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "FileAccess.get_file_as_string() takes 1 argument".into(),
                )));
            }
            let path = arg_as_string(args, 0, "FileAccess.get_file_as_string")?;
            let mut fa = FileAccess::open(&path, FileMode::Read).map_err(engine_err_to_runtime)?;
            let text = fa.get_as_text().map_err(engine_err_to_runtime)?;
            Ok(Variant::String(text))
        }
        other => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("FileAccess.{other}"),
        ))),
    }
}

fn call_file_access_instance(
    id: gdcore::ObjectId,
    method: &str,
    args: &[Variant],
) -> Result<Variant, RuntimeError> {
    use gdcore::FileAccess;
    match method {
        "get_as_text" => {
            if !args.is_empty() {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "get_as_text() takes 0 arguments".into(),
                )));
            }
            FileAccess::with_handle_mut(id, |fa| fa.get_as_text().map(Variant::String))
                .map_err(engine_err_to_runtime)
        }
        "get_buffer" => {
            let len = match args.first() {
                Some(Variant::Int(i)) if *i >= 0 => *i as usize,
                Some(Variant::Float(f)) if *f >= 0.0 => *f as usize,
                None => usize::MAX,
                _ => {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "get_buffer() length must be a non-negative integer".into(),
                    )))
                }
            };
            FileAccess::with_handle_mut(id, |fa| {
                fa.get_buffer(len).map(|b| {
                    Variant::Array(b.into_iter().map(|x| Variant::Int(x as i64)).collect())
                })
            })
            .map_err(engine_err_to_runtime)
        }
        "store_string" => {
            let text = arg_as_string(args, 0, "store_string")?;
            FileAccess::with_handle_mut(id, |fa| {
                fa.store_string(&text)?;
                Ok(Variant::Nil)
            })
            .map_err(engine_err_to_runtime)
        }
        "store_buffer" => {
            if args.len() != 1 {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "store_buffer() takes 1 argument".into(),
                )));
            }
            let bytes: Vec<u8> = match &args[0] {
                Variant::Array(items) => items
                    .iter()
                    .map(|v| match v {
                        Variant::Int(i) => Ok(*i as u8),
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "store_buffer() array must contain integers".into(),
                        ))),
                    })
                    .collect::<Result<_, _>>()?,
                Variant::String(s) => s.as_bytes().to_vec(),
                _ => {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "store_buffer() expects an Array or String".into(),
                    )))
                }
            };
            FileAccess::with_handle_mut(id, |fa| {
                fa.store_buffer(&bytes)?;
                Ok(Variant::Nil)
            })
            .map_err(engine_err_to_runtime)
        }
        "close" => {
            FileAccess::close_handle(id);
            Ok(Variant::Nil)
        }
        "get_error" => FileAccess::with_handle_mut(id, |fa| {
            Ok(match fa.get_error() {
                Some(e) => Variant::String(e.to_string()),
                None => Variant::Int(0), // Godot OK
            })
        })
        .map_err(engine_err_to_runtime),
        other => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("FileAccess.{other}"),
        ))),
    }
}

fn call_dir_access_static(method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    use gdcore::DirAccess;
    match method {
        "open" => {
            if args.len() != 1 {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "DirAccess.open() takes 1 argument".into(),
                )));
            }
            let path = arg_as_string(args, 0, "DirAccess.open")?;
            let id = DirAccess::open_handle(&path).map_err(engine_err_to_runtime)?;
            Ok(Variant::ObjectId(id))
        }
        "dir_exists" => {
            if args.len() != 1 {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "DirAccess.dir_exists() takes 1 argument".into(),
                )));
            }
            let path = arg_as_string(args, 0, "DirAccess.dir_exists")?;
            Ok(Variant::Bool(DirAccess::dir_exists(&path)))
        }
        other => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("DirAccess.{other}"),
        ))),
    }
}

fn call_dir_access_instance(
    id: gdcore::ObjectId,
    method: &str,
    args: &[Variant],
) -> Result<Variant, RuntimeError> {
    use gdcore::DirAccess;
    match method {
        "list_dir_begin" => DirAccess::with_handle_mut(id, |da| {
            da.list_dir_begin()?;
            Ok(Variant::Int(0))
        })
        .map_err(engine_err_to_runtime),
        "get_next" => DirAccess::with_handle_mut(id, |da| Ok(Variant::String(da.get_next())))
            .map_err(engine_err_to_runtime),
        "current_is_dir" => {
            DirAccess::with_handle_mut(id, |da| Ok(Variant::Bool(da.current_is_dir())))
                .map_err(engine_err_to_runtime)
        }
        "list_dir_end" => DirAccess::with_handle_mut(id, |da| {
            da.list_dir_end();
            Ok(Variant::Nil)
        })
        .map_err(engine_err_to_runtime),
        "make_dir" => {
            let path = arg_as_string(args, 0, "make_dir")?;
            DirAccess::with_handle_mut(id, |da| {
                da.make_dir(&path)?;
                Ok(Variant::Int(0))
            })
            .map_err(engine_err_to_runtime)
        }
        "remove" => {
            let path = arg_as_string(args, 0, "remove")?;
            DirAccess::with_handle_mut(id, |da| {
                da.remove(&path)?;
                Ok(Variant::Int(0))
            })
            .map_err(engine_err_to_runtime)
        }
        "rename" => {
            if args.len() != 2 {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "rename() takes 2 arguments".into(),
                )));
            }
            let from = arg_as_string(args, 0, "rename")?;
            let to = arg_as_string(args, 1, "rename")?;
            DirAccess::with_handle_mut(id, |da| {
                da.rename(&from, &to)?;
                Ok(Variant::Int(0))
            })
            .map_err(engine_err_to_runtime)
        }
        other => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("DirAccess.{other}"),
        ))),
    }
}

// ---------------------------------------------------------------------------
// Godot singletons: OS / Engine / Time / JSON / ConfigFile
// ---------------------------------------------------------------------------

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

static PROCESS_EPOCH: OnceLock<Instant> = OnceLock::new();
static ENGINE_EDITOR_HINT: AtomicBool = AtomicBool::new(false);
static ENGINE_FPS_MILLI: AtomicU64 = AtomicU64::new(60_000); // fps * 1000

fn process_epoch() -> &'static Instant {
    PROCESS_EPOCH.get_or_init(Instant::now)
}

fn ticks_msec() -> i64 {
    process_epoch().elapsed().as_millis() as i64
}

/// Test/runtime hook: set whether `Engine.is_editor_hint()` returns true.
pub fn set_engine_editor_hint(hint: bool) {
    ENGINE_EDITOR_HINT.store(hint, Ordering::Relaxed);
}

/// Test/runtime hook: set the FPS reported by `Engine.get_frames_per_second()`.
pub fn set_engine_frames_per_second(fps: f64) {
    let milli = (fps.max(0.0) * 1000.0) as u64;
    ENGINE_FPS_MILLI.store(milli, Ordering::Relaxed);
}

fn call_os_static(method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    match method {
        "get_name" => {
            if !args.is_empty() {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "OS.get_name() takes 0 arguments".into(),
                )));
            }
            let name = if cfg!(target_os = "macos") {
                "macOS"
            } else if cfg!(target_os = "windows") {
                "Windows"
            } else if cfg!(target_os = "linux") {
                "Linux"
            } else if cfg!(target_arch = "wasm32") {
                "Web"
            } else {
                "Unknown"
            };
            Ok(Variant::String(name.into()))
        }
        "get_ticks_msec" => {
            if !args.is_empty() {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "OS.get_ticks_msec() takes 0 arguments".into(),
                )));
            }
            Ok(Variant::Int(ticks_msec()))
        }
        "get_ticks_usec" => {
            if !args.is_empty() {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "OS.get_ticks_usec() takes 0 arguments".into(),
                )));
            }
            Ok(Variant::Int(process_epoch().elapsed().as_micros() as i64))
        }
        "is_debug_build" => {
            if !args.is_empty() {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "OS.is_debug_build() takes 0 arguments".into(),
                )));
            }
            Ok(Variant::Bool(cfg!(debug_assertions)))
        }
        // -- Expanded OS surface (pat-u3l8c.21) -----------------------------
        "get_environment" => {
            let var = match args.first() {
                Some(Variant::String(s)) => s.as_str(),
                _ => {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "OS.get_environment() takes a variable-name string".into(),
                    )))
                }
            };
            Ok(Variant::String(std::env::var(var).unwrap_or_default()))
        }
        "has_environment" => {
            let var = match args.first() {
                Some(Variant::String(s)) => s.as_str(),
                _ => {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "OS.has_environment() takes a variable-name string".into(),
                    )))
                }
            };
            Ok(Variant::Bool(std::env::var(var).is_ok()))
        }
        "get_cmdline_args" => {
            // Godot returns the args after the executable as a PackedStringArray.
            let args_vec: Vec<String> = std::env::args().skip(1).collect();
            Ok(Variant::PackedStringArray(args_vec))
        }
        "get_user_data_dir" => Ok(Variant::String(os_user_data_dir())),
        "has_feature" => {
            let feat = match args.first() {
                Some(Variant::String(s)) => s.as_str(),
                _ => {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "OS.has_feature() takes a feature-name string".into(),
                    )))
                }
            };
            let present = match feat {
                "debug" => cfg!(debug_assertions),
                "release" => !cfg!(debug_assertions),
                "editor" => false,
                "template" => true,
                "macos" => cfg!(target_os = "macos"),
                "windows" => cfg!(target_os = "windows"),
                "linux" | "linuxbsd" => cfg!(target_os = "linux"),
                "web" => cfg!(target_arch = "wasm32"),
                "x86_64" => cfg!(target_arch = "x86_64"),
                "arm64" | "aarch64" => cfg!(target_arch = "aarch64"),
                "64" => cfg!(target_pointer_width = "64"),
                "32" => cfg!(target_pointer_width = "32"),
                "pc" | "desktop" => {
                    cfg!(any(
                        target_os = "macos",
                        target_os = "windows",
                        target_os = "linux"
                    ))
                }
                "mobile" => false,
                _ => false,
            };
            Ok(Variant::Bool(present))
        }
        // -- Locale / hardware / user-args queries (pat-nzgdu.65) -----------
        "get_locale" => {
            // Godot returns e.g. "en_US". Derive from POSIX locale env vars,
            // stripping the ".ENCODING"/"@modifier" suffixes; the "C"/"POSIX"
            // (or unset) locale maps to "en".
            Ok(Variant::String(os_locale()))
        }
        "get_locale_language" => {
            // The language subtag only (e.g. "en" from "en_US").
            let loc = os_locale();
            let lang = loc.split(['_', '-']).next().unwrap_or("en");
            Ok(Variant::String(lang.to_string()))
        }
        "get_processor_count" => {
            let n = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1);
            Ok(Variant::Int(n as i64))
        }
        "get_cmdline_user_args" => {
            // Godot: the args following a bare "--" separator.
            let all: Vec<String> = std::env::args().collect();
            let user: Vec<String> = match all.iter().position(|a| a == "--") {
                Some(i) => all[i + 1..].to_vec(),
                None => Vec::new(),
            };
            Ok(Variant::PackedStringArray(user))
        }
        "get_data_dir" => Ok(Variant::String(os_user_data_dir())),
        "execute" => {
            // OS.execute(path, arguments) -> exit code (-1 if launch failed).
            let path = match args.first() {
                Some(Variant::String(s)) => s.clone(),
                _ => {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "OS.execute() first argument must be an executable path".into(),
                    )))
                }
            };
            let cmd_args: Vec<String> = match args.get(1) {
                Some(Variant::Array(a)) => a.iter().map(|v| v.to_string_lossy()).collect(),
                Some(Variant::PackedStringArray(a)) => a.clone(),
                None | Some(Variant::Nil) => Vec::new(),
                _ => {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "OS.execute() arguments must be an array of strings".into(),
                    )))
                }
            };
            match std::process::Command::new(&path).args(&cmd_args).output() {
                Ok(out) => Ok(Variant::Int(out.status.code().unwrap_or(-1) as i64)),
                Err(_) => Ok(Variant::Int(-1)),
            }
        }
        other => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("OS.{other}"),
        ))),
    }
}

/// `ClassDB` singleton — read-only reflection over the registered class table
/// (pat-u3l8c.21). Exposes the existing `gdobject::class_db` registry to script.
fn call_classdb_static(method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    fn class_name_arg(args: &[Variant], idx: usize, who: &str) -> Result<String, RuntimeError> {
        match args.get(idx) {
            Some(Variant::String(s)) => Ok(s.clone()),
            Some(Variant::StringName(sn)) => Ok(sn.as_str().to_string()),
            _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                "{who} expects a class-name string argument"
            )))),
        }
    }
    match method {
        "class_exists" => {
            let name = class_name_arg(args, 0, "ClassDB.class_exists()")?;
            Ok(Variant::Bool(gdobject::class_exists(&name)))
        }
        "can_instantiate" => {
            let name = class_name_arg(args, 0, "ClassDB.can_instantiate()")?;
            Ok(Variant::Bool(gdobject::class_db::can_instantiate(&name)))
        }
        "class_has_method" => {
            let class = class_name_arg(args, 0, "ClassDB.class_has_method()")?;
            let m = class_name_arg(args, 1, "ClassDB.class_has_method()")?;
            Ok(Variant::Bool(gdobject::class_has_method(&class, &m)))
        }
        "get_class_list" => Ok(Variant::PackedStringArray(
            gdobject::class_db::get_class_list(),
        )),
        // -- pat-nzgdu.98: parent / signal / constant / enum reflection ------
        "get_parent_class" => {
            let name = class_name_arg(args, 0, "ClassDB.get_parent_class()")?;
            // Godot yields "" for an unregistered class and for Object itself.
            Ok(Variant::String(
                gdobject::get_class_info(&name)
                    .map(|i| i.parent_class)
                    .unwrap_or_default(),
            ))
        }
        "class_has_signal" => {
            let class = class_name_arg(args, 0, "ClassDB.class_has_signal()")?;
            let signal = class_name_arg(args, 1, "ClassDB.class_has_signal()")?;
            Ok(Variant::Bool(gdobject::class_has_signal(&class, &signal)))
        }
        "class_get_signal_list" => {
            let class = class_name_arg(args, 0, "ClassDB.class_get_signal_list()")?;
            // Godot yields Array[Dictionary]; each entry carries at least the
            // signal name and its argument schema.
            let list = gdobject::get_signal_list(&class)
                .into_iter()
                .map(|s| {
                    let mut d = Dictionary::new();
                    d.insert("name", Variant::String(s.name));
                    d.insert("args", Variant::PackedStringArray(s.arguments));
                    Variant::Dictionary(d)
                })
                .collect();
            Ok(Variant::Array(list))
        }
        "class_get_integer_constant" => {
            let class = class_name_arg(args, 0, "ClassDB.class_get_integer_constant()")?;
            let name = class_name_arg(args, 1, "ClassDB.class_get_integer_constant()")?;
            // Godot returns 0 for an unknown constant rather than erroring.
            Ok(Variant::Int(
                gdobject::class_get_integer_constant(&class, &name).unwrap_or(0),
            ))
        }
        "class_get_integer_constant_list" => {
            let class = class_name_arg(args, 0, "ClassDB.class_get_integer_constant_list()")?;
            Ok(Variant::PackedStringArray(
                gdobject::get_integer_constant_list(&class),
            ))
        }
        "class_has_enum" => {
            let class = class_name_arg(args, 0, "ClassDB.class_has_enum()")?;
            let name = class_name_arg(args, 1, "ClassDB.class_has_enum()")?;
            Ok(Variant::Bool(gdobject::class_has_enum(&class, &name)))
        }
        "class_get_enum_list" => {
            let class = class_name_arg(args, 0, "ClassDB.class_get_enum_list()")?;
            Ok(Variant::PackedStringArray(gdobject::get_enum_list(&class)))
        }
        "class_get_enum_constants" => {
            let class = class_name_arg(args, 0, "ClassDB.class_get_enum_constants()")?;
            let name = class_name_arg(args, 1, "ClassDB.class_get_enum_constants()")?;
            Ok(Variant::PackedStringArray(gdobject::get_enum_constants(
                &class, &name,
            )))
        }
        other => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("ClassDB.{other}"),
        ))),
    }
}

/// Best-effort per-user writable data dir (Godot's `user://` root).
fn os_user_data_dir() -> String {
    let base = if cfg!(target_os = "macos") {
        std::env::var("HOME")
            .map(|h| format!("{h}/Library/Application Support"))
            .unwrap_or_default()
    } else if cfg!(target_os = "windows") {
        std::env::var("APPDATA").unwrap_or_default()
    } else {
        std::env::var("XDG_DATA_HOME")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|h| format!("{h}/.local/share"))
            })
            .unwrap_or_default()
    };
    if base.is_empty() {
        "user://".to_string()
    } else {
        format!("{base}/Patina")
    }
}

/// Best-effort POSIX locale (Godot's `OS.get_locale()` format, e.g. "en_US").
/// Reads `LC_ALL` / `LC_MESSAGES` / `LANG`, strips the `.ENCODING` and
/// `@modifier` suffixes, and maps the unset / "C" / "POSIX" locale to "en".
fn os_locale() -> String {
    let raw = std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_default();
    let base = raw
        .split('.')
        .next()
        .unwrap_or("")
        .split('@')
        .next()
        .unwrap_or("");
    if base.is_empty() || base == "C" || base == "POSIX" {
        "en".to_string()
    } else {
        base.replace('-', "_")
    }
}

thread_local! {
    /// Process-local `ProjectSettings` store (pat-nzgdu.68). Godot's
    /// ProjectSettings is a global singleton; a full implementation seeds this
    /// from `project.godot` at runner boot (see the still-orphaned
    /// `gdcore::project_settings`). Until then, scripts can round-trip their own
    /// `set_setting` / `get_setting` / `has_setting` values. Under `nextest`
    /// each test runs in its own process, so this store is test-isolated.
    static PROJECT_SETTINGS: std::cell::RefCell<HashMap<String, Variant>> =
        std::cell::RefCell::new(HashMap::new());
}

/// `ProjectSettings` singleton (pat-u3l8c.21, pat-nzgdu.68). `get_setting` /
/// `set_setting` / `has_setting` round-trip through a process-local store (see
/// [`PROJECT_SETTINGS`]); `globalize_path` / `localize_path` convert between the
/// `res://` / `user://` schemes and absolute filesystem paths. Seeding the store
/// from `project.godot` (so project-declared settings read back) is still TODO
/// and lives in the runner boot path.
fn call_projectsettings_static(method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    fn setting_name(args: &[Variant], who: &str) -> Result<String, RuntimeError> {
        match args.first() {
            Some(Variant::String(s)) => Ok(s.clone()),
            Some(Variant::StringName(sn)) => Ok(sn.as_str().to_string()),
            _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                "{who} expects a setting-name string"
            )))),
        }
    }
    match method {
        "get_setting" => {
            let name = setting_name(args, "ProjectSettings.get_setting()")?;
            let stored = PROJECT_SETTINGS.with(|s| s.borrow().get(&name).cloned());
            // Godot: a missing setting returns the caller's default (arg 1), Nil.
            Ok(stored.unwrap_or_else(|| args.get(1).cloned().unwrap_or(Variant::Nil)))
        }
        "get_setting_with_override" => {
            // Feature-tag overrides require the project.godot feature set (not
            // yet seeded), so fall back to the base value for now.
            let name = setting_name(args, "ProjectSettings.get_setting_with_override()")?;
            let stored = PROJECT_SETTINGS.with(|s| s.borrow().get(&name).cloned());
            Ok(stored.unwrap_or_else(|| args.get(1).cloned().unwrap_or(Variant::Nil)))
        }
        "set_setting" => {
            let name = setting_name(args, "ProjectSettings.set_setting()")?;
            let value = args.get(1).cloned().unwrap_or(Variant::Nil);
            PROJECT_SETTINGS.with(|s| {
                s.borrow_mut().insert(name, value);
            });
            Ok(Variant::Nil)
        }
        "has_setting" => {
            let name = setting_name(args, "ProjectSettings.has_setting()")?;
            Ok(Variant::Bool(
                PROJECT_SETTINGS.with(|s| s.borrow().contains_key(&name)),
            ))
        }
        "clear" => {
            let name = setting_name(args, "ProjectSettings.clear()")?;
            PROJECT_SETTINGS.with(|s| {
                s.borrow_mut().remove(&name);
            });
            Ok(Variant::Nil)
        }
        "globalize_path" => {
            let path = match args.first() {
                Some(Variant::String(s)) => s.clone(),
                _ => {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "ProjectSettings.globalize_path() takes a path string".into(),
                    )))
                }
            };
            let globalized = if let Some(rest) = path.strip_prefix("user://") {
                format!("{}/{}", os_user_data_dir().trim_end_matches('/'), rest)
            } else if let Some(rest) = path.strip_prefix("res://") {
                let base = std::env::current_dir()
                    .ok()
                    .and_then(|p| p.to_str().map(str::to_string))
                    .unwrap_or_default();
                if base.is_empty() {
                    path.clone()
                } else {
                    format!("{}/{}", base.trim_end_matches('/'), rest)
                }
            } else {
                path.clone()
            };
            Ok(Variant::String(globalized))
        }
        "localize_path" => {
            let path = match args.first() {
                Some(Variant::String(s)) => s.clone(),
                _ => {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "ProjectSettings.localize_path() takes a path string".into(),
                    )))
                }
            };
            // Inverse of globalize_path: an absolute path under the user data
            // dir maps back to `user://`, one under the project (cwd) root maps
            // to `res://`; already-localized paths pass through.
            let localized = if path.starts_with("res://") || path.starts_with("user://") {
                path.clone()
            } else {
                let user_dir = os_user_data_dir();
                let cwd = std::env::current_dir()
                    .ok()
                    .and_then(|p| p.to_str().map(str::to_string))
                    .unwrap_or_default();
                if !user_dir.is_empty() && path.starts_with(&user_dir) {
                    format!("user://{}", path[user_dir.len()..].trim_start_matches('/'))
                } else if !cwd.is_empty() && path.starts_with(&cwd) {
                    format!("res://{}", path[cwd.len()..].trim_start_matches('/'))
                } else {
                    path.clone()
                }
            };
            Ok(Variant::String(localized))
        }
        other => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("ProjectSettings.{other}"),
        ))),
    }
}

thread_local! {
    /// Process-local clipboard for the headless `DisplayServer` (pat-nzgdu.67).
    /// The wasm build routes clipboard through `navigator.clipboard`; this store
    /// gives a working round-trip in headless / tests.
    static DISPLAY_CLIPBOARD: std::cell::RefCell<String> = std::cell::RefCell::new(String::new());
    /// Records `tts_speak` utterances so headless tests can observe them
    /// (SpeechSynthesis on web).
    static DISPLAY_TTS_LOG: std::cell::RefCell<Vec<String>> = std::cell::RefCell::new(Vec::new());
}

/// `DisplayServer` singleton (pat-nzgdu.67). Headless dispatch so `DisplayServer.*`
/// script calls resolve without UndefinedFunction; the real web surface
/// (devicePixelRatio, Fullscreen API, `navigator.clipboard`, SpeechSynthesis) is
/// wired in gdplatform under wasm32. Screen metrics use headless defaults.
/// Answers a `DisplayServer.*` call from the installed backend.
///
/// Returns `None` for methods the backend seam does not cover, so the caller
/// falls through to its own headless handling — that keeps this list additive
/// and means an unrecognised method still reaches the `UndefinedFunction` arm
/// instead of silently becoming `Nil`.
fn call_displayserver_via_backend(
    backend: &dyn gdcore::display_backend::DisplayBackend,
    method: &str,
    args: &[Variant],
) -> Option<Result<Variant, RuntimeError>> {
    let size_variant =
        |(w, h): (i64, i64)| Variant::Vector2(gdcore::math::Vector2::new(w as f32, h as f32));
    match method {
        "window_get_size" | "window_get_size_with_decorations" => {
            Some(Ok(size_variant(backend.window_size())))
        }
        "screen_get_size" => Some(Ok(size_variant(backend.screen_size()))),
        "screen_get_dpi" => Some(Ok(Variant::Int(backend.screen_dpi()))),
        "screen_get_scale" | "screen_get_max_scale" => {
            Some(Ok(Variant::Float(backend.screen_scale())))
        }
        "get_screen_count" => Some(Ok(Variant::Int(backend.screen_count()))),
        "clipboard_set" => match args.first() {
            Some(Variant::String(s)) => {
                backend.clipboard_set(s);
                Some(Ok(Variant::Nil))
            }
            // Preserve the existing type error rather than swallowing it.
            _ => Some(Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                "DisplayServer.clipboard_set() takes a string".into(),
            )))),
        },
        "clipboard_get" => Some(Ok(Variant::String(backend.clipboard_get()))),
        "clipboard_has" => Some(Ok(Variant::Bool(backend.clipboard_has()))),
        "tts_speak" => {
            if let Some(Variant::String(s)) = args.first() {
                backend.tts_speak(s);
            }
            Some(Ok(Variant::Nil))
        }
        "tts_stop" => {
            backend.tts_stop();
            Some(Ok(Variant::Nil))
        }
        "tts_is_speaking" => Some(Ok(Variant::Bool(backend.tts_is_speaking()))),
        "tts_get_voices" => Some(Ok(Variant::Array(
            backend
                .tts_voices()
                .into_iter()
                .map(Variant::String)
                .collect(),
        ))),
        "tts_get_voices_for_language" => Some(Ok(Variant::PackedStringArray(backend.tts_voices()))),
        "has_feature" => {
            let feature = match args.first() {
                Some(Variant::Int(i)) => *i,
                _ => return Some(Ok(Variant::Bool(false))),
            };
            Some(Ok(Variant::Bool(backend.has_feature(feature))))
        }
        _ => None,
    }
}

fn call_displayserver_static(method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    // Route through the installed platform backend when there is one, so a
    // script sees the real window/clipboard/TTS of the process it runs in
    // rather than interpreter-local constants (pat-0q2kx.38). With no backend
    // installed every arm below falls through to the previous headless answers,
    // so nothing that worked before changes.
    if let Some(backend) = gdcore::display_backend::display_backend() {
        if let Some(result) = call_displayserver_via_backend(backend.as_ref(), method, args) {
            return result;
        }
    }
    match method {
        // -- Screen / window metrics (headless defaults) -------------------
        "screen_get_dpi" => Ok(Variant::Int(96)),
        "screen_get_scale" | "screen_get_max_scale" => Ok(Variant::Float(1.0)),
        "screen_get_size" | "window_get_size" | "window_get_size_with_decorations" => {
            Ok(Variant::Vector2(gdcore::math::Vector2::new(1920.0, 1080.0)))
        }
        "get_screen_count" => Ok(Variant::Int(1)),
        "screen_get_orientation" => Ok(Variant::Int(0)),
        "window_get_mode" => Ok(Variant::Int(0)),
        "window_set_mode" | "window_set_title" | "window_set_size" => Ok(Variant::Nil),
        // -- Clipboard (navigator.clipboard on web; store round-trip here) --
        "clipboard_set" => {
            let text = match args.first() {
                Some(Variant::String(s)) => s.clone(),
                _ => {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "DisplayServer.clipboard_set() takes a string".into(),
                    )))
                }
            };
            DISPLAY_CLIPBOARD.with(|c| *c.borrow_mut() = text);
            Ok(Variant::Nil)
        }
        "clipboard_get" => Ok(Variant::String(
            DISPLAY_CLIPBOARD.with(|c| c.borrow().clone()),
        )),
        "clipboard_has" => Ok(Variant::Bool(
            DISPLAY_CLIPBOARD.with(|c| !c.borrow().is_empty()),
        )),
        // -- Text-to-speech (SpeechSynthesis on web; recorded here) --------
        "tts_speak" => {
            let text = match args.first() {
                Some(Variant::String(s)) => s.clone(),
                _ => String::new(),
            };
            DISPLAY_TTS_LOG.with(|l| l.borrow_mut().push(text));
            Ok(Variant::Nil)
        }
        "tts_pause" | "tts_resume" | "tts_stop" => Ok(Variant::Nil),
        "tts_get_voices" => Ok(Variant::Array(Vec::new())),
        "tts_get_voices_for_language" => Ok(Variant::PackedStringArray(Vec::new())),
        "tts_is_speaking" | "tts_is_paused" => Ok(Variant::Bool(false)),
        // -- Virtual keyboard / cursor (web / mobile; headless no-ops) ------
        "virtual_keyboard_show" | "virtual_keyboard_hide" => Ok(Variant::Nil),
        "virtual_keyboard_get_height" => Ok(Variant::Int(0)),
        "cursor_set_shape" | "cursor_set_custom_image" => Ok(Variant::Nil),
        "has_feature" => Ok(Variant::Bool(false)),
        other => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("DisplayServer.{other}"),
        ))),
    }
}

thread_local! {
    /// Records `JavaScriptBridge` calls (eval code, download names) so headless
    /// tests / the runner dump can observe them; the wasm build runs the real
    /// browser calls via web-sys.
    static JS_BRIDGE_LOG: std::cell::RefCell<Vec<String>> = std::cell::RefCell::new(Vec::new());
}

/// `JavaScriptBridge` singleton (pat-nzgdu.66). The browser bridge only does real
/// work in a wasm build (web-sys `eval` / `navigator` / download); in headless /
/// native tests every call resolves without UndefinedFunction and is recorded in
/// [`JS_BRIDGE_LOG`], with `eval` returning Nil (no native JS engine).
fn call_javascriptbridge_static(method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    match method {
        "eval" => {
            if let Some(Variant::String(code)) = args.first() {
                JS_BRIDGE_LOG.with(|l| l.borrow_mut().push(format!("eval:{code}")));
            }
            // No native JS engine: Godot's non-web JavaScriptBridge.eval yields null.
            Ok(Variant::Nil)
        }
        "download_buffer" | "download" => {
            // Godot: download_buffer(buffer, name, mime). Record the filename.
            let name = match args.get(1) {
                Some(Variant::String(s)) => s.clone(),
                _ => String::new(),
            };
            JS_BRIDGE_LOG.with(|l| l.borrow_mut().push(format!("download:{name}")));
            Ok(Variant::Nil)
        }
        "get_interface" | "create_callback" | "create_object" => Ok(Variant::Nil),
        "force_fs_sync" | "pwa_update" => Ok(Variant::Nil),
        "pwa_needs_update" => Ok(Variant::Bool(false)),
        // pat-nzgdu.146: the ArrayBuffer helpers. Off the web there is no JS
        // heap, so nothing can be a JS buffer and the conversion yields an
        // empty PackedByteArray — defined values rather than an error, matching
        // how the rest of this singleton degrades natively.
        "is_js_buffer" => Ok(Variant::Bool(false)),
        "js_buffer_to_packed_byte_array" => Ok(Variant::PackedByteArray(Vec::new())),
        other => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("JavaScriptBridge.{other}"),
        ))),
    }
}

// ---------------------------------------------------------------------------
// CameraServer / CameraFeed (pat-nzgdu.179)
// ---------------------------------------------------------------------------

/// Tags an `ObjectId` issued by [`new_camera_feed_handle`]; mirrors the
/// FileAccess / DirAccess / ConfigFile handle tags (0xFA / 0xDA / 0xCF).
const CAMERA_FEED_HANDLE_TAG: u64 = 0xCA00_0000_0000_0000;

/// A script-visible `CameraFeed`. Godot's CameraFeed carries id/name/position/
/// active plus the formats a camera driver reports. Neither Patina nor Godot's
/// own web export ships a camera driver (upstream `modules/camera` builds only
/// for android/linux/macos/windows), so a feed exists only once a script adds
/// one and never carries driver-supplied image data.
#[derive(Clone, Debug)]
struct CameraFeedState {
    id: i64,
    name: String,
    position: i64,
    active: bool,
}

thread_local! {
    /// Live `CameraFeed` handles, keyed by raw ObjectId.
    static CAMERA_FEEDS: std::cell::RefCell<HashMap<u64, CameraFeedState>> =
        std::cell::RefCell::new(HashMap::new());
    /// The CameraServer feed list in add order — `get_feed(idx)` indexes this.
    static CAMERA_SERVER_FEEDS: std::cell::RefCell<Vec<u64>> =
        std::cell::RefCell::new(Vec::new());
    /// Sequence backing both the handle id and the Godot-visible feed id.
    static NEXT_CAMERA_FEED: std::cell::Cell<u64> = std::cell::Cell::new(1);
    /// `CameraServer.set_monitoring_feeds` flag.
    static CAMERA_MONITORING: std::cell::Cell<bool> = std::cell::Cell::new(false);
}

/// Returns `true` when `id` was issued by [`new_camera_feed_handle`].
fn is_camera_feed_handle(id: gdcore::id::ObjectId) -> bool {
    id.raw() & 0xFF00_0000_0000_0000 == CAMERA_FEED_HANDLE_TAG
}

// ---------------------------------------------------------------------------
// EngineDebugger (pat-nzgdu.176)
// ---------------------------------------------------------------------------
//
// Godot's EngineDebugger is the script-facing face of the remote debugger: the
// running game reports breakpoints/stack/profiler frames to the editor over a
// protocol. `gdcore::debugger::Debugger` already implements the breakpoint and
// call-stack model, and `gdcore::debug_protocol` already implements the
// transport — but nothing referenced either outside gdcore, and `EngineDebugger`
// appeared nowhere in the workspace. This arm makes the singleton
// script-reachable over the real gdcore debugger rather than a private mock.
//
// `is_active()` follows Godot: false until a debug session attaches (Godot only
// activates the debugger under `--remote-debug`), so a normal run reports false.

/// A registered `EngineProfiler`, keyed by name.
#[derive(Debug, Default, Clone)]
struct DebugProfilerState {
    /// Whether `profiler_enable` has switched this profiler on.
    enabled: bool,
    /// Frame data handed to `profiler_add_frame_data`, in arrival order.
    frames: Vec<Vec<Variant>>,
}

thread_local! {
    /// The process-local debugger backing `EngineDebugger` (pat-nzgdu.176).
    /// Under `nextest` each test runs in its own process, so this is
    /// test-isolated. A real session is opened by the runner via
    /// [`engine_debugger_attach`].
    static ENGINE_DEBUGGER: std::cell::RefCell<gdcore::debugger::Debugger> =
        std::cell::RefCell::new(gdcore::debugger::Debugger::new());
    /// Profilers registered by `register_profiler`, keyed by name.
    static DEBUG_PROFILERS: std::cell::RefCell<HashMap<String, DebugProfilerState>> =
        std::cell::RefCell::new(HashMap::new());
    /// Message captures registered by `register_message_capture`. Godot takes a
    /// Callable; we keep the Variant so a future dispatch can invoke it.
    static DEBUG_CAPTURES: std::cell::RefCell<HashMap<String, Variant>> =
        std::cell::RefCell::new(HashMap::new());
    /// Messages passed to `send_message`, in order — what the transport drains
    /// to the editor. See [`drain_engine_debugger_messages`].
    static DEBUG_MESSAGES: std::cell::RefCell<Vec<(String, Vec<Variant>)>> =
        std::cell::RefCell::new(Vec::new());
    /// `set_depth` / `get_depth` — Godot's stepping depth counter. Distinct from
    /// the *call stack* depth `Debugger::stack_depth()` reports.
    static DEBUG_DEPTH: std::cell::Cell<i64> = std::cell::Cell::new(0);
    /// `set_lines_left` / `get_lines_left` — lines remaining before the debugger
    /// breaks again while stepping.
    static DEBUG_LINES_LEFT: std::cell::Cell<i64> = std::cell::Cell::new(0);
    /// `is_skipping_breakpoints` — set when the user asks the editor to ignore
    /// breakpoints without removing them.
    static DEBUG_SKIP_BREAKPOINTS: std::cell::Cell<bool> = std::cell::Cell::new(false);
}

/// Runtime hook: attach the debugger, making `EngineDebugger.is_active()` report
/// true. The runner calls this when a debug session is established (Godot
/// activates the debugger under `--remote-debug`).
pub fn engine_debugger_attach() {
    ENGINE_DEBUGGER.with(|d| d.borrow_mut().attach());
}

/// Runtime hook: detach the debugger — `is_active()` reports false again.
pub fn engine_debugger_detach() {
    ENGINE_DEBUGGER.with(|d| d.borrow_mut().detach());
}

/// Runtime hook: run `f` against the process-local debugger, so the runner can
/// drive breakpoints/stack while scripts observe the same state.
///
/// # Panics
///
/// Panics with `BorrowMutError` if `f` re-enters the interpreter — the debugger
/// stays mutably borrowed for the whole of `f`, and any script it runs that
/// touches `EngineDebugger` would borrow it again. Decide inside `f`, act after:
///
/// ```ignore
/// let hit = with_engine_debugger(|d| d.should_break(script, line)); // borrow ends here
/// if hit { interp.run(on_break)?; }
/// ```
pub fn with_engine_debugger<R>(f: impl FnOnce(&mut gdcore::debugger::Debugger) -> R) -> R {
    ENGINE_DEBUGGER.with(|d| f(&mut d.borrow_mut()))
}

/// Runtime hook: set whether the debugger ignores breakpoints without removing
/// them. Godot exposes only the `is_skipping_breakpoints()` getter to scripts —
/// the editor sets this over the debug protocol, so this is the transport's
/// seam, mirroring [`engine_debugger_attach`].
pub fn engine_debugger_set_skipping_breakpoints(skip: bool) {
    DEBUG_SKIP_BREAKPOINTS.with(|s| s.set(skip));
}

/// Runtime hook: take everything `send_message` has queued, leaving the queue
/// empty. This is the seam the debug-protocol transport drains to the editor.
pub fn drain_engine_debugger_messages() -> Vec<(String, Vec<Variant>)> {
    DEBUG_MESSAGES.with(|m| std::mem::take(&mut *m.borrow_mut()))
}

/// Runtime hook: the frame data queued for `profiler`, in arrival order.
pub fn engine_debugger_profiler_frames(profiler: &str) -> Vec<Vec<Variant>> {
    DEBUG_PROFILERS.with(|p| {
        p.borrow()
            .get(profiler)
            .map(|s| s.frames.clone())
            .unwrap_or_default()
    })
}

/// Reads a `StringName`/`String` argument, which Godot's EngineDebugger uses for
/// profiler, capture and breakpoint-source names.
///
/// Godot types every one of these as `StringName`, so both spellings are
/// accepted — matching how the rest of this file reads name arguments. Scripts
/// can only produce `Variant::String` today (the tokenizer has no `&"..."`
/// literal), but StringNames flow in from other interpreter paths.
fn debug_name_arg(args: &[Variant], at: usize, method: &str) -> Result<String, RuntimeError> {
    match args.get(at) {
        Some(Variant::String(s)) => Ok(s.clone()),
        Some(Variant::StringName(s)) => Ok(s.as_str().to_string()),
        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
            "EngineDebugger.{method}() expects a name at argument {at}"
        )))),
    }
}

/// Reads a strict bool argument. Godot's `profiler_enable(name, enable)` takes a
/// real bool; accepting anything truthy would turn `profiler_enable("p", 1)` —
/// a realistic mistake — into a silent *disable*.
fn debug_bool_arg(args: &[Variant], at: usize, method: &str) -> Result<bool, RuntimeError> {
    match args.get(at) {
        Some(Variant::Bool(b)) => Ok(*b),
        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
            "EngineDebugger.{method}() expects a bool at argument {at}"
        )))),
    }
}

/// Reads an integer argument (line numbers, depth, lines-left).
fn debug_int_arg(args: &[Variant], at: usize, method: &str) -> Result<i64, RuntimeError> {
    match args.get(at) {
        Some(Variant::Int(i)) => Ok(*i),
        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
            "EngineDebugger.{method}() expects an integer at argument {at}"
        )))),
    }
}

/// Builds the gdcore breakpoint location for the Godot-order `(line, source)`
/// argument pair.
///
/// ⚠️ Godot spells these `insert_breakpoint(line: int, source: StringName)` —
/// **line first** — while `BreakpointLocation::new(script, line)` takes the
/// script first. Both are read out of `&[Variant]`, so swapping them is not a
/// compile error; it silently registers a breakpoint on the wrong script.
fn debug_breakpoint_arg(
    args: &[Variant],
    method: &str,
) -> Result<gdcore::debugger::BreakpointLocation, RuntimeError> {
    let line = debug_int_arg(args, 0, method)?;
    let source = debug_name_arg(args, 1, method)?;
    if line < 0 {
        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
            "EngineDebugger.{method}() line must be non-negative, got {line}"
        ))));
    }
    Ok(gdcore::debugger::BreakpointLocation::new(
        source,
        line as usize,
    ))
}

fn call_enginedebugger_static(method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    match method {
        // Godot reports false until a session attaches — a plain run is not
        // being debugged.
        "is_active" => Ok(Variant::Bool(
            ENGINE_DEBUGGER.with(|d| d.borrow().is_attached()),
        )),

        // -- breakpoints (line FIRST, source second — see debug_breakpoint_arg)
        "insert_breakpoint" => {
            let loc = debug_breakpoint_arg(args, "insert_breakpoint")?;
            ENGINE_DEBUGGER.with(|d| d.borrow_mut().add_breakpoint(loc));
            Ok(Variant::Nil)
        }
        "remove_breakpoint" => {
            let loc = debug_breakpoint_arg(args, "remove_breakpoint")?;
            ENGINE_DEBUGGER.with(|d| d.borrow_mut().remove_breakpoint(&loc));
            Ok(Variant::Nil)
        }
        "is_breakpoint" => {
            let loc = debug_breakpoint_arg(args, "is_breakpoint")?;
            Ok(Variant::Bool(
                ENGINE_DEBUGGER.with(|d| d.borrow().has_breakpoint(&loc)),
            ))
        }
        "clear_breakpoints" => {
            ENGINE_DEBUGGER.with(|d| d.borrow_mut().clear_breakpoints());
            Ok(Variant::Nil)
        }
        "is_skipping_breakpoints" => Ok(Variant::Bool(DEBUG_SKIP_BREAKPOINTS.with(|s| s.get()))),

        // -- stepping counters. Godot's `depth` is the stepping counter, NOT the
        // call-stack depth Debugger::stack_depth() reports.
        "set_depth" => {
            let d = debug_int_arg(args, 0, "set_depth")?;
            DEBUG_DEPTH.with(|c| c.set(d));
            Ok(Variant::Nil)
        }
        "get_depth" => Ok(Variant::Int(DEBUG_DEPTH.with(|c| c.get()))),
        "set_lines_left" => {
            let l = debug_int_arg(args, 0, "set_lines_left")?;
            DEBUG_LINES_LEFT.with(|c| c.set(l));
            Ok(Variant::Nil)
        }
        "get_lines_left" => Ok(Variant::Int(DEBUG_LINES_LEFT.with(|c| c.get()))),

        // -- profilers
        "register_profiler" => {
            let name = debug_name_arg(args, 0, "register_profiler")?;
            // The trailing `;` matters: without it the closure would return the
            // `&mut` from `or_default()`, which borrows the temporary RefMut.
            DEBUG_PROFILERS.with(|p| {
                p.borrow_mut().entry(name).or_default();
            });
            Ok(Variant::Nil)
        }
        "unregister_profiler" => {
            let name = debug_name_arg(args, 0, "unregister_profiler")?;
            DEBUG_PROFILERS.with(|p| p.borrow_mut().remove(&name));
            Ok(Variant::Nil)
        }
        "has_profiler" => {
            let name = debug_name_arg(args, 0, "has_profiler")?;
            Ok(Variant::Bool(
                DEBUG_PROFILERS.with(|p| p.borrow().contains_key(&name)),
            ))
        }
        // Registered and *enabled* are different: a registered profiler is off
        // until profiler_enable turns it on.
        "is_profiling" => {
            let name = debug_name_arg(args, 0, "is_profiling")?;
            Ok(Variant::Bool(DEBUG_PROFILERS.with(|p| {
                p.borrow().get(&name).map(|s| s.enabled).unwrap_or(false)
            })))
        }
        "profiler_enable" => {
            let name = debug_name_arg(args, 0, "profiler_enable")?;
            let enable = debug_bool_arg(args, 1, "profiler_enable")?;
            DEBUG_PROFILERS.with(|p| {
                p.borrow_mut().entry(name).or_default().enabled = enable;
            });
            Ok(Variant::Nil)
        }
        "profiler_add_frame_data" => {
            let name = debug_name_arg(args, 0, "profiler_add_frame_data")?;
            let data = match args.get(1) {
                Some(Variant::Array(a)) => a.clone(),
                _ => {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "EngineDebugger.profiler_add_frame_data() expects an Array of data".into(),
                    )))
                }
            };
            DEBUG_PROFILERS.with(|p| {
                p.borrow_mut().entry(name).or_default().frames.push(data);
            });
            Ok(Variant::Nil)
        }

        // -- message captures
        "register_message_capture" => {
            let name = debug_name_arg(args, 0, "register_message_capture")?;
            let callable = args.get(1).cloned().unwrap_or(Variant::Nil);
            DEBUG_CAPTURES.with(|c| c.borrow_mut().insert(name, callable));
            Ok(Variant::Nil)
        }
        "unregister_message_capture" => {
            let name = debug_name_arg(args, 0, "unregister_message_capture")?;
            DEBUG_CAPTURES.with(|c| c.borrow_mut().remove(&name));
            Ok(Variant::Nil)
        }
        "has_capture" => {
            let name = debug_name_arg(args, 0, "has_capture")?;
            Ok(Variant::Bool(
                DEBUG_CAPTURES.with(|c| c.borrow().contains_key(&name)),
            ))
        }

        // -- transport
        "send_message" => {
            let message = debug_name_arg(args, 0, "send_message")?;
            let data = match args.get(1) {
                Some(Variant::Array(a)) => a.clone(),
                None | Some(Variant::Nil) => Vec::new(),
                _ => {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "EngineDebugger.send_message() expects an Array of data".into(),
                    )))
                }
            };
            DEBUG_MESSAGES.with(|m| m.borrow_mut().push((message, data)));
            Ok(Variant::Nil)
        }
        // Godot polls the debugger between lines; with no live session there is
        // nothing to poll, so this is a no-op rather than an error.
        "line_poll" => Ok(Variant::Nil),

        other => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("EngineDebugger.{other}"),
        ))),
    }
}

// ---------------------------------------------------------------------------
// UndoRedo (pat-nzgdu.155)
// ---------------------------------------------------------------------------

/// Tags an `ObjectId` issued by [`new_undo_redo_handle`]; mirrors the
/// ConfigFile / FileAccess / DirAccess / CameraFeed handle tags
/// (0xCF / 0xFA / 0xDA / 0xCA).
const UNDO_REDO_HANDLE_TAG: u64 = 0xED00_0000_0000_0000;

/// Godot's `UndoRedo.MergeMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum MergeMode {
    /// Never merge — the default.
    #[default]
    Disable,
    /// Merge with the previous action when it has the same name, keeping only
    /// the *ends*: the new do-ops replace the previous ones, but the ORIGINAL
    /// undo-ops are kept, so undoing returns to the state before the first of
    /// the merged pair.
    Ends,
    /// Merge with the previous same-named action, appending both halves.
    All,
}

impl MergeMode {
    fn from_int(v: i64) -> Self {
        match v {
            1 => MergeMode::Ends,
            2 => MergeMode::All,
            _ => MergeMode::Disable,
        }
    }
}

/// A single recorded property write.
#[derive(Debug, Clone)]
struct PropOp {
    object: u64,
    property: String,
    value: Variant,
}

/// One committed (or in-progress) undo/redo action.
#[derive(Debug, Clone, Default)]
struct UndoRedoAction {
    name: String,
    merge_mode: MergeMode,
    do_ops: Vec<PropOp>,
    undo_ops: Vec<PropOp>,
}

/// A script-visible `UndoRedo` instance.
///
/// Held interpreter-local (like `CameraFeed`) rather than delegating to
/// gdeditor's `UndoRedoManager`: gdeditor **depends on** gdscript-interop, so
/// reaching the other way would be a dependency cycle (pat-nzgdu.155).
#[derive(Debug, Default)]
struct UndoRedoState {
    /// Committed actions, oldest first.
    history: Vec<UndoRedoAction>,
    /// Index of the action that `undo()` would revert; `-1` when fully undone.
    current: i64,
    /// The action being built by `create_action()`, if any.
    pending: Option<UndoRedoAction>,
    /// Bumped on every commit/undo/redo (Godot's `get_version()`).
    version: u64,
    /// True while `commit_action()` is applying its operations.
    committing: bool,
}

thread_local! {
    /// Live `UndoRedo` handles, keyed by raw ObjectId.
    static UNDO_REDOS: std::cell::RefCell<HashMap<u64, UndoRedoState>> =
        std::cell::RefCell::new(HashMap::new());
    /// Sequence backing the handle id.
    static NEXT_UNDO_REDO: std::cell::Cell<u64> = std::cell::Cell::new(1);
}

/// Returns `true` when `id` was issued by [`new_undo_redo_handle`].
fn is_undo_redo_handle(id: gdcore::id::ObjectId) -> bool {
    id.raw() & 0xFF00_0000_0000_0000 == UNDO_REDO_HANDLE_TAG
}

// ---------------------------------------------------------------------------
// EncodedObjectAsID (pat-nzgdu.189)
// ---------------------------------------------------------------------------

/// Tags an `ObjectId` standing for an `EncodedObjectAsID`; mirrors the
/// ConfigFile / FileAccess / DirAccess / CameraFeed / UndoRedo tags
/// (0xCF / 0xFA / 0xDA / 0xCA / 0xED).
const ENCODED_OBJECT_AS_ID_HANDLE_TAG: u64 = 0xE1_00_0000_0000_0000;

thread_local! {
    /// Maps an EncodedObjectAsID handle to the raw id it *reports*.
    ///
    /// The target is data, never a reference: nothing in this file resolves it
    /// against the scene. That is the entire point — see
    /// [`wrap_decoded_object_ids`].
    static ENCODED_OBJECT_IDS: std::cell::RefCell<HashMap<u64, u64>> =
        std::cell::RefCell::new(HashMap::new());
    static NEXT_ENCODED_OBJECT_ID: std::cell::Cell<u64> = std::cell::Cell::new(1);
}

/// Returns `true` when `id` was issued by [`new_encoded_object_as_id_handle`].
fn is_encoded_object_as_id_handle(id: gdcore::id::ObjectId) -> bool {
    id.raw() & 0xFF00_0000_0000_0000 == ENCODED_OBJECT_AS_ID_HANDLE_TAG
}

/// Mints an `EncodedObjectAsID` reporting `target`.
fn new_encoded_object_as_id_handle(target: u64) -> gdcore::id::ObjectId {
    let seq = NEXT_ENCODED_OBJECT_ID.with(|n| {
        let v = n.get();
        n.set(v + 1);
        v
    });
    let raw = ENCODED_OBJECT_AS_ID_HANDLE_TAG | seq;
    ENCODED_OBJECT_IDS.with(|m| m.borrow_mut().insert(raw, target));
    gdcore::id::ObjectId::from_raw(raw)
}

/// Replaces every decoded object id with an inert `EncodedObjectAsID`.
///
/// **This is a security boundary, not a formality.** Godot's `bytes_to_var`
/// hands back an `EncodedObjectAsID` whose only surface is `object_id`; turning
/// that id into a usable object needs an explicit `instance_from_id()`. That
/// extra step is exactly what `p_allow_objects = false` buys
/// (`core/io/marshalls.cpp:699-715`).
///
/// In this port `Variant::ObjectId` is *not* an inert id — it is the live node
/// reference `get_node()` returns, and calling `get_children()` on one walks the
/// scene tree. Returning a decoded id directly would therefore let
/// `bytes_to_var(untrusted).get_children()` reach into the running scene, with
/// ids that are trivially guessable (they start at 1). So every decoded id is
/// re-wrapped in a tagged handle that reports the number and dereferences to
/// nothing.
///
/// Applied recursively: an RPC payload is normally an array of arguments, and an
/// id nested three levels down is exactly as dangerous as a bare one.
fn wrap_decoded_object_ids(v: Variant) -> Variant {
    match v {
        Variant::ObjectId(id) => Variant::ObjectId(new_encoded_object_as_id_handle(id.raw())),
        Variant::Array(items) => {
            Variant::Array(items.into_iter().map(wrap_decoded_object_ids).collect())
        }
        Variant::Dictionary(d) => {
            let mut out = gdvariant::dictionary::Dictionary::new();
            for (k, val) in d.iter() {
                // Keys are wrapped too: a dictionary keyed by object is rare but
                // legal, and a live key is just as reachable as a live value.
                out.insert(
                    wrap_decoded_object_ids(k.clone()),
                    wrap_decoded_object_ids(val.clone()),
                );
            }
            Variant::Dictionary(out)
        }
        other => other,
    }
}

/// `EncodedObjectAsID`'s whole method surface: the id it reports, and nothing
/// that could resolve it.
fn call_encoded_object_as_id_instance(
    id: gdcore::id::ObjectId,
    method: &str,
    args: &[Variant],
) -> Result<Variant, RuntimeError> {
    let raw = id.raw();
    match method {
        "get_object_id" => Ok(Variant::Int(
            ENCODED_OBJECT_IDS.with(|m| m.borrow().get(&raw).copied().unwrap_or(0)) as i64,
        )),
        "set_object_id" => {
            let target = match args.first() {
                Some(Variant::Int(i)) => *i as u64,
                _ => {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "EncodedObjectAsID.set_object_id() expects an int id".into(),
                    )))
                }
            };
            ENCODED_OBJECT_IDS.with(|m| m.borrow_mut().insert(raw, target));
            Ok(Variant::Nil)
        }
        other => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("EncodedObjectAsID.{other}"),
        ))),
    }
}

/// Allocates an `UndoRedo` in the handle table (script `UndoRedo.new()`).
fn new_undo_redo_handle() -> gdcore::id::ObjectId {
    let seq = NEXT_UNDO_REDO.with(|n| {
        let v = n.get();
        n.set(v + 1);
        v
    });
    let raw = UNDO_REDO_HANDLE_TAG | seq;
    UNDO_REDOS.with(|m| {
        m.borrow_mut().insert(
            raw,
            UndoRedoState {
                current: -1,
                ..Default::default()
            },
        );
    });
    gdcore::id::ObjectId::from_raw(raw)
}

/// Reads a `(object, property, value)` triple from an add_*_property call.
fn prop_op_args(args: &[Variant], who: &str) -> Result<PropOp, RuntimeError> {
    let object = match args.first() {
        Some(Variant::ObjectId(id)) => id.raw(),
        _ => {
            return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                "{who} expects an object as its first argument"
            ))))
        }
    };
    let property = match args.get(1) {
        Some(Variant::String(s)) => s.clone(),
        Some(Variant::StringName(s)) => s.as_str().to_string(),
        _ => {
            return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                "{who} expects a property name"
            ))))
        }
    };
    Ok(PropOp {
        object,
        property,
        value: args.get(2).cloned().unwrap_or(Variant::Nil),
    })
}

/// `UndoRedo` instance methods (pat-nzgdu.155).
///
/// `scene_access` applies the property writes; without it the history is still
/// recorded (so a headless script can build actions) but nothing is written —
/// there is no object model to write to.
fn call_undo_redo_instance(
    id: gdcore::id::ObjectId,
    method: &str,
    args: &[Variant],
    access: Option<&mut (dyn SceneAccess + 'static)>,
) -> Result<Variant, RuntimeError> {
    let raw = id.raw();

    /// Applies a batch of property writes through the scene.
    ///
    /// Note the explicit `+ 'static` on the trait object, and the `Option`
    /// taken **by value**. Both matter, and the elided `Option<&mut dyn
    /// SceneAccess>` does NOT compile here (E0521, "borrowed data escapes"):
    /// the caller's `scene_access: Option<Box<dyn SceneAccess>>` field means
    /// `as_deref_mut()` yields `Option<&'r mut (dyn SceneAccess + 'static)>`,
    /// where `'r` is only a reborrow of the interpreter's `self`. Eliding the
    /// bound here would instead ask for `Option<&'x mut (dyn SceneAccess +
    /// 'x)>`, and coercions do not reach inside a generic constructor like
    /// `Option` — so the types must match by subtyping, `&mut` is invariant in
    /// its pointee, and `'x` is forced to `'static`, dragging `'r` with it.
    /// Naming `'static` (which the boxed accessor genuinely is) makes the
    /// pointees equal, leaving only the covariant `'r: 'x` to satisfy.
    /// Contrast the bare `&mut dyn SceneAccess` params above, whose callers
    /// pass `&mut **access` — a fresh reborrow, which *is* a coercion site.
    /// Same shape, same reason: `gdplatform`'s `NetworkContext::peer_mut`
    /// (`network.rs:289`) also spells out `Option<&mut (dyn NetworkPeer +
    /// 'static)>` over an `Option<Box<dyn _>>` field.
    fn apply(ops: &[PropOp], access: Option<&mut (dyn SceneAccess + 'static)>) {
        if let Some(a) = access {
            for op in ops {
                a.set_node_property(op.object, &op.property, op.value.clone());
            }
        }
    }

    match method {
        "create_action" => {
            let name = match args.first() {
                Some(Variant::String(s)) => s.clone(),
                Some(Variant::StringName(s)) => s.as_str().to_string(),
                _ => String::new(),
            };
            let merge_mode = match args.get(1) {
                Some(Variant::Int(i)) => MergeMode::from_int(*i),
                _ => MergeMode::Disable,
            };
            UNDO_REDOS.with(|m| {
                if let Some(s) = m.borrow_mut().get_mut(&raw) {
                    s.pending = Some(UndoRedoAction {
                        name,
                        merge_mode,
                        ..Default::default()
                    });
                }
            });
            Ok(Variant::Nil)
        }
        "add_do_property" => {
            let op = prop_op_args(args, "UndoRedo.add_do_property()")?;
            UNDO_REDOS.with(|m| {
                if let Some(s) = m.borrow_mut().get_mut(&raw) {
                    if let Some(p) = s.pending.as_mut() {
                        p.do_ops.push(op);
                    }
                }
            });
            Ok(Variant::Nil)
        }
        "add_undo_property" => {
            let op = prop_op_args(args, "UndoRedo.add_undo_property()")?;
            UNDO_REDOS.with(|m| {
                if let Some(s) = m.borrow_mut().get_mut(&raw) {
                    if let Some(p) = s.pending.as_mut() {
                        p.undo_ops.push(op);
                    }
                }
            });
            Ok(Variant::Nil)
        }
        "commit_action" => {
            // Godot: commit_action(execute = true) — false records the action
            // without running the do-half (the caller already applied it).
            let execute = !matches!(args.first(), Some(Variant::Bool(false)));
            let to_apply = UNDO_REDOS.with(|m| {
                let mut map = m.borrow_mut();
                let s = match map.get_mut(&raw) {
                    Some(s) => s,
                    None => return Vec::new(),
                };
                let action = match s.pending.take() {
                    Some(a) => a,
                    None => return Vec::new(),
                };

                // Anything redoable ahead of the cursor is discarded — a new
                // action forks the history (Godot's behaviour).
                s.history.truncate((s.current + 1).max(0) as usize);

                let merged = match action.merge_mode {
                    MergeMode::Disable => false,
                    MergeMode::Ends | MergeMode::All => s
                        .history
                        .last()
                        .map(|prev| prev.name == action.name)
                        .unwrap_or(false),
                };

                let ops = action.do_ops.clone();
                if merged {
                    let prev = s.history.last_mut().expect("merged implies a previous");
                    match action.merge_mode {
                        MergeMode::Ends => {
                            // Keep the ORIGINAL undo-ops (the far end) and take
                            // the NEW do-ops: undo jumps back past both.
                            prev.do_ops = action.do_ops;
                        }
                        MergeMode::All => {
                            prev.do_ops.extend(action.do_ops);
                            prev.undo_ops.extend(action.undo_ops);
                        }
                        MergeMode::Disable => unreachable!(),
                    }
                } else {
                    s.history.push(action);
                    s.current = s.history.len() as i64 - 1;
                }
                s.version += 1;
                s.committing = true;
                ops
            });

            if execute {
                apply(&to_apply, access);
            }
            UNDO_REDOS.with(|m| {
                if let Some(s) = m.borrow_mut().get_mut(&raw) {
                    s.committing = false;
                }
            });
            Ok(Variant::Nil)
        }
        "undo" => {
            let ops = UNDO_REDOS.with(|m| {
                let mut map = m.borrow_mut();
                let s = match map.get_mut(&raw) {
                    Some(s) => s,
                    None => return None,
                };
                if s.current < 0 {
                    return None;
                }
                let ops = s.history[s.current as usize].undo_ops.clone();
                s.current -= 1;
                s.version += 1;
                Some(ops)
            });
            match ops {
                Some(ops) => {
                    apply(&ops, access);
                    Ok(Variant::Bool(true))
                }
                None => Ok(Variant::Bool(false)),
            }
        }
        "redo" => {
            let ops = UNDO_REDOS.with(|m| {
                let mut map = m.borrow_mut();
                let s = match map.get_mut(&raw) {
                    Some(s) => s,
                    None => return None,
                };
                let next = s.current + 1;
                if next as usize >= s.history.len() {
                    return None;
                }
                let ops = s.history[next as usize].do_ops.clone();
                s.current = next;
                s.version += 1;
                Some(ops)
            });
            match ops {
                Some(ops) => {
                    apply(&ops, access);
                    Ok(Variant::Bool(true))
                }
                None => Ok(Variant::Bool(false)),
            }
        }
        "has_undo" => Ok(Variant::Bool(UNDO_REDOS.with(|m| {
            m.borrow()
                .get(&raw)
                .map(|s| s.current >= 0)
                .unwrap_or(false)
        }))),
        "has_redo" => Ok(Variant::Bool(UNDO_REDOS.with(|m| {
            m.borrow()
                .get(&raw)
                .map(|s| (s.current + 1) < s.history.len() as i64)
                .unwrap_or(false)
        }))),
        "is_committing_action" => {
            Ok(Variant::Bool(UNDO_REDOS.with(|m| {
                m.borrow().get(&raw).map(|s| s.committing).unwrap_or(false)
            })))
        }
        "get_version" => {
            Ok(Variant::Int(UNDO_REDOS.with(|m| {
                m.borrow().get(&raw).map(|s| s.version as i64).unwrap_or(0)
            })))
        }
        "get_history_count" => Ok(Variant::Int(UNDO_REDOS.with(|m| {
            m.borrow()
                .get(&raw)
                .map(|s| s.history.len() as i64)
                .unwrap_or(0)
        }))),
        "get_current_action" => {
            Ok(Variant::Int(UNDO_REDOS.with(|m| {
                m.borrow().get(&raw).map(|s| s.current).unwrap_or(-1)
            })))
        }
        "get_current_action_name" => Ok(Variant::String(UNDO_REDOS.with(|m| {
            m.borrow()
                .get(&raw)
                .and_then(|s| {
                    if s.current < 0 {
                        None
                    } else {
                        s.history.get(s.current as usize).map(|a| a.name.clone())
                    }
                })
                .unwrap_or_default()
        }))),
        "get_action_name" => {
            let idx = match args.first() {
                Some(Variant::Int(i)) => *i,
                _ => -1,
            };
            Ok(Variant::String(UNDO_REDOS.with(|m| {
                m.borrow()
                    .get(&raw)
                    .and_then(|s| {
                        if idx < 0 {
                            None
                        } else {
                            s.history.get(idx as usize).map(|a| a.name.clone())
                        }
                    })
                    .unwrap_or_default()
            })))
        }
        "clear_history" => {
            UNDO_REDOS.with(|m| {
                if let Some(s) = m.borrow_mut().get_mut(&raw) {
                    s.history.clear();
                    s.current = -1;
                    s.pending = None;
                    s.version += 1;
                }
            });
            Ok(Variant::Nil)
        }
        other => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("UndoRedo.{other}"),
        ))),
    }
}

/// Allocates a `CameraFeed` in the handle table (script `CameraFeed.new()`).
fn new_camera_feed_handle() -> gdcore::id::ObjectId {
    let seq = NEXT_CAMERA_FEED.with(|n| {
        let v = n.get();
        n.set(v + 1);
        v
    });
    let raw = CAMERA_FEED_HANDLE_TAG | seq;
    CAMERA_FEEDS.with(|feeds| {
        feeds.borrow_mut().insert(
            raw,
            CameraFeedState {
                id: seq as i64,
                name: String::new(),
                // CameraFeed.FEED_UNSPECIFIED
                position: 0,
                active: false,
            },
        );
    });
    gdcore::id::ObjectId::from_raw(raw)
}

/// `CameraFeed` instance methods (pat-nzgdu.179).
fn call_camera_feed_instance(
    id: gdcore::id::ObjectId,
    method: &str,
    args: &[Variant],
) -> Result<Variant, RuntimeError> {
    let raw = id.raw();
    CAMERA_FEEDS.with(|feeds| {
        let mut map = feeds.borrow_mut();
        let feed = map.get_mut(&raw).ok_or_else(|| {
            RuntimeError::new(RuntimeErrorKind::TypeError(
                "CameraFeed handle is invalid".into(),
            ))
        })?;
        match method {
            "get_id" => Ok(Variant::Int(feed.id)),
            "get_name" => Ok(Variant::String(feed.name.clone())),
            "set_name" => match args.first() {
                Some(Variant::String(s)) => {
                    feed.name = s.clone();
                    Ok(Variant::Nil)
                }
                _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "CameraFeed.set_name() takes a string".into(),
                ))),
            },
            "get_position" => Ok(Variant::Int(feed.position)),
            "set_position" => {
                if let Some(Variant::Int(p)) = args.first() {
                    feed.position = *p;
                }
                Ok(Variant::Nil)
            }
            "is_active" => Ok(Variant::Bool(feed.active)),
            "set_active" => {
                if let Some(Variant::Bool(b)) = args.first() {
                    feed.active = *b;
                }
                Ok(Variant::Nil)
            }
            // No camera driver: the feed never carries an image, so datatype is
            // CameraFeed.FEED_NOIMAGE (0), no formats are reported, set_format
            // fails, and no GPU texture is allocated.
            "get_datatype" => Ok(Variant::Int(0)),
            "get_formats" => Ok(Variant::Array(Vec::new())),
            "set_format" => Ok(Variant::Bool(false)),
            "get_texture_tex_id" => Ok(Variant::Int(0)),
            other => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
                format!("CameraFeed.{other}"),
            ))),
        }
    })
}

/// Extracts the raw `CameraFeed` handle from an `add_feed`/`remove_feed` arg.
fn camera_feed_arg(args: &[Variant], who: &str) -> Result<u64, RuntimeError> {
    match args.first() {
        Some(Variant::ObjectId(oid)) if is_camera_feed_handle(*oid) => Ok(oid.raw()),
        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
            "CameraServer.{who}() takes a CameraFeed"
        )))),
    }
}

/// `CameraServer` singleton (pat-nzgdu.179). Godot exposes CameraServer on every
/// platform but ships no web camera driver, so on the web target it resolves and
/// reports zero feeds rather than erroring. Patina matches that parity headlessly:
/// `get_feed_count()` is 0 until a script calls `add_feed`, and every method
/// resolves instead of raising UndefinedFunction. A real getUserMedia-backed feed
/// is a browser-side (wasm) concern layered on this same surface.
fn call_cameraserver_static(method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    match method {
        "get_feed_count" => Ok(Variant::Int(
            CAMERA_SERVER_FEEDS.with(|f| f.borrow().len() as i64),
        )),
        "get_feed" => {
            let idx = match args.first() {
                Some(Variant::Int(i)) => *i,
                _ => {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "CameraServer.get_feed() takes an index".into(),
                    )))
                }
            };
            Ok(CAMERA_SERVER_FEEDS.with(|f| {
                let list = f.borrow();
                if idx < 0 {
                    return Variant::Nil;
                }
                match list.get(idx as usize) {
                    Some(raw) => Variant::ObjectId(gdcore::id::ObjectId::from_raw(*raw)),
                    None => Variant::Nil,
                }
            }))
        }
        "feeds" => Ok(Variant::Array(CAMERA_SERVER_FEEDS.with(|f| {
            f.borrow()
                .iter()
                .map(|raw| Variant::ObjectId(gdcore::id::ObjectId::from_raw(*raw)))
                .collect()
        }))),
        "add_feed" => {
            let raw = camera_feed_arg(args, "add_feed")?;
            CAMERA_SERVER_FEEDS.with(|f| {
                let mut list = f.borrow_mut();
                if !list.contains(&raw) {
                    list.push(raw);
                }
            });
            Ok(Variant::Nil)
        }
        "remove_feed" => {
            let raw = camera_feed_arg(args, "remove_feed")?;
            CAMERA_SERVER_FEEDS.with(|f| f.borrow_mut().retain(|r| *r != raw));
            Ok(Variant::Nil)
        }
        "set_monitoring_feeds" => {
            if let Some(Variant::Bool(b)) = args.first() {
                CAMERA_MONITORING.with(|m| m.set(*b));
            }
            Ok(Variant::Nil)
        }
        "is_monitoring_feeds" => Ok(Variant::Bool(CAMERA_MONITORING.with(|m| m.get()))),
        other => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("CameraServer.{other}"),
        ))),
    }
}

/// `FramebufferCacheRD.get_cache_multipass(...)` (pat-nzgdu.210).
///
/// ⚠️ Answers `Nil`, and that is the *documented unsupported value*, not a
/// placeholder someone forgot to fill in. Godot returns an **`RID`** for the
/// cached framebuffer, and Patina's `Variant` has **no `RID` variant at all** —
/// `gdvariant::binary` lists `RID` among the types the enum lacks. There is also
/// no `RenderingDevice` behind it to cache anything (pat-nzgdu.162). `Nil` is
/// the only value this can honestly name until both exist.
///
/// Without this arm the class is reachable from `ClassDB.class_exists` yet a
/// script calling it gets `UndefinedVariable("FramebufferCacheRD")` — the class
/// resolves for reflection while the call does not, which is the gap the bead is
/// about ("so scripts referencing it resolve").
fn call_framebuffercacherd_static(method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    match method {
        // (textures: RID[], passes: RDFramebufferPass[], views: int). The arity
        // is checked so a miscall is a TypeError, never a panic.
        "get_cache_multipass" => {
            if args.len() != 3 {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                    "FramebufferCacheRD.get_cache_multipass() takes 3 arguments \
                     (textures, passes, views), got {}",
                    args.len()
                ))));
            }
            Ok(Variant::Nil)
        }
        other => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("FramebufferCacheRD.{other}"),
        ))),
    }
}

/// Standard (RFC 4648) base64 encode with `=` padding.
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[((n >> 18) & 63) as usize] as char);
        out.push(CHARS[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            CHARS[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            CHARS[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Standard (RFC 4648) base64 decode. Ignores `=` padding and ASCII whitespace;
/// errors on any other invalid character.
fn base64_decode(s: &str) -> Result<Vec<u8>, RuntimeError> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let filtered: Vec<u8> = s
        .bytes()
        .filter(|&c| c != b'=' && !c.is_ascii_whitespace())
        .collect();
    let mut out = Vec::with_capacity(filtered.len() / 4 * 3);
    for chunk in filtered.chunks(4) {
        let mut n = 0u32;
        for &c in chunk {
            let v = val(c).ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::TypeError(
                    "invalid base64 character".into(),
                ))
            })?;
            n = (n << 6) | v;
        }
        let bits = chunk.len() * 6;
        let bytes_out = bits / 8;
        n <<= (24 - bits) as u32; // MSB-align within a 24-bit window
        for i in 0..bytes_out {
            out.push(((n >> (16 - i * 8)) & 0xFF) as u8);
        }
    }
    Ok(out)
}

/// `Marshalls` singleton (pat-nzgdu.79) — base64 <-> Variant / raw / utf8 via
/// the shared `gdvariant` binary codec.
fn call_marshalls_static(method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    let first_str = |who: &str| -> Result<String, RuntimeError> {
        match args.first() {
            Some(Variant::String(s)) => Ok(s.clone()),
            _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                "{who} expects a base64 string"
            )))),
        }
    };
    match method {
        "variant_to_base64" => {
            let v = args.first().ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::TypeError(
                    "Marshalls.variant_to_base64() takes a value".into(),
                ))
            })?;
            let bytes = gdvariant::binary::encode_variant(v).map_err(|e| {
                RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                    "variant_to_base64: {e:?}"
                )))
            })?;
            Ok(Variant::String(base64_encode(&bytes)))
        }
        "base64_to_variant" => {
            let bytes = base64_decode(&first_str("Marshalls.base64_to_variant()")?)?;
            let (v, _) = gdvariant::binary::decode_variant(&bytes).map_err(|e| {
                RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                    "base64_to_variant: {e:?}"
                )))
            })?;
            // Same decode, same hazard as `bytes_to_var`: base64 is just a
            // transport for the identical bytes, so an id decoded here must be
            // an inert EncodedObjectAsID too (pat-nzgdu.189).
            Ok(wrap_decoded_object_ids(v))
        }
        "raw_to_base64" => match args.first() {
            Some(Variant::PackedByteArray(b)) => Ok(Variant::String(base64_encode(b))),
            _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                "Marshalls.raw_to_base64() expects a PackedByteArray".into(),
            ))),
        },
        "base64_to_raw" => Ok(Variant::PackedByteArray(base64_decode(&first_str(
            "Marshalls.base64_to_raw()",
        )?)?)),
        "utf8_to_base64" => Ok(Variant::String(base64_encode(
            first_str("Marshalls.utf8_to_base64()")?.as_bytes(),
        ))),
        "base64_to_utf8" => {
            let bytes = base64_decode(&first_str("Marshalls.base64_to_utf8()")?)?;
            Ok(Variant::String(
                String::from_utf8_lossy(&bytes).into_owned(),
            ))
        }
        other => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("Marshalls.{other}"),
        ))),
    }
}

/// `ResourceUID` singleton (pat-nzgdu.79) — id <-> `uid://` text via
/// `gdresource::uid`.
fn call_resourceuid_static(method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    match method {
        "id_to_text" => {
            let id = to_int(args.first().ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::TypeError(
                    "ResourceUID.id_to_text() takes an id".into(),
                ))
            })?)?;
            Ok(Variant::String(gdresource::uid::id_to_text(id)))
        }
        "text_to_id" => match args.first() {
            Some(Variant::String(s)) => Ok(Variant::Int(gdresource::uid::text_to_id(s).raw())),
            _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                "ResourceUID.text_to_id() expects a uid:// string".into(),
            ))),
        },
        "has_id" => {
            let id = to_int(args.first().ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::TypeError(
                    "ResourceUID.has_id() takes an id".into(),
                ))
            })?)?;
            Ok(Variant::Bool(
                gdresource::uid::id_to_text(id) != "uid://<invalid>",
            ))
        }
        other => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("ResourceUID.{other}"),
        ))),
    }
}

thread_local! {
    /// Active locale for `TranslationServer` / `tr()` (pat-nzgdu.92). Godot's
    /// default is the OS locale; headless defaults to "en".
    static TR_LOCALE: std::cell::RefCell<String> = std::cell::RefCell::new(String::from("en"));
    /// Loaded translation catalogs, `locale -> (source -> translated)`. The
    /// `.po` / `.csv` importers (gdresource) populate this at boot; scripts /
    /// tests can seed it via `TranslationServer.add_message`.
    static TR_CATALOGS: std::cell::RefCell<HashMap<String, HashMap<String, String>>> =
        std::cell::RefCell::new(HashMap::new());
}

/// Translates `key` via the active-locale catalog, returning `key` unchanged
/// when there is no entry (Godot's fallback). Shared by `tr()` and
/// `TranslationServer.translate()`.
fn tr_lookup(key: &str) -> String {
    let locale = TR_LOCALE.with(|l| l.borrow().clone());
    TR_CATALOGS.with(|c| {
        c.borrow()
            .get(&locale)
            .and_then(|cat| cat.get(key))
            .cloned()
            .unwrap_or_else(|| key.to_string())
    })
}

/// Plural-aware lookup (`tr_n` / `translate_plural`): picks the singular `key`
/// for `n == 1`, otherwise `plural`, then translates.
fn tr_lookup_plural(key: &str, plural: &str, n: i64) -> String {
    tr_lookup(if n == 1 { key } else { plural })
}

/// `TranslationServer` singleton (pat-nzgdu.92) — locale state + catalog lookup.
/// The `.po`/`.csv` loaders (gdresource) and Translation resources are the
/// cross-crate remainder; `add_message` is the interim seed path for the store.
fn call_translationserver_static(method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    let arg_string = |idx: usize, who: &str| -> Result<String, RuntimeError> {
        match args.get(idx) {
            Some(Variant::String(s)) => Ok(s.clone()),
            Some(Variant::StringName(sn)) => Ok(sn.as_str().to_string()),
            _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                "{who} expects a string argument"
            )))),
        }
    };
    match method {
        "set_locale" => {
            let loc = arg_string(0, "TranslationServer.set_locale()")?;
            TR_LOCALE.with(|l| *l.borrow_mut() = loc);
            Ok(Variant::Nil)
        }
        "get_locale" => Ok(Variant::String(TR_LOCALE.with(|l| l.borrow().clone()))),
        "translate" => Ok(Variant::String(tr_lookup(&arg_string(
            0,
            "TranslationServer.translate()",
        )?))),
        "translate_plural" => {
            let key = arg_string(0, "TranslationServer.translate_plural()")?;
            let plural = arg_string(1, "TranslationServer.translate_plural()")?;
            let n = to_int(args.get(2).unwrap_or(&Variant::Int(1)))?;
            Ok(Variant::String(tr_lookup_plural(&key, &plural, n)))
        }
        // Patina seed path pending the .po/.csv loaders + Translation resources:
        // add_message(locale, source, translated).
        "add_message" => {
            let locale = arg_string(0, "TranslationServer.add_message()")?;
            let src = arg_string(1, "TranslationServer.add_message()")?;
            let xlated = arg_string(2, "TranslationServer.add_message()")?;
            TR_CATALOGS.with(|c| {
                c.borrow_mut()
                    .entry(locale)
                    .or_default()
                    .insert(src, xlated);
            });
            Ok(Variant::Nil)
        }
        "get_loaded_locales" => Ok(Variant::PackedStringArray(
            TR_CATALOGS.with(|c| c.borrow().keys().cloned().collect()),
        )),
        "clear" => {
            TR_CATALOGS.with(|c| c.borrow_mut().clear());
            Ok(Variant::Nil)
        }
        "get_translation_object" => Ok(Variant::Nil),
        other => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("TranslationServer.{other}"),
        ))),
    }
}

fn call_engine_static(method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    match method {
        "is_editor_hint" => {
            if !args.is_empty() {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "Engine.is_editor_hint() takes 0 arguments".into(),
                )));
            }
            Ok(Variant::Bool(ENGINE_EDITOR_HINT.load(Ordering::Relaxed)))
        }
        "get_frames_per_second" => {
            if !args.is_empty() {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "Engine.get_frames_per_second() takes 0 arguments".into(),
                )));
            }
            let milli = ENGINE_FPS_MILLI.load(Ordering::Relaxed);
            Ok(Variant::Float(milli as f64 / 1000.0))
        }
        other => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("Engine.{other}"),
        ))),
    }
}

fn call_time_static(method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    match method {
        "get_ticks_msec" => {
            if !args.is_empty() {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "Time.get_ticks_msec() takes 0 arguments".into(),
                )));
            }
            Ok(Variant::Int(ticks_msec()))
        }
        "get_ticks_usec" => {
            if !args.is_empty() {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "Time.get_ticks_usec() takes 0 arguments".into(),
                )));
            }
            Ok(Variant::Int(process_epoch().elapsed().as_micros() as i64))
        }
        "get_unix_time_from_system" => {
            if !args.is_empty() {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "Time.get_unix_time_from_system() takes 0 arguments".into(),
                )));
            }
            let secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);
            Ok(Variant::Float(secs))
        }
        "get_datetime_dict_from_system" => {
            // Bounded: return a dictionary with unix time fields; full local
            // calendar decomposition is deferred.
            if args.len() > 1 {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "Time.get_datetime_dict_from_system() takes 0 or 1 arguments".into(),
                )));
            }
            let unix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            // Approximate UTC breakdown without external chrono crate.
            let days = unix / 86_400;
            let day_secs = unix.rem_euclid(86_400);
            let hour = day_secs / 3600;
            let minute = (day_secs % 3600) / 60;
            let second = day_secs % 60;
            // Civil date from days since 1970-01-01 (Howard Hinnant algorithm).
            let (year, month, day) = civil_from_days(days);
            let mut dict = Dictionary::new();
            dict.insert("year", Variant::Int(year));
            dict.insert("month", Variant::Int(month));
            dict.insert("day", Variant::Int(day));
            dict.insert("hour", Variant::Int(hour));
            dict.insert("minute", Variant::Int(minute));
            dict.insert("second", Variant::Int(second));
            Ok(Variant::Dictionary(dict))
        }
        other => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("Time.{other}"),
        ))),
    }
}

/// Converts days since Unix epoch to (year, month, day) in the proleptic Gregorian calendar.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as i64, d as i64)
}

fn json_value_to_variant(v: &serde_json::Value) -> Variant {
    match v {
        serde_json::Value::Null => Variant::Nil,
        serde_json::Value::Bool(b) => Variant::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Variant::Int(i)
            } else if let Some(f) = n.as_f64() {
                Variant::Float(f)
            } else {
                Variant::Nil
            }
        }
        serde_json::Value::String(s) => Variant::String(s.clone()),
        serde_json::Value::Array(arr) => {
            Variant::Array(arr.iter().map(json_value_to_variant).collect())
        }
        serde_json::Value::Object(map) => {
            let mut dict = Dictionary::new();
            for (k, val) in map {
                dict.insert(k.clone(), json_value_to_variant(val));
            }
            Variant::Dictionary(dict)
        }
    }
}

fn variant_to_json_value(v: &Variant) -> serde_json::Value {
    match v {
        Variant::Nil => serde_json::Value::Null,
        Variant::Bool(b) => serde_json::Value::Bool(*b),
        Variant::Int(i) => serde_json::json!(*i),
        Variant::Float(f) => serde_json::json!(*f),
        Variant::String(s) => serde_json::Value::String(s.clone()),
        Variant::Array(items) => {
            serde_json::Value::Array(items.iter().map(variant_to_json_value).collect())
        }
        Variant::Dictionary(map) => {
            let mut obj = serde_json::Map::new();
            for (k, val) in map.iter() {
                // JSON object keys are strings; stringify non-string Variant keys.
                let key = match k {
                    Variant::String(s) => s.clone(),
                    other => other.to_string_lossy(),
                };
                obj.insert(key, variant_to_json_value(val));
            }
            serde_json::Value::Object(obj)
        }
        other => serde_json::Value::String(format!("{other:?}")),
    }
}

fn call_json_static(method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    match method {
        "parse" | "parse_string" => {
            if args.len() != 1 {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "JSON.parse() takes 1 argument".into(),
                )));
            }
            let text = arg_as_string(args, 0, "JSON.parse")?;
            match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(val) => Ok(json_value_to_variant(&val)),
                Err(e) => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                    "JSON.parse error: {e}"
                )))),
            }
        }
        "stringify" => {
            if args.is_empty() || args.len() > 2 {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "JSON.stringify() takes 1 or 2 arguments".into(),
                )));
            }
            let value = variant_to_json_value(&args[0]);
            match serde_json::to_string(&value) {
                Ok(s) => Ok(Variant::String(s)),
                Err(e) => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                    "JSON.stringify error: {e}"
                )))),
            }
        }
        other => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("JSON.{other}"),
        ))),
    }
}

fn call_config_file_instance(
    id: gdcore::ObjectId,
    method: &str,
    args: &[Variant],
) -> Result<Variant, RuntimeError> {
    use gdresource::ConfigFile;
    match method {
        "set_value" => {
            if args.len() != 3 {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "ConfigFile.set_value() takes 3 arguments".into(),
                )));
            }
            let section = arg_as_string(args, 0, "set_value")?;
            let key = arg_as_string(args, 1, "set_value")?;
            let value = args[2].clone();
            ConfigFile::with_handle_mut(id, |cfg| {
                cfg.set_value(&section, &key, value);
                Ok(Variant::Nil)
            })
            .map_err(engine_err_to_runtime)
        }
        "get_value" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "ConfigFile.get_value() takes 2 or 3 arguments".into(),
                )));
            }
            let section = arg_as_string(args, 0, "get_value")?;
            let key = arg_as_string(args, 1, "get_value")?;
            let default = args.get(2).cloned().unwrap_or(Variant::Nil);
            ConfigFile::with_handle_mut(id, |cfg| {
                Ok(cfg
                    .get_value(&section, &key)
                    .cloned()
                    .unwrap_or(default.clone()))
            })
            .map_err(engine_err_to_runtime)
        }
        "has_section" => {
            let section = arg_as_string(args, 0, "has_section")?;
            ConfigFile::with_handle_mut(id, |cfg| Ok(Variant::Bool(cfg.has_section(&section))))
                .map_err(engine_err_to_runtime)
        }
        "has_section_key" => {
            if args.len() != 2 {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "ConfigFile.has_section_key() takes 2 arguments".into(),
                )));
            }
            let section = arg_as_string(args, 0, "has_section_key")?;
            let key = arg_as_string(args, 1, "has_section_key")?;
            ConfigFile::with_handle_mut(id, |cfg| {
                Ok(Variant::Bool(cfg.has_section_key(&section, &key)))
            })
            .map_err(engine_err_to_runtime)
        }
        "get_sections" => ConfigFile::with_handle_mut(id, |cfg| {
            let arr = cfg
                .get_sections()
                .into_iter()
                .map(|s| Variant::String(s.to_string()))
                .collect();
            Ok(Variant::Array(arr))
        })
        .map_err(engine_err_to_runtime),
        "get_section_keys" => {
            let section = arg_as_string(args, 0, "get_section_keys")?;
            ConfigFile::with_handle_mut(id, |cfg| {
                let arr = cfg
                    .get_section_keys(&section)
                    .into_iter()
                    .map(|s| Variant::String(s.to_string()))
                    .collect();
                Ok(Variant::Array(arr))
            })
            .map_err(engine_err_to_runtime)
        }
        "clear" => ConfigFile::with_handle_mut(id, |cfg| {
            cfg.clear();
            Ok(Variant::Nil)
        })
        .map_err(engine_err_to_runtime),
        "save" => {
            // Save to a path via FileAccess when a path is given; otherwise return text.
            if args.len() != 1 {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "ConfigFile.save() takes 1 argument".into(),
                )));
            }
            let path = arg_as_string(args, 0, "save")?;
            let text = ConfigFile::with_handle_mut(id, |cfg| Ok(cfg.save_to_string()))
                .map_err(engine_err_to_runtime)?;
            let mut fa = gdcore::FileAccess::open(&path, gdcore::FileMode::Write)
                .map_err(engine_err_to_runtime)?;
            fa.store_string(&text).map_err(engine_err_to_runtime)?;
            Ok(Variant::Int(0)) // OK
        }
        "load" => {
            if args.len() != 1 {
                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                    "ConfigFile.load() takes 1 argument".into(),
                )));
            }
            let path = arg_as_string(args, 0, "load")?;
            let mut fa = gdcore::FileAccess::open(&path, gdcore::FileMode::Read)
                .map_err(engine_err_to_runtime)?;
            let text = fa.get_as_text().map_err(engine_err_to_runtime)?;
            ConfigFile::with_handle_mut(id, |cfg| {
                let loaded = ConfigFile::load_from_string(&text)
                    .map_err(|e| gdcore::EngineError::Parse(format!("ConfigFile.load: {e}")))?;
                *cfg = loaded;
                Ok(Variant::Int(0))
            })
            .map_err(engine_err_to_runtime)
        }
        other => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("ConfigFile.{other}"),
        ))),
    }
}

/// Returns `true` if the identifier is a known Godot node class name that
/// supports `.new()` for runtime instantiation.
/// Resolves the `(target_object, method_name)` a signal connection should
/// store from a `Callable` argument (pat-0q2kx.9).
///
/// `Callable(self, "m")` records `target_id == 0` meaning "the calling object",
/// so an explicit 0 resolves against `self_node`. A plain String/StringName is
/// also accepted: Patina scripts predate first-class Callables, and Godot 3's
/// `connect(sig, obj, "method")` form still reaches here through the
/// object-level `connect` arm.
fn callable_target(v: &Variant, self_node: Option<u64>) -> Option<(u64, String)> {
    match v {
        Variant::Callable(c) => match c.inner_callable() {
            CallableRef::Method { target_id, method } => {
                let target = if *target_id == 0 {
                    self_node?
                } else {
                    *target_id
                };
                Some((target, method.clone()))
            }
            // A lambda has no bindable target/method name to store in the
            // SignalStore, so it cannot be connected here.
            _ => None,
        },
        Variant::String(s) => Some((self_node?, s.clone())),
        Variant::StringName(s) => Some((self_node?, s.as_str().to_string())),
        _ => None,
    }
}

fn is_node_class_name(name: &str) -> bool {
    matches!(
        name,
        "Node"
            | "Node2D"
            | "Node3D"
            | "Control"
            | "Sprite2D"
            | "Sprite3D"
            | "Area2D"
            | "Area3D"
            | "CharacterBody2D"
            | "CharacterBody3D"
            | "RigidBody2D"
            | "RigidBody3D"
            | "StaticBody2D"
            | "StaticBody3D"
            | "CollisionShape2D"
            | "CollisionShape3D"
            | "Timer"
            | "Camera2D"
            | "Camera3D"
            | "Label"
            | "Button"
            | "TextureRect"
            | "AudioStreamPlayer"
            | "AnimationPlayer"
            | "CanvasLayer"
            | "ColorRect"
    )
}

fn variant_eq(a: &Variant, b: &Variant) -> bool {
    match (a, b) {
        (Variant::Nil, Variant::Nil) => true,
        (Variant::Bool(a), Variant::Bool(b)) => a == b,
        (Variant::Int(a), Variant::Int(b)) => a == b,
        (Variant::Float(a), Variant::Float(b)) => a == b,
        (Variant::Int(a), Variant::Float(b)) => (*a as f64) == *b,
        (Variant::Float(a), Variant::Int(b)) => *a == (*b as f64),
        (Variant::String(a), Variant::String(b)) => a == b,
        (Variant::Vector2(a), Variant::Vector2(b)) => a == b,
        (Variant::Vector3(a), Variant::Vector3(b)) => a == b,
        (Variant::Color(a), Variant::Color(b)) => a == b,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// GDScriptInstance — ScriptInstance adapter
// ---------------------------------------------------------------------------

/// A `ScriptInstance` implementation backed by the tree-walk interpreter.
pub struct GDScriptInstance {
    interpreter: Interpreter,
    script_name: String,
    /// Exported-variable hint metadata (`@export*` annotations), surfaced via
    /// [`list_properties`](ScriptInstance::list_properties) so the editor
    /// inspector can render the right property editor (pat-nzgdu.5).
    exports: Vec<ExportInfo>,
}

impl GDScriptInstance {
    /// Creates a new `GDScriptInstance` by parsing and executing the given
    /// source to register its top-level function definitions and variables.
    pub fn from_source(name: &str, source: &str) -> Result<Self, RuntimeError> {
        let mut interpreter = Interpreter::new();
        interpreter.run(source)?;
        // Collect the script's `@export` hint metadata via a throwaway pass:
        // `run_class` extracts the class structure (vars/exports) without
        // disturbing the interpreter executed above. Empty for scripts with no
        // class-level exports (pat-nzgdu.5).
        let exports = Interpreter::new()
            .run_class(source)
            .map(|c| c.exports)
            .unwrap_or_default();
        Ok(Self {
            interpreter,
            script_name: name.to_string(),
            exports,
        })
    }
}

impl ScriptInstance for GDScriptInstance {
    fn call_method(&mut self, name: &str, args: &[Variant]) -> Result<Variant, ScriptError> {
        self.interpreter
            .call_user_func(name, args)
            .map_err(|e| match e.kind {
                RuntimeErrorKind::UndefinedFunction(n) => ScriptError::MethodNotFound(n),
                RuntimeErrorKind::TypeError(msg) => ScriptError::TypeError(msg),
                other => ScriptError::TypeError(other.to_string()),
            })
    }

    fn get_property(&self, name: &str) -> Option<Variant> {
        self.interpreter.environment.get(name).ok()
    }

    fn set_property(&mut self, name: &str, value: Variant) -> bool {
        self.interpreter.environment.set(name, value).is_ok()
    }

    fn list_methods(&self) -> Vec<MethodInfo> {
        self.interpreter
            .function_registry
            .iter()
            .map(|(name, func)| MethodInfo {
                name: name.clone(),
                argument_names: func.params.clone(),
                return_type: gdvariant::variant::VariantType::Nil,
                flags: MethodFlags::NORMAL,
            })
            .collect()
    }

    fn list_properties(&self) -> Vec<ScriptPropertyInfo> {
        let scope = &self.interpreter.environment.scopes[0];
        scope
            .iter()
            .map(|(name, val)| {
                // Attach `@export` hint metadata (pat-nzgdu.5) when this
                // property was exported, so the inspector renders the right
                // editor; non-exported vars report PROPERTY_HINT_NONE.
                let (hint, hint_string) = self
                    .exports
                    .iter()
                    .find(|e| &e.name == name)
                    .map(|e| (e.hint, e.hint_string.clone()))
                    .unwrap_or((PROPERTY_HINT_NONE, String::new()));
                ScriptPropertyInfo {
                    name: name.clone(),
                    property_type: val.variant_type(),
                    default_value: val.clone(),
                    hint,
                    hint_string,
                }
            })
            .collect()
    }

    fn get_script_name(&self) -> &str {
        &self.script_name
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::{Color, Vector2, Vector3};

    fn run(src: &str) -> InterpreterResult {
        let mut interp = Interpreter::new();
        interp.run(src).unwrap()
    }

    fn run_val(src: &str) -> Variant {
        run(src).return_value.unwrap_or(Variant::Nil)
    }

    fn run_err(src: &str) -> RuntimeError {
        let mut interp = Interpreter::new();
        interp.run(src).unwrap_err()
    }

    // -- Arithmetic ---------------------------------------------------------

    #[test]
    fn add_integers() {
        let r = run("return 1 + 2\n");
        assert_eq!(r.return_value, Some(Variant::Int(3)));
    }

    #[test]
    fn multiply_integers() {
        assert_eq!(run_val("return 3 * 4\n"), Variant::Int(12));
    }

    #[test]
    fn integer_division() {
        assert_eq!(run_val("return 10 / 3\n"), Variant::Int(3));
    }

    #[test]
    fn is_same_requires_exact_type_and_value() {
        // Value types: exact type + value (unlike `==`, no int/float coercion).
        assert_eq!(run_val("return is_same(1, 1)\n"), Variant::Bool(true));
        assert_eq!(run_val("return is_same(1, 1.0)\n"), Variant::Bool(false));
        assert_eq!(run_val("return is_same(true, 1)\n"), Variant::Bool(false));
        assert_eq!(
            run_val("return is_same(\"a\", \"a\")\n"),
            Variant::Bool(true)
        );
        assert_eq!(
            run_val("return is_same(\"a\", \"b\")\n"),
            Variant::Bool(false)
        );
    }

    #[test]
    fn negation() {
        assert_eq!(run_val("return -5\n"), Variant::Int(-5));
    }

    #[test]
    fn operator_precedence() {
        // 2 + 3 * 4 = 14
        assert_eq!(run_val("return 2 + 3 * 4\n"), Variant::Int(14));
    }

    #[test]
    fn parenthesized_precedence() {
        // (2 + 3) * 4 = 20
        assert_eq!(run_val("return (2 + 3) * 4\n"), Variant::Int(20));
    }

    // -- ShaderMaterial.set/get_shader_parameter (pat-u3l8c.43) -------------

    #[test]
    fn shader_material_set_get_parameter_roundtrips() {
        // A ShaderMaterial resource variable: setting a parameter then reading
        // it back yields the stored value (persisted via the lvalue write-back).
        let v = run_val(
            "var mat = preload(\"res://water.tres\")\n\
             mat.set_shader_parameter(\"wave_height\", 0.5)\n\
             return mat.get_shader_parameter(\"wave_height\")\n",
        );
        assert_eq!(v, Variant::Float(0.5));
    }

    #[test]
    fn shader_parameter_stored_under_prefixed_key() {
        // The value is stored under `shader_parameter/<name>` so it matches the
        // tscn/tres representation the render server consumes.
        let v = run_val(
            "var mat = preload(\"res://water.tres\")\n\
             mat.set_shader_parameter(\"tint\", 3)\n\
             return mat.get(\"shader_parameter/tint\")\n",
        );
        assert_eq!(v, Variant::Int(3));
    }

    #[test]
    fn get_shader_parameter_missing_returns_nil() {
        let v = run_val(
            "var mat = preload(\"res://water.tres\")\n\
             return mat.get_shader_parameter(\"nope\")\n",
        );
        assert_eq!(v, Variant::Nil);
    }

    #[test]
    fn preloaded_gdshader_is_shader_class() {
        let v = run_val(
            "var s = preload(\"res://outline.gdshader\")\n\
             return s.get_class()\n",
        );
        assert_eq!(v, Variant::String("Shader".into()));
    }

    #[test]
    fn shader_material_set_get_shader_roundtrips() {
        let v = run_val(
            "var mat = preload(\"res://water.tres\")\n\
             mat.set_shader(preload(\"res://water.gdshader\"))\n\
             return mat.get_shader().get_class()\n",
        );
        assert_eq!(v, Variant::String("Shader".into()));
    }

    #[test]
    fn modulo() {
        assert_eq!(run_val("return 10 % 3\n"), Variant::Int(1));
    }

    // -- Variables ----------------------------------------------------------

    #[test]
    fn var_declaration_and_use() {
        let r = run("var x = 5\nreturn x\n");
        assert_eq!(r.return_value, Some(Variant::Int(5)));
    }

    #[test]
    fn var_reassignment() {
        let src = "var x = 5\nx = x + 1\nreturn x\n";
        assert_eq!(run_val(src), Variant::Int(6));
    }

    #[test]
    fn plus_assign() {
        let src = "var x = 10\nx += 5\nreturn x\n";
        assert_eq!(run_val(src), Variant::Int(15));
    }

    #[test]
    fn minus_assign() {
        let src = "var x = 10\nx -= 3\nreturn x\n";
        assert_eq!(run_val(src), Variant::Int(7));
    }

    // -- Strings ------------------------------------------------------------

    #[test]
    fn string_concatenation() {
        assert_eq!(
            run_val("return \"hello\" + \" world\"\n"),
            Variant::String("hello world".into())
        );
    }

    #[test]
    fn string_len() {
        assert_eq!(run_val("return len(\"abc\")\n"), Variant::Int(3));
    }

    #[test]
    fn string_indexing() {
        assert_eq!(
            run_val("return \"hello\"[1]\n"),
            Variant::String("e".into())
        );
    }

    // -- If / elif / else ---------------------------------------------------

    #[test]
    fn if_true_branch() {
        let src = "\
var r = 0
if true:
    r = 1
return r
";
        assert_eq!(run_val(src), Variant::Int(1));
    }

    #[test]
    fn if_false_else() {
        let src = "\
var r = 0
if false:
    r = 1
else:
    r = 2
return r
";
        assert_eq!(run_val(src), Variant::Int(2));
    }

    #[test]
    fn if_elif_else() {
        let src = "\
var x = 5
var r = 0
if x == 1:
    r = 10
elif x == 5:
    r = 50
else:
    r = 99
return r
";
        assert_eq!(run_val(src), Variant::Int(50));
    }

    // -- While loop ---------------------------------------------------------

    #[test]
    fn while_sum_1_to_10() {
        let src = "\
var sum = 0
var i = 1
while i <= 10:
    sum += i
    i += 1
return sum
";
        assert_eq!(run_val(src), Variant::Int(55));
    }

    // -- For loop -----------------------------------------------------------

    #[test]
    fn for_range() {
        let src = "\
var sum = 0
for i in range(5):
    sum += i
return sum
";
        assert_eq!(run_val(src), Variant::Int(10)); // 0+1+2+3+4
    }

    #[test]
    fn for_in_int_iterates_range() {
        // pat-q7048: `for i in <int>` iterates 0..int (Godot semantics),
        // equivalent to `for i in range(int)`.
        let sum = "\
var sum = 0
for i in 5:
    sum += i
return sum
";
        assert_eq!(run_val(sum), Variant::Int(10)); // 0+1+2+3+4

        // Confirm the exact 0..n sequence.
        let seq = "\
var out = []
for i in 4:
    out.append(i)
return out
";
        assert_eq!(
            run_val(seq),
            Variant::Array(vec![
                Variant::Int(0),
                Variant::Int(1),
                Variant::Int(2),
                Variant::Int(3),
            ])
        );

        // Non-positive int -> empty iteration (body never runs).
        let empty = "\
var count = 0
for i in 0:
    count += 1
for i in -3:
    count += 1
return count
";
        assert_eq!(run_val(empty), Variant::Int(0));
    }

    #[test]
    fn for_range_start_end() {
        let src = "\
var sum = 0
for i in range(3, 6):
    sum += i
return sum
";
        assert_eq!(run_val(src), Variant::Int(12)); // 3+4+5
    }

    #[test]
    fn for_over_array() {
        let src = "\
var arr = [10, 20, 30]
var sum = 0
for x in arr:
    sum += x
return sum
";
        assert_eq!(run_val(src), Variant::Int(60));
    }

    // -- Callable.bind/unbind + ConnectFlags constants (pat-u3l8c.17) --------

    #[test]
    fn callable_bind_appends_argument() {
        // `.bind(10)` appends 10 after the call-site arg, so add(5, 10) == 15.
        let src = "\
func add(a, b):
    return a + b
return Callable(null, \"add\").bind(10).call(5)
";
        assert_eq!(run_val(src), Variant::Int(15));
    }

    #[test]
    fn callable_bind_multiple_arguments() {
        // `.bind(2, 3)` appends both, so addv(1, 2, 3) == 6.
        let src = "\
func addv(a, b, c):
    return a + b + c
return Callable(null, \"addv\").bind(2, 3).call(1)
";
        assert_eq!(run_val(src), Variant::Int(6));
    }

    #[test]
    fn callable_unbind_drops_trailing_argument() {
        // `.unbind(1)` drops the last call-site arg, so the extra 999 never
        // reaches echo1 and the call returns its single argument unchanged.
        let src = "\
func echo1(a):
    return a
return Callable(null, \"echo1\").unbind(1).call(42, 999)
";
        assert_eq!(run_val(src), Variant::Int(42));
    }

    #[test]
    fn connect_flag_constants_resolve() {
        // Godot Object.ConnectFlags are exposed as @GlobalScope barewords.
        assert_eq!(run_val("return CONNECT_DEFERRED\n"), Variant::Int(1));
        assert_eq!(run_val("return CONNECT_PERSIST\n"), Variant::Int(2));
        assert_eq!(run_val("return CONNECT_ONE_SHOT\n"), Variant::Int(4));
        assert_eq!(
            run_val("return CONNECT_REFERENCE_COUNTED\n"),
            Variant::Int(8)
        );
    }

    // -- Functions ----------------------------------------------------------

    #[test]
    fn simple_function() {
        let src = "\
func add(a, b):
    return a + b
return add(3, 4)
";
        assert_eq!(run_val(src), Variant::Int(7));
    }

    #[test]
    fn function_no_return() {
        let src = "\
func greet():
    pass
return greet()
";
        assert_eq!(run_val(src), Variant::Nil);
    }

    // -- Recursion ----------------------------------------------------------

    #[test]
    fn recursive_factorial() {
        let src = "\
func factorial(n):
    if n <= 1:
        return 1
    return n * factorial(n - 1)
return factorial(5)
";
        assert_eq!(run_val(src), Variant::Int(120));
    }

    #[test]
    fn max_recursion_depth() {
        let src = "\
func inf(n):
    return inf(n + 1)
inf(0)
";
        let err = run_err(src);
        assert!(matches!(err.kind, RuntimeErrorKind::MaxRecursionDepth(_)));
    }

    // -- Arrays -------------------------------------------------------------

    #[test]
    fn array_literal_and_index() {
        assert_eq!(run_val("return [1, 2, 3][1]\n"), Variant::Int(2));
    }

    #[test]
    fn array_push_back() {
        let src = "\
var a = [1, 2]
a.push_back(3)
return len(a)
";
        assert_eq!(run_val(src), Variant::Int(3));
    }

    #[test]
    fn array_negative_index() {
        assert_eq!(run_val("return [10, 20, 30][-1]\n"), Variant::Int(30));
    }

    // -- Dicts --------------------------------------------------------------

    #[test]
    fn dict_literal_and_access() {
        let src = "\
var d = {\"x\": 1, \"y\": 2}
return d[\"x\"]
";
        assert_eq!(run_val(src), Variant::Int(1));
    }

    #[test]
    fn dict_member_access() {
        let src = "\
var d = {\"name\": \"Alice\"}
return d.name
";
        assert_eq!(run_val(src), Variant::String("Alice".into()));
    }

    #[test]
    fn dict_assignment() {
        let src = "\
var d = {\"a\": 1}
d[\"b\"] = 2
return d[\"b\"]
";
        assert_eq!(run_val(src), Variant::Int(2));
    }

    // -- Type coercion ------------------------------------------------------

    #[test]
    fn int_plus_float() {
        assert_eq!(run_val("return 1 + 2.5\n"), Variant::Float(3.5));
    }

    #[test]
    fn float_plus_int() {
        assert_eq!(run_val("return 1.5 + 1\n"), Variant::Float(2.5));
    }

    // -- Built-ins ----------------------------------------------------------

    #[test]
    fn builtin_print() {
        let r = run("print(\"hello\")\nprint(42)\n");
        assert_eq!(r.output, vec!["hello", "42"]);
    }

    #[test]
    fn builtin_print_multiple_args() {
        let r = run("print(\"a\", \"b\", \"c\")\n");
        assert_eq!(r.output, vec!["a b c"]);
    }

    #[test]
    fn builtin_str() {
        assert_eq!(run_val("return str(42)\n"), Variant::String("42".into()));
    }

    #[test]
    fn builtin_int_from_float() {
        assert_eq!(run_val("return int(3.7)\n"), Variant::Int(3));
    }

    #[test]
    fn builtin_float_from_int() {
        assert_eq!(run_val("return float(5)\n"), Variant::Float(5.0));
    }

    #[test]
    fn builtin_len_array() {
        assert_eq!(run_val("return len([1, 2, 3])\n"), Variant::Int(3));
    }

    #[test]
    fn builtin_range_single() {
        assert_eq!(
            run_val("return range(3)\n"),
            Variant::Array(vec![Variant::Int(0), Variant::Int(1), Variant::Int(2)])
        );
    }

    #[test]
    fn builtin_typeof() {
        // Godot `typeof()` returns the `Variant.Type` integer, matching the
        // `TYPE_*` constants: TYPE_INT == 2, TYPE_STRING == 4. (pat-nzgdu.7)
        assert_eq!(run_val("return typeof(42)\n"), Variant::Int(2));
        assert_eq!(run_val("return typeof(\"hi\")\n"), Variant::Int(4));
        assert_eq!(
            run_val("return typeof(42) == TYPE_INT\n"),
            Variant::Bool(true)
        );
        assert_eq!(
            run_val("return typeof(3.14) == TYPE_FLOAT\n"),
            Variant::Bool(true)
        );
    }

    // -- pat-u3l8c.20 builtins: print/error family, serialization, utility --

    /// Runs `src` and returns the interpreter so tests can inspect `errors()`.
    fn run_interp(src: &str) -> Interpreter {
        let mut interp = Interpreter::new();
        interp.run(src).unwrap();
        interp
    }

    #[test]
    fn builtin_prints_space_separated() {
        let r = run("prints(\"a\", \"b\", \"c\")\n");
        assert_eq!(r.output, vec!["a b c"]);
    }

    #[test]
    fn builtin_printt_tab_separated() {
        let r = run("printt(\"a\", \"b\")\n");
        assert_eq!(r.output, vec!["a\tb"]);
    }

    #[test]
    fn builtin_push_error_surfaces_line() {
        let interp = run_interp("push_error(\"boom\")\n");
        assert_eq!(interp.errors().len(), 1);
        let entry = &interp.errors()[0];
        assert!(entry.contains("boom"), "message missing: {entry}");
        assert!(entry.contains("at:"), "source location missing: {entry}");
        // A line number is appended after the final ':'.
        let line_num = entry.rsplit(':').next().unwrap().trim();
        assert!(
            line_num.parse::<i64>().is_ok(),
            "line number not present: {entry}"
        );
    }

    #[test]
    fn builtin_printerr_routes_to_error_channel() {
        let interp = run_interp("printerr(\"oops\", 42)\n");
        assert_eq!(interp.errors().len(), 1);
        assert_eq!(interp.errors()[0], "oops42");
    }

    #[test]
    fn builtin_print_rich_strips_bbcode() {
        let r = run("print_rich(\"[b]bold[/b] text\")\n");
        assert_eq!(r.output, vec!["bold text"]);
    }

    #[test]
    fn builtin_var_to_str_str_to_var_roundtrip() {
        let mut d = Dictionary::new();
        d.insert(Variant::String("a".into()), Variant::Int(1));
        d.insert(Variant::String("b".into()), Variant::Float(2.5));
        let cases = vec![
            Variant::Int(42),
            Variant::Int(-7),
            Variant::Float(3.5),
            Variant::String("hello world".into()),
            Variant::Bool(true),
            Variant::Bool(false),
            Variant::Nil,
            Variant::Vector2(Vector2::new(1.0, 2.0)),
            Variant::Array(vec![
                Variant::Int(1),
                Variant::String("x".into()),
                Variant::Bool(false),
            ]),
            Variant::Dictionary(d),
        ];
        for v in cases {
            let text = variant_to_gd_string(&v);
            let back = gd_string_to_variant(&text);
            assert_eq!(back, v, "round-trip failed for {v:?} via {text:?}");
        }
    }

    #[test]
    fn builtin_var_to_bytes_bytes_to_var_roundtrip() {
        // Exercise via GDScript so the builtins (not just the codec) are covered.
        assert_eq!(
            run_val("return bytes_to_var(var_to_bytes(42))\n"),
            Variant::Int(42)
        );
        assert_eq!(
            run_val("return bytes_to_var(var_to_bytes(\"hi\"))\n"),
            Variant::String("hi".into())
        );
        assert_eq!(
            run_val("return bytes_to_var(var_to_bytes([1, 2, 3]))\n"),
            Variant::Array(vec![Variant::Int(1), Variant::Int(2), Variant::Int(3)])
        );
        // var_to_bytes yields a PackedByteArray.
        assert!(matches!(
            run_val("return var_to_bytes(7)\n"),
            Variant::PackedByteArray(_)
        ));
    }

    #[test]
    fn builtin_type_string_names() {
        assert_eq!(
            run_val("return type_string(2)\n"),
            Variant::String("int".into())
        );
        assert_eq!(
            run_val("return type_string(4)\n"),
            Variant::String("String".into())
        );
        assert_eq!(
            run_val("return type_string(5)\n"),
            Variant::String("Vector2".into())
        );
        assert_eq!(
            run_val("return type_string(29)\n"),
            Variant::String("PackedByteArray".into())
        );
        // Unknown codes map to the empty string (matching Godot).
        assert_eq!(
            run_val("return type_string(999)\n"),
            Variant::String(String::new())
        );
    }

    #[test]
    fn builtin_color8() {
        assert_eq!(
            run_val("return Color8(255, 0, 0)\n"),
            Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0))
        );
        assert_eq!(
            run_val("return Color8(0, 255, 0, 128)\n"),
            Variant::Color(Color::new(0.0, 1.0, 0.0, 128.0 / 255.0))
        );
    }

    #[test]
    fn builtin_char_ord_roundtrip() {
        assert_eq!(run_val("return char(65)\n"), Variant::String("A".into()));
        assert_eq!(run_val("return ord(\"A\")\n"), Variant::Int(65));
        assert_eq!(run_val("return ord(char(97))\n"), Variant::Int(97));
        // Invalid code point yields an empty string.
        assert_eq!(run_val("return char(-1)\n"), Variant::String(String::new()));
    }

    #[test]
    fn builtin_hash_deterministic() {
        assert_eq!(
            run_val("return hash(\"abc\") == hash(\"abc\")\n"),
            Variant::Bool(true)
        );
        assert_eq!(
            run_val("return hash(12345) == hash(12345)\n"),
            Variant::Bool(true)
        );
        // Distinct values (overwhelmingly likely) hash differently.
        assert_eq!(
            run_val("return hash(\"abc\") == hash(\"abd\")\n"),
            Variant::Bool(false)
        );
        // Direct helper determinism.
        assert_eq!(
            variant_hash(&Variant::Int(42)),
            variant_hash(&Variant::Int(42))
        );
    }

    #[test]
    fn builtin_is_instance_of_type_code_and_name() {
        // Int type code (TYPE_INT == 2).
        assert_eq!(
            run_val("return is_instance_of(5, TYPE_INT)\n"),
            Variant::Bool(true)
        );
        assert_eq!(
            run_val("return is_instance_of(5, TYPE_STRING)\n"),
            Variant::Bool(false)
        );
        // Type name string.
        assert_eq!(
            run_val("return is_instance_of(\"hi\", \"String\")\n"),
            Variant::Bool(true)
        );
    }

    // -- Comparison / boolean -----------------------------------------------

    #[test]
    fn comparison_operators() {
        assert_eq!(run_val("return 1 < 2\n"), Variant::Bool(true));
        assert_eq!(run_val("return 2 > 3\n"), Variant::Bool(false));
        assert_eq!(run_val("return 5 == 5\n"), Variant::Bool(true));
        assert_eq!(run_val("return 5 != 5\n"), Variant::Bool(false));
        assert_eq!(run_val("return 3 <= 3\n"), Variant::Bool(true));
        assert_eq!(run_val("return 3 >= 4\n"), Variant::Bool(false));
    }

    #[test]
    fn logical_and_or() {
        assert_eq!(run_val("return true and false\n"), Variant::Bool(false));
        assert_eq!(run_val("return false or true\n"), Variant::Bool(true));
    }

    #[test]
    fn not_operator() {
        assert_eq!(run_val("return not true\n"), Variant::Bool(false));
        assert_eq!(run_val("return not false\n"), Variant::Bool(true));
    }

    // -- Error cases --------------------------------------------------------

    #[test]
    fn undefined_variable_error() {
        let err = run_err("return x\n");
        assert!(matches!(err.kind, RuntimeErrorKind::UndefinedVariable(_)));
    }

    #[test]
    fn division_by_zero_error() {
        let err = run_err("return 1 / 0\n");
        assert!(matches!(err.kind, RuntimeErrorKind::DivisionByZero));
    }

    #[test]
    fn type_error_add_int_bool() {
        let err = run_err("return 1 + true\n");
        assert!(matches!(err.kind, RuntimeErrorKind::TypeError(_)));
    }

    #[test]
    fn undefined_function_error() {
        let err = run_err("foo()\n");
        assert!(matches!(err.kind, RuntimeErrorKind::UndefinedFunction(_)));
    }

    #[test]
    fn index_out_of_bounds_error() {
        let err = run_err("return [1, 2][5]\n");
        assert!(matches!(
            err.kind,
            RuntimeErrorKind::IndexOutOfBounds { .. }
        ));
    }

    // -- GDScriptInstance (ScriptInstance trait) ----------------------------

    #[test]
    fn gdscript_instance_call_method() {
        let src = "\
var health = 100
func take_damage(amount):
    health -= amount
    return health
";
        let mut inst = GDScriptInstance::from_source("Player", src).unwrap();
        let result = inst
            .call_method("take_damage", &[Variant::Int(30)])
            .unwrap();
        assert_eq!(result, Variant::Int(70));
    }

    #[test]
    fn gdscript_instance_properties() {
        let src = "var x = 42\nvar name = \"test\"\n";
        let inst = GDScriptInstance::from_source("Test", src).unwrap();
        assert_eq!(inst.get_property("x"), Some(Variant::Int(42)));
        assert_eq!(
            inst.get_property("name"),
            Some(Variant::String("test".into()))
        );
        assert_eq!(inst.get_property("missing"), None);
    }

    #[test]
    fn gdscript_instance_set_property() {
        let src = "var x = 0\n";
        let mut inst = GDScriptInstance::from_source("Test", src).unwrap();
        assert!(inst.set_property("x", Variant::Int(99)));
        assert_eq!(inst.get_property("x"), Some(Variant::Int(99)));
        assert!(!inst.set_property("nonexistent", Variant::Int(1)));
    }

    #[test]
    fn gdscript_instance_list_methods() {
        let src = "\
func foo():
    pass
func bar(a, b):
    return a + b
";
        let inst = GDScriptInstance::from_source("Test", src).unwrap();
        let methods = inst.list_methods();
        let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"foo"));
        assert!(names.contains(&"bar"));
    }

    #[test]
    fn gdscript_instance_script_name() {
        let inst = GDScriptInstance::from_source("MyScript", "pass\n").unwrap();
        assert_eq!(inst.get_script_name(), "MyScript");
    }

    #[test]
    fn gdscript_instance_method_not_found() {
        let mut inst = GDScriptInstance::from_source("Test", "pass\n").unwrap();
        let err = inst.call_method("nonexistent", &[]).unwrap_err();
        assert!(matches!(err, ScriptError::MethodNotFound(_)));
    }

    // -- Break / Continue ---------------------------------------------------

    #[test]
    fn while_break() {
        let src = "\
var i = 0
while true:
    if i == 5:
        break
    i += 1
return i
";
        assert_eq!(run_val(src), Variant::Int(5));
    }

    #[test]
    fn for_continue() {
        let src = "\
var sum = 0
for i in range(10):
    if i % 2 == 0:
        continue
    sum += i
return sum
";
        // 1+3+5+7+9 = 25
        assert_eq!(run_val(src), Variant::Int(25));
    }

    // -- In operator --------------------------------------------------------

    #[test]
    fn in_array() {
        assert_eq!(run_val("return 2 in [1, 2, 3]\n"), Variant::Bool(true));
        assert_eq!(run_val("return 5 in [1, 2, 3]\n"), Variant::Bool(false));
    }

    #[test]
    fn in_dict() {
        let src = "\
var d = {\"a\": 1}
return \"a\" in d
";
        assert_eq!(run_val(src), Variant::Bool(true));
    }

    // -- Nested function calls ----------------------------------------------

    #[test]
    fn nested_function_calls() {
        let src = "\
func double(x):
    return x * 2
func add_one(x):
    return x + 1
return add_one(double(5))
";
        assert_eq!(run_val(src), Variant::Int(11));
    }

    // -- Fibonacci ----------------------------------------------------------

    #[test]
    fn fibonacci() {
        let src = "\
func fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)
return fib(10)
";
        assert_eq!(run_val(src), Variant::Int(55));
    }

    // -- Bool literal in expression -----------------------------------------

    #[test]
    fn bool_equality() {
        assert_eq!(run_val("return true == true\n"), Variant::Bool(true));
        assert_eq!(run_val("return true == false\n"), Variant::Bool(false));
    }

    // -- Null ---------------------------------------------------------------

    #[test]
    fn null_is_falsy() {
        assert_eq!(run_val("return not null\n"), Variant::Bool(true));
    }

    // -- Class system -------------------------------------------------------

    fn run_class(src: &str) -> (Interpreter, ClassDef) {
        let mut interp = Interpreter::new();
        let class_def = interp.run_class(src).unwrap();
        (interp, class_def)
    }

    #[test]
    fn class_extends_parsing() {
        let (_, class_def) = run_class("extends Node\nclass_name Player\n");
        assert_eq!(class_def.parent_class.as_deref(), Some("Node"));
    }

    #[test]
    fn class_extends_string() {
        let (_, class_def) = run_class("extends \"Node2D\"\n");
        assert_eq!(class_def.parent_class.as_deref(), Some("Node2D"));
    }

    #[test]
    fn class_name_parsing() {
        let (_, class_def) = run_class("class_name Player\n");
        assert_eq!(class_def.name.as_deref(), Some("Player"));
    }

    #[test]
    fn class_signal_declaration() {
        let (_, class_def) = run_class(
            "\
class_name Entity
signal health_changed
signal damage_taken
",
        );
        assert_eq!(class_def.signals, vec!["health_changed", "damage_taken"]);
    }

    #[test]
    fn class_signal_with_params() {
        let (_, class_def) = run_class("signal hit(damage, source)\n");
        assert_eq!(class_def.signals, vec!["hit"]);
    }

    #[test]
    fn class_enum_values() {
        let (_, class_def) = run_class("enum State { IDLE, RUNNING, JUMPING }\n");
        let state_enum = class_def.enums.get("State").unwrap();
        assert_eq!(state_enum.get("IDLE"), Some(&0));
        assert_eq!(state_enum.get("RUNNING"), Some(&1));
        assert_eq!(state_enum.get("JUMPING"), Some(&2));
    }

    #[test]
    fn class_enum_multiple() {
        let (_, class_def) = run_class(
            "\
enum Color { RED, GREEN, BLUE }
enum Direction { UP, DOWN }
",
        );
        assert!(class_def.enums.contains_key("Color"));
        assert!(class_def.enums.contains_key("Direction"));
        assert_eq!(
            class_def.enums.get("Direction").unwrap().get("UP"),
            Some(&0)
        );
    }

    #[test]
    fn class_enum_explicit_values_honored() {
        let (_, class_def) = run_class("enum Bits { A = 1, B = 2, C, D = 10 }\n");
        let e = class_def.enums.get("Bits").unwrap();
        assert_eq!(e.get("A"), Some(&1));
        assert_eq!(e.get("B"), Some(&2));
        // Unspecified variant auto-increments from the previous value.
        assert_eq!(e.get("C"), Some(&3));
        assert_eq!(e.get("D"), Some(&10));
    }

    #[test]
    fn class_anonymous_enum_becomes_constants() {
        let (_, class_def) = run_class("enum { RED, GREEN = 5, BLUE }\n");
        // Anonymous enum variants are registered as script constants with the
        // right values (RED=0, GREEN=5, BLUE=6 — auto-increment after 5).
        let mut found = std::collections::HashMap::new();
        for (name, expr) in &class_def.constants {
            if let Expr::Literal(Variant::Int(v)) = expr {
                found.insert(name.as_str(), *v);
            }
        }
        assert_eq!(found.get("RED"), Some(&0));
        assert_eq!(found.get("GREEN"), Some(&5));
        assert_eq!(found.get("BLUE"), Some(&6));
    }

    #[test]
    fn class_export_var() {
        let (_, class_def) = run_class(
            "\
@export
var speed: float = 100.0
var health: int = 100
",
        );
        assert_eq!(class_def.exports.len(), 1);
        assert_eq!(class_def.exports[0].name, "speed");
        assert_eq!(class_def.exports[0].type_hint.as_deref(), Some("float"));
        assert_eq!(class_def.instance_vars.len(), 2);
    }

    #[test]
    fn class_var_type_hint() {
        let (_, class_def) = run_class("var health: int = 100\n");
        assert_eq!(class_def.instance_vars.len(), 1);
        assert_eq!(class_def.instance_vars[0].name, "health");
        assert_eq!(class_def.instance_vars[0].type_hint.as_deref(), Some("int"));
    }

    #[test]
    fn class_methods() {
        let (_, class_def) = run_class(
            "\
func _ready():
    pass
func _process(delta):
    pass
",
        );
        assert!(class_def.methods.contains_key("_ready"));
        assert!(class_def.methods.contains_key("_process"));
        assert_eq!(class_def.methods["_process"].params, vec!["delta"]);
    }

    #[test]
    fn class_ready_and_process() {
        let (_, class_def) = run_class(
            "\
class_name Player
var health: int = 100

func _ready():
    return health

func _process(delta):
    return delta
",
        );
        assert!(class_def.methods.contains_key("_ready"));
        assert!(class_def.methods.contains_key("_process"));
        assert_eq!(class_def.name.as_deref(), Some("Player"));
    }

    #[test]
    fn class_instance_creation() {
        let (mut interp, class_def) = run_class(
            "\
var health = 100
var speed = 50
",
        );
        let inst = interp.instantiate_class(&class_def).unwrap();
        assert_eq!(inst.properties.get("health"), Some(&Variant::Int(100)));
        assert_eq!(inst.properties.get("speed"), Some(&Variant::Int(50)));
    }

    #[test]
    fn class_instance_default_nil() {
        let (mut interp, class_def) = run_class("var x\n");
        let inst = interp.instantiate_class(&class_def).unwrap();
        assert_eq!(inst.properties.get("x"), Some(&Variant::Nil));
    }

    #[test]
    fn class_method_dispatch() {
        let (mut interp, class_def) = run_class(
            "\
var health = 100
func get_health():
    return health
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let result = interp
            .call_instance_method(&mut inst, "get_health", &[])
            .unwrap();
        assert_eq!(result, Variant::Int(100));
    }

    #[test]
    fn class_method_with_args() {
        let (mut interp, class_def) = run_class(
            "\
var health = 100
func take_damage(amount):
    health -= amount
    return health
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let result = interp
            .call_instance_method(&mut inst, "take_damage", &[Variant::Int(30)])
            .unwrap();
        assert_eq!(result, Variant::Int(70));
    }

    #[test]
    fn class_self_member_access() {
        let (mut interp, class_def) = run_class(
            "\
var health = 100
func get_health():
    return self.health
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let result = interp
            .call_instance_method(&mut inst, "get_health", &[])
            .unwrap();
        assert_eq!(result, Variant::Int(100));
    }

    #[test]
    fn class_self_member_assignment() {
        let (mut interp, class_def) = run_class(
            "\
var health = 100
func set_health(val):
    self.health = val
    return self.health
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let result = interp
            .call_instance_method(&mut inst, "set_health", &[Variant::Int(50)])
            .unwrap();
        assert_eq!(result, Variant::Int(50));
        assert_eq!(inst.properties.get("health"), Some(&Variant::Int(50)));
    }

    #[test]
    fn class_self_method_call() {
        let (mut interp, class_def) = run_class(
            "\
func helper():
    return 42
func main():
    return self.helper()
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let result = interp.call_instance_method(&mut inst, "main", &[]).unwrap();
        assert_eq!(result, Variant::Int(42));
    }

    #[test]
    fn class_inheritance_chain() {
        let mut interp = Interpreter::new();
        let _parent = interp
            .run_class(
                "\
class_name BaseEntity
var name = \"base\"
func _init():
    return name
",
            )
            .unwrap();
        let child = interp
            .run_class(
                "\
class_name Player
extends BaseEntity
var level = 1
func get_level():
    return level
",
            )
            .unwrap();
        assert_eq!(child.parent_class.as_deref(), Some("BaseEntity"));
        assert_eq!(child.name.as_deref(), Some("Player"));
    }

    #[test]
    fn class_super_call() {
        let mut interp = Interpreter::new();
        let _parent = interp
            .run_class(
                "\
class_name Base
func _init():
    return 42
",
            )
            .unwrap();
        let child = interp
            .run_class(
                "\
class_name Child
extends Base
func call_super():
    return super()
",
            )
            .unwrap();
        let mut inst = interp.instantiate_class(&child).unwrap();
        let result = interp
            .call_instance_method(&mut inst, "call_super", &[])
            .unwrap();
        assert_eq!(result, Variant::Int(42));
    }

    #[test]
    fn class_full_script() {
        let src = "\
extends Node
class_name Player

signal health_changed(new_health)

enum State { IDLE, RUNNING, JUMPING }

@export
var speed: float = 100.0
var health: int = 100

func _ready():
    return health

func take_damage(amount):
    health -= amount
    return health

func get_speed():
    return speed
";
        let (mut interp, class_def) = run_class(src);
        assert_eq!(class_def.name.as_deref(), Some("Player"));
        assert_eq!(class_def.parent_class.as_deref(), Some("Node"));
        assert_eq!(class_def.signals, vec!["health_changed"]);
        assert!(class_def.enums.contains_key("State"));
        assert_eq!(class_def.exports.len(), 1);
        assert!(class_def.methods.contains_key("_ready"));
        assert!(class_def.methods.contains_key("take_damage"));

        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let hp = interp
            .call_instance_method(&mut inst, "_ready", &[])
            .unwrap();
        assert_eq!(hp, Variant::Int(100));

        let hp = interp
            .call_instance_method(&mut inst, "take_damage", &[Variant::Int(25)])
            .unwrap();
        assert_eq!(hp, Variant::Int(75));

        let spd = interp
            .call_instance_method(&mut inst, "get_speed", &[])
            .unwrap();
        assert_eq!(spd, Variant::Float(100.0));
    }

    #[test]
    fn class_registered_in_registry() {
        let mut interp = Interpreter::new();
        interp.run_class("class_name Foo\nvar x = 1\n").unwrap();
        assert!(interp.class_registry.contains_key("Foo"));
    }

    #[test]
    fn class_no_name_not_registered() {
        let mut interp = Interpreter::new();
        interp.run_class("var x = 1\n").unwrap();
        assert!(interp.class_registry.is_empty());
    }

    #[test]
    fn class_method_undefined_error() {
        let (mut interp, class_def) = run_class("func foo():\n    pass\n");
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let err = interp
            .call_instance_method(&mut inst, "bar", &[])
            .unwrap_err();
        assert!(matches!(err.kind, RuntimeErrorKind::UndefinedFunction(_)));
    }

    #[test]
    fn class_multiple_methods_dispatch() {
        let (mut interp, class_def) = run_class(
            "\
func add(a, b):
    return a + b
func mul(a, b):
    return a * b
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let sum = interp
            .call_instance_method(&mut inst, "add", &[Variant::Int(3), Variant::Int(4)])
            .unwrap();
        assert_eq!(sum, Variant::Int(7));
        let prod = interp
            .call_instance_method(&mut inst, "mul", &[Variant::Int(3), Variant::Int(4)])
            .unwrap();
        assert_eq!(prod, Variant::Int(12));
    }

    #[test]
    fn class_enum_variant_count() {
        let (_, class_def) = run_class("enum Dir { N, S, E, W }\n");
        let dir_enum = class_def.enums.get("Dir").unwrap();
        assert_eq!(dir_enum.len(), 4);
        assert_eq!(dir_enum.get("W"), Some(&3));
    }

    #[test]
    fn class_export_onready_annotation() {
        let (_, class_def) = run_class(
            "\
@onready
var node = null
@export
var speed = 10
",
        );
        // Only speed should be in exports (onready is not export)
        assert_eq!(class_def.exports.len(), 1);
        assert_eq!(class_def.exports[0].name, "speed");
        // But both should be in instance_vars
        assert_eq!(class_def.instance_vars.len(), 2);
        assert_eq!(class_def.instance_vars[0].annotations[0].name, "onready");
    }

    #[test]
    fn class_extends_without_class_name() {
        let (_, class_def) = run_class("extends Sprite2D\nvar x = 0\n");
        assert_eq!(class_def.parent_class.as_deref(), Some("Sprite2D"));
        assert!(class_def.name.is_none());
    }

    // -- New: match, ternary, string formatting, built-ins, methods --
    #[test]
    fn match_literal_int() {
        assert_eq!(run_val("var r = 0\nmatch 2:\n    1:\n        r = 10\n    2:\n        r = 20\n    3:\n        r = 30\nreturn r\n"), Variant::Int(20));
    }
    #[test]
    fn match_wildcard() {
        assert_eq!(
            run_val("match 99:\n    1:\n        return 10\n    _:\n        return 42\n"),
            Variant::Int(42)
        );
    }
    #[test]
    fn match_variable_binding() {
        // A `var` pattern binds; a bare identifier is a constant reference
        // (Godot semantics — see pat-u3l8c.9).
        assert_eq!(
            run_val("match 7:\n    var x:\n        return x * 2\n"),
            Variant::Int(14)
        );
    }
    #[test]
    fn match_string_pattern() {
        assert_eq!(run_val("match \"hello\":\n    \"world\":\n        return 1\n    \"hello\":\n        return 2\n    _:\n        return 3\n"), Variant::Int(2));
    }
    #[test]
    fn match_array_destructure() {
        assert_eq!(
            run_val("match [1, 2]:\n    [1, 2]:\n        return 99\n    _:\n        return 0\n"),
            Variant::Int(99)
        );
    }
    #[test]
    fn match_array_binding() {
        // Array element binds require `var` in each slot (Godot semantics).
        assert_eq!(
            run_val("match [10, 20]:\n    [var a, var b]:\n        return a + b\n"),
            Variant::Int(30)
        );
    }
    #[test]
    fn match_no_arm() {
        assert_eq!(
            run_val(
                "var r = 0\nmatch 5:\n    1:\n        r = 10\n    2:\n        r = 20\nreturn r\n"
            ),
            Variant::Int(0)
        );
    }
    #[test]
    fn ternary_true() {
        assert_eq!(run_val("return 10 if true else 20\n"), Variant::Int(10));
    }
    #[test]
    fn ternary_false() {
        assert_eq!(run_val("return 10 if false else 20\n"), Variant::Int(20));
    }
    #[test]
    fn ternary_expr() {
        assert_eq!(
            run_val("var x = 5\nreturn \"big\" if x > 3 else \"small\"\n"),
            Variant::String("big".into())
        );
    }
    #[test]
    fn str_fmt_basic() {
        assert_eq!(
            run_val("return \"Hello %s\" % \"world\"\n"),
            Variant::String("Hello world".into())
        );
    }
    #[test]
    fn str_fmt_multi() {
        assert_eq!(
            run_val("return \"%s has %d items\" % [\"bag\", 5]\n"),
            Variant::String("bag has 5 items".into())
        );
    }
    #[test]
    fn str_fmt_escape() {
        assert_eq!(
            run_val("return \"100%%\" % []\n"),
            Variant::String("100%".into())
        );
    }
    #[test]
    fn preload_placeholder() {
        // preload() now returns a Resource variant with path and inferred class
        let result = run_val("return preload(\"res://scene.tscn\")\n");
        match result {
            Variant::Resource(r) => {
                assert_eq!(r.path, "res://scene.tscn");
                assert_eq!(r.class_name, "PackedScene");
            }
            other => panic!("expected Resource, got {other:?}"),
        }
    }
    #[test]
    fn builtin_min_max() {
        assert_eq!(run_val("return min(3, 7)\n"), Variant::Int(3));
        assert_eq!(run_val("return max(3, 7)\n"), Variant::Int(7));
    }
    #[test]
    fn builtin_clamp() {
        assert_eq!(run_val("return clamp(15, 0, 10)\n"), Variant::Int(10));
        assert_eq!(run_val("return clamp(-5, 0, 10)\n"), Variant::Int(0));
    }
    #[test]
    fn builtin_lerp() {
        assert_eq!(
            run_val("return lerp(0.0, 10.0, 0.5)\n"),
            Variant::Float(5.0)
        );
    }
    #[test]
    fn builtin_sign() {
        assert_eq!(run_val("return sign(5)\n"), Variant::Int(1));
        assert_eq!(run_val("return sign(-3)\n"), Variant::Int(-1));
        assert_eq!(run_val("return sign(0)\n"), Variant::Int(0));
    }
    #[test]
    fn builtin_floor_ceil_round() {
        assert_eq!(run_val("return floor(3.7)\n"), Variant::Float(3.0));
        assert_eq!(run_val("return ceil(3.2)\n"), Variant::Float(4.0));
        assert_eq!(run_val("return round(3.5)\n"), Variant::Float(4.0));
    }
    #[test]
    fn builtin_sqrt() {
        assert_eq!(run_val("return sqrt(9.0)\n"), Variant::Float(3.0));
    }
    #[test]
    fn builtin_pow() {
        assert_eq!(run_val("return pow(2.0, 3.0)\n"), Variant::Float(8.0));
    }
    #[test]
    fn builtin_sin_cos() {
        assert_eq!(run_val("return sin(0.0)\n"), Variant::Float(0.0));
        assert_eq!(run_val("return cos(0.0)\n"), Variant::Float(1.0));
    }
    #[test]
    fn constants_pi_tau() {
        let pi = run_val("return PI\n");
        assert!(matches!(pi, Variant::Float(f) if (f - std::f64::consts::PI).abs() < 1e-10));
        let tau = run_val("return TAU\n");
        assert!(matches!(tau, Variant::Float(f) if (f - std::f64::consts::TAU).abs() < 1e-10));
    }
    #[test]
    fn constants_inf_nan() {
        assert!(matches!(run_val("return INF\n"), Variant::Float(f) if f.is_infinite()));
        assert!(matches!(run_val("return NAN\n"), Variant::Float(f) if f.is_nan()));
    }
    #[test]
    fn builtin_randi() {
        assert!(matches!(run_val("return randi()\n"), Variant::Int(_)));
    }
    #[test]
    fn builtin_randf() {
        if let Variant::Float(f) = run_val("return randf()\n") {
            assert!(f >= -1.0 && f <= 1.0);
        } else {
            panic!("expected float");
        }
    }
    #[test]
    fn array_sort_test() {
        assert_eq!(
            run_val("var a = [3, 1, 2]\na.sort()\nreturn a\n"),
            Variant::Array(vec![Variant::Int(1), Variant::Int(2), Variant::Int(3)])
        );
    }
    #[test]
    fn array_reverse_test() {
        assert_eq!(
            run_val("var a = [1, 2, 3]\na.reverse()\nreturn a\n"),
            Variant::Array(vec![Variant::Int(3), Variant::Int(2), Variant::Int(1)])
        );
    }
    #[test]
    fn array_find_has() {
        assert_eq!(run_val("return [10, 20, 30].find(20)\n"), Variant::Int(1));
        assert_eq!(
            run_val("return [10, 20, 30].has(20)\n"),
            Variant::Bool(true)
        );
        assert_eq!(
            run_val("return [10, 20, 30].has(99)\n"),
            Variant::Bool(false)
        );
    }
    #[test]
    fn array_erase_test() {
        assert_eq!(
            run_val("var a = [1, 2, 3, 2]\na.erase(2)\nreturn a\n"),
            Variant::Array(vec![Variant::Int(1), Variant::Int(3), Variant::Int(2)])
        );
    }
    #[test]
    fn array_insert_test() {
        assert_eq!(
            run_val("var a = [1, 3]\na.insert(1, 2)\nreturn a\n"),
            Variant::Array(vec![Variant::Int(1), Variant::Int(2), Variant::Int(3)])
        );
    }
    #[test]
    fn array_slice_test() {
        assert_eq!(
            run_val("return [10, 20, 30, 40, 50].slice(1, 4)\n"),
            Variant::Array(vec![Variant::Int(20), Variant::Int(30), Variant::Int(40)])
        );
    }
    #[test]
    fn str_upper_lower() {
        assert_eq!(
            run_val("return \"hello\".to_upper()\n"),
            Variant::String("HELLO".into())
        );
        assert_eq!(
            run_val("return \"HELLO\".to_lower()\n"),
            Variant::String("hello".into())
        );
    }
    #[test]
    fn str_begins_ends() {
        assert_eq!(
            run_val("return \"hello world\".begins_with(\"hello\")\n"),
            Variant::Bool(true)
        );
        assert_eq!(
            run_val("return \"hello world\".ends_with(\"world\")\n"),
            Variant::Bool(true)
        );
    }
    #[test]
    fn str_split_join() {
        assert_eq!(
            run_val("return \"a,b,c\".split(\",\")\n"),
            Variant::Array(vec![
                Variant::String("a".into()),
                Variant::String("b".into()),
                Variant::String("c".into())
            ])
        );
        assert_eq!(
            run_val("return \"-\".join([\"a\", \"b\", \"c\"])\n"),
            Variant::String("a-b-c".into())
        );
    }
    #[test]
    fn str_replace() {
        assert_eq!(
            run_val("return \"hello world\".replace(\"world\", \"rust\")\n"),
            Variant::String("hello rust".into())
        );
    }
    #[test]
    fn str_find_method() {
        assert_eq!(run_val("return \"hello\".find(\"ll\")\n"), Variant::Int(2));
        assert_eq!(
            run_val("return \"hello\".find(\"xyz\")\n"),
            Variant::Int(-1)
        );
    }
    #[test]
    fn str_substr() {
        assert_eq!(
            run_val("return \"hello\".substr(1, 3)\n"),
            Variant::String("ell".into())
        );
    }
    #[test]
    fn dict_keys_vals() {
        let v = run_val("var d = {\"a\": 1}\nreturn d.keys()\n");
        assert!(matches!(v, Variant::Array(ref a) if a.len() == 1));
    }
    #[test]
    fn dict_erase_test() {
        assert_eq!(
            run_val("var d = {\"a\": 1, \"b\": 2}\nd.erase(\"a\")\nreturn d.size()\n"),
            Variant::Int(1)
        );
    }
    #[test]
    fn dict_merge_test() {
        assert_eq!(
            run_val("var d = {\"a\": 1}\nd.merge({\"b\": 2})\nreturn d.size()\n"),
            Variant::Int(2)
        );
    }
    #[test]
    fn dict_get_default() {
        assert_eq!(
            run_val("var d = {\"a\": 1}\nreturn d.get(\"b\", 42)\n"),
            Variant::Int(42)
        );
    }

    #[test]
    fn error_includes_source_location() {
        let mut interp = Interpreter::new();
        let err = interp
            .run("var x = 1\nvar y = undefined_var\n")
            .unwrap_err();
        assert!(matches!(err.kind, RuntimeErrorKind::UndefinedVariable(_)));
    }
    #[test]
    fn error_display_shows_kind() {
        assert!(
            format!("{}", RuntimeError::new(RuntimeErrorKind::DivisionByZero))
                .contains("division by zero")
        );
    }
    #[test]
    fn error_display_with_location() {
        let err = RuntimeError::new(RuntimeErrorKind::TypeError("bad".into())).with_location(
            SourceLocation {
                line: 5,
                column: 3,
                source_line: "var x = bad".to_string(),
            },
        );
        let msg = format!("{err}");
        assert!(msg.contains("line 5") && msg.contains("var x = bad"));
    }
    #[test]
    fn error_display_with_call_stack() {
        let err =
            RuntimeError::new(RuntimeErrorKind::DivisionByZero).with_call_stack(vec![StackFrame {
                function_name: "foo".to_string(),
                source_location: Some(SourceLocation {
                    line: 10,
                    column: 1,
                    source_line: "func foo():".to_string(),
                }),
            }]);
        let msg = format!("{err}");
        assert!(msg.contains("Call stack") && msg.contains("in foo"));
    }
    #[test]
    fn error_has_std_error_source() {
        assert!(
            std::error::Error::source(&RuntimeError::new(RuntimeErrorKind::DivisionByZero))
                .is_some()
        );
    }
    #[test]
    fn runtime_error_kind_variants() {
        let _ = (
            RuntimeErrorKind::UndefinedVariable("x".into()),
            RuntimeErrorKind::TypeError("t".into()),
            RuntimeErrorKind::DivisionByZero,
            RuntimeErrorKind::UndefinedFunction("f".into()),
            RuntimeErrorKind::IndexOutOfBounds {
                index: 0,
                length: 0,
            },
            RuntimeErrorKind::MaxRecursionDepth(10),
        );
    }
    #[test]
    fn source_location_display() {
        let msg = format!(
            "{}",
            SourceLocation {
                line: 3,
                column: 5,
                source_line: "var x = 1".to_string()
            }
        );
        assert!(msg.contains("line 3") && msg.contains("var x = 1") && msg.contains("^"));
    }
    #[test]
    fn stack_frame_in_error() {
        let mut interp = Interpreter::new();
        let err = interp
            .run("func foo():\n    return 1 / 0\nfoo()\n")
            .unwrap_err();
        assert!(matches!(err.kind, RuntimeErrorKind::DivisionByZero));
    }
    #[test]
    fn warning_unreachable_code() {
        let mut interp = Interpreter::new();
        let _ = interp.run("return 1\nvar x = 2\n");
        assert!(interp
            .warnings()
            .iter()
            .any(|w| matches!(w, ScriptWarning::UnreachableCode { .. })));
    }
    #[test]
    fn warning_display() {
        let w = ScriptWarning::UnusedVariable {
            name: "x".to_string(),
            location: SourceLocation {
                line: 1,
                column: 5,
                source_line: "var x = 1".to_string(),
            },
        };
        assert!(format!("{w}").contains("unused variable"));
    }
    #[test]
    fn interpreter_result_is_debug() {
        let _ = format!(
            "{:?}",
            InterpreterResult {
                output: vec![],
                return_value: None
            }
        );
    }
    #[test]
    fn error_from_undefined_function() {
        assert!(matches!(
            Interpreter::new().run("no_such_func()").unwrap_err().kind,
            RuntimeErrorKind::UndefinedFunction(_)
        ));
    }
    #[test]
    fn error_from_index_out_of_bounds() {
        assert!(matches!(
            Interpreter::new()
                .run("var a = [1,2]\nvar x = a[10]\n")
                .unwrap_err()
                .kind,
            RuntimeErrorKind::IndexOutOfBounds { .. }
        ));
    }
    #[test]
    fn match_stmt_basic() {
        let res = Interpreter::new().run("var x = 2\nmatch x:\n    1:\n        print(\"one\")\n    2:\n        print(\"two\")\n    _:\n        print(\"other\")\n").unwrap();
        assert_eq!(res.output, vec!["two"]);
    }
    #[test]
    fn ternary_expr_with_print() {
        let res = Interpreter::new()
            .run("var x = 10 if true else 20\nprint(x)\n")
            .unwrap();
        assert_eq!(res.output, vec!["10"]);
    }

    // -----------------------------------------------------------------------
    // Vector2 / Vector3 / Color built-in type tests
    // -----------------------------------------------------------------------

    #[test]
    fn vector2_constructor() {
        let v = run_val("return Vector2(3.0, 4.0)");
        assert!(matches!(v, Variant::Vector2(v) if v.x == 3.0 && v.y == 4.0));
    }

    #[test]
    fn vector2_constructor_from_int() {
        let v = run_val("return Vector2(3, 4)");
        assert!(matches!(v, Variant::Vector2(v) if v.x == 3.0 && v.y == 4.0));
    }

    #[test]
    fn vector3_constructor() {
        let v = run_val("return Vector3(1.0, 2.0, 3.0)");
        assert!(matches!(v, Variant::Vector3(v) if v.x == 1.0 && v.y == 2.0 && v.z == 3.0));
    }

    #[test]
    fn color_constructor_rgb() {
        let v = run_val("return Color(1.0, 0.0, 0.0)");
        assert!(
            matches!(v, Variant::Color(c) if c.r == 1.0 && c.g == 0.0 && c.b == 0.0 && c.a == 1.0)
        );
    }

    #[test]
    fn color_constructor_rgba() {
        let v = run_val("return Color(1.0, 0.5, 0.0, 0.8)");
        if let Variant::Color(c) = v {
            assert!((c.r - 1.0).abs() < 1e-6);
            assert!((c.g - 0.5).abs() < 1e-6);
            assert!((c.b - 0.0).abs() < 1e-6);
            assert!((c.a - 0.8).abs() < 1e-6);
        } else {
            panic!("expected Color");
        }
    }

    #[test]
    fn vector2_static_zero() {
        assert_eq!(
            run_val("return Vector2.ZERO"),
            Variant::Vector2(gdcore::math::Vector2::ZERO)
        );
    }

    #[test]
    fn vector2_static_one() {
        assert_eq!(
            run_val("return Vector2.ONE"),
            Variant::Vector2(gdcore::math::Vector2::ONE)
        );
    }

    #[test]
    fn vector2_static_directions() {
        assert_eq!(
            run_val("return Vector2.UP"),
            Variant::Vector2(gdcore::math::Vector2::UP)
        );
        assert_eq!(
            run_val("return Vector2.DOWN"),
            Variant::Vector2(gdcore::math::Vector2::DOWN)
        );
        assert_eq!(
            run_val("return Vector2.LEFT"),
            Variant::Vector2(gdcore::math::Vector2::LEFT)
        );
        assert_eq!(
            run_val("return Vector2.RIGHT"),
            Variant::Vector2(gdcore::math::Vector2::RIGHT)
        );
    }

    #[test]
    fn vector3_static_constants() {
        assert_eq!(
            run_val("return Vector3.ZERO"),
            Variant::Vector3(gdcore::math::Vector3::ZERO)
        );
        assert_eq!(
            run_val("return Vector3.ONE"),
            Variant::Vector3(gdcore::math::Vector3::ONE)
        );
        assert_eq!(
            run_val("return Vector3.UP"),
            Variant::Vector3(gdcore::math::Vector3::UP)
        );
        assert_eq!(
            run_val("return Vector3.DOWN"),
            Variant::Vector3(gdcore::math::Vector3::DOWN)
        );
        assert_eq!(
            run_val("return Vector3.FORWARD"),
            Variant::Vector3(gdcore::math::Vector3::FORWARD)
        );
    }

    #[test]
    fn color_static_constants() {
        assert_eq!(
            run_val("return Color.WHITE"),
            Variant::Color(gdcore::math::Color::WHITE)
        );
        assert_eq!(
            run_val("return Color.BLACK"),
            Variant::Color(gdcore::math::Color::BLACK)
        );
        assert_eq!(
            run_val("return Color.RED"),
            Variant::Color(gdcore::math::Color::new(1.0, 0.0, 0.0, 1.0))
        );
        assert_eq!(
            run_val("return Color.GREEN"),
            Variant::Color(gdcore::math::Color::new(0.0, 1.0, 0.0, 1.0))
        );
        assert_eq!(
            run_val("return Color.BLUE"),
            Variant::Color(gdcore::math::Color::new(0.0, 0.0, 1.0, 1.0))
        );
    }

    #[test]
    fn vector2_property_access_xy() {
        let r = run("var v = Vector2(3.0, 4.0)\nprint(v.x)\nprint(v.y)\n");
        assert_eq!(r.output, vec!["3", "4"]);
    }

    #[test]
    fn vector3_property_access_xyz() {
        let r = run("var v = Vector3(1.0, 2.0, 3.0)\nprint(v.x)\nprint(v.y)\nprint(v.z)\n");
        assert_eq!(r.output, vec!["1", "2", "3"]);
    }

    #[test]
    fn color_property_access_rgba() {
        let v = run_val("var c = Color(0.25, 0.5, 0.75, 1.0)\nreturn c.g");
        if let Variant::Float(f) = v {
            assert!((f - 0.5).abs() < 1e-5);
        } else {
            panic!("expected Float");
        }
    }

    #[test]
    fn vector2_add_op() {
        let v = run_val("return Vector2(1.0, 2.0) + Vector2(3.0, 4.0)");
        assert_eq!(v, Variant::Vector2(gdcore::math::Vector2::new(4.0, 6.0)));
    }

    #[test]
    fn vector2_sub_op() {
        let v = run_val("return Vector2(5.0, 7.0) - Vector2(2.0, 3.0)");
        assert_eq!(v, Variant::Vector2(gdcore::math::Vector2::new(3.0, 4.0)));
    }

    #[test]
    fn vector2_mul_scalar_op() {
        let v = run_val("return Vector2(1.0, 2.0) * 3.0");
        assert_eq!(v, Variant::Vector2(gdcore::math::Vector2::new(3.0, 6.0)));
    }

    #[test]
    fn scalar_mul_vector2_op() {
        let v = run_val("return 2.0 * Vector2(3.0, 4.0)");
        assert_eq!(v, Variant::Vector2(gdcore::math::Vector2::new(6.0, 8.0)));
    }

    #[test]
    fn vector2_div_scalar_op() {
        let v = run_val("return Vector2(6.0, 8.0) / 2.0");
        assert_eq!(v, Variant::Vector2(gdcore::math::Vector2::new(3.0, 4.0)));
    }

    #[test]
    fn vector2_negate_op() {
        let v = run_val("return -Vector2(1.0, 2.0)");
        assert_eq!(v, Variant::Vector2(gdcore::math::Vector2::new(-1.0, -2.0)));
    }

    #[test]
    fn vector2_length_method() {
        let v = run_val("var v = Vector2(3.0, 4.0)\nreturn v.length()");
        assert_eq!(v, Variant::Float(5.0));
    }

    #[test]
    fn vector2_length_squared_method() {
        let v = run_val("var v = Vector2(3.0, 4.0)\nreturn v.length_squared()");
        assert_eq!(v, Variant::Float(25.0));
    }

    #[test]
    fn vector2_normalized_method() {
        let v = run_val("var v = Vector2(3.0, 4.0)\nvar n = v.normalized()\nreturn n.length()");
        if let Variant::Float(f) = v {
            assert!((f - 1.0).abs() < 1e-5);
        } else {
            panic!("expected Float");
        }
    }

    #[test]
    fn vector2_dot_method() {
        let v = run_val("return Vector2(1.0, 0.0).dot(Vector2(0.0, 1.0))");
        assert_eq!(v, Variant::Float(0.0));
    }

    #[test]
    fn vector2_distance_to_method() {
        let v = run_val("return Vector2(0.0, 0.0).distance_to(Vector2(3.0, 4.0))");
        assert_eq!(v, Variant::Float(5.0));
    }

    #[test]
    fn vector2_lerp_method() {
        let v = run_val("return Vector2(0.0, 0.0).lerp(Vector2(10.0, 20.0), 0.5)");
        assert_eq!(v, Variant::Vector2(gdcore::math::Vector2::new(5.0, 10.0)));
    }

    #[test]
    fn vector2_angle_method() {
        let v = run_val("return Vector2(1.0, 0.0).angle()");
        assert_eq!(v, Variant::Float(0.0));
    }

    #[test]
    fn vector2_cross_method() {
        let v = run_val("return Vector2(1.0, 0.0).cross(Vector2(0.0, 1.0))");
        assert_eq!(v, Variant::Float(1.0));
    }

    #[test]
    fn vector3_add_op() {
        let v = run_val("return Vector3(1.0, 2.0, 3.0) + Vector3(4.0, 5.0, 6.0)");
        assert_eq!(
            v,
            Variant::Vector3(gdcore::math::Vector3::new(5.0, 7.0, 9.0))
        );
    }

    #[test]
    fn vector3_mul_scalar_op() {
        let v = run_val("return Vector3(1.0, 2.0, 3.0) * 2.0");
        assert_eq!(
            v,
            Variant::Vector3(gdcore::math::Vector3::new(2.0, 4.0, 6.0))
        );
    }

    #[test]
    fn vector3_div_scalar_op() {
        let v = run_val("return Vector3(6.0, 8.0, 10.0) / 2.0");
        assert_eq!(
            v,
            Variant::Vector3(gdcore::math::Vector3::new(3.0, 4.0, 5.0))
        );
    }

    #[test]
    fn vector3_negate_op() {
        let v = run_val("return -Vector3(1.0, 2.0, 3.0)");
        assert_eq!(
            v,
            Variant::Vector3(gdcore::math::Vector3::new(-1.0, -2.0, -3.0))
        );
    }

    #[test]
    fn vector3_length_method() {
        let v = run_val("return Vector3(1.0, 2.0, 2.0).length()");
        if let Variant::Float(f) = v {
            assert!((f - 3.0).abs() < 1e-5);
        } else {
            panic!("expected Float");
        }
    }

    #[test]
    fn vector3_dot_method() {
        let v = run_val("return Vector3(1.0, 0.0, 0.0).dot(Vector3(0.0, 1.0, 0.0))");
        assert_eq!(v, Variant::Float(0.0));
    }

    #[test]
    fn vector3_cross_method() {
        let v = run_val("return Vector3(1.0, 0.0, 0.0).cross(Vector3(0.0, 1.0, 0.0))");
        assert_eq!(
            v,
            Variant::Vector3(gdcore::math::Vector3::new(0.0, 0.0, 1.0))
        );
    }

    #[test]
    fn vector2_equality_op() {
        assert_eq!(
            run_val("return Vector2(1.0, 2.0) == Vector2(1.0, 2.0)"),
            Variant::Bool(true)
        );
        assert_eq!(
            run_val("return Vector2(1.0, 2.0) == Vector2(3.0, 4.0)"),
            Variant::Bool(false)
        );
        assert_eq!(
            run_val("return Vector2(1.0, 2.0) != Vector2(3.0, 4.0)"),
            Variant::Bool(true)
        );
    }

    #[test]
    fn vector2_int_mul_op() {
        let v = run_val("return Vector2(1.0, 2.0) * 3");
        assert_eq!(v, Variant::Vector2(gdcore::math::Vector2::new(3.0, 6.0)));
    }

    #[test]
    fn vector2_div_zero_error() {
        let err = run_err("return Vector2(1.0, 2.0) / 0.0");
        assert!(matches!(err.kind, RuntimeErrorKind::DivisionByZero));
    }

    #[test]
    fn vector2_complex_expression() {
        let v = run_val(
            "var a = Vector2(1.0, 2.0)\nvar b = Vector2(3.0, 4.0)\nvar c = (a + b) * 2.0\nreturn c",
        );
        assert_eq!(v, Variant::Vector2(gdcore::math::Vector2::new(8.0, 12.0)));
    }

    // -----------------------------------------------------------------------
    // Default parameter values (interpreter)
    // -----------------------------------------------------------------------

    #[test]
    fn default_param_used_when_no_arg() {
        let v = run_val("func foo(x = 42):\n    return x\nreturn foo()\n");
        assert_eq!(v, Variant::Int(42));
    }

    #[test]
    fn default_param_overridden_by_arg() {
        let v = run_val("func foo(x = 42):\n    return x\nreturn foo(99)\n");
        assert_eq!(v, Variant::Int(99));
    }

    #[test]
    fn default_param_string() {
        let v = run_val("func greet(name = \"world\"):\n    return name\nreturn greet()\n");
        assert_eq!(v, Variant::String("world".into()));
    }

    #[test]
    fn default_param_mixed_required_and_default() {
        let v = run_val("func add(a, b = 10):\n    return a + b\nreturn add(5)\n");
        assert_eq!(v, Variant::Int(15));
    }

    #[test]
    fn default_param_mixed_all_provided() {
        let v = run_val("func add(a, b = 10):\n    return a + b\nreturn add(5, 20)\n");
        assert_eq!(v, Variant::Int(25));
    }

    #[test]
    fn default_param_multiple_defaults() {
        let v = run_val("func make(x = 1, y = 2, z = 3):\n    return x + y + z\nreturn make()\n");
        assert_eq!(v, Variant::Int(6));
    }

    #[test]
    fn default_param_partial_override() {
        let v = run_val("func make(x = 1, y = 2, z = 3):\n    return x + y + z\nreturn make(10)\n");
        assert_eq!(v, Variant::Int(15));
    }

    #[test]
    fn default_param_negative_value() {
        let v = run_val("func offset(x = -5):\n    return x\nreturn offset()\n");
        assert_eq!(v, Variant::Int(-5));
    }

    #[test]
    fn default_param_too_many_args_error() {
        let err = run_err("func foo(x = 1):\n    return x\nreturn foo(1, 2)\n");
        assert!(matches!(err.kind, RuntimeErrorKind::TypeError(_)));
    }

    #[test]
    fn default_param_too_few_args_error() {
        let err = run_err("func foo(a, b, c = 1):\n    return a\nreturn foo()\n");
        assert!(matches!(err.kind, RuntimeErrorKind::TypeError(_)));
    }

    // -----------------------------------------------------------------------
    // Static functions (interpreter)
    // -----------------------------------------------------------------------

    #[test]
    fn static_func_executes() {
        let v = run_val("static func helper() -> int:\n    return 42\nreturn helper()\n");
        assert_eq!(v, Variant::Int(42));
    }

    #[test]
    fn static_func_with_params() {
        let v = run_val(
            "static func add(a: int, b: int) -> int:\n    return a + b\nreturn add(3, 4)\n",
        );
        assert_eq!(v, Variant::Int(7));
    }

    #[test]
    fn static_func_in_class() {
        let mut interp = Interpreter::new();
        let class_def = interp
            .run_class("class_name Util\nstatic func double(x: int) -> int:\n    return x * 2\n")
            .unwrap();
        assert!(class_def.methods.contains_key("double"));
        assert!(class_def.methods["double"].is_static);
    }

    // -----------------------------------------------------------------------
    // Empty return (interpreter)
    // -----------------------------------------------------------------------

    #[test]
    fn empty_return_yields_nil() {
        let v = run_val("func foo():\n    return\nreturn foo()\n");
        assert_eq!(v, Variant::Nil);
    }

    #[test]
    fn empty_return_exits_early() {
        let r =
            run("func foo():\n    print(\"before\")\n    return\n    print(\"after\")\nfoo()\n");
        assert_eq!(r.output, vec!["before"]);
    }

    // -----------------------------------------------------------------------
    // Negative number literals (interpreter)
    // -----------------------------------------------------------------------

    #[test]
    fn negative_int_in_var() {
        let v = run_val("var x = -10\nreturn x\n");
        assert_eq!(v, Variant::Int(-10));
    }

    #[test]
    fn negative_float_in_var() {
        let v = run_val("var x = -3.14\nreturn x\n");
        assert_eq!(v, Variant::Float(-3.14));
    }

    #[test]
    fn negative_in_expression() {
        let v = run_val("return 5 + -3\n");
        assert_eq!(v, Variant::Int(2));
    }

    // -----------------------------------------------------------------------
    // Expanded OS singleton surface (pat-u3l8c.21)
    // -----------------------------------------------------------------------

    #[test]
    fn os_get_environment_unset_returns_empty() {
        // An unset variable resolves to "" (Godot semantics), deterministically.
        let v = run_val("return OS.get_environment(\"PATINA_DEFINITELY_UNSET_VAR_XYZ\")\n");
        assert_eq!(v, Variant::String(String::new()));
    }

    #[test]
    fn os_has_environment_unset_is_false() {
        let v = run_val("return OS.has_environment(\"PATINA_DEFINITELY_UNSET_VAR_XYZ\")\n");
        assert_eq!(v, Variant::Bool(false));
    }

    #[test]
    fn os_has_feature_known_flags() {
        // Deterministic regardless of build mode / host platform.
        assert_eq!(
            run_val("return OS.has_feature(\"template\")\n"),
            Variant::Bool(true)
        );
        assert_eq!(
            run_val("return OS.has_feature(\"editor\")\n"),
            Variant::Bool(false)
        );
        assert_eq!(
            run_val("return OS.has_feature(\"totally_unknown_feature\")\n"),
            Variant::Bool(false)
        );
    }

    #[test]
    fn os_get_cmdline_args_is_packed_string_array() {
        let v = run_val("return OS.get_cmdline_args()\n");
        assert!(matches!(v, Variant::PackedStringArray(_)), "got {v:?}");
    }

    #[test]
    fn os_get_user_data_dir_nonempty() {
        match run_val("return OS.get_user_data_dir()\n") {
            Variant::String(s) => assert!(!s.is_empty(), "user data dir should be non-empty"),
            other => panic!("expected String, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn os_execute_echo_returns_zero() {
        // /bin/echo exists on macOS + Linux and exits 0.
        let v = run_val("return OS.execute(\"/bin/echo\", [\"hi\"])\n");
        assert_eq!(v, Variant::Int(0));
    }

    // -----------------------------------------------------------------------
    // ClassDB reflection singleton (pat-u3l8c.21)
    // -----------------------------------------------------------------------

    #[test]
    fn classdb_class_exists_unknown_is_false() {
        // Returning Bool(false) (not an UndefinedFunction error) proves the
        // ClassDB routing -> gdobject::class_exists wiring is live.
        let v = run_val("return ClassDB.class_exists(\"__PatinaNoSuchClass__\")\n");
        assert_eq!(v, Variant::Bool(false));
    }

    #[test]
    fn classdb_can_instantiate_unknown_is_false() {
        let v = run_val("return ClassDB.can_instantiate(\"__PatinaNoSuchClass__\")\n");
        assert_eq!(v, Variant::Bool(false));
    }

    #[test]
    fn classdb_class_has_method_unknown_is_false() {
        let v = run_val("return ClassDB.class_has_method(\"__PatinaNoSuchClass__\", \"foo\")\n");
        assert_eq!(v, Variant::Bool(false));
    }

    #[test]
    fn classdb_get_class_list_is_packed_string_array() {
        let v = run_val("return ClassDB.get_class_list()\n");
        assert!(matches!(v, Variant::PackedStringArray(_)), "got {v:?}");
    }

    // -----------------------------------------------------------------------
    // ClassDB parent/signal/constant/enum reflection (pat-nzgdu.98)
    // -----------------------------------------------------------------------

    /// The reflection surface needs a populated registry — a bare interpreter
    /// test starts with an empty ClassDB. Registration is `class_exists`-guarded
    /// and therefore idempotent.
    fn run_val_with_classes(src: &str) -> Variant {
        gdobject::register_2d_classes();
        run_val(src)
    }

    #[test]
    fn classdb_get_parent_class_walks_the_registry() {
        let v = run_val_with_classes("return ClassDB.get_parent_class(\"Sprite2D\")\n");
        assert_eq!(v, Variant::String("Node2D".to_string()));
    }

    #[test]
    fn classdb_get_parent_class_unknown_is_empty_string() {
        // Godot yields "" rather than erroring.
        let v = run_val("return ClassDB.get_parent_class(\"__PatinaNoSuchClass__\")\n");
        assert_eq!(v, Variant::String(String::new()));
    }

    #[test]
    fn classdb_class_get_signal_list_is_non_empty_for_node() {
        let v = run_val_with_classes("return ClassDB.class_get_signal_list(\"Node\")\n");
        let Variant::Array(list) = v else {
            panic!("expected Array, got {v:?}");
        };
        assert!(!list.is_empty(), "class_get_signal_list(\"Node\") is empty");
        assert!(
            matches!(list[0], Variant::Dictionary(_)),
            "entries are Dictionaries, got {:?}",
            list[0],
        );
    }

    #[test]
    fn classdb_class_has_signal_routes_to_gdobject() {
        let v =
            run_val_with_classes("return ClassDB.class_has_signal(\"Node\", \"tree_entered\")\n");
        assert_eq!(v, Variant::Bool(true));
        let v = run_val_with_classes("return ClassDB.class_has_signal(\"Node\", \"__nope__\")\n");
        assert_eq!(v, Variant::Bool(false));
    }

    #[test]
    fn classdb_integer_constant_reflection() {
        let v = run_val_with_classes(
            "return ClassDB.class_get_integer_constant(\"Node\", \"PROCESS_MODE_ALWAYS\")\n",
        );
        assert_eq!(v, Variant::Int(3));
        // Godot returns 0 for an unknown constant rather than erroring.
        let v = run_val_with_classes(
            "return ClassDB.class_get_integer_constant(\"Node\", \"__nope__\")\n",
        );
        assert_eq!(v, Variant::Int(0));
    }

    #[test]
    fn classdb_enum_reflection() {
        let v = run_val_with_classes("return ClassDB.class_has_enum(\"Node\", \"ProcessMode\")\n");
        assert_eq!(v, Variant::Bool(true));

        let v = run_val_with_classes(
            "return ClassDB.class_get_enum_constants(\"Node\", \"ProcessMode\")\n",
        );
        let Variant::PackedStringArray(items) = v else {
            panic!("expected PackedStringArray, got {v:?}");
        };
        assert!(
            items.contains(&"PROCESS_MODE_ALWAYS".to_string()),
            "{items:?}"
        );
    }

    #[test]
    fn classdb_acceptance_script_runs_without_undefined_function() {
        // The ClassDB half of pat-nzgdu.98's acceptance script. The instance
        // half (has_method/get_method_list on self) still needs the SceneAccess
        // reflection entry points and is NOT covered here.
        let v = run_val_with_classes(
            "assert(ClassDB.class_exists(\"Sprite2D\"))\n\
             assert(ClassDB.get_parent_class(\"Sprite2D\") == \"Node2D\")\n\
             assert(ClassDB.class_has_method(\"Node\", \"get_parent\"))\n\
             assert(ClassDB.class_get_signal_list(\"Node\").size() > 0)\n\
             return true\n",
        );
        assert_eq!(v, Variant::Bool(true));
    }

    // -----------------------------------------------------------------------
    // ProjectSettings singleton (pat-u3l8c.21)
    // -----------------------------------------------------------------------

    #[test]
    fn projectsettings_get_setting_returns_default() {
        // A missing setting yields the caller-supplied default (Godot semantics).
        let v = run_val("return ProjectSettings.get_setting(\"application/config/nope\", 42)\n");
        assert_eq!(v, Variant::Int(42));
    }

    #[test]
    fn projectsettings_get_setting_no_default_is_nil() {
        let v = run_val("return ProjectSettings.get_setting(\"application/config/nope\")\n");
        assert_eq!(v, Variant::Nil);
    }

    #[test]
    fn projectsettings_has_setting_is_false() {
        assert_eq!(
            run_val("return ProjectSettings.has_setting(\"anything\")\n"),
            Variant::Bool(false)
        );
    }

    #[test]
    fn projectsettings_globalize_user_path() {
        match run_val("return ProjectSettings.globalize_path(\"user://save.dat\")\n") {
            Variant::String(s) => {
                assert!(!s.starts_with("user://"), "should resolve the scheme: {s}");
                assert!(s.ends_with("save.dat"), "should keep the filename: {s}");
            }
            other => panic!("expected String, got {other:?}"),
        }
    }

    #[test]
    fn projectsettings_globalize_absolute_passthrough() {
        let v = run_val("return ProjectSettings.globalize_path(\"/already/global.txt\")\n");
        assert_eq!(v, Variant::String("/already/global.txt".to_string()));
    }

    // -----------------------------------------------------------------------
    // print() with multiple args (interpreter)
    // -----------------------------------------------------------------------

    #[test]
    fn print_zero_args() {
        let r = run("print()\n");
        assert_eq!(r.output, vec![""]);
    }

    #[test]
    fn print_mixed_types() {
        let r = run("print(\"count:\", 42, true, null)\n");
        assert_eq!(r.output, vec!["count: 42 true <null>"]);
    }

    #[test]
    fn print_many_string_args() {
        let r = run("print(\"a\", \"b\", \"c\", \"d\", \"e\")\n");
        assert_eq!(r.output, vec!["a b c d e"]);
    }

    // -----------------------------------------------------------------------
    // @onready in class context (interpreter)
    // -----------------------------------------------------------------------

    #[test]
    fn onready_var_parsed_in_class() {
        let mut interp = Interpreter::new();
        let class_def = interp
            .run_class("extends Node\n@onready\nvar label = null\n")
            .unwrap();
        assert_eq!(class_def.instance_vars.len(), 1);
        assert_eq!(class_def.instance_vars[0].name, "label");
        assert!(class_def.instance_vars[0]
            .annotations
            .iter()
            .any(|a| a.name == "onready"));
    }

    // -----------------------------------------------------------------------
    // Typed var declarations (interpreter)
    // -----------------------------------------------------------------------

    #[test]
    fn typed_var_float_in_class() {
        let mut interp = Interpreter::new();
        let class_def = interp.run_class("var speed: float = 200.0\n").unwrap();
        assert_eq!(class_def.instance_vars[0].name, "speed");
        assert_eq!(
            class_def.instance_vars[0].type_hint.as_deref(),
            Some("float")
        );
    }

    #[test]
    fn typed_var_initialized() {
        let v = run_val("var speed: float = 200.0\nreturn speed\n");
        assert_eq!(v, Variant::Float(200.0));
    }

    // -----------------------------------------------------------------------
    // Default params in class methods (interpreter)
    // -----------------------------------------------------------------------

    #[test]
    fn class_method_default_params() {
        let mut interp = Interpreter::new();
        let class_def = interp
            .run_class("class_name Calc\nfunc add(a, b = 10):\n    return a + b\n")
            .unwrap();
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let result = interp
            .call_instance_method(&mut inst, "add", &[Variant::Int(5)])
            .unwrap();
        assert_eq!(result, Variant::Int(15));
    }

    #[test]
    fn class_method_default_params_overridden() {
        let mut interp = Interpreter::new();
        let class_def = interp
            .run_class("class_name Calc\nfunc add(a, b = 10):\n    return a + b\n")
            .unwrap();
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let result = interp
            .call_instance_method(&mut inst, "add", &[Variant::Int(5), Variant::Int(20)])
            .unwrap();
        assert_eq!(result, Variant::Int(25));
    }

    // -----------------------------------------------------------------------
    // Bare variable assignment in instance methods (#80)
    // -----------------------------------------------------------------------

    #[test]
    fn bare_assign_instance_var_in_ready() {
        let (mut interp, class_def) = run_class(
            "\
var count: int = 0
func _ready():
    count = 10
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        interp
            .call_instance_method(&mut inst, "_ready", &[])
            .unwrap();
        assert_eq!(inst.properties.get("count"), Some(&Variant::Int(10)));
    }

    #[test]
    fn bare_assign_instance_var_accumulates_across_calls() {
        let (mut interp, class_def) = run_class(
            "\
var count: int = 0
func _ready():
    count = 10
func _process(delta):
    count = count + 1
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        interp
            .call_instance_method(&mut inst, "_ready", &[])
            .unwrap();
        assert_eq!(inst.properties.get("count"), Some(&Variant::Int(10)));
        interp
            .call_instance_method(&mut inst, "_process", &[Variant::Float(0.016)])
            .unwrap();
        assert_eq!(inst.properties.get("count"), Some(&Variant::Int(11)));
        interp
            .call_instance_method(&mut inst, "_process", &[Variant::Float(0.016)])
            .unwrap();
        assert_eq!(inst.properties.get("count"), Some(&Variant::Int(12)));
    }

    #[test]
    fn bare_add_assign_instance_var() {
        let (mut interp, class_def) = run_class(
            "\
var score: int = 0
func add_score(amount):
    score += amount
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        interp
            .call_instance_method(&mut inst, "add_score", &[Variant::Int(10)])
            .unwrap();
        assert_eq!(inst.properties.get("score"), Some(&Variant::Int(10)));
        interp
            .call_instance_method(&mut inst, "add_score", &[Variant::Int(5)])
            .unwrap();
        assert_eq!(inst.properties.get("score"), Some(&Variant::Int(15)));
    }

    #[test]
    fn bare_sub_assign_instance_var() {
        let (mut interp, class_def) = run_class(
            "\
var health: int = 100
func take_damage(amount):
    health -= amount
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        interp
            .call_instance_method(&mut inst, "take_damage", &[Variant::Int(30)])
            .unwrap();
        assert_eq!(inst.properties.get("health"), Some(&Variant::Int(70)));
        interp
            .call_instance_method(&mut inst, "take_damage", &[Variant::Int(20)])
            .unwrap();
        assert_eq!(inst.properties.get("health"), Some(&Variant::Int(50)));
    }

    #[test]
    fn bare_assign_new_local_var_not_instance() {
        let (mut interp, class_def) = run_class(
            "\
var count: int = 0
func compute():
    var temp = 99
    temp = temp + 1
    return temp
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let result = interp
            .call_instance_method(&mut inst, "compute", &[])
            .unwrap();
        assert_eq!(result, Variant::Int(100));
        // count should be unchanged
        assert_eq!(inst.properties.get("count"), Some(&Variant::Int(0)));
        // temp should NOT appear in instance properties
        assert!(inst.properties.get("temp").is_none());
    }

    #[test]
    fn bare_assign_local_shadows_instance_var() {
        let (mut interp, class_def) = run_class(
            "\
var count: int = 42
func shadow_test():
    var count = 0
    count = count + 1
    return count
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let result = interp
            .call_instance_method(&mut inst, "shadow_test", &[])
            .unwrap();
        // local var shadows instance var — local should be 1
        assert_eq!(result, Variant::Int(1));
        // instance var should be unchanged
        assert_eq!(inst.properties.get("count"), Some(&Variant::Int(42)));
    }

    #[test]
    fn bare_assign_multiple_instance_vars() {
        let (mut interp, class_def) = run_class(
            "\
var x: int = 0
var y: int = 0
func set_pos(nx, ny):
    x = nx
    y = ny
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        interp
            .call_instance_method(&mut inst, "set_pos", &[Variant::Int(10), Variant::Int(20)])
            .unwrap();
        assert_eq!(inst.properties.get("x"), Some(&Variant::Int(10)));
        assert_eq!(inst.properties.get("y"), Some(&Variant::Int(20)));
    }

    #[test]
    fn bare_read_then_modify_instance_var() {
        let (mut interp, class_def) = run_class(
            "\
var value: int = 5
func double_it():
    var old = value
    value = old * 2
    return old
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let result = interp
            .call_instance_method(&mut inst, "double_it", &[])
            .unwrap();
        assert_eq!(result, Variant::Int(5));
        assert_eq!(inst.properties.get("value"), Some(&Variant::Int(10)));
    }

    #[test]
    fn bare_instance_var_read_returns_instance_value() {
        let (mut interp, class_def) = run_class(
            "\
var name = \"hello\"
func get_name():
    return name
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        // Modify the property directly, then read via method
        inst.properties
            .insert("name".to_string(), Variant::String("world".to_string()));
        let result = interp
            .call_instance_method(&mut inst, "get_name", &[])
            .unwrap();
        assert_eq!(result, Variant::String("world".to_string()));
    }

    #[test]
    fn bare_assign_different_types() {
        let (mut interp, class_def) = run_class(
            "\
var i: int = 0
var f: float = 0.0
var s = \"\"
var b: bool = false
func set_all():
    i = 42
    f = 3.14
    s = \"hello\"
    b = true
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        interp
            .call_instance_method(&mut inst, "set_all", &[])
            .unwrap();
        assert_eq!(inst.properties.get("i"), Some(&Variant::Int(42)));
        assert_eq!(inst.properties.get("f"), Some(&Variant::Float(3.14)));
        assert_eq!(
            inst.properties.get("s"),
            Some(&Variant::String("hello".to_string()))
        );
        assert_eq!(inst.properties.get("b"), Some(&Variant::Bool(true)));
    }

    #[test]
    fn bare_assign_persists_after_method_call() {
        let (mut interp, class_def) = run_class(
            "\
var count: int = 0
func increment():
    count = count + 1
func get_count():
    return count
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        interp
            .call_instance_method(&mut inst, "increment", &[])
            .unwrap();
        interp
            .call_instance_method(&mut inst, "increment", &[])
            .unwrap();
        interp
            .call_instance_method(&mut inst, "increment", &[])
            .unwrap();
        let result = interp
            .call_instance_method(&mut inst, "get_count", &[])
            .unwrap();
        assert_eq!(result, Variant::Int(3));
        assert_eq!(inst.properties.get("count"), Some(&Variant::Int(3)));
    }

    // -----------------------------------------------------------------------
    // Compound member assignment (self.position.x = value)
    // -----------------------------------------------------------------------

    #[test]
    fn compound_member_assign_self_position_x() {
        let (mut interp, class_def) = run_class(
            "\
var position = Vector2(100, 200)
func set_x(val):
    self.position.x = val
    return self.position
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let result = interp
            .call_instance_method(&mut inst, "set_x", &[Variant::Float(5.0)])
            .unwrap();
        assert_eq!(result, Variant::Vector2(Vector2::new(5.0, 200.0)));
    }

    #[test]
    fn compound_member_assign_self_position_y() {
        let (mut interp, class_def) = run_class(
            "\
var position = Vector2(100, 200)
func set_y(val):
    self.position.y = val
    return self.position
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let result = interp
            .call_instance_method(&mut inst, "set_y", &[Variant::Float(42.0)])
            .unwrap();
        assert_eq!(result, Variant::Vector2(Vector2::new(100.0, 42.0)));
    }

    #[test]
    fn compound_member_add_assign_self_position_x() {
        let (mut interp, class_def) = run_class(
            "\
var position = Vector2(100, 200)
func move_x(amount):
    self.position.x += amount
    return self.position
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let result = interp
            .call_instance_method(&mut inst, "move_x", &[Variant::Float(50.0)])
            .unwrap();
        assert_eq!(result, Variant::Vector2(Vector2::new(150.0, 200.0)));
    }

    #[test]
    fn compound_member_add_assign_bare_position_x() {
        let (mut interp, class_def) = run_class(
            "\
var position = Vector2(100, 200)
func move_x(amount):
    position.x += amount
    return position
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let result = interp
            .call_instance_method(&mut inst, "move_x", &[Variant::Float(50.0)])
            .unwrap();
        assert_eq!(result, Variant::Vector2(Vector2::new(150.0, 200.0)));
    }

    #[test]
    fn compound_member_assign_both_x_and_y() {
        let (mut interp, class_def) = run_class(
            "\
var position = Vector2(10, 20)
func set_both(x, y):
    self.position.x = x
    self.position.y = y
    return self.position
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let result = interp
            .call_instance_method(
                &mut inst,
                "set_both",
                &[Variant::Float(99.0), Variant::Float(77.0)],
            )
            .unwrap();
        assert_eq!(result, Variant::Vector2(Vector2::new(99.0, 77.0)));
    }

    #[test]
    fn compound_member_assign_vector3_components() {
        let (mut interp, class_def) = run_class(
            "\
var velocity = Vector3(1, 2, 3)
func set_z(val):
    self.velocity.z = val
    return self.velocity
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let result = interp
            .call_instance_method(&mut inst, "set_z", &[Variant::Float(99.0)])
            .unwrap();
        assert_eq!(result, Variant::Vector3(Vector3::new(1.0, 2.0, 99.0)));
    }

    #[test]
    fn compound_member_assign_color_components() {
        let (mut interp, class_def) = run_class(
            "\
var tint = Color(0.1, 0.2, 0.3, 1.0)
func set_red(val):
    self.tint.r = val
    return self.tint
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let result = interp
            .call_instance_method(&mut inst, "set_red", &[Variant::Float(0.9)])
            .unwrap();
        if let Variant::Color(c) = result {
            assert!((c.r - 0.9).abs() < 0.001);
            assert!((c.g - 0.2).abs() < 0.001);
            assert!((c.b - 0.3).abs() < 0.001);
            assert!((c.a - 1.0).abs() < 0.001);
        } else {
            panic!("expected Color, got {:?}", result);
        }
    }

    #[test]
    fn compound_member_read_modify_write() {
        let (mut interp, class_def) = run_class(
            "\
var position = Vector2(100, 200)
func move_by(delta):
    self.position.x = self.position.x + delta
    return self.position
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let result = interp
            .call_instance_method(&mut inst, "move_by", &[Variant::Float(15.0)])
            .unwrap();
        assert_eq!(result, Variant::Vector2(Vector2::new(115.0, 200.0)));
    }

    #[test]
    fn compound_member_read_modify_write_bare_position() {
        let (mut interp, class_def) = run_class(
            "\
var position = Vector2(100, 200)
func move_by(delta):
    position.x = position.x + delta
    return position
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let result = interp
            .call_instance_method(&mut inst, "move_by", &[Variant::Float(15.0)])
            .unwrap();
        assert_eq!(result, Variant::Vector2(Vector2::new(115.0, 200.0)));
    }

    #[test]
    fn compound_member_preserves_other_components() {
        let (mut interp, class_def) = run_class(
            "\
var position = Vector2(10, 20)
func modify():
    self.position.x = 999.0
    return self.position.y
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let result = interp
            .call_instance_method(&mut inst, "modify", &[])
            .unwrap();
        assert_eq!(result, Variant::Float(20.0));
    }

    #[test]
    fn compound_member_multiple_frames_accumulation() {
        let (mut interp, class_def) = run_class(
            "\
var position = Vector2(0, 0)
func tick(speed):
    self.position.x += speed
    return self.position
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let r1 = interp
            .call_instance_method(&mut inst, "tick", &[Variant::Float(10.0)])
            .unwrap();
        assert_eq!(r1, Variant::Vector2(Vector2::new(10.0, 0.0)));
        let r2 = interp
            .call_instance_method(&mut inst, "tick", &[Variant::Float(10.0)])
            .unwrap();
        assert_eq!(r2, Variant::Vector2(Vector2::new(20.0, 0.0)));
        let r3 = interp
            .call_instance_method(&mut inst, "tick", &[Variant::Float(10.0)])
            .unwrap();
        assert_eq!(r3, Variant::Vector2(Vector2::new(30.0, 0.0)));
    }

    #[test]
    fn compound_member_sub_assign() {
        let (mut interp, class_def) = run_class(
            "\
var position = Vector2(100, 200)
func move_left(amount):
    self.position.x -= amount
    return self.position
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let result = interp
            .call_instance_method(&mut inst, "move_left", &[Variant::Float(30.0)])
            .unwrap();
        assert_eq!(result, Variant::Vector2(Vector2::new(70.0, 200.0)));
    }

    #[test]
    fn compound_member_assign_vector3_all_components() {
        let (mut interp, class_def) = run_class(
            "\
var vel = Vector3(1, 2, 3)
func set_all(x, y, z):
    self.vel.x = x
    self.vel.y = y
    self.vel.z = z
    return self.vel
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let result = interp
            .call_instance_method(
                &mut inst,
                "set_all",
                &[
                    Variant::Float(10.0),
                    Variant::Float(20.0),
                    Variant::Float(30.0),
                ],
            )
            .unwrap();
        assert_eq!(result, Variant::Vector3(Vector3::new(10.0, 20.0, 30.0)));
    }

    #[test]
    fn compound_member_assign_color_alpha() {
        let (mut interp, class_def) = run_class(
            "\
var color = Color(1.0, 1.0, 1.0, 1.0)
func fade(alpha):
    self.color.a = alpha
    return self.color
",
        );
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let result = interp
            .call_instance_method(&mut inst, "fade", &[Variant::Float(0.5)])
            .unwrap();
        if let Variant::Color(c) = result {
            assert!((c.a - 0.5).abs() < 0.001);
            assert!((c.r - 1.0).abs() < 0.001);
        } else {
            panic!("expected Color, got {:?}", result);
        }
    }

    // -----------------------------------------------------------------------
    // deg_to_rad / rad_to_deg
    // -----------------------------------------------------------------------

    #[test]
    fn builtin_deg_to_rad() {
        let r = run("var x = deg_to_rad(180.0)\n");
        assert!(r.output.is_empty());
    }

    #[test]
    fn builtin_deg_to_rad_value() {
        let src = "var x = deg_to_rad(180.0)\nprint(x)\n";
        let r = run(src);
        let val: f64 = r.output[0].parse().unwrap();
        assert!(
            (val - std::f64::consts::PI).abs() < 1e-6,
            "expected PI, got {val}"
        );
    }

    #[test]
    fn builtin_rad_to_deg_value() {
        let src = "var x = rad_to_deg(3.14159265358979)\nprint(x)\n";
        let r = run(src);
        let val: f64 = r.output[0].parse().unwrap();
        assert!((val - 180.0).abs() < 0.01, "expected 180, got {val}");
    }

    #[test]
    fn builtin_deg_to_rad_zero() {
        let src = "var x = deg_to_rad(0)\nprint(x)\n";
        let r = run(src);
        let val: f64 = r.output[0].parse().unwrap();
        assert!(val.abs() < 1e-10, "expected 0, got {val}");
    }

    #[test]
    fn builtin_rad_to_deg_zero() {
        let src = "var x = rad_to_deg(0)\nprint(x)\n";
        let r = run(src);
        let val: f64 = r.output[0].parse().unwrap();
        assert!(val.abs() < 1e-10, "expected 0, got {val}");
    }

    #[test]
    fn builtin_deg_to_rad_90() {
        let src = "var x = deg_to_rad(90.0)\nprint(x)\n";
        let r = run(src);
        let val: f64 = r.output[0].parse().unwrap();
        assert!(
            (val - std::f64::consts::FRAC_PI_2).abs() < 1e-6,
            "expected PI/2, got {val}"
        );
    }

    #[test]
    fn builtin_move_toward_basic() {
        let src = "return move_toward(0.0, 10.0, 3.0)\n";
        assert_eq!(run_val(src), Variant::Float(3.0));
    }

    #[test]
    fn builtin_move_toward_overshoot() {
        let src = "return move_toward(8.0, 10.0, 5.0)\n";
        assert_eq!(run_val(src), Variant::Float(10.0));
    }

    #[test]
    fn builtin_move_toward_negative() {
        let src = "return move_toward(5.0, 0.0, 2.0)\n";
        assert_eq!(run_val(src), Variant::Float(3.0));
    }

    #[test]
    fn builtin_move_toward_already_at_target() {
        let src = "return move_toward(5.0, 5.0, 1.0)\n";
        assert_eq!(run_val(src), Variant::Float(5.0));
    }

    // -- Input singleton tests -----------------------------------------------

    /// Mock SceneAccess that provides input state for testing Input.* calls.
    struct MockInputAccess {
        pressed_actions: std::collections::HashSet<String>,
        just_pressed_actions: std::collections::HashSet<String>,
        pressed_keys: std::collections::HashSet<String>,
        action_map: std::collections::HashMap<String, Vec<String>>,
    }

    impl MockInputAccess {
        fn new() -> Self {
            Self {
                pressed_actions: std::collections::HashSet::new(),
                just_pressed_actions: std::collections::HashSet::new(),
                pressed_keys: std::collections::HashSet::new(),
                action_map: std::collections::HashMap::new(),
            }
        }

        fn press_action(mut self, action: &str) -> Self {
            self.pressed_actions.insert(action.to_string());
            self
        }

        fn just_press_action(mut self, action: &str) -> Self {
            self.just_pressed_actions.insert(action.to_string());
            self.pressed_actions.insert(action.to_string());
            self
        }

        fn press_key(mut self, key: &str) -> Self {
            self.pressed_keys.insert(key.to_string());
            self
        }
    }

    impl SceneAccess for MockInputAccess {
        fn get_node(&self, _from: u64, _path: &str) -> Option<u64> {
            None
        }
        fn get_parent(&self, _node: u64) -> Option<u64> {
            None
        }
        fn get_children(&self, _node: u64) -> Vec<u64> {
            vec![]
        }
        fn get_node_property(&self, _node: u64, _prop: &str) -> Variant {
            Variant::Nil
        }
        fn set_node_property(&mut self, _node: u64, _prop: &str, _value: Variant) {}
        fn emit_signal(&mut self, _node: u64, _signal: &str, _args: &[Variant]) {}
        fn connect_signal(&mut self, _source: u64, _signal: &str, _target: u64, _method: &str) {}
        fn get_node_name(&self, _node: u64) -> Option<String> {
            None
        }

        fn is_input_action_pressed(&self, action: &str) -> bool {
            self.pressed_actions.contains(action)
        }

        fn is_input_action_just_pressed(&self, action: &str) -> bool {
            self.just_pressed_actions.contains(action)
        }

        fn is_input_key_pressed(&self, key: &str) -> bool {
            self.pressed_keys.contains(key)
        }
    }

    fn run_class_with_input(
        src: &str,
        access: MockInputAccess,
    ) -> (Interpreter, ClassDef, ClassInstance) {
        let mut interp = Interpreter::new();
        let class_def = interp.run_class(src).unwrap();
        let instance = interp.instantiate_class(&class_def).unwrap();
        interp.set_scene_access(Box::new(access), 1);
        (interp, class_def, instance)
    }

    // pat-u3l8c.22: mouse mode, programmatic actions, joypad enumeration and
    // vibration dispatched from GDScript's Input singleton to SceneAccess.
    #[derive(Default)]
    struct InputCmdState {
        mouse_mode: i64,
        warped_to: Option<(f32, f32)>,
        pressed: std::collections::HashSet<String>,
        released: std::collections::HashSet<String>,
        joypads: Vec<i64>,
        vibrations: std::collections::HashMap<i64, (f32, f32, f32)>,
    }

    struct InputCmdAccess {
        state: std::rc::Rc<std::cell::RefCell<InputCmdState>>,
    }

    impl SceneAccess for InputCmdAccess {
        fn get_node(&self, _from: u64, _path: &str) -> Option<u64> {
            None
        }
        fn get_parent(&self, _node: u64) -> Option<u64> {
            None
        }
        fn get_children(&self, _node: u64) -> Vec<u64> {
            vec![]
        }
        fn get_node_property(&self, _node: u64, _prop: &str) -> Variant {
            Variant::Nil
        }
        fn set_node_property(&mut self, _node: u64, _prop: &str, _value: Variant) {}
        fn emit_signal(&mut self, _node: u64, _signal: &str, _args: &[Variant]) {}
        fn connect_signal(&mut self, _source: u64, _signal: &str, _target: u64, _method: &str) {}
        fn get_node_name(&self, _node: u64) -> Option<String> {
            None
        }

        fn set_mouse_mode(&mut self, mode: i64) {
            self.state.borrow_mut().mouse_mode = mode;
        }
        fn get_mouse_mode(&self) -> i64 {
            self.state.borrow().mouse_mode
        }
        fn warp_mouse(&mut self, x: f32, y: f32) {
            self.state.borrow_mut().warped_to = Some((x, y));
        }
        fn action_press(&mut self, action: &str, _strength: f32) {
            self.state.borrow_mut().pressed.insert(action.to_string());
        }
        fn action_release(&mut self, action: &str) {
            let mut st = self.state.borrow_mut();
            st.pressed.remove(action);
            st.released.insert(action.to_string());
        }
        fn get_connected_joypads(&self) -> Vec<i64> {
            self.state.borrow().joypads.clone()
        }
        fn start_joy_vibration(&mut self, device: i64, weak: f32, strong: f32, duration: f32) {
            self.state
                .borrow_mut()
                .vibrations
                .insert(device, (weak, strong, duration));
        }
        fn stop_joy_vibration(&mut self, device: i64) {
            self.state.borrow_mut().vibrations.remove(&device);
        }
    }

    fn input_cmd_interp() -> (Interpreter, std::rc::Rc<std::cell::RefCell<InputCmdState>>) {
        let state = std::rc::Rc::new(std::cell::RefCell::new(InputCmdState::default()));
        let mut interp = Interpreter::new();
        interp.set_scene_access(
            Box::new(InputCmdAccess {
                state: state.clone(),
            }),
            1,
        );
        (interp, state)
    }

    #[test]
    fn input_set_and_get_mouse_mode() {
        let (mut interp, state) = input_cmd_interp();
        let r = interp
            .run("Input.set_mouse_mode(2)\nreturn Input.get_mouse_mode()\n")
            .unwrap();
        assert_eq!(r.return_value, Some(Variant::Int(2)));
        assert_eq!(state.borrow().mouse_mode, 2);
    }

    #[test]
    fn input_action_press_and_release() {
        let (mut interp, state) = input_cmd_interp();
        interp
            .run("Input.action_press(\"jump\")\nInput.action_press(\"shoot\", 0.5)\nInput.action_release(\"jump\")\n")
            .unwrap();
        let st = state.borrow();
        assert!(st.pressed.contains("shoot"), "shoot pressed");
        assert!(!st.pressed.contains("jump"), "jump released");
        assert!(st.released.contains("jump"), "jump release recorded");
    }

    #[test]
    fn input_get_connected_joypads() {
        let (mut interp, state) = input_cmd_interp();
        state.borrow_mut().joypads = vec![0, 2];
        let r = interp
            .run("return Input.get_connected_joypads()\n")
            .unwrap();
        assert_eq!(
            r.return_value,
            Some(Variant::Array(vec![Variant::Int(0), Variant::Int(2)]))
        );
    }

    #[test]
    fn input_joy_vibration_start_stop() {
        let (mut interp, state) = input_cmd_interp();
        interp
            .run("Input.start_joy_vibration(0, 0.5, 0.9, 1.0)\n")
            .unwrap();
        assert_eq!(state.borrow().vibrations.get(&0), Some(&(0.5, 0.9, 1.0)));
        interp.run("Input.stop_joy_vibration(0)\n").unwrap();
        assert!(state.borrow().vibrations.get(&0).is_none());
    }

    #[test]
    fn input_warp_mouse() {
        let (mut interp, state) = input_cmd_interp();
        interp.run("Input.warp_mouse(Vector2(10, 20))\n").unwrap();
        assert_eq!(state.borrow().warped_to, Some((10.0, 20.0)));
    }

    #[test]
    fn input_is_action_pressed_true() {
        let src = "\
extends Node2D
var moving = false
func _process(delta):
    if Input.is_action_pressed(\"ui_right\"):
        self.moving = true
";
        let access = MockInputAccess::new().press_action("ui_right");
        let (mut interp, _, mut inst) = run_class_with_input(src, access);
        interp
            .call_instance_method(&mut inst, "_process", &[Variant::Float(0.016)])
            .unwrap();
        assert_eq!(inst.properties.get("moving"), Some(&Variant::Bool(true)));
    }

    #[test]
    fn input_is_action_pressed_false() {
        let src = "\
extends Node2D
var moving = false
func _process(delta):
    if Input.is_action_pressed(\"ui_right\"):
        self.moving = true
";
        let access = MockInputAccess::new(); // nothing pressed
        let (mut interp, _, mut inst) = run_class_with_input(src, access);
        interp
            .call_instance_method(&mut inst, "_process", &[Variant::Float(0.016)])
            .unwrap();
        assert_eq!(inst.properties.get("moving"), Some(&Variant::Bool(false)));
    }

    #[test]
    fn input_is_action_just_pressed_true() {
        let src = "\
extends Node2D
var fired = false
func _process(delta):
    if Input.is_action_just_pressed(\"shoot\"):
        self.fired = true
";
        let access = MockInputAccess::new().just_press_action("shoot");
        let (mut interp, _, mut inst) = run_class_with_input(src, access);
        interp
            .call_instance_method(&mut inst, "_process", &[Variant::Float(0.016)])
            .unwrap();
        assert_eq!(inst.properties.get("fired"), Some(&Variant::Bool(true)));
    }

    #[test]
    fn input_is_action_just_pressed_false_when_only_held() {
        let src = "\
extends Node2D
var fired = false
func _process(delta):
    if Input.is_action_just_pressed(\"shoot\"):
        self.fired = true
";
        // Action is pressed (held) but NOT just-pressed (not first frame)
        let access = MockInputAccess::new().press_action("shoot");
        let (mut interp, _, mut inst) = run_class_with_input(src, access);
        interp
            .call_instance_method(&mut inst, "_process", &[Variant::Float(0.016)])
            .unwrap();
        assert_eq!(inst.properties.get("fired"), Some(&Variant::Bool(false)));
    }

    #[test]
    fn input_is_key_pressed() {
        let src = "\
extends Node2D
var result = false
func _process(delta):
    if Input.is_key_pressed(\"ArrowLeft\"):
        self.result = true
";
        let access = MockInputAccess::new().press_key("ArrowLeft");
        let (mut interp, _, mut inst) = run_class_with_input(src, access);
        interp
            .call_instance_method(&mut inst, "_process", &[Variant::Float(0.016)])
            .unwrap();
        assert_eq!(inst.properties.get("result"), Some(&Variant::Bool(true)));
    }

    #[test]
    fn input_get_vector_right() {
        let src = "\
extends Node2D
var dir_x = 0.0
var dir_y = 0.0
func _process(delta):
    var dir = Input.get_vector(\"ui_left\", \"ui_right\", \"ui_up\", \"ui_down\")
    self.dir_x = dir.x
    self.dir_y = dir.y
";
        let access = MockInputAccess::new().press_action("ui_right");
        let (mut interp, _, mut inst) = run_class_with_input(src, access);
        interp
            .call_instance_method(&mut inst, "_process", &[Variant::Float(0.016)])
            .unwrap();
        assert_eq!(inst.properties.get("dir_x"), Some(&Variant::Float(1.0)));
        assert_eq!(inst.properties.get("dir_y"), Some(&Variant::Float(0.0)));
    }

    #[test]
    fn input_get_vector_diagonal_normalized() {
        let src = "\
extends Node2D
var dir_x = 0.0
var dir_y = 0.0
func _process(delta):
    var dir = Input.get_vector(\"ui_left\", \"ui_right\", \"ui_up\", \"ui_down\")
    self.dir_x = dir.x
    self.dir_y = dir.y
";
        let access = MockInputAccess::new()
            .press_action("ui_right")
            .press_action("ui_down");
        let (mut interp, _, mut inst) = run_class_with_input(src, access);
        interp
            .call_instance_method(&mut inst, "_process", &[Variant::Float(0.016)])
            .unwrap();
        let dx = match inst.properties.get("dir_x") {
            Some(Variant::Float(f)) => *f,
            other => panic!("expected Float, got {:?}", other),
        };
        let dy = match inst.properties.get("dir_y") {
            Some(Variant::Float(f)) => *f,
            other => panic!("expected Float, got {:?}", other),
        };
        let len = (dx * dx + dy * dy).sqrt();
        assert!(
            (len - 1.0).abs() < 1e-6,
            "diagonal vector should be normalized, got len={len}"
        );
        assert!(dx > 0.0, "should move right");
        assert!(dy > 0.0, "should move down");
    }

    #[test]
    fn input_get_vector_no_input() {
        let src = "\
extends Node2D
var dir_x = 99.0
var dir_y = 99.0
func _process(delta):
    var dir = Input.get_vector(\"ui_left\", \"ui_right\", \"ui_up\", \"ui_down\")
    self.dir_x = dir.x
    self.dir_y = dir.y
";
        let access = MockInputAccess::new(); // nothing pressed
        let (mut interp, _, mut inst) = run_class_with_input(src, access);
        interp
            .call_instance_method(&mut inst, "_process", &[Variant::Float(0.016)])
            .unwrap();
        assert_eq!(inst.properties.get("dir_x"), Some(&Variant::Float(0.0)));
        assert_eq!(inst.properties.get("dir_y"), Some(&Variant::Float(0.0)));
    }

    #[test]
    fn input_nonexistent_method_errors() {
        let src = "\
extends Node2D
func _process(delta):
    Input.nonexistent_method()
";
        let access = MockInputAccess::new();
        let (mut interp, _, mut inst) = run_class_with_input(src, access);
        let result = interp.call_instance_method(&mut inst, "_process", &[Variant::Float(0.016)]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err.kind, RuntimeErrorKind::UndefinedFunction(ref name) if name == "Input.nonexistent_method"),
            "expected UndefinedFunction for Input.nonexistent_method, got {:?}",
            err.kind
        );
    }

    #[test]
    fn input_without_scene_access_returns_false() {
        // When no scene access is set, Input methods should return false/zero
        let src = "\
extends Node2D
var result = true
func _process(delta):
    self.result = Input.is_action_pressed(\"ui_right\")
";
        let (mut interp, class_def) = run_class(src);
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        // No scene_access set
        interp
            .call_instance_method(&mut inst, "_process", &[Variant::Float(0.016)])
            .unwrap();
        assert_eq!(inst.properties.get("result"), Some(&Variant::Bool(false)));
    }

    // -- pat-13d: Typed arrays in classes ------------------------------------

    #[test]
    fn class_typed_array_var() {
        let (_, class_def) = run_class("var items: Array[int] = [1, 2, 3]\n");
        assert_eq!(class_def.instance_vars[0].name, "items");
        assert_eq!(
            class_def.instance_vars[0].type_hint.as_deref(),
            Some("Array[int]")
        );
    }

    #[test]
    fn class_typed_array_var_node() {
        let (_, class_def) = run_class("var children: Array[Node]\n");
        assert_eq!(
            class_def.instance_vars[0].type_hint.as_deref(),
            Some("Array[Node]")
        );
    }

    #[test]
    fn class_typed_array_export() {
        let (_, class_def) = run_class("@export\nvar speeds: Array[float] = []\n");
        assert_eq!(class_def.exports[0].name, "speeds");
        assert_eq!(
            class_def.exports[0].type_hint.as_deref(),
            Some("Array[float]")
        );
    }

    #[test]
    fn class_typed_array_method_param_accepted() {
        // Typed array params should parse without error in class methods
        let mut interp = Interpreter::new();
        let class_def = interp
            .run_class("class_name Sorter\nfunc sort(items: Array[int]):\n    return items\n")
            .unwrap();
        assert!(class_def.methods.contains_key("sort"));
        assert_eq!(class_def.methods["sort"].params, vec!["items"]);
    }

    #[test]
    fn typed_array_var_runs() {
        let mut interp = Interpreter::new();
        let result = interp
            .run("var items: Array[int] = [1, 2, 3]\nreturn items[0]\n")
            .unwrap();
        assert_eq!(result.return_value, Some(Variant::Int(1)));
    }

    // -- pat-c13: Inner classes in interpreter --------------------------------

    #[test]
    fn class_inner_class_parsed() {
        let (_, class_def) = run_class("class State:\n    var name = \"idle\"\n");
        assert!(class_def.inner_classes.contains_key("State"));
        let inner = &class_def.inner_classes["State"];
        assert_eq!(inner.name.as_deref(), Some("State"));
        assert_eq!(inner.instance_vars.len(), 1);
        assert_eq!(inner.instance_vars[0].name, "name");
    }

    #[test]
    fn class_inner_class_with_method() {
        let (_, class_def) =
            run_class("class Enemy:\n    var hp = 10\n    func get_hp():\n        return hp\n");
        let inner = &class_def.inner_classes["Enemy"];
        assert_eq!(inner.instance_vars.len(), 1);
        assert!(inner.methods.contains_key("get_hp"));
    }

    #[test]
    fn class_inner_class_with_signal() {
        let (_, class_def) = run_class("class EventBus:\n    signal fired\n    var count = 0\n");
        let inner = &class_def.inner_classes["EventBus"];
        assert_eq!(inner.signals, vec!["fired"]);
        assert_eq!(inner.instance_vars.len(), 1);
    }

    #[test]
    fn class_multiple_inner_classes() {
        let src = "\
class StateA:
    var x = 1
class StateB:
    var y = 2
";
        let (_, class_def) = run_class(src);
        assert_eq!(class_def.inner_classes.len(), 2);
        assert!(class_def.inner_classes.contains_key("StateA"));
        assert!(class_def.inner_classes.contains_key("StateB"));
    }

    #[test]
    fn class_inner_class_alongside_vars() {
        let src = "\
extends Node
var health = 100
class State:
    var name = \"idle\"
func _ready():
    pass
";
        let (_, class_def) = run_class(src);
        assert_eq!(class_def.instance_vars.len(), 1);
        assert_eq!(class_def.instance_vars[0].name, "health");
        assert!(class_def.inner_classes.contains_key("State"));
        assert!(class_def.methods.contains_key("_ready"));
    }

    #[test]
    fn class_inner_class_not_in_instance_vars() {
        let (_, class_def) = run_class("class Foo:\n    var x = 1\nvar y = 2\n");
        // Inner class vars should not leak into outer class instance_vars
        assert_eq!(class_def.instance_vars.len(), 1);
        assert_eq!(class_def.instance_vars[0].name, "y");
    }

    #[test]
    fn class_inner_class_is_tool_false() {
        let (_, class_def) = run_class("class Inner:\n    var x = 1\n");
        let inner = &class_def.inner_classes["Inner"];
        assert!(!inner.is_tool);
    }

    #[test]
    fn class_inner_class_empty_methods() {
        let (_, class_def) = run_class("class Empty:\n    pass\n");
        let inner = &class_def.inner_classes["Empty"];
        assert!(inner.methods.is_empty());
        assert!(inner.instance_vars.is_empty());
    }

    // -- pat-916: @tool scripts in interpreter --------------------------------

    #[test]
    fn class_tool_script_is_tool_true() {
        let (_, class_def) = run_class("@tool\nextends Node\n");
        assert!(class_def.is_tool);
    }

    #[test]
    fn class_non_tool_script_is_tool_false() {
        let (_, class_def) = run_class("extends Node\n");
        assert!(!class_def.is_tool);
    }

    #[test]
    fn class_tool_with_class_name() {
        let (_, class_def) = run_class("@tool\nextends Node\nclass_name MyTool\n");
        assert!(class_def.is_tool);
        assert_eq!(class_def.name.as_deref(), Some("MyTool"));
        assert_eq!(class_def.parent_class.as_deref(), Some("Node"));
    }

    #[test]
    fn class_tool_with_vars_and_methods() {
        let src = "\
@tool
extends Node
var speed = 10.0
func _process(delta):
    pass
";
        let (_, class_def) = run_class(src);
        assert!(class_def.is_tool);
        assert_eq!(class_def.instance_vars.len(), 1);
        assert!(class_def.methods.contains_key("_process"));
    }

    #[test]
    fn class_tool_does_not_change_runtime() {
        // @tool scripts should still execute normally
        let src = "\
@tool
extends Node
var x = 42
func get_x():
    return self.x
";
        let mut interp = Interpreter::new();
        let class_def = interp.run_class(src).unwrap();
        assert!(class_def.is_tool);
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let result = interp
            .call_instance_method(&mut inst, "get_x", &[])
            .unwrap();
        assert_eq!(result, Variant::Int(42));
    }

    // ----- await / coroutines (pat-p0s82.7) ---------------------------------

    #[test]
    fn await_timer_suspends_and_resumes_across_poll() {
        let mut interp = Interpreter::new();
        interp
            .run(
                r#"
func go():
    print("start")
    await create_timer(0.5)
    print("after")
"#,
            )
            .unwrap();
        interp.call_user_func("go", &[]).unwrap();
        assert!(
            interp.output.iter().any(|l| l == "start"),
            "must print before await: {:?}",
            interp.output
        );
        assert!(
            !interp.output.iter().any(|l| l == "after"),
            "must NOT print after await before poll: {:?}",
            interp.output
        );
        assert_eq!(interp.suspended_coroutine_count(), 1);

        // Partial poll — still waiting
        interp.poll_coroutines(0.2).unwrap();
        assert!(
            !interp.output.iter().any(|l| l == "after"),
            "still waiting after 0.2s"
        );
        assert_eq!(interp.suspended_coroutine_count(), 1);

        // Complete the wait
        interp.poll_coroutines(0.4).unwrap();
        assert!(
            interp.output.iter().any(|l| l == "after"),
            "must resume after full timer: {:?}",
            interp.output
        );
        assert_eq!(interp.suspended_coroutine_count(), 0);
        // Must not have emitted the old sync-warning.
        assert!(
            !interp
                .output
                .iter()
                .any(|l| l.contains("coroutines not fully supported")),
            "sync await warning must be gone"
        );
    }

    #[test]
    fn await_signal_suspends_until_notify() {
        let mut interp = Interpreter::new();
        interp
            .run(
                r#"
func worker():
    print("waiting")
    await wait_signal("ready")
    print("got_it")
"#,
            )
            .unwrap();
        interp.call_user_func("worker", &[]).unwrap();
        assert!(interp.output.iter().any(|l| l == "waiting"));
        assert!(!interp.output.iter().any(|l| l == "got_it"));
        assert_eq!(interp.suspended_coroutine_count(), 1);

        interp.notify_await_signal("ready", &[]).unwrap();
        assert!(
            interp.output.iter().any(|l| l == "got_it"),
            "signal must resume: {:?}",
            interp.output
        );
        assert_eq!(interp.suspended_coroutine_count(), 0);
    }

    #[test]
    fn concurrent_coroutines_on_different_timers() {
        let mut interp = Interpreter::new();
        interp
            .run(
                r#"
func a():
    print("a1")
    await create_timer(0.1)
    print("a2")
func b():
    print("b1")
    await create_timer(0.3)
    print("b2")
"#,
            )
            .unwrap();
        interp.call_user_func("a", &[]).unwrap();
        interp.call_user_func("b", &[]).unwrap();
        assert_eq!(interp.suspended_coroutine_count(), 2);

        interp.poll_coroutines(0.15).unwrap();
        assert!(interp.output.iter().any(|l| l == "a2"));
        assert!(!interp.output.iter().any(|l| l == "b2"));
        assert_eq!(interp.suspended_coroutine_count(), 1);

        interp.poll_coroutines(0.2).unwrap();
        assert!(interp.output.iter().any(|l| l == "b2"));
        assert_eq!(interp.suspended_coroutine_count(), 0);
    }

    #[test]
    fn await_frames_and_instance_method() {
        let mut interp = Interpreter::new();
        let class_def = interp
            .run_class(
                r#"
extends Node
func start():
    print("f1")
    await wait_frames(2)
    print("f2")
"#,
            )
            .unwrap();
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        interp
            .call_instance_method(&mut inst, "start", &[])
            .unwrap();
        assert!(interp.output.iter().any(|l| l == "f1"));
        assert!(!interp.output.iter().any(|l| l == "f2"));
        interp.tick_coroutine_frames().unwrap();
        assert!(!interp.output.iter().any(|l| l == "f2"));
        interp.tick_coroutine_frames().unwrap();
        assert!(
            interp.output.iter().any(|l| l == "f2"),
            "frame await resume: {:?}",
            interp.output
        );
    }

    // =======================================================================
    // pat-u3l8c.7 — const / assert / is / as
    // =======================================================================

    fn run_float(src: &str) -> f64 {
        match run_val(src) {
            Variant::Float(f) => f,
            Variant::Int(i) => i as f64,
            other => panic!("expected number, got {other:?}"),
        }
    }

    #[test]
    fn const_decl_and_use() {
        assert_eq!(run_val("const MAX = 5\nreturn MAX + 1\n"), Variant::Int(6));
    }

    #[test]
    fn const_reassignment_is_error() {
        let err = run_err("const MAX = 5\nMAX = 6\n");
        assert!(matches!(err.kind, RuntimeErrorKind::TypeError(_)));
        assert!(err.to_string().contains("constant"));
    }

    #[test]
    fn const_local_in_function() {
        let src = "\
func f():
    const K = 3
    return K * 2
return f()
";
        assert_eq!(run_val(src), Variant::Int(6));
    }

    #[test]
    fn class_level_const_visible_in_method() {
        let mut interp = Interpreter::new();
        let class_def = interp
            .run_class(
                "\
const SPEED = 42
func get_speed():
    return SPEED
",
            )
            .unwrap();
        let mut inst = interp.instantiate_class(&class_def).unwrap();
        let r = interp
            .call_instance_method(&mut inst, "get_speed", &[])
            .unwrap();
        assert_eq!(r, Variant::Int(42));
    }

    #[test]
    fn assert_true_passes() {
        assert_eq!(run_val("assert(true)\nreturn 7\n"), Variant::Int(7));
        assert_eq!(
            run_val("assert(1 < 2, \"ok\")\nreturn 7\n"),
            Variant::Int(7)
        );
    }

    #[test]
    fn assert_false_raises_with_message() {
        let err = run_err("assert(false, \"boom\")\n");
        match err.kind {
            RuntimeErrorKind::AssertionFailed(msg) => assert_eq!(msg, "boom"),
            other => panic!("expected AssertionFailed, got {other:?}"),
        }
    }

    #[test]
    fn is_operator_builtin_types() {
        assert_eq!(run_val("return 5 is int\n"), Variant::Bool(true));
        assert_eq!(run_val("return 5 is float\n"), Variant::Bool(false));
        assert_eq!(run_val("return 5.0 is float\n"), Variant::Bool(true));
        assert_eq!(run_val("return \"hi\" is String\n"), Variant::Bool(true));
        assert_eq!(run_val("return [1, 2] is Array\n"), Variant::Bool(true));
        assert_eq!(
            run_val("return {\"a\": 1} is Dictionary\n"),
            Variant::Bool(true)
        );
        assert_eq!(run_val("return 5 is String\n"), Variant::Bool(false));
    }

    #[test]
    fn as_operator_numeric_coercion() {
        assert_eq!(run_val("return 5 as float\n"), Variant::Float(5.0));
        assert_eq!(run_val("return 5.7 as int\n"), Variant::Int(5));
        // No-op when the type already matches.
        assert_eq!(run_val("return 5 as int\n"), Variant::Int(5));
        assert_eq!(run_val("return 2.5 as float\n"), Variant::Float(2.5));
    }

    // =======================================================================
    // pat-u3l8c.8 — compound assignment / bitwise / power
    // =======================================================================

    #[test]
    fn bitwise_operators() {
        assert_eq!(run_val("return 6 & 3\n"), Variant::Int(2));
        assert_eq!(run_val("return 6 | 1\n"), Variant::Int(7));
        assert_eq!(run_val("return 6 ^ 3\n"), Variant::Int(5));
        assert_eq!(run_val("return 1 << 4\n"), Variant::Int(16));
        assert_eq!(run_val("return 16 >> 2\n"), Variant::Int(4));
        assert_eq!(run_val("return ~0\n"), Variant::Int(-1));
    }

    #[test]
    fn bitwise_precedence_below_comparison_above_addition() {
        // `1 | 2 & 2` == `1 | (2 & 2)` == 3
        assert_eq!(run_val("return 1 | 2 & 2\n"), Variant::Int(3));
        // `&` binds looser than `+`: `1 & 2 + 1` == `1 & 3` == 1
        assert_eq!(run_val("return 1 & 2 + 1\n"), Variant::Int(1));
    }

    #[test]
    fn power_operator() {
        assert_eq!(run_val("return 2 ** 10\n"), Variant::Int(1024));
        assert_eq!(run_val("return 2.0 ** 3\n"), Variant::Float(8.0));
        assert_eq!(run_val("return 2 ** -1\n"), Variant::Float(0.5));
        // Power binds tighter than unary minus: -2 ** 2 == -4.
        assert_eq!(run_val("return -2 ** 2\n"), Variant::Int(-4));
        // Left-associative (matching Godot's implementation): (2**3)**2 == 64.
        assert_eq!(run_val("return 2 ** 3 ** 2\n"), Variant::Int(64));
    }

    #[test]
    fn power_tighter_than_multiplication() {
        // 2 * 3 ** 2 == 2 * 9 == 18
        assert_eq!(run_val("return 2 * 3 ** 2\n"), Variant::Int(18));
    }

    #[test]
    fn bitwise_requires_int() {
        let err = run_err("return 1.5 & 2\n");
        assert!(matches!(err.kind, RuntimeErrorKind::TypeError(_)));
    }

    #[test]
    fn compound_assignments_on_locals() {
        assert_eq!(run_val("var x = 10\nx *= 3\nreturn x\n"), Variant::Int(30));
        assert_eq!(run_val("var x = 12\nx /= 4\nreturn x\n"), Variant::Int(3));
        assert_eq!(run_val("var x = 10\nx %= 3\nreturn x\n"), Variant::Int(1));
        assert_eq!(run_val("var x = 3\nx **= 3\nreturn x\n"), Variant::Int(27));
        assert_eq!(run_val("var x = 6\nx &= 3\nreturn x\n"), Variant::Int(2));
        assert_eq!(run_val("var x = 4\nx |= 1\nreturn x\n"), Variant::Int(5));
        assert_eq!(run_val("var x = 6\nx ^= 3\nreturn x\n"), Variant::Int(5));
        assert_eq!(run_val("var x = 1\nx <<= 4\nreturn x\n"), Variant::Int(16));
        assert_eq!(run_val("var x = 16\nx >>= 2\nreturn x\n"), Variant::Int(4));
    }

    #[test]
    fn compound_assignment_on_index_target() {
        assert_eq!(
            run_val("var a = [1, 2, 3]\na[0] += 10\nreturn a[0]\n"),
            Variant::Int(11)
        );
        assert_eq!(
            run_val("var a = [1, 2, 3]\na[1] *= 3\nreturn a[1]\n"),
            Variant::Int(6)
        );
    }

    // =======================================================================
    // pat-u3l8c.9 — match semantics
    // =======================================================================

    #[test]
    fn match_bare_identifier_is_constant_ref_regression() {
        // The old bug always bound bare identifiers, so this would match the
        // first arm. Now `RED` is a constant reference compared by value.
        let src = "\
const RED = 1
var x = 2
match x:
    RED:
        return \"red\"
    _:
        return \"other\"
";
        assert_eq!(run_val(src), Variant::String("other".into()));
    }

    #[test]
    fn match_constant_ref_matches_when_equal() {
        let src = "\
const RED = 1
var x = 1
match x:
    RED:
        return \"red\"
    _:
        return \"other\"
";
        assert_eq!(run_val(src), Variant::String("red".into()));
    }

    #[test]
    fn match_var_binding() {
        let src = "\
var x = 42
match x:
    var y:
        return y
";
        assert_eq!(run_val(src), Variant::Int(42));
    }

    #[test]
    fn match_multiple_patterns() {
        let src = "\
match 2:
    1, 2, 3:
        return \"low\"
    _:
        return \"high\"
";
        assert_eq!(run_val(src), Variant::String("low".into()));
    }

    #[test]
    fn match_array_rest_pattern() {
        let src = "\
match [1, 2, 3, 4]:
    [1, ..]:
        return \"starts\"
    _:
        return \"no\"
";
        assert_eq!(run_val(src), Variant::String("starts".into()));
    }

    #[test]
    fn match_array_exact_length_without_rest() {
        let src = "\
match [1, 2, 3]:
    [1, 2]:
        return \"two\"
    [1, 2, 3]:
        return \"three\"
    _:
        return \"other\"
";
        assert_eq!(run_val(src), Variant::String("three".into()));
    }

    #[test]
    fn match_array_binding_in_pattern() {
        let src = "\
match [1, 99]:
    [1, var second]:
        return second
    _:
        return -1
";
        assert_eq!(run_val(src), Variant::Int(99));
    }

    #[test]
    fn match_dictionary_pattern_with_rest() {
        let src = "\
match {\"type\": \"a\", \"val\": 1}:
    {\"type\": \"a\", ..}:
        return \"typeA\"
    _:
        return \"no\"
";
        assert_eq!(run_val(src), Variant::String("typeA".into()));
    }

    #[test]
    fn match_dictionary_pattern_exact() {
        let src = "\
match {\"a\": 1}:
    {\"a\": 1}:
        return \"yes\"
    _:
        return \"no\"
";
        assert_eq!(run_val(src), Variant::String("yes".into()));
        // A dict with an extra key must not match an exact (rest-less) pattern.
        let src2 = "\
match {\"a\": 1, \"b\": 2}:
    {\"a\": 1}:
        return \"yes\"
    _:
        return \"no\"
";
        assert_eq!(run_val(src2), Variant::String("no".into()));
    }

    #[test]
    fn match_when_guard() {
        let src = "\
var x = 5
match x:
    var n when n > 10:
        return \"big\"
    var n when n > 0:
        return \"small\"
    _:
        return \"other\"
";
        assert_eq!(run_val(src), Variant::String("small".into()));
    }

    // =======================================================================
    // pat-u3l8c.10 — math utility functions
    // =======================================================================

    #[test]
    fn math_posmod_and_fmod() {
        assert_eq!(run_val("return posmod(-1, 3)\n"), Variant::Int(2));
        assert_eq!(run_val("return posmod(7, 3)\n"), Variant::Int(1));
        assert!((run_float("return fmod(5.5, 2.0)\n") - 1.5).abs() < 1e-9);
    }

    #[test]
    fn math_snapped_smoothstep_remap() {
        assert!((run_float("return snapped(0.3, 0.5)\n") - 0.5).abs() < 1e-9);
        assert!((run_float("return smoothstep(0.0, 1.0, 0.5)\n") - 0.5).abs() < 1e-9);
        assert!((run_float("return remap(5.0, 0.0, 10.0, 0.0, 100.0)\n") - 50.0).abs() < 1e-9);
        assert!((run_float("return inverse_lerp(0.0, 10.0, 5.0)\n") - 0.5).abs() < 1e-9);
    }

    #[test]
    fn globalscope_typed_math_builtins() {
        // pat-nzgdu.39: typed *i/*f variants force int/float returns; min/max
        // are variadic; wrap is Variant-generic; abs/clamp/sign/snapped are
        // componentwise on vectors.

        // Integer-forcing variants return Variant::Int (Godot's INT typeof).
        assert_eq!(run_val("return floori(2.7)\n"), Variant::Int(2));
        assert_eq!(run_val("return ceili(2.1)\n"), Variant::Int(3));
        assert_eq!(run_val("return roundi(2.5)\n"), Variant::Int(3));
        assert_eq!(run_val("return absi(-4)\n"), Variant::Int(4));
        assert_eq!(run_val("return signi(-3)\n"), Variant::Int(-1));
        assert_eq!(run_val("return snappedi(12.0, 5)\n"), Variant::Int(10));
        assert_eq!(run_val("return maxi(3, 7)\n"), Variant::Int(7));
        assert_eq!(run_val("return mini(3, 7)\n"), Variant::Int(3));
        assert_eq!(run_val("return clampi(15, 0, 10)\n"), Variant::Int(10));
        // Type is genuinely INT, not a coerced float.
        assert!(matches!(run_val("return floori(2.7)\n"), Variant::Int(_)));

        // Float-forcing variants return Variant::Float.
        assert!((run_float("return floorf(2.7)\n") - 2.0).abs() < 1e-9);
        assert!((run_float("return ceilf(2.1)\n") - 3.0).abs() < 1e-9);
        assert!((run_float("return roundf(2.4)\n") - 2.0).abs() < 1e-9);
        assert!((run_float("return absf(-4.5)\n") - 4.5).abs() < 1e-9);
        assert!((run_float("return signf(-3.0)\n") + 1.0).abs() < 1e-9);
        assert!((run_float("return snappedf(1.37, 0.1)\n") - 1.4).abs() < 1e-9);
        assert!((run_float("return maxf(1, 2.5)\n") - 2.5).abs() < 1e-9);
        assert!((run_float("return minf(1, 2.5)\n") - 1.0).abs() < 1e-9);
        assert!((run_float("return clampf(15.0, 0.0, 10.0)\n") - 10.0).abs() < 1e-9);
        assert!((run_float("return lerpf(0.0, 10.0, 0.25)\n") - 2.5).abs() < 1e-9);
        assert!(matches!(run_val("return floorf(2.7)\n"), Variant::Float(_)));

        // Variadic min/max (2+ args).
        assert_eq!(run_val("return min(3, 1, 2)\n"), Variant::Int(1));
        assert_eq!(run_val("return max(3, 1, 2, 9, 4)\n"), Variant::Int(9));

        // Variant-generic wrap: int operands wrap as int, float as float.
        assert_eq!(run_val("return wrap(5, 0, 3)\n"), Variant::Int(2));
        assert!((run_float("return wrap(5.0, 0.0, 3.0)\n") - 2.0).abs() < 1e-9);

        // Componentwise vector abs/clamp/sign/snapped.
        assert_eq!(
            run_val("return abs(Vector2(-1, -2))\n"),
            Variant::Vector2(gdcore::math::Vector2::new(1.0, 2.0))
        );
        assert_eq!(
            run_val("return clamp(Vector2(5, -5), Vector2(0, 0), Vector2(3, 3))\n"),
            Variant::Vector2(gdcore::math::Vector2::new(3.0, 0.0))
        );
        assert_eq!(
            run_val("return sign(Vector3(-2, 0, 5))\n"),
            Variant::Vector3(gdcore::math::Vector3::new(-1.0, 0.0, 1.0))
        );
        assert_eq!(
            run_val("return abs(Vector3(-1, -2, -3))\n"),
            Variant::Vector3(gdcore::math::Vector3::new(1.0, 2.0, 3.0))
        );
    }

    #[test]
    fn globalscope_interp_math_builtins() {
        use std::f64::consts::PI;

        // is_finite / step_decimals.
        assert_eq!(run_val("return is_finite(1.0)\n"), Variant::Bool(true));
        assert_eq!(run_val("return is_finite(INF)\n"), Variant::Bool(false));
        assert_eq!(run_val("return is_finite(NAN)\n"), Variant::Bool(false));
        assert_eq!(run_val("return step_decimals(0.01)\n"), Variant::Int(2));
        assert_eq!(run_val("return step_decimals(1.0)\n"), Variant::Int(0));

        // Bézier + cubic interpolation.
        assert!(
            (run_float("return bezier_interpolate(0.0, 0.0, 1.0, 1.0, 0.5)\n") - 0.5).abs() < 1e-9
        );
        assert!(
            (run_float("return bezier_derivative(0.0, 0.0, 1.0, 1.0, 0.5)\n") - 1.5).abs() < 1e-9
        );
        assert!(
            (run_float("return cubic_interpolate(0.0, 1.0, 0.0, 1.0, 0.5)\n") - 0.5).abs() < 1e-9
        );
        assert!(
            (run_float("return cubic_interpolate_angle(0.0, 1.0, 0.0, 1.0, 0.5)\n") - 0.5).abs()
                < 1e-9
        );
        assert!(
            (run_float(
                "return cubic_interpolate_in_time(0.0, 1.0, 0.0, 1.0, 0.5, 1.0, -1.0, 2.0)\n"
            ) - 0.5)
                .abs()
                < 1e-9
        );
        assert!(
            (run_float(
                "return cubic_interpolate_angle_in_time(0.0, 1.0, 0.0, 1.0, 0.5, 1.0, -1.0, 2.0)\n"
            ) - 0.5)
                .abs()
                < 1e-9
        );

        // Angle helpers.
        assert!((run_float("return angle_difference(0.0, 1.0)\n") - 1.0).abs() < 1e-9);
        // At the +/-PI boundary the sign is ambiguous; assert the magnitude.
        assert!((run_float("return angle_difference(0.0, PI)\n").abs() - PI).abs() < 1e-9);
        // Unambiguous small rotation moves exactly by delta toward the target.
        assert!((run_float("return rotate_toward(0.0, 1.0, 0.1)\n") - 0.1).abs() < 1e-9);
        // Boundary case: direction ambiguous, but the step magnitude is delta.
        assert!((run_float("return rotate_toward(0.0, PI, 0.1)\n").abs() - 0.1).abs() < 1e-9);
        // A delta larger than the gap lands exactly on the target.
        assert!((run_float("return rotate_toward(0.0, 1.0, 5.0)\n") - 1.0).abs() < 1e-9);

        // fposmod carries the sign of the divisor.
        assert!((run_float("return fposmod(-1.0, 3.0)\n") - 2.0).abs() < 1e-9);
        assert!((run_float("return fposmod(5.0, 3.0)\n") - 2.0).abs() < 1e-9);

        // Hyperbolics.
        assert!(run_float("return sinh(0.0)\n").abs() < 1e-9);
        assert!((run_float("return cosh(0.0)\n") - 1.0).abs() < 1e-9);
        assert!(run_float("return tanh(0.0)\n").abs() < 1e-9);
        assert!(run_float("return asinh(0.0)\n").abs() < 1e-9);
        assert!(run_float("return acosh(1.0)\n").abs() < 1e-9);
        assert!(run_float("return atanh(0.0)\n").abs() < 1e-9);
    }

    #[test]
    fn globalscope_diagnostics_print_builtins() {
        // prints joins args with spaces; printt with tabs.
        assert_eq!(
            run("prints(\"a\", 1, true)\n").output,
            vec!["a 1 true".to_string()]
        );
        assert_eq!(
            run("printt(\"a\", \"b\")\n").output,
            vec!["a\tb".to_string()]
        );

        // str() is variadic and concatenates the string form of every arg.
        assert_eq!(
            run_val("return str(\"x\", 2, \"y\")\n"),
            Variant::String("x2y".into())
        );
        assert_eq!(
            run("print(str(\"x\", 2, \"y\"))\n").output,
            vec!["x2y".to_string()]
        );

        // printraw has no trailing newline: consecutive raws share one line.
        assert_eq!(
            run("printraw(\"ab\")\nprintraw(\"cd\")\n").output,
            vec!["abcd".to_string()]
        );

        // print_verbose is silent by default (headless verbose off).
        assert!(run("print_verbose(\"hidden\")\n").output.is_empty());

        // push_error / push_warning / printerr route to the observable
        // diagnostics sink (not silently dropped).
        let errs = run_interp("push_error(\"boom\")\n");
        assert!(errs.errors().iter().any(|e| e.contains("boom")));
        let warn = run_interp("push_warning(\"careful\")\n");
        assert!(warn
            .errors()
            .iter()
            .any(|e| e.contains("careful") && e.contains("WARNING")));
        let perr = run_interp("printerr(\"eek\")\n");
        assert!(perr.errors().iter().any(|e| e.contains("eek")));
    }

    #[test]
    fn packed_bytearray_string_conversions() {
        // pat-nzgdu.33 (interpreter slice): PackedByteArray string-conversion
        // methods. "Hi" == bytes [72, 105].
        assert_eq!(
            run_val("var b = PackedByteArray([72, 105])\nreturn b.get_string_from_utf8()\n"),
            Variant::String("Hi".into())
        );
        assert_eq!(
            run_val("var b = PackedByteArray([72, 105])\nreturn b.get_string_from_ascii()\n"),
            Variant::String("Hi".into())
        );
        // hex_encode: 72 -> "48", 105 -> "69".
        assert_eq!(
            run_val("var b = PackedByteArray([72, 105])\nreturn b.hex_encode()\n"),
            Variant::String("4869".into())
        );
        // Multibyte UTF-8: [226, 156, 147] is U+2713 CHECK MARK.
        assert_eq!(
            run_val("var b = PackedByteArray([226, 156, 147])\nreturn b.get_string_from_utf8()\n"),
            Variant::String("\u{2713}".into())
        );
    }

    #[test]
    fn os_singleton_query_methods() {
        // pat-nzgdu.65 (interpreter slice): OS singleton query methods resolve
        // through call_os_static without UndefinedFunction.
        // has_feature returns a bool; "web" tracks the build target.
        assert_eq!(
            run_val("return OS.has_feature(\"web\")\n"),
            Variant::Bool(cfg!(target_arch = "wasm32"))
        );
        // get_locale / get_locale_language are non-empty strings.
        match run_val("return OS.get_locale()\n") {
            Variant::String(s) => assert!(!s.is_empty(), "locale should be non-empty"),
            other => panic!("expected String locale, got {other:?}"),
        }
        match run_val("return OS.get_locale_language()\n") {
            Variant::String(s) => assert!(!s.is_empty(), "locale language should be non-empty"),
            other => panic!("expected String, got {other:?}"),
        }
        // processor_count >= 1.
        match run_val("return OS.get_processor_count()\n") {
            Variant::Int(n) => assert!(n >= 1, "processor_count should be >=1, got {n}"),
            other => panic!("expected Int, got {other:?}"),
        }
        // get_cmdline_user_args returns a (possibly empty) PackedStringArray.
        assert!(matches!(
            run_val("return OS.get_cmdline_user_args()\n"),
            Variant::PackedStringArray(_)
        ));
        // get_name is non-empty (macOS / Linux / Windows / Web).
        match run_val("return OS.get_name()\n") {
            Variant::String(s) => assert!(!s.is_empty()),
            other => panic!("expected String, got {other:?}"),
        }
    }

    #[test]
    fn project_settings_singleton_roundtrip() {
        // pat-nzgdu.68 (interpreter slice): ProjectSettings set/get/has store +
        // globalize/localize path round-trip. (Unique "pat68/*" keys keep this
        // isolated even if the store isn't process-fresh.)
        // A missing setting returns the caller's default.
        assert_eq!(
            run_val("return ProjectSettings.get_setting(\"pat68/unset\", 111.0)\n"),
            Variant::Float(111.0)
        );
        // set_setting then get_setting round-trips the stored value (over the default).
        assert_eq!(
            run_val(
                "ProjectSettings.set_setting(\"pat68/x\", 7)\nreturn ProjectSettings.get_setting(\"pat68/x\", 0)\n"
            ),
            Variant::Int(7)
        );
        // has_setting reflects presence / absence.
        assert_eq!(
            run_val(
                "ProjectSettings.set_setting(\"pat68/y\", 1)\nreturn ProjectSettings.has_setting(\"pat68/y\")\n"
            ),
            Variant::Bool(true)
        );
        assert_eq!(
            run_val("return ProjectSettings.has_setting(\"pat68/never_set\")\n"),
            Variant::Bool(false)
        );
        // globalize_path -> absolute; localize_path round-trips it back to res://.
        assert_eq!(
            run_val(
                "return ProjectSettings.localize_path(ProjectSettings.globalize_path(\"res://a.txt\"))\n"
            ),
            Variant::String("res://a.txt".into())
        );
    }

    #[test]
    fn display_server_singleton_methods() {
        // pat-nzgdu.67 (interpreter slice): DisplayServer.* resolves headless
        // without UndefinedFunction; clipboard round-trips; tts is recorded.
        match run_val("return DisplayServer.screen_get_scale()\n") {
            Variant::Float(_) => {}
            other => panic!("expected Float scale, got {other:?}"),
        }
        assert!(matches!(
            run_val("return DisplayServer.screen_get_dpi()\n"),
            Variant::Int(_)
        ));
        // clipboard round-trip.
        assert_eq!(
            run_val("DisplayServer.clipboard_set(\"hi\")\nreturn DisplayServer.clipboard_get()\n"),
            Variant::String("hi".into())
        );
        // tts_speak resolves without error (Nil); tts_get_voices is an Array.
        assert_eq!(
            run_val("return DisplayServer.tts_speak(\"hello\", 0)\n"),
            Variant::Nil
        );
        assert!(matches!(
            run_val("return DisplayServer.tts_get_voices()\n"),
            Variant::Array(_)
        ));
        // window_get_size is a Vector2.
        assert!(matches!(
            run_val("return DisplayServer.window_get_size()\n"),
            Variant::Vector2(_)
        ));
    }

    #[test]
    fn javascript_bridge_singleton_methods() {
        // pat-nzgdu.66 (interpreter slice): JavaScriptBridge.* resolves headless
        // without UndefinedFunction; eval yields Nil (no native JS engine).
        assert_eq!(
            run_val("return JavaScriptBridge.eval(\"1+2\")\n"),
            Variant::Nil
        );
        assert_eq!(
            run_val("return JavaScriptBridge.eval(\"1+2\", true)\n"),
            Variant::Nil
        );
        assert_eq!(
            run_val("return JavaScriptBridge.get_interface(\"localStorage\")\n"),
            Variant::Nil
        );
        assert_eq!(
            run_val("return JavaScriptBridge.create_object(\"Object\")\n"),
            Variant::Nil
        );
        // download_buffer resolves (Nil) without error.
        assert_eq!(
            run_val("return JavaScriptBridge.download_buffer(0, \"f.txt\", \"text/plain\")\n"),
            Variant::Nil
        );
        // pwa_needs_update is a Bool.
        assert_eq!(
            run_val("return JavaScriptBridge.pwa_needs_update()\n"),
            Variant::Bool(false)
        );
    }

    #[test]
    fn javascript_bridge_buffer_helpers_have_native_fallbacks() {
        // pat-nzgdu.146: Godot's ArrayBuffer helpers. Off the web there is no JS
        // heap, so these must still resolve with defined values rather than an
        // UndefinedFunction — the same way the rest of the singleton degrades.
        assert_eq!(
            run_val("return JavaScriptBridge.is_js_buffer(0)\n"),
            Variant::Bool(false)
        );
        assert_eq!(
            run_val("return JavaScriptBridge.js_buffer_to_packed_byte_array(0)\n"),
            Variant::PackedByteArray(Vec::new())
        );
    }

    #[test]
    fn javascript_bridge_singleton_script_reachable() {
        // pat-nzgdu.146 acceptance: a script referencing JavaScriptBridge must
        // dispatch through the singleton table and return the defined values,
        // never an unknown-identifier/UndefinedFunction error.
        // Every call runs in one script: any UndefinedFunction would surface as
        // an Err and panic the harness, so reaching the return proves dispatch.
        let v = run_val(
            "var calls = 0\n\
             JavaScriptBridge.create_object(\"Object\")\n\
             calls += 1\n\
             JavaScriptBridge.eval(\"1+2\")\n\
             calls += 1\n\
             JavaScriptBridge.get_interface(\"localStorage\")\n\
             calls += 1\n\
             JavaScriptBridge.download_buffer(0, \"f.txt\", \"text/plain\")\n\
             calls += 1\n\
             JavaScriptBridge.force_fs_sync()\n\
             calls += 1\n\
             return calls\n",
        );
        assert_eq!(v, Variant::Int(5), "all five calls dispatched");

        // An unknown method still errors, so the arm is a real dispatch table
        // rather than a catch-all that silently swallows typos.
        let err = run_err("return JavaScriptBridge.__no_such_method__()\n");
        assert!(
            matches!(err.kind, RuntimeErrorKind::UndefinedFunction(_)),
            "got: {err:?}",
        );
    }

    #[test]
    fn io_singletons_marshalls_and_resourceuid() {
        // pat-nzgdu.79 (interpreter slice): Marshalls base64<->Variant and
        // ResourceUID id<->text round-trips.
        // Variant round-trips through base64 (via the gdvariant binary codec).
        assert_eq!(
            run_val(
                "var v = 42\nreturn Marshalls.base64_to_variant(Marshalls.variant_to_base64(v))\n"
            ),
            Variant::Int(42)
        );
        assert_eq!(
            run_val("return Marshalls.base64_to_variant(Marshalls.variant_to_base64(\"hello\"))\n"),
            Variant::String("hello".into())
        );
        // raw -> base64 is a String; utf8 round-trips through base64.
        assert!(matches!(
            run_val("return Marshalls.raw_to_base64(PackedByteArray([1, 2, 3]))\n"),
            Variant::String(_)
        ));
        assert_eq!(
            run_val("return Marshalls.base64_to_utf8(Marshalls.utf8_to_base64(\"Hi!\"))\n"),
            Variant::String("Hi!".into())
        );
        // ResourceUID id -> text -> id round-trips (canonical text from id_to_text).
        assert_eq!(
            run_val("return ResourceUID.text_to_id(ResourceUID.id_to_text(12345))\n"),
            Variant::Int(12345)
        );
        assert!(matches!(
            run_val("return ResourceUID.text_to_id(\"uid://abc\")\n"),
            Variant::Int(_)
        ));
        assert!(matches!(
            run_val("return ResourceUID.id_to_text(0)\n"),
            Variant::String(_)
        ));
    }

    #[test]
    fn tr_and_translationserver_locale() {
        // pat-nzgdu.92 (interpreter slice): tr()/tr_n() + TranslationServer
        // locale state + catalog lookup. Unique pat92* keys/locales keep this
        // isolated from the process-global translation store.
        // No catalog entry -> tr() falls back to the key.
        assert_eq!(
            run_val("TranslationServer.set_locale(\"pat92xx\")\nreturn tr(\"pat92_missing\")\n"),
            Variant::String("pat92_missing".into())
        );
        // Seed a catalog, switch locale, translate via tr().
        assert_eq!(
            run_val(
                "TranslationServer.add_message(\"pat92fr\", \"pat92_hi\", \"Bonjour\")\nTranslationServer.set_locale(\"pat92fr\")\nreturn tr(\"pat92_hi\")\n"
            ),
            Variant::String("Bonjour".into())
        );
        // TranslationServer.translate matches tr().
        assert_eq!(
            run_val(
                "TranslationServer.set_locale(\"pat92fr\")\nreturn TranslationServer.translate(\"pat92_hi\")\n"
            ),
            Variant::String("Bonjour".into())
        );
        // get_locale round-trips the set locale.
        assert_eq!(
            run_val("TranslationServer.set_locale(\"pat92de\")\nreturn TranslationServer.get_locale()\n"),
            Variant::String("pat92de".into())
        );
        // tr_n picks singular vs plural (fallback, untranslated unique keys).
        assert_eq!(
            run_val(
                "TranslationServer.set_locale(\"pat92xx\")\nreturn tr_n(\"pat92_apple\", \"pat92_apples\", 1)\n"
            ),
            Variant::String("pat92_apple".into())
        );
        assert_eq!(
            run_val(
                "TranslationServer.set_locale(\"pat92xx\")\nreturn tr_n(\"pat92_apple\", \"pat92_apples\", 3)\n"
            ),
            Variant::String("pat92_apples".into())
        );
    }

    #[test]
    fn math_wrap_and_pingpong() {
        assert_eq!(run_val("return wrapi(5, 0, 3)\n"), Variant::Int(2));
        assert!((run_float("return wrapf(5.0, 0.0, 3.0)\n") - 2.0).abs() < 1e-9);
        assert!((run_float("return pingpong(3.0, 2.0)\n") - 1.0).abs() < 1e-9);
    }

    #[test]
    fn math_ease_and_approx() {
        assert!((run_float("return ease(0.5, 1.0)\n") - 0.5).abs() < 1e-9);
        assert!((run_float("return ease(0.5, 2.0)\n") - 0.25).abs() < 1e-9);
        assert_eq!(
            run_val("return is_equal_approx(1.0, 1.0)\n"),
            Variant::Bool(true)
        );
        assert_eq!(
            run_val("return is_equal_approx(1.0, 1.5)\n"),
            Variant::Bool(false)
        );
        assert_eq!(run_val("return is_zero_approx(0.0)\n"), Variant::Bool(true));
        assert_eq!(
            run_val("return is_zero_approx(0.1)\n"),
            Variant::Bool(false)
        );
    }

    #[test]
    fn math_nan_inf_and_po2() {
        assert_eq!(run_val("return is_nan(sqrt(-1.0))\n"), Variant::Bool(true));
        assert_eq!(run_val("return is_nan(1.0)\n"), Variant::Bool(false));
        assert_eq!(run_val("return is_inf(INF)\n"), Variant::Bool(true));
        assert_eq!(run_val("return is_inf(1.0)\n"), Variant::Bool(false));
        assert_eq!(run_val("return nearest_po2(17)\n"), Variant::Int(32));
        assert_eq!(run_val("return nearest_po2(16)\n"), Variant::Int(16));
        assert_eq!(run_val("return nearest_po2(0)\n"), Variant::Int(0));
    }

    #[test]
    fn math_trig_exp_log_db() {
        assert!((run_float("return exp(0.0)\n") - 1.0).abs() < 1e-9);
        assert!((run_float("return log(1.0)\n")).abs() < 1e-9);
        assert!((run_float("return tan(0.0)\n")).abs() < 1e-9);
        assert!((run_float("return atan2(1.0, 1.0)\n") - std::f64::consts::FRAC_PI_4).abs() < 1e-9);
        assert!((run_float("return db_to_linear(0.0)\n") - 1.0).abs() < 1e-9);
        assert!((run_float("return linear_to_db(1.0)\n")).abs() < 1e-9);
    }

    // =======================================================================
    // pat-u3l8c.14 — general lvalue write path
    // =======================================================================

    fn run_str(src: &str) -> String {
        match run_val(src) {
            Variant::String(s) => s,
            other => panic!("expected string, got {other:?}"),
        }
    }

    #[test]
    fn lvalue_bare_index_assign() {
        assert_eq!(
            run_val("var a = [1, 2, 3]\na[1] = 99\nreturn a[1]\n"),
            Variant::Int(99)
        );
    }

    #[test]
    fn lvalue_nested_index_assign() {
        assert_eq!(
            run_val("var g = [[1, 2], [3, 4]]\ng[0][1] = 99\nreturn g[0][1]\n"),
            Variant::Int(99)
        );
        // Sibling elements are untouched.
        assert_eq!(
            run_val("var g = [[1, 2], [3, 4]]\ng[0][1] = 99\nreturn g[1][0]\n"),
            Variant::Int(3)
        );
    }

    #[test]
    fn lvalue_dict_index_and_nested_method() {
        assert_eq!(
            run_val("var d = {}\nd[\"a\"] = 1\nreturn d[\"a\"]\n"),
            Variant::Int(1)
        );
        // Mutating method through an index base persists.
        assert_eq!(
            run_val("var m = {\"list\": [1]}\nm[\"list\"].append(2)\nreturn m[\"list\"]\n"),
            Variant::Array(vec![Variant::Int(1), Variant::Int(2)])
        );
    }

    #[test]
    fn lvalue_self_member_index_assign() {
        let mut interp = Interpreter::new();
        let cd = interp
            .run_class(
                "var grid = [0, 0, 0]\nfunc setit():\n    self.grid[1] = 7\n    return self.grid[1]\n",
            )
            .unwrap();
        let mut inst = interp.instantiate_class(&cd).unwrap();
        let r = interp
            .call_instance_method(&mut inst, "setit", &[])
            .unwrap();
        assert_eq!(r, Variant::Int(7));
        assert_eq!(
            inst.properties.get("grid"),
            Some(&Variant::Array(vec![
                Variant::Int(0),
                Variant::Int(7),
                Variant::Int(0)
            ]))
        );
    }

    #[test]
    fn lvalue_instance_prop_array_method_mutates() {
        let mut interp = Interpreter::new();
        let cd = interp
            .run_class(
                "var items = []\nfunc add():\n    items.append(5)\n    self.items.append(6)\n    return items.size()\n",
            )
            .unwrap();
        let mut inst = interp.instantiate_class(&cd).unwrap();
        let r = interp.call_instance_method(&mut inst, "add", &[]).unwrap();
        assert_eq!(r, Variant::Int(2));
        assert_eq!(
            inst.properties.get("items"),
            Some(&Variant::Array(vec![Variant::Int(5), Variant::Int(6)]))
        );
    }

    // =======================================================================
    // pat-u3l8c.14 — Array method surface
    // =======================================================================

    #[test]
    fn array_stack_and_queue_ops() {
        assert_eq!(
            run_val("var a = [1, 2]\na.push_front(0)\nreturn a\n"),
            Variant::Array(vec![Variant::Int(0), Variant::Int(1), Variant::Int(2)])
        );
        assert_eq!(
            run_val("var a = [1, 2, 3]\nreturn a.pop_front()\n"),
            Variant::Int(1)
        );
        assert_eq!(
            run_val("var a = [1, 2, 3]\nreturn a.pop_at(1)\n"),
            Variant::Int(2)
        );
        assert_eq!(run_val("return [5, 6, 7].front()\n"), Variant::Int(5));
        assert_eq!(run_val("return [5, 6, 7].back()\n"), Variant::Int(7));
        assert_eq!(
            run_val("var a = [1, 2]\na.clear()\nreturn a.is_empty()\n"),
            Variant::Bool(true)
        );
    }

    #[test]
    fn array_query_and_resize_ops() {
        assert_eq!(run_val("return [1, 2, 2, 3].count(2)\n"), Variant::Int(2));
        assert_eq!(run_val("return [1, 2, 1].rfind(1)\n"), Variant::Int(2));
        assert_eq!(run_val("return [3, 1, 2].max()\n"), Variant::Int(3));
        assert_eq!(run_val("return [3, 1, 2].min()\n"), Variant::Int(1));
        assert_eq!(run_val("return [1, 3, 5, 7].bsearch(4)\n"), Variant::Int(2));
        assert_eq!(
            run_val("var a = [1, 2]\na.append_array([3, 4])\nreturn a\n"),
            Variant::Array((1..=4).map(Variant::Int).collect())
        );
        assert_eq!(
            run_val("var a = [1, 2]\na.resize(4)\nreturn a\n"),
            Variant::Array(vec![
                Variant::Int(1),
                Variant::Int(2),
                Variant::Nil,
                Variant::Nil
            ])
        );
        assert_eq!(
            run_val("var a = [1, 2, 3]\na.fill(0)\nreturn a\n"),
            Variant::Array(vec![Variant::Int(0), Variant::Int(0), Variant::Int(0)])
        );
    }

    #[test]
    fn array_slice_step_and_negative() {
        assert_eq!(
            run_val("return [0, 1, 2, 3, 4, 5].slice(1, 5, 2)\n"),
            Variant::Array(vec![Variant::Int(1), Variant::Int(3)])
        );
        // Negative indices.
        assert_eq!(
            run_val("return [0, 1, 2, 3, 4].slice(-2, 5)\n"),
            Variant::Array(vec![Variant::Int(3), Variant::Int(4)])
        );
        // Single-arg slice to the end.
        assert_eq!(
            run_val("return [0, 1, 2, 3].slice(2)\n"),
            Variant::Array(vec![Variant::Int(2), Variant::Int(3)])
        );
    }

    #[test]
    fn array_higher_order_methods() {
        assert_eq!(
            run_val("return [1, 2, 3].map(func(x): return x * 2)\n"),
            Variant::Array(vec![Variant::Int(2), Variant::Int(4), Variant::Int(6)])
        );
        assert_eq!(
            run_val("return [1, 2, 3, 4].filter(func(x): return x % 2 == 0)\n"),
            Variant::Array(vec![Variant::Int(2), Variant::Int(4)])
        );
        assert_eq!(
            run_val("return [1, 2, 3, 4].reduce(func(a, b): return a + b)\n"),
            Variant::Int(10)
        );
        assert_eq!(
            run_val("return [1, 2, 3].reduce(func(a, b): return a + b, 100)\n"),
            Variant::Int(106)
        );
        assert_eq!(
            run_val("return [1, 2, 3].any(func(x): return x > 2)\n"),
            Variant::Bool(true)
        );
        assert_eq!(
            run_val("return [1, 2, 3].all(func(x): return x > 0)\n"),
            Variant::Bool(true)
        );
        assert_eq!(
            run_val("return [1, 2, 3].all(func(x): return x > 1)\n"),
            Variant::Bool(false)
        );
    }

    #[test]
    fn array_sort_custom() {
        assert_eq!(
            run_val("var a = [3, 1, 2]\na.sort_custom(func(x, y): return x < y)\nreturn a\n"),
            Variant::Array(vec![Variant::Int(1), Variant::Int(2), Variant::Int(3)])
        );
        // Descending.
        assert_eq!(
            run_val("var a = [3, 1, 2]\na.sort_custom(func(x, y): return x > y)\nreturn a\n"),
            Variant::Array(vec![Variant::Int(3), Variant::Int(2), Variant::Int(1)])
        );
    }

    #[test]
    fn array_pick_random_and_shuffle_deterministic() {
        let v = run_val("return [10, 20, 30].pick_random()\n");
        assert!(matches!(v, Variant::Int(x) if x == 10 || x == 20 || x == 30));
        // Shuffle preserves the multiset of elements.
        assert_eq!(
            run_val("var a = [1, 2, 3, 4, 5]\na.shuffle()\na.sort()\nreturn a\n"),
            Variant::Array((1..=5).map(Variant::Int).collect())
        );
    }

    // =======================================================================
    // pat-u3l8c.13 — String method surface + multibyte length
    // =======================================================================

    #[test]
    fn string_length_is_char_count_multibyte() {
        assert_eq!(run_val("return \"héllo\".length()\n"), Variant::Int(5));
        assert_eq!(run_val("return len(\"héllo\")\n"), Variant::Int(5));
        // substr is char-index based.
        assert_eq!(run_str("return \"héllo\".substr(1, 3)\n"), "éll");
        assert_eq!(run_str("return \"héllo\".left(2)\n"), "hé");
        assert_eq!(run_str("return \"héllo\".right(2)\n"), "lo");
    }

    #[test]
    fn string_search_methods() {
        assert_eq!(run_val("return \"hello\".find(\"l\")\n"), Variant::Int(2));
        assert_eq!(
            run_val("return \"hello\".find(\"l\", 3)\n"),
            Variant::Int(3)
        );
        assert_eq!(run_val("return \"hello\".rfind(\"l\")\n"), Variant::Int(3));
        assert_eq!(
            run_val("return \"Hello\".findn(\"HELLO\")\n"),
            Variant::Int(0)
        );
        assert_eq!(run_val("return \"a-b-c\".count(\"-\")\n"), Variant::Int(2));
        assert_eq!(
            run_val("return \"hello\".contains(\"ell\")\n"),
            Variant::Bool(true)
        );
        assert_eq!(
            run_val("return \"Hello\".containsn(\"hello\")\n"),
            Variant::Bool(true)
        );
    }

    #[test]
    fn string_edit_methods() {
        assert_eq!(run_str("return \"  hi  \".strip_edges()\n"), "hi");
        assert_eq!(run_str("return \"ab\".repeat(3)\n"), "ababab");
        assert_eq!(run_str("return \"7\".lpad(3, \"0\")\n"), "007");
        assert_eq!(run_str("return \"7\".rpad(3, \".\")\n"), "7..");
        assert_eq!(run_str("return \"12\".pad_zeros(4)\n"), "0012");
        assert_eq!(run_str("return \"3.5\".pad_decimals(3)\n"), "3.500");
        assert_eq!(run_str("return \"3\".pad_decimals(2)\n"), "3.00");
        assert_eq!(run_str("return \"pre_val\".trim_prefix(\"pre_\")\n"), "val");
        assert_eq!(run_str("return \"val.txt\".trim_suffix(\".txt\")\n"), "val");
    }

    #[test]
    fn string_number_conversions() {
        assert_eq!(run_val("return \"42abc\".to_int()\n"), Variant::Int(42));
        assert_eq!(run_val("return \"  -7x\".to_int()\n"), Variant::Int(-7));
        assert_eq!(run_val("return \"abc\".to_int()\n"), Variant::Int(0));
        assert!((run_float("return \"3.14xy\".to_float()\n") - 3.14).abs() < 1e-9);
        assert_eq!(
            run_val("return \"42\".is_valid_int()\n"),
            Variant::Bool(true)
        );
        assert_eq!(
            run_val("return \"4a\".is_valid_int()\n"),
            Variant::Bool(false)
        );
        assert_eq!(
            run_val("return \"3.14\".is_valid_float()\n"),
            Variant::Bool(true)
        );
        assert_eq!(run_val("return \"ff\".hex_to_int()\n"), Variant::Int(255));
        assert_eq!(run_val("return \"0xff\".hex_to_int()\n"), Variant::Int(255));
        assert_eq!(run_val("return \"101\".bin_to_int()\n"), Variant::Int(5));
        assert_eq!(run_val("return \"0b101\".bin_to_int()\n"), Variant::Int(5));
    }

    #[test]
    fn string_case_conversions() {
        assert_eq!(
            run_str("return \"hello_world\".capitalize()\n"),
            "Hello World"
        );
        assert_eq!(
            run_str("return \"helloWorld\".capitalize()\n"),
            "Hello World"
        );
        assert_eq!(
            run_str("return \"helloWorld\".to_snake_case()\n"),
            "hello_world"
        );
        assert_eq!(
            run_str("return \"hello_world\".to_camel_case()\n"),
            "helloWorld"
        );
        assert_eq!(
            run_str("return \"hello_world\".to_pascal_case()\n"),
            "HelloWorld"
        );
    }

    #[test]
    fn string_path_methods() {
        assert_eq!(run_str("return \"a/b/c.txt\".get_extension()\n"), "txt");
        assert_eq!(run_str("return \"a/b/c.txt\".get_file()\n"), "c.txt");
        assert_eq!(run_str("return \"a/b/c.txt\".get_basename()\n"), "a/b/c");
        assert_eq!(run_str("return \"a/b/c.txt\".get_base_dir()\n"), "a/b");
        assert_eq!(run_str("return \"a\".path_join(\"b\")\n"), "a/b");
        assert_eq!(run_str("return \"a/./b/../c\".simplify_path()\n"), "a/c");
        assert_eq!(
            run_val("return \"/a/b\".is_absolute_path()\n"),
            Variant::Bool(true)
        );
        assert_eq!(
            run_val("return \"a/b\".is_relative_path()\n"),
            Variant::Bool(true)
        );
    }

    #[test]
    fn string_format_and_escapes() {
        assert_eq!(
            run_str("return \"{0} {1}\".format([\"a\", \"b\"])\n"),
            "a b"
        );
        assert_eq!(run_str("return \"{} {}\".format([\"x\", \"y\"])\n"), "x y");
        assert_eq!(
            run_str("return \"{name}\".format({\"name\": \"Bob\"})\n"),
            "Bob"
        );
        assert_eq!(run_str("return \"<a>\".xml_escape()\n"), "&lt;a&gt;");
        assert_eq!(run_str("return \"&lt;a&gt;\".xml_unescape()\n"), "<a>");
        assert_eq!(run_str("return \"a b\".uri_encode()\n"), "a%20b");
        assert_eq!(run_str("return \"a%20b\".uri_decode()\n"), "a b");
    }

    #[test]
    fn string_misc_methods() {
        assert_eq!(run_val("return \"\".hash()\n"), Variant::Int(5381));
        assert_eq!(
            run_str("return \"abc\".sha256_text()\n"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!((run_float("return \"test\".similarity(\"test\")\n") - 1.0).abs() < 1e-9);
        assert!((run_float("return \"abc\".similarity(\"xyz\")\n")).abs() < 1e-9);
        assert_eq!(
            run_val("return \"a\".casecmp_to(\"b\")\n"),
            Variant::Int(-1)
        );
        assert_eq!(run_val("return \"b\".casecmp_to(\"a\")\n"), Variant::Int(1));
        assert_eq!(
            run_val("return \"file10\".naturalnocasecmp_to(\"file9\")\n"),
            Variant::Int(1)
        );
        assert_eq!(run_str("return \"  a\\n  b\".dedent()\n"), "a\nb");
    }

    // =======================================================================
    // pat-u3l8c.12 — preload/load → real PackedScene instantiation
    // =======================================================================

    #[derive(Default)]
    struct MockSceneState {
        next_id: u64,
        /// parent id → child ids (records add_child / instantiate wiring).
        children: std::collections::HashMap<u64, Vec<u64>>,
        /// Scene paths that "exist" and can be instantiated.
        known_scenes: std::collections::HashSet<String>,
        /// Script paths that "exist" and can be instantiated via `.new()`.
        known_scripts: std::collections::HashSet<String>,
        /// (path, root_id) for each instantiate/new call.
        instantiated: Vec<(String, u64)>,
    }

    /// Mock SceneAccess for scene-instantiation tests. Shares its state via an
    /// `Rc<RefCell<..>>` so the test can inspect it after the interpreter run.
    struct MockSceneAccess {
        state: std::rc::Rc<std::cell::RefCell<MockSceneState>>,
    }

    impl SceneAccess for MockSceneAccess {
        fn get_node(&self, _from: u64, _path: &str) -> Option<u64> {
            None
        }
        fn get_parent(&self, _node: u64) -> Option<u64> {
            None
        }
        fn get_children(&self, node: u64) -> Vec<u64> {
            self.state
                .borrow()
                .children
                .get(&node)
                .cloned()
                .unwrap_or_default()
        }
        fn get_node_property(&self, _node: u64, _prop: &str) -> Variant {
            Variant::Nil
        }
        fn set_node_property(&mut self, _node: u64, _prop: &str, _value: Variant) {}
        fn emit_signal(&mut self, _node: u64, _signal: &str, _args: &[Variant]) {}
        fn connect_signal(&mut self, _source: u64, _signal: &str, _target: u64, _method: &str) {}
        fn get_node_name(&self, _node: u64) -> Option<String> {
            None
        }
        fn add_child(&mut self, parent_id: u64, child_id: u64) -> bool {
            self.state
                .borrow_mut()
                .children
                .entry(parent_id)
                .or_default()
                .push(child_id);
            true
        }
        fn instantiate_packed_scene(&mut self, path: &str) -> Option<u64> {
            let mut st = self.state.borrow_mut();
            if !st.known_scenes.contains(path) {
                return None;
            }
            st.next_id += 1;
            let root = st.next_id;
            st.instantiated.push((path.to_string(), root));
            // Simulate a two-node subtree: root with one child.
            let child = root + 10_000;
            st.children.insert(root, vec![child]);
            Some(root)
        }
        fn instantiate_script(&mut self, path: &str) -> Option<u64> {
            let mut st = self.state.borrow_mut();
            if !st.known_scripts.contains(path) {
                return None;
            }
            st.next_id += 1;
            let id = st.next_id;
            st.instantiated.push((path.to_string(), id));
            Some(id)
        }
    }

    fn mock_scene_interp(
        known: &[&str],
    ) -> (Interpreter, std::rc::Rc<std::cell::RefCell<MockSceneState>>) {
        let mut state = MockSceneState::default();
        state.next_id = 100; // keep instantiated ids clear of current_node_id
        for s in known {
            state.known_scenes.insert(s.to_string());
        }
        let state = std::rc::Rc::new(std::cell::RefCell::new(state));
        let mut interp = Interpreter::new();
        interp.set_scene_access(
            Box::new(MockSceneAccess {
                state: state.clone(),
            }),
            1,
        );
        (interp, state)
    }

    #[test]
    fn preload_returns_packed_scene_resource() {
        assert_eq!(
            run_val("return preload(\"res://x.tscn\").get_class()\n"),
            Variant::String("PackedScene".into())
        );
        assert_eq!(
            run_val("return preload(\"res://x.tscn\").get_path()\n"),
            Variant::String("res://x.tscn".into())
        );
        assert_eq!(
            run_val("return preload(\"res://x.tscn\").can_instantiate()\n"),
            Variant::Bool(true)
        );
    }

    #[test]
    fn preload_instantiate_add_child_spawns_subtree() {
        let (mut interp, state) = mock_scene_interp(&["res://bullet.tscn"]);
        let r = interp
            .run("var b = preload(\"res://bullet.tscn\").instantiate()\nadd_child(b)\nreturn b\n")
            .unwrap();
        let root_id = match r.return_value {
            Some(Variant::ObjectId(oid)) => oid.raw(),
            other => panic!("expected ObjectId, got {other:?}"),
        };
        let st = state.borrow();
        // instantiate was called with the scene path and produced the root.
        assert_eq!(st.instantiated.len(), 1);
        assert_eq!(st.instantiated[0].0, "res://bullet.tscn");
        assert_eq!(st.instantiated[0].1, root_id);
        // The subtree exists (root has a child).
        assert!(!st.children.get(&root_id).unwrap().is_empty());
        // add_child(self=1, root) wired the spawned root under the caller node.
        assert!(st
            .children
            .get(&1)
            .map(|c| c.contains(&root_id))
            .unwrap_or(false));
    }

    #[test]
    fn load_instantiate_spawns_subtree() {
        let (mut interp, state) = mock_scene_interp(&["res://enemy.tscn"]);
        let r = interp
            .run("var e = load(\"res://enemy.tscn\").instantiate()\nadd_child(e)\nreturn e\n")
            .unwrap();
        assert!(matches!(r.return_value, Some(Variant::ObjectId(_))));
        assert_eq!(state.borrow().instantiated.len(), 1);
    }

    #[test]
    fn load_missing_scene_instantiate_returns_nil() {
        // Scene not registered → instantiate hook returns None → Nil.
        let (mut interp, _state) = mock_scene_interp(&[]);
        let r = interp
            .run("return load(\"res://missing.tscn\").instantiate()\n")
            .unwrap();
        assert_eq!(r.return_value, Some(Variant::Nil));
    }

    #[test]
    fn instantiate_without_scene_access_errors() {
        let mut interp = Interpreter::new();
        let err = interp
            .run("return preload(\"res://x.tscn\").instantiate()\n")
            .unwrap_err();
        assert!(matches!(err.kind, RuntimeErrorKind::TypeError(_)));
    }

    #[test]
    fn load_gd_new_creates_scripted_instance() {
        // `load("x.gd")` yields a GDScript resource; `.new()` asks the host to
        // build a scripted instance and returns its object id (pat-u3l8c.12).
        let (mut interp, state) = mock_scene_interp(&[]);
        state
            .borrow_mut()
            .known_scripts
            .insert("res://enemy.gd".to_string());
        let r = interp
            .run("var e = load(\"res://enemy.gd\").new()\nreturn e\n")
            .unwrap();
        match r.return_value {
            Some(Variant::ObjectId(oid)) => {
                let st = state.borrow();
                assert_eq!(st.instantiated.len(), 1);
                assert_eq!(st.instantiated[0].0, "res://enemy.gd");
                assert_eq!(st.instantiated[0].1, oid.raw());
            }
            other => panic!("expected ObjectId from load(gd).new(), got {other:?}"),
        }
    }

    #[test]
    fn load_missing_gd_new_returns_nil() {
        // Script not registered → host returns None → Nil (Godot's `new()`).
        let (mut interp, _state) = mock_scene_interp(&[]);
        let r = interp
            .run("return load(\"res://missing.gd\").new()\n")
            .unwrap();
        assert_eq!(r.return_value, Some(Variant::Nil));
    }

    #[test]
    fn gd_new_without_scene_access_errors() {
        // No scene access wired → cannot build a scripted instance → error,
        // mirroring `instantiate()` on a PackedScene without scene access.
        let mut interp = Interpreter::new();
        let err = interp
            .run("return load(\"res://x.gd\").new()\n")
            .unwrap_err();
        assert!(matches!(err.kind, RuntimeErrorKind::TypeError(_)));
    }

    // =======================================================================
    // pat-nzgdu.2 — AudioStreamPlayer play/stop/seek/is_playing dispatched
    // from GDScript to the bridge boundary.
    //
    // The interpreter routes `$Player.play()` / `stop()` / `seek()` /
    // `is_playing()` / `get_playback_position()` / `set_stream_paused()` to the
    // exact node properties that `AudioServerBridge::sync_player_node` polls each
    // frame (`playing`, `seek_request`, `stream_paused`, `playback_position`).
    // Before this bead these calls hit the ObjectId method catch-all and threw
    // `UndefinedFunction("ObjectId.play")`; only the autoplay flag or a direct
    // `playing = true` assignment produced sound.
    //
    // This mock stands in for that bridge boundary: it records every
    // `set_node_property` write and serves stored values back through
    // `get_node_property`, so the dispatch is asserted directly in-crate without
    // needing the gdscene `MainLoop` (the full end-to-end mix path is covered by
    // `gdscene/tests/audio_script_dispatch_test.rs`).
    // =======================================================================

    #[derive(Default)]
    struct AudioDispatchState {
        /// node id → (prop → last written value), captured from set_node_property.
        props: std::collections::HashMap<u64, std::collections::HashMap<String, Variant>>,
        /// Node id returned for `get_node("Player")`.
        player_id: u64,
    }

    struct AudioDispatchAccess {
        state: std::rc::Rc<std::cell::RefCell<AudioDispatchState>>,
    }

    impl AudioDispatchState {
        fn prop(&self, node: u64, prop: &str) -> Option<Variant> {
            self.props.get(&node).and_then(|p| p.get(prop)).cloned()
        }
    }

    impl SceneAccess for AudioDispatchAccess {
        fn get_node(&self, _from: u64, path: &str) -> Option<u64> {
            if path == "Player" {
                Some(self.state.borrow().player_id)
            } else {
                None
            }
        }
        fn get_parent(&self, _node: u64) -> Option<u64> {
            None
        }
        fn get_children(&self, _node: u64) -> Vec<u64> {
            Vec::new()
        }
        fn get_node_property(&self, node: u64, prop: &str) -> Variant {
            self.state.borrow().prop(node, prop).unwrap_or(Variant::Nil)
        }
        fn set_node_property(&mut self, node: u64, prop: &str, value: Variant) {
            self.state
                .borrow_mut()
                .props
                .entry(node)
                .or_default()
                .insert(prop.to_string(), value);
        }
        fn emit_signal(&mut self, _node: u64, _signal: &str, _args: &[Variant]) {}
        fn connect_signal(&mut self, _source: u64, _signal: &str, _target: u64, _method: &str) {}
        fn get_node_name(&self, _node: u64) -> Option<String> {
            None
        }
    }

    /// Interpreter wired to the audio-dispatch mock, with the script's own node
    /// id at 1 and the `Player` target at 42.
    fn audio_dispatch_interp() -> (
        Interpreter,
        std::rc::Rc<std::cell::RefCell<AudioDispatchState>>,
    ) {
        let state = std::rc::Rc::new(std::cell::RefCell::new(AudioDispatchState {
            player_id: 42,
            ..Default::default()
        }));
        let mut interp = Interpreter::new();
        interp.set_scene_access(
            Box::new(AudioDispatchAccess {
                state: state.clone(),
            }),
            1,
        );
        (interp, state)
    }

    #[test]
    fn audio_play_dispatches_playing_true() {
        let (mut interp, state) = audio_dispatch_interp();
        interp
            .run("var p = get_node(\"Player\")\np.play()\n")
            .unwrap();
        assert_eq!(
            state.borrow().prop(42, "playing"),
            Some(Variant::Bool(true)),
            "play() must set the node `playing` property the bridge polls",
        );
    }

    #[test]
    fn audio_stop_dispatches_playing_false() {
        let (mut interp, state) = audio_dispatch_interp();
        interp
            .run("var p = get_node(\"Player\")\np.play()\np.stop()\n")
            .unwrap();
        assert_eq!(
            state.borrow().prop(42, "playing"),
            Some(Variant::Bool(false)),
            "stop() after play() must return `playing` to false",
        );
    }

    #[test]
    fn audio_is_playing_reads_live_playing_property() {
        let (mut interp, state) = audio_dispatch_interp();
        // is_playing() reads back the property the previous play()/stop() wrote.
        let r = interp
            .run(
                "var p = get_node(\"Player\")\np.play()\nvar a = p.is_playing()\np.stop()\nvar b = p.is_playing()\nreturn [a, b]\n",
            )
            .unwrap();
        let arr = match r.return_value {
            Some(Variant::Array(a)) => a,
            other => panic!("expected Array, got {other:?}"),
        };
        assert_eq!(
            arr.get(0),
            Some(&Variant::Bool(true)),
            "is_playing() true after play()"
        );
        assert_eq!(
            arr.get(1),
            Some(&Variant::Bool(false)),
            "is_playing() false after stop()"
        );
        // No lingering true state on the boundary.
        assert_eq!(
            state.borrow().prop(42, "playing"),
            Some(Variant::Bool(false))
        );
    }

    #[test]
    fn audio_seek_dispatches_seek_request() {
        let (mut interp, state) = audio_dispatch_interp();
        interp
            .run("var p = get_node(\"Player\")\np.seek(1.5)\n")
            .unwrap();
        assert_eq!(
            state.borrow().prop(42, "seek_request"),
            Some(Variant::Float(1.5)),
            "seek(pos) must request a seek at the given position",
        );
    }

    #[test]
    fn audio_play_from_position_requests_seek_then_plays() {
        let (mut interp, state) = audio_dispatch_interp();
        interp
            .run("var p = get_node(\"Player\")\np.play(2.0)\n")
            .unwrap();
        let st = state.borrow();
        assert_eq!(
            st.prop(42, "seek_request"),
            Some(Variant::Float(2.0)),
            "play(from) must request a seek to `from` first",
        );
        assert_eq!(st.prop(42, "playing"), Some(Variant::Bool(true)));
    }

    #[test]
    fn audio_set_stream_paused_dispatches_stream_paused() {
        let (mut interp, state) = audio_dispatch_interp();
        interp
            .run("var p = get_node(\"Player\")\np.set_stream_paused(true)\n")
            .unwrap();
        assert_eq!(
            state.borrow().prop(42, "stream_paused"),
            Some(Variant::Bool(true)),
            "set_stream_paused(true) must set the `stream_paused` property",
        );
    }

    #[test]
    fn audio_get_playback_position_reads_property() {
        let (mut interp, state) = audio_dispatch_interp();
        state
            .borrow_mut()
            .props
            .entry(42)
            .or_default()
            .insert("playback_position".to_string(), Variant::Float(0.75));
        let r = interp
            .run("var p = get_node(\"Player\")\nreturn p.get_playback_position()\n")
            .unwrap();
        assert_eq!(
            r.return_value,
            Some(Variant::Float(0.75)),
            "get_playback_position() must read back the `playback_position` property",
        );
    }

    // -- Packed arrays: GDScript surface (pat-u3l8c.11) ---------------------

    #[test]
    fn packed_int32_construct_and_index() {
        assert_eq!(
            run_val("var a = PackedInt32Array([10, 20, 30])\nreturn a[1]\n"),
            Variant::Int(20)
        );
    }

    #[test]
    fn packed_byte_array_empty_construct_size() {
        assert_eq!(
            run_val("var a = PackedByteArray()\nreturn a.size()\n"),
            Variant::Int(0)
        );
    }

    #[test]
    fn packed_array_append_persists_size_and_element() {
        assert_eq!(
            run_val("var a = PackedInt32Array()\na.append(5)\na.append(7)\nreturn a.size()\n"),
            Variant::Int(2)
        );
        assert_eq!(
            run_val("var a = PackedInt32Array()\na.append(42)\nreturn a[0]\n"),
            Variant::Int(42)
        );
    }

    #[test]
    fn packed_array_index_assignment() {
        assert_eq!(
            run_val("var a = PackedInt32Array([1, 2, 3])\na[1] = 99\nreturn a[1]\n"),
            Variant::Int(99)
        );
    }

    #[test]
    fn packed_array_negative_index() {
        assert_eq!(
            run_val("var a = PackedInt32Array([1, 2, 3])\nreturn a[-1]\n"),
            Variant::Int(3)
        );
    }

    #[test]
    fn packed_array_resize_fills_zero() {
        assert_eq!(
            run_val("var a = PackedInt32Array([1, 2])\na.resize(4)\nreturn a[3]\n"),
            Variant::Int(0)
        );
    }

    #[test]
    fn packed_array_is_empty() {
        assert_eq!(
            run_val("return PackedByteArray().is_empty()\n"),
            Variant::Bool(true)
        );
        assert_eq!(
            run_val("return PackedByteArray([1]).is_empty()\n"),
            Variant::Bool(false)
        );
    }

    #[test]
    fn packed_array_slice_keeps_type_and_len() {
        assert_eq!(
            run_val(
                "var a = PackedInt32Array([0, 1, 2, 3, 4])\nvar b = a.slice(1, 3)\nreturn b.size()\n"
            ),
            Variant::Int(2)
        );
    }

    #[test]
    fn packed_int32_to_byte_array_len() {
        assert_eq!(
            run_val("var a = PackedInt32Array([1])\nreturn a.to_byte_array().size()\n"),
            Variant::Int(4)
        );
    }

    #[test]
    fn packed_string_array_construct_and_index() {
        assert_eq!(
            run_val("var a = PackedStringArray([\"x\", \"y\"])\nreturn a[1]\n"),
            Variant::String("y".into())
        );
    }

    #[test]
    fn packed_vector2_array_element_is_vector2() {
        assert_eq!(
            run_val("var a = PackedVector2Array([Vector2(1, 2)])\nreturn a[0]\n"),
            Variant::Vector2(gdcore::math::Vector2::new(1.0, 2.0))
        );
    }

    #[test]
    fn packed_array_index_out_of_bounds_errors() {
        let err = run_err("var a = PackedInt32Array([1])\nreturn a[5]\n");
        assert!(matches!(
            err.kind,
            RuntimeErrorKind::IndexOutOfBounds { .. }
        ));
    }
}
