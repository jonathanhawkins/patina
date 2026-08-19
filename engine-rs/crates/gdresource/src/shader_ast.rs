//! GDShader parser, AST and CPU evaluator (pat-0q2kx.142).
//!
//! Godot's `servers/rendering/shader_language.cpp` implements a full GLSL-ES-3
//! subset: local variables, `if`/`else`, loops, user functions, swizzles, and
//! ~100 builtin functions, with line-numbered errors on invalid source.
//!
//! Patina's shader support was a **six-instruction line matcher**, duplicated in
//! `gdserver2d` and `gdserver3d`, which silently skipped any line it could not
//! classify — so a shader using a local variable or an `if` rendered wrong
//! output with no diagnostic at all.
//!
//! This module is the real thing: a recursive-descent parser over the existing
//! [`shader_tokenizer`](crate::shader_tokenizer) (which already carries
//! line/col on every token) producing a typed AST, plus a tree-walking
//! evaluator over a per-stage context.
//!
//! Scope note: this covers the *language* — statements, expressions, control
//! flow, user functions, swizzles and the common builtin maths. Texture
//! sampling is represented in the AST and delegated to a caller-supplied
//! sampler, since the pixel source differs per renderer.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use crate::shader_tokenizer::{
    tokenize_shader_no_comments, ShaderLexError, ShaderToken, ShaderTokenSpan,
};

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------

/// A runtime shader value.
///
/// Ints and floats are kept distinct because GLSL's `/` differs between them
/// and because a loop counter must stay integral.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShaderValue {
    /// A boolean.
    Bool(bool),
    /// A signed integer.
    Int(i64),
    /// A scalar float.
    Float(f32),
    /// A 2-component vector.
    Vec2([f32; 2]),
    /// A 3-component vector.
    Vec3([f32; 3]),
    /// A 4-component vector.
    Vec4([f32; 4]),
}

impl ShaderValue {
    /// The value's components, scalars widening to a 1-element slice.
    pub fn components(&self) -> Vec<f32> {
        match self {
            ShaderValue::Bool(b) => vec![if *b { 1.0 } else { 0.0 }],
            ShaderValue::Int(i) => vec![*i as f32],
            ShaderValue::Float(f) => vec![*f],
            ShaderValue::Vec2(v) => v.to_vec(),
            ShaderValue::Vec3(v) => v.to_vec(),
            ShaderValue::Vec4(v) => v.to_vec(),
        }
    }

    /// Builds a value from components, choosing the matching vector width.
    pub fn from_components(c: &[f32]) -> ShaderValue {
        match c.len() {
            0 => ShaderValue::Float(0.0),
            1 => ShaderValue::Float(c[0]),
            2 => ShaderValue::Vec2([c[0], c[1]]),
            3 => ShaderValue::Vec3([c[0], c[1], c[2]]),
            _ => ShaderValue::Vec4([c[0], c[1], c[2], c[3]]),
        }
    }

    /// The value as a scalar float (first component).
    pub fn as_f32(&self) -> f32 {
        self.components().first().copied().unwrap_or(0.0)
    }

    /// Truthiness, for conditions.
    pub fn as_bool(&self) -> bool {
        match self {
            ShaderValue::Bool(b) => *b,
            other => other.as_f32() != 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------

/// An expression node.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// A literal scalar.
    Literal(ShaderValue),
    /// A variable, uniform or stage builtin by name.
    ///
    /// Carries its own source position so an undeclared identifier can be
    /// reported at the line the author wrote it on — Godot reports these at
    /// compile time, and a position-less AST could only say "somewhere".
    Ident {
        /// The identifier text.
        name: String,
        /// 1-based source line.
        line: usize,
        /// 1-based column.
        col: usize,
    },
    /// A unary operator (`-`, `!`).
    Unary {
        /// The operator text.
        op: String,
        /// Operand.
        expr: Box<Expr>,
    },
    /// A binary operator.
    Binary {
        /// The operator text (`+`, `<`, `&&`, …).
        op: String,
        /// Left operand.
        lhs: Box<Expr>,
        /// Right operand.
        rhs: Box<Expr>,
    },
    /// A function call, a type constructor (`vec3(…)`), or a texture sample.
    Call {
        /// Callee name.
        name: String,
        /// Argument expressions.
        args: Vec<Expr>,
    },
    /// A member access — either a swizzle (`c.rgb`) or a struct field.
    Member {
        /// The object expression.
        object: Box<Expr>,
        /// The member/swizzle text.
        member: String,
    },
    /// A ternary conditional.
    Ternary {
        /// The condition.
        cond: Box<Expr>,
        /// Value when true.
        then: Box<Expr>,
        /// Value when false.
        other: Box<Expr>,
    },
}

/// A statement node.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// A local declaration, e.g. `float t = 0.5;`.
    Local {
        /// Declared name.
        name: String,
        /// Initializer, if any.
        init: Option<Expr>,
    },
    /// An assignment to a name or member path, e.g. `ALBEDO.r = 1.0;`.
    Assign {
        /// The assignment target.
        target: Expr,
        /// The compound operator (`+=` → `"+"`), or empty for plain `=`.
        op: String,
        /// The assigned value.
        value: Expr,
    },
    /// A conditional.
    If {
        /// The condition.
        cond: Expr,
        /// Taken when the condition is true.
        then: Vec<Stmt>,
        /// Taken otherwise.
        otherwise: Vec<Stmt>,
    },
    /// A C-style `for` loop.
    For {
        /// Loop initializer.
        init: Box<Option<Stmt>>,
        /// Loop condition.
        cond: Option<Expr>,
        /// Per-iteration step.
        step: Box<Option<Stmt>>,
        /// Loop body.
        body: Vec<Stmt>,
    },
    /// A `while` loop.
    While {
        /// The condition.
        cond: Expr,
        /// Loop body.
        body: Vec<Stmt>,
    },
    /// A `return`, optionally with a value.
    Return(Option<Expr>),
    /// `discard;`.
    Discard,
    /// `break;`.
    Break,
    /// `continue;`.
    Continue,
    /// A bare expression statement (usually a call).
    Expr(Expr),
}

/// A user-defined function.
#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    /// Function name.
    pub name: String,
    /// Parameter names, in order.
    pub params: Vec<String>,
    /// Body statements.
    pub body: Vec<Stmt>,
}

/// A parsed shader program.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShaderProgram {
    /// `shader_type` value, if declared.
    pub shader_type: Option<String>,
    /// Declared uniform names, in declaration order.
    pub uniforms: Vec<String>,
    /// Declared varying names.
    pub varyings: Vec<String>,
    /// Every function, keyed by name.
    pub functions: HashMap<String, Function>,
}

impl ShaderProgram {
    /// Returns the named function, if the shader declares one.
    pub fn function(&self, name: &str) -> Option<&Function> {
        self.functions.get(name)
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// A parse or evaluation failure, always carrying the source line.
#[derive(Debug, Clone, PartialEq)]
pub struct ShaderError {
    /// Human-readable message.
    pub message: String,
    /// 1-based source line.
    pub line: usize,
    /// 1-based column.
    pub col: usize,
}

impl fmt::Display for ShaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at line {}, column {}",
            self.message, self.line, self.col
        )
    }
}

impl std::error::Error for ShaderError {}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

struct Parser {
    tokens: Vec<ShaderTokenSpan>,
    pos: usize,
}

