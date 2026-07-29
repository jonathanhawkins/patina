//! Idle-viewport render gating (pat-5xrsr).
//!
//! The editor example's main loop used to re-render the full, software-rasterized
//! viewport ~10x/second on every idle iteration — even when nothing had changed.
//! An idle editor therefore burned 30-77% CPU doing nothing, and orphaned
//! instances accumulated enormous CPU time that starved the shared build lock and
//! wedged every agent's builds.
//!
//! The fix is *render-on-change*: the idle loop computes a cheap
//! [`ViewportRenderSignature`] each tick and only re-renders when it differs from
//! the last rendered signature (plus a low-frequency keepalive). The
//! running-game (60fps) and animation-preview paths are unaffected — they keep
//! rendering every frame, gated separately by the loop, not by this signature.
//!
//! This lives in the library (rather than the example binary) purely so the
//! gating logic is unit-testable: you can't assert a CPU percentage in a test,
//! but you can assert the signature is *stable* across idle ticks and *changes*
//! on each thing that affects the rendered frame.

use crate::editor_server::EditorState;
use std::time::Duration;

/// A cheap, comparable fingerprint of all editor state that determines what the
/// idle viewport renders.
///
/// Two signatures compare equal iff re-rendering would (for the inputs we model)
/// produce the same frame, so the idle loop can skip the expensive software
/// raster whenever the signature is unchanged. It is intentionally cheap to
/// compute — only scalar reads plus the selection list, no rendering or IO — so
/// recomputing it ~20x/second while idle costs effectively nothing.
///
/// Floating-point inputs (zoom, pan, camera orbit) are stored as their IEEE-754
/// bit patterns. That lets the type derive `Eq`/`Hash` and makes *any* change —
/// including sub-epsilon orbit/zoom nudges — register as a difference, which is
/// exactly what we want for change detection.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ViewportRenderSignature {
    /// The active main-screen mode string (`"2d"`, `"3d"`, `"script"`, …). It
    /// selects which renderer and viewport chrome is drawn, so a mode switch must
    /// force a re-render.
    mode: String,
    /// Undo-stack depth. Every scene edit pushes (or, on undo, pops) a command,
    /// so this changes whenever the edited scene changes.
    undo_depth: usize,
    /// Redo-stack depth. Redoing pops the redo stack and re-applies an edit; a
    /// fresh edit clears it. Tracking it alongside `undo_depth` catches undo→redo
    /// transitions that leave the undo depth momentarily equal.
    redo_depth: usize,
    /// The primary selected node (drives the selection outline / inspector).
    selected: Option<u64>,
    /// The full multi-selection (drives multi-select outlines and the 3D gizmo).
    selected_multi: Vec<u64>,
    /// 2D viewport zoom, as bits.
    zoom_bits: u64,
    /// 2D viewport pan offset `(x, y)`, as bits.
    pan_bits: (u64, u64),
    /// Browser-reported render width of the viewport panel.
    render_w: u32,
    /// Browser-reported render height of the viewport panel.
    render_h: u32,
    /// The persistent 3D editor camera (orbit/pan/zoom): yaw, pitch, distance,
    /// and focus point `(x, y, z)` — all as bits — so orbiting, dollying, or
    /// panning the 3D view re-renders.
    cam_bits: [u32; 6],
}

impl ViewportRenderSignature {
    /// Computes the signature from the current editor state.
    ///
    /// Pure and cheap: it only reads scalars and copies the selection list. It
    /// performs no rendering and touches no IO, so the idle loop can call it on
    /// every tick to decide whether a real render is warranted.
    pub fn from_state(state: &EditorState) -> Self {
        let cam = &state.editor_viewport_3d.camera;
        Self {
            mode: state.editor_mode.clone(),
            undo_depth: state.undo_stack.len(),
            redo_depth: state.redo_stack.len(),
            selected: state.selected_node.map(|n| n.raw()),
            selected_multi: state.selected_nodes.iter().map(|n| n.raw()).collect(),
            zoom_bits: state.viewport_zoom.to_bits(),
            pan_bits: (
                state.viewport_pan.0.to_bits(),
                state.viewport_pan.1.to_bits(),
            ),
            render_w: state.render_width,
            render_h: state.render_height,
            cam_bits: [
                cam.yaw.to_bits(),
                cam.pitch.to_bits(),
                cam.distance.to_bits(),
                cam.focus_point.x.to_bits(),
                cam.focus_point.y.to_bits(),
                cam.focus_point.z.to_bits(),
            ],
        }
    }
}

