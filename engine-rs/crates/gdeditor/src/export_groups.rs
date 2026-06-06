//! `@export_group` / `@export_subgroup` nesting and collapsing for the inspector.
//!
//! Godot groups script-exported properties under collapsible headers: an
//! `@export_group("Name")` annotation starts a top-level group that the
//! following exports nest under, and `@export_subgroup("Name")` nests a
//! subgroup beneath the current group. Collapsing a group (or subgroup) hides
//! its member properties in the inspector. This module parses that structure
//! and computes which properties are visible given the collapse state.

/// A subgroup beneath an export group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subgroup {
    /// The subgroup name (from `@export_subgroup("name")`).
    pub name: String,
    /// Whether the subgroup is collapsed (members hidden).
    pub collapsed: bool,
    /// Exported properties directly under this subgroup, in declaration order.
    pub properties: Vec<String>,
}

/// A top-level export group with optional nested subgroups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// The group name (from `@export_group("name")`).
    pub name: String,
    /// Whether the group is collapsed (all members, including subgroups, hidden).
    pub collapsed: bool,
    /// Exported properties directly under this group (before any subgroup).
    pub properties: Vec<String>,
    /// Nested subgroups, in declaration order.
    pub subgroups: Vec<Subgroup>,
}

/// The parsed export-group layout of a script.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportGroupLayout {
    /// Exported properties declared before any group.
    pub ungrouped: Vec<String>,
    /// Top-level groups, in declaration order.
    pub groups: Vec<Group>,
}

impl ExportGroupLayout {
    /// Parses a GDScript source into its export-group layout.
    pub fn parse(source: &str) -> Self {
        let mut layout = ExportGroupLayout::default();
        // Index of the current group / subgroup the next export nests under.
        let mut current_group: Option<usize> = None;
        let mut current_subgroup: Option<usize> = None;
        let mut pending_export = false;

        for raw_line in source.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some(name) = annotation_string_arg(line, "@export_group") {
                layout.groups.push(Group {
                    name,
                    collapsed: false,
                    properties: Vec::new(),
                    subgroups: Vec::new(),
                });
                current_group = Some(layout.groups.len() - 1);
                current_subgroup = None;
                pending_export = false;
                continue;
            }

            if let Some(name) = annotation_string_arg(line, "@export_subgroup") {
                if let Some(gi) = current_group {
                    layout.groups[gi].subgroups.push(Subgroup {
                        name,
                        collapsed: false,
                        properties: Vec::new(),
                    });
                    current_subgroup = Some(layout.groups[gi].subgroups.len() - 1);
                }
                pending_export = false;
                continue;
            }

            // `@export_category` is a separate feature (top-level divider); a
            // following export does not belong to it, so just reset state.
            if line.starts_with("@export_category") {
                pending_export = false;
                continue;
            }

            if line.starts_with("@export") {
                if let Some(name) = export_var_name(line) {
                    push_property(&mut layout, current_group, current_subgroup, name);
                    pending_export = false;
                } else {
                    pending_export = true;
                }
                continue;
            }

            if pending_export {
                if let Some(name) = var_name(line) {
                    push_property(&mut layout, current_group, current_subgroup, name);
                }
                pending_export = false;
            }
        }

        layout
    }

    /// Toggles the collapsed state of the named top-level group. Returns the new
    /// collapsed state, or `None` if no such group exists.
    pub fn toggle_group(&mut self, name: &str) -> Option<bool> {
        let group = self.groups.iter_mut().find(|g| g.name == name)?;
        group.collapsed = !group.collapsed;
        Some(group.collapsed)
    }

    /// Toggles the collapsed state of `subgroup` under `group`. Returns the new
    /// collapsed state, or `None` if not found.
    pub fn toggle_subgroup(&mut self, group: &str, subgroup: &str) -> Option<bool> {
        let g = self.groups.iter_mut().find(|g| g.name == group)?;
        let sg = g.subgroups.iter_mut().find(|s| s.name == subgroup)?;
        sg.collapsed = !sg.collapsed;
        Some(sg.collapsed)
    }

    /// The properties currently visible in the inspector, honoring collapse
    /// state: a collapsed group hides all its members (including its subgroups),
    /// and a collapsed subgroup hides its own members.
    pub fn visible_properties(&self) -> Vec<String> {
        let mut visible = self.ungrouped.clone();
        for group in &self.groups {
            if group.collapsed {
                continue;
            }
            visible.extend(group.properties.iter().cloned());
            for subgroup in &group.subgroups {
                if subgroup.collapsed {
                    continue;
                }
                visible.extend(subgroup.properties.iter().cloned());
            }
        }
        visible
    }
}

