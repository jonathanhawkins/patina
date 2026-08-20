//! pat-s3rrv.128 — `tr(message, context)` honours the `msgctxt`.
//!
//! `tr()` accepted a second argument and **dropped it**, so a context-qualified
//! lookup silently returned the context-free translation. `parse_po` had no
//! `msgctxt` handling at all, so the contextual entries never existed to be
//! found.
//!
//! 🔑 This is a silent wrong-answer bug, not a missing feature: the call
//! succeeded and returned a plausible string. `msgctxt` exists precisely for
//! words whose translation depends on their context — "File" the menu versus
//! "file" the noun — so returning the wrong one is worse than returning nothing.

use gdcore::translation::{parse_po, TranslationServer};

/// A catalog where one msgid carries two different contexts plus a bare entry,
/// so all three are distinguishable.
const PO: &str = r#"
msgid "FILE"
msgstr "bare"

msgctxt "menu"
msgid "FILE"
msgstr "Menu File"

msgctxt "noun"
msgid "FILE"
msgstr "A file"
"#;

fn server() -> TranslationServer {
    let mut s = TranslationServer::new();
    s.add_translation(parse_po("en", PO));
    s.set_locale("en");
    s
}

/// **The core of the clause.** The same msgid under two contexts resolves to two
/// different strings.
#[test]
fn the_same_message_under_two_contexts_differs() {
    let s = server();
    let menu = s.translate_with_context("FILE", "menu");
    let noun = s.translate_with_context("FILE", "noun");

    assert_eq!(menu, "Menu File");
    assert_eq!(noun, "A file");
    assert_ne!(
        menu, noun,
        "the whole point of msgctxt is that these differ"
    );
}

/// ⚠️ A context-free lookup must NOT pick up a contextual entry, and vice
/// versa. Godot's map is an exact `(context, key)` pair
/// (`core/string/translation.cpp:169`) with no fall-through.
#[test]
fn the_bare_entry_and_the_contextual_ones_are_disjoint() {
    let s = server();
    assert_eq!(
        s.translate("FILE"),
        "bare",
        "no context reads the bare entry"
    );
    assert_eq!(
        s.translate_with_context("FILE", ""),
        "bare",
        "an empty context is the same as no context"
    );
}

/// 🔑 A context with no matching entry falls back to the KEY — it must not
/// silently return the bare translation.
///
/// ⚠️ This is the assertion that fails for the tempting implementation:
/// "look up (context, key), and if that misses try (·, key)". That version
/// returns "bare" here, which is exactly the wrong-answer this feature exists
/// to prevent.
#[test]
fn an_unknown_context_does_not_fall_back_to_the_bare_entry() {
    let s = server();
    assert_eq!(
        s.translate_with_context("FILE", "verb"),
        "FILE",
        "an unmatched context returns the source text, never the context-free \
         translation"
    );
}

/// The parser must attach a `msgctxt` to the message that FOLLOWS it, not the
/// one before it — `msgctxt` precedes its `msgid` in the .po format.
#[test]
fn msgctxt_binds_to_the_following_message() {
    let t = parse_po("en", PO);
    assert_eq!(
        t.get_message_with_context("menu", "FILE").as_deref(),
        Some("Menu File")
    );
    assert_eq!(t.get_message("FILE").as_deref(), Some("bare"));
    // If the context had bound backwards, the bare entry would have been filed
    // under "menu" and this would be the bare string.
    assert_ne!(
        t.get_message_with_context("menu", "FILE").as_deref(),
        Some("bare")
    );
}

/// **The interpreter half.** `tr(msg, ctx)` from GDScript routes the context
/// through — the argument used to be parsed and discarded.
///
/// ⓘ Seeds the engine-wide server, which is process-global; nextest runs each
/// test in its own process, so this cannot disturb the others.
#[test]
fn tr_from_gdscript_passes_the_context_through() {
    gdcore::translation::with_translation_server_mut(|s| {
        s.add_translation(parse_po("en", PO));
        s.set_locale("en");
    });

    let probe = "extends Node\n\
                 func menu():\n\
                 \x20   return tr(\"FILE\", \"menu\")\n\
                 func bare():\n\
                 \x20   return tr(\"FILE\")\n";

    let mut interp = gdscript_interop::interpreter::Interpreter::new();
    let class_def = interp.run_class(probe).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");

    let mut call = |m: &str| match interp.call_instance_method(&mut inst, m, &[]) {
        Ok(gdvariant::Variant::String(s)) => s,
        other => panic!("tr() must return a String, got {other:?}"),
    };

    assert_eq!(
        call("menu"),
        "Menu File",
        "the context argument must reach the lookup"
    );
    assert_eq!(
        call("bare"),
        "bare",
        "and a call with no context still reads the bare entry"
    );
}
