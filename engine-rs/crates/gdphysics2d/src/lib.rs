//! # gdphysics2d
//!
//! 2D physics implementation for the Patina Engine runtime.
//!
//! Provides collision shapes, rigid/static/kinematic bodies, narrow-phase
//! collision detection with resolution, a physics world with simulation
//! stepping, and raycasting.
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

// Re-export key types for convenience.
pub use body::{BodyId, BodyType, PhysicsBody2D};
pub use collision::CollisionResult;
pub use shape::Shape2D;
pub use world::{PhysicsWorld2D, RaycastHit};
