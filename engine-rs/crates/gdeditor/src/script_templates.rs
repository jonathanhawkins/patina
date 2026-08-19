//! Script templates for the Create-Script flow (pat-s3rrv.63).
//!
//! Godot's Create Script dialog offers a Template dropdown populated from three
//! locations — the project's `res://script_templates/`, the editor's user
//! templates, and the language's built-ins — each searched across the *whole
//! ancestor chain* of the base class, so a `Sprite2D` is still offered `Node`'s
//! default template (`editor/script/script_create_dialog.cpp`,
//! `_update_template_menu` → `_get_hierarchy`).
//!
//! Patina generated its script body as a client-side string literal in
//! `editor_ui.rs`, so there was nothing to populate a dropdown from and every
//! new script got the same body. This module is the registry and the renderer;
//! the HTTP surface in `editor_server.rs` is a thin wrapper over it.
//!
//! Built-in template content is ported verbatim from the vendored
//! `upstream/godot/modules/gdscript/editor/script_templates/`.

use std::path::{Path, PathBuf};

/// Where a template came from. Godot groups the dropdown by this and searches
/// project templates before built-ins, so a project can shadow a built-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateOrigin {
    /// Shipped with the engine.
    BuiltIn,
    /// Found under `res://script_templates/<Class>/`.
    Project,
}

impl TemplateOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            TemplateOrigin::BuiltIn => "built_in",
            TemplateOrigin::Project => "project",
        }
    }
}

/// One entry in the Template dropdown.
#[derive(Debug, Clone)]
pub struct ScriptTemplate {
    /// Stable identifier, `"<Class>/<file stem>"` — what the client sends back.
    pub id: String,
    /// Display name: `# meta-name:` if present, else the file stem capitalized.
    pub name: String,
    /// The class this template is registered under. A template applies to that
    /// class AND everything deriving from it.
    pub inherit: String,
    /// `# meta-description:` if present.
    pub description: String,
    pub origin: TemplateOrigin,
    /// Raw body, still carrying `_BASE_` / `_CLASS_` / `_TS_` placeholders.
    pub content: String,
}

/// Godot's `_TS_` placeholder stands in for one indent level so a template can
/// be re-indented to the editor's setting (`make_template` →
/// `_get_indentation()`).
///
/// ⚠️ We default to four spaces rather than Godot's tab, because every script
/// Patina writes today is space-indented and its GDScript fixtures assume that.
/// The placeholder is what matters for parity; the width stays a parameter.
pub const DEFAULT_INDENT: &str = "    ";

/// Substitutes a template's placeholders, exactly as
/// `GDScriptLanguage::make_template` does.
///
/// ⚠️ ORDER IS LOAD-BEARING: `_CLASS_SNAKE_CASE_` must be replaced **before**
/// `_CLASS_`, because `_CLASS_` is a prefix of it. Doing `_CLASS_` first turns
/// `_CLASS_SNAKE_CASE_` into `MyNodeSNAKE_CASE_` — a silent corruption that
/// only shows up in templates using the snake-case form.
pub fn render(content: &str, class_name: &str, base_class: &str, indent: &str) -> String {
    content
        .replace("_BASE_", base_class)
        .replace("_CLASS_SNAKE_CASE_", &to_snake_case(class_name))
        .replace("_CLASS_", &to_pascal_case(class_name))
        .replace("_TS_", indent)
}

