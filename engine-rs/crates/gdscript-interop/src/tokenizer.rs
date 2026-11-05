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
    /// `self`
    Self_,
    /// `super`
    Super,
    /// `class_name`
    ClassName,
    /// `@onready`
    Onready,
    /// `@export`
    Export,
    /// `await`
    Await,
    /// `is` — type-test operator.
    Is,
    /// `as` — type-cast operator.
    As,
    /// `when` — match pattern guard.
    When,

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
    /// `*=`
    StarAssign,
    /// `/=`
    SlashAssign,
    /// `%=`
    PercentAssign,
    /// `**`
    StarStar,
    /// `**=`
    StarStarAssign,
    /// `&`
    Amp,
    /// `|`
    Pipe,
    /// `^`
    Caret,
    /// `~`
    Tilde,
    /// `<<`
    Shl,
    /// `>>`
    Shr,
    /// `&=`
    AmpAssign,
    /// `|=`
    PipeAssign,
    /// `^=`
    CaretAssign,
    /// `<<=`
    ShlAssign,
    /// `>>=`
    ShrAssign,
    /// `->`
    Arrow,
    /// `@`
    AtSign,
    /// `$` — get_node shorthand.
    Dollar,

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
    /// `..` — rest pattern in `match`.
    DotDot,
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
            Token::Self_ => write!(f, "self"),
            Token::Super => write!(f, "super"),
            Token::ClassName => write!(f, "class_name"),
            Token::Onready => write!(f, "onready"),
            Token::Export => write!(f, "export"),
            Token::Await => write!(f, "await"),
            Token::Is => write!(f, "is"),
            Token::As => write!(f, "as"),
            Token::When => write!(f, "when"),
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
            Token::StarAssign => write!(f, "*="),
            Token::SlashAssign => write!(f, "/="),
            Token::PercentAssign => write!(f, "%="),
            Token::StarStar => write!(f, "**"),
            Token::StarStarAssign => write!(f, "**="),
            Token::Amp => write!(f, "&"),
            Token::Pipe => write!(f, "|"),
            Token::Caret => write!(f, "^"),
            Token::Tilde => write!(f, "~"),
            Token::Shl => write!(f, "<<"),
            Token::Shr => write!(f, ">>"),
            Token::AmpAssign => write!(f, "&="),
            Token::PipeAssign => write!(f, "|="),
            Token::CaretAssign => write!(f, "^="),
            Token::ShlAssign => write!(f, "<<="),
            Token::ShrAssign => write!(f, ">>="),
            Token::Arrow => write!(f, "->"),
            Token::AtSign => write!(f, "@"),
            Token::Dollar => write!(f, "$"),
            Token::LParen => write!(f, "("),
            Token::RParen => write!(f, ")"),
            Token::LBracket => write!(f, "["),
            Token::RBracket => write!(f, "]"),
            Token::LBrace => write!(f, "{{"),
            Token::RBrace => write!(f, "}}"),
            Token::Colon => write!(f, ":"),
            Token::Comma => write!(f, ","),
            Token::Dot => write!(f, "."),
            Token::DotDot => write!(f, ".."),
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

/// One open nesting level while tokenizing (pat-rp4bp).
///
/// Godot's tokenizer is driven by the parser: `(`/`[`/`{` switch it into
/// multiline mode (no NEWLINE, no indentation), and `parse_lambda` switches it
/// BACK for the lambda body with `push_expression_indented_block` /
/// `pop_expression_indented_block` (`gdscript_parser.cpp`). This tokenizer runs
/// ahead of the parser, so it recognises the lambda body itself: a `func`
/// inside brackets followed by its `:` at the same nesting level.
enum Nest {
    /// An open bracket: newlines and indentation are suppressed.
    Bracket,
    /// A lambda body inside brackets: newlines and indentation are live again.
    Lambda {
        /// The indent stack when the body began; restored when it ends, which
        /// is what Godot's `pop_expression_indented_block` does.
        saved_indent: Vec<usize>,
        /// Indentation of the body's first line (multi-line bodies only).
        body_indent: Option<usize>,
        /// `func(x): return x` — the body is the rest of the header line.
        single_line: bool,
    },
}