/// Parses GDShader source into a [`ShaderProgram`].
///
/// Errors carry the line and column of the offending token, which is what
/// `/api/shader_validate` needs to point an editor at the mistake — the old
/// line matcher simply dropped anything it did not recognise.
pub fn parse_shader(source: &str) -> Result<ShaderProgram, ShaderError> {
    let tokens = tokenize_shader_no_comments(source).map_err(|e| {
        // ⚠️ The lexer ALREADY knows where the problem is — every
        // `ShaderLexError` variant carries `line`/`col`, and its `Display` even
        // prints them. This used to build `ShaderError { line: 0, col: 0 }`,
        // throwing that away and reporting every lexical failure as "line 0"
        // (pat-s3rrv.36).
        //
        // Line 0 is not merely imprecise, it is out of range: `ShaderError.line`
        // is documented 1-based, so 0 is a line no editor can highlight. Godot
        // names the offending line for a shader compile error, and this bead's
        // acceptance asks for exactly that.
        //
        // The or-pattern is wildcard-free on purpose, mirroring
        // `shader_lex_error_pos` in gdeditor's shader editor: a new lex variant
        // must fail to compile HERE rather than silently fall back to 0 again.
        let (line, col) = match &e {
            ShaderLexError::UnterminatedBlockComment { line, col }
            | ShaderLexError::UnexpectedChar { line, col, .. }
            | ShaderLexError::InvalidNumber { line, col, .. } => (*line, *col),
        };
        ShaderError {
            message: e.to_string(),
            line,
            col,
        }
    })?;
    Parser { tokens, pos: 0 }.parse_program()
}

impl Parser {
    fn peek(&self) -> &ShaderToken {
        self.tokens
            .get(self.pos)
            .map(|s| &s.token)
            .unwrap_or(&ShaderToken::Eof)
    }

    fn line_col(&self) -> (usize, usize) {
        self.tokens
            .get(self.pos)
            .map(|s| (s.line, s.col))
            .or_else(|| self.tokens.last().map(|s| (s.line, s.col)))
            .unwrap_or((0, 0))
    }

    fn err<T>(&self, message: impl Into<String>) -> Result<T, ShaderError> {
        let (line, col) = self.line_col();
        Err(ShaderError {
            message: message.into(),
            line,
            col,
        })
    }

