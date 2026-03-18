//! GDScript tree-walk interpreter.
//!
//! Evaluates a parsed GDScript AST, maintaining an environment of scoped
//! variables and a registry of user-defined functions. Built-in functions
//! (print, str, int, float, len, range, typeof) are provided out of the box.

use std::collections::HashMap;

use gdvariant::Variant;

use crate::bindings::{MethodFlags, MethodInfo, ScriptError, ScriptInstance, ScriptPropertyInfo};
use crate::parser::{Annotation, AssignOp, BinOp, Expr, Parser, Stmt, UnaryOp};
use crate::tokenizer::tokenize;

/// Maximum call-stack depth before we bail out.
const MAX_RECURSION_DEPTH: usize = 64;

/// A runtime error produced during interpretation.
#[derive(Debug, Clone, thiserror::Error)]
pub enum RuntimeError {
    /// Reference to a variable that has not been defined.
    #[error("undefined variable: '{0}'")]
    UndefinedVariable(String),

    /// A type mismatch during an operation.
    #[error("type error: {0}")]
    TypeError(String),

    /// Division (or modulo) by zero.
    #[error("division by zero")]
    DivisionByZero,

    /// Call to a function that does not exist.
    #[error("undefined function: '{0}'")]
    UndefinedFunction(String),

    /// Array or string index out of bounds.
    #[error("index out of bounds: {index} (length {length})")]
    IndexOutOfBounds {
        /// The index that was accessed.
        index: i64,
        /// The length of the container.
        length: usize,
    },

    /// Exceeded the maximum recursion depth.
    #[error("maximum recursion depth exceeded ({0})")]
    MaxRecursionDepth(usize),

    /// Propagated parse error.
    #[error("parse error: {0}")]
    ParseError(String),

    /// Propagated lex error.
    #[error("lex error: {0}")]
    LexError(String),
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
        Err(RuntimeError::UndefinedVariable(name.to_string()))
    }

    /// Set an existing variable. Searches scopes from inner to outer.
    fn set(&mut self, name: &str, value: Variant) -> Result<(), RuntimeError> {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), value);
                return Ok(());
            }
        }
        Err(RuntimeError::UndefinedVariable(name.to_string()))
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
#[derive(Debug, Clone)]
pub struct Interpreter {
    environment: Environment,
    function_registry: HashMap<String, FuncDef>,
    output: Vec<String>,
    call_depth: usize,
    /// The current class instance when executing inside a class method.
    self_instance: Option<ClassInstance>,
    /// Registry of known class definitions (for super lookup).
    class_registry: HashMap<String, ClassDef>,
}