/// Adds `name` to the current subgroup, group, or the ungrouped list.
fn push_property(
    layout: &mut ExportGroupLayout,
    current_group: Option<usize>,
    current_subgroup: Option<usize>,
    name: String,
) {
    match (current_group, current_subgroup) {
        (Some(gi), Some(si)) => layout.groups[gi].subgroups[si].properties.push(name),
        (Some(gi), None) => layout.groups[gi].properties.push(name),
        (None, _) => layout.ungrouped.push(name),
    }
}

/// Extracts the first double-quoted argument from `<annotation>(...)`. Returns
/// `None` if `line` does not start with `annotation` (as a whole token) or has
/// no quoted argument.
fn annotation_string_arg(line: &str, annotation: &str) -> Option<String> {
    let rest = line.strip_prefix(annotation)?;
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
    parse_identifier(line.strip_prefix("var ")?)
}

/// Parses a leading identifier (letters, digits, `_`) from `s`.
fn parse_identifier(s: &str) -> Option<String> {
    let ident: String = s
        .trim_start()
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

    /// Acceptance (pat-z8ve3): a script with an export group and subgroup
    /// renders nested collapsible headers, and toggling a group hides its
    /// members.
    #[test]
    fn inspector_export_groups_nest_and_collapse() {
        let script = r#"
@export var top: int = 0
@export_group("Stats")
@export var hp: int = 100
@export var mp: int = 50
@export_subgroup("Regen")
@export var hp_regen: float = 1.0
@export_group("Combat")
@export var attack: int = 10
"#;

        let mut layout = ExportGroupLayout::parse(script);

        // Property before any group stays ungrouped.
        assert_eq!(layout.ungrouped, vec!["top".to_string()]);

        // Two top-level groups, with the subgroup nested under the first.
        assert_eq!(layout.groups.len(), 2);
        let stats = &layout.groups[0];
        assert_eq!(stats.name, "Stats");
        assert_eq!(stats.properties, vec!["hp".to_string(), "mp".to_string()]);
        assert_eq!(stats.subgroups.len(), 1);
        assert_eq!(stats.subgroups[0].name, "Regen");
        assert_eq!(
            stats.subgroups[0].properties,
            vec!["hp_regen".to_string()]
        );
        assert_eq!(layout.groups[1].name, "Combat");
        assert_eq!(layout.groups[1].properties, vec!["attack".to_string()]);

        // Everything visible initially.
        let visible = layout.visible_properties();
        assert!(visible.contains(&"hp".to_string()));
        assert!(visible.contains(&"hp_regen".to_string()));

        // Collapsing the Stats group hides its direct and subgroup members.
        assert_eq!(layout.toggle_group("Stats"), Some(true));
        let visible = layout.visible_properties();
        assert!(
            !visible.contains(&"hp".to_string()),
            "collapsed group hides direct members"
        );
        assert!(
            !visible.contains(&"hp_regen".to_string()),
            "collapsed group hides subgroup members"
        );
        assert!(
            visible.contains(&"top".to_string()),
            "ungrouped properties stay visible"
        );
        assert!(
            visible.contains(&"attack".to_string()),
            "other groups stay visible"
        );

        // Expanding it again restores visibility.
        assert_eq!(layout.toggle_group("Stats"), Some(false));
        assert!(layout.visible_properties().contains(&"hp".to_string()));
    }

    #[test]
    fn subgroup_collapse_hides_only_subgroup_members() {
        let script = r#"
@export_group("Stats")
@export var hp: int = 1
@export_subgroup("Regen")
@export var hp_regen: float = 1.0
"#;
        let mut layout = ExportGroupLayout::parse(script);

        assert_eq!(layout.toggle_subgroup("Stats", "Regen"), Some(true));
        let visible = layout.visible_properties();
        assert!(visible.contains(&"hp".to_string()), "group member stays visible");
        assert!(
            !visible.contains(&"hp_regen".to_string()),
            "collapsed subgroup hides its own members"
        );
    }
}
