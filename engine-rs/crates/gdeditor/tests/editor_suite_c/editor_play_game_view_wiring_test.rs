//! Frontend-wiring regression guard: pressing Play must switch the browser UI
//! to the Game view and render the running scene there (Godot
//! game_view_plugin.cpp `GameView::_play_pressed` — launching selects
//! `EditorMainScreen::EDITOR_GAME`; `_stop_pressed` returns to the prior screen).
//!
//! Why this test exists — the bug it guards: the SERVER side already switched
//! `editor_mode` to "game" on Play and restored it on Stop, and the server-side
//! e2e tests (`editor_play_e2e_test`, `editor_play_stop_restore_test`) were green.
//! But the served browser JS never acted on it: the Play button handler updated
//! the play buttons and started polling yet never swapped the central view, so
//! the editor stayed on the 2D/3D viewport and the running game was invisible —
//! exactly the user-reported "clicking Play doesn't switch to the Game tab and I
//! don't see the rendering". A pure model/endpoint test cannot catch that,
//! because the gap is in the client wiring, not the server. This inspects the
//! served `EDITOR_HTML` and asserts the Play/Stop handlers are actually wired to
//! switch to / restore from the Game view, and that the Game view is fed the
//! rendered frame — so the wiring can never silently regress to a no-op again.
//!
//! It inspects the static `EDITOR_HTML` constant directly (no server boot), so it
//! is fast and deterministic and sidesteps the host's code-sign/AMFI launch path.

const HTML: &str = gdeditor::editor_ui::EDITOR_HTML;

/// Count non-overlapping occurrences of `needle` in `HTML`.
fn count(needle: &str) -> usize {
    HTML.matches(needle).count()
}

#[test]
fn editor_play_game_view_wiring_test_play_switches_to_game_and_renders() {
    // --- The Game main-screen button + view exist (the destination) ---------
    assert!(
        HTML.contains(r#"data-mode="game""#),
        "the top bar exposes a Game main-screen button"
    );
    assert!(
        HTML.contains(r#"id="game-panel""#) && HTML.contains(r#"id="game-screen-img""#),
        "the Game view has a screen image to render the running scene into"
    );

    // --- Play is wired to switch the central view to the Game view ----------
    // The helper that performs the switch must be DEFINED and CALLED. The
    // definition is `function enterGameViewOnPlay()`; call sites are
    // `enterGameViewOnPlay();` — matching the call form proves the Play handler
    // actually invokes it (not merely that the function exists).
    assert!(
        HTML.contains("function enterGameViewOnPlay"),
        "a Play->Game-view switch helper is defined"
    );
    assert!(
        count("enterGameViewOnPlay();") >= 1,
        "the Play handler(s) actually call the Game-view switch (this was the bug: \
         the server set editor_mode=game but the client never switched the view)"
    );

    // The switch must (a) actually mount the Game view and (b) remember the
    // mode we came from so Stop can return to it (Godot screen_index_before_start).
    assert!(
        HTML.contains("applyMode('game')"),
        "the Play switch mounts the Game central view via applyMode('game')"
    );
    assert!(
        HTML.contains("prePlayMode"),
        "Play remembers the pre-play main-screen mode so Stop can restore it"
    );

    // --- Stop is wired to restore the pre-play view -------------------------
    assert!(
        HTML.contains("function restoreViewAfterStop"),
        "a Stop->restore-prior-view helper is defined"
    );
    assert!(
        count("restoreViewAfterStop();") >= 1,
        "the Stop handler actually calls the restore (so the editor is not left \
         stranded on the Game view after the run ends)"
    );

    // --- The Game view is fed the live rendered frame -----------------------
    // The viewport poll fetches the runtime's PNG and mirrors it into the Game
    // screen image, so the running scene is visible in the Game view. Without
    // this feed the view would switch but show nothing.
    assert!(
        HTML.contains("game-screen-img"),
        "the rendered frame is mirrored into the Game view's screen image"
    );
    // The live feed is the frame pump's long-poll of `/api/viewport?after=<seq>`
    // (an uncompressed BMP, ~7x cheaper to encode than PNG — see the comment
    // on `refreshViewport` in editor_ui.rs). The old `/api/viewport/png` poll
    // was deliberately replaced; pin the endpoint the client really uses.
    assert!(
        HTML.contains("fetch('/api/viewport?after='"),
        "the client polls the runtime-rendered frame to display it"
    );

    // --- applyMode is a real, non-cosmetic view swap ------------------------
    // applyMode must drive the central-view swap (setCentralView), not just
    // restyle a button — otherwise the Game tab would highlight but show nothing.
    assert!(
        HTML.contains("function applyMode") && HTML.contains("setCentralView"),
        "applyMode swaps the central view (setCentralView), not just the button style"
    );
}
