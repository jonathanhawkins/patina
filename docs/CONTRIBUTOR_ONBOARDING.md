# Contributor Onboarding

Welcome to Patina Engine. This guide gets new contributors productive across the runtime, the test surface, and the upstream Godot oracle workflows. Read `AGENTS.md` first for safety rules and coding conventions, then return here for hands-on setup.

For background on the broader contributor workflow, see also `docs/contributor-onboarding.md`.

## Setup

### Prerequisites

- **Rust** (latest stable, 2021 edition) with `cargo`
- **Godot 4.6.1-stable** (oracle workflows only)
- **Python 3** (oracle wrapper scripts)
- **pnpm** (website and monorepo tooling)
- **jq** (optional, JSON validation in extraction scripts)

### Clone the repository

```bash
git clone --recurse-submodules https://github.com/patinaengine/patina.git
cd patina
```

If you cloned without `--recurse-submodules`:

```bash
git submodule update --init --recursive
```

### Install the Rust toolchain

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default stable
```

Optional but recommended:

```bash
cargo install cargo-nextest --locked
```

### Bootstrap the workspace

```bash
cd engine-rs
cargo build
```

This compiles the workspace and warms the cargo cache so subsequent test runs are fast.

## Running Tests

All Rust commands MUST go through `./scripts/rust_task.sh` from the repo root. The wrapper serializes builds via a build-slot lock and caps `CARGO_BUILD_JOBS=2`, which keeps multi-worker swarms from thrashing the cache.

### Targeted test (preferred)

```bash
./scripts/rust_task.sh nextest run --test <test_binary_name>
```

Examples:

```bash
./scripts/rust_task.sh nextest run --test fuzz_geometry2d_property_test
./scripts/rust_task.sh nextest run --test phase6_3d_fixture_corpus_audit_test
./scripts/rust_task.sh nextest run --test docs_contributor_onboarding_test
```

`--test <name>` compiles only that one integration binary (typically 1–2 minutes). Avoid `--workspace` and `-E` filter expressions — they enumerate all 300+ test binaries and take 10+ minutes.

### Package-scoped test

When the test lives inside a crate's own `tests/` folder rather than `engine-rs/tests/`, target the package:

```bash
./scripts/rust_task.sh nextest run -p patina-engine --test <test_binary_name>
```

### Why no raw `cargo test`?

The wrapper enforces serialization via the build-slot lock under `.orchestrator/`. Concurrent raw `cargo` invocations corrupt the incremental cache and double build time. If you see `[rust_task] BLOCKED: worker '<name>' cannot run Rust builds.`, you're inside a worker pane — report the test command in your completion report and let the verifier lane execute it.

## Oracle Workflows

Patina uses the upstream Godot binary as a parity oracle. For each measured fixture, Godot produces a reference scene-tree dump and physics trace; the Rust runtime must reproduce it bit-for-bit (or within an audited tolerance).

### Capture an oracle output

```bash
python3 tools/oracle/capture_scene_tree.py fixtures/scenes/<name>.tscn
```

This invokes Godot in headless mode, loads the .tscn fixture, walks the scene tree, and writes JSON under `fixtures/golden/scenes/<name>.json`.

### Compare runtime to oracle

```bash
./scripts/rust_task.sh nextest run --test hierarchy_3d_fixture_parity_test
./scripts/rust_task.sh nextest run --test render_physics_comparison_tooling_test
```

These tests load the .tscn fixture and the matching golden JSON, run the Rust runtime, and assert the structural and numerical match.

### Add a new fixture to the corpus

1. Drop the .tscn into `fixtures/scenes/`.
2. Run the capture script (above) to produce a golden JSON.
3. Add the fixture name to the audit doc and the corresponding `phase*_*_audit_test.rs` corpus list.
4. Run the audit test to confirm the fixture is recognized.

## Build/Verify Loop

Day-to-day contributor cycle for a non-trivial change:

1. **Read `AGENTS.md` and `CLAUDE.md`** for the active gates and conventions.
2. **Pick a bead** with `br ready --json --unassigned --limit 5` and claim it via `br update <id> --status in_progress`.
3. **Reserve files** you'll edit via `/skill mail-reserve` to avoid stomping on other workers.
4. **Implement** the change. Add or update tests with the implementation — every fix needs a regression test.
5. **Verify locally**:
   - `./scripts/rust_task.sh nextest run --test <relevant_test>` for the targeted gate
   - `./scripts/rust_task.sh check -p <crate>` for a fast type/borrow check
6. **Report completion** with `/skill mail-complete <bead-id> --to <coordinator> --file <path> --test <command>`. The verifier lane re-runs your test commands; do NOT mark beads done yourself.
7. **Release reservations** with `/skill mail-release` once the verifier closes the bead.

If the verifier reopens the bead, read the reopen message body for the exact failure (exit 100 = build error, exit 101 = test panic), then iterate. Don't expand scope to fix unrelated test failures — file a separate bead for those.
