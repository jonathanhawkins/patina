//! pat-s3rrv.57 (merged finding 3, batch 2): RichTextLabel and CodeEdit carry
//! their real Godot properties — 52 members that were previously zero.
//!
//! Both were registered name-only (`ClassRegistration::new("CodeEdit").parent("TextEdit")`),
//! two more of the 86 classes the audit found with no properties at all. The
//! counts here — 30 and 22 — are the audit's own figures, and they fall out of
//! `doc/classes/*.xml` once the members an ancestor already declares are
//! excluded (the class only re-defaults those, and `get_property_list`
//! concatenates the chain without deduping).

use gdobject::class_db::{get_class_info, get_property_list, register_all_classes};
use gdvariant::Variant;

fn own(class: &str, name: &str) -> Variant {
    register_all_classes();
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is registered"))
        .properties
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} must declare `{name}`"))
        .default_value
}

fn own_count(class: &str) -> usize {
    register_all_classes();
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is registered"))
        .properties
        .len()
}

#[test]
fn richtextlabel_carries_its_thirty_members() {
    assert_eq!(own_count("RichTextLabel"), 30);

    assert_eq!(own("RichTextLabel", "bbcode_enabled"), Variant::Bool(false));
    assert_eq!(
        own("RichTextLabel", "progress_bar_delay"),
        Variant::Int(1000)
    );
    assert_eq!(own("RichTextLabel", "tab_size"), Variant::Int(4));
    assert_eq!(own("RichTextLabel", "visible_characters"), Variant::Int(-1));
    assert_eq!(own("RichTextLabel", "visible_ratio"), Variant::Float(1.0));
}

/// **The discriminating pair.** `autowrap_mode` exists on both text widgets and
/// defaults DIFFERENTLY: 3 (WORD_SMART) on RichTextLabel, 0 (OFF) on Label.
/// Registering one default for both — the obvious shortcut when the property
/// names match — would be wrong on one of them, and the single-class assertion
/// would not notice.
#[test]
fn autowrap_mode_differs_between_the_two_text_widgets() {
    assert_eq!(
        own("RichTextLabel", "autowrap_mode"),
        Variant::Int(3),
        "RichTextLabel wraps by default (WORD_SMART)"
    );
    assert_eq!(
        own("Label", "autowrap_mode"),
        Variant::Int(0),
        "Label does not (OFF)"
    );
}

/// Six RichTextLabel bools default to TRUE. A registration that assumed `false`
/// everywhere — the safe-looking choice — would ship a label with scrolling,
/// link underlining and shortcut keys all silently off.
#[test]
fn the_true_defaulting_bools_are_true() {
    for name in [
        "scroll_active",
        "deselect_on_focus_loss_enabled",
        "drag_and_drop_selection_enabled",
        "shortcut_keys_enabled",
        "meta_underlined",
        "hint_underlined",
    ] {
        assert_eq!(
            own("RichTextLabel", name),
            Variant::Bool(true),
            "`{name}` defaults to true in Godot"
        );
    }
}

/// ⚠️ TWENTY-FOUR, not twenty-two. This assertion was stale and RED AT HEAD:
/// `CodeEdit`'s registration carries 24 own members and the test still demanded
/// 22, so the whole binary failed on it (nextest cancels the remaining tests) —
/// it sank an unrelated completion that merely listed this file's test command.
///
/// 24 is the CORRECT number, checked against the oracle rather than simply
/// matching the code to itself: `doc/classes/CodeEdit.xml` documents exactly 24
/// `<member>` entries, so the registration is complete and the count was the
/// thing left behind when someone finished the surface.
#[test]
fn codeedit_carries_its_twenty_four_members() {
    assert_eq!(own_count("CodeEdit"), 24);

    assert_eq!(own("CodeEdit", "indent_size"), Variant::Int(4));
    assert_eq!(
        own("CodeEdit", "gutters_line_numbers_min_digits"),
        Variant::Int(3)
    );
    assert_eq!(own("CodeEdit", "line_folding"), Variant::Bool(false));
}