/// A stored user-defined function.
#[derive(Debug, Clone)]
pub struct FuncDef {
    /// Parameter names.
    pub params: Vec<String>,
    /// Function body statements.
    pub body: Vec<Stmt>,
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
        }
    }

    /// Tokenizes, parses, and executes a GDScript source string.
    pub fn run(&mut self, source: &str) -> Result<InterpreterResult, RuntimeError> {
        let tokens = tokenize(source).map_err(|e| RuntimeError::LexError(e.to_string()))?;
        let mut parser = Parser::new(tokens);
        let stmts = parser
            .parse_script()
            .map_err(|e| RuntimeError::ParseError(e.to_string()))?;

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
                        return Err(RuntimeError::TypeError(format!(
                            "cannot iterate over {}",
                            other.variant_type()
                        )));
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
                name, params, body, ..
            } => {
                self.function_registry.insert(
                    name.clone(),
                    FuncDef {
                        params: params.clone(),
                        body: body.clone(),
                    },
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

            // Class-level statements are no-ops during normal execution;
            // they are processed by `run_class()`.
            Stmt::Extends { .. }
            | Stmt::ClassNameDecl { .. }
            | Stmt::SignalDecl { .. }
            | Stmt::EnumDecl { .. } => Ok(None),
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

    fn exec_assignment(
        &mut self,
        target: &Expr,
        op: &AssignOp,
        rhs: Variant,
    ) -> Result<(), RuntimeError> {
        match target {
            Expr::Ident(name) => {
                let final_val = match op {
                    AssignOp::Assign => rhs,
                    AssignOp::AddAssign => {
                        let cur = self.environment.get(name)?;
                        self.binary_add(&cur, &rhs)?
                    }
                    AssignOp::SubAssign => {
                        let cur = self.environment.get(name)?;
                        self.binary_sub(&cur, &rhs)?
                    }
                };
                self.environment.set(name, final_val)
            }
            Expr::Index { object, index } => {
                let idx = self.eval_expr(index)?;
                // We need to get the container, mutate it, and set it back.
                let container_name = match object.as_ref() {
                    Expr::Ident(n) => n.clone(),
                    _ => {
                        return Err(RuntimeError::TypeError(
                            "indexed assignment only supported on variables".into(),
                        ));
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
                // Handle self.member = value
                if matches!(object.as_ref(), Expr::SelfRef) {
                    if self.self_instance.is_none() {
                        return Err(RuntimeError::TypeError(
                            "'self' used outside of a class instance".into(),
                        ));
                    }
                    let final_val = match op {
                        AssignOp::Assign => rhs,
                        AssignOp::AddAssign => {
                            let cur = self
                                .self_instance
                                .as_ref()
                                .unwrap()
                                .properties
                                .get(member)
                                .cloned()
                                .unwrap_or(Variant::Nil);
                            self.binary_add(&cur, &rhs)?
                        }
                        AssignOp::SubAssign => {
                            let cur = self
                                .self_instance
                                .as_ref()
                                .unwrap()
                                .properties
                                .get(member)
                                .cloned()
                                .unwrap_or(Variant::Nil);
                            self.binary_sub(&cur, &rhs)?
                        }
                    };
                    self.self_instance
                        .as_mut()
                        .unwrap()
                        .properties
                        .insert(member.clone(), final_val);
                    return Ok(());
                }
                let obj_name = match object.as_ref() {
                    Expr::Ident(n) => n.clone(),
                    _ => {
                        return Err(RuntimeError::TypeError(
                            "member assignment only supported on variables".into(),
                        ));
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
            _ => Err(RuntimeError::TypeError("invalid assignment target".into())),
        }
    }

    // -----------------------------------------------------------------------
    // Expression evaluation
    // -----------------------------------------------------------------------

    fn eval_expr(&mut self, expr: &Expr) -> Result<Variant, RuntimeError> {
        match expr {
            Expr::Literal(v) => Ok(v.clone()),

            Expr::Ident(name) => self.environment.get(name),

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
                        _ => Err(RuntimeError::TypeError(format!(
                            "cannot negate {}",
                            val.variant_type()
                        ))),
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
                        self.call_user_func(name, &evaluated_args)
                    }
                    Expr::MemberAccess { object, member } => {
                        // Handle self.method() — dispatch to class methods
                        if matches!(object.as_ref(), Expr::SelfRef) {
                            return self.call_user_func(member, &evaluated_args);
                        }
                        let obj = self.eval_expr(object)?;
                        self.call_method_on(&obj, member, &evaluated_args, object)
                    }
                    // super() — call parent class method with same name
                    Expr::SuperRef => self.call_super(&evaluated_args),
                    _ => Err(RuntimeError::TypeError("not callable".into())),
                }
            }

            Expr::MemberAccess { object, member } => {
                // Handle self.member specially
                if matches!(object.as_ref(), Expr::SelfRef) {
                    if let Some(ref inst) = self.self_instance {
                        return inst
                            .properties
                            .get(member)
                            .cloned()
                            .ok_or_else(|| RuntimeError::UndefinedVariable(member.clone()));
                    } else {
                        return Err(RuntimeError::TypeError(
                            "'self' used outside of a class instance".into(),
                        ));
                    }
                }
                let obj = self.eval_expr(object)?;
                match &obj {
                    Variant::Dictionary(d) => d
                        .get(member)
                        .cloned()
                        .ok_or_else(|| RuntimeError::UndefinedVariable(member.clone())),
                    _ => Err(RuntimeError::TypeError(format!(
                        "cannot access member on {}",
                        obj.variant_type()
                    ))),
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
                if let Some(ref inst) = self.self_instance {
                    Ok(Variant::Dictionary(inst.properties.clone()))
                } else {
                    Err(RuntimeError::TypeError(
                        "'self' used outside of a class instance".into(),
                    ))
                }
            }

            Expr::SuperRef => {
                // super is only meaningful as a call target; return Nil as marker
                Ok(Variant::Nil)
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
                _ => Err(RuntimeError::TypeError(format!(
                    "cannot use 'in' with {}",
                    rhs.variant_type()
                ))),
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
            _ => Err(RuntimeError::TypeError(format!(
                "cannot add {} and {}",
                lhs.variant_type(),
                rhs.variant_type()
            ))),
        }
    }

    fn binary_sub(&self, lhs: &Variant, rhs: &Variant) -> Result<Variant, RuntimeError> {
        match (lhs, rhs) {
            (Variant::Int(a), Variant::Int(b)) => Ok(Variant::Int(a - b)),
            (Variant::Float(a), Variant::Float(b)) => Ok(Variant::Float(a - b)),
            (Variant::Int(a), Variant::Float(b)) => Ok(Variant::Float(*a as f64 - b)),
            (Variant::Float(a), Variant::Int(b)) => Ok(Variant::Float(a - *b as f64)),
            _ => Err(RuntimeError::TypeError(format!(
                "cannot subtract {} from {}",
                rhs.variant_type(),
                lhs.variant_type()
            ))),
        }
    }

    fn binary_mul(&self, lhs: &Variant, rhs: &Variant) -> Result<Variant, RuntimeError> {
        match (lhs, rhs) {
            (Variant::Int(a), Variant::Int(b)) => Ok(Variant::Int(a * b)),
            (Variant::Float(a), Variant::Float(b)) => Ok(Variant::Float(a * b)),
            (Variant::Int(a), Variant::Float(b)) => Ok(Variant::Float(*a as f64 * b)),
            (Variant::Float(a), Variant::Int(b)) => Ok(Variant::Float(a * *b as f64)),
            _ => Err(RuntimeError::TypeError(format!(
                "cannot multiply {} and {}",
                lhs.variant_type(),
                rhs.variant_type()
            ))),
        }
    }

    fn binary_div(&self, lhs: &Variant, rhs: &Variant) -> Result<Variant, RuntimeError> {
        match (lhs, rhs) {
            (Variant::Int(a), Variant::Int(b)) => {
                if *b == 0 {
                    return Err(RuntimeError::DivisionByZero);
                }
                Ok(Variant::Int(a / b))
            }
            (Variant::Float(a), Variant::Float(b)) => {
                if *b == 0.0 {
                    return Err(RuntimeError::DivisionByZero);
                }
                Ok(Variant::Float(a / b))
            }
            (Variant::Int(a), Variant::Float(b)) => {
                if *b == 0.0 {
                    return Err(RuntimeError::DivisionByZero);
                }
                Ok(Variant::Float(*a as f64 / b))
            }
            (Variant::Float(a), Variant::Int(b)) => {
                if *b == 0 {
                    return Err(RuntimeError::DivisionByZero);
                }
                Ok(Variant::Float(a / *b as f64))
            }
            _ => Err(RuntimeError::TypeError(format!(
                "cannot divide {} by {}",
                lhs.variant_type(),
                rhs.variant_type()
            ))),
        }
    }

    fn binary_mod(&self, lhs: &Variant, rhs: &Variant) -> Result<Variant, RuntimeError> {
        match (lhs, rhs) {
            (Variant::Int(a), Variant::Int(b)) => {
                if *b == 0 {
                    return Err(RuntimeError::DivisionByZero);
                }
                Ok(Variant::Int(a % b))
            }
            (Variant::Float(a), Variant::Float(b)) => {
                if *b == 0.0 {
                    return Err(RuntimeError::DivisionByZero);
                }
                Ok(Variant::Float(a % b))
            }
            (Variant::Int(a), Variant::Float(b)) => {
                if *b == 0.0 {
                    return Err(RuntimeError::DivisionByZero);
                }
                Ok(Variant::Float(*a as f64 % b))
            }
            (Variant::Float(a), Variant::Int(b)) => {
                if *b == 0 {
                    return Err(RuntimeError::DivisionByZero);
                }
                Ok(Variant::Float(a % *b as f64))
            }
            _ => Err(RuntimeError::TypeError(format!(
                "cannot modulo {} by {}",
                lhs.variant_type(),
                rhs.variant_type()
            ))),
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
                return Err(RuntimeError::TypeError(format!(
                    "cannot compare {} and {}",
                    lhs.variant_type(),
                    rhs.variant_type()
                )));
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
                    return Err(RuntimeError::TypeError(
                        "str() takes exactly 1 argument".into(),
                    ));
                }
                Ok(Some(Variant::String(format!("{}", args[0]))))
            }
            "int" => {
                if args.len() != 1 {
                    return Err(RuntimeError::TypeError(
                        "int() takes exactly 1 argument".into(),
                    ));
                }
                match &args[0] {
                    Variant::Int(i) => Ok(Some(Variant::Int(*i))),
                    Variant::Float(f) => Ok(Some(Variant::Int(*f as i64))),
                    Variant::Bool(b) => Ok(Some(Variant::Int(if *b { 1 } else { 0 }))),
                    Variant::String(s) => {
                        let i: i64 = s.parse().map_err(|_| {
                            RuntimeError::TypeError(format!("cannot convert '{s}' to int"))
                        })?;
                        Ok(Some(Variant::Int(i)))
                    }
                    other => Err(RuntimeError::TypeError(format!(
                        "cannot convert {} to int",
                        other.variant_type()
                    ))),
                }
            }
            "float" => {
                if args.len() != 1 {
                    return Err(RuntimeError::TypeError(
                        "float() takes exactly 1 argument".into(),
                    ));
                }
                match &args[0] {
                    Variant::Float(f) => Ok(Some(Variant::Float(*f))),
                    Variant::Int(i) => Ok(Some(Variant::Float(*i as f64))),
                    Variant::Bool(b) => Ok(Some(Variant::Float(if *b { 1.0 } else { 0.0 }))),
                    Variant::String(s) => {
                        let f: f64 = s.parse().map_err(|_| {
                            RuntimeError::TypeError(format!("cannot convert '{s}' to float"))
                        })?;
                        Ok(Some(Variant::Float(f)))
                    }
                    other => Err(RuntimeError::TypeError(format!(
                        "cannot convert {} to float",
                        other.variant_type()
                    ))),
                }
            }
            "len" => {
                if args.len() != 1 {
                    return Err(RuntimeError::TypeError(
                        "len() takes exactly 1 argument".into(),
                    ));
                }
                match &args[0] {
                    Variant::String(s) => Ok(Some(Variant::Int(s.len() as i64))),
                    Variant::Array(a) => Ok(Some(Variant::Int(a.len() as i64))),
                    Variant::Dictionary(d) => Ok(Some(Variant::Int(d.len() as i64))),
                    other => Err(RuntimeError::TypeError(format!(
                        "len() not supported for {}",
                        other.variant_type()
                    ))),
                }
            }
            "range" => match args.len() {
                1 => match &args[0] {
                    Variant::Int(n) => {
                        let arr: Vec<Variant> = (0..*n).map(Variant::Int).collect();
                        Ok(Some(Variant::Array(arr)))
                    }
                    _ => Err(RuntimeError::TypeError(
                        "range() argument must be int".into(),
                    )),
                },
                2 => match (&args[0], &args[1]) {
                    (Variant::Int(start), Variant::Int(end)) => {
                        let arr: Vec<Variant> = (*start..*end).map(Variant::Int).collect();
                        Ok(Some(Variant::Array(arr)))
                    }
                    _ => Err(RuntimeError::TypeError(
                        "range() arguments must be int".into(),
                    )),
                },
                3 => match (&args[0], &args[1], &args[2]) {
                    (Variant::Int(start), Variant::Int(end), Variant::Int(step)) => {
                        if *step == 0 {
                            return Err(RuntimeError::TypeError(
                                "range() step cannot be zero".into(),
                            ));
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
                    _ => Err(RuntimeError::TypeError(
                        "range() arguments must be int".into(),
                    )),
                },
                _ => Err(RuntimeError::TypeError(
                    "range() takes 1, 2, or 3 arguments".into(),
                )),
            },
            "typeof" => {
                if args.len() != 1 {
                    return Err(RuntimeError::TypeError(
                        "typeof() takes exactly 1 argument".into(),
                    ));
                }
                Ok(Some(Variant::String(format!("{}", args[0].variant_type()))))
            }
            "abs" => {
                if args.len() != 1 {
                    return Err(RuntimeError::TypeError(
                        "abs() takes exactly 1 argument".into(),
                    ));
                }
                match &args[0] {
                    Variant::Int(i) => Ok(Some(Variant::Int(i.abs()))),
                    Variant::Float(f) => Ok(Some(Variant::Float(f.abs()))),
                    other => Err(RuntimeError::TypeError(format!(
                        "abs() not supported for {}",
                        other.variant_type()
                    ))),
                }
            }
            _ => Ok(None), // Not a built-in
        }
    }

    fn call_user_func(&mut self, name: &str, args: &[Variant]) -> Result<Variant, RuntimeError> {
        let func = self
            .function_registry
            .get(name)
            .cloned()
            .ok_or_else(|| RuntimeError::UndefinedFunction(name.to_string()))?;

        if args.len() != func.params.len() {
            return Err(RuntimeError::TypeError(format!(
                "{}() takes {} arguments, got {}",
                name,
                func.params.len(),
                args.len()
            )));
        }

        if self.call_depth >= MAX_RECURSION_DEPTH {
            return Err(RuntimeError::MaxRecursionDepth(MAX_RECURSION_DEPTH));
        }
        self.call_depth += 1;

        self.environment.push_scope();
        for (param, arg) in func.params.iter().zip(args.iter()) {
            self.environment.define(param.clone(), arg.clone());
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
        Ok(return_val)
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
                        return Err(RuntimeError::TypeError(format!(
                            "{method}() takes 1 argument"
                        )));
                    }
                    // Mutate the array in-place in the environment
                    let var_name = match object_expr {
                        Expr::Ident(n) => n.clone(),
                        _ => {
                            return Err(RuntimeError::TypeError(
                                "push_back only on variables".into(),
                            ));
                        }
                    };
                    let mut container = self.environment.get(&var_name)?;
                    if let Variant::Array(ref mut a) = container {
                        a.push(args[0].clone());
                    }
                    self.environment.set(&var_name, container)?;
                    Ok(Variant::Nil)
                }
                "pop_back" => {
                    let var_name = match object_expr {
                        Expr::Ident(n) => n.clone(),
                        _ => {
                            return Err(RuntimeError::TypeError(
                                "pop_back only on variables".into(),
                            ));
                        }
                    };
                    let mut container = self.environment.get(&var_name)?;
                    let result = if let Variant::Array(ref mut a) = container {
                        a.pop().unwrap_or(Variant::Nil)
                    } else {
                        Variant::Nil
                    };
                    self.environment.set(&var_name, container)?;
                    Ok(result)
                }
                _ => Err(RuntimeError::UndefinedFunction(format!("Array.{method}"))),
            },
            Variant::String(s) => match method {
                "length" => Ok(Variant::Int(s.len() as i64)),
                _ => Err(RuntimeError::UndefinedFunction(format!("String.{method}"))),
            },
            Variant::Dictionary(d) => match method {
                "size" => Ok(Variant::Int(d.len() as i64)),
                "has" => {
                    if args.len() != 1 {
                        return Err(RuntimeError::TypeError("has() takes 1 argument".into()));
                    }
                    let key = match &args[0] {
                        Variant::String(s) => s.clone(),
                        other => format!("{other}"),
                    };
                    Ok(Variant::Bool(d.contains_key(&key)))
                }
                _ => Err(RuntimeError::UndefinedFunction(format!(
                    "Dictionary.{method}"
                ))),
            },
            _ => Err(RuntimeError::TypeError(format!(
                "cannot call method on {}",
                obj.variant_type()
            ))),
        }
    }

    // -----------------------------------------------------------------------
    // Class system
    // -----------------------------------------------------------------------

    /// Parses a GDScript source as a class definition.
    pub fn run_class(&mut self, source: &str) -> Result<ClassDef, RuntimeError> {
        let tokens = tokenize(source).map_err(|e| RuntimeError::LexError(e.to_string()))?;
        let mut parser = Parser::new(tokens);
        let stmts = parser
            .parse_script()
            .map_err(|e| RuntimeError::ParseError(e.to_string()))?;

        let mut class_def = ClassDef {
            name: None,
            parent_class: None,
            signals: Vec::new(),
            enums: HashMap::new(),
            methods: HashMap::new(),
            instance_vars: Vec::new(),
            exports: Vec::new(),
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
                    name, params, body, ..
                } => {
                    class_def.methods.insert(
                        name.clone(),
                        FuncDef {
                            params: params.clone(),
                            body: body.clone(),
                        },
                    );
                }
                Stmt::VarDecl {
                    name,
                    type_hint,
                    value,
                    annotations,
                } => {
                    let var_decl = VarDecl {
                        name: name.clone(),
                        type_hint: type_hint.clone(),
                        default: value.clone(),
                        annotations: annotations.clone(),
                    };
                    if annotations.iter().any(|a| a.name == "export") {
                        class_def.exports.push(ExportInfo {
                            name: name.clone(),
                            type_hint: type_hint.clone(),
                        });
                    }
                    class_def.instance_vars.push(var_decl);
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
            .ok_or_else(|| RuntimeError::UndefinedFunction(method_name.to_string()))?;

        if args.len() != func.params.len() {
            return Err(RuntimeError::TypeError(format!(
                "{}() takes {} arguments, got {}",
                method_name,
                func.params.len(),
                args.len()
            )));
        }

        if self.call_depth >= MAX_RECURSION_DEPTH {
            return Err(RuntimeError::MaxRecursionDepth(MAX_RECURSION_DEPTH));
        }
        self.call_depth += 1;

        let prev_self = self.self_instance.take();
        self.self_instance = Some(instance.clone());

        for (name, func_def) in &instance.class_def.methods {
            self.function_registry
                .insert(name.clone(), func_def.clone());
        }

        self.environment.push_scope();
        for (param, arg) in func.params.iter().zip(args.iter()) {
            self.environment.define(param.clone(), arg.clone());
        }
        for (name, val) in &instance.properties {
            self.environment.define(name.clone(), val.clone());
        }

        let mut return_val = Variant::Nil;
        for stmt in &func.body {
            if let Some(ControlFlow::Return(v)) = self.exec_stmt(stmt)? {
                return_val = v.unwrap_or(Variant::Nil);
                break;
            }
        }

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
            .ok_or_else(|| RuntimeError::TypeError("super() called but no parent class".into()))?;

        let parent_def = self
            .class_registry
            .get(&parent_name)
            .cloned()
            .ok_or_else(|| {
                RuntimeError::UndefinedFunction(format!("parent class '{parent_name}'"))
            })?;

        if let Some(func) = parent_def.methods.get("_init") {
            if args.len() != func.params.len() {
                return Err(RuntimeError::TypeError(format!(
                    "super() takes {} arguments, got {}",
                    func.params.len(),
                    args.len()
                )));
            }
            self.environment.push_scope();
            for (param, arg) in func.params.iter().zip(args.iter()) {
                self.environment.define(param.clone(), arg.clone());
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
            a.get(index).cloned().ok_or(RuntimeError::IndexOutOfBounds {
                index: *i,
                length: a.len(),
            })
        }
        (Variant::Dictionary(d), Variant::String(k)) => d
            .get(k)
            .cloned()
            .ok_or_else(|| RuntimeError::UndefinedVariable(k.clone())),
        (Variant::String(s), Variant::Int(i)) => {
            let index = if *i < 0 {
                (s.len() as i64 + i) as usize
            } else {
                *i as usize
            };
            s.chars()
                .nth(index)
                .map(|c| Variant::String(c.to_string()))
                .ok_or(RuntimeError::IndexOutOfBounds {
                    index: *i,
                    length: s.len(),
                })
        }
        _ => Err(RuntimeError::TypeError(format!(
            "cannot index {} with {}",
            container.variant_type(),
            idx.variant_type()
        ))),
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
                return Err(RuntimeError::IndexOutOfBounds {
                    index: *i,
                    length: a.len(),
                });
            }
            a[index] = value;
            Ok(())
        }
        (Variant::Dictionary(d), Variant::String(k)) => {
            d.insert(k.clone(), value);
            Ok(())
        }
        _ => Err(RuntimeError::TypeError("invalid index assignment".into())),
    }
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
            .map_err(|e| match e {
                RuntimeError::UndefinedFunction(n) => ScriptError::MethodNotFound(n),
                RuntimeError::TypeError(msg) => ScriptError::TypeError(msg),
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
        assert!(matches!(err, RuntimeError::MaxRecursionDepth(_)));
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
        assert!(matches!(err, RuntimeError::UndefinedVariable(_)));
    }

    #[test]
    fn division_by_zero_error() {
        let err = run_err("return 1 / 0\n");
        assert!(matches!(err, RuntimeError::DivisionByZero));
    }

    #[test]
    fn type_error_add_int_bool() {
        let err = run_err("return 1 + true\n");
        assert!(matches!(err, RuntimeError::TypeError(_)));
    }

    #[test]
    fn undefined_function_error() {
        let err = run_err("foo()\n");
        assert!(matches!(err, RuntimeError::UndefinedFunction(_)));
    }

    #[test]
    fn index_out_of_bounds_error() {
        let err = run_err("return [1, 2][5]\n");
        assert!(matches!(err, RuntimeError::IndexOutOfBounds { .. }));
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
        assert!(matches!(err, RuntimeError::UndefinedFunction(_)));
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
}
