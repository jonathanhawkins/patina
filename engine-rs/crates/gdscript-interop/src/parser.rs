//! GDScript expression and statement parser.
//!
//! Parses a token stream produced by [`crate::tokenizer::tokenize`] into an AST
//! of [`Expr`] and [`Stmt`] nodes using precedence climbing for expressions.

use gdvariant::Variant;

use crate::tokenizer::{Token, TokenSpan};

/// An expression node in the GDScript AST.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// A literal value.
    Literal(Variant),
    /// A variable or name reference.
    Ident(String),
    /// A binary operation (e.g. `a + b`).
    BinaryOp {
        /// Left-hand operand.
        left: Box<Expr>,
        /// Operator token.
        op: BinOp,
        /// Right-hand operand.
        right: Box<Expr>,
    },
    /// A unary operation (e.g. `-x`, `not x`).
    UnaryOp {
        /// Operator.
        op: UnaryOp,
        /// Operand.
        expr: Box<Expr>,
    },
    /// A function call (e.g. `foo(a, b)`).
    Call {
        /// The expression being called.
        callee: Box<Expr>,
        /// Arguments.
        args: Vec<Expr>,
    },
    /// Member access (e.g. `obj.field`).
    MemberAccess {
        /// The object expression.
        object: Box<Expr>,
        /// The member name.
        member: String,
    },
    /// Index access (e.g. `arr[0]`).
    Index {
        /// The object expression.
        object: Box<Expr>,
        /// The index expression.
        index: Box<Expr>,
    },
    /// Array literal (e.g. `[1, 2, 3]`).
    ArrayLiteral(Vec<Expr>),
    /// Dictionary literal (e.g. `{"a": 1}`).
    DictLiteral(Vec<(Expr, Expr)>),
    /// `self` reference.
    SelfRef,
    /// `super` reference.
    SuperRef,
    Ternary {
        value: Box<Expr>,
        condition: Box<Expr>,
        else_value: Box<Expr>,
    },
    /// `$NodeName` / `$"Path/To/Node"` sugar for get_node.
    GetNode(String),
    /// A lambda / anonymous function: `func(x): return x + 1`
    Lambda {
        /// Parameter names.
        params: Vec<FuncParam>,
        /// Function body.
        body: Vec<Stmt>,
    },
    /// Type test: `value is TypeName` (Godot `is` operator).
    TypeTest {
        /// The value being tested.
        value: Box<Expr>,
        /// The type name to test against.
        type_name: String,
    },
    /// Type cast: `value as TypeName` (Godot `as` operator).
    Cast {
        /// The value being cast.
        value: Box<Expr>,
        /// The target type name.
        type_name: String,
    },
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `%`
    Mod,
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `<=`
    Le,
    /// `>=`
    Ge,
    /// `and`
    And,
    /// `or`
    Or,
    /// `in`
    In,
    /// `**`
    Pow,
    /// `&`
    BitAnd,
    /// `|`
    BitOr,
    /// `^`
    BitXor,
    /// `<<`
    Shl,
    /// `>>`
    Shr,
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    /// `-`
    Neg,
    /// `not`
    Not,
    /// `~`
    BitNot,
}

/// Assignment operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    /// `=`
    Assign,
    /// `+=`
    AddAssign,
    /// `-=`
    SubAssign,
    /// `*=`
    MulAssign,
    /// `/=`
    DivAssign,
    /// `%=`
    ModAssign,
    /// `**=`
    PowAssign,
    /// `&=`
    BitAndAssign,
    /// `|=`
    BitOrAssign,
    /// `^=`
    BitXorAssign,
    /// `<<=`
    ShlAssign,
    /// `>>=`
    ShrAssign,
}

/// An annotation on a declaration (e.g. `@export`, `@onready`, `@rpc`).
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    /// Annotation name (e.g. "export", "onready", "rpc").
    pub name: String,
    /// Parenthesised arguments captured verbatim (string literals unquoted),
    /// e.g. `@rpc("any_peer", "call_local", "reliable", 0)` parses to
    /// `["any_peer", "call_local", "reliable", "0"]`. Empty for bare annotations
    /// like `@export` (pat-u3l8c.75).
    pub args: Vec<String>,
}

/// A function parameter with optional type hint and default value.
#[derive(Debug, Clone, PartialEq)]
pub struct FuncParam {
    /// Parameter name.
    pub name: String,
    /// Optional type hint (e.g. `int`, `String`).
    pub type_hint: Option<String>,
    /// Optional default value expression.
    pub default: Option<Expr>,
}

/// A statement node in the GDScript AST.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// Constant declaration: `const NAME [: type] = value`
    ConstDecl {
        /// Constant name.
        name: String,
        /// Optional type hint.
        type_hint: Option<String>,
        /// The constant value expression.
        value: Expr,
    },
    /// Variable declaration: `var name [: type] [= value]`
    VarDecl {
        /// Variable name.
        name: String,
        /// Optional type hint.
        type_hint: Option<String>,
        /// Optional initial value.
        value: Option<Expr>,
        /// Annotations (e.g. `@export`).
        annotations: Vec<Annotation>,
        /// Optional setter function name (Godot 4: `set = _set_name`).
        setter: Option<String>,
        /// Optional getter function name (Godot 4: `get = _get_name`).
        getter: Option<String>,
    },
    /// Assignment: `target op value`
    Assignment {
        /// The assignment target.
        target: Expr,
        /// The assignment operator.
        op: AssignOp,
        /// The value being assigned.
        value: Expr,
    },
    /// If statement with optional elif/else branches.
    If {
        /// The primary condition.
        condition: Expr,
        /// The body when condition is true.
        body: Vec<Stmt>,
        /// Zero or more `elif` branches.
        elif_branches: Vec<(Expr, Vec<Stmt>)>,
        /// Optional `else` body.
        else_body: Option<Vec<Stmt>>,
    },
    /// While loop.
    While {
        /// Loop condition.
        condition: Expr,
        /// Loop body.
        body: Vec<Stmt>,
    },
    /// For loop: `for var in iterable:`
    For {
        /// Loop variable name.
        var: String,
        /// The iterable expression.
        iterable: Expr,
        /// Loop body.
        body: Vec<Stmt>,
    },
    /// Return statement.
    Return(Option<Expr>),
    /// Function definition.
    FuncDef {
        /// Function name.
        name: String,
        /// Function parameters with optional type hints and defaults.
        params: Vec<FuncParam>,
        /// Optional return type hint.
        return_type: Option<String>,
        /// Function body.
        body: Vec<Stmt>,
        /// Whether this is a static function.
        is_static: bool,
        /// Annotations applied to the function, e.g. `@rpc(...)` (pat-u3l8c.75).
        annotations: Vec<Annotation>,
    },
    /// `extends ClassName` or `extends "ClassName"`.
    Extends {
        /// The parent class name.
        class_name: String,
    },
    /// `class_name MyClass`.
    ClassNameDecl {
        /// The class name.
        name: String,
    },
    /// `signal signal_name(param1, param2)`.
    SignalDecl {
        /// Signal name.
        name: String,
        /// Parameter names.
        params: Vec<String>,
    },
    /// `enum MyEnum { IDLE, RUNNING, JUMPING }`.
    EnumDecl {
        /// Enum name.
        name: String,
        /// Variant names (assigned ascending integer values starting at 0).
        variants: Vec<String>,
    },
    /// An expression used as a statement.
    ExprStmt(Expr),
    Match {
        value: Expr,
        arms: Vec<MatchArm>,
    },
    /// `pass`
    Pass,
    /// `break`
    Break,
    /// `continue`
    Continue,
    /// `await expr` — yields execution until the awaited expression resolves.
    /// `bind` is `Some(name)` for `var name = await expr`, binding the resumed
    /// value (a signal's emitted argument) to `name` (pat-nzgdu.6); `None` for a
    /// bare `await expr` whose result is discarded.
    Await {
        /// The awaited expression (produces an awaitable signal/timer/frames).
        expr: Expr,
        /// Optional local to bind the resumed value to.
        bind: Option<String>,
    },
    /// Inner class definition: `class InnerName:`
    InnerClass {
        /// The inner class name.
        name: String,
        /// The body statements of the inner class.
        body: Vec<Stmt>,
    },
    /// A standalone annotation statement (e.g. `@tool`).
    AnnotationStmt {
        /// The annotations.
        annotations: Vec<Annotation>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    /// One or more comma-separated patterns; the arm matches if any matches.
    pub patterns: Vec<MatchPattern>,
    /// Optional `when` guard expression evaluated after a pattern matches.
    pub guard: Option<Expr>,
    pub body: Vec<Stmt>,
}
#[derive(Debug, Clone, PartialEq)]
pub enum MatchPattern {
    /// A literal value compared by equality.
    Literal(Variant),
    /// `var name` — binds the matched value to `name`.
    Binding(String),
    /// A bare identifier or expression evaluated and compared by value
    /// (a constant reference, e.g. `RED` or `Color.RED`).
    ConstantRef(Expr),
    /// `_` — matches anything without binding.
    Wildcard,
    /// `..` — rest pattern (only meaningful inside array/dictionary patterns).
    Rest,
    /// `[a, b, ..]` — array pattern; a trailing [`MatchPattern::Rest`] allows
    /// extra elements.
    Array(Vec<MatchPattern>),
    /// `{ key: subpattern, .. }` — dictionary pattern.
    Dictionary {
        /// Key expression and optional value sub-pattern for each entry.
        entries: Vec<(Expr, Option<MatchPattern>)>,
        /// Whether a trailing `..` rest allows extra keys.
        rest: bool,
    },
}

/// A parse error.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ParseError {
    /// An unexpected token was found.
    #[error("unexpected token {token} at line {line}, col {col}; expected {expected}")]
    UnexpectedToken {
        /// The token that was found.
        token: String,
        /// What was expected.
        expected: String,
        /// Line number.
        line: usize,
        /// Column number.
        col: usize,
        source_line: Option<String>,
    },

    /// Reached end of input unexpectedly.
    #[error("unexpected end of input")]
    UnexpectedEof,
}

/// GDScript parser.
///
/// Consumes a flat token stream and produces an AST of statements.
pub struct Parser {
    tokens: Vec<TokenSpan>,
    pos: usize,
    source_lines: Vec<String>,
}

impl Parser {
    /// Creates a new parser from a token stream.
    pub fn new(tokens: Vec<TokenSpan>, source: &str) -> Self {
        Self {
            tokens,
            pos: 0,
            source_lines: source.lines().map(|l| l.to_string()).collect(),
        }
    }

