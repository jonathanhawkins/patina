//! Shader preprocessor — the pass Godot runs before parsing (pat-0q2kx.222).
//!
//! Godot ground truth: `servers/rendering/shader_preprocessor.cpp`.
//!
//! Patina had none: `#include` was reported as an unsupported feature and the
//! compiler treated every other directive as a junk line it silently skipped.
//! "Silently" is the problem — a shader guarded by `#ifdef` compiled with *both*
//! branches spliced together, and one behind `#ifndef` compiled with code that
//! should have been excluded.
//!
//! # Supported
//!
//! `#define NAME [value]` (object-like), `#undef`, `#ifdef` / `#ifndef` /
//! `#else` / `#endif` (nested), and `#include "path"` with cycle detection.
//! Output carries a line map so downstream diagnostics can name the original
//! file and line rather than a position in the spliced text.
//!
//! # Not yet supported, and deliberately passed through rather than guessed
//!
//! Function-like macros (`#define F(x)`), `#if` expressions with `defined()`,
//! `#elif`, `#pragma disable_preprocessor`, and `__FILE__` / `__LINE__`.
//! Unrecognised directives are emitted unchanged so that `#version` and
//! `#pragma` reach the compiler intact; treating them as errors would break
//! shaders that work today.

use std::collections::HashMap;

/// A preprocessing failure, located in the file that actually contains it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreprocessError {
    /// Include name the error occurred in; the root source's given name for
    /// top-level errors.
    pub file: String,
    /// 1-based line within `file`.
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for PreprocessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.file, self.line, self.message)
    }
}

impl std::error::Error for PreprocessError {}

/// Preprocessed source plus the mapping back to where each line came from.
#[derive(Debug, Clone, Default)]
pub struct Preprocessed {
    pub code: String,
    /// One entry per line of `code`: the originating file and its 1-based line.
    pub line_map: Vec<(String, usize)>,
}

impl Preprocessed {
    /// Where output line `line` (1-based) came from.
    ///
    /// This is the whole point of the line map: after an include is spliced in,
    /// a compiler error at output line 40 may belong to line 3 of an include,
    /// and reporting "line 40" of a file the author never wrote is useless.
    pub fn origin(&self, line: usize) -> Option<(&str, usize)> {
        line.checked_sub(1)
            .and_then(|i| self.line_map.get(i))
            .map(|(f, l)| (f.as_str(), *l))
    }
}

/// One `#ifdef`/`#ifndef` frame.
struct Cond {
    /// Whether the enclosing scope was emitting. `#else` can only enable a
    /// branch if its parent was already live.
    parent_emitting: bool,
    /// Whether any branch of this conditional has already been taken.
    taken: bool,
    emitting: bool,
}

struct Ctx<'a> {
    defines: HashMap<String, String>,
    out: String,
    line_map: Vec<(String, usize)>,
    /// Include names currently being expanded, for cycle detection.
    open: Vec<String>,
    resolve: &'a mut dyn FnMut(&str) -> Result<String, String>,
}

/// Expands `source` into a single translation unit.
///
/// `root_name` labels the top-level source in errors and the line map.
/// `resolve` maps an include path to its text; returning `Err` yields a
/// located [`PreprocessError`] rather than a panic.
pub fn preprocess(
    source: &str,
    root_name: &str,
    resolve: &mut dyn FnMut(&str) -> Result<String, String>,
) -> Result<Preprocessed, PreprocessError> {
    let mut ctx = Ctx {
        defines: HashMap::new(),
        out: String::new(),
        line_map: Vec::new(),
        open: Vec::new(),
        resolve,
    };
    ctx.run(source, root_name)?;
    Ok(Preprocessed {
        code: ctx.out,
        line_map: ctx.line_map,
    })
}

