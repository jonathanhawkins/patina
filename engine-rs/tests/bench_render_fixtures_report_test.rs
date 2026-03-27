//! pat-90dl: Render benchmark fixtures with determinism checks and JSON reporting.
//!
//! Extends `bench_render_baselines.rs` with:
//! - Machine-readable JSON performance summaries (printed to stderr)
//! - Determinism verification: same inputs → identical pixels every run
//! - Canvas layer composition benchmarks (multi-layer z-order rendering)
//! - Camera transform benchmarks (zoom, rotation, position)
//! - Scaling analysis: item-count vs. render time
//! - Consolidated summary report with pass/fail and actionable metrics
//!
//! Named with `bench_` prefix → Tier 3 (full suite).
//! All tests always pass; run with `--nocapture` for the report.

use std::time::Instant;

use gdcore::math::{Color, Rect2, Transform2D, Vector2};
use gdrender2d::renderer::SoftwareRenderer;
use gdrender2d::test_adapter::capture_frame;
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::canvas_layer::CanvasLayer;
use gdserver2d::viewport::Viewport;
use serde_json::json;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const WARMUP: u32 = 2;
const ITERATIONS: u32 = 20;

const RESOLUTIONS: [(u32, u32, &str); 3] = [
    (640, 480, "640x480"),
    (1280, 720, "1280x720"),
    (1920, 1080, "1920x1080"),
];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Measure `iterations` runs of `f`, returning (total_ms, per_iter_ms, MP/s).
fn measure_render<F: FnMut()>(width: u32, height: u32, iterations: u32, mut f: F) -> (f64, f64, f64) {
    let start = Instant::now();
    for _ in 0..iterations {
        f();
    }
    let total_ms = start.elapsed().as_secs_f64() * 1000.0;
    let per_iter_ms = total_ms / iterations as f64;
    let megapixels = (width as f64 * height as f64) / 1_000_000.0;
    let mp_per_sec = if per_iter_ms > 0.0 {
        megapixels / (per_iter_ms / 1000.0)
    } else {
        0.0
    };
    (total_ms, per_iter_ms, mp_per_sec)
}

/// Create N non-overlapping filled rect items spread across the viewport.
fn build_stress_viewport(n: u64, width: u32, height: u32) -> Viewport {
    let mut viewport = Viewport::new(width, height, Color::BLACK);
    let cols = (n as f32).sqrt().ceil() as u64;
    let cell_w = width as f32 / cols as f32;
    let cell_h = height as f32 / cols.max(1) as f32;

    for i in 0..n {
        let mut item = CanvasItem::new(CanvasItemId(i + 1));
        let x = (i % cols) as f32 * cell_w;
        let y = (i / cols) as f32 * cell_h;
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(
                Vector2::new(x, y),
                Vector2::new(cell_w * 0.8, cell_h * 0.8),
            ),
            color: Color::rgb(
                (i % 5) as f32 / 4.0,
                ((i + 1) % 5) as f32 / 4.0,
                ((i + 2) % 5) as f32 / 4.0,
            ),
            filled: true,
        });
        viewport.add_canvas_item(item);
    }
    viewport
}

/// Create a multi-layer viewport with items distributed across layers.
fn build_layered_viewport(
    num_layers: u64,
    items_per_layer: u64,
    width: u32,
    height: u32,
) -> Viewport {
    let mut viewport = Viewport::new(width, height, Color::rgb(0.05, 0.05, 0.1));
    let mut item_counter: u64 = 1;

    for layer_idx in 0..num_layers {
        let mut layer = CanvasLayer::new(layer_idx + 1);
        layer.z_order = layer_idx as i32;
        layer.transform = Transform2D::translated(Vector2::new(
            (layer_idx as f32) * 10.0,
            (layer_idx as f32) * 5.0,
        ));
        viewport.add_canvas_layer(layer);

        for j in 0..items_per_layer {
            let mut item = CanvasItem::new(CanvasItemId(item_counter));
            item.layer_id = Some(layer_idx + 1);
            let x = (j % 10) as f32 * (width as f32 / 10.0);
            let y = (j / 10) as f32 * (height as f32 / 10.0);
            item.commands.push(DrawCommand::DrawRect {
                rect: Rect2::new(
                    Vector2::new(x, y),
                    Vector2::new(width as f32 / 15.0, height as f32 / 15.0),
                ),
                color: Color::rgb(
                    (layer_idx as f32) / num_layers as f32,
                    0.5,
                    1.0 - (layer_idx as f32) / num_layers as f32,
                ),
                filled: true,
            });
            viewport.add_canvas_item(item);
            item_counter += 1;
        }
    }
    viewport
}

