//! # gdphysics3d
//!
//! 3D physics implementation for the Patina Engine runtime.
//!
//! Provides collision shapes, rigid/static/kinematic bodies, narrow-phase
//! collision detection with resolution, a physics world with simulation
//! stepping, and raycasting.
//!
//! ## Quick start
//!
//! ```
//! use gdphysics3d::world::PhysicsWorld3D;
//! use gdphysics3d::body::{PhysicsBody3D, BodyId3D, BodyType3D};
//! use gdphysics3d::shape::Shape3D;
//! use gdcore::math::Vector3;
//!
//! let mut world = PhysicsWorld3D::new();
//! let body = PhysicsBody3D::new(
//!     BodyId3D(0),
//!     BodyType3D::Rigid,
//!     Vector3::new(0.0, 10.0, 0.0),
//!     Shape3D::Sphere { radius: 1.0 },
//!     1.0,
//! );
//! let id = world.add_body(body);
//! world.step(1.0 / 60.0);
//! ```

#![warn(clippy::all)]

pub mod area3d;
pub mod body;
pub mod character;
pub mod collision;
pub mod joint;
pub mod query;
pub mod shape;
pub mod world;

pub use area3d::{
    Area3D, AreaId3D, AreaOverlapEvent3D, AreaStore3D, OverlapEvent3D, OverlapState3D,
    SpaceOverride,
};
pub use body::{BodyId3D, BodyType3D, ContactPoint3D, FreezeMode, PhysicsBody3D};
pub use character::CharacterBody3D;
pub use collision::CollisionResult3D;
pub use query::{PhysicsRayQuery3D, PhysicsShapeQuery3D, ShapeQueryResult3D};
pub use shape::Shape3D;
pub use world::PhysicsWorld3D;
