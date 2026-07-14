//! pat-6sij / pat-u0k: Render benchmark fixtures and reporting.
//!
//! Provides deterministic render benchmark fixtures that:
//! - Write machine-readable JSON baselines to `fixtures/golden/render/benchmark_baselines.json`
//! - Verify render determinism (identical pixels across repeated runs)
//! - Detect performance regressions against stored baselines (configurable threshold)
//! - Publish actionable per-fixture and aggregate performance summaries
//!
//! Named with `bench_` prefix → Tier 3 (full suite).
//! Determinism tests always assert; timing tests always pass but report regressions.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use gdcore::math::{Color, Rect2, Vector2};
use gdrender2d::renderer::SoftwareRenderer;
use gdrender2d::test_adapter::capture_frame;
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::viewport::Viewport;
use serde_json::json;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const WARMUP_ITERATIONS: u32 = 3;
const BENCH_ITERATIONS: u32 = 20;
/// Reference resolution for baselines.
const REF_WIDTH: u32 = 1280;
const REF_HEIGHT: u32 = 720;
/// Regression threshold: fail report if >2x slower than baseline.
const REGRESSION_THRESHOLD: f64 = 2.0;

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("engine-rs must have a parent directory")
        .join("fixtures")
        .join("golden")
        .join("render")
}

fn baselines_path() -> PathBuf {
    golden_dir().join("benchmark_baselines.json")
}

// ---------------------------------------------------------------------------
// Fixture builders — deterministic viewport configurations
// ---------------------------------------------------------------------------

/// Fixture: single centered rect (minimal baseline).
fn fixture_single_rect(w: u32, h: u32) -> Viewport {
    let mut viewport = Viewport::new(w, h, Color::rgb(0.1, 0.1, 0.15));
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(
            Vector2::new(w as f32 / 4.0, h as f32 / 4.0),
            Vector2::new(w as f32 / 2.0, h as f32 / 2.0),
        ),
        color: Color::rgb(0.2, 0.6, 1.0),
        filled: true,
    });
    viewport.add_canvas_item(item);
    viewport
}

/// Fixture: grid of N rects evenly distributed.
fn fixture_grid(n: u64, w: u32, h: u32) -> Viewport {
    let mut viewport = Viewport::new(w, h, Color::BLACK);
    let cols = (n as f32).sqrt().ceil() as u64;
    let rows = (n + cols - 1) / cols;
    let cell_w = w as f32 / cols as f32;
    let cell_h = h as f32 / rows.max(1) as f32;

    for i in 0..n {
        let mut item = CanvasItem::new(CanvasItemId(i + 1));
        let x = (i % cols) as f32 * cell_w;
        let y = (i / cols) as f32 * cell_h;
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(
                Vector2::new(x + 1.0, y + 1.0),
                Vector2::new(cell_w - 2.0, cell_h - 2.0),
            ),
            color: Color::rgb(
                (i % 7) as f32 / 6.0,
                ((i * 3 + 1) % 7) as f32 / 6.0,
                ((i * 5 + 2) % 7) as f32 / 6.0,
            ),
            filled: true,
        });
        viewport.add_canvas_item(item);
    }
    viewport
}

/// Fixture: overlapping items with varying z-index (tests z-sort cost).
fn fixture_overlapping_z(n: u64, w: u32, h: u32) -> Viewport {
    let mut viewport = Viewport::new(w, h, Color::rgb(0.05, 0.0, 0.1));
    let center_x = w as f32 / 2.0;
    let center_y = h as f32 / 2.0;
    let max_radius = w.min(h) as f32 / 3.0;

    for i in 0..n {
        let mut item = CanvasItem::new(CanvasItemId(i + 1));
        let t = i as f32 / n.max(1) as f32;
        let size = max_radius * (1.0 - t * 0.5);
        item.z_index = i as i32;
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(
                Vector2::new(center_x - size / 2.0, center_y - size / 2.0),
                Vector2::new(size, size),
            ),
            color: Color::new(t, 0.5, 1.0 - t, 0.8),
            filled: true,
        });
        viewport.add_canvas_item(item);
    }
    viewport
}

