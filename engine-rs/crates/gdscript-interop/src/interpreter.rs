//! GDScript tree-walk interpreter.
//!
//! Evaluates a parsed GDScript AST, maintaining an environment of scoped
//! variables and a registry of user-defined functions. Built-in functions
//! (print, str, int, float, len, range, typeof) are provided out of the box.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use gdvariant::{CallableRef, ResourceRef, Variant};

use crate::bindings::{
    MethodFlags, MethodInfo, SceneAccess, ScriptError, ScriptInstance, ScriptPropertyInfo,
};
use crate::parser::{
    Annotation, AssignOp, BinOp, Expr, FuncParam, MatchPattern, Parser, Stmt, UnaryOp,
};
use crate::tokenizer::tokenize;

/// Maximum call-stack depth before we bail out.
const MAX_RECURSION_DEPTH: usize = 64;

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
}

impl Environment {
    fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// Define a new variable in the current (innermost) scope.
    fn define(&mut self, name: String, value: Variant) {
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
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), value);
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
    /// Scene-tree access for `get_node`, `emit_signal`, etc.
    pub(crate) scene_access: Option<Box<dyn SceneAccess>>,
    /// The raw NodeId of the node this script is attached to.
    pub(crate) current_node_id: Option<u64>,
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
            scene_access: None,
            current_node_id: self.current_node_id,
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
            scene_access: None,
            current_node_id: None,
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
                            ControlFlow::Return(_) => return Ok(Some(cf)),
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
                    if let Some(bindings) = self.match_pattern(&arm.pattern, &val) {
                        self.environment.push_scope();
                        for (n, v) in bindings {
                            self.environment.define(n, v);
                        }
                        let r = self.exec_block_no_scope(&arm.body);
                        self.environment.pop_scope();
                        return r;
                    }
                }
                Ok(None)
            }

            Stmt::Await(expr) => {
                // Simplified v1: evaluate the expression, warn, return Nil.
                let _ = self.eval_expr(expr)?;
                self.output
                    .push("[warning] await: coroutines not fully supported; yielded expression evaluated synchronously".to_string());
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
    fn match_pattern(
        &self,
        pattern: &MatchPattern,
        value: &Variant,
    ) -> Option<Vec<(String, Variant)>> {
        match pattern {
            MatchPattern::Wildcard => Some(vec![]),
            MatchPattern::Variable(name) => Some(vec![(name.clone(), value.clone())]),
            MatchPattern::Literal(lit) => {
                if variant_eq(lit, value) {
                    Some(vec![])
                } else {
                    None
                }
            }
            MatchPattern::Array(patterns) => {
                if let Variant::Array(arr) = value {
                    if arr.len() != patterns.len() {
                        return None;
                    }
                    let mut bindings = vec![];
                    for (pat, val) in patterns.iter().zip(arr.iter()) {
                        match self.match_pattern(pat, val) {
                            Some(b) => bindings.extend(b),
                            None => return None,
                        }
                    }
                    Some(bindings)
                } else {
                    None
                }
            }
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

                let final_val = match op {
                    AssignOp::Assign => rhs,
                    AssignOp::AddAssign => {
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
                        self.binary_add(&cur, &rhs)?
                    }
                    AssignOp::SubAssign => {
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
                        self.binary_sub(&cur, &rhs)?
                    }
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
                // We need to get the container, mutate it, and set it back.
                let container_name = match object.as_ref() {
                    Expr::Ident(n) => n.clone(),
                    _ => {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "indexed assignment only supported on variables".into(),
                        )));
                    }
                };
                let mut container = self.environment.get(&container_name)?;
                let final_val = match op {
                    AssignOp::Assign => rhs,
                    AssignOp::AddAssign => {
                        let cur = index_into(&container, &idx)?;
                        self.binary_add(&cur, &rhs)?
                    }
                    AssignOp::SubAssign => {
                        let cur = index_into(&container, &idx)?;
                        self.binary_sub(&cur, &rhs)?
                    }
                };
                set_index(&mut container, &idx, final_val)?;
                self.environment.set(&container_name, container)
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
                    let final_val = match op {
                        AssignOp::Assign => rhs,
                        AssignOp::AddAssign => {
                            let cur = get_current(self, member);
                            self.binary_add(&cur, &rhs)?
                        }
                        AssignOp::SubAssign => {
                            let cur = get_current(self, member);
                            self.binary_sub(&cur, &rhs)?
                        }
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
                    let final_val = match op {
                        AssignOp::Assign => rhs,
                        AssignOp::AddAssign => {
                            let cur = if let Some(ref access) = self.scene_access {
                                access.get_node_property(oid.raw(), member)
                            } else {
                                Variant::Nil
                            };
                            self.binary_add(&cur, &rhs)?
                        }
                        AssignOp::SubAssign => {
                            let cur = if let Some(ref access) = self.scene_access {
                                access.get_node_property(oid.raw(), member)
                            } else {
                                Variant::Nil
                            };
                            self.binary_sub(&cur, &rhs)?
                        }
                    };
                    if let Some(ref mut access) = self.scene_access {
                        access.set_node_property(oid.raw(), member, final_val);
                    }
                    return Ok(());
                }
                let obj_name = match object.as_ref() {
                    Expr::Ident(n) => n.clone(),
                    _ => {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "member assignment only supported on variables".into(),
                        )));
                    }
                };
                let mut container = self.environment.get(&obj_name)?;
                let final_val = match op {
                    AssignOp::Assign => rhs,
                    AssignOp::AddAssign => {
                        let cur = index_into(&container, &Variant::String(member.clone()))?;
                        self.binary_add(&cur, &rhs)?
                    }
                    AssignOp::SubAssign => {
                        let cur = index_into(&container, &Variant::String(member.clone()))?;
                        self.binary_sub(&cur, &rhs)?
                    }
                };
                set_index(&mut container, &Variant::String(member.clone()), final_val)?;
                self.environment.set(&obj_name, container)
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

        let final_field = match op {
            AssignOp::Assign => rhs,
            AssignOp::AddAssign => self.binary_add(&current_field, &rhs)?,
            AssignOp::SubAssign => self.binary_sub(&current_field, &rhs)?,
        };
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
        let final_val = match op {
            AssignOp::Assign => rhs,
            AssignOp::AddAssign => self.binary_add(&current_field, &rhs)?,
            AssignOp::SubAssign => self.binary_sub(&current_field, &rhs)?,
        };

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
                        // Handle ClassName.new() — runtime node creation
                        if member == "new" {
                            if let Expr::Ident(class_name) = object.as_ref() {
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
                let mut map = HashMap::new();
                for (k, v) in entries {
                    let key = self.eval_expr(k)?;
                    let val = self.eval_expr(v)?;
                    let key_str = match key {
                        Variant::String(s) => s,
                        other => format!("{other}"),
                    };
                    map.insert(key_str, val);
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
                    Ok(Variant::Dictionary(inst.properties.clone()))
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
            BinOp::Eq => Ok(Variant::Bool(variant_eq(lhs, rhs))),
            BinOp::Ne => Ok(Variant::Bool(!variant_eq(lhs, rhs))),
            BinOp::Lt => self.binary_cmp(lhs, rhs, |o| o.is_lt()),
            BinOp::Gt => self.binary_cmp(lhs, rhs, |o| o.is_gt()),
            BinOp::Le => self.binary_cmp(lhs, rhs, |o| o.is_le()),
            BinOp::Ge => self.binary_cmp(lhs, rhs, |o| o.is_ge()),
            BinOp::In => match rhs {
                Variant::Array(a) => Ok(Variant::Bool(a.iter().any(|v| variant_eq(v, lhs)))),
                Variant::Dictionary(d) => {
                    let key = match lhs {
                        Variant::String(s) => s.clone(),
                        other => format!("{other}"),
                    };
                    Ok(Variant::Bool(d.contains_key(&key)))
                }
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
                    Variant::String(s) => Ok(Some(Variant::Int(s.len() as i64))),
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
                Ok(Some(Variant::String(format!("{}", args[0].variant_type()))))
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
                Ok(Some(Variant::Bool(!args[0].is_nil())))
            }
            _ => Ok(None), // Not a built-in
        }
    }

    fn call_user_func(&mut self, name: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
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
        for stmt in &func.body {
            if let Some(ControlFlow::Return(v)) = self.exec_stmt(stmt)? {
                return_val = v.unwrap_or(Variant::Nil);
                break;
            }
        }

        self.environment.pop_scope();
        self.call_depth -= 1;
        self.current_function = prev_function;
        self.call_stack_frames.pop();
        Ok(return_val)
    }

    /// Dispatch an `Input.method()` call through the scene_access input API.
    fn call_input_method(&self, method: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
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
            other => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
                format!("Input.{other}"),
            ))),
        }
    }

    /// Call a method on a value (e.g. array.push_back, array.size, etc.)
    fn call_method_on(
        &mut self,
        obj: &Variant,
        method: &str,
        args: &[Variant],
        object_expr: &Expr,
    ) -> Result<Variant, RuntimeError> {
        match obj {
            Variant::Array(arr) => match method {
                "size" | "length" => Ok(Variant::Int(arr.len() as i64)),
                "push_back" | "append" => {
                    if args.len() != 1 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                            "{method}() takes 1 argument"
                        ))));
                    }
                    let vn = var_name_from_expr(object_expr)?;
                    let mut c = self.environment.get(&vn)?;
                    if let Variant::Array(ref mut a) = c {
                        a.push(args[0].clone());
                    }
                    self.environment.set(&vn, c)?;
                    Ok(Variant::Nil)
                }
                "pop_back" => {
                    let vn = var_name_from_expr(object_expr)?;
                    let mut c = self.environment.get(&vn)?;
                    let r = if let Variant::Array(ref mut a) = c {
                        a.pop().unwrap_or(Variant::Nil)
                    } else {
                        Variant::Nil
                    };
                    self.environment.set(&vn, c)?;
                    Ok(r)
                }
                "sort" => {
                    let vn = var_name_from_expr(object_expr)?;
                    let mut c = self.environment.get(&vn)?;
                    if let Variant::Array(ref mut a) = c {
                        a.sort_by(variant_cmp);
                    }
                    self.environment.set(&vn, c)?;
                    Ok(Variant::Nil)
                }
                "reverse" => {
                    let vn = var_name_from_expr(object_expr)?;
                    let mut c = self.environment.get(&vn)?;
                    if let Variant::Array(ref mut a) = c {
                        a.reverse();
                    }
                    self.environment.set(&vn, c)?;
                    Ok(Variant::Nil)
                }
                "find" => {
                    if args.len() != 1 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "find() takes 1 argument".into(),
                        )));
                    }
                    Ok(Variant::Int(
                        arr.iter()
                            .position(|v| variant_eq(v, &args[0]))
                            .map(|idx| idx as i64)
                            .unwrap_or(-1),
                    ))
                }
                "has" => {
                    if args.len() != 1 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "has() takes 1 argument".into(),
                        )));
                    }
                    Ok(Variant::Bool(arr.iter().any(|v| variant_eq(v, &args[0]))))
                }
                "erase" => {
                    if args.len() != 1 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "erase() takes 1 argument".into(),
                        )));
                    }
                    let vn = var_name_from_expr(object_expr)?;
                    let mut c = self.environment.get(&vn)?;
                    if let Variant::Array(ref mut a) = c {
                        if let Some(pos) = a.iter().position(|v| variant_eq(v, &args[0])) {
                            a.remove(pos);
                        }
                    }
                    self.environment.set(&vn, c)?;
                    Ok(Variant::Nil)
                }
                "insert" => {
                    if args.len() != 2 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "insert() takes 2 arguments".into(),
                        )));
                    }
                    let vn = var_name_from_expr(object_expr)?;
                    let idx = match &args[0] {
                        Variant::Int(ii) => *ii as usize,
                        _ => {
                            return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                                "insert() index must be int".into(),
                            )))
                        }
                    };
                    let mut c = self.environment.get(&vn)?;
                    if let Variant::Array(ref mut a) = c {
                        if idx <= a.len() {
                            a.insert(idx, args[1].clone());
                        }
                    }
                    self.environment.set(&vn, c)?;
                    Ok(Variant::Nil)
                }
                "slice" => {
                    if args.len() != 2 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "slice() takes 2 arguments".into(),
                        )));
                    }
                    match (&args[0], &args[1]) {
                        (Variant::Int(from), Variant::Int(to)) => {
                            let f = (*from).max(0) as usize;
                            let t = ((*to).max(0) as usize).min(arr.len());
                            let f = f.min(t);
                            Ok(Variant::Array(arr[f..t].to_vec()))
                        }
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "slice() arguments must be int".into(),
                        ))),
                    }
                }
                _ => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
                    format!("Array.{method}"),
                ))),
            },
            Variant::String(s) => match method {
                "length" => Ok(Variant::Int(s.len() as i64)),
                "to_upper" => Ok(Variant::String(s.to_uppercase())),
                "to_lower" => Ok(Variant::String(s.to_lowercase())),
                "begins_with" => {
                    if args.len() != 1 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "begins_with() takes 1 argument".into(),
                        )));
                    }
                    match &args[0] {
                        Variant::String(pp) => Ok(Variant::Bool(s.starts_with(pp.as_str()))),
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "begins_with() arg must be string".into(),
                        ))),
                    }
                }
                "ends_with" => {
                    if args.len() != 1 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "ends_with() takes 1 argument".into(),
                        )));
                    }
                    match &args[0] {
                        Variant::String(pp) => Ok(Variant::Bool(s.ends_with(pp.as_str()))),
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "ends_with() arg must be string".into(),
                        ))),
                    }
                }
                "split" => {
                    if args.len() != 1 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "split() takes 1 argument".into(),
                        )));
                    }
                    match &args[0] {
                        Variant::String(d) => Ok(Variant::Array(
                            s.split(d.as_str())
                                .map(|pp| Variant::String(pp.to_string()))
                                .collect(),
                        )),
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "split() arg must be string".into(),
                        ))),
                    }
                }
                "join" => {
                    if args.len() != 1 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "join() takes 1 argument".into(),
                        )));
                    }
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
                    if args.len() != 2 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "replace() takes 2 arguments".into(),
                        )));
                    }
                    match (&args[0], &args[1]) {
                        (Variant::String(from), Variant::String(to)) => {
                            Ok(Variant::String(s.replace(from.as_str(), to.as_str())))
                        }
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "replace() args must be strings".into(),
                        ))),
                    }
                }
                "find" => {
                    if args.len() != 1 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "find() takes 1 argument".into(),
                        )));
                    }
                    match &args[0] {
                        Variant::String(n) => Ok(Variant::Int(
                            s.find(n.as_str()).map(|idx| idx as i64).unwrap_or(-1),
                        )),
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "find() arg must be string".into(),
                        ))),
                    }
                }
                "substr" => {
                    if args.len() != 2 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "substr() takes 2 arguments".into(),
                        )));
                    }
                    match (&args[0], &args[1]) {
                        (Variant::Int(from), Variant::Int(len)) => Ok(Variant::String(
                            s.chars().skip(*from as usize).take(*len as usize).collect(),
                        )),
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "substr() args must be int".into(),
                        ))),
                    }
                }
                _ => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
                    format!("String.{method}"),
                ))),
            },
            Variant::Dictionary(d) => match method {
                "size" => Ok(Variant::Int(d.len() as i64)),
                "has" => {
                    if args.len() != 1 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "has() takes 1 argument".into(),
                        )));
                    }
                    let key = match &args[0] {
                        Variant::String(ss) => ss.clone(),
                        other => format!("{other}"),
                    };
                    Ok(Variant::Bool(d.contains_key(&key)))
                }
                "keys" => Ok(Variant::Array(
                    d.keys().map(|k| Variant::String(k.clone())).collect(),
                )),
                "values" => Ok(Variant::Array(d.values().cloned().collect())),
                "erase" => {
                    if args.len() != 1 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "erase() takes 1 argument".into(),
                        )));
                    }
                    let key = match &args[0] {
                        Variant::String(ss) => ss.clone(),
                        other => format!("{other}"),
                    };
                    let vn = var_name_from_expr(object_expr)?;
                    let mut c = self.environment.get(&vn)?;
                    if let Variant::Dictionary(ref mut dm) = c {
                        dm.remove(&key);
                    }
                    self.environment.set(&vn, c)?;
                    Ok(Variant::Nil)
                }
                "merge" => {
                    if args.len() != 1 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "merge() takes 1 argument".into(),
                        )));
                    }
                    match &args[0] {
                        Variant::Dictionary(other) => {
                            let vn = var_name_from_expr(object_expr)?;
                            let mut c = self.environment.get(&vn)?;
                            if let Variant::Dictionary(ref mut dm) = c {
                                for (k, v) in other {
                                    dm.insert(k.clone(), v.clone());
                                }
                            }
                            self.environment.set(&vn, c)?;
                            Ok(Variant::Nil)
                        }
                        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "merge() arg must be dictionary".into(),
                        ))),
                    }
                }
                "get" => {
                    if args.is_empty() || args.len() > 2 {
                        return Err(RuntimeError::new(RuntimeErrorKind::TypeError(
                            "get() takes 1-2 arguments".into(),
                        )));
                    }
                    let key = match &args[0] {
                        Variant::String(ss) => ss.clone(),
                        other => format!("{other}"),
                    };
                    let default = if args.len() == 2 {
                        args[1].clone()
                    } else {
                        Variant::Nil
                    };
                    Ok(d.get(&key).cloned().unwrap_or(default))
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
                        if let Some(ref mut access) = self.scene_access {
                            access.connect_signal(id, &signal, target_id, &method_name);
                            Ok(Variant::Nil)
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
                    };
                    Ok(Variant::Bool(valid))
                }
                "get_method" => match &**callable_ref {
                    CallableRef::Method { method, .. } => Ok(Variant::String(method.clone())),
                    CallableRef::Lambda { .. } => Ok(Variant::String("<lambda>".into())),
                },
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
                _ => Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(
                    format!("Resource.{method}"),
                ))),
            },
            _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(format!(
                "cannot call method on {}",
                obj.variant_type()
            )))),
        }
    }

    /// Invokes a Callable (lambda or method reference).
    fn invoke_callable(
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
        };

        for stmt in &stmts {
            match stmt {
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
                    let mut map = HashMap::new();
                    for (i, v) in variants.iter().enumerate() {
                        map.insert(v.clone(), i as i64);
                    }
                    class_def.enums.insert(name.clone(), map);
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
                    if annotations.iter().any(|a| a.name == "export") {
                        class_def.exports.push(ExportInfo {
                            name: name.clone(),
                            type_hint: type_hint.clone(),
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
                    };
                    for inner_stmt in body {
                        match inner_stmt {
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
            let val = match &var.default {
                Some(expr) => self.eval_expr(expr)?,
                None => Variant::Nil,
            };
            properties.insert(var.name.clone(), val);
        }
        Ok(ClassInstance {
            class_def: class_def.clone(),
            properties,
        })
    }

    /// Calls a method on a class instance.
    pub fn call_instance_method(
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

        // Push an inner scope for the method body so that local `var`
        // declarations correctly shadow instance properties / parameters.
        self.environment.push_scope();
        let mut return_val = Variant::Nil;
        for stmt in &func.body {
            if let Some(ControlFlow::Return(v)) = self.exec_stmt(stmt)? {
                return_val = v.unwrap_or(Variant::Nil);
                break;
            }
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

fn index_into(container: &Variant, idx: &Variant) -> Result<Variant, RuntimeError> {
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
        (Variant::Dictionary(d), Variant::String(k)) => d
            .get(k)
            .cloned()
            .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::UndefinedVariable(k.clone()))),
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
        (Variant::Dictionary(d), Variant::String(k)) => {
            d.insert(k.clone(), value);
            Ok(())
        }
        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
            "invalid index assignment".into(),
        ))),
    }
}