/// `"basic_movement"` → `"Basic Movement"`, Godot's `String::capitalize()`,
/// used when a template file carries no `# meta-name:`.
fn capitalize_words(stem: &str) -> String {
    stem.split(['_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut cs = w.chars();
            match cs.next() {
                Some(c) => c.to_uppercase().collect::<String>() + cs.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn to_pascal_case(name: &str) -> String {
    capitalize_words(name).replace(' ', "")
}

fn to_snake_case(name: &str) -> String {
    // Split on existing separators first so `my_thing` survives unchanged, then
    // break camel humps.
    let mut out = String::new();
    for (i, ch) in name.chars().enumerate() {
        if ch == ' ' || ch == '-' {
            out.push('_');
        } else if ch.is_uppercase() && i > 0 && !out.ends_with('_') {
            out.push('_');
            out.extend(ch.to_lowercase());
        } else {
            out.extend(ch.to_lowercase());
        }
    }
    out
}

/// Parses one template file into a [`ScriptTemplate`].
///
/// Mirrors `ScriptCreateDialog::_parse_template`: `# meta-name:`,
/// `# meta-description:` and `# meta-space-indent:` lines are metadata rather
/// than body, leading indentation is folded back to `_TS_`, and the body has
/// its leading blank lines stripped.
pub fn parse_template(
    inherit: &str,
    file_stem: &str,
    source: &str,
    origin: TemplateOrigin,
) -> ScriptTemplate {
    const META: &str = "# meta-";
    let mut name = String::new();
    let mut description = String::new();
    // Godot lets a template declare how many spaces make one indent level, so a
    // space-indented template still round-trips through `_TS_`.
    let mut space_indent_size: usize = 4;
    let mut body = String::new();

    for line in source.lines() {
        if let Some(meta) = line.strip_prefix(META) {
            if let Some(v) = meta.strip_prefix("name:") {
                name = v.trim().to_string();
            } else if let Some(v) = meta.strip_prefix("description:") {
                description = v.trim().to_string();
            } else if let Some(v) = meta.strip_prefix("space-indent:") {
                // A non-integer or negative value is ignored with the default
                // kept, matching Godot's WARN_PRINT-and-carry-on behaviour.
                if let Ok(n) = v.trim().parse::<usize>() {
                    if n > 0 {
                        space_indent_size = n;
                    }
                }
            }
            continue;
        }
        body.push_str(&fold_indentation(line, space_indent_size));
        body.push('\n');
    }

    ScriptTemplate {
        id: format!("{inherit}/{file_stem}"),
        name: if name.is_empty() {
            capitalize_words(file_stem)
        } else {
            name
        },
        inherit: inherit.to_string(),
        description,
        origin,
        content: body.trim_start_matches('\n').to_string(),
    }
}

/// Rewrites a line's leading whitespace as `_TS_` repeats.
fn fold_indentation(line: &str, space_indent_size: usize) -> String {
    let mut out = String::new();
    let mut spaces = 0usize;
    for (i, ch) in line.char_indices() {
        if ch == '\t' {
            // A pending run of spaces shorter than one level is literal.
            if spaces > 0 {
                out.push_str(&" ".repeat(spaces));
                spaces = 0;
            }
            out.push_str("_TS_");
        } else if ch == ' ' {
            spaces += 1;
            if spaces == space_indent_size {
                out.push_str("_TS_");
                spaces = 0;
            }
        } else {
            // First non-whitespace character: the rest of the line is content.
            if spaces > 0 {
                out.push_str(&" ".repeat(spaces));
            }
            out.push_str(&line[i..]);
            return out;
        }
    }
    // Whitespace-only line.
    if spaces > 0 {
        out.push_str(&" ".repeat(spaces));
    }
    out
}

/// The engine's built-in templates, ported from the vendored Godot tree.
///
/// Only the ones whose base class Patina actually instantiates are carried;
/// `EditorPlugin`/`VisualShaderNodeCustom` templates would advertise editor
/// extension points that do not exist here yet.
pub fn built_in_templates() -> Vec<ScriptTemplate> {
    // Sources are the vendored files verbatim, so `parse_template` does the
    // same meta-stripping and `_TS_` folding it does for project templates —
    // one code path rather than a hand-transcribed second copy.
    const OBJECT_EMPTY: &str =
        "# meta-description: Empty template suitable for all Objects\n\nextends _BASE_\n";
    const NODE_DEFAULT: &str = "# meta-description: Base template for Node with default Godot cycle methods\n\nextends _BASE_\n\n\n# Called when the node enters the scene tree for the first time.\nfunc _ready() -> void:\n\tpass # Replace with function body.\n\n\n# Called every frame. 'delta' is the elapsed time since the previous frame.\nfunc _process(delta: float) -> void:\n\tpass\n";
    const CHARBODY_2D: &str = "# meta-description: Classic movement for gravity games (platformer, ...)\n\nextends _BASE_\n\n\nconst SPEED = 300.0\nconst JUMP_VELOCITY = -400.0\n\n\nfunc _physics_process(delta: float) -> void:\n\t# Add the gravity.\n\tif not is_on_floor():\n\t\tvelocity += get_gravity() * delta\n\n\t# Handle jump.\n\tif Input.is_action_just_pressed(\"ui_accept\") and is_on_floor():\n\t\tvelocity.y = JUMP_VELOCITY\n\n\t# Get the input direction and handle the movement/deceleration.\n\tvar direction := Input.get_axis(\"ui_left\", \"ui_right\")\n\tif direction:\n\t\tvelocity.x = direction * SPEED\n\telse:\n\t\tvelocity.x = move_toward(velocity.x, 0, SPEED)\n\n\tmove_and_slide()\n";

    vec![
        parse_template("Object", "empty", OBJECT_EMPTY, TemplateOrigin::BuiltIn),
        parse_template("Node", "default", NODE_DEFAULT, TemplateOrigin::BuiltIn),
        parse_template(
            "CharacterBody2D",
            "basic_movement",
            CHARBODY_2D,
            TemplateOrigin::BuiltIn,
        ),
    ]
}

/// Templates found under `<project_root>/script_templates/<Class>/*.gd`.
///
/// Returns an empty list when the directory is absent — a project without
/// templates is the normal case, not an error.
pub fn project_templates(project_root: &Path) -> Vec<ScriptTemplate> {
    let root = project_root.join("script_templates");
    let mut found = Vec::new();
    let Ok(classes) = std::fs::read_dir(&root) else {
        return found;
    };
    let mut class_dirs: Vec<PathBuf> = classes.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    // Directory order is filesystem-dependent; sort so the dropdown is stable
    // between requests.
    class_dirs.sort();
    for class_dir in class_dirs {
        if !class_dir.is_dir() {
            continue;
        }
        let Some(class_name) = class_dir.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Ok(entries) = std::fs::read_dir(&class_dir) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        files.sort();
        for file in files {
            if file.extension().and_then(|e| e.to_str()) != Some("gd") {
                continue;
            }
            let Some(stem) = file.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Ok(source) = std::fs::read_to_string(&file) else {
                continue;
            };
            found.push(parse_template(
                class_name,
                stem,
                &source,
                TemplateOrigin::Project,
            ));
        }
    }
    found
}

/// Every template applicable to `base_class`, nearest ancestor first.
///
/// Godot searches the base class's whole ancestor chain so a `Sprite2D` still
/// gets `Node`'s default template, and searches project locations before
/// built-ins so a project can shadow one (`_update_template_menu`).
///
/// ⚠️ The chain comes from ClassDB, which is EMPTY until the classes are
/// registered. An unseeded ClassDB would otherwise silently offer no templates
/// at all, so an unknown class falls back to the universal `Node` → `Object`
/// spine rather than returning nothing.
pub fn templates_for(base_class: &str, project_root: &Path) -> Vec<ScriptTemplate> {
    let mut chain = gdobject::class_db::inheritance_chain(base_class);
    if chain.is_empty() {
        chain = vec![base_class.to_string()];
    }
    for fallback in ["Node", "Object"] {
        if !chain.iter().any(|c| c == fallback) {
            chain.push(fallback.to_string());
        }
    }

    let project = project_templates(project_root);
    let built_in = built_in_templates();

    let mut out = Vec::new();
    for group in [&project, &built_in] {
        for class in &chain {
            for t in group.iter().filter(|t| &t.inherit == class) {
                out.push(t.clone());
            }
        }
    }
    out
}

/// Looks a template up by [`ScriptTemplate::id`].
pub fn find_template(id: &str, base_class: &str, project_root: &Path) -> Option<ScriptTemplate> {
    templates_for(base_class, project_root)
        .into_iter()
        .find(|t| t.id == id)
}

/// Why a script path was rejected, or `None` when it is usable.
///
/// Godot validates the path live and shows the reason in the dialog rather than
/// failing at creation time (`script_create_dialog.cpp`, `_path_changed`), so
/// this returns the reason rather than a bool.
pub fn script_path_problem(path: &str) -> Option<String> {
    if path.trim().is_empty() {
        return Some("a path is required".to_string());
    }
    let Some(rest) = path.strip_prefix("res://") else {
        return Some("path must start with res://".to_string());
    };
    if rest.is_empty() {
        return Some("path needs a file name".to_string());
    }
    if !path.ends_with(".gd") {
        return Some("path must end in .gd".to_string());
    }
    let file = rest.rsplit('/').next().unwrap_or("");
    if file == ".gd" {
        return Some("path needs a file name".to_string());
    }
    // A traversal would escape the project sandbox that `res://` defines.
    if rest.split('/').any(|seg| seg == "..") {
        return Some("path must stay inside res://".to_string());
    }
    None
}

/// Renders the body for a brand-new script.
///
/// `template_id` of `None` selects the nearest applicable template, which is
/// what the dialog pre-selects.
pub fn generate_script(
    base_class: &str,
    class_name: &str,
    template_id: Option<&str>,
    project_root: &Path,
    indent: &str,
) -> String {
    let chosen = match template_id {
        Some(id) => find_template(id, base_class, project_root),
        None => templates_for(base_class, project_root).into_iter().next(),
    };
    match chosen {
        Some(t) => render(&t.content, class_name, base_class, indent),
        // No template registered anywhere still has to produce a valid script,
        // and `extends <base>` is the minimum that compiles.
        None => format!("extends {base_class}\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_snake_case_is_substituted_before_class() {
        // `_CLASS_` is a prefix of `_CLASS_SNAKE_CASE_`, so the naive order
        // corrupts the longer key into `MyNodeSNAKE_CASE_`.
        let out = render("_CLASS_SNAKE_CASE_ _CLASS_", "MyNode", "Node", "    ");
        assert_eq!(out, "my_node MyNode");
    }

    #[test]
    fn placeholders_cover_base_and_indent() {
        let out = render(
            "extends _BASE_\nfunc f():\n_TS_pass\n",
            "X",
            "Control",
            "  ",
        );
        assert_eq!(out, "extends Control\nfunc f():\n  pass\n");
    }

    #[test]
    fn meta_lines_are_metadata_not_body() {
        let t = parse_template(
            "Node",
            "default",
            "# meta-name: Fancy\n# meta-description: Does things\n\nextends _BASE_\n",
            TemplateOrigin::BuiltIn,
        );
        assert_eq!(t.name, "Fancy");
        assert_eq!(t.description, "Does things");
        assert_eq!(t.content, "extends _BASE_\n");
        assert_eq!(t.id, "Node/default");
    }

    #[test]
    fn name_falls_back_to_the_capitalized_file_stem() {
        let t = parse_template(
            "Node",
            "basic_movement",
            "extends _BASE_\n",
            TemplateOrigin::BuiltIn,
        );
        assert_eq!(t.name, "Basic Movement");
    }

    #[test]
    fn tab_indentation_folds_to_the_placeholder() {
        let t = parse_template(
            "Node",
            "d",
            "func f():\n\tpass\n\t\tdeep\n",
            TemplateOrigin::BuiltIn,
        );
        assert_eq!(t.content, "func f():\n_TS_pass\n_TS__TS_deep\n");
        // And it round-trips back out at the requested width.
        assert_eq!(
            render(&t.content, "C", "Node", "  "),
            "func f():\n  pass\n    deep\n"
        );
    }

    #[test]
    fn space_indent_meta_folds_spaces() {
        let t = parse_template(
            "Node",
            "d",
            "# meta-space-indent: 2\nfunc f():\n  pass\n",
            TemplateOrigin::BuiltIn,
        );
        assert_eq!(t.content, "func f():\n_TS_pass\n");
    }

    #[test]
    fn built_in_node_template_is_the_godot_body() {
        let node = built_in_templates()
            .into_iter()
            .find(|t| t.id == "Node/default")
            .expect("Node/default must exist");
        assert!(
            node.content.starts_with("extends _BASE_"),
            "the meta-description line must not survive into the body: {:?}",
            node.content
        );
        assert!(node.content.contains("func _ready() -> void:"));
        assert!(node.description.contains("Base template for Node"));
    }

    #[test]
    fn generated_script_extends_the_requested_class() {
        let out = generate_script(
            "Control",
            "MyThing",
            None,
            Path::new("/nonexistent"),
            "    ",
        );
        assert!(
            out.starts_with("extends Control"),
            "a Control script must extend Control, got: {out}"
        );
        assert!(!out.contains("_BASE_"), "placeholders must be substituted");
        assert!(
            !out.contains("_TS_"),
            "indent placeholder must be substituted"
        );
    }

    #[test]
    fn path_validation_names_the_broken_rule() {
        assert_eq!(script_path_problem("res://a/b.gd"), None);
        assert!(script_path_problem("a/b.gd").unwrap().contains("res://"));
        assert!(script_path_problem("res://b.txt").unwrap().contains(".gd"));
        assert!(script_path_problem("").unwrap().contains("required"));
        assert!(script_path_problem("res://.gd")
            .unwrap()
            .contains("file name"));
        assert!(script_path_problem("res://../x.gd")
            .unwrap()
            .contains("inside res://"));
    }
}
