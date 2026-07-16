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
}

impl FuncDef {
    /// Constructs a `FuncDef` from parsed [`FuncParam`]s.
    pub fn from_params(params: &[FuncParam], body: &[Stmt], is_static: bool) -> Self {
        Self {
            params: params.iter().map(|p| p.name.clone()).collect(),
            defaults: params.iter().map(|p| p.default.clone()).collect(),
            body: body.to_vec(),
            is_static,
        }
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
    /// Creates a new interpreter with an empty environment.
    pub fn new() -> Self {
        Self {
            environment: Environment::new(),
            function_registry: HashMap::new(),
            output: Vec::new(),
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
                            ControlFlow::Return(_) | ControlFlow::Suspend(_) => {
                                return Ok(Some(cf))
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
                for item in &items {
                    self.environment.define(var.clone(), item.clone());
                    if let Some(cf) = self.exec_block(body)? {
                        match cf {
                            ControlFlow::Break => break,
                            ControlFlow::Continue => continue,
                            ControlFlow::Return(_) | ControlFlow::Suspend(_) => {
                                return Ok(Some(cf))
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
                ..
            } => {
                self.function_registry
                    .insert(name.clone(), FuncDef::from_params(params, body, *is_static));
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
                let awaited = self.eval_expr(expr)?;
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
        for stmt in stmts {
            if let Some(cf) = self.exec_stmt(stmt)? {
                // Return and Suspend both leave the block early.
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
        self.coroutine_host
            .suspend(crate::coroutine::SuspendedCoroutine {
                id: crate::coroutine::CoroutineId(0),
                remaining,
                locals,
                instance_props,
                function_name: function_name.to_string(),
                is_instance_method,
                wait,
                bind_target,
                resume_value: Variant::Nil,
                prior_output: self.output.clone(),
            });
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
        let value = args.first().cloned().unwrap_or(Variant::Nil);
        self.coroutine_host.notify_signal(signal_name, value);
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
        Ok(return_val)
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
                            return Ok(crate::coroutine::AwaitTarget::signal_variant(name));
                        }
                    }
                    // @GlobalScope named constants (KEY_*/OK/ERR_*/TYPE_*/...) are
                    // barewords in GDScript. Resolve them last, after user locals,
                    // members, `const`s, and node properties — matching Godot, where
                    // a local of the same name shadows the global constant. (pat-nzgdu.7)
                    if let Some(value) = crate::global_constants::global_constant(name) {
                        return Ok(value);
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
                        self.call_user_func(name, &evaluated_args)
                    }
                    Expr::MemberAccess { object, member } => {
                        // Handle self.method() — dispatch to class methods
                        // First try builtins (add_child, queue_free, etc.), then user funcs
                        if matches!(object.as_ref(), Expr::SelfRef) {
                            if let Some(result) = self.try_builtin(member, &evaluated_args)? {
                                return Ok(result);
                            }
                            return self.call_user_func(member, &evaluated_args);
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
                        // Handle ClassName.new() — runtime node creation / RefCounted helpers
                        if member == "new" {
                            if let Expr::Ident(class_name) = object.as_ref() {
                                if class_name == "ConfigFile" {
                                    return Ok(Variant::ObjectId(
                                        gdresource::ConfigFile::new_handle(),
                                    ));
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
                    _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                        "cannot access member on {}",
                        obj.variant_type()
                    )))),
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
        )
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
            _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                "cannot multiply {} and {}",
                lhs.variant_type(),
                rhs.variant_type()
            )))),
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
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "str() takes exactly 1 argument".into(),
                    )));
                }
                Ok(Some(Variant::String(format!("{}", args[0]))))
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
                    Variant::Vector2(v) => Ok(Some(Variant::Vector2(
                        gdcore::math::Vector2::new(v.x.abs(), v.y.abs()),
                    ))),
                    Variant::Vector3(v) => Ok(Some(Variant::Vector3(
                        gdcore::math::Vector3::new(v.x.abs(), v.y.abs(), v.z.abs()),
                    ))),
                    other => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                        "abs() not supported for {}",
                        other.variant_type()
                    )))),
                }
            }
            "preload" | "load" => {
                if args.len() != 1 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "preload/load takes 1 argument".into(),
                    )));
                }
                match &args[0] {
                    Variant::String(path) => {
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
                if args.len() != 2 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "min() takes 2 arguments".into(),
                    )));
                }
                Ok(Some(float_or_int(
                    to_float(&args[0])?.min(to_float(&args[1])?),
                    &args[0],
                    &args[1],
                )))
            }
            "max" => {
                if args.len() != 2 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "max() takes 2 arguments".into(),
                    )));
                }
                Ok(Some(float_or_int(
                    to_float(&args[0])?.max(to_float(&args[1])?),
                    &args[0],
                    &args[1],
                )))
            }
            "clamp" => {
                if args.len() != 3 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "clamp() takes 3 arguments".into(),
                    )));
                }
                let v = to_float(&args[0])?;
                let lo = to_float(&args[1])?;
                let hi = to_float(&args[2])?;
                Ok(Some(float_or_int(v.max(lo).min(hi), &args[0], &args[1])))
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
            "Vector2" => {
                if args.len() != 2 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "Vector2() takes 2 arguments".into(),
                    )));
                }
                let x = to_float(&args[0])? as f32;
                let y = to_float(&args[1])? as f32;
                Ok(Some(Variant::Vector2(gdcore::math::Vector2::new(x, y))))
            }
            "Vector3" => {
                if args.len() != 3 {
                    return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                        "Vector3() takes 3 arguments".into(),
                    )));
                }
                let x = to_float(&args[0])? as f32;
                let y = to_float(&args[1])? as f32;
                let z = to_float(&args[2])? as f32;
                Ok(Some(Variant::Vector3(gdcore::math::Vector3::new(x, y, z))))
            }
            "Color" => match args.len() {
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
                    "Color() takes 3 or 4 arguments".into(),
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
                let value = to_float(&args[0])?;
                let step = to_float(&args[1])?;
                let result = if step != 0.0 {
                    (value / step + 0.5).floor() * step
                } else {
                    value
                };
                Ok(Some(float_or_int(result, &args[0], &args[1])))
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
                Ok(Some(v))
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
    fn call_input_method(&mut self, method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
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
        }
        match obj {
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
                        if args.len() < 3 {
                            return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "connect() needs signal, target, method".into(),
                            )));
                        }
                        let signal = match &args[0] {
                            Variant::String(s) => s.clone(),
                            _ => {
                                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                    "connect() signal must be string".into(),
                                )))
                            }
                        };
                        let target_id = match &args[1] {
                            Variant::ObjectId(tid) => tid.raw(),
                            _ => {
                                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                    "connect() target must be ObjectId".into(),
                                )))
                            }
                        };
                        let method_name = match &args[2] {
                            Variant::String(s) => s.clone(),
                            _ => {
                                return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                    "connect() method must be string".into(),
                                )))
                            }
                        };
                        // Optional 4th arg = Godot ConnectFlags bitmask
                        // (DEFERRED=1, ONE_SHOT=4, ...). Older callers omit it.
                        let flags = match args.get(3) {
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
                            other => return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                format!(
                                    "call_deferred() method name must be a String, got {other:?}"
                                ),
                            ))),
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
                            access.set_node_property(id, "playing", Variant::Bool(false));
                            Ok(Variant::Nil)
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    "is_playing" => {
                        if let Some(ref access) = self.scene_access {
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
                            access.set_node_property(id, "seek_request", Variant::Float(pos));
                            Ok(Variant::Nil)
                        } else {
                            Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "no scene access".into(),
                            )))
                        }
                    }
                    "get_playback_position" => {
                        if let Some(ref access) = self.scene_access {
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
                    _ => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
                        format!("ObjectId.{method}"),
                    ))),
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

    fn invoke_callable_impl(
        &mut self,
        callable: &CallableRef,
        args: &[Variant],
    ) -> Result<Variant, RuntimeError> {
        match callable {
            CallableRef::Method { method, .. } => {
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
                    ..
                } => {
                    class_def
                        .methods
                        .insert(name.clone(), FuncDef::from_params(params, body, *is_static));
                }
                Stmt::VarDecl {
                    name,
                    type_hint,
                    value,
                    annotations,
                    setter,
                    getter,
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
                                ..
                            } => {
                                inner.methods.insert(
                                    name.clone(),
                                    FuncDef::from_params(params, body, *is_static),
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
    if s.contains('.') || s.contains('e') || s.contains('E') || s.contains("inf") || s.contains("nan")
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
                Some(Variant::Vector3(gdcore::math::Vector3::new(v[0], v[1], v[2])))
            }
            "Color" => {
                let v = self.parse_ctor_floats(4)?;
                Some(Variant::Color(gdcore::math::Color::new(v[0], v[1], v[2], v[3])))
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
    p.parse_value().unwrap_or(Variant::Nil)
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
        // @GlobalScope enum-qualified access (`Key.KEY_SPACE`, `Error.OK`, ...).
        // The 20 bareword global-enum prefixes delegate their member to the flat
        // @GlobalScope constant table. (pat-nzgdu.7)
        _ => crate::global_constants::global_enum_member(type_name, member),
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
                    cfg!(any(target_os = "macos", target_os = "windows", target_os = "linux"))
                }
                "mobile" => false,
                _ => false,
            };
            Ok(Variant::Bool(present))
        }
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
            .or_else(|| std::env::var("HOME").ok().map(|h| format!("{h}/.local/share")))
            .unwrap_or_default()
    };
    if base.is_empty() {
        "user://".to_string()
    } else {
        format!("{base}/Patina")
    }
}

/// `ProjectSettings` singleton (pat-u3l8c.21). No `project.godot` is loaded in
/// the interpreter runtime, so `get_setting` returns the caller default (Godot
/// semantics for a missing setting) and `globalize_path` resolves the
/// `res://` / `user://` schemes to filesystem paths.
fn call_projectsettings_static(method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
    match method {
        "get_setting" => Ok(args.get(1).cloned().unwrap_or(Variant::Nil)),
        "has_setting" => Ok(Variant::Bool(false)),
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
        other => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
            format!("ProjectSettings.{other}"),
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
        assert_eq!(run_val("return OS.has_feature(\"template\")\n"), Variant::Bool(true));
        assert_eq!(run_val("return OS.has_feature(\"editor\")\n"), Variant::Bool(false));
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
        interp.set_scene_access(Box::new(InputCmdAccess { state: state.clone() }), 1);
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
        let r = interp.run("return Input.get_connected_joypads()\n").unwrap();
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
