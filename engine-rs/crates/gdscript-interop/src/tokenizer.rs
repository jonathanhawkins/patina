//! GDScript tokenizer (lexer).
//!
//! Converts GDScript source text into a stream of [`TokenSpan`]s, handling
//! keywords, literals, operators, punctuation, comments, and indentation-based
//! `Indent`/`Dedent` tokens.

use std::fmt;

/// A lexical token produced by the GDScript tokenizer.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Keywords
    /// `var`
    Var,
    /// `func`
    Func,
    /// `if`
    If,
    /// `else`
    Else,
    /// `elif`
    Elif,
    /// `while`
    While,
    /// `for`
    For,
    /// `in`
    In,
    /// `return`
    Return,
    /// `class`
    Class,
    /// `extends`
    Extends,
    /// `signal`
    Signal,
    /// `enum`
    Enum,
    /// `match`
    Match,
    /// `pass`
    Pass,
    /// `break`
    Break,
    /// `continue`
    Continue,
    /// `const`
    Const,
    /// `static`
    Static,

    // Literals
    /// Integer literal.
    IntLit(i64),
    /// Float literal.
    FloatLit(f64),
    /// String literal (contents only, no quotes).
    StringLit(String),
    /// Boolean literal (`true` / `false`).
    BoolLit(bool),
    /// `null`
    Null,

    // Identifiers
    /// An identifier.
    Ident(String),

    // Operators
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `%`
    Percent,
    /// `==`
    EqEq,
    /// `!=`
    BangEq,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `<=`
    LtEq,
    /// `>=`
    GtEq,
    /// `and`
    And,
    /// `or`
    Or,
    /// `not`
    Not,
    /// `=`
    Assign,
    /// `+=`
    PlusAssign,
    /// `-=`
    MinusAssign,
    /// `->`
    Arrow,

    // Punctuation
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `:`
    Colon,
    /// `,`
    Comma,
    /// `.`
    Dot,
    /// `;`
    Semicolon,

    // Indentation
    /// An increase in indentation level.
    Indent,
    /// A decrease in indentation level.
    Dedent,
    /// End of a logical line.
    Newline,
    /// End of file.
    Eof,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Var => write!(f, "var"),
            Token::Func => write!(f, "func"),
            Token::If => write!(f, "if"),
            Token::Else => write!(f, "else"),
            Token::Elif => write!(f, "elif"),
            Token::While => write!(f, "while"),
            Token::For => write!(f, "for"),
            Token::In => write!(f, "in"),
            Token::Return => write!(f, "return"),
            Token::Class => write!(f, "class"),
            Token::Extends => write!(f, "extends"),
            Token::Signal => write!(f, "signal"),
            Token::Enum => write!(f, "enum"),
            Token::Match => write!(f, "match"),
            Token::Pass => write!(f, "pass"),
            Token::Break => write!(f, "break"),
            Token::Continue => write!(f, "continue"),
            Token::Const => write!(f, "const"),
            Token::Static => write!(f, "static"),
            Token::IntLit(v) => write!(f, "{v}"),
            Token::FloatLit(v) => write!(f, "{v}"),
            Token::StringLit(v) => write!(f, "\"{v}\""),
            Token::BoolLit(v) => write!(f, "{v}"),
            Token::Null => write!(f, "null"),
            Token::Ident(name) => write!(f, "{name}"),
            Token::Plus => write!(f, "+"),
            Token::Minus => write!(f, "-"),
            Token::Star => write!(f, "*"),
            Token::Slash => write!(f, "/"),
            Token::Percent => write!(f, "%"),
            Token::EqEq => write!(f, "=="),
            Token::BangEq => write!(f, "!="),
            Token::Lt => write!(f, "<"),
            Token::Gt => write!(f, ">"),
            Token::LtEq => write!(f, "<="),
            Token::GtEq => write!(f, ">="),
            Token::And => write!(f, "and"),
            Token::Or => write!(f, "or"),
            Token::Not => write!(f, "not"),
            Token::Assign => write!(f, "="),
            Token::PlusAssign => write!(f, "+="),
            Token::MinusAssign => write!(f, "-="),
            Token::Arrow => write!(f, "->"),
            Token::LParen => write!(f, "("),
            Token::RParen => write!(f, ")"),
            Token::LBracket => write!(f, "["),
            Token::RBracket => write!(f, "]"),
            Token::LBrace => write!(f, "{{"),
            Token::RBrace => write!(f, "}}"),
            Token::Colon => write!(f, ":"),
            Token::Comma => write!(f, ","),
            Token::Dot => write!(f, "."),
            Token::Semicolon => write!(f, ";"),
            Token::Indent => write!(f, "INDENT"),
            Token::Dedent => write!(f, "DEDENT"),
            Token::Newline => write!(f, "NEWLINE"),
            Token::Eof => write!(f, "EOF"),
        }
    }
}

