//! Static type checker for GDScript.
//!
//! Validates typed declarations in a parsed script without executing it:
//!   * `class_name MyClass` — flags duplicate declarations.
//!   * `func foo(x: int) -> String:` — flags literal arguments that do not
//!     match the declared parameter type and returned literals that do not
//!     match the declared return type.
//!   * `var x: int = ...` — flags literal initializers that mismatch the hint.
//!
//! The checker is literal-focused: it infers types only for `Variant` literals
//! (with `int` coercible to `float`). Non-literal expressions are treated as
//! unknown and silently accepted — this mirrors Godot's "loose by default,
//! strict when the hint matches a known shape" behavior and keeps the checker
//! from producing false positives on expressions the interpreter will resolve
//! dynamically at runtime.

use std::collections::HashMap;

use gdvariant::Variant;

use crate::parser::{Expr, FuncParam, ParseError, Parser, Stmt};
use crate::tokenizer::{tokenize, LexError};

/// The kind of static type error reported by [`type_check`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeErrorKind {
    /// More than one `class_name` declaration in a script.
    DuplicateClassName,
    /// A call passes a literal whose type does not match the parameter hint.
    ArgTypeMismatch {
        /// Function name.
        func: String,
        /// Parameter position (0-based).
        param_index: usize,
        /// Parameter name.
        param: String,
        /// Declared type hint.
        expected: String,
        /// Inferred literal type.
        found: String,
    },
    /// A call passes too many or too few arguments (only reported when all
    /// parameters have no defaults, so required arity is unambiguous).
    ArgCountMismatch {
        /// Function name.
        func: String,
        /// Minimum required arg count.
        expected: usize,
        /// Actual arg count.
        found: usize,
    },
    /// A `return <literal>` statement's literal type does not match the
    /// function's declared return type.
    ReturnTypeMismatch {
        /// Function name.
        func: String,
        /// Declared return type.
        expected: String,
        /// Inferred literal type (or "void" for bare `return`).
        found: String,
    },
    /// A `var name: T = <literal>` initializer mismatches `T`.
    VarTypeMismatch {
        /// Variable name.
        var: String,
        /// Declared type hint.
        expected: String,
        /// Inferred literal type.
        found: String,
    },
}

/// A single type error produced by the static checker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeError {
    /// Structured kind describing what went wrong.
    pub kind: TypeErrorKind,
    /// Human-readable description.
    pub message: String,
}