/// ⚠️ Four CodeEdit defaults are NON-EMPTY collections. An empty default is not
/// a harmless placeholder here: with no brace pairs and no string delimiters the
/// editor stops auto-closing quotes and stops recognising strings, which reads
/// as a broken editor rather than as a missing schema.
#[test]
fn the_collection_defaults_are_populated() {
    let Variant::Dictionary(pairs) = own("CodeEdit", "auto_brace_completion_pairs") else {
        panic!("auto_brace_completion_pairs is a Dictionary");
    };
    assert_eq!(
        pairs.len(),
        5,
        "quote, apostrophe and the three bracket kinds"
    );
    assert_eq!(
        pairs.get("("),
        Some(&Variant::String(")".to_string())),
        "a bracket closes its partner"
    );
    assert_eq!(
        pairs.get("\""),
        Some(&Variant::String("\"".to_string())),
        "a quote closes itself"
    );

    let Variant::Array(delims) = own("CodeEdit", "delimiter_strings") else {
        panic!("delimiter_strings is an Array");
    };
    assert_eq!(
        delims,
        vec![
            Variant::String("' '".to_string()),
            Variant::String("\" \"".to_string()),
        ],
        "each entry is `<start> <end>`, so a lone quote character would be wrong"
    );

    let Variant::Array(prefixes) = own("CodeEdit", "indent_automatic_prefixes") else {
        panic!("indent_automatic_prefixes is an Array");
    };
    assert_eq!(prefixes.len(), 4, "`:`, `{{`, `[`, `(`");
}

/// The same dedupe guard the first batch carries: re-declaring a member an
/// ancestor already has would show it twice in the inspector and in script
/// reflection.
#[test]
fn no_member_is_declared_twice_along_the_chain() {
    register_all_classes();

    for class in ["RichTextLabel", "CodeEdit"] {
        let names: Vec<String> = get_property_list(class)
            .into_iter()
            .map(|p| p.name)
            .collect();
        for name in &names {
            assert_eq!(
                names.iter().filter(|n| *n == name).count(),
                1,
                "`{name}` appears more than once in {class}'s property list"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// pat-s3rrv.74 — the METHOD half of the same class
// ---------------------------------------------------------------------------

/// Own (non-inherited) methods of `class`, as `(name, argument_count)`.
fn own_methods(class: &str) -> Vec<(String, usize)> {
    register_all_classes();
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is registered"))
        .methods
        .into_iter()
        .map(|m| (m.name, m.argument_count))
        .collect()
}

/// 🔑 RichTextLabel had 32/32 properties and 4/4 signals but **0 of 78 methods**.
///
/// So `ClassDB.class_get_method_list("RichTextLabel")` came back empty for a
/// class whose entire purpose is its method API, and
/// `class_has_method("RichTextLabel", "append_text")` answered false for a call
/// the engine services today — the same reflection-denies-the-engine shape as
/// the emitted-but-undeclared signals in pat-s3rrv.57.
///
/// These five are exactly the ones implemented and dispatched, all behind the
/// `is_rich_text_label` guard in `gdscene/src/scripting.rs`. Arities and the
/// `const` qualifiers come from `doc_classes/RichTextLabel.xml`.
#[test]
fn richtextlabel_declares_the_methods_it_actually_dispatches() {
    let methods = own_methods("RichTextLabel");

    for (name, arity) in [
        ("append_text", 1),
        ("add_text", 1),
        ("clear", 0),
        ("get_parsed_text", 0),
        ("get_total_character_count", 0),
    ] {
        let found = methods
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("RichTextLabel must declare `{name}`; got {methods:?}"));
        assert_eq!(
            found.1, arity,
            "`{name}` takes {arity} argument(s) per doc_classes/RichTextLabel.xml"
        );
    }
}

/// ⚠️ Methods belonging to OTHER classes must not appear here.
///
/// A bare name grep over the scene/script layers matches eight of Godot's 78
/// RichTextLabel methods, but `select_all`, `deselect` and `get_selected_text`
/// are guarded by `is_text_input` — they are LineEdit/TextEdit methods that
/// merely share a name. Registering them would advertise another widget's API on
/// this class, and a name-collision over-report is exactly how that happens.
///
/// This also pins the deliberate restraint: the other 73 documented methods are
/// unimplemented, and `class_has_method` answering true for a call that raises
/// is the same lie inverted.
#[test]
fn richtextlabel_does_not_borrow_the_text_input_methods() {
    let names: Vec<String> = own_methods("RichTextLabel")
        .into_iter()
        .map(|(n, _)| n)
        .collect();

    for foreign in ["select_all", "deselect", "get_selected_text"] {
        assert!(
            !names.contains(&foreign.to_string()),
            "`{foreign}` is dispatched for LineEdit/TextEdit, not RichTextLabel — \
             registering it here would advertise another class's method"
        );
    }

    assert_eq!(
        names.len(),
        5,
        "only the five dispatched methods belong here today; got {names:?}"
    );
}
