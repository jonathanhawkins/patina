# BENCHMARKS.md - Performance Measurement Framework

This document defines the performance measurement framework for the Patina Engine, including baseline workloads, metrics, determinism criteria, acceptable regression thresholds, and reporting format.

---

## Purpose

Performance is tracked continuously from the first runnable slice (Phase 4). The goal is not to optimize prematurely but to establish baselines, detect regressions early, and make performance visible throughout development.

---

## Baseline Workloads

Each workload exercises a specific subsystem or scenario. Baselines are measured against upstream Godot on the same hardware.

### Startup Workloads

| ID | Workload | Description |
|----|----------|-------------|
| BENCH-S01 | Empty project startup | Time from process launch to first frame ready |
| BENCH-S02 | Simple scene startup | Time to load and display a single-node 2D scene |
| BENCH-S03 | Complex scene startup | Time to load a scene with 100+ nodes and resources |
| BENCH-S04 | Resource-heavy startup | Time to load a project with 50+ distinct resources |

### Frame Time Workloads

| ID | Workload | Description |
|----|----------|-------------|
| BENCH-F01 | Idle frame | Frame time with an empty scene (no processing) |
| BENCH-F02 | Node processing | Frame time with 100 nodes running _process |
| BENCH-F03 | Signal-heavy frame | Frame time with 100 signal emissions per frame |
| BENCH-F04 | 2D sprite rendering | Frame time rendering 1000 Sprite2D nodes |
| BENCH-F05 | 2D physics step | Frame time with 100 physics bodies in simulation |

### Memory Workloads

| ID | Workload | Description |
|----|----------|-------------|
| BENCH-M01 | Baseline memory | Memory footprint of empty project at idle |
| BENCH-M02 | Scene memory | Memory footprint after loading complex scene |
| BENCH-M03 | Resource memory | Memory footprint with 100 loaded resources |
| BENCH-M04 | Sustained operation | Memory growth over 10,000 frames of operation |

### Resource Loading Workloads

| ID | Workload | Description |
|----|----------|-------------|
| BENCH-R01 | .tres parse time | Time to parse a typical .tres text resource |
| BENCH-R02 | .tscn parse time | Time to parse a typical .tscn scene file |
| BENCH-R03 | Texture load time | Time to load and prepare a texture resource |
| BENCH-R04 | Resource cache hit | Time for cached resource retrieval |

### Build Time Workloads

| ID | Workload | Description |
|----|----------|-------------|
| BENCH-B01 | Clean build | Full workspace build from clean state |
| BENCH-B02 | Incremental build | Rebuild after single-file change in a core crate |
| BENCH-B03 | Test suite run | Time to run full test suite |

---

## Metrics

### Primary Metrics

| Metric | Unit | Collection Method |
|--------|------|-------------------|
| Startup time | milliseconds | Wall clock from process launch to first frame |
| Frame time (mean) | milliseconds | Average over 1000-frame measurement window |
| Frame time (p99) | milliseconds | 99th percentile over 1000-frame window |
| Frame time (max) | milliseconds | Maximum in 1000-frame window |
| Memory footprint | megabytes | RSS at defined measurement points |
| Memory growth rate | KB/frame | Linear regression over 10,000-frame window |
| Resource load time | milliseconds | Wall clock for load operation |
| Build time | seconds | Wall clock for cargo build |

### Secondary Metrics

| Metric | Unit | Collection Method |
|--------|------|-------------------|
| CPU utilization | percent | System monitor during benchmark window |
| Allocation count | count | Allocator instrumentation per frame |
| Cache miss rate | percent | Hardware counters (when available) |
| Binary size | megabytes | Size of compiled engine binary |

---

## Determinism Criteria

Benchmarks must produce reliable, reproducible results:

1. **Warmup**: Discard the first 100 frames before measurement.
2. **Sample size**: Measure over at least 1000 frames or 10 iterations for non-frame metrics.
3. **Statistical reporting**: Report mean, median, standard deviation, p95, and p99.
4. **Environment control**: Document hardware, OS version, background load, and compiler version.
5. **Thermal management**: Allow system to reach thermal steady state before measurement.
6. **Repeat runs**: Each benchmark is run 3 times; report the median run.

### Nondeterministic Factors

The following factors may introduce variance and must be documented:

- Background system load
- Thermal throttling
- GPU driver behavior
- Memory allocation patterns
- OS scheduling

---

## Acceptable Regression Thresholds

Regressions are measured against the most recent established baseline.

| Metric | Warning Threshold | Failure Threshold |
|--------|-------------------|-------------------|
| Startup time | >10% regression | >25% regression |
| Frame time (mean) | >5% regression | >15% regression |
| Frame time (p99) | >10% regression | >25% regression |
| Memory footprint | >10% increase | >25% increase |
| Memory growth rate | Any sustained growth | >1 KB/frame sustained |
| Resource load time | >10% regression | >25% regression |
| Build time (clean) | >15% regression | >50% regression |
| Binary size | >10% increase | >25% increase |

