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
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    /// `-`
    Neg,
    /// `not`
    Not,
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
}

/// An annotation on a declaration (e.g. `@export`, `@onready`).
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    /// Annotation name (e.g. "export", "onready").
    pub name: String,
}

/// A statement node in the GDScript AST.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
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
        /// Parameter names.
        params: Vec<String>,
        /// Optional return type hint.
        return_type: Option<String>,
        /// Function body.
        body: Vec<Stmt>,
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: MatchPattern,
    pub body: Vec<Stmt>,
}
#[derive(Debug, Clone, PartialEq)]
pub enum MatchPattern {
    Literal(Variant),
    Variable(String),
    Wildcard,
    Array(Vec<MatchPattern>),
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
            Token::If => self.parse_if(),
            Token::While => self.parse_while(),
            Token::For => self.parse_for(),
            Token::Return => self.parse_return(),
            Token::Func => self.parse_func_def(),
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
            annotations.push(Annotation { name });
            self.skip_newlines();
        }
        // After annotations, expect a var decl (or could be func, etc.)
        match self.peek().clone() {
            Token::Var => self.parse_var_decl_with_annotations(annotations),
            _ => {
                let ts = self.tokens.get(self.pos);
                let (token, line, col) = match ts {
                    Some(ts) => (ts.token.to_string(), ts.line, ts.col),
                    None => ("EOF".to_string(), 0, 0),
                };
                Err(ParseError::UnexpectedToken {
                    token,
                    expected: "var declaration after annotation".to_string(),
                    line,
                    col,
                    source_line: None,
                })
            }
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
            Some(self.eat_ident()?)
        } else {
            None
        };
        let value = if self.check(&Token::Assign) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Stmt::VarDecl {
            name,
            type_hint,
            value,
            annotations,
        })
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
                while self.check(&Token::Comma) {
                    self.advance();
                    params.push(self.eat_ident()?);
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

    fn parse_func_def(&mut self) -> Result<Stmt, ParseError> {
        self.advance(); // consume `func`
        let name = self.eat_ident()?;
        self.expect(&Token::LParen)?;

        let mut params = Vec::new();
        if !self.check(&Token::RParen) {
            params.push(self.eat_ident()?);
            while self.check(&Token::Comma) {
                self.advance();
                params.push(self.eat_ident()?);
            }
        }
        self.expect(&Token::RParen)?;

        let return_type = if self.check(&Token::Arrow) {
            self.advance();
            Some(self.eat_ident()?)
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
            let pattern = self.parse_match_pattern()?;
            self.expect(&Token::Colon)?;
            let body = self.parse_block()?;
            arms.push(MatchArm { pattern, body });
            self.skip_newlines();
        }
        if self.check(&Token::Dedent) {
            self.advance();
        }
        Ok(Stmt::Match { value, arms })
    }

    fn parse_match_pattern(&mut self) -> Result<MatchPattern, ParseError> {
        match self.peek().clone() {
            Token::IntLit(v) => {
                self.advance();
                Ok(MatchPattern::Literal(Variant::Int(v)))
            }
            Token::FloatLit(v) => {
                self.advance();
                Ok(MatchPattern::Literal(Variant::Float(v)))
            }
            Token::StringLit(v) => {
                self.advance();
                Ok(MatchPattern::Literal(Variant::String(v)))
            }
            Token::BoolLit(v) => {
                self.advance();
                Ok(MatchPattern::Literal(Variant::Bool(v)))
            }
            Token::Null => {
                self.advance();
                Ok(MatchPattern::Literal(Variant::Nil))
            }
            Token::Ident(name) if name == "_" => {
                self.advance();
                Ok(MatchPattern::Wildcard)
            }
            Token::Ident(name) => {
                self.advance();
                Ok(MatchPattern::Variable(name))
            }
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
            Token::Minus => {
                self.advance();
                match self.peek().clone() {
                    Token::IntLit(v) => {
                        self.advance();
                        Ok(MatchPattern::Literal(Variant::Int(-v)))
                    }
                    Token::FloatLit(v) => {
                        self.advance();
                        Ok(MatchPattern::Literal(Variant::Float(-v)))
                    }
                    _ => {
                        let ts = self.tokens.get(self.pos);
                        let (token, line, col) = match ts {
                            Some(ts) => (ts.token.to_string(), ts.line, ts.col),
                            None => ("EOF".to_string(), 0, 0),
                        };
                        Err(ParseError::UnexpectedToken {
                            token,
                            expected: "number after minus".to_string(),
                            line,
                            col,
                            source_line: None,
                        })
                    }
                }
            }
            _ => {
                let ts = self.tokens.get(self.pos);
                let (token, line, col) = match ts {
                    Some(ts) => (ts.token.to_string(), ts.line, ts.col),
                    None => ("EOF".to_string(), 0, 0),
                };
                Err(ParseError::UnexpectedToken {
                    token,
                    expected: "match pattern".to_string(),
                    line,
                    col,
                    source_line: None,
                })
            }
        }
    }

    fn parse_expr_or_assign(&mut self) -> Result<Stmt, ParseError> {
        let expr = self.parse_expr()?;

        match self.peek() {
            Token::Assign => {
                self.advance();
                let value = self.parse_expr()?;
                Ok(Stmt::Assignment {
                    target: expr,
                    op: AssignOp::Assign,
                    value,
                })
            }
            Token::PlusAssign => {
                self.advance();
                let value = self.parse_expr()?;
                Ok(Stmt::Assignment {
                    target: expr,
                    op: AssignOp::AddAssign,
                    value,
                })
            }
            Token::MinusAssign => {
                self.advance();
                let value = self.parse_expr()?;
                Ok(Stmt::Assignment {
                    target: expr,
                    op: AssignOp::SubAssign,
                    value,
                })
            }
            _ => Ok(Stmt::ExprStmt(expr)),
        }
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
        let value = self.parse_or()?;
        if self.check(&Token::If) {
            self.advance();
            let condition = self.parse_or()?;
            self.expect(&Token::Else)?;
            let else_value = self.parse_expr()?;
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
        let mut left = self.parse_addition()?;
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
            _ => self.parse_postfix(),
        }
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
            assert_eq!(params, &["a", "b"]);
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
}
