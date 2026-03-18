//! # gdphysics2d
//!
//! 2D and 3D physics implementation for the Patina Engine runtime.
//!
//! Provides collision shapes, rigid/static/kinematic bodies, narrow-phase
//! collision detection with resolution, a physics world with simulation
//! stepping, and raycasting for both 2D and 3D.
//!
//! ## Quick start
//!
//! ```
//! use gdphysics2d::world::PhysicsWorld2D;
//! use gdphysics2d::body::{PhysicsBody2D, BodyId, BodyType};
//! use gdphysics2d::shape::Shape2D;
//! use gdcore::math::Vector2;
//!
//! let mut world = PhysicsWorld2D::new();
//! let body = PhysicsBody2D::new(
//!     BodyId(0),
//!     BodyType::Rigid,
//!     Vector2::new(0.0, 0.0),
//!     Shape2D::Circle { radius: 1.0 },
//!     1.0,
//! );
//! let id = world.add_body(body);
//! world.step(1.0 / 60.0);
//! ```

#![warn(clippy::all)]

pub mod body;
pub mod collision;
pub mod shape;
pub mod test_harness;
pub mod world;

// 3D physics modules.
pub mod body3d;
pub mod collision3d;
pub mod shape3d;
pub mod world3d;

// Re-export key types for convenience.
pub use body::{BodyId, BodyType, PhysicsBody2D};
pub use collision::CollisionResult;
pub use shape::Shape2D;
pub use world::{PhysicsWorld2D, RaycastHit};

// 3D re-exports.
pub use body3d::{BodyId3D, BodyType3D, PhysicsBody3D};
pub use collision3d::CollisionResult3D;
pub use shape3d::Shape3D;
pub use world3d::{PhysicsWorld3D, RaycastHit3D};