    fn advance(&mut self) -> ShaderToken {
        let t = self.peek().clone();
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, t: &ShaderToken) -> bool {
        if self.peek() == t {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, t: &ShaderToken, what: &str) -> Result<(), ShaderError> {
        if self.eat(t) {
            Ok(())
        } else {
            self.err(format!("expected {what}, found `{}`", self.peek()))
        }
    }

    /// True for tokens that introduce a type (and therefore a declaration).
    fn is_type_token(t: &ShaderToken) -> bool {
        matches!(
            t,
            ShaderToken::Bool
                | ShaderToken::Int
                | ShaderToken::Uint
                | ShaderToken::Float
                | ShaderToken::Vec2
                | ShaderToken::Vec3
                | ShaderToken::Vec4
                | ShaderToken::Ivec2
                | ShaderToken::Ivec3
                | ShaderToken::Ivec4
                | ShaderToken::Bvec2
                | ShaderToken::Bvec3
                | ShaderToken::Bvec4
                | ShaderToken::Mat2
                | ShaderToken::Mat3
                | ShaderToken::Mat4
                | ShaderToken::Void
        )
    }

    /// The constructor name for a type token, e.g. `vec3`.
    fn type_name(t: &ShaderToken) -> String {
        format!("{t}")
    }

    fn parse_program(&mut self) -> Result<ShaderProgram, ShaderError> {
        let mut program = ShaderProgram::default();

        loop {
            match self.peek().clone() {
                ShaderToken::Eof => break,
                ShaderToken::ShaderType => {
                    self.advance();
                    if let ShaderToken::Ident(name) = self.peek().clone() {
                        program.shader_type = Some(name);
                        self.advance();
                    }
                    self.skip_to_semicolon();
                }
                // `render_mode` and `group_uniforms` are consumed but not
                // modelled: they do not affect per-pixel evaluation, and
                // skipping them is honest about what this evaluator covers.
                ShaderToken::RenderMode | ShaderToken::GroupUniforms => {
                    self.skip_to_semicolon();
                }
                // `struct` likewise (pat-s3rrv.36). The comment here used to
                // claim struct was consumed, but this arm did not exist, so
                // `ShaderToken::Struct` fell through to the error below and a
                // shader declaring ANY struct failed to parse in its entirety —
                // taking its `fragment()` body with it.
                //
                // Skipped via the braced block, NOT `skip_to_semicolon`: a
                // struct body is full of semicolons (`vec3 pos;`), so stopping
                // at the first one would leave the parser inside the braces and
                // turn a tolerated declaration into a cascade of errors.
                ShaderToken::Struct => {
                    self.skip_braced_block();
                    self.skip_to_semicolon();
                }
                ShaderToken::Uniform | ShaderToken::Varying | ShaderToken::Const => {
                    let is_uniform = matches!(self.peek(), ShaderToken::Uniform);
                    let is_varying = matches!(self.peek(), ShaderToken::Varying);
                    self.advance();
                    // Skip precision/interpolation qualifiers and the type.
                    while !matches!(
                        self.peek(),
                        ShaderToken::Ident(_) | ShaderToken::Semicolon | ShaderToken::Eof
                    ) {
                        self.advance();
                    }
                    if let ShaderToken::Ident(name) = self.peek().clone() {
                        if is_uniform {
                            program.uniforms.push(name);
                        } else if is_varying {
                            program.varyings.push(name);
                        }
                    }
                    self.skip_to_semicolon();
                }
                t if Self::is_type_token(&t) => {
                    let func = self.parse_function()?;
                    program.functions.insert(func.name.clone(), func);
                }
                // An unexpected top-level token is a real error, not something
                // to skip — silently skipping is exactly the old behaviour.
                other => return self.err(format!("unexpected `{other}` at top level")),
            }
        }

        Ok(program)
    }

    /// Consumes tokens through a balanced `{ … }`, leaving the parser just past
    /// the closing brace (pat-s3rrv.36).
    ///
    /// Nesting-aware so a struct containing a braced initializer does not end
    /// the skip early. A missing `{` consumes nothing, and an unterminated
    /// block stops at EOF rather than looping.
    fn skip_braced_block(&mut self) {
        while !matches!(self.peek(), ShaderToken::LBrace | ShaderToken::Eof) {
            self.advance();
        }
        if matches!(self.peek(), ShaderToken::Eof) {
            return;
        }
        let mut depth = 0usize;
        loop {
            match self.peek() {
                ShaderToken::LBrace => depth += 1,
                ShaderToken::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        self.advance();
                        return;
                    }
                }
                ShaderToken::Eof => return,
                _ => {}
            }
            self.advance();
        }
    }

    fn skip_to_semicolon(&mut self) {
        while !matches!(self.peek(), ShaderToken::Semicolon | ShaderToken::Eof) {
            self.advance();
        }
        self.eat(&ShaderToken::Semicolon);
    }

    fn parse_function(&mut self) -> Result<Function, ShaderError> {
        self.advance(); // return type
        let name = match self.advance() {
            ShaderToken::Ident(n) => n,
            other => return self.err(format!("expected a function name, found `{other}`")),
        };
        self.expect(&ShaderToken::LParen, "`(` after the function name")?;

        let mut params = Vec::new();
        while !self.eat(&ShaderToken::RParen) {
            if matches!(self.peek(), ShaderToken::Eof) {
                return self.err("unterminated parameter list");
            }
            // Qualifiers and the type precede the name; the name is the last
            // identifier before `,` or `)`.
            if let ShaderToken::Ident(n) = self.peek().clone() {
                let next_is_end = self
                    .tokens
                    .get(self.pos + 1)
                    .map(|s| matches!(s.token, ShaderToken::Comma | ShaderToken::RParen))
                    .unwrap_or(false);
                if next_is_end {
                    params.push(n);
                }
            }
            self.advance();
            self.eat(&ShaderToken::Comma);
        }

        self.expect(&ShaderToken::LBrace, "`{` to open the function body")?;
        let body = self.parse_block()?;
        Ok(Function { name, params, body })
    }

    /// Parses statements until the matching `}` (which is consumed).
    fn parse_block(&mut self) -> Result<Vec<Stmt>, ShaderError> {
        let mut stmts = Vec::new();
        while !self.eat(&ShaderToken::RBrace) {
            if matches!(self.peek(), ShaderToken::Eof) {
                return self.err("unterminated block: expected `}`");
            }
            stmts.push(self.parse_stmt()?);
        }
        Ok(stmts)
    }

    /// A single statement, or a braced group flattened into the caller.
    fn parse_stmt(&mut self) -> Result<Stmt, ShaderError> {
        match self.peek().clone() {
            ShaderToken::If => self.parse_if(),
            ShaderToken::For => self.parse_for(),
            ShaderToken::While => {
                self.advance();
                self.expect(&ShaderToken::LParen, "`(` after `while`")?;
                let cond = self.parse_expr()?;
                self.expect(&ShaderToken::RParen, "`)` after the condition")?;
                let body = self.parse_body_or_stmt()?;
                Ok(Stmt::While { cond, body })
            }
            ShaderToken::Return => {
                self.advance();
                let value = if matches!(self.peek(), ShaderToken::Semicolon) {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                self.expect(&ShaderToken::Semicolon, "`;` after `return`")?;
                Ok(Stmt::Return(value))
            }
            ShaderToken::Discard => {
                self.advance();
                self.expect(&ShaderToken::Semicolon, "`;` after `discard`")?;
                Ok(Stmt::Discard)
            }
            ShaderToken::Break => {
                self.advance();
                self.expect(&ShaderToken::Semicolon, "`;` after `break`")?;
                Ok(Stmt::Break)
            }
            ShaderToken::Continue => {
                self.advance();
                self.expect(&ShaderToken::Semicolon, "`;` after `continue`")?;
                Ok(Stmt::Continue)
            }
            t if Self::is_type_token(&t) => self.parse_local(),
            _ => {
                let expr = self.parse_expr()?;
                // An assignment target is whatever preceded the `=`.
                let op = match self.peek() {
                    ShaderToken::Assign => Some(String::new()),
                    ShaderToken::PlusAssign => Some("+".into()),
                    ShaderToken::MinusAssign => Some("-".into()),
                    ShaderToken::StarAssign => Some("*".into()),
                    ShaderToken::SlashAssign => Some("/".into()),
                    _ => None,
                };
                if let Some(op) = op {
                    self.advance();
                    let value = self.parse_expr()?;
                    self.expect(&ShaderToken::Semicolon, "`;` after the assignment")?;
                    return Ok(Stmt::Assign {
                        target: expr,
                        op,
                        value,
                    });
                }
                self.expect(&ShaderToken::Semicolon, "`;` after the expression")?;
                Ok(Stmt::Expr(expr))
            }
        }
    }

    fn parse_local(&mut self) -> Result<Stmt, ShaderError> {
        self.advance(); // the type
        let name = match self.advance() {
            ShaderToken::Ident(n) => n,
            other => return self.err(format!("expected a variable name, found `{other}`")),
        };
        let init = if self.eat(&ShaderToken::Assign) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        self.expect(&ShaderToken::Semicolon, "`;` after the declaration")?;
        Ok(Stmt::Local { name, init })
    }

    fn parse_if(&mut self) -> Result<Stmt, ShaderError> {
        self.advance();
        self.expect(&ShaderToken::LParen, "`(` after `if`")?;
        let cond = self.parse_expr()?;
        self.expect(&ShaderToken::RParen, "`)` after the condition")?;
        let then = self.parse_body_or_stmt()?;
        let otherwise = if self.eat(&ShaderToken::Else) {
            if matches!(self.peek(), ShaderToken::If) {
                vec![self.parse_if()?]
            } else {
                self.parse_body_or_stmt()?
            }
        } else {
            Vec::new()
        };
        Ok(Stmt::If {
            cond,
            then,
            otherwise,
        })
    }

    fn parse_for(&mut self) -> Result<Stmt, ShaderError> {
        self.advance();
        self.expect(&ShaderToken::LParen, "`(` after `for`")?;

        let init = if self.eat(&ShaderToken::Semicolon) {
            None
        } else {
            Some(self.parse_stmt()?) // consumes its own `;`
        };
        let cond = if matches!(self.peek(), ShaderToken::Semicolon) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.expect(&ShaderToken::Semicolon, "`;` after the loop condition")?;

        // The step has no trailing `;`, so it is parsed by hand.
        let step = if matches!(self.peek(), ShaderToken::RParen) {
            None
        } else {
            Some(self.parse_for_step()?)
        };
        self.expect(&ShaderToken::RParen, "`)` after the loop header")?;

        let body = self.parse_body_or_stmt()?;
        Ok(Stmt::For {
            init: Box::new(init),
            cond,
            step: Box::new(step),
            body,
        })
    }

    /// The `i++` / `i += 1` / `i = i + 1` third clause of a `for` header.
    fn parse_for_step(&mut self) -> Result<Stmt, ShaderError> {
        let target = self.parse_unary()?;
        match self.peek().clone() {
            ShaderToken::PlusPlus | ShaderToken::MinusMinus => {
                let op = if matches!(self.peek(), ShaderToken::PlusPlus) {
                    "+"
                } else {
                    "-"
                };
                self.advance();
                Ok(Stmt::Assign {
                    target,
                    op: op.into(),
                    value: Expr::Literal(ShaderValue::Int(1)),
                })
            }
            ShaderToken::PlusAssign
            | ShaderToken::MinusAssign
            | ShaderToken::StarAssign
            | ShaderToken::SlashAssign
            | ShaderToken::Assign => {
                let op = match self.advance() {
                    ShaderToken::PlusAssign => "+",
                    ShaderToken::MinusAssign => "-",
                    ShaderToken::StarAssign => "*",
                    ShaderToken::SlashAssign => "/",
                    _ => "",
                };
                let value = self.parse_expr()?;
                Ok(Stmt::Assign {
                    target,
                    op: op.into(),
                    value,
                })
            }
            other => self.err(format!("expected a loop step, found `{other}`")),
        }
    }

    /// Either a `{ … }` block or a single statement, both as a statement list.
    fn parse_body_or_stmt(&mut self) -> Result<Vec<Stmt>, ShaderError> {
        if self.eat(&ShaderToken::LBrace) {
            self.parse_block()
        } else {
            Ok(vec![self.parse_stmt()?])
        }
    }

    // -- expressions (precedence climbing) ---------------------------------

    fn parse_expr(&mut self) -> Result<Expr, ShaderError> {
        self.parse_ternary()
    }

    fn parse_ternary(&mut self) -> Result<Expr, ShaderError> {
        let cond = self.parse_binary(0)?;
        if self.eat(&ShaderToken::Question) {
            let then = self.parse_expr()?;
            self.expect(&ShaderToken::Colon, "`:` in the conditional")?;
            let other = self.parse_expr()?;
            return Ok(Expr::Ternary {
                cond: Box::new(cond),
                then: Box::new(then),
                other: Box::new(other),
            });
        }
        Ok(cond)
    }

    /// Binding powers, loosest first — the standard GLSL ordering.
    fn binary_op(t: &ShaderToken) -> Option<(&'static str, u8)> {
        Some(match t {
            ShaderToken::PipePipe => ("||", 1),
            ShaderToken::AmpAmp => ("&&", 2),
            ShaderToken::EqEq => ("==", 3),
            ShaderToken::BangEq => ("!=", 3),
            ShaderToken::Lt => ("<", 4),
            ShaderToken::Gt => (">", 4),
            ShaderToken::LtEq => ("<=", 4),
            ShaderToken::GtEq => (">=", 4),
            ShaderToken::Plus => ("+", 5),
            ShaderToken::Minus => ("-", 5),
            ShaderToken::Star => ("*", 6),
            ShaderToken::Slash => ("/", 6),
            ShaderToken::Percent => ("%", 6),
            _ => return None,
        })
    }

    fn parse_binary(&mut self, min_bp: u8) -> Result<Expr, ShaderError> {
        let mut lhs = self.parse_unary()?;
        while let Some((op, bp)) = Self::binary_op(self.peek()) {
            if bp < min_bp {
                break;
            }
            self.advance();
            let rhs = self.parse_binary(bp + 1)?;
            lhs = Expr::Binary {
                op: op.into(),
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, ShaderError> {
        match self.peek().clone() {
            ShaderToken::Minus => {
                self.advance();
                Ok(Expr::Unary {
                    op: "-".into(),
                    expr: Box::new(self.parse_unary()?),
                })
            }
            ShaderToken::Bang => {
                self.advance();
                Ok(Expr::Unary {
                    op: "!".into(),
                    expr: Box::new(self.parse_unary()?),
                })
            }
            ShaderToken::Plus => {
                self.advance();
                self.parse_unary()
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, ShaderError> {
        let mut expr = self.parse_primary()?;
        loop {
            if self.eat(&ShaderToken::Dot) {
                match self.advance() {
                    ShaderToken::Ident(member) => {
                        expr = Expr::Member {
                            object: Box::new(expr),
                            member,
                        };
                    }
                    other => return self.err(format!("expected a member name, found `{other}`")),
                }
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, ShaderError> {
        match self.peek().clone() {
            ShaderToken::FloatLit(f) => {
                self.advance();
                Ok(Expr::Literal(ShaderValue::Float(f as f32)))
            }
            ShaderToken::IntLit(i) => {
                self.advance();
                Ok(Expr::Literal(ShaderValue::Int(i)))
            }
            ShaderToken::UintLit(u) => {
                self.advance();
                Ok(Expr::Literal(ShaderValue::Int(u as i64)))
            }
            ShaderToken::BoolLit(b) => {
                self.advance();
                Ok(Expr::Literal(ShaderValue::Bool(b)))
            }
            ShaderToken::LParen => {
                self.advance();
                let inner = self.parse_expr()?;
                self.expect(&ShaderToken::RParen, "`)`")?;
                Ok(inner)
            }
            ShaderToken::Ident(name) => {
                let (line, col) = self.line_col();
                self.advance();
                if self.eat(&ShaderToken::LParen) {
                    let args = self.parse_args()?;
                    return Ok(Expr::Call { name, args });
                }
                Ok(Expr::Ident { name, line, col })
            }
            // A type used as a constructor: `vec3(1.0, 0.0, 0.0)`.
            t if Self::is_type_token(&t) => {
                let name = Self::type_name(&t);
                self.advance();
                self.expect(&ShaderToken::LParen, "`(` in the constructor")?;
                let args = self.parse_args()?;
                Ok(Expr::Call { name, args })
            }
            other => self.err(format!("unexpected `{other}` in an expression")),
        }
    }

    fn parse_args(&mut self) -> Result<Vec<Expr>, ShaderError> {
        let mut args = Vec::new();
        if self.eat(&ShaderToken::RParen) {
            return Ok(args);
        }
        loop {
            args.push(self.parse_expr()?);
            if self.eat(&ShaderToken::Comma) {
                continue;
            }
            self.expect(&ShaderToken::RParen, "`)` after the arguments")?;
            break;
        }
        Ok(args)
    }
}

// ---------------------------------------------------------------------------
// Stage context
// ---------------------------------------------------------------------------

/// The per-invocation state a stage function reads and writes.
///
/// `builtins` holds the stage's own variables (`UV`, `TIME`, `COLOR`,
/// `ALBEDO`, `ALPHA`, …). The caller seeds the inputs and reads the outputs
/// back out after evaluation, which is what keeps this module free of any
/// renderer-specific types.
/// A caller-supplied texture sampler (pat-s3rrv.36).
///
/// `texture(TEXTURE, UV)` cannot be a pure function of [`ShaderValue`]s: the
/// first argument names a sampler, and the pixels behind it live in whichever
/// renderer is driving the shader. This closure is how a renderer hands the
/// evaluator that lookup — it receives the sampler's NAME (`TEXTURE`,
/// `SCREEN_TEXTURE`, a `uniform sampler2D`) plus normalized UV, and returns
/// straight RGBA.
///
/// Wrapped in a newtype rather than exposed as a bare `Arc<dyn Fn>` so
/// [`StageContext`] keeps its `Debug` and `Clone` derives; a closure is neither.
#[derive(Clone)]
pub struct Sampler(Arc<dyn Fn(&str, [f32; 2]) -> [f32; 4] + Send + Sync>);

impl Sampler {
    /// Wraps a sampling closure.
    pub fn new(f: impl Fn(&str, [f32; 2]) -> [f32; 4] + Send + Sync + 'static) -> Self {
        Self(Arc::new(f))
    }

    /// Samples `name` at `uv`.
    pub fn sample(&self, name: &str, uv: [f32; 2]) -> [f32; 4] {
        (self.0)(name, uv)
    }
}

impl std::fmt::Debug for Sampler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Sampler(<closure>)")
    }
}

#[derive(Debug, Clone, Default)]
pub struct StageContext {
    /// Uniform values by name.
    pub uniforms: HashMap<String, ShaderValue>,
    /// Stage built-ins by name, both in and out.
    pub builtins: HashMap<String, ShaderValue>,
    /// Set when the shader executed `discard`.
    pub discarded: bool,
    /// Texture lookup for `texture()` and friends, supplied by the renderer
    /// driving this evaluation (pat-s3rrv.36). `None` makes every sample
    /// transparent black rather than an error, so a shader that samples still
    /// runs under a caller that has no textures to offer.
    pub sampler: Option<Sampler>,
    /// Lexical scopes; the last is innermost.
    scopes: Vec<HashMap<String, ShaderValue>>,
}

impl StageContext {
    /// A context with the given uniforms and built-ins.
    pub fn new(
        uniforms: HashMap<String, ShaderValue>,
        builtins: HashMap<String, ShaderValue>,
    ) -> Self {
        Self {
            uniforms,
            builtins,
            discarded: false,
            sampler: None,
            scopes: Vec::new(),
        }
    }

    /// Attaches the renderer's texture lookup (pat-s3rrv.36).
    pub fn with_sampler(mut self, sampler: Sampler) -> Self {
        self.sampler = Some(sampler);
        self
    }

    /// Reads a name from the innermost scope outward, then uniforms, then
    /// built-ins.
    fn get(&self, name: &str) -> Option<ShaderValue> {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(name) {
                return Some(*v);
            }
        }
        self.uniforms
            .get(name)
            .or_else(|| self.builtins.get(name))
            .copied()
    }

    /// Assigns to an existing binding, or to the built-ins when the name is a
    /// stage output. Declares nothing — that is `Stmt::Local`'s job.
    fn set(&mut self, name: &str, value: ShaderValue) {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), value);
                return;
            }
        }
        if self.uniforms.contains_key(name) {
            self.uniforms.insert(name.to_string(), value);
            return;
        }
        self.builtins.insert(name.to_string(), value);
    }

    fn declare(&mut self, name: &str, value: ShaderValue) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), value);
        }
    }
}

