//! Benchmark suite for the Patina Engine.
//!
//! Runs performance benchmarks for key subsystems and prints results as JSON.
//! Uses `std::time::Instant` (no external dependencies).
//!
//! Run with: `cargo run --example benchmarks`

use std::time::Instant;

use gdcore::math::{Color, Rect2, Transform2D, Vector2, Vector3};
use gdphysics2d::body::{BodyId, BodyType, PhysicsBody2D};
use gdphysics2d::body3d::{BodyId3D, BodyType3D, PhysicsBody3D};
use gdphysics2d::shape::Shape2D;
use gdphysics2d::shape3d::Shape3D;
use gdphysics2d::world::PhysicsWorld2D;
use gdphysics2d::world3d::PhysicsWorld3D;
use gdrender2d::test_adapter::capture_frame;
use gdrender2d::SoftwareRenderer;
use gdscene::PackedScene;
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::viewport::Viewport;
use gdvariant::serialize::{from_json, to_json};
use gdvariant::Variant;

/// Number of iterations per benchmark for averaging.
const ITERATIONS: u32 = 100;

struct BenchResult {
    name: &'static str,
    iterations: u32,
    total_us: u128,
    avg_us: u128,
}

impl BenchResult {
    fn to_json(&self) -> String {
        format!(
            r#"    {{ "name": "{}", "iterations": {}, "total_us": {}, "avg_us": {} }}"#,
            self.name, self.iterations, self.total_us, self.avg_us
        )
    }
}

fn bench<F: FnMut()>(name: &'static str, iterations: u32, mut f: F) -> BenchResult {
    // Warm up.
    f();

    let start = Instant::now();
    for _ in 0..iterations {
        f();
    }
    let elapsed = start.elapsed();
    let total_us = elapsed.as_micros();

    BenchResult {
        name,
        iterations,
        total_us,
        avg_us: total_us / iterations as u128,
    }
}

fn bench_scene_load() -> BenchResult {
    let tscn = include_str!("../fixtures/scenes/demo_2d.tscn");
    bench("scene_load", ITERATIONS, || {
        let _ = PackedScene::from_tscn(tscn).unwrap();
    })
}

fn bench_resource_load() -> BenchResult {
    let tres = r#"[gd_resource type="Resource" format=3]

[resource]
name = "BenchResource"
value = 42
position = Vector2(10, 20)
flag = true
"#;
    let loader = gdresource::TresLoader::new();
    bench("resource_load", ITERATIONS, || {
        let _ = loader.parse_str(tres, "bench://resource.tres").unwrap();
    })
}

fn bench_physics_step_2d() -> BenchResult {
    bench("physics_step_2d", ITERATIONS, || {
        let mut world = PhysicsWorld2D::new();

        // Add 100 rigid circle bodies scattered across the scene.
        for i in 0..100u64 {
            let x = (i % 10) as f32 * 50.0;
            let y = (i / 10) as f32 * 50.0;
            let mut body = PhysicsBody2D::new(
                BodyId(0),
                BodyType::Rigid,
                Vector2::new(x, y),
                Shape2D::Circle { radius: 5.0 },
                1.0,
            );
            body.linear_velocity = Vector2::new(
                ((i as f32) * 1.3).sin() * 50.0,
                ((i as f32) * 0.7).cos() * 50.0,
            );
            world.add_body(body);
        }

        // Simulate 60 frames at 60 Hz.
        for _ in 0..60 {
            world.step(1.0 / 60.0);
        }
    })
}

fn bench_physics_step_3d() -> BenchResult {
    bench("physics_step_3d", ITERATIONS, || {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO; // Disable gravity for controlled benchmark.

        for i in 0..100u64 {
            let x = (i % 10) as f32 * 10.0;
            let y = ((i / 10) % 10) as f32 * 10.0;
            let z = (i / 100) as f32 * 10.0;
            let mut body = PhysicsBody3D::new(
                BodyId3D(0),
                BodyType3D::Rigid,
                Vector3::new(x, y, z),
                Shape3D::Sphere { radius: 2.0 },
                1.0,
            );
            body.linear_velocity = Vector3::new(
                ((i as f32) * 1.3).sin() * 10.0,
                ((i as f32) * 0.7).cos() * 10.0,
                ((i as f32) * 2.1).sin() * 10.0,
            );
            world.add_body(body);
        }

        for _ in 0..60 {
            world.step(1.0 / 60.0);
        }
    })
}

fn bench_variant_conversion() -> BenchResult {
    // Build a representative set of variants.
    let variants: Vec<Variant> = vec![
        Variant::Nil,
        Variant::Bool(true),
        Variant::Int(42),
        Variant::Float(3.14),
        Variant::String("hello world".into()),
        Variant::Vector2(Vector2::new(1.0, 2.0)),
        Variant::Vector3(Vector3::new(1.0, 2.0, 3.0)),
        Variant::Color(Color::new(1.0, 0.5, 0.25, 0.8)),
        Variant::Array(vec![Variant::Int(1), Variant::Int(2), Variant::Int(3)]),
        Variant::Nil, // pad to 10
    ];

    bench("variant_conversion", ITERATIONS, || {
        // 1000 roundtrips (100 iterations of 10 variants each).
        for _ in 0..100 {
            for v in &variants {
                let json = to_json(v);
                let _ = from_json(&json);
            }
        }
    })
}

fn bench_render_frame_2d() -> BenchResult {
    bench("render_frame_2d", ITERATIONS, || {
        let mut renderer = SoftwareRenderer::new();
        let mut viewport = Viewport::new(640, 480, Color::BLACK);

        // Create 100 canvas items with draw commands.
        for i in 0..100u64 {
            let x = (i % 10) as f32 * 60.0;
            let y = (i / 10) as f32 * 45.0;
            let mut item = CanvasItem::new(CanvasItemId(i + 1));
            item.transform = Transform2D::translated(Vector2::new(x, y));
            item.z_index = i as i32;
            if i % 2 == 0 {
                item.commands.push(DrawCommand::DrawRect {
                    rect: Rect2::new(Vector2::new(-10.0, -10.0), Vector2::new(20.0, 20.0)),
                    color: Color::rgb(1.0, 0.0, 0.0),
                    filled: true,
                });
            } else {
                item.commands.push(DrawCommand::DrawCircle {
                    center: Vector2::ZERO,
                    radius: 10.0,
                    color: Color::rgb(0.0, 0.0, 1.0),
                });
            }
            viewport.add_canvas_item(item);
        }

        let _ = capture_frame(&mut renderer, &viewport);
    })
}

fn main() {
    let results = vec![
        bench_scene_load(),
        bench_resource_load(),
        bench_physics_step_2d(),
        bench_physics_step_3d(),
        bench_variant_conversion(),
        bench_render_frame_2d(),
    ];

    // Print as JSON.
    println!("{{");
    println!(r#"  "engine": "patina","#);
    println!(r#"  "timestamp": "{}","#, chrono_lite());
    println!(r#"  "iterations_per_bench": {ITERATIONS},"#);
    println!(r#"  "benchmarks": ["#);
    for (i, r) in results.iter().enumerate() {
        let comma = if i + 1 < results.len() { "," } else { "" };
        println!("{}{comma}", r.to_json());
    }
    println!("  ]");
    println!("}}");
}

/// Lightweight timestamp without pulling in chrono.
fn chrono_lite() -> String {
    use std::time::SystemTime;
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => format!("unix:{}", d.as_secs()),
        Err(_) => "unknown".into(),
    }
}
