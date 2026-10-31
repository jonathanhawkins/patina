//! pat-nzgdu — every `Texture2D` virtual the engine INVOKES must stay declared.
//!
//! `gdscene/src/scene_tree.rs`'s scripted-resource draw path calls exactly
//! three virtuals on a procedural texture, and ClassDB declared **none** of
//! them: `Texture2D` carried three non-virtual methods and zero virtuals
//! against Godot's seven, while sibling `Texture3D` and `TextureLayered`
//! declare their full doc-XML virtual sets.
//!
//! 🔑 This is the INVERSE of the failure the rest of this audit finds.
//! Elsewhere ClassDB advertises methods the engine cannot run, so `has_method`
//! answers true and the call returns `Nil` silently. Here the engine runs the
//! override and ClassDB denied it existed — so a script written defensively as
//!
//! ```gdscript
//! if has_method("_get_width"): ...
//! ```
//!
//! skipped an override that would have worked, and the texture fell back to the
//! control's rect (`scene_tree.rs:5226` `.unwrap_or(rect.size.x)`) with nothing
//! reported. A fallback that works is what made this invisible.
//!
//! ⚠️ **Deliberately not the reverse check.** Godot gives Texture2D four more
//! virtuals (`_draw`, `_draw_rect_region`, `_has_alpha`, `_is_pixel_opaque`)
//! that no engine call site invokes. Asserting "documented implies declared"
//! would demand declaring uninvoked virtuals, which is the Nil-placeholder bug
//! this epic exists to find. This file checks only the implication that must
//! hold: invoked implies declared.

use gdobject::class_db::{class_has_method, get_class_info, register_all_classes};

/// The virtuals `scene_tree.rs` calls on a scripted Texture2D, with the arity
/// recorded at the call site and in `Texture2D.xml`.
///
/// ⚠️ Hand-maintained against the dispatch sites, like the `EditorPlugin`
/// sibling guard: the draw path passes each method name as a literal, so there
/// is nothing for a test to enumerate. Add a call site, add a row.
const INVOKED: &[(&str, usize, &str)] = &[
    ("_get_width", 0, "gdscene/src/scene_tree.rs:5226"),
    ("_get_height", 0, "gdscene/src/scene_tree.rs:5228"),
    ("_draw_rect", 5, "gdscene/src/scene_tree.rs:5235"),
];

/// Texture2D's OWN registered method names.
///
/// ⚠️ Deliberately `get_class_info(..).methods` and not `get_method_list`,
/// which walks the inheritance chain base-to-derived. An inherited declaration
/// would satisfy a `get_method_list` check while leaving Texture2D itself
/// undeclared, so that helper cannot pin where the registration lives.
macro_rules! own_methods {
    () => {{
        register_all_classes();
        get_class_info("Texture2D")
            .expect("Texture2D is registered")
            .methods
    }};
}

/// The assertion that would have caught the original gap.
#[test]
fn every_invoked_virtual_is_declared_on_texture2d_itself() {
    let methods = own_methods!();
    for (name, _, site) in INVOKED {
        assert!(
            methods.iter().any(|m| &m.name == name),
            "🔑 the engine calls `Texture2D.{name}()` at {site}, but Texture2D \
             declares no such method. `has_method` then answers FALSE for an \
             override the engine runs, so a script that guards with it skips a \
             working override and the draw silently falls back to the control's \
             rect. Declared methods: {:?}",
            methods.iter().map(|m| &m.name).collect::<Vec<_>>()
        );
    }
}

/// The arity must match the call site, not merely exist.
///
/// ⚠️ `_draw_rect` is the one that matters: the engine passes **five**
/// arguments (`to_canvas_item, rect, tile, modulate, transpose`). A declaration
/// with the wrong count still satisfies `has_method`, so the script would be
/// called with a mismatched signature and the error would surface inside the
/// script rather than at the seam.
#[test]
fn the_declared_arities_match_the_call_sites() {
    let methods = own_methods!();
    for (name, argc, site) in INVOKED {
        let m = methods
            .iter()
            .find(|m| &m.name == name)
            .unwrap_or_else(|| panic!("Texture2D.{name}() must be declared"));
        assert_eq!(
            m.argument_count, *argc,
            "the engine calls `Texture2D.{name}()` with {argc} argument(s) at \
             {site}; a mismatched declaration still passes `has_method` and \
             fails inside the script instead"
        );
    }
}

/// A declaration that is not marked virtual is not an override point.
///
/// All three are `virtual const` in `upstream/godot/doc/classes/Texture2D.xml`
/// (`_get_width`/`_get_height` as `virtual required const`), transcribed rather
/// than derived.
#[test]
fn the_declarations_are_virtual_and_const() {
    let methods = own_methods!();
    for (name, _, _) in INVOKED {
        let m = methods.iter().find(|m| &m.name == name).unwrap();
        assert!(
            m.is_virtual,
            "`Texture2D.{name}()` is `virtual const` upstream; without the \
             virtual flag it is a declared method rather than a script \
             override point"
        );
        assert!(
            m.is_const,
            "`Texture2D.{name}()` is `virtual const` upstream"
        );
    }
}

/// The predicate the interpreter actually consults.
///
/// `node_has_method` resolves through `class_has_method`, so this is the exact
/// question that answered `false` before the registration landed.
#[test]
fn class_has_method_now_answers_true() {
    register_all_classes();
    for (name, _, site) in INVOKED {
        assert!(
            class_has_method("Texture2D", name),
            "`class_has_method(\"Texture2D\", \"{name}\")` is what the \
             interpreter asks before dispatching; it must agree with the call \
             site at {site}"
        );
    }
}