/// Top-level errors the type checker can surface — either a front-end error
/// from the tokenizer/parser, or a collection of type errors.
#[derive(Debug, thiserror::Error)]
pub enum TypeCheckError {
    /// Tokenization failed before type checking could run.
    #[error(transparent)]
    Lex(#[from] LexError),
    /// Parsing failed before type checking could run.
    #[error(transparent)]
    Parse(#[from] ParseError),
}

/// Parses `source` and returns any static type errors. Syntactic errors from
/// the tokenizer/parser are surfaced via [`TypeCheckError`] because the
/// checker cannot run without an AST.
pub fn type_check(source: &str) -> Result<Vec<TypeError>, TypeCheckError> {
    let tokens = tokenize(source)?;
    let mut parser = Parser::new(tokens, source);
    let stmts = parser.parse_script()?;
    Ok(check_stmts(&stmts))
}

fn check_stmts(stmts: &[Stmt]) -> Vec<TypeError> {
    let mut errors = Vec::new();

    let funcs = collect_functions(stmts);
    let mut class_name_count = 0usize;

    for stmt in stmts {
        if let Stmt::ClassNameDecl { .. } = stmt {
            class_name_count += 1;
        }
        check_stmt(stmt, &funcs, &mut errors);
    }

    if class_name_count > 1 {
        errors.push(TypeError {
            kind: TypeErrorKind::DuplicateClassName,
            message: "duplicate class_name declaration".to_string(),
        });
    }

    errors
}

/// A lightweight signature used for checking call sites.
struct FuncSig {
    params: Vec<FuncParam>,
    return_type: Option<String>,
}

fn collect_functions(stmts: &[Stmt]) -> HashMap<String, FuncSig> {
    let mut out = HashMap::new();
    for stmt in stmts {
        if let Stmt::FuncDef {
            name,
            params,
            return_type,
            ..
        } = stmt
        {
            out.insert(
                name.clone(),
                FuncSig {
                    params: params.clone(),
                    return_type: return_type.clone(),
                },
            );
        }
    }
    out
}

fn check_stmt(stmt: &Stmt, funcs: &HashMap<String, FuncSig>, errors: &mut Vec<TypeError>) {
    match stmt {
        Stmt::VarDecl {
            name,
            type_hint: Some(hint),
            value: Some(expr),
            ..
        } => {
            if let Some(found) = literal_type(expr) {
                if !types_compatible(hint, &found) {
                    errors.push(TypeError {
                        kind: TypeErrorKind::VarTypeMismatch {
                            var: name.clone(),
                            expected: hint.clone(),
                            found: found.clone(),
                        },
                        message: format!(
                            "var '{name}' declared as {hint} but initialized with {found}"
                        ),
                    });
                }
            }
            check_expr(expr, funcs, errors);
        }
        Stmt::VarDecl { value, .. } => {
            if let Some(expr) = value {
                check_expr(expr, funcs, errors);
            }
        }
        Stmt::Assignment { target, value, .. } => {
            check_expr(target, funcs, errors);
            check_expr(value, funcs, errors);
        }
        Stmt::ExprStmt(expr) => check_expr(expr, funcs, errors),
        Stmt::If {
            condition,
            body,
            elif_branches,
            else_body,
        } => {
            check_expr(condition, funcs, errors);
            for s in body {
                check_stmt(s, funcs, errors);
            }
            for (cond, branch) in elif_branches {
                check_expr(cond, funcs, errors);
                for s in branch {
                    check_stmt(s, funcs, errors);
                }
            }
            if let Some(body) = else_body {
                for s in body {
                    check_stmt(s, funcs, errors);
                }
            }
        }
        Stmt::While { condition, body } => {
            check_expr(condition, funcs, errors);
            for s in body {
                check_stmt(s, funcs, errors);
            }
        }
        Stmt::For { iterable, body, .. } => {
            check_expr(iterable, funcs, errors);
            for s in body {
                check_stmt(s, funcs, errors);
            }
        }
        Stmt::FuncDef {
            name,
            return_type,
            body,
            ..
        } => {
            for s in body {
                check_stmt(s, funcs, errors);
            }
            if let Some(rt) = return_type {
                check_returns(body, name, rt, errors);
            }
        }
        Stmt::Return(Some(expr)) => {
            check_expr(expr, funcs, errors);
        }
        Stmt::Return(None) => {}
        Stmt::Match { value, arms } => {
            check_expr(value, funcs, errors);
            for arm in arms {
                for s in &arm.body {
                    check_stmt(s, funcs, errors);
                }
            }
        }
        Stmt::InnerClass { body, .. } => {
            let inner_funcs = collect_functions(body);
            for s in body {
                check_stmt(s, &inner_funcs, errors);
            }
        }
        Stmt::Await(expr) => check_expr(expr, funcs, errors),
        _ => {}
    }
}

/// Walks a function body to report return-type mismatches with the owning
/// function's name in the error payload.
fn check_returns(body: &[Stmt], func_name: &str, return_type: &str, errors: &mut Vec<TypeError>) {
    for stmt in body {
        match stmt {
            Stmt::Return(Some(expr)) => {
                if let Some(found) = literal_type(expr) {
                    if !types_compatible(return_type, &found) {
                        errors.push(TypeError {
                            kind: TypeErrorKind::ReturnTypeMismatch {
                                func: func_name.to_string(),
                                expected: return_type.to_string(),
                                found: found.clone(),
                            },
                            message: format!(
                                "function '{func_name}' declared -> {return_type} but returns {found}"
                            ),
                        });
                    }
                }
            }
            Stmt::Return(None) => {
                // Bare `return` in a typed function is a mismatch unless the
                // return type is `void` (GDScript spells this as no arrow —
                // i.e. we only get here when an arrow was declared).
                errors.push(TypeError {
                    kind: TypeErrorKind::ReturnTypeMismatch {
                        func: func_name.to_string(),
                        expected: return_type.to_string(),
                        found: "void".to_string(),
                    },
                    message: format!(
                        "function '{func_name}' declared -> {return_type} but returns nothing"
                    ),
                });
            }
            Stmt::If {
                body,
                elif_branches,
                else_body,
                ..
            } => {
                check_returns(body, func_name, return_type, errors);
                for (_, branch) in elif_branches {
                    check_returns(branch, func_name, return_type, errors);
                }
                if let Some(body) = else_body {
                    check_returns(body, func_name, return_type, errors);
                }
            }
            Stmt::While { body, .. } | Stmt::For { body, .. } => {
                check_returns(body, func_name, return_type, errors);
            }
            Stmt::Match { arms, .. } => {
                for arm in arms {
                    check_returns(&arm.body, func_name, return_type, errors);
                }
            }
            _ => {}
        }
    }
}

fn check_expr(expr: &Expr, funcs: &HashMap<String, FuncSig>, errors: &mut Vec<TypeError>) {
    match expr {
        Expr::Call { callee, args } => {
            if let Expr::Ident(name) = callee.as_ref() {
                if let Some(sig) = funcs.get(name) {
                    check_call(name, sig, args, errors);
                }
            }
            for arg in args {
                check_expr(arg, funcs, errors);
            }
        }
        Expr::BinaryOp { left, right, .. } => {
            check_expr(left, funcs, errors);
            check_expr(right, funcs, errors);
        }
        Expr::UnaryOp { expr, .. } => check_expr(expr, funcs, errors),
        Expr::MemberAccess { object, .. } => check_expr(object, funcs, errors),
        Expr::Index { object, index } => {
            check_expr(object, funcs, errors);
            check_expr(index, funcs, errors);
        }
        Expr::ArrayLiteral(items) => {
            for item in items {
                check_expr(item, funcs, errors);
            }
        }
        Expr::DictLiteral(pairs) => {
            for (k, v) in pairs {
                check_expr(k, funcs, errors);
                check_expr(v, funcs, errors);
            }
        }
        Expr::Ternary {
            value,
            condition,
            else_value,
        } => {
            check_expr(value, funcs, errors);
            check_expr(condition, funcs, errors);
            check_expr(else_value, funcs, errors);
        }
        _ => {}
    }
}

fn check_call(name: &str, sig: &FuncSig, args: &[Expr], errors: &mut Vec<TypeError>) {
    let required = sig
        .params
        .iter()
        .take_while(|p| p.default.is_none())
        .count();
    let max = sig.params.len();
    if args.len() < required || args.len() > max {
        errors.push(TypeError {
            kind: TypeErrorKind::ArgCountMismatch {
                func: name.to_string(),
                expected: required,
                found: args.len(),
            },
            message: format!(
                "call to '{name}': expected {required} args, found {found}",
                found = args.len()
            ),
        });
        return;
    }

    for (index, (param, arg)) in sig.params.iter().zip(args.iter()).enumerate() {
        let Some(hint) = &param.type_hint else {
            continue;
        };
        let Some(found) = literal_type(arg) else {
            continue;
        };
        if !types_compatible(hint, &found) {
            errors.push(TypeError {
                kind: TypeErrorKind::ArgTypeMismatch {
                    func: name.to_string(),
                    param_index: index,
                    param: param.name.clone(),
                    expected: hint.clone(),
                    found: found.clone(),
                },
                message: format!(
                    "call to '{name}': argument '{param}' expects {hint}, found {found}",
                    param = param.name
                ),
            });
        }
    }
}

fn literal_type(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Literal(v) => Some(variant_type_name(v).to_string()),
        Expr::ArrayLiteral(_) => Some("Array".to_string()),
        Expr::DictLiteral(_) => Some("Dictionary".to_string()),
        _ => None,
    }
}

fn variant_type_name(v: &Variant) -> &'static str {
    match v {
        Variant::Nil => "Nil",
        Variant::Bool(_) => "bool",
        Variant::Int(_) => "int",
        Variant::Float(_) => "float",
        Variant::String(_) => "String",
        Variant::StringName(_) => "StringName",
        Variant::NodePath(_) => "NodePath",
        _ => "Variant",
    }
}

