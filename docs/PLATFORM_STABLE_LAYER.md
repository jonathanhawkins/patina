# gdplatform: Stable Layer Specification

> **Source of truth**: [`prd/PHASE7_PLATFORM_PARITY_AUDIT.md`](../prd/PHASE7_PLATFORM_PARITY_AUDIT.md)
> classifies each platform family as Measured, Implemented-not-yet-measured,
> Deferred, or Missing. This document describes the **measured headless and
> compatibility slice** — claims here must not exceed what the audit supports.

The `gdplatform` crate provides the foundational windowing, input, timing, and OS integration layer for the Patina Engine runtime. This document defines the responsibilities, API contracts, and stability guarantees of the first stable platform layer.

## Coverage Scope

The stable layer is validated in **headless mode** — all contracts below are
exercised without OS windows or GPU. Native per-OS platform parity (live X11,
Cocoa, Win32 windowing) is classified as "Implemented, partly measured" in the
Phase 7 audit and is **not** claimed as full parity by this specification.

## Subsystem Responsibilities

### Windowing (`window`, `display`, `backend`)

| Responsibility | Module | Key Types | Audit Status |
|---|---|---|---|
| Window lifecycle (create, resize, close) | `window` | `WindowConfig`, `WindowManager`, `WindowId`, `HeadlessWindow` | Measured (headless) |
| Multi-window management and event routing | `display` | `DisplayServer`, `VsyncMode` | Measured (headless) |
| Platform backend abstraction | `backend` | `PlatformBackend` (trait), `HeadlessPlatform` | Measured |
| Native OS windowing (X11, Cocoa, Win32) | per-OS modules | `winit_backend` | Implemented, partly measured |

**Contracts:**
- `PlatformBackend` and `WindowManager` are object-safe traits, allowing runtime backend swapping.
- `HeadlessPlatform` provides a fully functional backend for testing and CI without OS windows or GPU.
- `WindowConfig` builder produces deterministic, reproducible configurations.
- Multi-window isolation: events from one window do not leak to another.

> **Scope note:** The above contracts are measured via headless backends.
> Native windowed operation through `winit_backend` is implemented but not yet
> proven as live parity against Godot's `DisplayServer`.

### Input (`input`)

| Responsibility | Module | Key Types | Audit Status |
|---|---|---|---|
| Keyboard and mouse state tracking | `input` | `InputState`, `InputEvent`, `Key`, `MouseButton` | Measured (headless) |
| Action map binding (Godot-compatible) | `input` | `InputMap`, `ActionBinding` | Measured (headless) |
| Input snapshot for frame-consistent reads | `input` | `InputSnapshot` | Measured (headless) |

**Contracts:**
- `WindowEvent` converts to `InputEvent` via `to_input_event()`. Non-input events (resize, focus) return `None`.
- Action bindings map physical keys/buttons to named actions (`is_action_pressed`).
- Input state is updated per-frame through the `DisplayServer::poll_events` pipeline.

> **Scope note:** Input contracts are validated end-to-end through injected
> `WindowEvent`s in headless mode. Physical device input from OS HID layers
> is not yet measured as parity.

### Timing (`time`)

| Responsibility | Module | Key Types | Audit Status |
|---|---|---|---|
| Frame-independent timers | `time` | `Timer` | Measured |
| Monotonic tick sources | `os` | `get_ticks_msec()`, `get_ticks_usec()` | Measured |

**Contracts:**
- `get_ticks_usec()` is monotonically non-decreasing.
- `get_ticks_msec()` is consistent with `get_ticks_usec() / 1000`.
- `Timer` fires deterministically when stepped with a fixed `dt`.

### OS Integration (`os`, `platform_targets`)

| Responsibility | Module | Key Types | Audit Status |
|---|---|---|---|
| Platform detection | `os` | `OsInfo`, `Platform`, `current_platform()` | Measured |
| Target enumeration and validation | `platform_targets` | `DesktopTarget`, `Architecture`, `PlatformCapability` | Measured (metadata) |
| Capability queries | `platform_targets` | `supports_capability()`, `validate_current_target()` | Measured |

