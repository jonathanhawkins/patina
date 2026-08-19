//! pat-s3rrv.63 — a new script extends the node's OWN class.
//!
//! `doAttachScript` wrote a hardcoded template:
//!
//! ```js
//! var template = 'extends Node2D\n\nfunc _ready():...'
//! ```
//!
//! for **every** node, whatever its class. Attaching a script to a `Control`,
//! a `Node3D` or a `Camera2D` produced a script whose base class did not match
//! its node. That is not a cosmetic default — it is a runtime error the moment
//! the script touches anything from its real base, and the editor offered no
//! hint that it had guessed.
//!
//! Godot's Create Script dialog (`editor/script/script_create_dialog.cpp`)
//! defaults the Inherits field to the selected node's class for this reason.
//!
//! ⚠️ Assertions slice the SPECIFIC function out of `EDITOR_HTML`. A page-wide
//! search would match any of the other places `extends` or `prompt` appear.

/// The source of the JS function named `name`, from its declaration to the
/// closing brace at the same indentation.
fn js_function<'a>(src: &'a str, name: &str) -> &'a str {
    let decl = format!("function {name}(");
    let start = src
        .find(&decl)
        .unwrap_or_else(|| panic!("no function {name} in EDITOR_HTML"));
    let rest = &src[start..];
    // Functions in this file are indented two spaces, so their closer is a
    // newline + exactly two spaces + `}`.
    let end = rest
        .find("\n  }")
        .unwrap_or_else(|| panic!("function {name} never closes"));
    &rest[..end]
}

/// **The acceptance test**: the template's base class comes from the node.
#[test]
fn attach_script_uses_the_nodes_class() {
    let html = gdeditor::editor_ui::EDITOR_HTML;
    let attach = js_function(html, "doAttachScript");

    assert!(
        !attach.contains("'extends Node2D"),
        "the template must not hardcode `extends Node2D` — that is the bug: \
         every node got a Node2D script regardless of its actual class",
    );
    // ⚠️ The base class no longer appears in `doAttachScript` as a literal.
    // That function now opens the Create Script dialog (pat-s3rrv.63) and the
    // body is generated server-side, so the assertion follows the code: the
    // dialog must SEED its Inherits field from the node, and must send that
    // class to the server. The contract is unchanged — a new script extends the
    // node's own class — only the place that enforces it moved.
    assert!(
        attach.contains("openCreateScriptDialog"),
        "attaching a script must open the Create Script dialog rather than a \
         prompt() — Godot opens ScriptCreateDialog here",
    );
    let open_fn = js_function(html, "openCreateScriptDialog");
    assert!(
        open_fn.contains("scriptBaseClassFor(nodeId)"),
        "the dialog's Inherits field must be seeded from the selected node",
    );
    let confirm_fn = js_function(html, "confirmCreateScript");
    assert!(
        confirm_fn.contains("base_class"),
        "…and that class must be sent to the server, which generates the body",
    );

    // The base is derived from the node's real class, with a sane fallback.
    let base_fn = js_function(html, "scriptBaseClassFor");
    assert!(
        base_fn.contains("selectedNodeData.class"),
        "the base class must be read off the selected node",
    );
    assert!(
        base_fn.contains("|| 'Node'"),
        "…falling back to `Node`, not `Node2D`: Node is the one base every node \
         in the tree actually has, so an unknown class degrades to 'less \
         specific' rather than 'wrong branch of the hierarchy'",
    );
}

/// The path is validated before anything is written (pat-s3rrv.63).
///
/// Previously any string was accepted: `doAttachScript` appended `.gd` and
/// POSTed it, so a typo'd or non-`res://` path created a file in a place the
/// project could never load from.
#[test]
fn the_script_path_is_validated() {
    let html = gdeditor::editor_ui::EDITOR_HTML;
    let check = js_function(html, "scriptPathProblem");

    assert!(
        check.contains("res://"),
        "a script path must be required to live under res://",
    );
    assert!(check.contains(".gd$"), "…and to end in .gd",);

    // The caller must actually consult it and stop — a validator nothing reads
    // is the same shape of bug as the missing Rect2 listener in pat-s3rrv.64.
    //
    // ⚠️ The consulting caller is now `validateCreateScript`, which the dialog
    // runs on EVERY keystroke instead of once at commit time. That is strictly
    // stronger than the old check: a bad path is refused before anything is
    // written AND the reason is shown next to the field.
    let validate = js_function(html, "validateCreateScript");
    assert!(
        validate.contains("scriptPathProblem("),
        "the dialog must call the validator",
    );
    assert!(
        validate.contains("disabled"),
        "…and must disable Create while the path is invalid, rather than \
         validating and carrying on regardless",
    );
    let confirm_fn = js_function(html, "confirmCreateScript");
    assert!(
        confirm_fn.contains("if (!validateCreateScript()) return;"),
        "…and must re-check on confirm, since Enter can reach it without the \
         button ever being clicked",
    );
}