    /// Parses a complete script into a list of top-level statements.
    pub fn parse_script(&mut self) -> Result<Vec<Stmt>, ParseError> {
        let mut stmts = Vec::new();
        self.skip_newlines();
        while !self.check(&Token::Eof) {
            stmts.push(self.parse_stmt()?);
            self.skip_newlines();
        }
        Ok(stmts)
    }

    // --- Helpers ---

    fn peek(&self) -> &Token {
        self.tokens
            .get(self.pos)
            .map(|ts| &ts.token)
            .unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> &TokenSpan {
        let ts = &self.tokens[self.pos];
        self.pos += 1;
        ts
    }

    fn check(&self, expected: &Token) -> bool {
        std::mem::discriminant(self.peek()) == std::mem::discriminant(expected)
    }

    fn expect(&mut self, expected: &Token) -> Result<(), ParseError> {
        if self.check(expected) {
            self.advance();
            Ok(())
        } else {
            let ts = self.tokens.get(self.pos);
            let (token, line, col) = match ts {
                Some(ts) => (ts.token.to_string(), ts.line, ts.col),
                None => ("EOF".to_string(), 0, 0),
            };
            Err(ParseError::UnexpectedToken {
                token,
                expected: expected.to_string(),
                line,
                col,
                source_line: if line > 0 {
                    self.source_lines.get(line - 1).cloned()
                } else {
                    None
                },
            })
        }
    }

    fn skip_newlines(&mut self) {
        while self.check(&Token::Newline) {
            self.advance();
        }
    }

    fn eat_ident(&mut self) -> Result<String, ParseError> {
        match self.peek().clone() {
            Token::Ident(name) => {
                self.advance();
                Ok(name)
            }
            _ => {
                let ts = self.tokens.get(self.pos);
                let (token, line, col) = match ts {
                    Some(ts) => (ts.token.to_string(), ts.line, ts.col),
                    None => ("EOF".to_string(), 0, 0),
                };
                Err(ParseError::UnexpectedToken {
                    token,
                    expected: "identifier".to_string(),
                    line,
                    col,
                    source_line: None,
                })
            }
        }
    }

    // --- Statement parsing ---

    fn parse_stmt(&mut self) -> Result<Stmt, ParseError> {
        match self.peek().clone() {
            Token::AtSign => self.parse_annotated_stmt(),
            Token::Var => self.parse_var_decl_with_annotations(vec![]),
            Token::Const => self.parse_const_decl(),
            Token::If => self.parse_if(),
            Token::While => self.parse_while(),
            Token::For => self.parse_for(),
            Token::Return => self.parse_return(),
            Token::Func => self.parse_func_def(false, Vec::new()),
            Token::Static => self.parse_static_func(),
            Token::Extends => self.parse_extends(),
            Token::ClassName => self.parse_class_name_decl(),
            Token::Signal => self.parse_signal_decl(),
            Token::Enum => self.parse_enum_decl(),
            Token::Pass => {
                self.advance();
                Ok(Stmt::Pass)
            }
            Token::Break => {
                self.advance();
                Ok(Stmt::Break)
            }
            Token::Continue => {
                self.advance();
                Ok(Stmt::Continue)
            }
            Token::Match => self.parse_match(),
            Token::Await => self.parse_await(),
            Token::Class => self.parse_inner_class(),
            _ => self.parse_expr_or_assign(),
        }
    }

    fn parse_annotated_stmt(&mut self) -> Result<Stmt, ParseError> {
        let mut annotations = Vec::new();
        while self.check(&Token::AtSign) {
            self.advance(); // consume `@`
                            // Annotation names may be keywords (e.g. `@export`, `@onready`).
            let name = match self.peek().clone() {
                Token::Ident(n) => {
                    self.advance();
                    n
                }
                other => {
                    // Accept any keyword-like token as an annotation name
                    // by using its Display representation.
                    let n = other.to_string();
                    self.advance();
                    n
                }
            };
            // Optional parenthesised arguments, e.g. `@rpc(mode, sync, transfer,
            // channel)`. Captured verbatim (string literals unquoted) so the
            // runtime can map them to an RPC configuration (pat-u3l8c.75).
            let mut args = Vec::new();
            if self.check(&Token::LParen) {
                self.advance(); // consume '('
                while !self.check(&Token::RParen) && !self.check(&Token::Eof) {
                    let arg = match self.peek().clone() {
                        Token::StringLit(s) => s,
                        other => other.to_string(),
                    };
                    args.push(arg);
                    self.advance();
                    if self.check(&Token::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
                if self.check(&Token::RParen) {
                    self.advance(); // consume ')'
                }
            }
            annotations.push(Annotation { name, args });
            self.skip_newlines();
        }
        // After annotations, expect a var decl, func def, or script-level annotation like @tool
        match self.peek().clone() {
            Token::Var => self.parse_var_decl_with_annotations(annotations),
            Token::Func => {
                // Attach parsed annotations (e.g. `@rpc(...)`) to the function
                // so the runtime can enforce their configuration (pat-u3l8c.75).
                self.parse_func_def(false, annotations)
            }
            Token::Static => self.parse_static_func(),
            _ => {
                // Script-level annotations like @tool: emit as standalone annotation statement
                if annotations.iter().any(|a| a.name == "tool") {
                    return Ok(Stmt::AnnotationStmt { annotations });
                }
                let ts = self.tokens.get(self.pos);
                let (token, line, col) = match ts {
                    Some(ts) => (ts.token.to_string(), ts.line, ts.col),
                    None => ("EOF".to_string(), 0, 0),
                };
                Err(ParseError::UnexpectedToken {
                    token,
                    expected: "var or func declaration after annotation".to_string(),
                    line,
                    col,
                    source_line: None,
                })
            }
        }
    }

    /// Parses a type hint, handling `Array[ElementType]` syntax.
    fn parse_type_hint(&mut self) -> Result<String, ParseError> {
        let base = self.eat_ident()?;
        if base == "Array" && self.check(&Token::LBracket) {
            self.advance(); // consume `[`
            let element_type = self.eat_ident()?;
            self.expect(&Token::RBracket)?;
            Ok(format!("Array[{element_type}]"))
        } else {
            Ok(base)
        }
    }

    fn parse_var_decl_with_annotations(
        &mut self,
        annotations: Vec<Annotation>,
    ) -> Result<Stmt, ParseError> {
        self.advance(); // consume `var`
        let name = self.eat_ident()?;
        let type_hint = if self.check(&Token::Colon) {
            self.advance();
            // Peek: if next is `set` or `get` immediately, there's no type hint
            match self.peek() {
                Token::Ident(n) if n == "set" || n == "get" => None,
                _ => Some(self.parse_type_hint()?),
            }
        } else {
            None
        };
        let value = if self.check(&Token::Assign) {
            self.advance();
            if self.check(&Token::Await) {
                // `var NAME = await EXPR` binds the resumed value (a signal's
                // emitted argument) to NAME. It must lower to an `await`
                // statement so it can suspend the frame — a plain VarDecl can't
                // (expression evaluation cannot yield). Setter/getter and type
                // hints are not combined with an awaited initializer (pat-nzgdu.6).
                self.advance(); // consume `await`
                let awaited = self.parse_expr()?;
                return Ok(Stmt::Await {
                    expr: awaited,
                    bind: Some(name),
                });
            }
            Some(self.parse_expr()?)
        } else {
            None
        };
        // Parse setter/getter: `var x: int = 0: set = _set_x, get = _get_x`
        // Or after value: `var x = 0: set = _set_x, get = _get_x`
        let (setter, getter) = if self.check(&Token::Colon) {
            self.advance();
            self.parse_setter_getter()?
        } else {
            (None, None)
        };
        Ok(Stmt::VarDecl {
            name,
            type_hint,
            value,
            annotations,
            setter,
            getter,
        })
    }

    fn parse_const_decl(&mut self) -> Result<Stmt, ParseError> {
        self.advance(); // consume `const`
        let name = self.eat_ident()?;
        let type_hint = if self.check(&Token::Colon) {
            self.advance();
            Some(self.parse_type_hint()?)
        } else {
            None
        };
        self.expect(&Token::Assign)?;
        let value = self.parse_expr()?;
        Ok(Stmt::ConstDecl {
            name,
            type_hint,
            value,
        })
    }

    /// Parses `set = _setter, get = _getter` in any order.
    fn parse_setter_getter(&mut self) -> Result<(Option<String>, Option<String>), ParseError> {
        let mut setter = None;
        let mut getter = None;
        loop {
            match self.peek().clone() {
                Token::Ident(ref kw) if kw == "set" => {
                    self.advance();
                    self.expect(&Token::Assign)?;
                    setter = Some(self.eat_ident()?);
                }
                Token::Ident(ref kw) if kw == "get" => {
                    self.advance();
                    self.expect(&Token::Assign)?;
                    getter = Some(self.eat_ident()?);
                }
                _ => break,
            }
            if !self.check(&Token::Comma) {
                break;
            }
            self.advance(); // consume comma
        }
        Ok((setter, getter))
    }

    fn parse_extends(&mut self) -> Result<Stmt, ParseError> {
        self.advance(); // consume `extends`
        let class_name = match self.peek().clone() {
            Token::StringLit(s) => {
                self.advance();
                s
            }
            Token::Ident(name) => {
                self.advance();
                name
            }
            _ => {
                let ts = self.tokens.get(self.pos);
                let (token, line, col) = match ts {
                    Some(ts) => (ts.token.to_string(), ts.line, ts.col),
                    None => ("EOF".to_string(), 0, 0),
                };
                return Err(ParseError::UnexpectedToken {
                    token,
                    expected: "class name or string".to_string(),
                    line,
                    col,
                    source_line: None,
                });
            }
        };
        Ok(Stmt::Extends { class_name })
    }

    fn parse_class_name_decl(&mut self) -> Result<Stmt, ParseError> {
        self.advance(); // consume `class_name`
        let name = self.eat_ident()?;
        Ok(Stmt::ClassNameDecl { name })
    }

    fn parse_signal_decl(&mut self) -> Result<Stmt, ParseError> {
        self.advance(); // consume `signal`
        let name = self.eat_ident()?;
        let mut params = Vec::new();
        if self.check(&Token::LParen) {
            self.advance();
            if !self.check(&Token::RParen) {
                params.push(self.eat_ident()?);
                // Skip optional type annotation `: Type`
                if self.check(&Token::Colon) {
                    self.advance();
                    let _ = self.eat_ident(); // consume type name
                }
                while self.check(&Token::Comma) {
                    self.advance();
                    params.push(self.eat_ident()?);
                    if self.check(&Token::Colon) {
                        self.advance();
                        let _ = self.eat_ident();
                    }
                }
            }
            self.expect(&Token::RParen)?;
        }
        Ok(Stmt::SignalDecl { name, params })
    }

    fn parse_enum_decl(&mut self) -> Result<Stmt, ParseError> {
        self.advance(); // consume `enum`
        let name = self.eat_ident()?;
        self.expect(&Token::LBrace)?;
        let mut variants = Vec::new();
        if !self.check(&Token::RBrace) {
            variants.push(self.eat_ident()?);
            while self.check(&Token::Comma) {
                self.advance();
                if self.check(&Token::RBrace) {
                    break;
                }
                variants.push(self.eat_ident()?);
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(Stmt::EnumDecl { name, variants })
    }

    fn parse_inner_class(&mut self) -> Result<Stmt, ParseError> {
        self.advance(); // consume `class`
        let name = self.eat_ident()?;
        self.expect(&Token::Colon)?;
        let body = self.parse_block()?;
        Ok(Stmt::InnerClass { name, body })
    }

    fn parse_if(&mut self) -> Result<Stmt, ParseError> {
        self.advance(); // consume `if`
        let condition = self.parse_expr()?;
        self.expect(&Token::Colon)?;
        let body = self.parse_block()?;

        let mut elif_branches = Vec::new();
        let mut else_body = None;

        loop {
            self.skip_newlines();
            if self.check(&Token::Elif) {
                self.advance();
                let elif_cond = self.parse_expr()?;
                self.expect(&Token::Colon)?;
                let elif_body = self.parse_block()?;
                elif_branches.push((elif_cond, elif_body));
            } else if self.check(&Token::Else) {
                self.advance();
                self.expect(&Token::Colon)?;
                else_body = Some(self.parse_block()?);
                break;
            } else {
                break;
            }
        }

        Ok(Stmt::If {
            condition,
            body,
            elif_branches,
            else_body,
        })
    }

    fn parse_while(&mut self) -> Result<Stmt, ParseError> {
        self.advance(); // consume `while`
        let condition = self.parse_expr()?;
        self.expect(&Token::Colon)?;
        let body = self.parse_block()?;
        Ok(Stmt::While { condition, body })
    }

    fn parse_for(&mut self) -> Result<Stmt, ParseError> {
        self.advance(); // consume `for`
        let var = self.eat_ident()?;
        self.expect(&Token::In)?;
        let iterable = self.parse_expr()?;
        self.expect(&Token::Colon)?;
        let body = self.parse_block()?;
        Ok(Stmt::For {
            var,
            iterable,
            body,
        })
    }

    fn parse_return(&mut self) -> Result<Stmt, ParseError> {
        self.advance(); // consume `return`
        let value =
            if self.check(&Token::Newline) || self.check(&Token::Eof) || self.check(&Token::Dedent)
            {
                None
            } else {
                Some(self.parse_expr()?)
            };
        Ok(Stmt::Return(value))
    }

    fn parse_static_func(&mut self) -> Result<Stmt, ParseError> {
        self.advance(); // consume `static`
        if !self.check(&Token::Func) {
            let ts = self.tokens.get(self.pos);
            let (token, line, col) = match ts {
                Some(ts) => (ts.token.to_string(), ts.line, ts.col),
                None => ("EOF".to_string(), 0, 0),
            };
            return Err(ParseError::UnexpectedToken {
                token,
                expected: "func after static".to_string(),
                line,
                col,
                source_line: None,
            });
        }
        self.parse_func_def(true, Vec::new())
    }

    fn parse_func_param(&mut self) -> Result<FuncParam, ParseError> {
        let name = self.eat_ident()?;
        let type_hint = if self.check(&Token::Colon) {
            self.advance();
            Some(self.parse_type_hint()?)
        } else {
            None
        };
        let default = if self.check(&Token::Assign) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(FuncParam {
            name,
            type_hint,
            default,
        })
    }

    fn parse_func_def(
        &mut self,
        is_static: bool,
        annotations: Vec<Annotation>,
    ) -> Result<Stmt, ParseError> {
        self.advance(); // consume `func`
        let name = self.eat_ident()?;
        self.expect(&Token::LParen)?;

        let mut params = Vec::new();
        if !self.check(&Token::RParen) {
            params.push(self.parse_func_param()?);
            while self.check(&Token::Comma) {
                self.advance();
                params.push(self.parse_func_param()?);
            }
        }
        self.expect(&Token::RParen)?;

        let return_type = if self.check(&Token::Arrow) {
            self.advance();
            Some(self.parse_type_hint()?)
        } else {
            None
        };

        self.expect(&Token::Colon)?;
        let body = self.parse_block()?;
        Ok(Stmt::FuncDef {
            name,
            params,
            return_type,
            body,
            is_static,
            annotations,
        })
    }

    fn parse_match(&mut self) -> Result<Stmt, ParseError> {
        self.advance();
        let value = self.parse_expr()?;
        self.expect(&Token::Colon)?;
        self.skip_newlines();
        self.expect(&Token::Indent)?;
        let mut arms = Vec::new();
        self.skip_newlines();
        while !self.check(&Token::Dedent) && !self.check(&Token::Eof) {
            arms.push(self.parse_match_branch()?);
            self.skip_newlines();
        }
        if self.check(&Token::Dedent) {
            self.advance();
        }
        Ok(Stmt::Match { value, arms })
    }

    /// Parses a single `match` branch: one or more comma-separated patterns,
    /// an optional `when` guard, and a block body.
    fn parse_match_branch(&mut self) -> Result<MatchArm, ParseError> {
        let mut patterns = vec![self.parse_match_pattern()?];
        while self.check(&Token::Comma) {
            self.advance();
            // Tolerate a trailing comma before `when`/`:`.
            if self.check(&Token::When) || self.check(&Token::Colon) {
                break;
            }
            patterns.push(self.parse_match_pattern()?);
        }
        let guard = if self.check(&Token::When) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };
        self.expect(&Token::Colon)?;
        let body = self.parse_block()?;
        Ok(MatchArm {
            patterns,
            guard,
            body,
        })
    }

    fn parse_match_pattern(&mut self) -> Result<MatchPattern, ParseError> {
        match self.peek().clone() {
            // `var name` binding pattern.
            Token::Var => {
                self.advance();
                let name = self.eat_ident()?;
                Ok(MatchPattern::Binding(name))
            }
            // `..` rest pattern.
            Token::DotDot => {
                self.advance();
                Ok(MatchPattern::Rest)
            }
            // `_` wildcard.
            Token::Ident(name) if name == "_" => {
                self.advance();
                Ok(MatchPattern::Wildcard)
            }
            // Array pattern `[p, p, ..]`.
            Token::LBracket => {
                self.advance();
                let mut pats = Vec::new();
                if !self.check(&Token::RBracket) {
                    pats.push(self.parse_match_pattern()?);
                    while self.check(&Token::Comma) {
                        self.advance();
                        if self.check(&Token::RBracket) {
                            break;
                        }
                        pats.push(self.parse_match_pattern()?);
                    }
                }
                self.expect(&Token::RBracket)?;
                Ok(MatchPattern::Array(pats))
            }
            // Dictionary pattern `{ key: subpattern, key, .. }`.
            Token::LBrace => {
                self.advance();
                let mut entries = Vec::new();
                let mut rest = false;
                if !self.check(&Token::RBrace) {
                    self.parse_dict_pattern_entry(&mut entries, &mut rest)?;
                    while self.check(&Token::Comma) {
                        self.advance();
                        if self.check(&Token::RBrace) {
                            break;
                        }
                        self.parse_dict_pattern_entry(&mut entries, &mut rest)?;
                    }
                }
                self.expect(&Token::RBrace)?;
                Ok(MatchPattern::Dictionary { entries, rest })
            }
            // Everything else is an expression: a literal folds to a value
            // comparison, anything else becomes a constant reference compared
            // by value (fixes the bare-identifier binding bug).
            _ => {
                let expr = self.parse_expr()?;
                match expr {
                    Expr::Literal(v) => Ok(MatchPattern::Literal(v)),
                    other => Ok(MatchPattern::ConstantRef(other)),
                }
            }
        }
    }

    /// Parses one entry of a dictionary pattern, pushing it into `entries` or
    /// setting `rest` for a trailing `..`.
    fn parse_dict_pattern_entry(
        &mut self,
        entries: &mut Vec<(Expr, Option<MatchPattern>)>,
        rest: &mut bool,
    ) -> Result<(), ParseError> {
        if self.check(&Token::DotDot) {
            self.advance();
            *rest = true;
            return Ok(());
        }
        let key = self.parse_expr()?;
        let sub = if self.check(&Token::Colon) {
            self.advance();
            Some(self.parse_match_pattern()?)
        } else {
            None
        };
        entries.push((key, sub));
        Ok(())
    }

    fn parse_await(&mut self) -> Result<Stmt, ParseError> {
        self.advance(); // consume `await`
        let expr = self.parse_expr()?;
        Ok(Stmt::Await { expr, bind: None })
    }

    fn parse_expr_or_assign(&mut self) -> Result<Stmt, ParseError> {
        let expr = self.parse_expr()?;

        let op = match self.peek() {
            Token::Assign => AssignOp::Assign,
            Token::PlusAssign => AssignOp::AddAssign,
            Token::MinusAssign => AssignOp::SubAssign,
            Token::StarAssign => AssignOp::MulAssign,
            Token::SlashAssign => AssignOp::DivAssign,
            Token::PercentAssign => AssignOp::ModAssign,
            Token::StarStarAssign => AssignOp::PowAssign,
            Token::AmpAssign => AssignOp::BitAndAssign,
            Token::PipeAssign => AssignOp::BitOrAssign,
            Token::CaretAssign => AssignOp::BitXorAssign,
            Token::ShlAssign => AssignOp::ShlAssign,
            Token::ShrAssign => AssignOp::ShrAssign,
            _ => return Ok(Stmt::ExprStmt(expr)),
        };
        self.advance();
        let value = self.parse_expr()?;
        Ok(Stmt::Assignment {
            target: expr,
            op,
            value,
        })
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>, ParseError> {
        self.skip_newlines();
        self.expect(&Token::Indent)?;
        let mut stmts = Vec::new();
        self.skip_newlines();
        while !self.check(&Token::Dedent) && !self.check(&Token::Eof) {
            stmts.push(self.parse_stmt()?);
            self.skip_newlines();
        }
        if self.check(&Token::Dedent) {
            self.advance();
        }
        Ok(stmts)
    }

    // --- Expression parsing (precedence climbing) ---

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        // `as` (cast) has the loosest precedence of any operator except
        // assignment, so it wraps the whole ternary expression.
        let mut value = self.parse_ternary()?;
        while self.check(&Token::As) {
            self.advance();
            let type_name = self.parse_type_hint()?;
            value = Expr::Cast {
                value: Box::new(value),
                type_name,
            };
        }
        Ok(value)
    }

    fn parse_ternary(&mut self) -> Result<Expr, ParseError> {
        let value = self.parse_or()?;
        if self.check(&Token::If) {
            self.advance();
            let condition = self.parse_or()?;
            self.expect(&Token::Else)?;
            // The else branch is another ternary (right-associative) but does
            // not absorb a trailing `as` cast, matching Godot precedence.
            let else_value = self.parse_ternary()?;
            return Ok(Expr::Ternary {
                value: Box::new(value),
                condition: Box::new(condition),
                else_value: Box::new(else_value),
            });
        }
        Ok(value)
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_and()?;
        while self.check(&Token::Or) {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinOp::Or,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_comparison()?;
        while self.check(&Token::And) {
            self.advance();
            let right = self.parse_comparison()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinOp::And,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_bit_or()?;
        loop {
            let op = match self.peek() {
                Token::EqEq => BinOp::Eq,
                Token::BangEq => BinOp::Ne,
                Token::Lt => BinOp::Lt,
                Token::Gt => BinOp::Gt,
                Token::LtEq => BinOp::Le,
                Token::GtEq => BinOp::Ge,
                Token::In => BinOp::In,
                _ => break,
            };
            self.advance();
            let right = self.parse_bit_or()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    // Bitwise operators sit between comparison and addition, ordered (loosest
    // to tightest): `|`, `^`, `&`, then `<<`/`>>`, matching Godot precedence.
    fn parse_bit_or(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_bit_xor()?;
        while self.check(&Token::Pipe) {
            self.advance();
            let right = self.parse_bit_xor()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinOp::BitOr,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_bit_xor(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_bit_and()?;
        while self.check(&Token::Caret) {
            self.advance();
            let right = self.parse_bit_and()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinOp::BitXor,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_bit_and(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_bit_shift()?;
        while self.check(&Token::Amp) {
            self.advance();
            let right = self.parse_bit_shift()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinOp::BitAnd,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_bit_shift(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_addition()?;
        loop {
            let op = match self.peek() {
                Token::Shl => BinOp::Shl,
                Token::Shr => BinOp::Shr,
                _ => break,
            };
            self.advance();
            let right = self.parse_addition()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_addition(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_multiplication()?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinOp::Add,
                Token::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplication()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_multiplication(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                Token::Percent => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        match self.peek().clone() {
            Token::Minus => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr::UnaryOp {
                    op: UnaryOp::Neg,
                    expr: Box::new(expr),
                })
            }
            Token::Not => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr::UnaryOp {
                    op: UnaryOp::Not,
                    expr: Box::new(expr),
                })
            }
            Token::Tilde => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr::UnaryOp {
                    op: UnaryOp::BitNot,
                    expr: Box::new(expr),
                })
            }
            _ => self.parse_power(),
        }
    }

    /// `**` binds tighter than the unary sign/bit-not operators and is
    /// left-associative (matching Godot's `parse_binary_operator`).
    fn parse_power(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_type_test()?;
        while self.check(&Token::StarStar) {
            self.advance();
            let right = self.parse_power_operand()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinOp::Pow,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    /// Right operand of `**`: allows unary prefixes (`2 ** -1`) without
    /// re-entering the power loop, so `**` chains stay left-associative.
    fn parse_power_operand(&mut self) -> Result<Expr, ParseError> {
        let op = match self.peek() {
            Token::Minus => Some(UnaryOp::Neg),
            Token::Not => Some(UnaryOp::Not),
            Token::Tilde => Some(UnaryOp::BitNot),
            _ => None,
        };
        match op {
            Some(op) => {
                self.advance();
                let expr = self.parse_power_operand()?;
                Ok(Expr::UnaryOp {
                    op,
                    expr: Box::new(expr),
                })
            }
            None => self.parse_type_test(),
        }
    }

    /// `is` type-test binds tighter than `**` but looser than call/subscript.
    fn parse_type_test(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_postfix()?;
        while self.check(&Token::Is) {
            self.advance();
            let type_name = self.parse_type_hint()?;
            left = Expr::TypeTest {
                value: Box::new(left),
                type_name,
            };
        }
        Ok(left)
    }

    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_primary()?;
        loop {
            match self.peek() {
                Token::LParen => {
                    self.advance();
                    let mut args = Vec::new();
                    if !self.check(&Token::RParen) {
                        args.push(self.parse_expr()?);
                        while self.check(&Token::Comma) {
                            self.advance();
                            args.push(self.parse_expr()?);
                        }
                    }
                    self.expect(&Token::RParen)?;
                    expr = Expr::Call {
                        callee: Box::new(expr),
                        args,
                    };
                }
                Token::Dot => {
                    self.advance();
                    let member = self.eat_ident()?;
                    expr = Expr::MemberAccess {
                        object: Box::new(expr),
                        member,
                    };
                }
                Token::LBracket => {
                    self.advance();
                    let index = self.parse_expr()?;
                    self.expect(&Token::RBracket)?;
                    expr = Expr::Index {
                        object: Box::new(expr),
                        index: Box::new(index),
                    };
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        match self.peek().clone() {
            Token::IntLit(v) => {
                self.advance();
                Ok(Expr::Literal(Variant::Int(v)))
            }
            Token::FloatLit(v) => {
                self.advance();
                Ok(Expr::Literal(Variant::Float(v)))
            }
            Token::StringLit(v) => {
                self.advance();
                Ok(Expr::Literal(Variant::String(v)))
            }
            Token::BoolLit(v) => {
                self.advance();
                Ok(Expr::Literal(Variant::Bool(v)))
            }
            Token::Null => {
                self.advance();
                Ok(Expr::Literal(Variant::Nil))
            }
            Token::Ident(name) => {
                self.advance();
                Ok(Expr::Ident(name))
            }
            Token::Func => {
                // Lambda / anonymous function: `func(x, y): return x + y`
                self.advance();
                self.expect(&Token::LParen)?;
                let mut params = Vec::new();
                if !self.check(&Token::RParen) {
                    params.push(self.parse_func_param()?);
                    while self.check(&Token::Comma) {
                        self.advance();
                        params.push(self.parse_func_param()?);
                    }
                }
                self.expect(&Token::RParen)?;
                self.expect(&Token::Colon)?;
                // Lambda body: either a single-line statement or a block
                let body = if self.check(&Token::Newline) || self.check(&Token::Indent) {
                    self.parse_block()?
                } else if self.check(&Token::Return) {
                    // Single-line: `func(x): return x + 1`
                    self.advance(); // consume `return`
                    let expr = self.parse_expr()?;
                    vec![Stmt::Return(Some(expr))]
                } else {
                    // Single expression body (implicit return)
                    let expr = self.parse_expr()?;
                    vec![Stmt::Return(Some(expr))]
                };
                Ok(Expr::Lambda { params, body })
            }
            Token::Self_ => {
                self.advance();
                Ok(Expr::SelfRef)
            }
            Token::Super => {
                self.advance();
                Ok(Expr::SuperRef)
            }
            Token::Dollar => {
                self.advance();
                let path = match self.peek().clone() {
                    Token::Ident(name) => {
                        self.advance();
                        let mut full = name;
                        while self.check(&Token::Slash) {
                            self.advance();
                            let next = self.eat_ident()?;
                            full.push('/');
                            full.push_str(&next);
                        }
                        full
                    }
                    Token::StringLit(s) => {
                        self.advance();
                        s
                    }
                    _ => {
                        let ts = self.tokens.get(self.pos);
                        let (token, line, col) = match ts {
                            Some(ts) => (ts.token.to_string(), ts.line, ts.col),
                            None => ("EOF".to_string(), 0, 0),
                        };
                        return Err(ParseError::UnexpectedToken {
                            token,
                            expected: "identifier or string after $".to_string(),
                            line,
                            col,
                            source_line: if line > 0 {
                                self.source_lines.get(line - 1).cloned()
                            } else {
                                None
                            },
                        });
                    }
                };
                Ok(Expr::GetNode(path))
            }
            Token::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            Token::LBracket => {
                self.advance();
                let mut elements = Vec::new();
                if !self.check(&Token::RBracket) {
                    elements.push(self.parse_expr()?);
                    while self.check(&Token::Comma) {
                        self.advance();
                        if self.check(&Token::RBracket) {
                            break;
                        }
                        elements.push(self.parse_expr()?);
                    }
                }
                self.expect(&Token::RBracket)?;
                Ok(Expr::ArrayLiteral(elements))
            }
            Token::LBrace => {
                self.advance();
                let mut entries = Vec::new();
                if !self.check(&Token::RBrace) {
                    let key = self.parse_expr()?;
                    self.expect(&Token::Colon)?;
                    let val = self.parse_expr()?;
                    entries.push((key, val));
                    while self.check(&Token::Comma) {
                        self.advance();
                        if self.check(&Token::RBrace) {
                            break;
                        }
                        let key = self.parse_expr()?;
                        self.expect(&Token::Colon)?;
                        let val = self.parse_expr()?;
                        entries.push((key, val));
                    }
                }
                self.expect(&Token::RBrace)?;
                Ok(Expr::DictLiteral(entries))
            }
            _ => {
                let ts = self.tokens.get(self.pos);
                let (token, line, col) = match ts {
                    Some(ts) => (ts.token.to_string(), ts.line, ts.col),
                    None => ("EOF".to_string(), 0, 0),
                };
                Err(ParseError::UnexpectedToken {
                    token,
                    expected: "expression".to_string(),
                    line,
                    col,
                    source_line: None,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizer::tokenize;

    fn parse(src: &str) -> Vec<Stmt> {
        let tokens = tokenize(src).unwrap();
        let mut parser = Parser::new(tokens, src);
        parser.parse_script().unwrap()
    }

    fn parse_expr_str(src: &str) -> Expr {
        let tokens = tokenize(src).unwrap();
        let mut parser = Parser::new(tokens, src);
        parser.parse_expr().unwrap()
    }

    #[test]
    fn parse_var_decl_with_value() {
        let stmts = parse("var x = 10\n");
        assert_eq!(stmts.len(), 1);
        assert!(matches!(
            &stmts[0],
            Stmt::VarDecl { name, type_hint: None, value: Some(Expr::Literal(Variant::Int(10))), .. }
            if name == "x"
        ));
    }

    #[test]
    fn parse_var_decl_with_type_hint() {
        let stmts = parse("var x: int = 5\n");
        assert!(matches!(
            &stmts[0],
            Stmt::VarDecl { name, type_hint: Some(th), value: Some(_), .. }
            if name == "x" && th == "int"
        ));
    }

    #[test]
    fn parse_binary_precedence() {
        let expr = parse_expr_str("1 + 2 * 3");
        // Should be 1 + (2 * 3)
        assert!(matches!(expr, Expr::BinaryOp { op: BinOp::Add, .. }));
        if let Expr::BinaryOp { right, .. } = &expr {
            assert!(matches!(**right, Expr::BinaryOp { op: BinOp::Mul, .. }));
        }
    }

    #[test]
    fn parse_unary_negation() {
        let expr = parse_expr_str("-5");
        assert!(matches!(
            expr,
            Expr::UnaryOp {
                op: UnaryOp::Neg,
                ..
            }
        ));
    }

    #[test]
    fn parse_not_expr() {
        let expr = parse_expr_str("not true");
        assert!(matches!(
            expr,
            Expr::UnaryOp {
                op: UnaryOp::Not,
                ..
            }
        ));
    }

    #[test]
    fn parse_function_call() {
        let expr = parse_expr_str("foo(1, 2)");
        assert!(matches!(expr, Expr::Call { .. }));
        if let Expr::Call { callee, args } = &expr {
            assert!(matches!(**callee, Expr::Ident(ref n) if n == "foo"));
            assert_eq!(args.len(), 2);
        }
    }

    #[test]
    fn parse_member_access() {
        let expr = parse_expr_str("obj.field");
        assert!(matches!(
            expr,
            Expr::MemberAccess { member: ref m, .. } if m == "field"
        ));
    }

    #[test]
    fn parse_index_access() {
        let expr = parse_expr_str("arr[0]");
        assert!(matches!(expr, Expr::Index { .. }));
    }

    #[test]
    fn parse_array_literal() {
        let expr = parse_expr_str("[1, 2, 3]");
        if let Expr::ArrayLiteral(elems) = &expr {
            assert_eq!(elems.len(), 3);
        } else {
            panic!("expected array literal");
        }
    }

    #[test]
    fn parse_dict_literal() {
        let expr = parse_expr_str("{\"a\": 1, \"b\": 2}");
        if let Expr::DictLiteral(entries) = &expr {
            assert_eq!(entries.len(), 2);
        } else {
            panic!("expected dict literal");
        }
    }

    #[test]
    fn parse_if_else() {
        let stmts = parse("if x:\n    pass\nelse:\n    pass\n");
        assert!(matches!(
            &stmts[0],
            Stmt::If {
                else_body: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn parse_if_elif_else() {
        let stmts = parse("if a:\n    pass\nelif b:\n    pass\nelse:\n    pass\n");
        if let Stmt::If {
            elif_branches,
            else_body,
            ..
        } = &stmts[0]
        {
            assert_eq!(elif_branches.len(), 1);
            assert!(else_body.is_some());
        } else {
            panic!("expected if statement");
        }
    }

    #[test]
    fn parse_while_loop() {
        let stmts = parse("while true:\n    pass\n");
        assert!(matches!(&stmts[0], Stmt::While { .. }));
    }

    #[test]
    fn parse_for_loop() {
        let stmts = parse("for i in items:\n    pass\n");
        assert!(matches!(
            &stmts[0],
            Stmt::For { var, .. } if var == "i"
        ));
    }

    #[test]
    fn parse_return_value() {
        let stmts = parse("func f():\n    return 42\n");
        if let Stmt::FuncDef { body, .. } = &stmts[0] {
            assert!(matches!(&body[0], Stmt::Return(Some(_))));
        } else {
            panic!("expected func def");
        }
    }

    #[test]
    fn parse_func_def_with_params() {
        let stmts = parse("func add(a, b):\n    return a + b\n");
        if let Stmt::FuncDef {
            name, params, body, ..
        } = &stmts[0]
        {
            assert_eq!(name, "add");
            assert_eq!(params.len(), 2);
            assert_eq!(params[0].name, "a");
            assert_eq!(params[1].name, "b");
            assert!(params[0].type_hint.is_none());
            assert!(params[0].default.is_none());
            assert_eq!(body.len(), 1);
        } else {
            panic!("expected func def");
        }
    }

    #[test]
    fn parse_func_with_return_type() {
        let stmts = parse("func get_name() -> String:\n    return \"hello\"\n");
        if let Stmt::FuncDef { return_type, .. } = &stmts[0] {
            assert_eq!(return_type.as_deref(), Some("String"));
        } else {
            panic!("expected func def");
        }
    }

    #[test]
    fn parse_assignment() {
        let stmts = parse("x = 10\n");
        assert!(matches!(
            &stmts[0],
            Stmt::Assignment {
                op: AssignOp::Assign,
                ..
            }
        ));
    }

    #[test]
    fn parse_plus_assign() {
        let stmts = parse("x += 5\n");
        assert!(matches!(
            &stmts[0],
            Stmt::Assignment {
                op: AssignOp::AddAssign,
                ..
            }
        ));
    }

    #[test]
    fn parse_pass_break_continue() {
        let stmts = parse("pass\n");
        assert!(matches!(&stmts[0], Stmt::Pass));
    }

    #[test]
    fn parse_logical_operators() {
        let expr = parse_expr_str("a and b or c");
        // or has lower precedence, so: (a and b) or c
        assert!(matches!(expr, Expr::BinaryOp { op: BinOp::Or, .. }));
    }

    #[test]
    fn parse_comparison_chain() {
        let expr = parse_expr_str("x == 1");
        assert!(matches!(expr, Expr::BinaryOp { op: BinOp::Eq, .. }));
    }

    #[test]
    fn parse_chained_calls() {
        let expr = parse_expr_str("a.b().c");
        assert!(matches!(expr, Expr::MemberAccess { .. }));
    }

    #[test]
    fn parse_parenthesized_expr() {
        let expr = parse_expr_str("(1 + 2) * 3");
        assert!(matches!(expr, Expr::BinaryOp { op: BinOp::Mul, .. }));
    }

    #[test]
    fn parse_empty_array() {
        let expr = parse_expr_str("[]");
        assert!(matches!(expr, Expr::ArrayLiteral(ref v) if v.is_empty()));
    }

    #[test]
    fn parse_empty_dict() {
        let expr = parse_expr_str("{}");
        assert!(matches!(expr, Expr::DictLiteral(ref v) if v.is_empty()));
    }

    #[test]
    fn parse_extends_ident() {
        let stmts = parse("extends Node\n");
        assert!(matches!(
            &stmts[0],
            Stmt::Extends { class_name } if class_name == "Node"
        ));
    }

    #[test]
    fn parse_extends_string() {
        let stmts = parse("extends \"Node2D\"\n");
        assert!(matches!(
            &stmts[0],
            Stmt::Extends { class_name } if class_name == "Node2D"
        ));
    }

    #[test]
    fn parse_class_name_decl() {
        let stmts = parse("class_name Player\n");
        assert!(matches!(
            &stmts[0],
            Stmt::ClassNameDecl { name } if name == "Player"
        ));
    }

    #[test]
    fn parse_signal_no_params() {
        let stmts = parse("signal health_changed\n");
        assert!(matches!(
            &stmts[0],
            Stmt::SignalDecl { name, params } if name == "health_changed" && params.is_empty()
        ));
    }

    #[test]
    fn parse_signal_with_params() {
        let stmts = parse("signal damage_taken(amount, source)\n");
        if let Stmt::SignalDecl { name, params } = &stmts[0] {
            assert_eq!(name, "damage_taken");
            assert_eq!(params, &["amount", "source"]);
        } else {
            panic!("expected SignalDecl");
        }
    }

    #[test]
    fn parse_enum_decl() {
        let stmts = parse("enum State { IDLE, RUNNING, JUMPING }\n");
        if let Stmt::EnumDecl { name, variants } = &stmts[0] {
            assert_eq!(name, "State");
            assert_eq!(variants, &["IDLE", "RUNNING", "JUMPING"]);
        } else {
            panic!("expected EnumDecl");
        }
    }

    #[test]
    fn parse_export_var() {
        let stmts = parse("@export\nvar speed: float = 100.0\n");
        if let Stmt::VarDecl {
            name,
            type_hint,
            annotations,
            ..
        } = &stmts[0]
        {
            assert_eq!(name, "speed");
            assert_eq!(type_hint.as_deref(), Some("float"));
            assert_eq!(annotations.len(), 1);
            assert_eq!(annotations[0].name, "export");
        } else {
            panic!("expected VarDecl with export annotation");
        }
    }

    #[test]
    fn parse_self_member_access() {
        let expr = parse_expr_str("self.health");
        assert!(matches!(
            expr,
            Expr::MemberAccess { ref object, ref member }
            if matches!(**object, Expr::SelfRef) && member == "health"
        ));
    }

    #[test]
    fn parse_self_method_call() {
        let expr = parse_expr_str("self.take_damage(10)");
        assert!(matches!(expr, Expr::Call { .. }));
    }

    #[test]
    fn parse_super_call() {
        let expr = parse_expr_str("super()");
        assert!(
            matches!(expr, Expr::Call { ref callee, .. } if matches!(**callee, Expr::SuperRef))
        );
    }

    #[test]
    fn parse_ternary() {
        let e = parse_expr_str("x if true else y");
        assert!(matches!(e, Expr::Ternary { .. }));
    }
    #[test]
    fn parse_match_stmt() {
        let s = parse("match x:\n    1:\n        pass\n    _:\n        pass\n");
        assert!(matches!(&s[0], Stmt::Match { .. }));
    }

    // -----------------------------------------------------------------------
    // const / is / as / bitwise / power / compound assignment (pat-u3l8c.7-.8)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_const_decl() {
        let stmts = parse("const MAX = 10\n");
        assert!(matches!(
            &stmts[0],
            Stmt::ConstDecl { name, type_hint: None, value: Expr::Literal(Variant::Int(10)) }
            if name == "MAX"
        ));
    }

    #[test]
    fn parse_const_decl_with_type() {
        let stmts = parse("const SPEED: float = 1.5\n");
        assert!(matches!(
            &stmts[0],
            Stmt::ConstDecl { name, type_hint: Some(t), .. }
            if name == "SPEED" && t == "float"
        ));
    }

    #[test]
    fn parse_is_operator() {
        let e = parse_expr_str("x is int");
        assert!(matches!(
            &e,
            Expr::TypeTest { type_name, .. } if type_name == "int"
        ));
    }

    #[test]
    fn parse_as_operator() {
        let e = parse_expr_str("x as float");
        assert!(matches!(
            &e,
            Expr::Cast { type_name, .. } if type_name == "float"
        ));
    }

    #[test]
    fn parse_as_binds_loosely() {
        // `a + b as int` == `(a + b) as int`
        let e = parse_expr_str("a + b as int");
        match e {
            Expr::Cast { value, type_name } => {
                assert_eq!(type_name, "int");
                assert!(matches!(*value, Expr::BinaryOp { op: BinOp::Add, .. }));
            }
            other => panic!("expected cast, got {other:?}"),
        }
    }

    #[test]
    fn parse_is_binds_tightly() {
        // `a + b is int` == `a + (b is int)`
        let e = parse_expr_str("a + b is int");
        match e {
            Expr::BinaryOp { op: BinOp::Add, right, .. } => {
                assert!(matches!(*right, Expr::TypeTest { .. }));
            }
            other => panic!("expected add, got {other:?}"),
        }
    }

    #[test]
    fn parse_bitwise_precedence() {
        // `a | b & c` == `a | (b & c)` since `&` binds tighter than `|`.
        let e = parse_expr_str("a | b & c");
        match e {
            Expr::BinaryOp { op: BinOp::BitOr, right, .. } => {
                assert!(matches!(*right, Expr::BinaryOp { op: BinOp::BitAnd, .. }));
            }
            other => panic!("expected bitor, got {other:?}"),
        }
    }

    #[test]
    fn parse_shift_between_bitand_and_addition() {
        // `a & b << c` == `a & (b << c)`
        let e = parse_expr_str("a & b << c");
        match e {
            Expr::BinaryOp { op: BinOp::BitAnd, right, .. } => {
                assert!(matches!(*right, Expr::BinaryOp { op: BinOp::Shl, .. }));
            }
            other => panic!("expected bitand, got {other:?}"),
        }
    }

    #[test]
    fn parse_power_tighter_than_unary_minus() {
        // `-2 ** 2` == `-(2 ** 2)`
        let e = parse_expr_str("-2 ** 2");
        match e {
            Expr::UnaryOp { op: UnaryOp::Neg, expr } => {
                assert!(matches!(*expr, Expr::BinaryOp { op: BinOp::Pow, .. }));
            }
            other => panic!("expected negation, got {other:?}"),
        }
    }

    #[test]
    fn parse_tilde_unary() {
        let e = parse_expr_str("~x");
        assert!(matches!(e, Expr::UnaryOp { op: UnaryOp::BitNot, .. }));
    }

    #[test]
    fn parse_compound_assignments() {
        for (src, expect) in [
            ("x *= 2\n", AssignOp::MulAssign),
            ("x /= 2\n", AssignOp::DivAssign),
            ("x %= 2\n", AssignOp::ModAssign),
            ("x **= 2\n", AssignOp::PowAssign),
            ("x &= 2\n", AssignOp::BitAndAssign),
            ("x |= 2\n", AssignOp::BitOrAssign),
            ("x ^= 2\n", AssignOp::BitXorAssign),
            ("x <<= 2\n", AssignOp::ShlAssign),
            ("x >>= 2\n", AssignOp::ShrAssign),
        ] {
            let stmts = parse(src);
            match &stmts[0] {
                Stmt::Assignment { op, .. } => assert_eq!(*op, expect, "for {src}"),
                other => panic!("expected assignment for {src}, got {other:?}"),
            }
        }
    }

    // -----------------------------------------------------------------------
    // match pattern semantics (pat-u3l8c.9)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_match_bare_ident_is_constant_ref_not_binding() {
        let s = parse("match x:\n    RED:\n        pass\n");
        if let Stmt::Match { arms, .. } = &s[0] {
            assert!(matches!(
                arms[0].patterns[0],
                MatchPattern::ConstantRef(Expr::Ident(ref n)) if n == "RED"
            ));
        } else {
            panic!("expected match");
        }
    }

    #[test]
    fn parse_match_var_binding() {
        let s = parse("match x:\n    var y:\n        pass\n");
        if let Stmt::Match { arms, .. } = &s[0] {
            assert!(matches!(
                arms[0].patterns[0],
                MatchPattern::Binding(ref n) if n == "y"
            ));
        } else {
            panic!("expected match");
        }
    }

    #[test]
    fn parse_match_multiple_patterns() {
        let s = parse("match x:\n    1, 2, 3:\n        pass\n");
        if let Stmt::Match { arms, .. } = &s[0] {
            assert_eq!(arms[0].patterns.len(), 3);
        } else {
            panic!("expected match");
        }
    }

    #[test]
    fn parse_match_array_rest() {
        let s = parse("match x:\n    [1, ..]:\n        pass\n");
        if let Stmt::Match { arms, .. } = &s[0] {
            if let MatchPattern::Array(pats) = &arms[0].patterns[0] {
                assert_eq!(pats.len(), 2);
                assert!(matches!(pats[1], MatchPattern::Rest));
            } else {
                panic!("expected array pattern");
            }
        } else {
            panic!("expected match");
        }
    }

    #[test]
    fn parse_match_dict_pattern_with_rest() {
        let s = parse("match x:\n    {\"a\": 1, ..}:\n        pass\n");
        if let Stmt::Match { arms, .. } = &s[0] {
            if let MatchPattern::Dictionary { entries, rest } = &arms[0].patterns[0] {
                assert_eq!(entries.len(), 1);
                assert!(*rest);
            } else {
                panic!("expected dict pattern");
            }
        } else {
            panic!("expected match");
        }
    }

    #[test]
    fn parse_match_when_guard() {
        let s = parse("match x:\n    var y when y > 0:\n        pass\n");
        if let Stmt::Match { arms, .. } = &s[0] {
            assert!(arms[0].guard.is_some());
        } else {
            panic!("expected match");
        }
    }

    // -----------------------------------------------------------------------
    // Default parameter values
    // -----------------------------------------------------------------------

    #[test]
    fn parse_func_default_int() {
        let stmts = parse("func foo(x: int = 5):\n    pass\n");
        if let Stmt::FuncDef { params, .. } = &stmts[0] {
            assert_eq!(params.len(), 1);
            assert_eq!(params[0].name, "x");
            assert_eq!(params[0].type_hint.as_deref(), Some("int"));
            assert!(matches!(
                &params[0].default,
                Some(Expr::Literal(Variant::Int(5)))
            ));
        } else {
            panic!("expected FuncDef");
        }
    }

    #[test]
    fn parse_func_default_string() {
        let stmts = parse("func greet(name: String = \"hello\"):\n    pass\n");
        if let Stmt::FuncDef { params, .. } = &stmts[0] {
            assert_eq!(params[0].name, "name");
            assert!(matches!(
                &params[0].default,
                Some(Expr::Literal(Variant::String(s))) if s == "hello"
            ));
        } else {
            panic!("expected FuncDef");
        }
    }

    #[test]
    fn parse_func_default_float() {
        let stmts = parse("func speed(v: float = 3.14):\n    pass\n");
        if let Stmt::FuncDef { params, .. } = &stmts[0] {
            assert!(matches!(
                &params[0].default,
                Some(Expr::Literal(Variant::Float(f))) if (*f - 3.14).abs() < 1e-10
            ));
        } else {
            panic!("expected FuncDef");
        }
    }

    #[test]
    fn parse_func_default_bool() {
        let stmts = parse("func toggle(on = true):\n    pass\n");
        if let Stmt::FuncDef { params, .. } = &stmts[0] {
            assert_eq!(params[0].name, "on");
            assert!(params[0].type_hint.is_none());
            assert!(matches!(
                &params[0].default,
                Some(Expr::Literal(Variant::Bool(true)))
            ));
        } else {
            panic!("expected FuncDef");
        }
    }

    #[test]
    fn parse_func_default_null() {
        let stmts = parse("func maybe(val = null):\n    pass\n");
        if let Stmt::FuncDef { params, .. } = &stmts[0] {
            assert!(matches!(
                &params[0].default,
                Some(Expr::Literal(Variant::Nil))
            ));
        } else {
            panic!("expected FuncDef");
        }
    }

    #[test]
    fn parse_func_mixed_default_and_required() {
        let stmts = parse("func foo(a, b: int, c: int = 10, d = \"hi\"):\n    pass\n");
        if let Stmt::FuncDef { params, .. } = &stmts[0] {
            assert_eq!(params.len(), 4);
            assert_eq!(params[0].name, "a");
            assert!(params[0].default.is_none());
            assert_eq!(params[1].name, "b");
            assert!(params[1].default.is_none());
            assert_eq!(params[2].name, "c");
            assert!(params[2].default.is_some());
            assert_eq!(params[3].name, "d");
            assert!(params[3].default.is_some());
        } else {
            panic!("expected FuncDef");
        }
    }

    #[test]
    fn parse_func_default_negative_number() {
        let stmts = parse("func offset(x = -10):\n    pass\n");
        if let Stmt::FuncDef { params, .. } = &stmts[0] {
            // -10 parses as UnaryOp(Neg, Literal(Int(10)))
            assert!(matches!(
                &params[0].default,
                Some(Expr::UnaryOp {
                    op: UnaryOp::Neg,
                    ..
                })
            ));
        } else {
            panic!("expected FuncDef");
        }
    }

    #[test]
    fn parse_func_default_expression() {
        let stmts = parse("func compute(x = 2 + 3):\n    pass\n");
        if let Stmt::FuncDef { params, .. } = &stmts[0] {
            assert!(matches!(
                &params[0].default,
                Some(Expr::BinaryOp { op: BinOp::Add, .. })
            ));
        } else {
            panic!("expected FuncDef");
        }
    }

    // -----------------------------------------------------------------------
    // Static functions
    // -----------------------------------------------------------------------

    #[test]
    fn parse_static_func_simple() {
        let stmts = parse("static func helper() -> int:\n    return 42\n");
        if let Stmt::FuncDef {
            name,
            is_static,
            return_type,
            ..
        } = &stmts[0]
        {
            assert_eq!(name, "helper");
            assert!(is_static);
            assert_eq!(return_type.as_deref(), Some("int"));
        } else {
            panic!("expected static FuncDef");
        }
    }

    #[test]
    fn parse_static_func_with_params() {
        let stmts = parse("static func add(a: int, b: int) -> int:\n    return a + b\n");
        if let Stmt::FuncDef {
            name,
            params,
            is_static,
            ..
        } = &stmts[0]
        {
            assert_eq!(name, "add");
            assert!(is_static);
            assert_eq!(params.len(), 2);
            assert_eq!(params[0].name, "a");
            assert_eq!(params[1].name, "b");
        } else {
            panic!("expected static FuncDef");
        }
    }

    #[test]
    fn parse_regular_func_not_static() {
        let stmts = parse("func foo():\n    pass\n");
        if let Stmt::FuncDef { is_static, .. } = &stmts[0] {
            assert!(!is_static);
        } else {
            panic!("expected FuncDef");
        }
    }

    #[test]
    fn parse_static_func_with_defaults() {
        let stmts = parse("static func create(name: String = \"default\"):\n    return name\n");
        if let Stmt::FuncDef {
            is_static, params, ..
        } = &stmts[0]
        {
            assert!(is_static);
            assert_eq!(params.len(), 1);
            assert!(params[0].default.is_some());
        } else {
            panic!("expected static FuncDef");
        }
    }

    // -----------------------------------------------------------------------
    // @onready annotation
    // -----------------------------------------------------------------------

    #[test]
    fn parse_onready_var() {
        let stmts = parse("@onready\nvar label = $Label\n");
        if let Stmt::VarDecl {
            name,
            annotations,
            value,
            ..
        } = &stmts[0]
        {
            assert_eq!(name, "label");
            assert_eq!(annotations.len(), 1);
            assert_eq!(annotations[0].name, "onready");
            assert!(matches!(value, Some(Expr::GetNode(ref p)) if p == "Label"));
        } else {
            panic!("expected VarDecl with @onready");
        }
    }

    #[test]
    fn parse_onready_var_with_path() {
        let stmts = parse("@onready\nvar btn = $UI/Button\n");
        if let Stmt::VarDecl {
            annotations, value, ..
        } = &stmts[0]
        {
            assert_eq!(annotations[0].name, "onready");
            assert!(matches!(value, Some(Expr::GetNode(ref p)) if p == "UI/Button"));
        } else {
            panic!("expected VarDecl with @onready");
        }
    }

    #[test]
    fn parse_onready_var_string_path() {
        let stmts = parse("@onready\nvar node = $\"Path/To/Node\"\n");
        if let Stmt::VarDecl {
            annotations, value, ..
        } = &stmts[0]
        {
            assert_eq!(annotations[0].name, "onready");
            assert!(matches!(value, Some(Expr::GetNode(ref p)) if p == "Path/To/Node"));
        } else {
            panic!("expected VarDecl with @onready");
        }
    }

    #[test]
    fn parse_onready_with_type() {
        let stmts = parse("@onready\nvar label: Label = $Label\n");
        if let Stmt::VarDecl {
            name,
            type_hint,
            annotations,
            ..
        } = &stmts[0]
        {
            assert_eq!(name, "label");
            assert_eq!(type_hint.as_deref(), Some("Label"));
            assert_eq!(annotations[0].name, "onready");
        } else {
            panic!("expected VarDecl with @onready and type");
        }
    }

    // -----------------------------------------------------------------------
    // Typed var declarations
    // -----------------------------------------------------------------------

    #[test]
    fn parse_typed_var_float() {
        let stmts = parse("var speed: float = 200.0\n");
        if let Stmt::VarDecl {
            name,
            type_hint,
            value,
            ..
        } = &stmts[0]
        {
            assert_eq!(name, "speed");
            assert_eq!(type_hint.as_deref(), Some("float"));
            assert!(matches!(
                value,
                Some(Expr::Literal(Variant::Float(f))) if (*f - 200.0).abs() < 1e-10
            ));
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn parse_typed_var_string() {
        let stmts = parse("var name: String = \"player\"\n");
        if let Stmt::VarDecl {
            name,
            type_hint,
            value,
            ..
        } = &stmts[0]
        {
            assert_eq!(name, "name");
            assert_eq!(type_hint.as_deref(), Some("String"));
            assert!(matches!(
                value,
                Some(Expr::Literal(Variant::String(ref s))) if s == "player"
            ));
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn parse_typed_var_no_value() {
        let stmts = parse("var count: int\n");
        if let Stmt::VarDecl {
            name,
            type_hint,
            value,
            ..
        } = &stmts[0]
        {
            assert_eq!(name, "count");
            assert_eq!(type_hint.as_deref(), Some("int"));
            assert!(value.is_none());
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn parse_typed_var_bool() {
        let stmts = parse("var active: bool = false\n");
        if let Stmt::VarDecl {
            type_hint, value, ..
        } = &stmts[0]
        {
            assert_eq!(type_hint.as_deref(), Some("bool"));
            assert!(matches!(value, Some(Expr::Literal(Variant::Bool(false)))));
        } else {
            panic!("expected VarDecl");
        }
    }

    // -----------------------------------------------------------------------
    // Empty return
    // -----------------------------------------------------------------------

    #[test]
    fn parse_empty_return() {
        let stmts = parse("func foo():\n    return\n");
        if let Stmt::FuncDef { body, .. } = &stmts[0] {
            assert!(matches!(&body[0], Stmt::Return(None)));
        } else {
            panic!("expected FuncDef");
        }
    }

    #[test]
    fn parse_empty_return_before_other_stmt() {
        let stmts = parse("func foo():\n    return\n    pass\n");
        if let Stmt::FuncDef { body, .. } = &stmts[0] {
            assert!(matches!(&body[0], Stmt::Return(None)));
        } else {
            panic!("expected FuncDef");
        }
    }

    #[test]
    fn parse_return_with_value_still_works() {
        let stmts = parse("func foo():\n    return 42\n");
        if let Stmt::FuncDef { body, .. } = &stmts[0] {
            assert!(matches!(&body[0], Stmt::Return(Some(_))));
        } else {
            panic!("expected FuncDef");
        }
    }

    // -----------------------------------------------------------------------
    // Negative number literals
    // -----------------------------------------------------------------------

    #[test]
    fn parse_negative_int_literal() {
        let expr = parse_expr_str("-5");
        assert!(matches!(
            expr,
            Expr::UnaryOp {
                op: UnaryOp::Neg,
                ref expr
            } if matches!(**expr, Expr::Literal(Variant::Int(5)))
        ));
    }

    #[test]
    fn parse_negative_float_literal() {
        let expr = parse_expr_str("-3.14");
        assert!(matches!(
            &expr,
            Expr::UnaryOp {
                op: UnaryOp::Neg,
                ..
            }
        ));
    }

    #[test]
    fn parse_var_with_negative_init() {
        let stmts = parse("var x = -10\n");
        if let Stmt::VarDecl { name, value, .. } = &stmts[0] {
            assert_eq!(name, "x");
            assert!(matches!(
                value,
                Some(Expr::UnaryOp {
                    op: UnaryOp::Neg,
                    ..
                })
            ));
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn parse_negative_in_default_param() {
        let stmts = parse("func offset(x: int = -5):\n    pass\n");
        if let Stmt::FuncDef { params, .. } = &stmts[0] {
            assert!(matches!(
                &params[0].default,
                Some(Expr::UnaryOp {
                    op: UnaryOp::Neg,
                    ..
                })
            ));
        } else {
            panic!("expected FuncDef");
        }
    }

    #[test]
    fn parse_negative_in_array() {
        let expr = parse_expr_str("[-1, -2, -3]");
        if let Expr::ArrayLiteral(elems) = &expr {
            assert_eq!(elems.len(), 3);
            assert!(matches!(
                &elems[0],
                Expr::UnaryOp {
                    op: UnaryOp::Neg,
                    ..
                }
            ));
        } else {
            panic!("expected array literal");
        }
    }

    #[test]
    fn parse_double_negation() {
        let expr = parse_expr_str("--5");
        // Should be Neg(Neg(5))
        assert!(matches!(
            &expr,
            Expr::UnaryOp {
                op: UnaryOp::Neg,
                expr: inner
            } if matches!(**inner, Expr::UnaryOp { op: UnaryOp::Neg, .. })
        ));
    }

    // -----------------------------------------------------------------------
    // Multi-arg print (parser side: just a normal call)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_print_multiple_args() {
        let expr = parse_expr_str("print(\"a\", \"b\", 42)");
        if let Expr::Call { callee, args } = &expr {
            assert!(matches!(**callee, Expr::Ident(ref n) if n == "print"));
            assert_eq!(args.len(), 3);
        } else {
            panic!("expected Call");
        }
    }

    #[test]
    fn parse_print_no_args() {
        let expr = parse_expr_str("print()");
        if let Expr::Call { args, .. } = &expr {
            assert!(args.is_empty());
        } else {
            panic!("expected Call");
        }
    }

    #[test]
    fn parse_print_single_arg() {
        let expr = parse_expr_str("print(\"hello\")");
        if let Expr::Call { args, .. } = &expr {
            assert_eq!(args.len(), 1);
        } else {
            panic!("expected Call");
        }
    }

    // -----------------------------------------------------------------------
    // Combined feature tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_static_func_with_typed_params_and_defaults() {
        let stmts = parse(
            "static func create(name: String = \"default\", count: int = 1) -> Node:\n    pass\n",
        );
        if let Stmt::FuncDef {
            name,
            params,
            is_static,
            return_type,
            ..
        } = &stmts[0]
        {
            assert_eq!(name, "create");
            assert!(is_static);
            assert_eq!(return_type.as_deref(), Some("Node"));
            assert_eq!(params.len(), 2);
            assert_eq!(params[0].name, "name");
            assert_eq!(params[0].type_hint.as_deref(), Some("String"));
            assert!(params[0].default.is_some());
            assert_eq!(params[1].name, "count");
            assert_eq!(params[1].type_hint.as_deref(), Some("int"));
            assert!(params[1].default.is_some());
        } else {
            panic!("expected static FuncDef");
        }
    }

    #[test]
    fn parse_class_with_onready_and_static() {
        let src = "extends Node\nclass_name Player\n@onready\nvar label = $Label\nvar speed: float = 200.0\nstatic func helper() -> int:\n    return 42\nfunc _ready():\n    pass\n";
        let stmts = parse(src);
        assert!(matches!(&stmts[0], Stmt::Extends { .. }));
        assert!(matches!(&stmts[1], Stmt::ClassNameDecl { .. }));
        assert!(
            matches!(&stmts[2], Stmt::VarDecl { annotations, .. } if annotations[0].name == "onready")
        );
        assert!(matches!(&stmts[3], Stmt::VarDecl { type_hint: Some(t), .. } if t == "float"));
        assert!(matches!(
            &stmts[4],
            Stmt::FuncDef {
                is_static: true,
                ..
            }
        ));
        assert!(matches!(
            &stmts[5],
            Stmt::FuncDef {
                is_static: false,
                ..
            }
        ));
    }

    #[test]
    fn parse_func_all_defaults() {
        let stmts = parse("func foo(a = 1, b = 2, c = 3):\n    pass\n");
        if let Stmt::FuncDef { params, .. } = &stmts[0] {
            assert_eq!(params.len(), 3);
            for p in params {
                assert!(p.default.is_some());
                assert!(p.type_hint.is_none());
            }
        } else {
            panic!("expected FuncDef");
        }
    }

    #[test]
    fn parse_func_param_types_preserved() {
        let stmts = parse("func process(delta: float, speed: int, name: String):\n    pass\n");
        if let Stmt::FuncDef { params, .. } = &stmts[0] {
            assert_eq!(params[0].type_hint.as_deref(), Some("float"));
            assert_eq!(params[1].type_hint.as_deref(), Some("int"));
            assert_eq!(params[2].type_hint.as_deref(), Some("String"));
        } else {
            panic!("expected FuncDef");
        }
    }

    // -- pat-13d: Typed arrays -----------------------------------------------

    #[test]
    fn parse_typed_array_var_int() {
        let stmts = parse("var items: Array[int] = [1, 2, 3]\n");
        if let Stmt::VarDecl {
            name,
            type_hint,
            value,
            ..
        } = &stmts[0]
        {
            assert_eq!(name, "items");
            assert_eq!(type_hint.as_deref(), Some("Array[int]"));
            assert!(value.is_some());
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn parse_typed_array_var_vector2() {
        let stmts = parse("var points: Array[Vector2]\n");
        if let Stmt::VarDecl {
            name, type_hint, ..
        } = &stmts[0]
        {
            assert_eq!(name, "points");
            assert_eq!(type_hint.as_deref(), Some("Array[Vector2]"));
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn parse_typed_array_var_node() {
        let stmts = parse("var nodes: Array[Node] = []\n");
        if let Stmt::VarDecl {
            name, type_hint, ..
        } = &stmts[0]
        {
            assert_eq!(name, "nodes");
            assert_eq!(type_hint.as_deref(), Some("Array[Node]"));
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn parse_typed_array_var_string() {
        let stmts = parse("var names: Array[String] = [\"a\", \"b\"]\n");
        if let Stmt::VarDecl {
            name, type_hint, ..
        } = &stmts[0]
        {
            assert_eq!(name, "names");
            assert_eq!(type_hint.as_deref(), Some("Array[String]"));
        } else {
            panic!("expected VarDecl");
        }
    }

    #[test]
    fn parse_typed_array_func_param() {
        let stmts = parse("func foo(items: Array[int]):\n    pass\n");
        if let Stmt::FuncDef { params, .. } = &stmts[0] {
            assert_eq!(params.len(), 1);
            assert_eq!(params[0].name, "items");
            assert_eq!(params[0].type_hint.as_deref(), Some("Array[int]"));
        } else {
            panic!("expected FuncDef");
        }
    }

    #[test]
    fn parse_typed_array_func_param_with_default() {
        let stmts = parse("func foo(items: Array[float] = []):\n    pass\n");
        if let Stmt::FuncDef { params, .. } = &stmts[0] {
            assert_eq!(params[0].type_hint.as_deref(), Some("Array[float]"));
            assert!(params[0].default.is_some());
        } else {
            panic!("expected FuncDef");
        }
    }

    #[test]
    fn parse_typed_array_return_type() {
        let stmts = parse("func get_items() -> Array[int]:\n    pass\n");
        if let Stmt::FuncDef { return_type, .. } = &stmts[0] {
            assert_eq!(return_type.as_deref(), Some("Array[int]"));
        } else {
            panic!("expected FuncDef");
        }
    }

    #[test]
    fn parse_typed_array_multiple_params() {
        let stmts = parse("func merge(a: Array[int], b: Array[int]) -> Array[int]:\n    pass\n");
        if let Stmt::FuncDef {
            params,
            return_type,
            ..
        } = &stmts[0]
        {
            assert_eq!(params[0].type_hint.as_deref(), Some("Array[int]"));
            assert_eq!(params[1].type_hint.as_deref(), Some("Array[int]"));
            assert_eq!(return_type.as_deref(), Some("Array[int]"));
        } else {
            panic!("expected FuncDef");
        }
    }

    #[test]
    fn parse_plain_array_type_still_works() {
        // Plain `Array` without element type should still parse
        let stmts = parse("var items: Array\n");
        if let Stmt::VarDecl { type_hint, .. } = &stmts[0] {
            assert_eq!(type_hint.as_deref(), Some("Array"));
        } else {
            panic!("expected VarDecl");
        }
    }

    // -- pat-c13: Inner classes -----------------------------------------------

    #[test]
    fn parse_inner_class_simple() {
        let stmts = parse("class State:\n    var name = \"idle\"\n");
        if let Stmt::InnerClass { name, body } = &stmts[0] {
            assert_eq!(name, "State");
            assert_eq!(body.len(), 1);
            assert!(matches!(&body[0], Stmt::VarDecl { name, .. } if name == "name"));
        } else {
            panic!("expected InnerClass, got {:?}", stmts[0]);
        }
    }

    #[test]
    fn parse_inner_class_with_method() {
        let stmts =
            parse("class Enemy:\n    var hp = 10\n    func take_damage(d):\n        hp = hp - d\n");
        if let Stmt::InnerClass { name, body } = &stmts[0] {
            assert_eq!(name, "Enemy");
            assert_eq!(body.len(), 2);
            assert!(matches!(&body[0], Stmt::VarDecl { .. }));
            assert!(matches!(&body[1], Stmt::FuncDef { .. }));
        } else {
            panic!("expected InnerClass");
        }
    }

    #[test]
    fn parse_inner_class_empty_pass() {
        let stmts = parse("class Empty:\n    pass\n");
        if let Stmt::InnerClass { name, body } = &stmts[0] {
            assert_eq!(name, "Empty");
            assert_eq!(body.len(), 1);
            assert!(matches!(&body[0], Stmt::Pass));
        } else {
            panic!("expected InnerClass");
        }
    }

    #[test]
    fn parse_inner_class_multiple_vars() {
        let stmts = parse("class Data:\n    var x = 0\n    var y = 0\n    var z = 0\n");
        if let Stmt::InnerClass { name, body } = &stmts[0] {
            assert_eq!(name, "Data");
            assert_eq!(body.len(), 3);
        } else {
            panic!("expected InnerClass");
        }
    }

    #[test]
    fn parse_inner_class_in_script() {
        let stmts =
            parse("extends Node\nclass State:\n    var name = \"idle\"\nvar current = null\n");
        assert!(matches!(&stmts[0], Stmt::Extends { .. }));
        assert!(matches!(&stmts[1], Stmt::InnerClass { name, .. } if name == "State"));
        assert!(matches!(&stmts[2], Stmt::VarDecl { .. }));
    }

    #[test]
    fn parse_two_inner_classes() {
        let stmts = parse("class A:\n    var x = 1\nclass B:\n    var y = 2\n");
        assert!(matches!(&stmts[0], Stmt::InnerClass { name, .. } if name == "A"));
        assert!(matches!(&stmts[1], Stmt::InnerClass { name, .. } if name == "B"));
    }

    #[test]
    fn parse_inner_class_with_typed_var() {
        let stmts = parse("class Config:\n    var speed: float = 100.0\n");
        if let Stmt::InnerClass { body, .. } = &stmts[0] {
            if let Stmt::VarDecl { type_hint, .. } = &body[0] {
                assert_eq!(type_hint.as_deref(), Some("float"));
            } else {
                panic!("expected VarDecl");
            }
        } else {
            panic!("expected InnerClass");
        }
    }

    #[test]
    fn parse_inner_class_with_signal() {
        let stmts = parse("class EventBus:\n    signal fired\n    var count = 0\n");
        if let Stmt::InnerClass { name, body } = &stmts[0] {
            assert_eq!(name, "EventBus");
            assert!(matches!(&body[0], Stmt::SignalDecl { name, .. } if name == "fired"));
            assert!(matches!(&body[1], Stmt::VarDecl { .. }));
        } else {
            panic!("expected InnerClass");
        }
    }

    // -- pat-916: @tool annotation --------------------------------------------

    #[test]
    fn parse_tool_annotation() {
        let stmts = parse("@tool\nextends Node\n");
        assert!(
            matches!(&stmts[0], Stmt::AnnotationStmt { annotations } if annotations.iter().any(|a| a.name == "tool"))
        );
        assert!(matches!(&stmts[1], Stmt::Extends { .. }));
    }

    #[test]
    fn parse_tool_annotation_with_class_name() {
        let stmts = parse("@tool\nextends Node\nclass_name MyTool\n");
        assert!(
            matches!(&stmts[0], Stmt::AnnotationStmt { annotations } if annotations[0].name == "tool")
        );
        assert!(matches!(&stmts[1], Stmt::Extends { .. }));
        assert!(matches!(&stmts[2], Stmt::ClassNameDecl { .. }));
    }

    #[test]
    fn parse_no_tool_annotation() {
        let stmts = parse("extends Node\n");
        assert!(matches!(&stmts[0], Stmt::Extends { .. }));
        // No @tool annotation stmt present
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn parse_tool_with_var_and_func() {
        let stmts = parse("@tool\nextends Node\nvar x = 1\nfunc _ready():\n    pass\n");
        assert!(matches!(&stmts[0], Stmt::AnnotationStmt { .. }));
        assert_eq!(stmts.len(), 4);
    }

    #[test]
    fn parse_tool_annotation_name_is_tool() {
        let stmts = parse("@tool\nextends Node\n");
        if let Stmt::AnnotationStmt { annotations } = &stmts[0] {
            assert_eq!(annotations.len(), 1);
            assert_eq!(annotations[0].name, "tool");
        } else {
            panic!("expected AnnotationStmt");
        }
    }

    #[test]
    fn parse_rpc_annotation_on_func_captures_args() {
        // @rpc(mode, sync, transfer, channel) is parsed and attached to the func.
        let stmts = parse(
            "@rpc(\"any_peer\", \"call_local\", \"reliable\", 0)\nfunc take_damage(amount):\n    pass\n",
        );
        let annotations = stmts
            .iter()
            .find_map(|s| match s {
                Stmt::FuncDef {
                    name, annotations, ..
                } if name == "take_damage" => Some(annotations),
                _ => None,
            })
            .expect("expected take_damage FuncDef");
        let rpc = annotations
            .iter()
            .find(|a| a.name == "rpc")
            .expect("expected @rpc annotation on func");
        assert_eq!(
            rpc.args,
            vec!["any_peer", "call_local", "reliable", "0"],
            "@rpc args parsed (string literals unquoted, int as text)"
        );
    }

    #[test]
    fn parse_bare_annotation_has_empty_args() {
        let stmts = parse("@export\nvar health: int = 100\n");
        if let Stmt::VarDecl { annotations, .. } = &stmts[0] {
            assert_eq!(annotations[0].name, "export");
            assert!(annotations[0].args.is_empty(), "bare @export has no args");
        } else {
            panic!("expected VarDecl");
        }
    }
}
