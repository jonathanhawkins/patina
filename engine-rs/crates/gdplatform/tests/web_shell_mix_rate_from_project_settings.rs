//! pat-mgixk.75 — the web shell runs the `AudioContext` at the project's mix
//! rate, not a hard-coded 44100.
//!
//! Godot resolves `audio/driver/mix_rate` through the platform feature tags, so
//! a web export honours `audio/driver/mix_rate.web` when the project sets one
//! and the base key otherwise (`platform/web/audio_driver_web.cpp` takes the
//! rate from the driver's settings, not a literal). Patina's generated shell
//! called `startWebAudio(44100)` outright, so a project asking for 48000 — or
//! for the browser's own rate — got neither.
//!
//! # 🔑 Why "non-zero", not "present"
//!
//! `audio/driver/mix_rate.web` has a **registered default of 0**
//! (`project_settings.rs:723`), and `ProjectSettings::get` falls back to the
//! defaults table (`:205-209`). So the `.web` key resolves for *every* project,
//! whether or not it was authored. A presence test — which is exactly what
//! `get_setting_with_override_and_custom_features` performs — would therefore
//! answer 0 for every project and the base key would be unreachable.
//!
//! `web_mix_rate` treats non-zero as "set". That is not a workaround: 0 is
//! Godot's own encoding of "let the browser decide", so "unset" and "pass 0
//! through" are the same answer. The first test below is the one that pins it.

use gdcore::{ProjectSettings, SettingsValue};
use gdplatform::web::{web_mix_rate, WebExportConfig};
use gdplatform::web_export::{layout, render_html_shell, DEFAULT_SHELL};

/// Godot's documented default for `audio/driver/mix_rate`.
const GODOT_DEFAULT: u32 = 44_100;
/// A rate the default could never produce, so an assertion on it cannot pass by
/// the resolver having done nothing.
const PROJECT_RATE: u32 = 48_000;
/// A `.web` override, distinct from both of the above.
const WEB_OVERRIDE: u32 = 22_050;

// ---------------------------------------------------------------------------
// Resolving the rate
// ---------------------------------------------------------------------------

/// **The seeded-default trap.** A project that authored nothing must still
/// resolve to the base default.
///
/// With a presence-based lookup this returns 0 — the `.web` default — and every
/// export would silently hand the browser "you choose" no matter what the
/// project asked for.
#[test]
fn an_unauthored_project_resolves_to_the_base_default() {
    let settings = ProjectSettings::new();

    // The trap is real, not hypothetical: assert the key DOES resolve, and to
    // zero, so the guard below is demonstrably load-bearing.
    assert_eq!(
        settings
            .get("audio/driver/mix_rate.web")
            .and_then(|v| v.as_int()),
        Some(0),
        "the `.web` key resolves for an unauthored project via the defaults \
         table; if this ever stops being true the non-zero guard in \
         `web_mix_rate` is no longer what makes the base key reachable"
    );

    assert_eq!(
        web_mix_rate(&settings),
        GODOT_DEFAULT,
        "an unauthored project must get Godot's base default; 0 would mean the \
         seeded `.web` default won"
    );
}

/// The base key is honoured when the project sets it.
#[test]
fn the_base_key_is_honoured() {
    let mut settings = ProjectSettings::new();
    settings.set(
        "audio/driver/mix_rate",
        SettingsValue::Int(PROJECT_RATE as i64),
    );
    assert_ne!(
        PROJECT_RATE, GODOT_DEFAULT,
        "the fixture rate must differ from the default or this is vacuous"
    );
    assert_eq!(web_mix_rate(&settings), PROJECT_RATE);
}

/// `.web` beats the base — the whole point of a per-platform override.
#[test]
fn a_web_override_beats_the_base_key() {
    let mut settings = ProjectSettings::new();
    settings.set(
        "audio/driver/mix_rate",
        SettingsValue::Int(PROJECT_RATE as i64),
    );
    settings.set(
        "audio/driver/mix_rate.web",
        SettingsValue::Int(WEB_OVERRIDE as i64),
    );
    assert_eq!(
        web_mix_rate(&settings),
        WEB_OVERRIDE,
        "`audio/driver/mix_rate.web` is the platform override and must win; \
         {PROJECT_RATE} means the base key was read instead"
    );
}

/// An explicit 0 passes through as 0 — Godot's "let the browser decide".
///
/// Not clamped up to 44100: `WebEngine::attach_audio_output` already treats 0 as
/// "leave the scene mixer's rate alone", so the `AudioContext`'s native rate
/// wins. Clamping here would quietly override a project that asked for exactly
/// that.
#[test]
fn an_explicit_zero_means_let_the_browser_choose() {
    let mut settings = ProjectSettings::new();
    settings.set("audio/driver/mix_rate", SettingsValue::Int(0));
    assert_eq!(web_mix_rate(&settings), 0);
}

// ---------------------------------------------------------------------------
// Reaching the shell
// ---------------------------------------------------------------------------

/// **The acceptance half that a browser would observe:** the rate reaches the
/// generated document.
#[test]
fn the_shell_calls_start_web_audio_with_the_configured_rate() {
    let mut config = WebExportConfig {
        mix_rate: PROJECT_RATE,
        ..WebExportConfig::default()
    };
    let plan = layout("My Game", &config);
    let html = render_html_shell("My Game", &config, &plan, None);

    assert!(
        html.contains(&format!("startWebAudio({PROJECT_RATE})")),
        "the shell must start the AudioContext at the project's rate"
    );
    assert!(
        !html.contains(&format!("startWebAudio({GODOT_DEFAULT})")),
        "…and must not also carry the old literal"
    );
    assert!(
        !html.contains("{{PATINA_MIX_RATE}}"),
        "an unsubstituted placeholder would reach the browser as a syntax error"
    );

    // The default still produces Godot's default, so an existing export is
    // byte-identical where it used to be.
    config.mix_rate = GODOT_DEFAULT;
    let html = render_html_shell("My Game", &config, &plan, None);
    assert!(html.contains(&format!("startWebAudio({GODOT_DEFAULT})")));
}

/// The template itself no longer hard-codes a rate.
///
/// 🔑 This is the assertion that fails at HEAD on *behaviour* rather than on a
/// missing symbol: `DEFAULT_SHELL` is public and unchanged in shape, so this
/// test says something even to a reader who only greps.
#[test]
fn the_template_has_no_hard_coded_rate() {
    assert!(
        !DEFAULT_SHELL.contains("startWebAudio(44100)"),
        "the shipped template must not pin a rate; that literal is what made \
         every export 44100 regardless of the project"
    );
    assert!(
        DEFAULT_SHELL.contains("{{PATINA_MIX_RATE}}"),
        "the template must take the rate as a placeholder"
    );
}