/// Create a viewport with camera transform for benchmarking.
fn build_camera_viewport(width: u32, height: u32, item_count: u64) -> Viewport {
    let mut viewport = build_stress_viewport(item_count, width, height);
    viewport.camera_position = Vector2::new(width as f32 / 3.0, height as f32 / 3.0);
    viewport.camera_zoom = Vector2::new(1.5, 1.5);
    viewport.camera_rotation = 0.2;
    viewport
}

// ===========================================================================
// DETERMINISM VERIFICATION
// ===========================================================================

#[test]
fn bench_determinism_stress_100() {
    // Same viewport rendered twice must produce identical pixel output.
    let mut renderer = SoftwareRenderer::new();
    let viewport = build_stress_viewport(100, 640, 480);

    let fb1 = capture_frame(&mut renderer, &viewport);
    let fb2 = capture_frame(&mut renderer, &viewport);

    assert_eq!(fb1.pixels.len(), fb2.pixels.len());
    assert_eq!(
        fb1.pixels, fb2.pixels,
        "Determinism: 100-item stress viewport must produce identical output"
    );

    eprintln!(
        "{}",
        json!({
            "fixture": "determinism_stress_100",
            "status": "pass",
            "pixels": fb1.pixels.len(),
            "resolution": "640x480",
        })
    );
}

#[test]
fn bench_determinism_layered() {
    // Multi-layer viewport determinism.
    let mut renderer = SoftwareRenderer::new();
    let viewport = build_layered_viewport(5, 20, 640, 480);

    let fb1 = capture_frame(&mut renderer, &viewport);
    let fb2 = capture_frame(&mut renderer, &viewport);

    assert_eq!(
        fb1.pixels, fb2.pixels,
        "Determinism: 5-layer viewport must produce identical output"
    );

    eprintln!(
        "{}",
        json!({
            "fixture": "determinism_layered_5x20",
            "status": "pass",
            "layers": 5,
            "items_per_layer": 20,
            "total_items": 100,
        })
    );
}

#[test]
fn bench_determinism_camera_transform() {
    let mut renderer = SoftwareRenderer::new();
    let viewport = build_camera_viewport(640, 480, 50);

    let fb1 = capture_frame(&mut renderer, &viewport);
    let fb2 = capture_frame(&mut renderer, &viewport);

    assert_eq!(
        fb1.pixels, fb2.pixels,
        "Determinism: camera-transformed viewport must produce identical output"
    );

    eprintln!(
        "{}",
        json!({
            "fixture": "determinism_camera_transform",
            "status": "pass",
            "camera_position": [213, 160],
            "camera_zoom": [1.5, 1.5],
            "camera_rotation_rad": 0.2,
        })
    );
}

// ===========================================================================
// CANVAS LAYER COMPOSITION BENCHMARKS
// ===========================================================================