/// A token together with its source location.
#[derive(Debug, Clone)]
pub struct TokenSpan {
    /// The token.
    pub token: Token,
    /// 1-based line number.
    pub line: usize,
    /// 1-based column number.
    pub col: usize,
}

/// An error produced during lexical analysis.
#[derive(Debug, Clone, thiserror::Error)]
pub enum LexError {
    /// An unterminated string literal was found.
    #[error("unterminated string at line {line}, column {col}")]
    UnterminatedString {
        /// Line where the string started.
        line: usize,
        /// Column where the string started.
        col: usize,
    },

    /// An unexpected character was encountered.
    #[error("unexpected character '{ch}' at line {line}, column {col}")]
    UnexpectedChar {
        /// The unexpected character.
        ch: char,
        /// Line of the character.
        line: usize,
        /// Column of the character.
        col: usize,
    },

    /// An invalid escape sequence in a string.
    #[error("invalid escape sequence '\\{ch}' at line {line}, column {col}")]
    InvalidEscape {
        /// The character after the backslash.
        ch: char,
        /// Line of the escape.
        line: usize,
        /// Column of the escape.
        col: usize,
    },
}

/// Tokenizes GDScript source code into a sequence of [`TokenSpan`]s.
///
/// Handles indentation tracking, producing `Indent` and `Dedent` tokens as
/// the indentation level changes. Comments (starting with `#`) are stripped.
pub fn tokenize(source: &str) -> Result<Vec<TokenSpan>, LexError> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = source.chars().collect();
    let len = chars.len();
    let mut pos = 0;
    let mut line: usize = 1;
    let mut col: usize = 1;
    let mut indent_stack: Vec<usize> = vec![0];
    let mut at_line_start = true;

    while pos < len {
        // Handle line-start indentation
        if at_line_start {
            let mut indent = 0;
            while pos < len && chars[pos] == ' ' {
                indent += 1;
                pos += 1;
                col += 1;
            }
            // Also count tabs as 4 spaces each
            while pos < len && chars[pos] == '\t' {
                indent += 4;
                pos += 1;
                col += 1;
            }

            at_line_start = false;

            // Skip blank lines and comment-only lines
            if pos >= len || chars[pos] == '\n' || chars[pos] == '\r' || chars[pos] == '#' {
                // Don't emit indent/dedent for blank/comment lines
                if pos < len && chars[pos] == '#' {
                    // Skip to end of line
                    while pos < len && chars[pos] != '\n' {
                        pos += 1;
                        col += 1;
                    }
                }
                if pos < len && (chars[pos] == '\n' || chars[pos] == '\r') {
                    if chars[pos] == '\r' && pos + 1 < len && chars[pos + 1] == '\n' {
                        pos += 2;
                    } else {
                        pos += 1;
                    }
                    line += 1;
                    col = 1;
                    at_line_start = true;
                }
                continue;
            }

            let current_indent = *indent_stack.last().unwrap();
            if indent > current_indent {
                indent_stack.push(indent);
                tokens.push(TokenSpan {
                    token: Token::Indent,
                    line,
                    col: 1,
                });
            } else if indent < current_indent {
                while *indent_stack.last().unwrap() > indent {
                    indent_stack.pop();
                    tokens.push(TokenSpan {
                        token: Token::Dedent,
                        line,
                        col: 1,
                    });
                }
            }
            continue;
        }

        let ch = chars[pos];

        // Skip spaces/tabs mid-line
        if ch == ' ' || ch == '\t' {
            pos += 1;
            col += 1;
            continue;
        }

        // Comments
        if ch == '#' {
            while pos < len && chars[pos] != '\n' {
                pos += 1;
                col += 1;
            }
            continue;
        }

        // Newlines
        if ch == '\n' || ch == '\r' {
            tokens.push(TokenSpan {
                token: Token::Newline,
                line,
                col,
            });
            if ch == '\r' && pos + 1 < len && chars[pos + 1] == '\n' {
                pos += 2;
            } else {
                pos += 1;
            }
            line += 1;
            col = 1;
            at_line_start = true;
            continue;
        }

        // Strings
        if ch == '"' || ch == '\'' {
            let quote = ch;
            let start_line = line;
            let start_col = col;
            pos += 1;
            col += 1;
            let mut s = String::new();
            loop {
                if pos >= len {
                    return Err(LexError::UnterminatedString {
                        line: start_line,
                        col: start_col,
                    });
                }
                let c = chars[pos];
                if c == quote {
                    pos += 1;
                    col += 1;
                    break;
                }
                if c == '\\' {
                    pos += 1;
                    col += 1;
                    if pos >= len {
                        return Err(LexError::UnterminatedString {
                            line: start_line,
                            col: start_col,
                        });
                    }
                    let esc = chars[pos];
                    match esc {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        'r' => s.push('\r'),
                        '\\' => s.push('\\'),
                        '\'' => s.push('\''),
                        '"' => s.push('"'),
                        '0' => s.push('\0'),
                        _ => {
                            return Err(LexError::InvalidEscape { ch: esc, line, col });
                        }
                    }
                    pos += 1;
                    col += 1;
                } else if c == '\n' {
                    return Err(LexError::UnterminatedString {
                        line: start_line,
                        col: start_col,
                    });
                } else {
                    s.push(c);
                    pos += 1;
                    col += 1;
                }
            }
            tokens.push(TokenSpan {
                token: Token::StringLit(s),
                line: start_line,
                col: start_col,
            });
            continue;
        }

        // Numbers
        if ch.is_ascii_digit() {
            let start_col = col;
            let mut num_str = String::new();
            let mut is_float = false;
            while pos < len
                && (chars[pos].is_ascii_digit() || chars[pos] == '.' || chars[pos] == '_')
            {
                if chars[pos] == '.' {
                    // Check for .. or method call like 1.method
                    if pos + 1 < len && chars[pos + 1] == '.' {
                        break;
                    }
                    if is_float {
                        break;
                    }
                    is_float = true;
                }
                if chars[pos] != '_' {
                    num_str.push(chars[pos]);
                }
                pos += 1;
                col += 1;
            }
            if is_float {
                let val: f64 = num_str.parse().unwrap_or(0.0);
                tokens.push(TokenSpan {
                    token: Token::FloatLit(val),
                    line,
                    col: start_col,
                });
            } else {
                let val: i64 = num_str.parse().unwrap_or(0);
                tokens.push(TokenSpan {
                    token: Token::IntLit(val),
                    line,
                    col: start_col,
                });
            }
            continue;
        }

        // Identifiers and keywords
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start_col = col;
            let mut ident = String::new();
            while pos < len && (chars[pos].is_ascii_alphanumeric() || chars[pos] == '_') {
                ident.push(chars[pos]);
                pos += 1;
                col += 1;
            }
            let token = match ident.as_str() {
                "var" => Token::Var,
                "func" => Token::Func,
                "if" => Token::If,
                "else" => Token::Else,
                "elif" => Token::Elif,
                "while" => Token::While,
                "for" => Token::For,
                "in" => Token::In,
                "return" => Token::Return,
                "class" => Token::Class,
                "extends" => Token::Extends,
                "signal" => Token::Signal,
                "enum" => Token::Enum,
                "match" => Token::Match,
                "pass" => Token::Pass,
                "break" => Token::Break,
                "continue" => Token::Continue,
                "const" => Token::Const,
                "static" => Token::Static,
                "true" => Token::BoolLit(true),
                "false" => Token::BoolLit(false),
                "null" => Token::Null,
                "and" => Token::And,
                "or" => Token::Or,
                "not" => Token::Not,
                _ => Token::Ident(ident),
            };
            tokens.push(TokenSpan {
                token,
                line,
                col: start_col,
            });
            continue;
        }

        // Multi-character operators
        let start_col = col;
        if pos + 1 < len {
            let two: String = chars[pos..pos + 2].iter().collect();
            match two.as_str() {
                "==" => {
                    tokens.push(TokenSpan {
                        token: Token::EqEq,
                        line,
                        col: start_col,
                    });
                    pos += 2;
                    col += 2;
                    continue;
                }
                "!=" => {
                    tokens.push(TokenSpan {
                        token: Token::BangEq,
                        line,
                        col: start_col,
                    });
                    pos += 2;
                    col += 2;
                    continue;
                }
                "<=" => {
                    tokens.push(TokenSpan {
                        token: Token::LtEq,
                        line,
                        col: start_col,
                    });
                    pos += 2;
                    col += 2;
                    continue;
                }
                ">=" => {
                    tokens.push(TokenSpan {
                        token: Token::GtEq,
                        line,
                        col: start_col,
                    });
                    pos += 2;
                    col += 2;
                    continue;
                }
                "+=" => {
                    tokens.push(TokenSpan {
                        token: Token::PlusAssign,
                        line,
                        col: start_col,
                    });
                    pos += 2;
                    col += 2;
                    continue;
                }
                "-=" => {
                    tokens.push(TokenSpan {
                        token: Token::MinusAssign,
                        line,
                        col: start_col,
                    });
                    pos += 2;
                    col += 2;
                    continue;
                }
                "->" => {
                    tokens.push(TokenSpan {
                        token: Token::Arrow,
                        line,
                        col: start_col,
                    });
                    pos += 2;
                    col += 2;
                    continue;
                }
                _ => {}
            }
        }

        // Single-character tokens
        let token = match ch {
            '+' => Token::Plus,
            '-' => Token::Minus,
            '*' => Token::Star,
            '/' => Token::Slash,
            '%' => Token::Percent,
            '<' => Token::Lt,
            '>' => Token::Gt,
            '=' => Token::Assign,
            '(' => Token::LParen,
            ')' => Token::RParen,
            '[' => Token::LBracket,
            ']' => Token::RBracket,
            '{' => Token::LBrace,
            '}' => Token::RBrace,
            ':' => Token::Colon,
            ',' => Token::Comma,
            '.' => Token::Dot,
            ';' => Token::Semicolon,
            _ => {
                return Err(LexError::UnexpectedChar { ch, line, col });
            }
        };
        tokens.push(TokenSpan {
            token,
            line,
            col: start_col,
        });
        pos += 1;
        col += 1;
    }

    // Emit remaining dedents at EOF
    while indent_stack.len() > 1 {
        indent_stack.pop();
        tokens.push(TokenSpan {
            token: Token::Dedent,
            line,
            col,
        });
    }

    tokens.push(TokenSpan {
        token: Token::Eof,
        line,
        col,
    });

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok_types(src: &str) -> Vec<Token> {
        tokenize(src)
            .unwrap()
            .into_iter()
            .map(|ts| ts.token)
            .collect()
    }

    #[test]
    fn tokenize_var_declaration() {
        let tokens = tok_types("var x = 10");
        assert_eq!(
            tokens,
            vec![
                Token::Var,
                Token::Ident("x".into()),
                Token::Assign,
                Token::IntLit(10),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenize_string_literals() {
        let tokens = tok_types(r#""hello" 'world'"#);
        assert_eq!(
            tokens,
            vec![
                Token::StringLit("hello".into()),
                Token::StringLit("world".into()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenize_string_escapes() {
        let tokens = tok_types(r#""line\nbreak""#);
        assert_eq!(
            tokens,
            vec![Token::StringLit("line\nbreak".into()), Token::Eof]
        );
    }

    #[test]
    fn tokenize_float_literal() {
        let tokens = tok_types("3.14");
        assert_eq!(tokens, vec![Token::FloatLit(3.14), Token::Eof]);
    }

    #[test]
    fn tokenize_keywords() {
        let tokens = tok_types("if else elif while for in return");
        assert_eq!(
            tokens,
            vec![
                Token::If,
                Token::Else,
                Token::Elif,
                Token::While,
                Token::For,
                Token::In,
                Token::Return,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenize_boolean_and_null() {
        let tokens = tok_types("true false null");
        assert_eq!(
            tokens,
            vec![
                Token::BoolLit(true),
                Token::BoolLit(false),
                Token::Null,
                Token::Eof
            ]
        );
    }

    #[test]
    fn tokenize_operators() {
        let tokens = tok_types("+ - * / % == != < > <= >= = += -= ->");
        assert_eq!(
            tokens,
            vec![
                Token::Plus,
                Token::Minus,
                Token::Star,
                Token::Slash,
                Token::Percent,
                Token::EqEq,
                Token::BangEq,
                Token::Lt,
                Token::Gt,
                Token::LtEq,
                Token::GtEq,
                Token::Assign,
                Token::PlusAssign,
                Token::MinusAssign,
                Token::Arrow,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenize_punctuation() {
        let tokens = tok_types("( ) [ ] { } : , . ;");
        assert_eq!(
            tokens,
            vec![
                Token::LParen,
                Token::RParen,
                Token::LBracket,
                Token::RBracket,
                Token::LBrace,
                Token::RBrace,
                Token::Colon,
                Token::Comma,
                Token::Dot,
                Token::Semicolon,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenize_indent_dedent() {
        let src = "if true:\n    pass\n";
        let tokens = tok_types(src);
        assert_eq!(
            tokens,
            vec![
                Token::If,
                Token::BoolLit(true),
                Token::Colon,
                Token::Newline,
                Token::Indent,
                Token::Pass,
                Token::Newline,
                Token::Dedent,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenize_nested_indent() {
        let src = "if true:\n    if false:\n        pass\n";
        let tokens = tok_types(src);
        assert_eq!(
            tokens,
            vec![
                Token::If,
                Token::BoolLit(true),
                Token::Colon,
                Token::Newline,
                Token::Indent,
                Token::If,
                Token::BoolLit(false),
                Token::Colon,
                Token::Newline,
                Token::Indent,
                Token::Pass,
                Token::Newline,
                Token::Dedent,
                Token::Dedent,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenize_comment_skipped() {
        let tokens = tok_types("x # comment\n");
        assert_eq!(
            tokens,
            vec![Token::Ident("x".into()), Token::Newline, Token::Eof]
        );
    }

    #[test]
    fn tokenize_logical_operators() {
        let tokens = tok_types("and or not");
        assert_eq!(tokens, vec![Token::And, Token::Or, Token::Not, Token::Eof]);
    }

    #[test]
    fn unterminated_string_error() {
        let err = tokenize("\"hello").unwrap_err();
        assert!(matches!(err, LexError::UnterminatedString { .. }));
    }

    #[test]
    fn unexpected_char_error() {
        let err = tokenize("@").unwrap_err();
        assert!(matches!(err, LexError::UnexpectedChar { ch: '@', .. }));
    }

    #[test]
    fn tokenize_func_definition() {
        let src = "func add(a, b):\n    return a + b\n";
        let tokens = tok_types(src);
        assert_eq!(
            tokens,
            vec![
                Token::Func,
                Token::Ident("add".into()),
                Token::LParen,
                Token::Ident("a".into()),
                Token::Comma,
                Token::Ident("b".into()),
                Token::RParen,
                Token::Colon,
                Token::Newline,
                Token::Indent,
                Token::Return,
                Token::Ident("a".into()),
                Token::Plus,
                Token::Ident("b".into()),
                Token::Newline,
                Token::Dedent,
                Token::Eof,
            ]
        );
    }
}
