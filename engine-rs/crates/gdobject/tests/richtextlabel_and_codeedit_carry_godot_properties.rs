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

#[test]
fn codeedit_carries_its_twenty_two_members() {
    assert_eq!(own_count("CodeEdit"), 22);

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