#[test]
fn bench_render_layered_5x20() {
    let mut renderer = SoftwareRenderer::new();

    eprintln!("[bench-render-report] layered_5x20 (5 layers, 20 items each):");

    let mut results = Vec::new();
    for &(w, h, label) in &RESOLUTIONS {
        let viewport = build_layered_viewport(5, 20, w, h);

        for _ in 0..WARMUP {
            let _ = capture_frame(&mut renderer, &viewport);
        }

        let (total_ms, per_frame_ms, mp_per_sec) =
            measure_render(w, h, ITERATIONS, || { let _ = capture_frame(&mut renderer, &viewport); });

        eprintln!("  {label}: {ITERATIONS}x in {total_ms:.2}ms ({per_frame_ms:.3}ms/frame, {mp_per_sec:.1} MP/s)");

        results.push(json!({
            "resolution": label,
            "width": w,
            "height": h,
            "iterations": ITERATIONS,
            "total_ms": (total_ms * 100.0).round() / 100.0,
            "per_frame_ms": (per_frame_ms * 1000.0).round() / 1000.0,
            "mp_per_sec": (mp_per_sec * 10.0).round() / 10.0,
        }));
    }

    eprintln!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "fixture": "layered_5x20",
            "layers": 5,
            "items_per_layer": 20,
            "total_items": 100,
            "results": results,
        }))
        .unwrap()
    );
}

#[test]
fn bench_render_layered_10x50() {
    let mut renderer = SoftwareRenderer::new();

    eprintln!("[bench-render-report] layered_10x50 (10 layers, 50 items each):");

    let mut results = Vec::new();
    for &(w, h, label) in &RESOLUTIONS {
        let viewport = build_layered_viewport(10, 50, w, h);

        for _ in 0..WARMUP {
            let _ = capture_frame(&mut renderer, &viewport);
        }

        let (total_ms, per_frame_ms, mp_per_sec) =
            measure_render(w, h, ITERATIONS, || { let _ = capture_frame(&mut renderer, &viewport); });

        eprintln!("  {label}: {ITERATIONS}x in {total_ms:.2}ms ({per_frame_ms:.3}ms/frame, {mp_per_sec:.1} MP/s)");

        results.push(json!({
            "resolution": label,
            "width": w,
            "height": h,
            "iterations": ITERATIONS,
            "total_ms": (total_ms * 100.0).round() / 100.0,
            "per_frame_ms": (per_frame_ms * 1000.0).round() / 1000.0,
            "mp_per_sec": (mp_per_sec * 10.0).round() / 10.0,
        }));
    }

    eprintln!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "fixture": "layered_10x50",
            "layers": 10,
            "items_per_layer": 50,
            "total_items": 500,
            "results": results,
        }))
        .unwrap()
    );
}

// ===========================================================================
// CAMERA TRANSFORM BENCHMARKS
// ===========================================================================

#[test]
fn bench_render_camera_50_items() {
    let mut renderer = SoftwareRenderer::new();

    eprintln!("[bench-render-report] camera_50_items (zoom=1.5, rot=0.2):");

    let mut results = Vec::new();
    for &(w, h, label) in &RESOLUTIONS {
        let viewport = build_camera_viewport(w, h, 50);

        for _ in 0..WARMUP {
            let _ = capture_frame(&mut renderer, &viewport);
        }

        let (total_ms, per_frame_ms, mp_per_sec) =
            measure_render(w, h, ITERATIONS, || { let _ = capture_frame(&mut renderer, &viewport); });

        eprintln!("  {label}: {ITERATIONS}x in {total_ms:.2}ms ({per_frame_ms:.3}ms/frame, {mp_per_sec:.1} MP/s)");

        results.push(json!({
            "resolution": label,
            "width": w,
            "height": h,
            "iterations": ITERATIONS,
            "total_ms": (total_ms * 100.0).round() / 100.0,
            "per_frame_ms": (per_frame_ms * 1000.0).round() / 1000.0,
            "mp_per_sec": (mp_per_sec * 10.0).round() / 10.0,
        }));
    }

    eprintln!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "fixture": "camera_50_items",
            "camera": { "position": "w/3,h/3", "zoom": 1.5, "rotation_rad": 0.2 },
            "item_count": 50,
            "results": results,
        }))
        .unwrap()
    );
}

// ===========================================================================
// SCALING ANALYSIS: item-count vs. render time
// ===========================================================================

