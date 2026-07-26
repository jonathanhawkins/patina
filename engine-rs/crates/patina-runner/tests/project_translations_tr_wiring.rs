//! pat-0q2kx.147 — a project's `.po` translations reach `tr()` at runtime.
//!
//! Godot boots `TranslationServer` from `project.godot`'s
//! `internationalization/locale/translations` plus the locale/fallback settings
//! (`core/string/translation_server.cpp`), and every `Node.tr()` resolves
//! through it.
//!
//! Patina had a correct catalog in `gdcore::translation` — locale-chain
//! resolution, PO and CSV parsers — with **zero consumers**, while the
//! interpreter's `tr()` read a private `thread_local` store seeded only by a
//! scripted `add_message`. Two stores that could never see each other, so a
//! project's `.po` files never reached a script.
//!
//! This boots a real fixture project off disk and asserts through the script
//! path, with no seeding anywhere.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;
use patina_runner::project_boot::boot_project;

const FR_PO: &str = r#"msgid ""
msgstr ""
"Language: fr\n"

msgid "HELLO"
msgstr "Bonjour"

msgid "GOODBYE"
msgstr "Au revoir"
"#;

const EN_PO: &str = r#"msgid ""
msgstr ""
"Language: en\n"

msgid "HELLO"
msgstr "Hello"
"#;

/// A unique scratch directory. Process id and a counter keep concurrent test
/// binaries from colliding on a shared `/tmp` path.
fn temp_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_tr_wiring_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes a fixture project declaring both catalogs and forcing `fr`, then
/// boots it. The boot is what must populate the translation server.
fn boot_fixture(tag: &str) -> PathBuf {
    let dir = temp_dir(tag);
    std::fs::write(dir.join("fr.po"), FR_PO).expect("write fr.po");
    std::fs::write(dir.join("en.po"), EN_PO).expect("write en.po");
    std::fs::write(
        dir.join("project.godot"),
        r#"; Engine configuration file.
config_version=5

[application]

config/name="Translated"

[internationalization]

locale/translations=PackedStringArray("res://fr.po", "res://en.po")
locale/test="fr"
locale/fallback="en"
"#,
    )
    .expect("write project.godot");

    boot_project(&dir).expect("project boots");
    dir
}

/// Runs `source` and returns the value it `return`s, as a string.
fn script_returns(source: &str) -> String {
    let mut interp = Interpreter::new();
    let result = interp
        .run(source)
        .unwrap_or_else(|e| panic!("script runs: {e:?}"));
    match result.return_value {
        Some(Variant::String(s)) => s,
        other => panic!("script should return a String, got {other:?}"),
    }
}

/// **The acceptance test.** A booted project's `fr.po` makes `tr("HELLO")`
/// answer in French, with no script-side seeding at all.
#[test]
fn project_translations_tr_wiring() {
    let _dir = boot_fixture("main");

    // `locale/test = "fr"` selected the French catalog at boot.
    assert_eq!(
        script_returns("return tr(\"HELLO\")\n"),
        "Bonjour",
        "boot must have loaded res://fr.po and selected the `fr` locale",
    );

    // An untranslated key returns itself, which is Godot's fallback.
    assert_eq!(
        script_returns("return tr(\"NOT_A_KEY\")\n"),
        "NOT_A_KEY",
        "an untranslated key must come back unchanged",
    );

    // Both catalogs are loaded and listed.
    let locales =
        script_returns("var l = TranslationServer.get_loaded_locales()\nreturn \",\".join(l)\n");
    assert!(
        locales.contains("fr") && locales.contains("en"),
        "both loaded locales must be listed, got `{locales}`",
    );

    // Flipping the locale flips the catalog. Done last because the server is
    // process-global: leaving it on `en` would change what the assertions above
    // see if they ran after it.
    assert_eq!(
        script_returns("TranslationServer.set_locale(\"en\")\nreturn tr(\"HELLO\")\n"),
        "Hello",
        "set_locale(\"en\") must switch to the English catalog",
    );

    // `fr_FR` has no catalog of its own; the language part `fr` does, so the
    // locale chain must resolve it.
    assert_eq!(
        script_returns("TranslationServer.set_locale(\"fr_FR\")\nreturn tr(\"HELLO\")\n"),
        "Bonjour",
        "`fr_FR` must resolve through its language part `fr`",
    );

    // A locale with no catalog at all falls back to `en`, which lacks
    // `GOODBYE` — so it comes back unchanged rather than leaking the French
    // text from a non-matching catalog.
    assert_eq!(
        script_returns("TranslationServer.set_locale(\"de\")\nreturn tr(\"GOODBYE\")\n"),
        "GOODBYE",
        "an absent locale falls back to `en`, which lacks the key",
    );
}