/// True when the idle viewport must re-render *every* tick regardless of the
/// signature, because the frame is changing in a way the signature can't see:
///
/// - an animation preview is playing (nodes are being keyframed each tick), or
/// - a node drag (`/api/viewport/drag`) or 3D transform-gizmo drag is in
///   progress. Drags move nodes **live without pushing an undo command** (the
///   undo entry is committed only on drag-end), so `undo_depth` — and therefore
///   the [`ViewportRenderSignature`] — doesn't change mid-drag. Without this the
///   drag would appear frozen until the keepalive fired (pat-5xrsr follow-up).
pub fn force_render_active(state: &EditorState) -> bool {
    state.animation_playback.playing
        || state.drag_state.is_some()
        || state.gizmo_drag_start_3d.is_some()
        || !state.gizmo_drag_starts_3d.is_empty()
}

/// Decides whether the idle editor should re-render the viewport this tick.
///
/// The idle loop re-renders when any of these hold, and skips the expensive
/// software raster otherwise (the fix for pat-5xrsr's busy-loop):
///
/// - `anim_active` — an animation preview is playing, so keep it smooth by
///   rendering every tick (the running game has its own 60fps path).
/// - the render [`ViewportRenderSignature`] changed since the last rendered
///   frame, so edits, selection, camera moves, zoom/pan, resizes, and mode
///   switches show up within a tick (≤ the loop's sleep, well under 100ms).
/// - `since_last_render >= keepalive` — a low-frequency safety net that re-renders
///   even when nothing modeled changed, bounding staleness for anything the
///   signature doesn't capture.
///
/// `last_rendered` is `None` before the first render (so the first tick always
/// renders).
pub fn should_render_idle(
    last_rendered: Option<&ViewportRenderSignature>,
    current: &ViewportRenderSignature,
    anim_active: bool,
    since_last_render: Duration,
    keepalive: Duration,
) -> bool {
    anim_active || last_rendered != Some(current) || since_last_render >= keepalive
}

/// The sleep before the next idle tick.
///
/// While an interaction is actively producing changed frames (e.g. the user is
/// dragging/orbiting the camera) the loop polls fast so a continuous interaction
/// renders near 60 fps instead of the ~20 fps a flat 50ms tick allows. Once that
/// window lapses it falls back to the 50ms low-CPU tick. Only the *cheap*
/// signature check runs at the fast rate — the expensive software raster still
/// only happens on an actual change (see [`should_render_idle`]) — so this keeps
/// the pat-5xrsr idle-CPU win while making active navigation smooth.
pub fn idle_tick_sleep(interaction_active: bool) -> Duration {
    if interaction_active {
        Duration::from_millis(8)
    } else {
        Duration::from_millis(50)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The idle cadence is fast during active interaction and slow when idle.
    #[test]
    fn idle_tick_sleep_fast_during_interaction_slow_when_idle() {
        // Fast tick: poll ~125x/s so a drag/orbit is picked up with low latency
        // and renders as fast as the (reduced-resolution) raster allows.
        assert_eq!(idle_tick_sleep(true), Duration::from_millis(8));
        // Idle tick stays at the low-CPU rate pat-5xrsr established.
        assert_eq!(idle_tick_sleep(false), Duration::from_millis(50));
        // And the active cadence must be strictly quicker than the idle one.
        assert!(idle_tick_sleep(true) < idle_tick_sleep(false));
    }
}