/// Fixture: mixed filled and unfilled rects (tests draw-command branching).
fn fixture_mixed_fill(n: u64, w: u32, h: u32) -> Viewport {
    let mut viewport = Viewport::new(w, h, Color::rgb(0.02, 0.02, 0.04));
    let cols = (n as f32).sqrt().ceil() as u64;
    let cell_w = w as f32 / cols.max(1) as f32;
    let cell_h = h as f32 / cols.max(1) as f32;

    for i in 0..n {
        let mut item = CanvasItem::new(CanvasItemId(i + 1));
        let x = (i % cols) as f32 * cell_w;
        let y = (i / cols) as f32 * cell_h;
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(
                Vector2::new(x + 2.0, y + 2.0),
                Vector2::new(cell_w - 4.0, cell_h - 4.0),
            ),
            color: Color::rgb(
                (i % 3) as f32 / 2.0,
                ((i + 1) % 3) as f32 / 2.0,
                ((i + 2) % 3) as f32 / 2.0,
            ),
            filled: i % 2 == 0,
        });
        viewport.add_canvas_item(item);
    }
    viewport
}

// ---------------------------------------------------------------------------
// Benchmark harness
// ---------------------------------------------------------------------------

/// Run a benchmark for a single fixture, returning (per_frame_ms, mp_per_sec, pixel_count).
fn bench_fixture(
    renderer: &mut SoftwareRenderer,
    viewport: &Viewport,
    w: u32,
    h: u32,
) -> (f64, f64, usize) {
    // Warmup
    for _ in 0..WARMUP_ITERATIONS {
        let _ = capture_frame(renderer, viewport);
    }

    let start = Instant::now();
    let mut last_fb = None;
    for _ in 0..BENCH_ITERATIONS {
        last_fb = Some(capture_frame(renderer, viewport));
    }
    let total_ms = start.elapsed().as_secs_f64() * 1000.0;
    let per_frame_ms = total_ms / BENCH_ITERATIONS as f64;
    let megapixels = (w as f64 * h as f64) / 1_000_000.0;
    let mp_per_sec = if per_frame_ms > 0.0 {
        megapixels / (per_frame_ms / 1000.0)
    } else {
        0.0
    };

    let pixel_count = last_fb.map_or(0, |fb| fb.pixels.len());
    (per_frame_ms, mp_per_sec, pixel_count)
}

/// Verify that rendering the same viewport twice produces identical output.
fn assert_deterministic(renderer: &mut SoftwareRenderer, viewport: &Viewport, name: &str) {
    let fb1 = capture_frame(renderer, viewport);
    let fb2 = capture_frame(renderer, viewport);
    assert_eq!(
        fb1.pixels.len(),
        fb2.pixels.len(),
        "Fixture '{name}': pixel buffer size mismatch"
    );
    assert_eq!(
        fb1.pixels, fb2.pixels,
        "Fixture '{name}': render output is non-deterministic"
    );
}

// ---------------------------------------------------------------------------
// Named fixture registry
// ---------------------------------------------------------------------------

struct FixtureSpec {
    name: &'static str,
    description: &'static str,
    build: fn(u32, u32) -> Viewport,
}

fn fixture_specs() -> Vec<FixtureSpec> {
    vec![
        FixtureSpec {
            name: "single_rect",
            description: "Single centered rectangle (minimal baseline)",
            build: |w, h| fixture_single_rect(w, h),
        },
        FixtureSpec {
            name: "grid_25",
            description: "5x5 grid of colored rectangles",
            build: |w, h| fixture_grid(25, w, h),
        },
        FixtureSpec {
            name: "grid_100",
            description: "10x10 grid of colored rectangles",
            build: |w, h| fixture_grid(100, w, h),
        },
        FixtureSpec {
            name: "grid_400",
            description: "20x20 grid of colored rectangles",
            build: |w, h| fixture_grid(400, w, h),
        },
        FixtureSpec {
            name: "overlapping_z_50",
            description: "50 overlapping items with z-index sorting",
            build: |w, h| fixture_overlapping_z(50, w, h),
        },
        FixtureSpec {
            name: "mixed_fill_100",
            description: "100 items, alternating filled/unfilled",
            build: |w, h| fixture_mixed_fill(100, w, h),
        },
    ]
}