/// How a statement list finished.
enum Flow {
    /// Ran to the end.
    Normal,
    /// Hit `break`.
    Break,
    /// Hit `continue`.
    Continue,
    /// Hit `return`, with its value.
    Return(Option<ShaderValue>),
}

/// Loop iterations are capped so a shader whose condition never goes false
/// cannot hang the renderer. Godot's compiler rejects unbounded loops; a
/// software evaluator has to defend itself at runtime instead.
const MAX_LOOP_ITERATIONS: usize = 65_536;

/// Runs `entry` (e.g. `"fragment"`) against `ctx`.
///
/// Outputs are read back from [`StageContext::builtins`]; `discarded` reports
/// whether the shader ran `discard`.
pub fn eval_function(
    program: &ShaderProgram,
    entry: &str,
    ctx: &mut StageContext,
) -> Result<(), ShaderError> {
    let Some(func) = program.function(entry) else {
        return Err(ShaderError {
            message: format!("shader has no `{entry}` function"),
            line: 0,
            col: 0,
        });
    };
    ctx.scopes.push(HashMap::new());
    let result = exec_block(program, &func.body, ctx);
    ctx.scopes.pop();
    result.map(|_| ())
}

fn exec_block(
    program: &ShaderProgram,
    stmts: &[Stmt],
    ctx: &mut StageContext,
) -> Result<Flow, ShaderError> {
    for stmt in stmts {
        match exec_stmt(program, stmt, ctx)? {
            Flow::Normal => {}
            other => return Ok(other),
        }
    }
    Ok(Flow::Normal)
}