impl Ctx<'_> {
    fn run(&mut self, source: &str, file: &str) -> Result<(), PreprocessError> {
        if self.open.iter().any(|f| f == file) {
            // Report at the include site's own file so the chain is readable.
            return Err(PreprocessError {
                file: file.to_string(),
                line: 1,
                message: format!(
                    "cyclic #include: {} already being expanded ({})",
                    file,
                    self.open.join(" -> ")
                ),
            });
        }
        self.open.push(file.to_string());

        let mut conds: Vec<Cond> = Vec::new();
        for (idx, raw) in source.lines().enumerate() {
            let lineno = idx + 1;
            let trimmed = raw.trim_start();

            if let Some(rest) = trimmed.strip_prefix('#') {
                let rest = rest.trim();
                let (word, arg) = split_directive(rest);
                match word {
                    "ifdef" | "ifndef" => {
                        let name = arg.trim();
                        let defined = self.defines.contains_key(name);
                        let want = if word == "ifdef" { defined } else { !defined };
                        let parent = emitting(&conds);
                        conds.push(Cond {
                            parent_emitting: parent,
                            taken: parent && want,
                            emitting: parent && want,
                        });
                    }
                    "else" => match conds.last_mut() {
                        Some(c) => {
                            c.emitting = c.parent_emitting && !c.taken;
                            c.taken = c.taken || c.emitting;
                        }
                        None => {
                            return Err(self.err(file, lineno, "#else without #ifdef"));
                        }
                    },
                    "endif" => {
                        if conds.pop().is_none() {
                            return Err(self.err(file, lineno, "#endif without #ifdef"));
                        }
                    }
                    "define" if emitting(&conds) => {
                        let (name, value) = split_directive(arg);
                        if name.is_empty() {
                            return Err(self.err(file, lineno, "#define needs a name"));
                        }
                        self.defines
                            .insert(name.to_string(), value.trim().to_string());
                    }
                    "undef" if emitting(&conds) => {
                        self.defines.remove(arg.trim());
                    }
                    "include" if emitting(&conds) => {
                        let path = parse_include(arg).ok_or_else(|| {
                            self.err(file, lineno, "#include expects a quoted path")
                        })?;
                        let text = (self.resolve)(&path).map_err(|e| PreprocessError {
                            file: file.to_string(),
                            line: lineno,
                            message: format!("cannot resolve #include \"{path}\": {e}"),
                        })?;
                        self.run(&text, &path)?;
                    }
                    // `define`/`undef`/`include` inside a dead branch fall here
                    // and are correctly skipped; anything unrecognised is passed
                    // through so `#version` and `#pragma` survive.
                    "define" | "undef" | "include" => {}
                    _ => {
                        if emitting(&conds) {
                            self.emit(raw, file, lineno);
                        }
                    }
                }
                continue;
            }

            if emitting(&conds) {
                let expanded = self.substitute(raw);
                self.emit(&expanded, file, lineno);
            }
        }

        if !conds.is_empty() {
            let line = source.lines().count().max(1);
            return Err(self.err(file, line, "unterminated #ifdef (missing #endif)"));
        }

        self.open.pop();
        Ok(())
    }

    fn err(&self, file: &str, line: usize, message: &str) -> PreprocessError {
        PreprocessError {
            file: file.to_string(),
            line,
            message: message.to_string(),
        }
    }

    fn emit(&mut self, text: &str, file: &str, line: usize) {
        self.out.push_str(text);
        self.out.push('\n');
        self.line_map.push((file.to_string(), line));
    }

    /// Replaces defined identifiers, re-scanning so a macro whose body names
    /// another macro resolves. Bounded, so a self-referential pair cannot hang
    /// the compile — Godot's own preprocessor caps expansion too.
    fn substitute(&self, line: &str) -> String {
        let mut cur = line.to_string();
        for _ in 0..8 {
            let next = substitute_once(&cur, &self.defines);
            if next == cur {
                return cur;
            }
            cur = next;
        }
        cur
    }
}

fn emitting(conds: &[Cond]) -> bool {
    conds.last().map(|c| c.emitting).unwrap_or(true)
}

/// Splits `"define FOO 1"` into `("define", "FOO 1")`.
fn split_directive(s: &str) -> (&str, &str) {
    let s = s.trim();
    match s.find(char::is_whitespace) {
        Some(i) => (&s[..i], s[i..].trim_start()),
        None => (s, ""),
    }
}

/// Extracts the path from `"foo.gdshaderinc"` or `<foo.gdshaderinc>`.
fn parse_include(arg: &str) -> Option<String> {
    let a = arg.trim();
    let (open, close) = match a.chars().next()? {
        '"' => ('"', '"'),
        '<' => ('<', '>'),
        _ => return None,
    };
    let rest = a.strip_prefix(open)?;
    let end = rest.find(close)?;
    Some(rest[..end].to_string())
}

/// One substitution pass, replacing whole identifiers only.
///
/// Whole-identifier matching is what stops `#define A 1` from corrupting
/// `MAX_A` or `ALBEDO` — a plain string replace would.
fn substitute_once(line: &str, defines: &HashMap<String, String>) -> String {
    if defines.is_empty() {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len());
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if is_ident_start(chars[i]) {
            let start = i;
            while i < chars.len() && is_ident_char(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            match defines.get(&word) {
                Some(v) => out.push_str(v),
                None => out.push_str(&word),
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}
