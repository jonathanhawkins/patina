//! Inline math-expression evaluation for numeric inspector fields.
//!
//! Mirrors Godot's inspector behaviour where a numeric field accepts a small
//! math expression (e.g. `2*PI`, `1+1`, `(3+4)/2`) and evaluates it to a
//! number when the edit is committed. An invalid expression is rejected so the
//! field keeps its previous value rather than being clobbered with garbage.

use gdscene::node::NodeId;
use gdscene::SceneTree;
use gdvariant::Variant;

/// A token in an inline numeric math expression.
#[derive(Debug, Clone, PartialEq)]
enum ExprToken {
    Number(f64),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
    Ident(String),
}

/// Tokenizes an inline numeric expression. Returns `None` on an unrecognized
/// character or a malformed numeric literal.
fn tokenize_expression(input: &str) -> Option<Vec<ExprToken>> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\n' | '\r' => i += 1,
            '+' => {
                tokens.push(ExprToken::Plus);
                i += 1;
            }
            '-' => {
                tokens.push(ExprToken::Minus);
                i += 1;
            }
            '*' => {
                tokens.push(ExprToken::Star);
                i += 1;
            }
            '/' => {
                tokens.push(ExprToken::Slash);
                i += 1;
            }
            '(' => {
                tokens.push(ExprToken::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(ExprToken::RParen);
                i += 1;
            }
            _ if c.is_ascii_digit() || c == '.' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let literal: String = chars[start..i].iter().collect();
                let value: f64 = literal.parse().ok()?;
                tokens.push(ExprToken::Number(value));
            }
            _ if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let ident: String = chars[start..i].iter().collect();
                tokens.push(ExprToken::Ident(ident));
            }
            _ => return None,
        }
    }
    Some(tokens)
}

/// Resolves a named numeric constant (Godot's inspector expression constants).
fn constant_value(name: &str) -> Option<f64> {
    match name.to_ascii_uppercase().as_str() {
        "PI" => Some(std::f64::consts::PI),
        "TAU" => Some(std::f64::consts::TAU),
        "E" => Some(std::f64::consts::E),
        "INF" => Some(f64::INFINITY),
        _ => None,
    }
}

/// A recursive-descent parser/evaluator over the token stream.
struct ExprParser {
    tokens: Vec<ExprToken>,
    pos: usize,
}

impl ExprParser {
    fn peek(&self) -> Option<&ExprToken> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<ExprToken> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    /// `expr := term (('+' | '-') term)*`
    fn parse_expr(&mut self) -> Option<f64> {
        let mut value = self.parse_term()?;
        while let Some(op) = self.peek() {
            match op {
                ExprToken::Plus => {
                    self.advance();
                    value += self.parse_term()?;
                }
                ExprToken::Minus => {
                    self.advance();
                    value -= self.parse_term()?;
                }
                _ => break,
            }
        }
        Some(value)
    }

    /// `term := factor (('*' | '/') factor)*`
    fn parse_term(&mut self) -> Option<f64> {
        let mut value = self.parse_factor()?;
        while let Some(op) = self.peek() {
            match op {
                ExprToken::Star => {
                    self.advance();
                    value *= self.parse_factor()?;
                }
                ExprToken::Slash => {
                    self.advance();
                    value /= self.parse_factor()?;
                }
                _ => break,
            }
        }
        Some(value)
    }

    /// `factor := ('+' | '-') factor | '(' expr ')' | number | ident`
    fn parse_factor(&mut self) -> Option<f64> {
        match self.advance()? {
            ExprToken::Number(n) => Some(n),
            ExprToken::Minus => Some(-self.parse_factor()?),
            ExprToken::Plus => self.parse_factor(),
            ExprToken::LParen => {
                let value = self.parse_expr()?;
                match self.advance()? {
                    ExprToken::RParen => Some(value),
                    _ => None,
                }
            }
            ExprToken::Ident(name) => constant_value(&name),
            _ => None,
        }
    }
}