fn exec_stmt(
    program: &ShaderProgram,
    stmt: &Stmt,
    ctx: &mut StageContext,
) -> Result<Flow, ShaderError> {
    match stmt {
        Stmt::Local { name, init } => {
            let value = match init {
                Some(e) => eval_expr(program, e, ctx)?,
                None => ShaderValue::Float(0.0),
            };
            ctx.declare(name, value);
            Ok(Flow::Normal)
        }
        Stmt::Assign { target, op, value } => {
            let rhs = eval_expr(program, value, ctx)?;
            let rhs = if op.is_empty() {
                rhs
            } else {
                let current = eval_expr(program, target, ctx)?;
                apply_binary(op, current, rhs)?
            };
            assign_to(program, target, rhs, ctx)?;
            Ok(Flow::Normal)
        }
        Stmt::If {
            cond,
            then,
            otherwise,
        } => {
            let taken = eval_expr(program, cond, ctx)?.as_bool();
            ctx.scopes.push(HashMap::new());
            let flow = exec_block(program, if taken { then } else { otherwise }, ctx);
            ctx.scopes.pop();
            flow
        }
        Stmt::While { cond, body } => {
            let mut iterations = 0usize;
            while eval_expr(program, cond, ctx)?.as_bool() {
                iterations += 1;
                if iterations > MAX_LOOP_ITERATIONS {
                    return Err(ShaderError {
                        message: "`while` exceeded the iteration cap".into(),
                        line: 0,
                        col: 0,
                    });
                }
                ctx.scopes.push(HashMap::new());
                let flow = exec_block(program, body, ctx);
                ctx.scopes.pop();
                match flow? {
                    Flow::Break => break,
                    Flow::Return(v) => return Ok(Flow::Return(v)),
                    _ => {}
                }
            }
            Ok(Flow::Normal)
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            // The initializer's binding is scoped to the loop.
            ctx.scopes.push(HashMap::new());
            let result = (|| -> Result<Flow, ShaderError> {
                if let Some(init) = init.as_ref() {
                    exec_stmt(program, init, ctx)?;
                }
                let mut iterations = 0usize;
                loop {
                    if let Some(c) = cond {
                        if !eval_expr(program, c, ctx)?.as_bool() {
                            break;
                        }
                    }
                    iterations += 1;
                    if iterations > MAX_LOOP_ITERATIONS {
                        return Err(ShaderError {
                            message: "`for` exceeded the iteration cap".into(),
                            line: 0,
                            col: 0,
                        });
                    }
                    ctx.scopes.push(HashMap::new());
                    let flow = exec_block(program, body, ctx);
                    ctx.scopes.pop();
                    match flow? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        _ => {}
                    }
                    if let Some(step) = step.as_ref() {
                        exec_stmt(program, step, ctx)?;
                    }
                }
                Ok(Flow::Normal)
            })();
            ctx.scopes.pop();
            result
        }
        Stmt::Return(v) => {
            let value = match v {
                Some(e) => Some(eval_expr(program, e, ctx)?),
                None => None,
            };
            Ok(Flow::Return(value))
        }
        Stmt::Discard => {
            ctx.discarded = true;
            Ok(Flow::Return(None))
        }
        Stmt::Break => Ok(Flow::Break),
        Stmt::Continue => Ok(Flow::Continue),
        Stmt::Expr(e) => {
            eval_expr(program, e, ctx)?;
            Ok(Flow::Normal)
        }
    }
}

/// Writes `value` through an assignment target — a name, or a swizzle on one.
fn assign_to(
    program: &ShaderProgram,
    target: &Expr,
    value: ShaderValue,
    ctx: &mut StageContext,
) -> Result<(), ShaderError> {
    match target {
        Expr::Ident { name, .. } => {
            ctx.set(name, value);
            Ok(())
        }
        Expr::Member { object, member } => {
            // `ALBEDO.rgb = …` writes the named lanes and leaves the rest.
            let Expr::Ident { name, line, col } = object.as_ref() else {
                return Err(ShaderError {
                    message: "only a variable or one of its components can be assigned".into(),
                    line: 0,
                    col: 0,
                });
            };
            let Some(current) = ctx.get(name) else {
                return Err(ShaderError {
                    message: format!("undeclared identifier `{name}`"),
                    line: *line,
                    col: *col,
                });
            };
            let mut comps = current.components();
            let incoming = value.components();
            for (i, ch) in member.chars().enumerate() {
                let Some(lane) = swizzle_index(ch) else {
                    return Err(ShaderError {
                        message: format!("`{ch}` is not a component name"),
                        line: *line,
                        col: *col,
                    });
                };
                if lane >= comps.len() {
                    return Err(ShaderError {
                        message: format!("`{name}` has no component `{ch}`"),
                        line: *line,
                        col: *col,
                    });
                }
                // A scalar assigned to several lanes broadcasts, matching GLSL.
                comps[lane] = *incoming.get(i).or_else(|| incoming.first()).unwrap_or(&0.0);
            }
            ctx.set(name, ShaderValue::from_components(&comps));
            Ok(())
        }
        _ => Err(ShaderError {
            message: "invalid assignment target".into(),
            line: 0,
            col: 0,
        }),
    }
}