/// Checks whether an inferred literal type satisfies a declared hint.
///
/// Rules:
///   * Exact match always compatible.
///   * `int` is compatible with `float` (widening, matches Godot).
///   * Type checker refuses to judge unknown/engine types (returns `true`).
fn types_compatible(expected: &str, found: &str) -> bool {
    if expected == found {
        return true;
    }
    if expected == "float" && found == "int" {
        return true;
    }
    // Unknown engine/user types (e.g. `Node2D`, `Array[int]`) are not
    // narrowed here — only literal mismatches are caught. Any type we can't
    // reason about, we allow.
    !is_builtin_scalar(expected) || !is_builtin_scalar(found)
}

fn is_builtin_scalar(t: &str) -> bool {
    matches!(t, "int" | "float" | "bool" | "String")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_script_has_no_errors() {
        let src = concat!(
            "func add(a: int, b: int) -> int:\n",
            "    return a + b\n",
            "\n",
            "var total: int = 5\n",
        );
        let errors = type_check(src).unwrap();
        assert!(errors.is_empty(), "expected no errors, got {errors:?}");
    }

    #[test]
    fn detects_arg_type_mismatch() {
        let src = concat!(
            "func greet(name: String) -> void:\n",
            "    pass\n",
            "\n",
            "func run() -> void:\n",
            "    greet(42)\n",
        );
        let errors = type_check(src).unwrap();
        assert!(errors
            .iter()
            .any(|e| matches!(e.kind, TypeErrorKind::ArgTypeMismatch { .. })));
    }

    #[test]
    fn detects_return_type_mismatch() {
        let src = concat!(
            "func number() -> int:\n",
            "    return \"forty-two\"\n",
        );
        let errors = type_check(src).unwrap();
        assert!(errors
            .iter()
            .any(|e| matches!(e.kind, TypeErrorKind::ReturnTypeMismatch { .. })));
    }

    #[test]
    fn detects_duplicate_class_name() {
        let src = concat!(
            "class_name Foo\n",
            "class_name Bar\n",
        );
        let errors = type_check(src).unwrap();
        assert!(errors
            .iter()
            .any(|e| matches!(e.kind, TypeErrorKind::DuplicateClassName)));
    }

    #[test]
    fn allows_int_to_float_widening() {
        let src = concat!(
            "func speed(v: float) -> float:\n",
            "    return v\n",
            "\n",
            "func run() -> void:\n",
            "    speed(5)\n",
        );
        let errors = type_check(src).unwrap();
        assert!(errors.is_empty(), "expected widening to succeed: {errors:?}");
    }
}