/// Evaluates an inline numeric math expression entered into a numeric inspector
/// field. Supports `+ - * /`, unary `+`/`-`, parentheses, integer/decimal
/// literals, and the named constants `PI`, `TAU`, `E`, and `INF`.
///
/// Returns `None` for a syntactically invalid or unparseable expression so the
/// caller can reject the edit without mutating the field.
pub fn eval_numeric_expression(input: &str) -> Option<f64> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let tokens = tokenize_expression(trimmed)?;
    if tokens.is_empty() {
        return None;
    }
    let mut parser = ExprParser { tokens, pos: 0 };
    let value = parser.parse_expr()?;
    // Reject leftover tokens (e.g. `1 2`, `1+`, `1)`).
    if parser.pos != parser.tokens.len() {
        return None;
    }
    Some(value)
}

/// Commits a numeric inspector field edit entered as text: the text is
/// evaluated as a math expression and, on success, the computed number is
/// written back to `property` on `node_id` — truncated to an integer when the
/// field currently holds an `Int`, otherwise stored as a `Float`.
///
/// An invalid expression is rejected and the field is left unchanged. Returns
/// the committed [`Variant`], or `None` if the expression was rejected or the
/// node is missing.
pub fn commit_numeric_expression(
    tree: &mut SceneTree,
    node_id: NodeId,
    property: &str,
    input: &str,
) -> Option<Variant> {
    let result = eval_numeric_expression(input)?;
    let node = tree.get_node_mut(node_id)?;
    let value = match node.get_property(property) {
        Variant::Int(_) => Variant::Int(result as i64),
        _ => Variant::Float(result),
    };
    node.set_property(property, value.clone());
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdscene::node::Node;

    #[test]
    fn eval_handles_arithmetic_constants_and_invalid() {
        assert_eq!(eval_numeric_expression("1+1"), Some(2.0));
        assert_eq!(eval_numeric_expression("2 * 3 - 4"), Some(2.0));
        assert_eq!(eval_numeric_expression("(3 + 4) / 2"), Some(3.5));
        assert_eq!(eval_numeric_expression("-5"), Some(-5.0));
        assert_eq!(
            eval_numeric_expression("2*PI"),
            Some(2.0 * std::f64::consts::PI)
        );

        // Invalid expressions evaluate to None.
        assert_eq!(eval_numeric_expression("2*"), None);
        assert_eq!(eval_numeric_expression("abc"), None);
        assert_eq!(eval_numeric_expression("1 2"), None);
        assert_eq!(eval_numeric_expression("(1+2"), None);
        assert_eq!(eval_numeric_expression(""), None);
        assert_eq!(eval_numeric_expression("3 !"), None);
    }

    /// Acceptance (pat-0yt5y): entering an expression like `2*PI` or `1+1`
    /// into a numeric field evaluates to the computed number on commit, and an
    /// invalid expression is rejected without mutating the value.
    #[test]
    fn inspector_numeric_inline_expression() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let id = tree.add_child(root, Node::new("Obj", "Node2D")).unwrap();

        // Seed a float numeric field.
        tree.get_node_mut(id)
            .unwrap()
            .set_property("value", Variant::Float(0.0));

        // `1+1` commits to 2.0.
        assert_eq!(
            commit_numeric_expression(&mut tree, id, "value", "1+1"),
            Some(Variant::Float(2.0))
        );
        assert_eq!(
            tree.get_node(id).unwrap().get_property("value"),
            Variant::Float(2.0)
        );

        // `2*PI` commits to the computed number.
        match commit_numeric_expression(&mut tree, id, "value", "2*PI") {
            Some(Variant::Float(v)) => {
                assert!((v - 2.0 * std::f64::consts::PI).abs() < 1e-9)
            }
            other => panic!("expected a float commit, got {other:?}"),
        }

        // An invalid expression is rejected and leaves the value untouched.
        let before = tree.get_node(id).unwrap().get_property("value");
        assert_eq!(
            commit_numeric_expression(&mut tree, id, "value", "2*"),
            None
        );
        assert_eq!(
            commit_numeric_expression(&mut tree, id, "value", "abc!"),
            None
        );
        assert_eq!(
            tree.get_node(id).unwrap().get_property("value"),
            before,
            "a rejected expression must not mutate the field"
        );

        // Integer fields truncate the evaluated result to an integer.
        tree.get_node_mut(id)
            .unwrap()
            .set_property("count", Variant::Int(0));
        assert_eq!(
            commit_numeric_expression(&mut tree, id, "count", "3*2"),
            Some(Variant::Int(6))
        );
        assert_eq!(
            tree.get_node(id).unwrap().get_property("count"),
            Variant::Int(6)
        );
    }
}