/// The component index a swizzle character selects.
fn swizzle_index(ch: char) -> Option<usize> {
    match ch {
        'x' | 'r' | 's' => Some(0),
        'y' | 'g' | 't' => Some(1),
        'z' | 'b' | 'p' => Some(2),
        'w' | 'a' | 'q' => Some(3),
        _ => None,
    }
}

/// Evaluates `texture(sampler, uv)` / `textureLod(sampler, uv, lod)`
/// (pat-s3rrv.36).
///
/// Argument 0 is taken as a NAME rather than a value, which is why this runs
/// before the generic argument evaluation. Godot's `texture()` returns vec4 for
/// a `sampler2D`, so that is what comes back here.
///
/// With no sampler attached the result is transparent black — the same thing a
/// shader reads from an unbound texture unit — rather than an error, so a
/// caller that has no textures can still run a shader that samples one.
fn eval_texture_call(
    program: &ShaderProgram,
    name: &str,
    args: &[Expr],
    ctx: &mut StageContext,
) -> Result<ShaderValue, ShaderError> {
    if args.len() < 2 {
        return Err(ShaderError {
            message: format!("`{name}` takes at least 2 arguments"),
            line: 0,
            col: 0,
        });
    }
    let sampler_name = match &args[0] {
        Expr::Ident { name, .. } => name.clone(),
        _ => {
            return Err(ShaderError {
                message: format!("`{name}`'s first argument must name a sampler"),
                line: 0,
                col: 0,
            })
        }
    };
    let uv = eval_expr(program, &args[1], ctx)?;
    // `textureLod`'s third argument is a mip level. The software renderers have
    // one mip, so it is evaluated (for side effects and to reject bad
    // expressions) and then ignored, rather than silently unparsed.
    for extra in &args[2..] {
        eval_expr(program, extra, ctx)?;
    }
    let uv = match uv {
        ShaderValue::Vec2(v) => v,
        other => {
            let c = other.components();
            [
                c.first().copied().unwrap_or(0.0),
                c.get(1).copied().unwrap_or(0.0),
            ]
        }
    };
    Ok(ShaderValue::Vec4(match &ctx.sampler {
        Some(sampler) => sampler.sample(&sampler_name, uv),
        None => [0.0, 0.0, 0.0, 0.0],
    }))
}

fn eval_expr(
    program: &ShaderProgram,
    expr: &Expr,
    ctx: &mut StageContext,
) -> Result<ShaderValue, ShaderError> {
    match expr {
        Expr::Literal(v) => Ok(*v),
        Expr::Ident { name, line, col } => ctx.get(name).ok_or_else(|| ShaderError {
            message: format!("undeclared identifier `{name}`"),
            line: *line,
            col: *col,
        }),
        Expr::Unary { op, expr } => {
            let v = eval_expr(program, expr, ctx)?;
            Ok(match op.as_str() {
                "-" => ShaderValue::from_components(
                    &v.components().iter().map(|c| -c).collect::<Vec<_>>(),
                ),
                _ => ShaderValue::Bool(!v.as_bool()),
            })
        }
        Expr::Binary { op, lhs, rhs } => {
            // `&&` and `||` short-circuit, as they do in GLSL.
            if op == "&&" {
                return Ok(ShaderValue::Bool(
                    eval_expr(program, lhs, ctx)?.as_bool()
                        && eval_expr(program, rhs, ctx)?.as_bool(),
                ));
            }
            if op == "||" {
                return Ok(ShaderValue::Bool(
                    eval_expr(program, lhs, ctx)?.as_bool()
                        || eval_expr(program, rhs, ctx)?.as_bool(),
                ));
            }
            let a = eval_expr(program, lhs, ctx)?;
            let b = eval_expr(program, rhs, ctx)?;
            apply_binary(op, a, b)
        }
        Expr::Ternary { cond, then, other } => {
            if eval_expr(program, cond, ctx)?.as_bool() {
                eval_expr(program, then, ctx)
            } else {
                eval_expr(program, other, ctx)
            }
        }
        Expr::Member { object, member } => {
            let v = eval_expr(program, object, ctx)?;
            let comps = v.components();
            let mut out = Vec::with_capacity(member.len());
            for ch in member.chars() {
                let Some(lane) = swizzle_index(ch) else {
                    return Err(ShaderError {
                        message: format!("`{ch}` is not a component name"),
                        line: 0,
                        col: 0,
                    });
                };
                let Some(c) = comps.get(lane) else {
                    return Err(ShaderError {
                        message: format!("value has no component `{ch}`"),
                        line: 0,
                        col: 0,
                    });
                };
                out.push(*c);
            }
            Ok(ShaderValue::from_components(&out))
        }
        Expr::Call { name, args } => {
            // Texture lookups are intercepted BEFORE the arguments are
            // evaluated (pat-s3rrv.36). `texture(TEXTURE, UV)` names a sampler
            // in argument 0, and a sampler is not a `ShaderValue` — evaluating
            // it eagerly would fail on an undeclared identifier.
            if matches!(name.as_str(), "texture" | "textureLod") {
                return eval_texture_call(program, name, args, ctx);
            }
            let mut values = Vec::with_capacity(args.len());
            for a in args {
                values.push(eval_expr(program, a, ctx)?);
            }
            // A user function shadows nothing builtin, but it is checked first
            // so a shader can define its own helper freely.
            if let Some(func) = program.function(name) {
                let mut scope = HashMap::new();
                for (param, value) in func.params.iter().zip(values.iter()) {
                    scope.insert(param.clone(), *value);
                }
                ctx.scopes.push(scope);
                let flow = exec_block(program, &func.body, ctx);
                ctx.scopes.pop();
                return match flow? {
                    Flow::Return(Some(v)) => Ok(v),
                    _ => Ok(ShaderValue::Float(0.0)),
                };
            }
            call_builtin(name, &values)
        }
    }
}