**Contracts:**
- `OsInfo::detect()` and `current_platform()` are deterministic (same host = same result).
- `current_target()` always resolves to a valid, CI-tested desktop target on supported hosts.
- Desktop targets declare capabilities (FileSystem, Networking, Threading, Windowing).

### Peripheral Subsystems (`clipboard`, `cursor`, `thread`)

| Responsibility | Module | Key Types | Audit Status |
|---|---|---|---|
| Clipboard read/write | `clipboard` | `Clipboard` (trait), `HeadlessClipboard` | Measured (headless) |
| Cursor shape and position | `cursor` | `CursorManager`, `CursorShape` | Measured (headless) |
| Threading primitives | `thread` | `GodotMutex`, `GodotSemaphore`, `GodotThread`, `WorkerThreadPool` | Measured |

**Contracts:**
- `Clipboard` is object-safe with headless implementation for tests.
- `WorkerThreadPool` + `GodotSemaphore` compose correctly for parallel task submission.
- `GodotMutex` + `GodotThread` provide safe concurrent access.

### Startup and Packaging (`patina-runner::bootstrap`, `export`)

| Responsibility | Module | Key Types | Audit Status |
|---|---|---|---|
| Engine bootstrap sequence (Godot init order) | `patina_runner::bootstrap` | `EngineBootstrap`, `BootPhase`, `BootConfig` | Measured (headless) |
| Export config and template generation | `gdplatform::export` | `ExportConfig`, `ExportTemplate`, `BuildProfile` | Measured (Patina staging) |
| Resource collection and manifest staging | `gdplatform::export` | `PackageExecutor`, `ResourceEntry`, `PackageResult` | Measured (Patina staging) |

**Contracts:**
- `EngineBootstrap` walks through 8 phases (Core → Servers → Resources → SceneTree → MainScene → Scripts → Lifecycle → Running) matching Godot's documented initialization order.
- `PackageExecutor` validates platform targets against the audited desktop target set, collects resources via `res://` resolution, and stages a manifest + resource listing + output marker.
- Packaging output is deterministic: identical config + identical project → identical manifest and resource listing.

> **Scope note:** The packaging flow exercises Patina's *staging* artifact path:
> config → resource collection → manifest → output marker. It does **not** claim
> full Godot export-template parity or native app-bundle distribution. The output
> marker is a placeholder — Patina does not yet produce native binaries through
> this path. See `prd/PHASE7_PLATFORM_PARITY_AUDIT.md` § "Packaging / Export Notes".

**Test coverage:** `engine-rs/tests/startup_runtime_packaging_flow_test.rs`

## Stability Guarantees

1. **Trait object safety**: `PlatformBackend`, `WindowManager`, and `Clipboard` remain object-safe.
2. **Headless parity**: Every windowed operation has a headless counterpart for CI.
3. **Deterministic construction**: All public configuration types produce identical results from identical inputs.
4. **Event pipeline integrity**: `WindowEvent -> InputEvent -> InputState -> ActionMap` pipeline is fully tested end-to-end.
5. **Packaging determinism**: `PackageExecutor` produces identical manifest and resource listing from identical inputs.

## Test Coverage

The stable layer is validated by `engine-rs/tests/platform_first_stable_layer_test.rs` which covers:

1. Full headless runtime lifecycle (init -> frame loop -> shutdown)
2. Platform backend trait object safety
3. Window manager trait object safety
4. Clipboard trait object safety
5. Display server input pipeline routing
6. Multi-window event isolation
7. Platform target validation and CI coverage
8. OS info consistency
9. Tick monotonicity
10. Worker pool and semaphore integration
11. Thread and mutex integration
12. Cursor + clipboard + display composition
13. WindowConfig builder validation
14. WindowEvent -> InputEvent roundtrip for all event types
15. Deterministic construction of all public types
16. Documentation validation — stable layer doc cites Phase 7 audit and covers all subsystems
17. Audit alignment — claimed responsibilities match audit classifications

See also `prd/PHASE7_PLATFORM_PARITY_AUDIT.md` § "Stable Layer / Startup Notes"
for the audit classification that backs each claim in this document.
