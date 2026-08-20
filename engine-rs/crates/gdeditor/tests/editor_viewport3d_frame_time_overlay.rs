//! pat-s3rrv.150 — the 3D viewport's 'View Frame Time' overlay.
//!
//! Godot ground truth: `editor/scene/3d/node_3d_editor_plugin.h:128-164`
//! (`VIEW_FRAME_TIME` shows per-frame timings over the 3D viewport).
//!
//! `GET /api/viewport/3d/perf` has been served all along with **zero** client
//! references. This is its first consumer.
//!
//! ⚠️ **Only two of that endpoint's seven fields are actually produced**, and
//! the overlay deliberately shows only those. Checked in the workspace:
//!
//! | field | producer |
//! |---|---|
//! | `frames` | `PERF_FRAME_COUNT.fetch_add` — 2 real sites |
//! | `encode_ms` | `PERF_LAST_ENCODE_US.store` around the encode |
//! | `raster_ms` | **no `.store()` anywhere** — permanently `0.000` |
//! | `scale` | stored only by its own setter, read only by its own getter |
//!
//! That last row is why this bead's suggested "Half Resolution" cannot be built
//! on the render scale: nothing renders with it.

use gdeditor::editor_ui::EDITOR_HTML;

fn window_after(needle: &str, len: usize) -> String {
    let start = EDITOR_HTML
        .find(needle)
        .unwrap_or_else(|| panic!("`{needle}` must appear in EDITOR_HTML"));
    EDITOR_HTML[start..].chars().take(len).collect()
}

/// The endpoint is reachable from the client, and the menu can toggle it.
#[test]
fn the_frame_time_overlay_is_wired_into_the_view_menu() {
    assert!(
        EDITOR_HTML.contains("/api/viewport/3d/perf"),
        "the perf endpoint had no client reference at all before this"
    );
    assert!(
        EDITOR_HTML.contains("View Frame Time"),
        "Godot exposes this through the 3D View menu"
    );
    assert!(
        EDITOR_HTML.contains("data-act=\"view-frame-time\""),
        "the menu entry must carry the action the handler dispatches on"
    );
    assert!(
        EDITOR_HTML.contains("act === 'view-frame-time'"),
        "…and the handler must exist, or the entry is decoration"
    );
}

/// 🔑 The overlay renders ONLY the fields the engine actually produces.
///
/// ⚠️ `raster_ms` has no `.store()` anywhere in the workspace, so it is
/// permanently `0.000`, and `scale` is a knob nothing renders with. Displaying
/// either would put a number on screen that looks measured and never changes —
/// worse than omitting it, because it reads as a working instrument reporting
/// that nothing takes any time.
#[test]
fn the_overlay_shows_no_field_the_engine_never_writes() {
    assert!(
        !EDITOR_HTML.contains("d.raster_ms"),
        "🔑 `raster_ms` is never stored — reading it would always render 0.000"
    );
    let body = window_after("function sampleVp3dPerf", 1400);
    assert!(
        !body.contains("raster_ms"),
        "the overlay body must not reference the dead timing field; got: {body}"
    );
    assert!(
        !body.contains("scale"),
        "nor the render scale, which nothing consumes; got: {body}"
    );
    // And it DOES show the two live ones.
    assert!(
        body.contains("encode_ms") && body.contains("frames"),
        "the two produced fields are the whole point of the overlay"
    );
}

/// 🔑 FPS comes from the frame COUNTER delta, not from wall-clock sampling.
///
/// ⚠️ The viewport only re-encodes when something changed. A rate computed from
/// the polling interval alone would read as full speed on a completely idle
/// scene — an FPS meter that never drops is indistinguishable from one that is
/// not measuring anything.
#[test]
fn fps_is_derived_from_the_frame_counter_not_the_poll_interval() {
    let body = window_after("function sampleVp3dPerf", 1400);
    assert!(
        body.contains("d.frames - vp3dPerfLast.frames"),
        "🔑 the numerator must be the counter delta between two samples; \
         got: {body}"
    );
    assert!(
        body.contains("vp3dPerfLast"),
        "a previous sample has to be retained for a delta to exist"
    );
}

/// The overlay is created lazily and hidden when off.
///
/// ⚠️ A poll left running while the overlay is hidden would keep hitting the
/// server every 500ms for a panel nobody can see.
#[test]
fn the_poll_stops_when_the_overlay_is_off() {
    let body = window_after("function updateVp3dPerfOverlay", 700);
    assert!(
        body.contains("clearInterval"),
        "the timer must be cleared, not merely reassigned; got: {body}"
    );
    assert!(
        body.contains("editorMode === '3d'"),
        "the overlay belongs to the 3D viewport, so leaving 3D must stop it"
    );
}