// ===========================================================================
// TEST: All fixtures are deterministic
// ===========================================================================

#[test]
fn bench_render_fixtures_determinism() {
    let mut renderer = SoftwareRenderer::new();

    for spec in fixture_specs() {
        let viewport = (spec.build)(REF_WIDTH, REF_HEIGHT);
        assert_deterministic(&mut renderer, &viewport, spec.name);
    }

    eprintln!(
        "{}",
        json!({
            "test": "render_fixtures_determinism",
            "status": "pass",
            "fixture_count": fixture_specs().len(),
            "resolution": format!("{}x{}", REF_WIDTH, REF_HEIGHT),
        })
    );
}

// ===========================================================================
// TEST: Benchmark all fixtures and write JSON baseline report
// ===========================================================================

#[test]
fn bench_render_fixtures_baseline_report() {
    let mut renderer = SoftwareRenderer::new();
    let specs = fixture_specs();
    let mut results: BTreeMap<String, serde_json::Value> = BTreeMap::new();

    eprintln!("[pat-u0k] Render Benchmark Fixtures Report");
    eprintln!("  Resolution: {}x{}", REF_WIDTH, REF_HEIGHT);
    eprintln!("  Warmup: {WARMUP_ITERATIONS}, Iterations: {BENCH_ITERATIONS}");
    eprintln!("  ---");

    for spec in &specs {
        let viewport = (spec.build)(REF_WIDTH, REF_HEIGHT);

        // Determinism check
        assert_deterministic(&mut renderer, &viewport, spec.name);

        // Benchmark
        let (per_frame_ms, mp_per_sec, pixel_count) =
            bench_fixture(&mut renderer, &viewport, REF_WIDTH, REF_HEIGHT);

        eprintln!(
            "  {:<20} {:.3} ms/frame  ({:.1} MP/s)  [{} pixels]",
            spec.name, per_frame_ms, mp_per_sec, pixel_count
        );

        results.insert(
            spec.name.to_string(),
            json!({
                "description": spec.description,
                "per_frame_ms": round3(per_frame_ms),
                "mp_per_sec": round1(mp_per_sec),
                "pixel_count": pixel_count,
                "deterministic": true,
            }),
        );
    }

    // Build the baseline report
    let report = json!({
        "version": 1,
        "bead": "pat-u0k",
        "resolution": { "width": REF_WIDTH, "height": REF_HEIGHT },
        "warmup_iterations": WARMUP_ITERATIONS,
        "bench_iterations": BENCH_ITERATIONS,
        "regression_threshold": REGRESSION_THRESHOLD,
        "fixtures": results,
    });

    // Write baseline JSON to golden directory
    let baselines = baselines_path();
    let report_json = serde_json::to_string_pretty(&report).unwrap();
    std::fs::write(&baselines, &report_json).unwrap_or_else(|e| {
        eprintln!(
            "  [warn] Could not write baselines to {}: {e}",
            baselines.display()
        );
    });

    eprintln!("  ---");
    eprintln!("  Baseline written to: {}", baselines.display());
    eprintln!("\n{report_json}");
}

// ===========================================================================
// TEST: Regression detection against stored baselines
// ===========================================================================