/// Applies a binary operator with GLSL's scalar-broadcast rules.
fn apply_binary(op: &str, a: ShaderValue, b: ShaderValue) -> Result<ShaderValue, ShaderError> {
    // Comparisons and equality are scalar predicates.
    match op {
        "<" | ">" | "<=" | ">=" | "==" | "!=" => {
            let (x, y) = (a.as_f32(), b.as_f32());
            return Ok(ShaderValue::Bool(match op {
                "<" => x < y,
                ">" => x > y,
                "<=" => x <= y,
                ">=" => x >= y,
                "==" => (x - y).abs() < f32::EPSILON,
                _ => (x - y).abs() >= f32::EPSILON,
            }));
        }
        _ => {}
    }

    // Integer arithmetic stays integral so loop counters and `%` behave.
    if let (ShaderValue::Int(x), ShaderValue::Int(y)) = (a, b) {
        return Ok(ShaderValue::Int(match op {
            "+" => x + y,
            "-" => x - y,
            "*" => x * y,
            "/" => {
                if y == 0 {
                    0
                } else {
                    x / y
                }
            }
            "%" => {
                if y == 0 {
                    0
                } else {
                    x % y
                }
            }
            _ => {
                return Err(ShaderError {
                    message: format!("unsupported operator `{op}`"),
                    line: 0,
                    col: 0,
                })
            }
        }));
    }

    let (ac, bc) = (a.components(), b.components());
    let width = ac.len().max(bc.len());
    let mut out = Vec::with_capacity(width);
    for i in 0..width {
        // A scalar operand broadcasts across the vector, per GLSL.
        let x = if ac.len() == 1 {
            ac[0]
        } else {
            *ac.get(i).unwrap_or(&0.0)
        };
        let y = if bc.len() == 1 {
            bc[0]
        } else {
            *bc.get(i).unwrap_or(&0.0)
        };
        out.push(match op {
            "+" => x + y,
            "-" => x - y,
            "*" => x * y,
            "/" => {
                if y == 0.0 {
                    0.0
                } else {
                    x / y
                }
            }
            "%" => {
                if y == 0.0 {
                    0.0
                } else {
                    x % y
                }
            }
            _ => {
                return Err(ShaderError {
                    message: format!("unsupported operator `{op}`"),
                    line: 0,
                    col: 0,
                })
            }
        });
    }
    Ok(ShaderValue::from_components(&out))
}

