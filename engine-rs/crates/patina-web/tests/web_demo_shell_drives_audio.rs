//! The last link of pat-u3l8c.112's acceptance: the browser demo actually
//! drives the AudioWorklet backend.
//!
//! `web_audio_worklet.rs` proves the Rust half works — the mixer resamples into
//! the ring and `pull_block` hands a worklet full 128-frame quanta. It cannot
//! prove anything about the page, and the acceptance is *"sound plays in the
//! browser demo"*. Today nothing fails if the audio wiring is deleted from
//! `web/index.html`: every listed test still passes and the demo is silent.
//!
//! The sharper risk is a **name drift across the Rust/JS boundary**. The shell
//! reaches the backend entirely through `wasm_bindgen` `js_name`s and one
//! function exported by generated glue:
//!
//! ```text
//!   index.html  --  startWebAudio(44100)            -> web_audio.rs  js_name = startWebAudio
//!               --  audio.workletGlue               -> web_audio.rs  js_name = workletGlue
//!               --  audio.context                   -> web_audio.rs  getter  context
//!               --  audio.markUnlocked()            -> web_audio.rs  js_name = markUnlocked
//!               --  audio.pullBlock(renderQuantum)  -> web_audio.rs  js_name = pullBlock / renderQuantum
//!               --  glue.patinaInstallGestureUnlock -> gdaudio::web_output glue source
//! ```
//!
//! Rename any of those on the Rust side and the browser throws
//! `TypeError: ... is not a function` at runtime while the whole native suite
//! stays green. These tests pin the pairing instead, so the break lands in CI.
//!
//! They assert on *tokens*, not whole lines, so reformatting the shell (the
//! import list is currently being reflowed by pat-u3l8c.111/.114) does not
//! make them brittle.

use patina_web::audio::WebAudioEngine;

/// The engine's mix rate — Godot's `audio/driver/mix_rate` default, and what
/// the shell passes to `startWebAudio`.
const MIX_RATE: u32 = 44_100;
/// A rate the browser might pick, deliberately different from `MIX_RATE` so a
/// missing resample step cannot pass unnoticed. Mirrors `web_audio_worklet.rs`.
const CONTEXT_RATE: u32 = 48_000;

/// The demo shell that ships with the crate, read from disk.
fn shell() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/web/index.html");
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("the demo shell must ship with the crate ({path}): {e}"))
}

/// The page has to start the backend, hand the glue to the worklet, unlock on a
/// gesture and pump blocks. Miss any one and the graph is built but silent.
#[test]
fn the_demo_shell_drives_the_audio_backend() {
    let html = shell();

    for (needle, why) in [
        ("startWebAudio", "the page must start the audio backend"),
        (
            "workletGlue",
            "the processor source comes from the backend, not a copy in the page",
        ),
        (
            "patinaInstallGestureUnlock",
            "browsers start every AudioContext suspended — without this the graph runs and stays inaudible",
        ),
        (
            "markUnlocked",
            "the gesture has to be reported back, or the backend never believes it is unlocked",
        ),
        ("pullBlock", "something must actually drain the ring"),
        (
            "renderQuantum",
            "the block size must come from the backend rather than a hardcoded 128",
        ),
        (
            "enableBrowserMp3Decoding",
            "gdaudio's ISO MP3 decoder is target-gated off wasm32, so without this \
             every AudioStreamMP3 in a web scene decodes to UnsupportedFormat \
             (pat-mgixk.4)",
        ),
        (
            "registerDecodedMp3",
            "enabling browser MP3 decoding only installs the lookup — the page still \
             has to hand over what decodeAudioData produced, or every MP3 is a miss",
        ),
    ] {
        assert!(
            html.contains(needle),
            "web/index.html no longer references `{needle}` — {why}"
        );
    }
}

/// Every name the shell reaches across the boundary with must exist on the Rust
/// side. This is the half no native test covered: the glue is a *generated
/// string*, so a renamed export is not a compile error anywhere.
#[test]
fn the_shell_only_calls_names_the_rust_side_exports() {
    let html = shell();
    let engine = WebAudioEngine::new(CONTEXT_RATE, MIX_RATE);
    let glue = engine.worklet_glue();

    // The one function the page calls *on the glue module* rather than on the
    // backend object. `gdaudio::web_output` emits it; nothing else checks that
    // the emitted name is the one the page calls.
    assert!(
        html.contains("glue.patinaInstallGestureUnlock"),
        "the page is expected to install the gesture unlock through the glue module"
    );
    assert!(
        glue.contains("export function patinaInstallGestureUnlock"),
        "the glue must export the name the page calls, or the import resolves to undefined"
    );

    // The backend object's members, each declared with an explicit `js_name`
    // (or a plain getter for `context`) in `web_audio.rs`. Asserting against
    // that source keeps the camelCase spellings honest.
    let bindings = include_str!("../src/web_audio.rs");
    for (js_name, declaration) in [
        ("startWebAudio", "js_name = startWebAudio"),
        ("workletGlue", "js_name = workletGlue"),
        ("markUnlocked", "js_name = markUnlocked"),
        ("pullBlock", "js_name = pullBlock"),
        ("renderQuantum", "js_name = renderQuantum"),
        (
            "enableBrowserMp3Decoding",
            "js_name = enableBrowserMp3Decoding",
        ),
        ("registerDecodedMp3", "js_name = registerDecodedMp3"),
        ("decodedMp3Count", "js_name = decodedMp3Count"),
    ] {
        assert!(
            bindings.contains(declaration),
            "the page calls `{js_name}`, so web_audio.rs must still declare `{declaration}`"
        );
    }
    assert!(
        bindings.contains("pub fn context(&self)"),
        "the page passes `audio.context` to the gesture unlock, so the plain \
         `context` getter must stay named that — a js_name rename would break it silently"
    );
}