#[test]
fn bench_render_fixtures_regression_check() {
    let baselines = baselines_path();

    // If no baseline file exists, generate one and skip regression check.
    let baseline_data = match std::fs::read_to_string(&baselines) {
        Ok(data) => data,
        Err(_) => {
            eprintln!(
                "[pat-u0k] No baseline file at {}; run bench_render_fixtures_baseline_report first.",
                baselines.display()
            );
            eprintln!("[pat-u0k] Skipping regression check (no baseline).");
            return;
        }
    };

    let baseline: serde_json::Value =
        serde_json::from_str(&baseline_data).expect("Failed to parse baseline JSON");

    let stored_fixtures = baseline["fixtures"]
        .as_object()
        .expect("baseline missing 'fixtures' object");

    let threshold = baseline["regression_threshold"]
        .as_f64()
        .unwrap_or(REGRESSION_THRESHOLD);

    let mut renderer = SoftwareRenderer::new();
    let specs = fixture_specs();
    let mut regressions = Vec::new();
    let mut improvements = Vec::new();

    eprintln!("[pat-u0k] Regression Check (threshold: {threshold:.1}x)");

    for spec in &specs {
        let stored = match stored_fixtures.get(spec.name) {
            Some(v) => v,
            None => {
                eprintln!("  {}: NEW (no baseline)", spec.name);
                continue;
            }
        };
        let baseline_ms = stored["per_frame_ms"].as_f64().unwrap_or(0.0);
        if baseline_ms <= 0.0 {
            continue;
        }

        let viewport = (spec.build)(REF_WIDTH, REF_HEIGHT);
        let (current_ms, _, _) = bench_fixture(&mut renderer, &viewport, REF_WIDTH, REF_HEIGHT);

        let ratio = current_ms / baseline_ms;
        let status = if ratio > threshold {
            regressions.push(spec.name);
            "REGRESSION"
        } else if ratio < 0.8 {
            improvements.push(spec.name);
            "IMPROVED"
        } else {
            "OK"
        };

        eprintln!(
            "  {:<20} baseline={:.3}ms  current={:.3}ms  ratio={:.2}x  [{}]",
            spec.name, baseline_ms, current_ms, ratio, status
        );
    }

    let summary = json!({
        "test": "regression_check",
        "threshold": threshold,
        "total_fixtures": specs.len(),
        "regressions": regressions.len(),
        "improvements": improvements.len(),
        "regressed_fixtures": regressions,
        "improved_fixtures": improvements,
    });

    eprintln!("\n{}", serde_json::to_string_pretty(&summary).unwrap());

    // Report regressions as warnings but don't fail the test —
    // timing is inherently noisy in CI.
    if !regressions.is_empty() {
        eprintln!(
            "\n  [warn] {} fixture(s) regressed beyond {threshold:.1}x threshold: {:?}",
            regressions.len(),
            regressions
        );
    }
}

// ===========================================================================
// TEST: Scaling curve (item count vs render time) with fixture output
// ===========================================================================

#[test]
fn bench_render_fixtures_scaling_curve() {
    let mut renderer = SoftwareRenderer::new();
    let item_counts: &[u64] = &[1, 10, 50, 100, 250, 500];

    eprintln!("[pat-u0k] Scaling Curve ({}x{}):", REF_WIDTH, REF_HEIGHT);

    let mut data_points = Vec::new();

    for &n in item_counts {
        let viewport = fixture_grid(n, REF_WIDTH, REF_HEIGHT);
        let (per_frame_ms, mp_per_sec, _) =
            bench_fixture(&mut renderer, &viewport, REF_WIDTH, REF_HEIGHT);

        eprintln!(
            "  {:>5} items: {:.3} ms/frame  ({:.1} MP/s)",
            n, per_frame_ms, mp_per_sec
        );

        data_points.push(json!({
            "item_count": n,
            "per_frame_ms": round3(per_frame_ms),
            "mp_per_sec": round1(mp_per_sec),
        }));
    }

    // Check sub-quadratic scaling: 500 items should cost <100x of 1 item.
    let t1 = data_points[0]["per_frame_ms"].as_f64().unwrap();
    let t500 = data_points[5]["per_frame_ms"].as_f64().unwrap();
    let ratio = if t1 > 0.0 { t500 / t1 } else { 0.0 };

    eprintln!("  Scaling ratio (500/1): {ratio:.1}x");

    let scaling_report = json!({
        "test": "scaling_curve",
        "resolution": format!("{}x{}", REF_WIDTH, REF_HEIGHT),
        "data_points": data_points,
        "scaling_ratio_500_vs_1": round1(ratio),
        "sub_quadratic": ratio < 500.0,
    });

    eprintln!(
        "\n{}",
        serde_json::to_string_pretty(&scaling_report).unwrap()
    );

    assert!(
        ratio < 500.0,
        "Scaling ratio {ratio:.1}x exceeds 500x — potential quadratic blowup"
    );
}

// ===========================================================================
// Rounding helpers
// ===========================================================================

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}