/// Evaluates a builtin function or type constructor.
fn call_builtin(name: &str, args: &[ShaderValue]) -> Result<ShaderValue, ShaderError> {
    let unknown = || ShaderError {
        message: format!("unknown function `{name}`"),
        line: 0,
        col: 0,
    };

    // Constructors: `vec3(1.0)` broadcasts, `vec3(a, b, c)` concatenates.
    if let Some(width) = match name {
        "vec2" => Some(2),
        "vec3" => Some(3),
        "vec4" => Some(4),
        _ => None,
    } {
        let mut comps: Vec<f32> = args.iter().flat_map(|a| a.components()).collect();
        if comps.len() == 1 {
            comps = vec![comps[0]; width];
        }
        comps.resize(width, 0.0);
        return Ok(ShaderValue::from_components(&comps));
    }
    if name == "float" {
        return Ok(ShaderValue::Float(args.first().map_or(0.0, |a| a.as_f32())));
    }
    if name == "int" {
        return Ok(ShaderValue::Int(
            args.first().map_or(0.0, |a| a.as_f32()) as i64
        ));
    }

    // Component-wise unary maths.
    let unary = |f: fn(f32) -> f32| -> Result<ShaderValue, ShaderError> {
        let a = args.first().ok_or_else(unknown)?;
        Ok(ShaderValue::from_components(
            &a.components().iter().map(|c| f(*c)).collect::<Vec<_>>(),
        ))
    };
    match name {
        "sin" => return unary(f32::sin),
        "cos" => return unary(f32::cos),
        "tan" => return unary(f32::tan),
        "abs" => return unary(f32::abs),
        "floor" => return unary(f32::floor),
        "ceil" => return unary(f32::ceil),
        "sqrt" => return unary(f32::sqrt),
        "fract" => return unary(|x| x - x.floor()),
        "sign" => return unary(f32::signum),
        // pat-s3rrv.36: the rest of Godot's component-wise scalar set. These
        // were missing entirely, so a shader calling any of them died with
        // "unknown function" and — through the renderers' old line matcher —
        // was silently skipped instead.
        "asin" => return unary(f32::asin),
        "acos" => return unary(f32::acos),
        "atan" if args.len() == 1 => return unary(f32::atan),
        "exp" => return unary(f32::exp),
        "exp2" => return unary(f32::exp2),
        "log" => return unary(f32::ln),
        "log2" => return unary(f32::log2),
        "radians" => return unary(f32::to_radians),
        "degrees" => return unary(f32::to_degrees),
        "sinh" => return unary(f32::sinh),
        "cosh" => return unary(f32::cosh),
        "tanh" => return unary(f32::tanh),
        // GLSL rounds halfway cases to even; Rust's `round` goes away from
        // zero, so `round(0.5)` would differ (1.0 vs GLSL's 0.0).
        "round" => {
            return unary(|x| {
                let r = x.round();
                if (x - x.trunc()).abs() == 0.5 && r % 2.0 != 0.0 {
                    r - x.signum()
                } else {
                    r
                }
            })
        }
        "roundEven" => {
            return unary(|x| {
                let r = x.round();
                if (x - x.trunc()).abs() == 0.5 && r % 2.0 != 0.0 {
                    r - x.signum()
                } else {
                    r
                }
            })
        }
        "trunc" => return unary(f32::trunc),
        "inversesqrt" => return unary(|x| 1.0 / x.sqrt()),
        // Screen-space derivatives have no meaning in a per-fragment CPU
        // evaluator with no neighbouring invocations. Godot's own software
        // paths return 0 rather than failing, and 0 is the correct derivative
        // of the constant this evaluator effectively sees.
        "dFdx" | "dFdy" | "fwidth" => return unary(|_| 0.0),
        "not" => {
            let a = args.first().ok_or_else(unknown)?;
            return Ok(ShaderValue::from_components(
                &a.components()
                    .iter()
                    .map(|c| if *c == 0.0 { 1.0 } else { 0.0 })
                    .collect::<Vec<_>>(),
            ));
        }
        // `any`/`all` collapse a boolean vector to one bool.
        "any" => {
            let a = args.first().ok_or_else(unknown)?;
            return Ok(ShaderValue::Bool(a.components().iter().any(|c| *c != 0.0)));
        }
        "all" => {
            let a = args.first().ok_or_else(unknown)?;
            return Ok(ShaderValue::Bool(a.components().iter().all(|c| *c != 0.0)));
        }
        _ => {}
    }

    // Component-wise binary maths.
    let binary = |f: fn(f32, f32) -> f32| -> Result<ShaderValue, ShaderError> {
        let (a, b) = (
            args.first().ok_or_else(unknown)?,
            args.get(1).ok_or_else(unknown)?,
        );
        let (ac, bc) = (a.components(), b.components());
        let width = ac.len().max(bc.len());
        let out: Vec<f32> = (0..width)
            .map(|i| {
                let x = if ac.len() == 1 {
                    ac[0]
                } else {
                    *ac.get(i).unwrap_or(&0.0)
                };
                let y = if bc.len() == 1 {
                    bc[0]
                } else {
                    *bc.get(i).unwrap_or(&0.0)
                };
                f(x, y)
            })
            .collect();
        Ok(ShaderValue::from_components(&out))
    };
    match name {
        "min" => return binary(f32::min),
        "max" => return binary(f32::max),
        // Godot spells two-argument arctangent `atan(y, x)`, not `atan2`, but
        // accept both — the one-argument form is handled with the other unary
        // maths above (pat-s3rrv.36).
        "atan" | "atan2" => return binary(f32::atan2),
        "pow" => return binary(f32::powf),
        "mod" => {
            return binary(|x, y| {
                if y == 0.0 {
                    0.0
                } else {
                    x - y * (x / y).floor()
                }
            })
        }
        "step" => return binary(|edge, x| if x < edge { 0.0 } else { 1.0 }),
        _ => {}
    }

    match name {
        "mix" => {
            let (a, b, t) = (
                args.first().ok_or_else(unknown)?,
                args.get(1).ok_or_else(unknown)?,
                args.get(2).ok_or_else(unknown)?,
            );
            let (ac, bc) = (a.components(), b.components());
            let tc = t.components();
            let width = ac.len().max(bc.len());
            let out: Vec<f32> = (0..width)
                .map(|i| {
                    let x = *ac.get(i).unwrap_or(&0.0);
                    let y = *bc.get(i).unwrap_or(&0.0);
                    let f = if tc.len() == 1 {
                        tc[0]
                    } else {
                        *tc.get(i).unwrap_or(&0.0)
                    };
                    x + (y - x) * f
                })
                .collect();
            Ok(ShaderValue::from_components(&out))
        }
        "clamp" => {
            let (v, lo, hi) = (
                args.first().ok_or_else(unknown)?,
                args.get(1).ok_or_else(unknown)?,
                args.get(2).ok_or_else(unknown)?,
            );
            let (lo, hi) = (lo.as_f32(), hi.as_f32());
            Ok(ShaderValue::from_components(
                &v.components()
                    .iter()
                    .map(|c| c.clamp(lo, hi))
                    .collect::<Vec<_>>(),
            ))
        }
        "smoothstep" => {
            // GLSL: t = clamp((x - e0) / (e1 - e0), 0, 1); t * t * (3 - 2t).
            let (e0, e1, x) = (
                args.first().ok_or_else(unknown)?.as_f32(),
                args.get(1).ok_or_else(unknown)?.as_f32(),
                args.get(2).ok_or_else(unknown)?,
            );
            let span = e1 - e0;
            Ok(ShaderValue::from_components(
                &x.components()
                    .iter()
                    .map(|c| {
                        if span == 0.0 {
                            return if *c < e0 { 0.0 } else { 1.0 };
                        }
                        let t = ((c - e0) / span).clamp(0.0, 1.0);
                        t * t * (3.0 - 2.0 * t)
                    })
                    .collect::<Vec<_>>(),
            ))
        }
        "length" => {
            let a = args.first().ok_or_else(unknown)?;
            Ok(ShaderValue::Float(
                a.components().iter().map(|c| c * c).sum::<f32>().sqrt(),
            ))
        }
        "dot" => {
            let (a, b) = (
                args.first().ok_or_else(unknown)?,
                args.get(1).ok_or_else(unknown)?,
            );
            let (ac, bc) = (a.components(), b.components());
            Ok(ShaderValue::Float(
                ac.iter().zip(bc.iter()).map(|(x, y)| x * y).sum(),
            ))
        }
        "normalize" => {
            let a = args.first().ok_or_else(unknown)?;
            let comps = a.components();
            let len = comps.iter().map(|c| c * c).sum::<f32>().sqrt();
            if len == 0.0 {
                return Ok(*a);
            }
            Ok(ShaderValue::from_components(
                &comps.iter().map(|c| c / len).collect::<Vec<_>>(),
            ))
        }
        // pat-s3rrv.36: the geometric and comparison builtins Godot defines
        // that this table was missing.
        "distance" => {
            let (a, b) = (
                args.first().ok_or_else(unknown)?,
                args.get(1).ok_or_else(unknown)?,
            );
            let (ac, bc) = (a.components(), b.components());
            let sum: f32 = ac
                .iter()
                .zip(bc.iter())
                .map(|(x, y)| (x - y) * (x - y))
                .sum();
            Ok(ShaderValue::Float(sum.sqrt()))
        }
        "cross" => {
            let (a, b) = (
                args.first().ok_or_else(unknown)?,
                args.get(1).ok_or_else(unknown)?,
            );
            let (u, v) = (a.components(), b.components());
            if u.len() < 3 || v.len() < 3 {
                return Err(unknown());
            }
            Ok(ShaderValue::Vec3([
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ]))
        }
        // `reflect(I, N)` = I - 2*dot(N, I)*N, with N assumed normalized.
        "reflect" => {
            let (i, n) = (
                args.first().ok_or_else(unknown)?,
                args.get(1).ok_or_else(unknown)?,
            );
            let (ic, nc) = (i.components(), n.components());
            let d: f32 = ic.iter().zip(nc.iter()).map(|(x, y)| x * y).sum();
            Ok(ShaderValue::from_components(
                &ic.iter()
                    .zip(nc.iter())
                    .map(|(x, y)| x - 2.0 * d * y)
                    .collect::<Vec<_>>(),
            ))
        }
        // `faceforward(N, I, Nref)` flips N away from I.
        "faceforward" => {
            let (n, i, nref) = (
                args.first().ok_or_else(unknown)?,
                args.get(1).ok_or_else(unknown)?,
                args.get(2).ok_or_else(unknown)?,
            );
            let (nc, ic, rc) = (n.components(), i.components(), nref.components());
            let d: f32 = rc.iter().zip(ic.iter()).map(|(x, y)| x * y).sum();
            Ok(ShaderValue::from_components(
                &nc.iter()
                    .map(|c| if d < 0.0 { *c } else { -*c })
                    .collect::<Vec<_>>(),
            ))
        }
        // `refract(I, N, eta)`; total internal reflection returns 0, as GLSL
        // specifies, rather than a NaN from the negative square root.
        "refract" => {
            let (i, n, eta) = (
                args.first().ok_or_else(unknown)?,
                args.get(1).ok_or_else(unknown)?,
                args.get(2).ok_or_else(unknown)?,
            );
            let (ic, nc) = (i.components(), n.components());
            let eta = eta.as_f32();
            let d: f32 = nc.iter().zip(ic.iter()).map(|(x, y)| x * y).sum();
            let k = 1.0 - eta * eta * (1.0 - d * d);
            if k < 0.0 {
                return Ok(ShaderValue::from_components(&vec![0.0; ic.len()]));
            }
            let scale = eta * d + k.sqrt();
            Ok(ShaderValue::from_components(
                &ic.iter()
                    .zip(nc.iter())
                    .map(|(x, y)| eta * x - scale * y)
                    .collect::<Vec<_>>(),
            ))
        }
        // Component-wise comparisons return a boolean vector, which this value
        // model represents as 1.0/0.0 components — `any`/`all` then collapse it.
        "lessThan" | "lessThanEqual" | "greaterThan" | "greaterThanEqual" | "equal"
        | "notEqual" => {
            let (a, b) = (
                args.first().ok_or_else(unknown)?,
                args.get(1).ok_or_else(unknown)?,
            );
            let (ac, bc) = (a.components(), b.components());
            let out: Vec<f32> = ac
                .iter()
                .zip(bc.iter())
                .map(|(x, y)| {
                    let t = match name {
                        "lessThan" => x < y,
                        "lessThanEqual" => x <= y,
                        "greaterThan" => x > y,
                        "greaterThanEqual" => x >= y,
                        "equal" => x == y,
                        _ => x != y,
                    };
                    if t {
                        1.0
                    } else {
                        0.0
                    }
                })
                .collect();
            Ok(ShaderValue::from_components(&out))
        }
        _ => Err(unknown()),
    }
}