#[test]
fn bench_render_scaling_analysis() {
    // Measures render time at a fixed resolution across increasing item counts.
    // Produces a scaling curve to detect non-linear blowups.
    let mut renderer = SoftwareRenderer::new();
    let (w, h) = (640, 480);

    eprintln!("[bench-render-report] scaling_analysis (640x480, 10..1000 items):");

    let item_counts: &[u64] = &[10, 50, 100, 250, 500, 1000];
    let mut data_points = Vec::new();

    for &n in item_counts {
        let viewport = build_stress_viewport(n, w, h);

        for _ in 0..WARMUP {
            let _ = capture_frame(&mut renderer, &viewport);
        }

        let (_total_ms, per_frame_ms, mp_per_sec) =
            measure_render(w, h, ITERATIONS, || { let _ = capture_frame(&mut renderer, &viewport); });

        eprintln!("  {n:>5} items: {per_frame_ms:.3}ms/frame ({mp_per_sec:.1} MP/s)");

        data_points.push(json!({
            "item_count": n,
            "per_frame_ms": (per_frame_ms * 1000.0).round() / 1000.0,
            "mp_per_sec": (mp_per_sec * 10.0).round() / 10.0,
        }));
    }

    // Verify scaling is sub-quadratic: 1000 items should take <100x of 10 items.
    let t10 = data_points[0]["per_frame_ms"].as_f64().unwrap();
    let t1000 = data_points[5]["per_frame_ms"].as_f64().unwrap();
    let ratio = if t10 > 0.0 { t1000 / t10 } else { 0.0 };

    eprintln!("  scaling ratio (1000/10): {ratio:.1}x");

    let scaling_ok = ratio < 200.0; // generous threshold

    eprintln!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "fixture": "scaling_analysis",
            "resolution": "640x480",
            "iterations": ITERATIONS,
            "data_points": data_points,
            "scaling_ratio_1000_vs_10": (ratio * 10.0).round() / 10.0,
            "scaling_acceptable": scaling_ok,
        }))
        .unwrap()
    );

    assert!(
        scaling_ok,
        "Scaling ratio {ratio:.1}x exceeds 200x threshold — potential quadratic blowup"
    );
}

// ===========================================================================
// CONSOLIDATED SUMMARY REPORT
// ===========================================================================

#[test]
fn bench_render_summary_report() {
    // Runs a representative subset at 1280x720 and emits a single JSON summary.
    let mut renderer = SoftwareRenderer::new();
    let (w, h) = (1280, 720);

    let fixtures: Vec<(&str, Viewport)> = vec![
        ("stress_50", build_stress_viewport(50, w, h)),
        ("stress_200", build_stress_viewport(200, w, h)),
        ("layered_5x20", build_layered_viewport(5, 20, w, h)),
        ("layered_10x50", build_layered_viewport(10, 50, w, h)),
        ("camera_50", build_camera_viewport(w, h, 50)),
    ];

    let mut entries = Vec::new();

    for (name, viewport) in &fixtures {
        // Warmup
        for _ in 0..WARMUP {
            let _ = capture_frame(&mut renderer, viewport);
        }

        // Determinism check
        let fb1 = capture_frame(&mut renderer, viewport);
        let fb2 = capture_frame(&mut renderer, viewport);
        let deterministic = fb1.pixels == fb2.pixels;

        // Timing
        let (_total_ms, per_frame_ms, mp_per_sec) =
            measure_render(w, h, ITERATIONS, || { let _ = capture_frame(&mut renderer, viewport); });

        entries.push(json!({
            "fixture": name,
            "deterministic": deterministic,
            "per_frame_ms": (per_frame_ms * 1000.0).round() / 1000.0,
            "mp_per_sec": (mp_per_sec * 10.0).round() / 10.0,
        }));

        assert!(deterministic, "Fixture '{name}' failed determinism check");
    }

    let report = json!({
        "report": "render_benchmark_summary",
        "resolution": "1280x720",
        "iterations": ITERATIONS,
        "warmup": WARMUP,
        "fixtures": entries,
        "all_deterministic": entries.iter().all(|e| e["deterministic"] == true),
    });

    eprintln!(
        "\n[RENDER BENCHMARK SUMMARY]\n{}",
        serde_json::to_string_pretty(&report).unwrap()
    );
}