/// Ends the innermost lambda body: emits the Dedents that close its block and
/// restores the indent stack it started from.
fn end_lambda(
    nest: &mut Vec<Nest>,
    indent_stack: &mut Vec<usize>,
    tokens: &mut Vec<TokenSpan>,
    line: usize,
    col: usize,
) {
    if let Some(Nest::Lambda { saved_indent, .. }) = nest.pop() {
        if indent_stack.len() > saved_indent.len() {
            // The closer sits on the body's last line (`x = 1)`): the block
            // still needs its terminating Newline before the Dedents.
            if !matches!(tokens.last().map(|t| &t.token), Some(Token::Newline)) {
                tokens.push(TokenSpan {
                    token: Token::Newline,
                    line,
                    col,
                });
            }
            while indent_stack.len() > saved_indent.len() {
                indent_stack.pop();
                tokens.push(TokenSpan {
                    token: Token::Dedent,
                    line,
                    col,
                });
            }
        }
        *indent_stack = saved_indent;
    }
}

/// A closing bracket: ends any lambda bodies opened inside it, then pops the
/// bracket itself. Saturates on a stray closer so it cannot underflow.
fn close_bracket(
    nest: &mut Vec<Nest>,
    indent_stack: &mut Vec<usize>,
    tokens: &mut Vec<TokenSpan>,
    line: usize,
    col: usize,
) {
    while matches!(nest.last(), Some(Nest::Lambda { .. })) {
        end_lambda(nest, indent_stack, tokens, line, col);
    }
    nest.pop();
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
    // Open `(`/`[`/`{` nesting. Godot's tokenizer suppresses NEWLINE and all
    // indentation changes while this is non-zero
    // (`gdscript_tokenizer.cpp`, paren-depth newline suppression), which is what
    // makes a multi-line array literal, dictionary or call argument list legal.
    // Without it `var a = [\n 1,\n 2\n]` emitted a Newline mid-expression and
    // died in `parse_primary`, which never skips Newline/Indent (pat-mgixk.17).
    //
    // A lambda body inside brackets re-enables both (see [`Nest`]), so the
    // nesting is a stack rather than a counter (pat-rp4bp): truck_town's
    // `t.finished.connect(func() -> void:` + indented body + `)` died with
    // "unexpected token =" because the body's Newline/Indent were swallowed.
    let mut nest: Vec<Nest> = Vec::new();
    // `nest.len()` at a `func` keyword seen inside brackets: the `:` at that
    // same level opens the lambda's body.
    let mut pending_lambda: Option<usize> = None;

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

            // Inside brackets, indentation carries no structure: the line is a
            // continuation of the expression, so neither Indent/Dedent nor the
            // indent stack may move. Leaving the stack alone matters as much as
            // skipping the tokens — a pushed level would desync every Dedent
            // after the closing bracket.
            match nest.last_mut() {
                Some(Nest::Bracket) => continue,
                Some(Nest::Lambda {
                    saved_indent,
                    body_indent,
                    ..
                }) => {
                    // A line indented less than the body's first line ends the
                    // body (Godot: the DEDENT that ends `parse_suite`); the rest
                    // of the line is bracket continuation again.
                    let outer = *saved_indent.last().unwrap();
                    let ends = match *body_indent {
                        Some(b) => indent < b,
                        None => indent <= outer,
                    };
                    if ends {
                        end_lambda(&mut nest, &mut indent_stack, &mut tokens, line, 1);
                        continue;
                    }
                    if body_indent.is_none() {
                        *body_indent = Some(indent);
                    }
                }
                None => {}
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

        // A trailing backslash joins the next line onto this one, so no Newline
        // is emitted and the continuation's indentation is not structural.
        if ch == '\\' {
            let mut peek = pos + 1;
            while peek < len && (chars[peek] == ' ' || chars[peek] == '\t') {
                peek += 1;
            }
            if peek < len && (chars[peek] == '\n' || chars[peek] == '\r') {
                pos = if chars[peek] == '\r' && peek + 1 < len && chars[peek + 1] == '\n' {
                    peek + 2
                } else {
                    peek + 1
                };
                line += 1;
                col = 1;
                // Deliberately NOT `at_line_start = true`: the joined line must
                // not produce Indent/Dedent. Leading whitespace on it is eaten
                // by the mid-line space skip above.
                continue;
            }
        }

        // Newlines
        if ch == '\n' || ch == '\r' {
            // Suppressed inside brackets — see `nest`.
            match nest.last() {
                Some(Nest::Bracket) => {}
                // A one-line lambda body ends with its line; what follows is
                // bracket continuation (Godot's parser skips the NEWLINE).
                Some(Nest::Lambda {
                    single_line: true, ..
                }) => {
                    nest.pop();
                }
                _ => tokens.push(TokenSpan {
                    token: Token::Newline,
                    line,
                    col,
                }),
            }
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

        // Strings, in Godot's three forms (`string()`,
        // `gdscript_tokenizer.cpp`): ordinary, triple-quoted (newlines allowed)
        // and `r`-prefixed raw (no escape processing).
        //
        // ⚠️ The raw check has to happen HERE, before the identifier scanner
        // below, because `r"..."` begins with an alphabetic char. It fires only
        // when a quote immediately follows, so a variable actually named `r`
        // still lexes as an identifier.
        let raw_prefix =
            ch == 'r' && pos + 1 < len && (chars[pos + 1] == '"' || chars[pos + 1] == '\'');
        if ch == '"' || ch == '\'' || raw_prefix {
            let is_raw = raw_prefix;
            let qpos = if is_raw { pos + 1 } else { pos };
            let quote = chars[qpos];
            // Three identical quotes open a triple-quoted string, inside which
            // a bare newline is content rather than the error it used to be.
            let is_triple = qpos + 2 < len && chars[qpos + 1] == quote && chars[qpos + 2] == quote;
            let start_line = line;
            let start_col = col;
            let opener = if is_triple { 3 } else { 1 };
            col += (qpos - pos) + opener;
            pos = qpos + opener;
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
                    if !is_triple {
                        pos += 1;
                        col += 1;
                        break;
                    }
                    if pos + 2 < len && chars[pos + 1] == quote && chars[pos + 2] == quote {
                        pos += 3;
                        col += 3;
                        break;
                    }
                    // A lone quote inside a triple is ordinary content.
                    s.push(c);
                    pos += 1;
                    col += 1;
                    continue;
                }
                if c == '\\' && !is_raw {
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
                        'a' => s.push('\u{07}'),
                        'b' => s.push('\u{08}'),
                        'f' => s.push('\u{0C}'),
                        'v' => s.push('\u{0B}'),
                        '\\' => s.push('\\'),
                        '\'' => s.push('\''),
                        '"' => s.push('"'),
                        '0' => s.push('\0'),
                        // `\uXXXX` (4 hex) and `\UXXXXXX` (6 hex).
                        'u' | 'U' => {
                            let want = if esc == 'u' { 4 } else { 6 };
                            let mut cp: u32 = 0;
                            let mut taken = 0;
                            while taken < want
                                && pos + 1 + taken < len
                                && chars[pos + 1 + taken].is_ascii_hexdigit()
                            {
                                cp = cp * 16 + chars[pos + 1 + taken].to_digit(16).unwrap_or(0);
                                taken += 1;
                            }
                            if taken == 0 {
                                return Err(LexError::InvalidEscape { ch: esc, line, col });
                            }
                            // A surrogate half or out-of-range value has no
                            // `char`; U+FFFD keeps the script lexable rather
                            // than failing it all on one bad escape.
                            s.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                            pos += taken;
                            col += taken;
                        }
                        // A newline right after a backslash is a continuation
                        // inside the string: contributes nothing, but the line
                        // counter still has to advance.
                        '\n' => {
                            line += 1;
                            col = 0;
                        }
                        _ => {
                            return Err(LexError::InvalidEscape { ch: esc, line, col });
                        }
                    }
                    pos += 1;
                    col += 1;
                } else if c == '\n' {
                    // Legal in a triple-quoted string; still an error in a
                    // single-quoted one, as in Godot.
                    if !is_triple {
                        return Err(LexError::UnterminatedString {
                            line: start_line,
                            col: start_col,
                        });
                    }
                    s.push(c);
                    pos += 1;
                    line += 1;
                    col = 1;
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

        // Radix-prefixed integers: `0xFF`, `0b1010` (Godot's `number()`,
        // `gdscript_tokenizer.cpp`). Must precede the decimal scanner, which
        // consumes only digits/`.`/`_` and so lexed `0xFF` as `IntLit(0)`
        // followed by the identifier `xFF` — a silently wrong value rather than
        // an error (pat-mgixk.17).
        if ch == '0' && pos + 1 < len && matches!(chars[pos + 1], 'x' | 'X' | 'b' | 'B') {
            let radix = if matches!(chars[pos + 1], 'x' | 'X') {
                16
            } else {
                2
            };
            let start_col = col;
            let mut digits = String::new();
            let mut scan = pos + 2;
            while scan < len
                && (chars[scan] == '_'
                    || if radix == 16 {
                        chars[scan].is_ascii_hexdigit()
                    } else {
                        chars[scan] == '0' || chars[scan] == '1'
                    })
            {
                if chars[scan] != '_' {
                    digits.push(chars[scan]);
                }
                scan += 1;
            }
            // A bare `0x` with no digits is not a number; leave it to the
            // decimal scanner so the `0` still lexes and the stray letter
            // reports itself, rather than pushing IntLit(0) and swallowing it.
            if !digits.is_empty() {
                let val = i64::from_str_radix(&digits, radix).unwrap_or(0);
                col += scan - pos;
                pos = scan;
                tokens.push(TokenSpan {
                    token: Token::IntLit(val),
                    line,
                    col: start_col,
                });
                continue;
            }
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
            // Exponent suffix: `1e3`, `1.5e-3`, `2E+8`. Previously `1e3` lexed
            // as `IntLit(1)` plus the identifier `e3`.
            //
            // ⚠️ Only consumed when at least one digit follows the optional
            // sign. `1.e` or `1e` alone must leave the `e` to the identifier
            // scanner, or a member access like `1.0.exp` would be swallowed.
            if pos < len && matches!(chars[pos], 'e' | 'E') {
                let mut scan = pos + 1;
                if scan < len && (chars[scan] == '+' || chars[scan] == '-') {
                    scan += 1;
                }
                if scan < len && chars[scan].is_ascii_digit() {
                    num_str.push('e');
                    if chars[pos + 1] == '+' || chars[pos + 1] == '-' {
                        num_str.push(chars[pos + 1]);
                    }
                    while scan < len && (chars[scan].is_ascii_digit() || chars[scan] == '_') {
                        if chars[scan] != '_' {
                            num_str.push(chars[scan]);
                        }
                        scan += 1;
                    }
                    col += scan - pos;
                    pos = scan;
                    // An exponent makes it a float even without a '.'.
                    is_float = true;
                }
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
            if ident == "func" && matches!(nest.last(), Some(Nest::Bracket)) {
                pending_lambda = Some(nest.len());
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
                "self" => Token::Self_,
                "super" => Token::Super,
                "class_name" => Token::ClassName,
                "onready" => Token::Onready,
                "export" => Token::Export,
                "true" => Token::BoolLit(true),
                "false" => Token::BoolLit(false),
                "null" => Token::Null,
                "and" => Token::And,
                "or" => Token::Or,
                "not" => Token::Not,
                "await" => Token::Await,
                "is" => Token::Is,
                "as" => Token::As,
                "when" => Token::When,
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
        // Three-character operators first (maximal munch): `**=`, `<<=`, `>>=`.
        if pos + 2 < len {
            let three: String = chars[pos..pos + 3].iter().collect();
            let three_tok = match three.as_str() {
                "**=" => Some(Token::StarStarAssign),
                "<<=" => Some(Token::ShlAssign),
                ">>=" => Some(Token::ShrAssign),
                _ => None,
            };
            if let Some(tok) = three_tok {
                tokens.push(TokenSpan {
                    token: tok,
                    line,
                    col: start_col,
                });
                pos += 3;
                col += 3;
                continue;
            }
        }
        if pos + 1 < len {
            let two: String = chars[pos..pos + 2].iter().collect();
            // Two-character operators with a single token mapping.
            let two_tok = match two.as_str() {
                "**" => Some(Token::StarStar),
                "*=" => Some(Token::StarAssign),
                "/=" => Some(Token::SlashAssign),
                "%=" => Some(Token::PercentAssign),
                "&=" => Some(Token::AmpAssign),
                "|=" => Some(Token::PipeAssign),
                "^=" => Some(Token::CaretAssign),
                "<<" => Some(Token::Shl),
                ">>" => Some(Token::Shr),
                ".." => Some(Token::DotDot),
                // Godot emits distinct AMPERSAND_AMPERSAND / PIPE_PIPE tokens,
                // but its parser maps them to the same precedence rules as the
                // `and` / `or` keywords (`gdscript_parser.cpp`: PREC_LOGIC_AND /
                // PREC_LOGIC_OR). Mapping them onto the existing keyword tokens
                // here reuses the parser and type-checker arms that already
                // handle `and`/`or`, rather than adding variants every
                // exhaustive match on `Token` would have to grow.
                //
                // ⚠️ Must stay AHEAD of the single-char table, or `a && b`
                // lexes as two `Amp`s and dies in `parse_primary`'s
                // `&"StringName"` arm.
                "&&" => Some(Token::And),
                "||" => Some(Token::Or),
                _ => None,
            };
            if let Some(tok) = two_tok {
                tokens.push(TokenSpan {
                    token: tok,
                    line,
                    col: start_col,
                });
                pos += 2;
                col += 2;
                continue;
            }
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
            '&' => Token::Amp,
            '|' => Token::Pipe,
            '^' => Token::Caret,
            '~' => Token::Tilde,
            // `!=` is consumed by the two-char table above, so a `!` reaching
            // here is the unary logical not. Mapped onto the `not` keyword's
            // token for the same reason as `&&`/`||`. Before this it fell
            // through to `UnexpectedChar`, so `if !ready:` was a lex error.
            '!' => Token::Not,
            '<' => Token::Lt,
            '>' => Token::Gt,
            '=' => Token::Assign,
            // The bracket counters drive the Newline/Indent suppression above.
            // `)`/`]`/`}` saturate at zero so a stray closer cannot drive the
            // depth negative and silently re-enable suppression later.
            '(' => {
                nest.push(Nest::Bracket);
                Token::LParen
            }
            ')' => {
                close_bracket(&mut nest, &mut indent_stack, &mut tokens, line, col);
                if pending_lambda.is_some_and(|d| d > nest.len()) {
                    pending_lambda = None;
                }
                Token::RParen
            }
            '[' => {
                nest.push(Nest::Bracket);
                Token::LBracket
            }
            ']' => {
                close_bracket(&mut nest, &mut indent_stack, &mut tokens, line, col);
                if pending_lambda.is_some_and(|d| d > nest.len()) {
                    pending_lambda = None;
                }
                Token::RBracket
            }
            '{' => {
                nest.push(Nest::Bracket);
                Token::LBrace
            }
            '}' => {
                close_bracket(&mut nest, &mut indent_stack, &mut tokens, line, col);
                if pending_lambda.is_some_and(|d| d > nest.len()) {
                    pending_lambda = None;
                }
                Token::RBrace
            }
            ':' => {
                if pending_lambda == Some(nest.len()) && matches!(nest.last(), Some(Nest::Bracket))
                {
                    pending_lambda = None;
                    let mut peek = pos + 1;
                    while peek < len && (chars[peek] == ' ' || chars[peek] == '\t') {
                        peek += 1;
                    }
                    let single_line = !(peek >= len
                        || chars[peek] == '\n'
                        || chars[peek] == '\r'
                        || chars[peek] == '#');
                    nest.push(Nest::Lambda {
                        saved_indent: indent_stack.clone(),
                        body_indent: None,
                        single_line,
                    });
                }
                Token::Colon
            }
            ',' => {
                // `call(func(x): return x, 2)`: the comma ends a one-line body.
                if matches!(
                    nest.last(),
                    Some(Nest::Lambda {
                        single_line: true,
                        ..
                    })
                ) {
                    nest.pop();
                }
                Token::Comma
            }
            '.' => Token::Dot,
            ';' => Token::Semicolon,
            '@' => Token::AtSign,
            '$' => Token::Dollar,
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
        let err = tokenize("`").unwrap_err();
        assert!(matches!(err, LexError::UnexpectedChar { ch: '`', .. }));
    }

    #[test]
    fn tokenize_at_sign() {
        let tokens = tok_types("@export");
        assert_eq!(tokens, vec![Token::AtSign, Token::Export, Token::Eof]);
    }

    #[test]
    fn tokenize_self_and_super() {
        let tokens = tok_types("self super");
        assert_eq!(tokens, vec![Token::Self_, Token::Super, Token::Eof]);
    }

    #[test]
    fn tokenize_class_name_keyword() {
        let tokens = tok_types("class_name MyClass");
        assert_eq!(
            tokens,
            vec![Token::ClassName, Token::Ident("MyClass".into()), Token::Eof]
        );
    }

    #[test]
    fn tokenize_bitwise_and_compound_operators() {
        let tokens = tok_types("& | ^ ~ << >> ** *= /= %= **= &= |= ^= <<= >>=");
        assert_eq!(
            tokens,
            vec![
                Token::Amp,
                Token::Pipe,
                Token::Caret,
                Token::Tilde,
                Token::Shl,
                Token::Shr,
                Token::StarStar,
                Token::StarAssign,
                Token::SlashAssign,
                Token::PercentAssign,
                Token::StarStarAssign,
                Token::AmpAssign,
                Token::PipeAssign,
                Token::CaretAssign,
                Token::ShlAssign,
                Token::ShrAssign,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenize_is_as_when_keywords() {
        let tokens = tok_types("x is int as float when");
        assert_eq!(
            tokens,
            vec![
                Token::Ident("x".into()),
                Token::Is,
                Token::Ident("int".into()),
                Token::As,
                Token::Ident("float".into()),
                Token::When,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn tokenize_maximal_munch_star_star() {
        // `**` must not be split into two `*`, and `**=` must win over `**`.
        assert_eq!(
            tok_types("2**3"),
            vec![
                Token::IntLit(2),
                Token::StarStar,
                Token::IntLit(3),
                Token::Eof
            ]
        );
        assert_eq!(
            tok_types("x**=2"),
            vec![
                Token::Ident("x".into()),
                Token::StarStarAssign,
                Token::IntLit(2),
                Token::Eof
            ]
        );
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
