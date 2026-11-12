//! pat-s3rrv.212 — named `TranslationDomain`s and the pseudolocalization
//! transform.
//!
//! The pipeline is ported from
//! `upstream/godot/core/string/translation_domain.cpp`: `pseudolocalize`
//! (:480), the four per-character transforms, `_add_padding`, `_is_placeholder`
//! and the `_character_to_accented` table (:42). Defaults come from
//! `PseudolocalizationConfig` in the matching header.

use gdcore::translation::{PseudolocalizationConfig, Translation, TranslationServer};

fn catalog(locale: &str, key: &str, text: &str) -> Translation {
    let mut t = Translation::new(locale);
    t.add_message(key, text);
    t
}

fn cfg() -> PseudolocalizationConfig {
    PseudolocalizationConfig {
        enabled: true,
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Pseudolocalization
// ---------------------------------------------------------------------------

/// 🔑 The default configuration accents and brackets but does NOT lengthen:
/// `accents_enabled` is true while `expansion_ratio` is 0.0.
///
/// Worth pinning because "pseudolocalization is on" reads like "the string gets
/// longer", and a test asserting growth here would fail against correct Godot
/// behaviour.
#[test]
fn the_default_config_accents_and_brackets_without_expanding() {
    assert_eq!(cfg().pseudolocalize("abc"), "[áḅć]");
}

/// ⚠️ Padding derives from the ORIGINAL length, not the transformed one.
///
/// With double vowels on, "aaa" becomes six characters — but the underscore
/// count comes from the three it started with. Using the transformed length
/// doubles the padding too, which is the natural mistake when porting this.
#[test]
fn padding_uses_the_original_length_not_the_transformed_one() {
    let c = PseudolocalizationConfig {
        enabled: true,
        accents_enabled: false,
        double_vowels_enabled: true,
        expansion_ratio: 2.0,
        ..Default::default()
    };
    assert_eq!(c.pseudolocalize("aaa"), "[___aaaaaa___]");
}

/// `%s`-style placeholders survive every transform untouched — accenting the
/// `s` would break the format string at runtime.
#[test]
fn placeholders_are_skipped_by_accents_and_double_vowels() {
    let c = PseudolocalizationConfig {
        enabled: true,
        double_vowels_enabled: true,
        ..Default::default()
    };
    assert!(c.pseudolocalize("a %s a").contains("%s"));
}

/// ⚠️ A trailing `%` is not a placeholder, and `%z` is not one either — only
/// `%[scdoxXf]` counts.
#[test]
fn a_trailing_percent_is_not_a_placeholder() {
    let c = PseudolocalizationConfig {
        enabled: true,
        accents_enabled: false,
        double_vowels_enabled: true,
        ..Default::default()
    };
    assert_eq!(c.pseudolocalize("%z%"), "[%z%]");
}

/// 🔑 The accent table is transcribed, and these entries prove it: no
/// "add an accent" rule produces a Cyrillic or a Greek letter.
#[test]
fn the_surprising_accent_entries_match_godot() {
    let c = PseudolocalizationConfig {
        enabled: true,
        prefix: String::new(),
        suffix: String::new(),
        ..Default::default()
    };
    assert_eq!(c.pseudolocalize("N"), "й", "Godot maps N to CYRILLIC й");
    assert_eq!(c.pseudolocalize("n"), "ή", "and n to GREEK ή");
    assert_eq!(c.pseudolocalize("B"), "ß");
    assert_eq!(c.pseudolocalize("K"), "ĸ");
}

/// Fake bidi re-opens the RTL override after each newline, because the override
/// is popped by every one.
#[test]
fn fake_bidi_reopens_the_override_after_a_newline() {
    let c = PseudolocalizationConfig {
        enabled: true,
        accents_enabled: false,
        fake_bidi_enabled: true,
        prefix: String::new(),
        suffix: String::new(),
        ..Default::default()
    };
    assert_eq!(
        c.pseudolocalize("a\nb"),
        "\u{202e}a\u{202c}\n\u{202e}b\u{202c}"
    );
}

/// An empty message returns empty, NOT the prefix and suffix alone — Godot
/// returns before padding, so an empty Label stays empty rather than showing
/// "[]".
#[test]
fn an_empty_message_is_returned_untouched() {
    assert_eq!(cfg().pseudolocalize(""), "");
}

/// Override replaces the body with asterisks while still honouring placeholder
/// skipping.
#[test]
fn override_replaces_the_body_but_keeps_placeholders() {
    let c = PseudolocalizationConfig {
        enabled: true,
        accents_enabled: false,
        override_enabled: true,
        prefix: String::new(),
        suffix: String::new(),
        ..Default::default()
    };
    assert_eq!(c.pseudolocalize("ab %s"), "***%s");
}

// ---------------------------------------------------------------------------
// Named domains
// ---------------------------------------------------------------------------

/// A named domain resolves against its own catalogs, not the server's.
#[test]
fn a_named_domain_uses_its_own_catalog() {
    let mut server = TranslationServer::new();
    server.set_locale("fr");
    server.add_translation(catalog("fr", "GREET", "bonjour"));
    server
        .get_or_add_domain("debug")
        .add_translation(catalog("fr", "GREET", "BONJOUR-DEBUG"));

    assert_eq!(server.translate("GREET"), "bonjour");
    assert_eq!(
        server.translate_in_domain("debug", "GREET"),
        "BONJOUR-DEBUG"
    );
}

/// ⚠️ `fr_FR` must find a catalog shipped as `fr`. Without the language-part
/// step every region-tagged locale misses every catalog and `tr()` silently
/// returns the key — which reads as missing translations rather than a
/// resolution bug.
#[test]
fn a_region_locale_falls_back_to_its_language_catalog() {
    let mut server = TranslationServer::new();
    server.set_locale("fr_FR");
    server
        .get_or_add_domain("ui")
        .add_translation(catalog("fr", "GREET", "bonjour"));

    assert_eq!(server.translate_in_domain("ui", "GREET"), "bonjour");
}

/// A miss returns the KEY, not an empty string.
#[test]
fn a_missing_key_returns_itself() {
    let mut server = TranslationServer::new();
    server.get_or_add_domain("ui");
    assert_eq!(server.translate_in_domain("ui", "ABSENT"), "ABSENT");
}

/// An unknown domain name falls back to the default domain.
#[test]
fn an_unknown_domain_falls_back_to_the_default() {
    let mut server = TranslationServer::new();
    server.set_locale("fr");
    server.add_translation(catalog("fr", "GREET", "bonjour"));
    assert_eq!(server.translate_in_domain("nope", "GREET"), "bonjour");
}

/// ⚠️ The unnamed domain cannot be removed — doing so would silently drop every
/// default catalog.
#[test]
fn the_default_domain_is_permanent() {
    let mut server = TranslationServer::new();
    server.add_translation(catalog("en", "GREET", "hello"));
    assert!(server.has_domain(""));
    server.remove_domain("");
    assert_eq!(server.translate("GREET"), "hello");
}

/// 🔑 **The acceptance's behavioural hinge.** Toggling pseudolocalization
/// changes what `translate` returns — and `tr()` routes through it, so a Label
/// bound to a translated string re-renders differently with no change to the
/// interpreter's dispatch layer.
#[test]
fn toggling_pseudolocalization_changes_what_translate_returns() {
    let mut server = TranslationServer::new();
    server.set_locale("en");
    server.add_translation(catalog("en", "GREET", "hello"));
    assert_eq!(server.translate("GREET"), "hello");

    server.set_pseudolocalization_enabled(true);
    server.pseudolocalization_mut().accents_enabled = false;
    server.pseudolocalization_mut().expansion_ratio = 1.0;
    server.pseudolocalization_mut().prefix = "<<".to_string();
    server.pseudolocalization_mut().suffix = ">>".to_string();

    // "hello" is 5 chars; 5 * 1.0 / 2 truncates to 2 underscores a side.
    assert_eq!(server.translate("GREET"), "<<__hello__>>");
}

/// A pseudolocalized MISS is transformed too — that is what makes strings which
/// never reached the catalog visible in a running UI.
#[test]
fn a_pseudolocalized_miss_is_also_transformed() {
    let mut server = TranslationServer::new();
    server.set_pseudolocalization_enabled(true);
    server.pseudolocalization_mut().accents_enabled = false;
    assert_eq!(server.translate("UNTRANSLATED"), "[UNTRANSLATED]");
}

/// Each domain carries its own settings, so a debug overlay can be
/// pseudolocalized while the game's UI is not.
#[test]
fn domains_pseudolocalize_independently() {
    let mut server = TranslationServer::new();
    server.set_locale("en");
    server.add_translation(catalog("en", "GREET", "hello"));
    {
        let d = server.get_or_add_domain("debug");
        d.add_translation(catalog("en", "GREET", "hello"));
        d.pseudolocalization.enabled = true;
        d.pseudolocalization.accents_enabled = false;
    }
    assert_eq!(server.translate("GREET"), "hello");
    assert_eq!(server.translate_in_domain("debug", "GREET"), "[hello]");
}

/// `get_loaded_domains` lists the named domains, sorted for determinism.
#[test]
fn loaded_domains_are_listed_deterministically() {
    let mut server = TranslationServer::new();
    server.get_or_add_domain("ui");
    server.get_or_add_domain("debug");
    assert_eq!(server.get_loaded_domains(), vec!["debug", "ui"]);
}
