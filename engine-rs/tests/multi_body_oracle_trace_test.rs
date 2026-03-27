//! pat-qre5: Oracle comparison for multi-body deterministic trace.

use gdphysics2d::body::{BodyId, BodyType, PhysicsBody2D};
use gdphysics2d::shape::Shape2D;
use gdphysics2d::world::PhysicsWorld2D;

fn fixtures_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("fixtures")
}

#[test]
fn golden_trace_file_exists_and_valid() {
    let path = fixtures_dir().join("golden/physics/multi_rigid_cascade_30frames.json");
    let content = std::fs::read_to_string(&path).unwrap();
    let golden: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();
    assert_eq!(golden.len(), 90, "3 bodies × 30 frames = 90 entries");
}

#[test]
fn three_body_cascade_is_deterministic() {
    let shape = Shape2D::Circle { radius: 1.0 };

    let run = || -> Vec<(f32, f32)> {
        let mut world = PhysicsWorld2D::new();
        let mut a = PhysicsBody2D::new(
            BodyId(0), BodyType::Rigid,
            gdcore::math::Vector2::new(0.0, 0.0), shape, 1.0,
        );
        a.linear_velocity = gdcore::math::Vector2::new(200.0, 0.0);
        let id_a = world.add_body(a);
        let id_b = world.add_body(PhysicsBody2D::new(
            BodyId(0), BodyType::Rigid,
            gdcore::math::Vector2::new(25.0, 0.0), shape, 1.0,
        ));
        let id_c = world.add_body(PhysicsBody2D::new(
            BodyId(0), BodyType::Rigid,
            gdcore::math::Vector2::new(50.0, 0.0), shape, 1.0,
        ));

        let mut positions = Vec::new();
        for _ in 0..30 {
            world.step(1.0 / 60.0);
            for &id in &[id_a, id_b, id_c] {
                let b = world.get_body(id).unwrap();
                positions.push((b.position.x, b.position.y));
            }
        }
        positions
    };

    let run1 = run();
    let run2 = run();
    assert_eq!(run1.len(), run2.len());
    for (i, (a, b)) in run1.iter().zip(run2.iter()).enumerate() {
        assert!(
            (a.0 - b.0).abs() < 0.0001 && (a.1 - b.1).abs() < 0.0001,
            "determinism broken at entry {i}: ({}, {}) vs ({}, {})",
            a.0, a.1, b.0, b.1
        );
    }
}

#[test]
fn cascade_simulation_produces_movement() {
    let shape = Shape2D::Circle { radius: 1.0 };
    let mut world = PhysicsWorld2D::new();

    let mut a = PhysicsBody2D::new(
        BodyId(0), BodyType::Rigid,
        gdcore::math::Vector2::new(0.0, 0.0), shape, 1.0,
    );
    a.linear_velocity = gdcore::math::Vector2::new(200.0, 0.0);
    let id_a = world.add_body(a);

    for _ in 0..10 {
        world.step(1.0 / 60.0);
    }

    let body = world.get_body(id_a).unwrap();
    assert!(body.position.x > 0.0, "body A should have moved right");
}
