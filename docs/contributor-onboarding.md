# Contributor Onboarding — Runtime and Oracle Workflows

This guide covers the two core contributor workflows in the Patina engine:
running and testing the Rust runtime, and working with the upstream Godot
oracle. Read `AGENTS.md` first for safety rules and coding conventions.

## Prerequisites

- **Rust** (latest stable, 2021 edition) with `cargo`
- **Godot 4.6.1-stable** (for oracle workflows only)
- **Python 3** (for oracle wrapper scripts)
- **pnpm** (for website and monorepo tooling)
- **jq** (optional, for JSON validation in extraction scripts)

## Dev Environment Setup

Follow these steps to get a working development environment from scratch.

### 1. Clone the Repository

```bash
git clone --recurse-submodules https://github.com/patinaengine/patina.git
cd patina
```

If you already cloned without `--recurse-submodules`:

```bash
git submodule update --init --recursive
```

### 2. Install Rust

Install via [rustup](https://rustup.rs/):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default stable
```

Verify: `rustc --version` should print 1.75+ (2021 edition).

### 3. Install Node.js and pnpm

Required for the website (`apps/web/`) and monorepo tooling:

```bash
# Install Node.js 18+ via your preferred method (nvm, brew, etc.)
npm install -g pnpm
```

### 4. Install Python 3

Required for oracle wrapper scripts. Python 3.9+ is recommended:

```bash
python3 --version   # Verify 3.9+
```

### 5. Install Godot (optional — oracle workflows only)

Download Godot 4.6.1-stable from the official site or use a package manager.
Set the `PATINA_GODOT` environment variable if the binary is not on `PATH`:

```bash
export PATINA_GODOT=/path/to/godot
```

### 6. Verify the Setup

```bash
# Build the engine
cd engine-rs && cargo build

# Run fast tests (should complete in ~10s)
cargo test --workspace 2>&1 | tail -5

# Build the website
cd ../apps/web && pnpm install && pnpm build
```

If `cargo build` succeeds and tests pass, your environment is ready.

### Platform Notes

- **macOS**: Install Xcode command line tools (`xcode-select --install`) for
  the C linker. Homebrew is recommended for Python and jq.
- **Linux**: Install `build-essential` (Debian/Ubuntu) or equivalent for the
  C toolchain. Wayland or X11 headers may be needed for windowed tests.
- **Windows**: Install Visual Studio Build Tools with the "Desktop development
  with C++" workload. Use `rustup default stable-x86_64-pc-windows-msvc`.

## Repository Layout

```
engine-rs/          Rust engine workspace (crates + integration tests)
  crates/           Engine crates (gdcore, gdscene, gdphysics2d, etc.)
  tests/            Integration and parity tests
  Makefile          Test tier shortcuts
apps/godot/         GDExtension compatibility lab (godot-rust probes)
apps/web/           patinaengine.com website (Next.js)
tools/oracle/       Oracle capture scripts (GDScript + Python)
fixtures/           Golden data (scenes, resources, physics, render PNGs)
  oracle_outputs/   Extracted API artifacts (classdb signatures, scene trees)
  golden/           Golden comparison files (physics traces, render images)
  scenes/           .tscn fixture scenes used by tests
scripts/            Automation scripts (API refresh, agent mail, coordinator)
upstream/godot/     Pinned upstream Godot submodule
prd/                Product requirements and execution maps
docs/               Architecture docs and guides
```

## Runtime Workflow

### Building the Engine

```bash
cd engine-rs
cargo build
```

### Running Tests

Tests are organized in three tiers:

| Tier | Command | Scope | Time |
|------|---------|-------|------|
| 1 | `make test-fast` | Unit and integration (no goldens) | ~10s |
| 2 | `make test-golden` | Tier 1 + golden comparisons | ~30s |
| 3 | `make test` | Full suite (`cargo test --workspace`) | ~60s |

Always run at least Tier 1 before committing. Run the full suite before
submitting a PR.

### Running a Specific Test

```bash
# Run a single test file
cargo test --test gdphysics2d_scene_fixed_step_test

# Run a single test function
cargo test --test gdphysics2d_scene_fixed_step_test rigid_body_falls_under_gravity

# Run all tests in a crate
cargo test -p gdresource
```

### Writing Tests

Every change must include tests. Tests live in `engine-rs/tests/` as
integration tests. Name them descriptively with a suffix indicating the
test type:

- `*_parity_test.rs` — Compares Patina behavior against Godot oracle output
- `*_golden_test.rs` — Compares output against checked-in golden files
- `*_test.rs` — General integration or unit tests

Each test file should have a header comment documenting:
1. The bead ID it implements
2. What behaviors it covers
3. Coverage list (numbered)

Example:

```rust
//! pat-XXXX: Description of what this test covers.
//!
//! Coverage:
//!  1. First behavior tested
//!  2. Second behavior tested
```

### Engine Crate Structure

| Crate | Purpose |
|-------|---------|
| `gdcore` | Math types (Vector2, Transform2D, Color, Rect2) |
| `gdvariant` | Variant type system (Int, Float, String, Vector2, etc.) |
| `gdobject` | Object model, notifications, class registration |
| `gdresource` | Resource loading (.tres), caching, UID registry |
| `gdscene` | Scene tree, nodes, packed scenes, physics server, main loop |
| `gdphysics2d` | 2D physics simulation (bodies, shapes, collision, areas) |
| `gdrender2d` | 2D software renderer (framebuffer, draw commands, textures) |
| `gdserver2d` | Canvas server (viewport, canvas items, draw ordering) |
| `gdplatform` | Platform abstraction (input, windowing, headless backend) |
| `gdscript-interop` | GDScript variable and expression evaluation |
| `gdaudio` | Audio playback stubs |
| `gdeditor` | Editor server (web-based scene editor) |
| `patina-runner` | Executable runner entry point |

### Architecture Walkthrough

This section explains how the engine crates connect and how data flows
through the system at runtime.

#### Dependency Graph

The crates form a layered dependency tree. Lower layers know nothing about
higher layers.

```
                       patina-runner
                       /           \
                 gdeditor        gdplatform
                /    |    \         |
          gdscene  gdresource  gdscript-interop
           / | \       |
  gdphysics2d |  gdserver2d
       |      |       |
  gdrender2d  gdobject |
       \      |      /
        gdvariant
            |
          gdcore
```

Key rules:
- `gdcore` is the foundation — math types, IDs, and utilities only.
- `gdvariant` depends only on `gdcore` and defines the `Variant` enum.
- `gdobject` adds the object model (ObjectId, notifications, class info).
- `gdscene` is the central hub — it owns the `SceneTree`, `Node`, and
  `MainLoop`. Most higher crates depend on it.
- Server crates (`gdphysics2d`, `gdrender2d`, `gdserver2d`) implement
  specific subsystems and read from the scene tree but do not own it.
- `gdeditor` depends on most crates and provides the web-based editor.
- `gdplatform` handles OS-level concerns (windowing, input) and is used
  by the runner and editor.

#### Runtime Data Flow

A typical frame proceeds through these stages:

```
1. Input       gdplatform collects OS events (keyboard, mouse, gamepad)
     |
2. Process     MainLoop calls _process(delta) on all nodes in tree order
     |
3. Physics     MainLoop calls PhysicsServer.step(fixed_delta)
     |         PhysicsServer syncs gdphysics2d bodies ↔ scene nodes
     |
4. Signals     Deferred signals fire (connect_deferred, call_deferred)
     |
5. Render      RenderServer walks canvas items (gdserver2d)
     |         gdrender2d draws to the framebuffer
     |
6. Output      gdplatform presents the framebuffer to the window
```

#### Scene Loading Pipeline

Loading a `.tscn` file follows this path:

```
.tscn file on disk
    |  gdresource::parse_tres()
    v
PackedScene (ext_resources, sub_resources, nodes, connections)
    |  gdscene::add_packed_scene_to_tree()
    v
SceneTree (live Node hierarchy with properties set)
    |  SceneTree::notify_ready()
    v
Nodes receive NOTIFICATION_READY, signals connect, game runs
```

Resources (`.tres` files) follow a similar parse path but produce
`Resource` values instead of scene trees. The `ResourceCache` deduplicates
loads by path and UID.

#### Editor Architecture

The editor is a Rust HTTP server that serves a browser-based UI:

```
Browser (HTML/CSS/JS generated by editor_ui)
    |  HTTP REST requests
    v
EditorServer (TcpListener on localhost)
    |  Dispatches to handler functions
    v
EditorState
    ├── SceneTree (the scene being edited)
    ├── SceneEditor (add/delete/move/rename operations)
    ├── InspectorPanel (property viewing/editing)
    ├── EditorFileSystem (project file browsing)
    └── EditorPluginRegistry (plugin dock/type registration)
```

The editor does not run a game loop — it modifies the scene tree directly
through REST endpoints and re-renders the viewport on demand.

#### Key Design Patterns

**Variant boxing**: All node properties are stored as `Variant`. The
`Variant` enum wraps typed values (Int, Float, String, Vector2, etc.) and
provides conversion methods. This matches Godot's property system.

**Notification dispatch**: Nodes receive integer notification codes
(NOTIFICATION_READY, NOTIFICATION_PROCESS, etc.) through the
`_notification(what)` callback, matching Godot's notification system.

**Oracle-driven development**: The engine does not read Godot source code.
Instead, it captures Godot's runtime behavior via oracle probes and writes
parity tests that compare Patina's output against the captured data.

### Key Concepts

**Scene Tree**: Godot's node hierarchy. Nodes have a class name, properties
(stored as `Variant`), children, and groups. The `SceneTree` owns all nodes.

**PackedScene**: Parsed from `.tscn` files. Contains ext_resources, nodes,
and connections. Use `add_packed_scene_to_tree()` to instance into a tree.

**MainLoop**: Drives the scene tree through a deterministic frame loop with
fixed-timestep physics and variable-timestep process callbacks. Call
`step(delta)` for one frame or `run_frames(n, delta)` for batch execution.

**PhysicsServer**: Bridges scene nodes to `gdphysics2d`. Scans the tree for
RigidBody2D, StaticBody2D, CharacterBody2D, and Area2D nodes, creates
physics bodies, and syncs transforms each frame.

## Oracle Workflow

The oracle is upstream Godot. Patina tests compare engine behavior against
Godot's actual runtime output, not its source code.

### Version Pinning

The pinned Godot version is defined in `tools/oracle/common.py`:

```python
UPSTREAM_VERSION = "4.6.1-stable"
UPSTREAM_COMMIT  = "14d19694e0c82..."
```

The `upstream/godot/` submodule should match this commit. The refresh
scripts validate this automatically.

### Refreshing Oracle Artifacts

The master command to refresh all extracted API artifacts:

```bash
./scripts/refresh_api.sh
```

This runs two phases:

1. **GDExtension probes** (`apps/godot/extract_probes.sh`): Builds the
   GDExtension, runs Godot headless, captures ClassDB metadata (methods,
   properties, signals) for 28+ core classes.

2. **Oracle fixture capture** (`tools/oracle/run_all.sh`): Runs GDScript
   probes inside Godot on fixture scenes to capture scene trees, property
   values, signal traces, notification ordering, and frame traces.

Flags:

```bash
./scripts/refresh_api.sh --probes-only    # Skip oracle fixtures
./scripts/refresh_api.sh --oracle-only    # Skip GDExtension probes
./scripts/refresh_api.sh --dry-run        # Validate setup only
```

Environment variables:

- `PATINA_GODOT` — Path to Godot binary (auto-detected if not set)
- `PATINA_SKIP_BUILD` — Set to `1` to skip `cargo build` for probes

### Oracle Output Structure

Oracle captures produce JSON files in `fixtures/oracle_outputs/`:

- `classdb_probe_signatures.json` — ClassDB metadata for core classes
- `<scene>_tree.json` — Node hierarchy for each fixture scene
- `<scene>_properties.json` — Property values for each fixture scene
- `<scene>.json` — Combined capture (tree + properties + traces)

### GDExtension Probes

The GDExtension lab in `apps/godot/` contains Rust probe implementations
that extract ClassDB metadata via Godot's reflection API:

| Probe | Captures |
|-------|----------|
| `classdb_probe.rs` | Method signatures, property metadata, signals |
| `enum_constants_probe.rs` | Integer constants and enum values |
| `node_defaults_probe.rs` | Default property values per class |
| `singleton_probe.rs` | Singleton API surfaces |
| `resource_subtype_probe.rs` | Resource class subtypes |

Probes output JSONL with a `PATINA_PROBE:` prefix, which
`extract_probes.sh` splits by `capture_type` into separate files.

### GDScript Oracle Probes

The `tools/oracle/` directory contains GDScript probes that run inside
Godot to capture runtime behavior:

| Probe | File | Captures |
|-------|------|----------|
| Scene tree | `scene_tree_dumper.gd` | Node hierarchy, classes, paths |
| Properties | `property_dumper.gd` | Per-node property values |
| Signals | `signal_tracer.gd` | Signal emissions with arguments |
| Notifications | `notification_tracer.gd` | Lifecycle notification ordering |
| Frame trace | `frame_trace_capture.gd` | Per-frame execution order |
| Render | `render_capture.gd` | Framebuffer pixel data |
| Resources | `resource_roundtrip.gd` | Load/save/reload fidelity |

### Writing Parity Tests

Parity tests compare Patina output against oracle golden data. The pattern:

1. Load a fixture scene (`.tscn` or inline)
2. Build a scene tree and run frames
3. Compare properties/behavior against oracle JSON or known Godot values
4. Assert with meaningful error messages

```rust
#[test]
fn node_position_matches_oracle() {
    let tree = load_fixture_scene("platformer.tscn");
    let player = tree.get_node_by_path("/root/World/Player").unwrap();
    let pos = get_position(&tree, player);
    // Oracle says Player starts at (100, 300)
    assert_eq!(pos, Vector2::new(100.0, 300.0));
}
```

### Adding a New Oracle Capture

1. Create or modify a GDScript probe in `tools/oracle/`
2. Add a Python wrapper if needed
3. Update `extract_probes.sh` capture type list if it's a GDExtension probe
4. Run `./scripts/refresh_api.sh` to regenerate artifacts
5. Write parity tests that consume the new golden data
6. Commit the golden files alongside the tests

## CI Pipeline

All PRs and pushes to `main` run the CI workflow at
`.github/workflows/ci.yml`. Understanding the gate structure helps you
diagnose failures and write tests that land in the right slice.

### Main CI Gates

| Gate | Purpose | Depends on |
|------|---------|------------|
| `rust-fmt` | `cargo fmt --check` | — |
| `rust` | `cargo test --workspace` + clippy (Linux, macOS, Windows) | `rust-fmt` |
| `rust-render-goldens` | Render golden image comparison | `rust-fmt` |
| `rust-release` | Release build check | `rust-fmt` |
| `rust-audit` | `cargo audit` for dependency vulnerabilities | — |
| `rust-oracle-parity` | Oracle parity regression suite | `rust-fmt` |
| `web` | Website build and lint | — |

### Runtime Compat Slice Gates

These gates validate specific runtime slices in isolation:

| Gate | Slice | Key test patterns |
|------|-------|-------------------|
| `rust-compat-headless` | Headless runtime | `resource_`, `scene_`, `signal_`, `notification_`, `object_`, `classdb_`, `lifecycle_`, `packed_scene_` |
| `rust-compat-2d` | 2D rendering + physics | `physics_`, `render_`, `collision_`, `node2d_`, `geometry2d_`, `vertical_slice` |
| `rust-compat-3d` | 3D runtime | `node3d_`, `physics3d_`, `transform3d_` |
| `rust-compat-platform` | Platform layer | `input_`, `window_`, `platform_`, `audio_` |
| `rust-compat-fuzz` | Fuzz/property tests | `fuzz_property`, `property_tests` |

All slice gates depend on `rust-fmt`. The platform gate runs on all three
desktop OSes (Linux, macOS, Windows).

### Repin Validation Pipeline

When the upstream Godot submodule, oracle outputs, or golden fixtures
change, the repin validation workflow (`.github/workflows/repin-validation.yml`)
runs automatically. It can also be triggered manually via `workflow_dispatch`.

| Job | Purpose |
|-----|---------|
| `detect-version` | Auto-detects Godot version from submodule or dispatch input |
| `oracle-parity` | Tier 1 + Tier 2 oracle parity tests |
| `render-goldens` | Render golden comparison tests |
| `physics-trace-goldens` | Physics trace, deterministic, playground, stepping tests |
| `runtime-compat-slices` | Headless + 2D + 3D + platform compat tests |
| `pin-verification` | Validates submodule pin, oracle outputs, and golden fixtures exist |
| `gdextension-lab` | Builds GDExtension lab (optional, requires Godot on PATH) |
| `parity-summary` | Collects all gate results into a Markdown summary |

The `parity-summary` job fails if any required gate fails, blocking the
repin until all validation passes.

### Reading CI Failures

1. Check the failing gate name to identify the slice
2. Click into the job log to find the failing test name
3. Run the test locally: `cargo test --test <test_name> -- --nocapture`
4. If the failure is in a golden test, check whether the golden file
   needs regeneration: `./scripts/refresh_api.sh`

## Repin Workflow (Godot Version Bump)

A "repin" updates the upstream Godot version that Patina targets. This is
a multi-step process with CI validation at each stage.

### Step-by-step

1. **Update the submodule pin**:
   ```bash
   cd upstream/godot
   git fetch origin
   git checkout <new-tag-or-commit>
   cd ../..
   git add upstream/godot
   ```

2. **Update version constants**:
   - `tools/oracle/common.py`: `UPSTREAM_VERSION` and `UPSTREAM_COMMIT`

3. **Regenerate oracle artifacts**:
   ```bash
   ./scripts/refresh_api.sh
   ```
   This rebuilds GDExtension probes and recaptures all oracle fixtures.

4. **Regenerate golden files**:
   ```bash
   cd engine-rs
   # Run golden tests with PATINA_UPDATE_GOLDENS=1 to overwrite
   PATINA_UPDATE_GOLDENS=1 cargo test --workspace
   ```

5. **Run the full test suite**:
   ```bash
   cd engine-rs && make test
   ```
   Fix any parity regressions before proceeding.

6. **Commit and push**: The repin-validation workflow triggers automatically
   on changes to `upstream/godot`, `fixtures/oracle_outputs/**`, and
   `fixtures/golden/**`.

7. **Review the parity summary**: The `parity-summary` job produces a
   Markdown report with pass/fail counts per gate.

### Manual Dispatch

For intentional repins, use the GitHub Actions UI:

1. Go to Actions → "Repin Validation"
2. Click "Run workflow"
3. Enter the Godot version string (e.g., `4.6.1-stable`)
4. Optionally enable the GDExtension lab build

## Your First Issue

This walkthrough guides you through picking up and completing your first
issue (called a "bead" in this project).

### Step 1: Find a Good First Issue

Use the `br` CLI to list available work:

```bash
br ready --unassigned --limit 10
```

Look for beads tagged `good-first-issue` or P3 (low priority) tasks.
Documentation beads, test-coverage beads, and small feature additions are
good starting points.

### Step 2: Claim the Bead

```bash
br update <bead-id> --assignee <your-name> --status in_progress
```

This marks the bead as yours so others don't duplicate your work.

### Step 3: Read the Bead Description

```bash
br show <bead-id>
```

Pay attention to:
- The **Acceptance** criteria — what the bead requires to be complete
- Any **file paths** mentioned — read those files for context
- Any **dependencies** — other beads that must complete first

### Step 4: Implement

1. Create a feature branch: `git checkout -b <bead-id>-short-description`
2. Make your changes following `AGENTS.md` conventions
3. Write tests — every change needs at least one test
4. Run the test suite: `cd engine-rs && cargo test --workspace`

### Step 5: Submit

1. Commit with the bead ID in the message: `[<bead-id>] Description`
2. Push and open a PR targeting `main`
3. The CI pipeline will run automatically — check that all gates pass
4. Mark the bead done: `br update <bead-id> --status done`

### Example: Adding a Missing Parity Test

A common first issue is writing a parity test for an untested Godot
behavior. Here is the typical workflow:

```bash
# 1. Check oracle output for the behavior
cat fixtures/oracle_outputs/minimal_3d_properties.json | jq '.Camera3D'

# 2. Create a test file
cat > engine-rs/tests/my_parity_test.rs << 'EOF'
//! pat-XXXX: Camera3D default FOV parity.
//!
//! Coverage:
//!  1. Camera3D default fov matches Godot oracle value

use gdscene::SceneTree;

#[test]
fn camera3d_default_fov_matches_oracle() {
    let tree = SceneTree::new();
    // ... load scene, check property against oracle
}
EOF

# 3. Run the test
cargo test --test my_parity_test

# 4. Iterate until green, then commit
```

## Common Tasks

### Adding a new scene fixture

1. Create a `.tscn` file in `fixtures/scenes/`
2. Run oracle capture: `./scripts/refresh_api.sh --oracle-only`
3. Write tests that load and validate the scene

### Debugging a parity failure

1. Run the failing test with `--nocapture` to see details:
   `cargo test --test my_test -- --nocapture`
2. Check the oracle golden file for expected values
3. Compare with Patina's actual output
4. If the oracle is stale, refresh: `./scripts/refresh_api.sh`

### Updating after a Godot version bump

1. Update `UPSTREAM_VERSION` and `UPSTREAM_COMMIT` in
   `tools/oracle/common.py`
2. Update the `upstream/godot` submodule:
   `cd upstream/godot && git checkout <new-commit>`
3. Refresh all artifacts: `./scripts/refresh_api.sh`
4. Run full test suite: `cd engine-rs && make test`
5. Fix any parity regressions

## Further Reading

- `AGENTS.md` — Safety rules, coding conventions, bead workflow
- `prd/BEAD_EXECUTION_MAP.md` — Current work priorities and lane assignments
- `prd/PORT_GODOT_TO_RUST_PLAN.md` — Full porting strategy
- `tools/oracle/README.md` — Oracle toolchain details
- `.github/workflows/ci.yml` — Main CI pipeline definition
- `.github/workflows/repin-validation.yml` — Repin validation pipeline
- `docs/agent-mail-orchestration.md` — Multi-agent coordination
- `docs/flywheel-glossary.md` — Flywheel methodology terms