fn var_name_from_expr(expr: &Expr) -> Result<String, RuntimeError> {
    match expr {
        Expr::Ident(n) => Ok(n.clone()),
        _ => Err(RuntimeError::new(RuntimeErrorKind::TypeError(
            "method only supported on variables".into(),
        ))),
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
        _ => None,
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
}

impl GDScriptInstance {
    /// Creates a new `GDScriptInstance` by parsing and executing the given
    /// source to register its top-level function definitions and variables.
    pub fn from_source(name: &str, source: &str) -> Result<Self, RuntimeError> {
        let mut interpreter = Interpreter::new();
        interpreter.run(source)?;
        Ok(Self {
            interpreter,
            script_name: name.to_string(),
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
            .map(|(name, val)| ScriptPropertyInfo {
                name: name.clone(),
                property_type: val.variant_type(),
                default_value: val.clone(),
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
        assert_eq!(
            run_val("return typeof(42)\n"),
            Variant::String("int".into())
        );
        assert_eq!(
            run_val("return typeof(\"hi\")\n"),
            Variant::String("String".into())
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
        assert_eq!(
            run_val("match 7:\n    x:\n        return x * 2\n"),
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
        assert_eq!(
            run_val("match [10, 20]:\n    [a, b]:\n        return a + b\n"),
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
}
