# gdplatform Roadmap

Planned refactors for the `gdplatform` crate to separate backend concerns
from engine-owned runtime services.

## Current State

`winit_backend.rs` currently bundles multiple responsibilities:
- Window creation and lifecycle (winit `Window` + `ApplicationHandler`)
- Input event translation (winit key codes → `InputEvent`)
- Display/framebuffer presentation (softbuffer surface management)
- Event loop ownership and frame timing

`display.rs` manages `DisplayServer` state but window and display flow
is partially duplicated in examples that orchestrate their own loops.

## Planned: Break winit backend into runtime-owned services (pat-icz)

Split `winit_backend.rs` responsibilities into distinct runtime-facing APIs:

1. **WindowService** — window creation, resize, close, focus events.
   Owned by runtime, not by the backend event loop.
2. **InputService** — raw event translation and routing into `InputState`.
   Already partially done; needs the backend to only push events, not own state.
3. **PresentationService** — framebuffer blit / surface management.
   Should be callable from the render step without reaching into backend internals.

The winit backend becomes a thin adapter that drives these services from the
OS event loop, rather than owning their state.

## Planned: Normalize display and window state flow (pat-oa3)

Make `DisplayServer` the single authority for window state:

1. Examples and tests should not create windows directly — they go through
   `DisplayServer` or a headless equivalent.
2. Window resize, vsync, and focus state should flow through `DisplayServer`
   events, not be read from backend internals.
3. Headless mode (tests, CI) should use the existing `HeadlessWindow` path
   without any winit dependency.

## Planned: Window lifecycle coverage (pat-v1w)

Add integration tests covering:
- Window create → resize → close flow
- Focus gain/loss events
- Multi-window state (if applicable)