### Threshold Enforcement

- **Warning**: Logged and flagged for review but does not block merge.
- **Failure**: Blocks merge until investigated. May be overridden with explicit justification documented in the PR.

---

## Reporting Format

Benchmark results are stored as JSON files under `tools/benchmarks/results/`:

```json
{
  "benchmark_id": "BENCH-F04",
  "description": "2D sprite rendering, 1000 Sprite2D nodes",
  "timestamp": "2026-03-18T00:00:00Z",
  "environment": {
    "os": "Linux 6.x x86_64",
    "cpu": "AMD Ryzen 9 7950X",
    "ram_gb": 64,
    "gpu": "NVIDIA RTX 4090",
    "rust_version": "1.XX.0",
    "build_profile": "release"
  },
  "upstream_baseline": {
    "mean_ms": 2.1,
    "median_ms": 2.0,
    "p95_ms": 2.8,
    "p99_ms": 3.2,
    "max_ms": 4.1,
    "stddev_ms": 0.3
  },
  "patina_result": {
    "mean_ms": null,
    "median_ms": null,
    "p95_ms": null,
    "p99_ms": null,
    "max_ms": null,
    "stddev_ms": null
  },
  "comparison": {
    "mean_ratio": null,
    "status": "not_measured"
  }
}
```

### Status Values

| Status | Meaning |
|--------|---------|
| `not_measured` | Patina result not yet available |
| `pass` | Within acceptable thresholds |
| `warning` | Exceeds warning threshold |
| `fail` | Exceeds failure threshold |
| `baseline_only` | Only upstream baseline measured |

---

## Comparison Against Upstream Godot

For every benchmark workload, the upstream Godot baseline is measured first using the pinned upstream version (see TEST_ORACLE.md). Patina results are then compared as ratios:

- **Ratio < 1.0**: Patina is faster than upstream (good)
- **Ratio = 1.0**: Parity
- **Ratio > 1.0**: Patina is slower than upstream (investigate if above threshold)

The goal for v1 is not to beat upstream Godot on performance but to be "competitive" -- within the warning thresholds defined above.

---

## Headless Runtime Baselines

In addition to the example harness, `engine-rs/tests/bench_runtime_baselines.rs`
provides test-based benchmarks that print timing data to stderr. These run as
part of the Tier 3 test suite (`cargo test --workspace`).

### Implemented Runtime Baselines

| Benchmark | Workload | Metric |
|-----------|----------|--------|
| `bench_step_1000_frames_*` | Step 1000 frames at 60 Hz | ms/frame |
| `bench_load_instance_*` | Parse .tscn + instance into SceneTree (100x) | ms/iter |
| `bench_physics_100_frames_*` | 100 physics frames at 60 Hz | ms/frame |
| `bench_variant_roundtrip` | 9-variant JSON roundtrip (100x) | ms/iter |
| `bench_parse_script_*` | Tokenize + parse .gd script (100x) | ms/iter |

Scenes covered: `space_shooter`, `platformer`, `physics_playground`, `hierarchy`, `minimal`.

### Running Baselines

```bash
cd engine-rs
# Run all benchmarks (output on stderr)
cargo test bench_ -- --nocapture 2>&1 | grep '\[bench\]'

# Run the example harness (JSON on stdout)
cargo run --example benchmarks
```

Baseline numbers are machine-dependent. To establish baselines for your
hardware, run the benchmarks and save the output. Compare future runs against
your saved baselines using the regression thresholds defined above.

## CI Integration

- Benchmark workloads run on dedicated CI hardware (not shared runners) to ensure consistent results.
- Results are stored in `tools/benchmarks/results/` and committed to the repository.
- A summary dashboard is generated from the result files.
- Regressions above the failure threshold block merges to main.
- Historical trends are tracked to detect gradual degradation.

---

## Runnable Benchmark Harness

A concrete benchmark harness is available at `engine-rs/examples/benchmarks.rs`. It uses `std::time::Instant` (no external dependencies) and outputs JSON to stdout.

### Running

```bash
cd engine-rs
cargo run --example benchmarks
cargo run --example benchmarks > benchmark_results.json  # Save to file
```

### Implemented Benchmarks

| Name | Workload |
|------|----------|
| `scene_load` | Parse demo_2d.tscn (5 nodes) |
| `resource_load` | Parse 4-property .tres resource |
| `physics_step_2d` | 100 rigid circles, 60 frames at 60 Hz |
| `physics_step_3d` | 100 rigid spheres, 60 frames at 60 Hz |
| `variant_conversion` | 1000 Variant JSON roundtrips |
| `render_frame_2d` | 100 canvas items into 640x480 framebuffer |

Each runs 100 iterations. Output format:

```json
{
  "engine": "patina",
  "timestamp": "unix:1710720000",
  "iterations_per_bench": 100,
  "benchmarks": [
    { "name": "scene_load", "iterations": 100, "total_us": 0, "avg_us": 0 }
  ]
}
```
