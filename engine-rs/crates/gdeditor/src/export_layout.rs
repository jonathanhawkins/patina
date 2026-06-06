//! Inspector layout for script-declared `@export` properties, including
//! `@export_category` dividers.
//!
//! Godot's `@export_category("Name")` annotation inserts a labeled top-level
//! divider into the inspector at the point it is declared, separating the
//! exports that follow it. This module parses a GDScript's export declarations
//! in source order and produces the ordered list of inspector items
//! (category dividers interleaved with exported properties) the inspector
//! renders.

/// An item in the inspector's script-export layout, in declaration order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportLayoutItem {
    /// A labeled category divider from `@export_category("name")`.
    Category(String),
    /// An exported property declared with `@export var <name>`.
    Property(String),
}

/// Parses a GDScript source's export layout: each `@export_category("name")`
/// becomes a [`ExportLayoutItem::Category`] divider at its declared position,
/// and each exported `var` becomes a [`ExportLayoutItem::Property`], preserving
/// source order. Non-exported declarations and `@export_group`/`@export_subgroup`
/// markers are ignored (groups are a separate feature).
pub fn parse_export_layout(source: &str) -> Vec<ExportLayoutItem> {
    let mut items = Vec::new();
    // Tracks a bare `@export` / `@export_range(...)` annotation whose `var`
    // appears on the following line.
    let mut pending_export = false;

    for raw_line in source.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // `@export_category("Label")` → a divider.
        if let Some(label) = annotation_string_arg(line, "@export_category") {
            items.push(ExportLayoutItem::Category(label));
            pending_export = false;
            continue;
        }

        // `@export_group` / `@export_subgroup` are a separate feature; skip them
        // (and don't let a following `var` be mistaken for a loose export).
        if line.starts_with("@export_group") || line.starts_with("@export_subgroup") {
            pending_export = false;
            continue;
        }

        // An `@export ...` annotation line.
        if line.starts_with("@export") {
            if let Some(name) = export_var_name(line) {
                // `@export var name ...` on a single line.
                items.push(ExportLayoutItem::Property(name));
                pending_export = false;
            } else {
                // Bare `@export` / `@export_range(...)`; the `var` is on the
                // next line.
                pending_export = true;
            }
            continue;
        }

        // A `var name` line directly following a bare `@export` annotation.
        if pending_export {
            if let Some(name) = var_name(line) {
                items.push(ExportLayoutItem::Property(name));
            }
            pending_export = false;
        }
    }

    items
}

/// Extracts the first double-quoted argument from `<annotation>(...)`, e.g.
/// `@export_category("Combat")` → `Combat`. Returns `None` if `line` does not
/// start with `annotation` or has no quoted argument.
fn annotation_string_arg(line: &str, annotation: &str) -> Option<String> {
    let rest = line.strip_prefix(annotation)?;
    // Guard against prefixes like `@export_categoryX`: the next char must not
    // be an identifier character.
    if let Some(c) = rest.chars().next() {
        if c.is_alphanumeric() || c == '_' {
            return None;
        }
    }
    let open = rest.find('"')? + 1;
    let close = rest[open..].find('"')? + open;
    Some(rest[open..close].to_string())
}

/// Extracts the variable name from an inline `@export ... var <name>` line.
fn export_var_name(line: &str) -> Option<String> {
    let idx = line.find(" var ")?;
    parse_identifier(&line[idx + " var ".len()..])
}

/// Extracts the variable name from a `var <name>` line.
fn var_name(line: &str) -> Option<String> {
    let rest = line.strip_prefix("var ")?;
    parse_identifier(rest)
}

/// Parses a leading identifier (letters, digits, `_`) from `s`.
fn parse_identifier(s: &str) -> Option<String> {
    let trimmed = s.trim_start();
    let ident: String = trimmed
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if ident.is_empty() {
        None
    } else {
        Some(ident)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-8sm1n): a script using `@export_category` renders a
    /// labeled divider at the declared position, separating the exports that
    /// follow it.
    #[test]
    fn inspector_export_category_divider_renders() {
        let script = r#"
extends Node2D

@export var health: int = 100
@export_category("Combat")
@export var attack: int = 10
@export var defense: int = 5
@export_category("Movement")
@export var speed: float = 3.0
"#;

        let layout = parse_export_layout(script);

        assert_eq!(
            layout,
            vec![
                ExportLayoutItem::Property("health".to_string()),
                ExportLayoutItem::Category("Combat".to_string()),
                ExportLayoutItem::Property("attack".to_string()),
                ExportLayoutItem::Property("defense".to_string()),
                ExportLayoutItem::Category("Movement".to_string()),
                ExportLayoutItem::Property("speed".to_string()),
            ],
            "category dividers appear at their declared positions among exports"
        );

        // The "Combat" divider separates `health` from `attack`/`defense`.
        let combat = layout
            .iter()
            .position(|i| *i == ExportLayoutItem::Category("Combat".to_string()))
            .unwrap();
        assert_eq!(
            layout[combat - 1],
            ExportLayoutItem::Property("health".to_string())
        );
        assert_eq!(
            layout[combat + 1],
            ExportLayoutItem::Property("attack".to_string())
        );
    }

    #[test]
    fn export_layout_handles_bare_export_and_ignores_non_exports() {
        let script = r#"
@export_category("Stats")
@export
var hp: int = 10
var hidden := 5
@export var mana: int = 3
"#;

        let layout = parse_export_layout(script);

        // `@export` on its own line applies to the following `var`; the
        // non-exported `var hidden` is ignored.
        assert_eq!(
            layout,
            vec![
                ExportLayoutItem::Category("Stats".to_string()),
                ExportLayoutItem::Property("hp".to_string()),
                ExportLayoutItem::Property("mana".to_string()),
            ]
        );
    }

    #[test]
    fn export_layout_empty_without_exports() {
        assert!(parse_export_layout("extends Node\nvar x = 1\n").is_empty());
    }
}
