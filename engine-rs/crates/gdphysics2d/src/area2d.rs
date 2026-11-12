//! Area2D overlap detection.
//!
//! Provides [`Area2D`] for detecting when physics bodies enter or exit a
//! spatial region, without applying physics forces. Useful for triggers,
//! damage zones, and spatial queries.

use std::collections::{HashMap, HashSet};

use gdcore::math::{Transform2D, Vector2};

use crate::body::{BodyId, PhysicsBody2D, ShapeOwner2D};
use crate::collision;
use crate::shape::Shape2D;

/// Unique identifier for an area within a [`PhysicsWorld2D`](crate::world::PhysicsWorld2D).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AreaId(pub u64);

/// Whether a body just entered or exited an area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlapState {
    /// The body entered the area this frame.
    Entered,
    /// The body exited the area this frame.
    Exited,
}

/// How an [`Area2D`] combines its gravity/damp with lower-priority areas and the
/// world default, matching Godot's `Area2D.SpaceOverride` enum.
///
/// The discriminant order mirrors Godot's enum
/// (`SPACE_OVERRIDE_DISABLED = 0` … `SPACE_OVERRIDE_REPLACE_COMBINE = 4`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AreaSpaceOverride {
    /// This area does not affect gravity/damping (`SPACE_OVERRIDE_DISABLED`).
    #[default]
    Disabled,
    /// Add this area's value to whatever was computed so far
    /// (`SPACE_OVERRIDE_COMBINE`).
    Combine,
    /// Add this area's value, then stop processing lower-priority areas and the
    /// default (`SPACE_OVERRIDE_COMBINE_REPLACE`).
    CombineReplace,
    /// Replace everything so far with this area's value, then stop
    /// (`SPACE_OVERRIDE_REPLACE`).
    Replace,
    /// Replace everything so far with this area's value, but keep processing
    /// lower-priority areas (`SPACE_OVERRIDE_REPLACE_COMBINE`).
    ReplaceCombine,
}

/// An overlap event between an area and a body.
#[derive(Debug, Clone, Copy)]
pub struct OverlapEvent {
    /// The area involved.
    pub area_id: AreaId,
    /// The body that entered or exited.
    pub body_id: BodyId,
    /// Whether the body entered or exited.
    pub state: OverlapState,
}

/// An overlap event between two areas.
///
/// The pair is *directional*: `area_id` is the monitoring area that detected
/// `other_area_id`. `(A, B)` and `(B, A)` are independent events, matching
/// Godot's `Area2D.area_entered(area)` / `area_exited(area)` contract where each
/// area reports the other from its own perspective.
#[derive(Debug, Clone, Copy)]
pub struct AreaOverlapEvent {
    /// The monitoring area that detected the overlap.
    pub area_id: AreaId,
    /// The other area that entered or exited `area_id`.
    pub other_area_id: AreaId,
    /// Whether the other area entered or exited.
    pub state: OverlapState,
}

/// A 2D area used for overlap detection (no physics response).
#[derive(Debug, Clone)]
pub struct Area2D {
    /// Unique identifier.
    pub id: AreaId,
    /// World-space position.
    pub position: Vector2,
    /// Collision shape in local space.
    pub shape: Shape2D,
    /// The collider child's own transform relative to the area — Godot's
    /// shape-owner transform, the `Area2D` twin of
    /// [`PhysicsBody2D::shape_transform`](crate::body::PhysicsBody2D::shape_transform)
    /// (pat-s3rrv.25).
    ///
    /// Bodies honoured their collider's offset before areas did, which left the
    /// engine inconsistent in the most confusing possible way: a body and an
    /// area authored with the same offset collider would disagree about where
    /// they were. Rotation and translation only — scale stays baked into the
    /// geometry by `scale_shape`.
    pub shape_transform: Transform2D,
    /// Collider children beyond the first, as [`ShapeOwner2D`]s carrying each
    /// child's local transform — the `Area2D` twin of
    /// [`PhysicsBody2D::extra_shapes`](crate::body::PhysicsBody2D::extra_shapes)
    /// (pat-s3rrv.25).
    ///
    /// Bodies got their list first, which briefly left an area with two
    /// colliders monitoring through only one while a body with two was solid on
    /// both. This closes that.
    ///
    /// The owner's one-way fields are carried for shape only: an area never
    /// blocks anything, so nothing on the area path reads them.
    pub extra_shapes: Vec<ShapeOwner2D>,
    /// Collision layer bitmask — which layers this area occupies.
    pub collision_layer: u32,
    /// Collision mask bitmask — which layers this area scans for overlaps.
    pub collision_mask: u32,
    /// Whether this area is actively detecting overlaps.
    pub monitoring: bool,

    /// How this area overrides gravity in the space it occupies
    /// (Godot `gravity_space_override`). Defaults to
    /// [`AreaSpaceOverride::Disabled`].
    pub gravity_space_override: AreaSpaceOverride,
    /// Gravity magnitude in px/s² (Godot `gravity`). Defaults to `980.0`.
    pub gravity: f32,
    /// Directional gravity's unit direction (Godot `gravity_direction`).
    /// Only used when [`gravity_is_point`](Self::gravity_is_point) is `false`.
    /// Defaults to `(0, 1)` (down).
    pub gravity_direction: Vector2,
    /// Whether gravity points toward [`gravity_point_center`](Self::gravity_point_center)
    /// instead of a fixed direction (Godot `gravity_point`). Defaults to `false`.
    pub gravity_is_point: bool,
    /// World-space attractor point for point gravity
    /// (Godot `gravity_point_center`, resolved to world space). Defaults to
    /// [`Vector2::ZERO`].
    pub gravity_point_center: Vector2,
    /// Distance (px) at which point gravity equals [`gravity`](Self::gravity)
    /// (Godot `gravity_point_unit_distance`). `0.0` disables distance falloff.
    pub gravity_point_unit_distance: f32,
    /// How this area overrides linear damping in the space it occupies
    /// (Godot `linear_damp_space_override`). Defaults to
    /// [`AreaSpaceOverride::Disabled`].
    pub linear_damp_space_override: AreaSpaceOverride,
    /// Linear damping coefficient applied to bodies in this area
    /// (Godot `linear_damp`). Defaults to `0.1`.
    pub linear_damp: f32,
    /// Processing priority: higher-priority areas are applied first when
    /// combining overrides (Godot `priority`). Defaults to `0`.
    pub priority: i32,

    // -- pat-0q2kx.112 -----------------------------------------------------
    /// This area's own rotation, in radians.
    ///
    /// Overlap tests previously built a translation-only transform, so a
    /// rotated area was tested as though it were axis-aligned — a 45° box
    /// caught bodies it should have missed and missed ones it should have
    /// caught.
    pub rotation: f32,
    /// Whether OTHER areas can detect this one (Godot `monitorable`).
    ///
    /// Distinct from [`monitoring`](Self::monitoring), which is whether *this*
    /// area detects others. Godot gates area-vs-area overlap on the target
    /// being monitorable.
    pub monitorable: bool,
    /// How this area overrides angular damping (Godot
    /// `angular_damp_space_override`) — the rotational twin of
    /// [`linear_damp_space_override`](Self::linear_damp_space_override).
    pub angular_damp_space_override: AreaSpaceOverride,
    /// Angular damping coefficient applied to bodies in this area
    /// (Godot `angular_damp`). Defaults to `1.0`.
    pub angular_damp: f32,
}

impl Area2D {
    /// Creates a new area with the given properties.
    pub fn new(id: AreaId, position: Vector2, shape: Shape2D) -> Self {
        Self {
            id,
            position,
            shape,
            shape_transform: Transform2D::IDENTITY,
            extra_shapes: Vec::new(),
            collision_layer: 1,
            collision_mask: 1,
            monitoring: true,
            gravity_space_override: AreaSpaceOverride::Disabled,
            gravity: 980.0,
            gravity_direction: Vector2::new(0.0, 1.0),
            gravity_is_point: false,
            gravity_point_center: Vector2::ZERO,
            gravity_point_unit_distance: 0.0,
            linear_damp_space_override: AreaSpaceOverride::Disabled,
            linear_damp: 0.1,
            priority: 0,
            rotation: 0.0,
            // Godot's Area2D is monitorable by default.
            monitorable: true,
            angular_damp_space_override: AreaSpaceOverride::Disabled,
            angular_damp: 1.0,
        }
    }

    /// Every collision shape this area owns, each with its **world** transform:
    /// the primary shape first, then [`extra_shapes`](Self::extra_shapes)
    /// (pat-s3rrv.25).
    ///
    /// The `Area2D` twin of `PhysicsBody2D::shape_owners`. Its first element is
    /// exactly what [`transform`](Self::transform) pairs with `shape`, so
    /// converting a call site cannot change single-collider behaviour.
    pub fn shape_owners(&self) -> Vec<ShapeOwner2D> {
        let base = Transform2D::from_rotation_origin(self.rotation, self.position);
        let mut out = Vec::with_capacity(1 + self.extra_shapes.len());
        out.push(ShapeOwner2D::solid(self.shape, base * self.shape_transform));
        for extra in &self.extra_shapes {
            out.push(ShapeOwner2D {
                transform: base * extra.transform,
                ..*extra
            });
        }
        out
    }

    /// This area's world transform, honouring its rotation (pat-0q2kx.112) and
    /// its collider child's own offset (pat-s3rrv.25).
    ///
    /// Every overlap test in this module places the area's shape through here,
    /// so composing the shape transform once covers body-vs-area and
    /// area-vs-area alike. Order is Godot's: `a * b` applies `b` first.
    pub fn transform(&self) -> Transform2D {
        Transform2D::from_rotation_origin(self.rotation, self.position) * self.shape_transform
    }

    /// Returns this area's gravity contribution at `body_pos`.
    ///
    /// For point gravity the result points from `body_pos` toward
    /// [`gravity_point_center`](Self::gravity_point_center); when
    /// [`gravity_point_unit_distance`](Self::gravity_point_unit_distance) is
    /// positive the magnitude falls off with the inverse square of distance
    /// (matching Godot's `godot_area_2d.cpp`). For directional gravity the
    /// result is `gravity_direction.normalized() * gravity`.
    pub fn compute_gravity(&self, body_pos: Vector2) -> Vector2 {
        if self.gravity_is_point {
            let to_center = self.gravity_point_center - body_pos;
            let dist = to_center.length();
            if dist <= 1e-6 {
                return Vector2::ZERO;
            }
            let dir = to_center / dist;
            let mut mag = self.gravity;
            if self.gravity_point_unit_distance > 0.0 {
                let ratio = self.gravity_point_unit_distance / dist;
                mag *= ratio * ratio;
            }
            dir * mag
        } else {
            // `normalized()` returns ZERO for a zero-length direction, so this
            // naturally guards against a degenerate `gravity_direction`.
            self.gravity_direction.normalized() * self.gravity
        }
    }
}

/// Stores areas and tracks overlaps across frames.
pub struct AreaStore {
    areas: HashMap<AreaId, Area2D>,
    next_id: u64,
    /// Previous frame overlaps: (area_id, body_id) pairs.
    previous_overlaps: HashSet<(AreaId, BodyId)>,
    /// Previous frame area-area overlaps: directional (monitor, other) pairs.
    previous_area_overlaps: HashSet<(AreaId, AreaId)>,
}

impl AreaStore {
    /// Creates an empty area store.
    pub fn new() -> Self {
        Self {
            areas: HashMap::new(),
            next_id: 1,
            previous_overlaps: HashSet::new(),
            previous_area_overlaps: HashSet::new(),
        }
    }

    /// Adds an area and returns its unique ID.
    pub fn add_area(&mut self, mut area: Area2D) -> AreaId {
        let id = AreaId(self.next_id);
        self.next_id += 1;
        area.id = id;
        self.areas.insert(id, area);
        id
    }

    /// Removes an area by ID.
    pub fn remove_area(&mut self, id: AreaId) -> Option<Area2D> {
        self.areas.remove(&id)
    }

    /// Returns a reference to an area by ID.
    pub fn get_area(&self, id: AreaId) -> Option<&Area2D> {
        self.areas.get(&id)
    }

    /// Returns a mutable reference to an area by ID.
    pub fn get_area_mut(&mut self, id: AreaId) -> Option<&mut Area2D> {
        self.areas.get_mut(&id)
    }

    /// Returns the number of areas in the store.
    pub fn area_count(&self) -> usize {
        self.areas.len()
    }

    /// Every area whose shape contains `world_point`, smallest id first
    /// (pat-0q2kx.138).
    ///
    /// The area-side twin of [`PhysicsWorld2D::point_query`], which walks bodies
    /// only. Godot's picking is `intersect_point` with
    /// `collide_with_areas = true`, and an `Area2D` is the *usual* pickable —
    /// a click target is far more often a trigger volume than a solid body — so
    /// without this, physics picking finds nothing for the common case.
    ///
    /// [`PhysicsWorld2D::point_query`]: crate::world::PhysicsWorld2D::point_query
    /// Like [`point_query`](Self::point_query) but also reports **which**
    /// collider of each area contained the point — the `shape_idx` Godot passes
    /// to `_input_event` / `_mouse_shape_enter` (pat-s3rrv.25).
    ///
    /// First matching collider per area, matching the body-side rule: Godot
    /// delivers one `shape_idx` per object per event.
    pub fn point_query_with_shape(&self, world_point: Vector2) -> Vec<(AreaId, usize)> {
        let mut hits: Vec<(AreaId, usize)> = self
            .areas
            .iter()
            .filter_map(|(&id, area)| {
                area.shape_owners()
                    .into_iter()
                    .position(|owner| {
                        owner
                            .shape
                            .contains_point(owner.transform.affine_inverse().xform(world_point))
                    })
                    .map(|idx| (id, idx))
            })
            .collect();
        hits.sort_by_key(|(id, _)| id.0);
        hits
    }

    pub fn point_query(&self, world_point: Vector2) -> Vec<AreaId> {
        let mut hits: Vec<AreaId> = self
            .areas
            .iter()
            .filter_map(|(&id, area)| {
                // World -> local through `shape_owners`, so a rotated area picks
                // on its rotated shape AND an area with several colliders — or
                // one authored at an offset — picks on all of them.
                //
                // ⚠️ This used to rebuild the transform by hand as
                // `rotated(rotation)` + origin, which silently dropped
                // `shape_transform`. Area picking therefore ignored an authored
                // collider offset even after overlap detection honoured it
                // (pat-s3rrv.25) — the same area answered two different
                // questions about where it was.
                area.shape_owners()
                    .into_iter()
                    .any(|owner| {
                        owner
                            .shape
                            .contains_point(owner.transform.affine_inverse().xform(world_point))
                    })
                    .then_some(id)
            })
            .collect();
        // `AreaId` is not `Ord`; sort by its inner id for a deterministic order.
        hits.sort_by_key(|id| id.0);
        hits
    }

    /// Detects overlaps between all areas and the given bodies.
    ///
    /// Returns enter/exit events by comparing against the previous frame.
    ///
    /// Areas with `monitoring == false` are skipped entirely: they produce no
    /// enter events and their previous-overlap entries are silently cleared so
    /// that re-enabling monitoring later correctly fires `Entered` for any
    /// bodies already inside the region.
    pub fn detect_overlaps(
        &mut self,
        bodies: &HashMap<BodyId, PhysicsBody2D>,
    ) -> Vec<OverlapEvent> {
        let mut current_overlaps = HashSet::new();
        let mut events = Vec::new();

        // Collect the set of area IDs that are not monitoring so we can
        // suppress exit events and clean up stale previous-overlap entries.
        let non_monitoring: HashSet<AreaId> = self
            .areas
            .values()
            .filter(|a| !a.monitoring)
            .map(|a| a.id)
            .collect();

        for area in self.areas.values() {
            if !area.monitoring {
                continue;
            }

            // Constant per area, so computed outside the body loop.
            let area_owners = area.shape_owners();

            for body in bodies.values() {
                // Layer/mask filtering
                if (area.collision_mask & body.collision_layer) == 0 {
                    continue;
                }

                // Every collider the body owns, so a body whose SECOND
                // CollisionShape2D is the one inside the area still registers
                // (pat-s3rrv.25). Matches the body-vs-body loop in `world.rs`;
                // an Area2D and a StaticBody2D must not disagree about whether a
                // two-collider body is touching them.
                //
                // The area side iterates its own owners too, so a two-collider
                // Area2D monitors through both.
                let overlaps = area_owners.iter().any(|area_owner| {
                    body.shape_owners().into_iter().any(|body_owner| {
                        matches!(
                            collision::test_collision(
                                &area_owner.shape,
                                &area_owner.transform,
                                &body_owner.shape,
                                &body_owner.transform,
                            ),
                            Some(r) if r.colliding && r.depth > 0.0
                        )
                    })
                });
                if overlaps {
                    current_overlaps.insert((area.id, body.id));
                }
            }
        }

        // Detect entered
        for &pair in &current_overlaps {
            if !self.previous_overlaps.contains(&pair) {
                events.push(OverlapEvent {
                    area_id: pair.0,
                    body_id: pair.1,
                    state: OverlapState::Entered,
                });
            }
        }

        // Detect exited — only for areas that are still monitoring.
        // Non-monitoring areas silently drop their previous state.
        for &pair in &self.previous_overlaps {
            if non_monitoring.contains(&pair.0) {
                // Area stopped monitoring — silently forget the overlap.
                continue;
            }
            if !current_overlaps.contains(&pair) {
                events.push(OverlapEvent {
                    area_id: pair.0,
                    body_id: pair.1,
                    state: OverlapState::Exited,
                });
            }
        }

        self.previous_overlaps = current_overlaps;
        events
    }

    /// Returns the monitoring areas overlapping `body` (respecting layer/mask),
    /// sorted by [`priority`](Area2D::priority) ascending.
    ///
    /// Uses the same collision test as [`detect_overlaps`](Self::detect_overlaps)
    /// (`colliding && depth > 0`) so gravity/damp overrides and enter/exit
    /// events agree on what "inside" means.
    fn overlapping_areas_sorted(&self, body: &PhysicsBody2D) -> Vec<&Area2D> {
        // Computed once rather than per area: the owner list is the same for
        // every area tested against this body (pat-s3rrv.25).
        let owners = body.shape_owners();
        let mut overlapping: Vec<&Area2D> = self
            .areas
            .values()
            .filter(|area| {
                if !area.monitoring {
                    return false;
                }
                if (area.collision_mask & body.collision_layer) == 0 {
                    return false;
                }
                area.shape_owners().into_iter().any(|area_owner| {
                    owners.iter().any(|body_owner| {
                        matches!(
                            collision::test_collision(
                                &area_owner.shape,
                                &area_owner.transform,
                                &body_owner.shape,
                                &body_owner.transform,
                            ),
                            Some(r) if r.colliding && r.depth > 0.0
                        )
                    })
                })
            })
            .collect();
        overlapping.sort_by_key(|a| a.priority);
        overlapping
    }

    /// Computes the effective gravity acting on `body`, faithful to Godot's
    /// `GodotBody2D::integrate_forces` area-override loop.
    ///
    /// Overlapping monitoring areas are visited from highest to lowest priority;
    /// each area's [`gravity_space_override`](Area2D::gravity_space_override)
    /// combines/replaces the accumulated gravity. When no area with a `Replace`
    /// or `CombineReplace` override stops the loop, `default_gravity` is added.
    ///
    /// The returned vector is the total gravity *before* the body's own
    /// `gravity_scale` is applied (the integrator multiplies by `gravity_scale`,
    /// as Godot does).
    pub fn compute_body_gravity(&self, body: &PhysicsBody2D, default_gravity: Vector2) -> Vector2 {
        let overlapping = self.overlapping_areas_sorted(body);
        let mut gravity = Vector2::ZERO;
        let mut stopped = false;
        // Highest priority first.
        for area in overlapping.iter().rev() {
            match area.gravity_space_override {
                AreaSpaceOverride::Disabled => continue,
                AreaSpaceOverride::Combine => {
                    gravity = gravity + area.compute_gravity(body.position);
                }
                AreaSpaceOverride::CombineReplace => {
                    gravity = gravity + area.compute_gravity(body.position);
                    stopped = true;
                }
                AreaSpaceOverride::Replace => {
                    gravity = area.compute_gravity(body.position);
                    stopped = true;
                }
                AreaSpaceOverride::ReplaceCombine => {
                    gravity = area.compute_gravity(body.position);
                }
            }
            if stopped {
                break;
            }
        }
        if !stopped {
            gravity = gravity + default_gravity;
        }
        gravity
    }

    /// Computes the effective linear damping acting on `body`, mirroring
    /// [`compute_body_gravity`](Self::compute_body_gravity) but for the scalar
    /// [`linear_damp_space_override`](Area2D::linear_damp_space_override).
    ///
    /// `default_damp` is the baseline damping (typically the body's own
    /// `linear_damp`) added when no area stops the override chain.
    /// The angular-damp equivalent of
    /// [`compute_body_damp`](Self::compute_body_damp) (pat-0q2kx.112).
    ///
    /// Godot's `angular_damp_space_override` combines exactly as the linear one
    /// does — same priority order, same Combine / CombineReplace / Replace /
    /// ReplaceCombine semantics — it just feeds the body's *rotational* damping.
    /// Patina had the linear half only, so an area could slow a body's travel
    /// but never its spin.
    pub fn compute_body_angular_damp(&self, body: &PhysicsBody2D, default_damp: f32) -> f32 {
        let overlapping = self.overlapping_areas_sorted(body);
        let mut damp = 0.0_f32;
        let mut stopped = false;
        for area in overlapping.iter().rev() {
            match area.angular_damp_space_override {
                AreaSpaceOverride::Disabled => continue,
                AreaSpaceOverride::Combine => {
                    damp += area.angular_damp;
                }
                AreaSpaceOverride::CombineReplace => {
                    damp += area.angular_damp;
                    stopped = true;
                }
                AreaSpaceOverride::Replace => {
                    damp = area.angular_damp;
                    stopped = true;
                }
                AreaSpaceOverride::ReplaceCombine => {
                    damp = area.angular_damp;
                }
            }
            if stopped {
                break;
            }
        }
        if !stopped {
            damp += default_damp;
        }
        damp
    }

    pub fn compute_body_damp(&self, body: &PhysicsBody2D, default_damp: f32) -> f32 {
        let overlapping = self.overlapping_areas_sorted(body);
        let mut damp = 0.0_f32;
        let mut stopped = false;
        for area in overlapping.iter().rev() {
            match area.linear_damp_space_override {
                AreaSpaceOverride::Disabled => continue,
                AreaSpaceOverride::Combine => {
                    damp += area.linear_damp;
                }
                AreaSpaceOverride::CombineReplace => {
                    damp += area.linear_damp;
                    stopped = true;
                }
                AreaSpaceOverride::Replace => {
                    damp = area.linear_damp;
                    stopped = true;
                }
                AreaSpaceOverride::ReplaceCombine => {
                    damp = area.linear_damp;
                }
            }
            if stopped {
                break;
            }
        }
        if !stopped {
            damp += default_damp;
        }
        damp
    }

    /// Returns `true` if any area has a non-`Disabled` gravity or linear-damp
    /// space override, i.e. per-body gravity/damp computation is worth running.
    pub fn has_space_overrides(&self) -> bool {
        self.areas.values().any(|a| {
            a.gravity_space_override != AreaSpaceOverride::Disabled
                || a.linear_damp_space_override != AreaSpaceOverride::Disabled
                // Angular counts too (pat-0q2kx.112). This gate short-circuits
                // the whole override path, so omitting it would leave an area
                // that declares ONLY an angular override doing nothing at all —
                // the override would be computed correctly and never reached.
                || a.angular_damp_space_override != AreaSpaceOverride::Disabled
        })
    }

    /// Detects overlaps between pairs of areas and returns enter/exit events.
    ///
    /// The relation is *directional*: for each ordered pair `(A, B)` of distinct
    /// areas where `A` is monitoring and `A.collision_mask & B.collision_layer`
    /// is non-zero and their shapes overlap, `(A.id, B.id)` is recorded. New
    /// pairs emit [`OverlapState::Entered`]; pairs present last frame but not
    /// this frame emit [`OverlapState::Exited`]. Mirroring
    /// [`detect_overlaps`](Self::detect_overlaps), pairs whose monitoring area
    /// stopped monitoring silently drop their previous state instead of firing
    /// an exit.
    /// The bodies currently overlapping `area`, as of the last
    /// [`detect_overlaps`](Self::detect_overlaps) (pat-0q2kx.112).
    ///
    /// Godot's `Area2D.get_overlapping_bodies()` answers from the space's live
    /// overlap set rather than recomputing, so this reads the same set the
    /// enter/exit signals were derived from — a script cannot see a different
    /// answer from the one the signals just reported.
    pub fn overlapping_bodies(&self, area: AreaId) -> Vec<BodyId> {
        let mut ids: Vec<BodyId> = self
            .previous_overlaps
            .iter()
            .filter(|(a, _)| *a == area)
            .map(|(_, b)| *b)
            .collect();
        // The backing set is unordered; sort so a script sees a stable order.
        ids.sort_by_key(|b| b.0);
        ids
    }

    /// The areas currently overlapping `area`, as of the last
    /// [`detect_area_overlaps`](Self::detect_area_overlaps) (pat-0q2kx.112).
    ///
    /// The area-vs-area twin of
    /// [`overlapping_bodies`](Self::overlapping_bodies), and it reads the same
    /// set the `area_entered` / `area_exited` signals were derived from, so a
    /// script cannot see a different answer from the one the signals just
    /// reported. The `monitorable` gate is already baked into that set by
    /// `detect_area_overlaps`, so an area opted out of detection never appears
    /// here either.
    pub fn overlapping_areas(&self, area: AreaId) -> Vec<AreaId> {
        let mut ids: Vec<AreaId> = self
            .previous_area_overlaps
            .iter()
            .filter(|(a, _)| *a == area)
            .map(|(_, b)| *b)
            .collect();
        // The backing set is unordered; sort so a script sees a stable order.
        ids.sort_by_key(|a| a.0);
        ids
    }

    pub fn detect_area_overlaps(&mut self) -> Vec<AreaOverlapEvent> {
        let mut current: HashSet<(AreaId, AreaId)> = HashSet::new();
        let mut events = Vec::new();

        let non_monitoring: HashSet<AreaId> = self
            .areas
            .values()
            .filter(|a| !a.monitoring)
            .map(|a| a.id)
            .collect();

        for a in self.areas.values() {
            if !a.monitoring {
                continue;
            }
            for b in self.areas.values() {
                if a.id == b.id {
                    continue;
                }
                // Godot gates area-vs-area detection on the TARGET being
                // monitorable: `monitoring` is whether `a` looks, `monitorable`
                // is whether `b` can be seen (`area_2d.cpp`). Without this an
                // area opted out of detection by other areas was still found by
                // every one of them (pat-0q2kx.112).
                if !b.monitorable {
                    continue;
                }
                if (a.collision_mask & b.collision_layer) == 0 {
                    continue;
                }
                let touching = a.shape_owners().into_iter().any(|oa| {
                    b.shape_owners().into_iter().any(|ob| {
                        matches!(
                            collision::test_collision(
                                &oa.shape,
                                &oa.transform,
                                &ob.shape,
                                &ob.transform,
                            ),
                            Some(r) if r.colliding && r.depth > 0.0
                        )
                    })
                });
                if touching {
                    current.insert((a.id, b.id));
                }
            }
        }

        // Entered: pairs new this frame.
        for &pair in &current {
            if !self.previous_area_overlaps.contains(&pair) {
                events.push(AreaOverlapEvent {
                    area_id: pair.0,
                    other_area_id: pair.1,
                    state: OverlapState::Entered,
                });
            }
        }

        // Exited: pairs gone this frame, unless the monitor stopped monitoring.
        for &pair in &self.previous_area_overlaps {
            if non_monitoring.contains(&pair.0) {
                continue;
            }
            if !current.contains(&pair) {
                events.push(AreaOverlapEvent {
                    area_id: pair.0,
                    other_area_id: pair.1,
                    state: OverlapState::Exited,
                });
            }
        }

        self.previous_area_overlaps = current;
        events
    }
}

impl Default for AreaStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::BodyType;

    fn make_body(id: u64, pos: Vector2) -> PhysicsBody2D {
        PhysicsBody2D::new(
            BodyId(id),
            BodyType::Rigid,
            pos,
            Shape2D::Circle { radius: 1.0 },
            1.0,
        )
    }

    #[test]
    fn area_detects_body_enter() {
        let mut store = AreaStore::new();
        let area_id = store.add_area(Area2D::new(
            AreaId(0),
            Vector2::ZERO,
            Shape2D::Circle { radius: 5.0 },
        ));

        let mut bodies = HashMap::new();
        bodies.insert(BodyId(1), make_body(1, Vector2::new(3.0, 0.0)));

        let events = store.detect_overlaps(&bodies);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].area_id, area_id);
        assert_eq!(events[0].body_id, BodyId(1));
        assert_eq!(events[0].state, OverlapState::Entered);
    }

    #[test]
    fn area_detects_body_exit() {
        let mut store = AreaStore::new();
        store.add_area(Area2D::new(
            AreaId(0),
            Vector2::ZERO,
            Shape2D::Circle { radius: 5.0 },
        ));

        let mut bodies = HashMap::new();
        bodies.insert(BodyId(1), make_body(1, Vector2::new(3.0, 0.0)));

        // First frame: enter
        let _ = store.detect_overlaps(&bodies);

        // Second frame: body moved out
        bodies.get_mut(&BodyId(1)).unwrap().position = Vector2::new(100.0, 0.0);
        let events = store.detect_overlaps(&bodies);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].state, OverlapState::Exited);
    }

    #[test]
    fn area_no_events_when_stable_inside() {
        let mut store = AreaStore::new();
        store.add_area(Area2D::new(
            AreaId(0),
            Vector2::ZERO,
            Shape2D::Circle { radius: 5.0 },
        ));

        let mut bodies = HashMap::new();
        bodies.insert(BodyId(1), make_body(1, Vector2::new(2.0, 0.0)));

        // First frame: enter
        let events = store.detect_overlaps(&bodies);
        assert_eq!(events.len(), 1);

        // Second frame: no change
        let events = store.detect_overlaps(&bodies);
        assert_eq!(events.len(), 0);
    }

    #[test]
    fn area_not_monitoring_produces_no_events() {
        let mut store = AreaStore::new();
        let area_id = store.add_area(Area2D::new(
            AreaId(0),
            Vector2::ZERO,
            Shape2D::Circle { radius: 5.0 },
        ));
        store.get_area_mut(area_id).unwrap().monitoring = false;

        let mut bodies = HashMap::new();
        bodies.insert(BodyId(1), make_body(1, Vector2::new(2.0, 0.0)));

        let events = store.detect_overlaps(&bodies);
        assert_eq!(events.len(), 0);
    }

    #[test]
    fn area_layer_mask_filtering() {
        let mut store = AreaStore::new();
        let area_id = store.add_area(Area2D::new(
            AreaId(0),
            Vector2::ZERO,
            Shape2D::Circle { radius: 5.0 },
        ));
        // Area scans layer 2 only
        store.get_area_mut(area_id).unwrap().collision_mask = 2;

        let mut bodies = HashMap::new();
        let mut body = make_body(1, Vector2::new(2.0, 0.0));
        body.collision_layer = 1; // Body is on layer 1 — should not match
        bodies.insert(BodyId(1), body);

        let events = store.detect_overlaps(&bodies);
        assert_eq!(events.len(), 0);
    }

    // -----------------------------------------------------------------------
    // pat-u3l8c.51 — Area2D gravity/damp space overrides + point gravity
    // -----------------------------------------------------------------------

    /// Adds a large circular area at the origin so any test body overlaps it.
    fn add_big_area(store: &mut AreaStore) -> AreaId {
        store.add_area(Area2D::new(
            AreaId(0),
            Vector2::ZERO,
            Shape2D::Circle { radius: 100.0 },
        ))
    }

    #[test]
    fn area_directional_gravity_replace_applies() {
        let mut store = AreaStore::new();
        let aid = add_big_area(&mut store);
        {
            let area = store.get_area_mut(aid).unwrap();
            area.gravity_space_override = AreaSpaceOverride::Replace;
            area.gravity = 500.0;
            area.gravity_direction = Vector2::new(0.0, -1.0); // up
        }
        let body = make_body(1, Vector2::ZERO);
        let default_gravity = Vector2::new(0.0, 980.0);

        let g = store.compute_body_gravity(&body, default_gravity);
        // Replace ⇒ the area's gravity only, NOT the default.
        assert!(g.x.abs() < 1e-4, "x should be 0, got {}", g.x);
        assert!((g.y + 500.0).abs() < 1e-4, "expected (0,-500), got {:?}", g);
    }

    #[test]
    fn area_point_gravity_pulls_toward_center() {
        let center = Vector2::ZERO;
        let mut area = Area2D::new(AreaId(1), center, Shape2D::Circle { radius: 100.0 });
        area.gravity_is_point = true;
        area.gravity = 100.0;
        area.gravity_point_center = center;
        area.gravity_point_unit_distance = 0.0; // no falloff

        // Body to the right of the center is pulled left (toward the center).
        let g = area.compute_gravity(Vector2::new(10.0, 0.0));
        assert!(g.x < 0.0, "gravity should point toward center, got {:?}", g);
        assert!(g.y.abs() < 1e-4);
        assert!(
            (g.length() - 100.0).abs() < 1e-4,
            "no unit distance ⇒ magnitude == gravity, got {}",
            g.length()
        );

        // With unit_distance > 0 the magnitude falls off with distance:
        // closer ⇒ stronger.
        area.gravity_point_unit_distance = 10.0;
        let near = area.compute_gravity(Vector2::new(5.0, 0.0)).length();
        let far = area.compute_gravity(Vector2::new(20.0, 0.0)).length();
        assert!(
            near > far,
            "closer must be stronger: near={near}, far={far}"
        );
    }

    #[test]
    fn area_gravity_combine_adds_to_default() {
        let mut store = AreaStore::new();
        let aid = add_big_area(&mut store);
        {
            let area = store.get_area_mut(aid).unwrap();
            area.gravity_space_override = AreaSpaceOverride::Combine;
            area.gravity = 200.0;
            area.gravity_direction = Vector2::new(0.0, 1.0); // down (same as default)
        }
        let body = make_body(1, Vector2::ZERO);
        let default_gravity = Vector2::new(0.0, 980.0);

        let g = store.compute_body_gravity(&body, default_gravity);
        // Combine ⇒ area gravity + default: (0,200) + (0,980) = (0,1180).
        assert!(g.x.abs() < 1e-4);
        assert!(
            (g.y - 1180.0).abs() < 1e-3,
            "expected (0,1180), got {:?}",
            g
        );
    }

    #[test]
    fn area_gravity_priority_replace_wins() {
        let mut store = AreaStore::new();
        // Low-priority Combine area pointing down.
        let low = add_big_area(&mut store);
        {
            let a = store.get_area_mut(low).unwrap();
            a.priority = 0;
            a.gravity_space_override = AreaSpaceOverride::Combine;
            a.gravity = 300.0;
            a.gravity_direction = Vector2::new(0.0, 1.0);
        }
        // High-priority Replace area pointing up.
        let high = add_big_area(&mut store);
        {
            let a = store.get_area_mut(high).unwrap();
            a.priority = 10;
            a.gravity_space_override = AreaSpaceOverride::Replace;
            a.gravity = 400.0;
            a.gravity_direction = Vector2::new(0.0, -1.0);
        }
        let body = make_body(1, Vector2::ZERO);
        let default_gravity = Vector2::new(0.0, 980.0);

        let g = store.compute_body_gravity(&body, default_gravity);
        // Highest-priority Replace wins and stops the chain: (0,-400) only.
        assert!(g.x.abs() < 1e-4);
        assert!(
            (g.y + 400.0).abs() < 1e-3,
            "high-priority Replace should win, got {:?}",
            g
        );
    }

    #[test]
    fn area_linear_damp_override() {
        let mut store = AreaStore::new();
        let aid = add_big_area(&mut store);
        store.get_area_mut(aid).unwrap().linear_damp_space_override = AreaSpaceOverride::Replace;
        store.get_area_mut(aid).unwrap().linear_damp = 5.0;

        let body = make_body(1, Vector2::ZERO);
        // Replace ⇒ area damp only; the default is ignored.
        let d = store.compute_body_damp(&body, 0.1);
        assert!((d - 5.0).abs() < 1e-4, "expected 5.0, got {d}");

        // Combine ⇒ area damp + default.
        store.get_area_mut(aid).unwrap().linear_damp_space_override = AreaSpaceOverride::Combine;
        let d2 = store.compute_body_damp(&body, 0.1);
        assert!(
            (d2 - 5.1).abs() < 1e-4,
            "Combine: 5.0 + 0.1 = 5.1, got {d2}"
        );
    }

    #[test]
    fn area_area_enter_and_exit() {
        let mut store = AreaStore::new();
        // Two overlapping monitoring areas (default layer/mask = 1 ⇒ aligned).
        let a = store.add_area(Area2D::new(
            AreaId(0),
            Vector2::ZERO,
            Shape2D::Circle { radius: 5.0 },
        ));
        let b = store.add_area(Area2D::new(
            AreaId(0),
            Vector2::new(3.0, 0.0),
            Shape2D::Circle { radius: 5.0 },
        ));

        // Frame 1: both directional pairs (a→b) and (b→a) enter.
        let events = store.detect_area_overlaps();
        assert_eq!(events.len(), 2, "both ordered pairs should enter");
        assert!(events.iter().all(|e| e.state == OverlapState::Entered));
        assert!(events
            .iter()
            .any(|e| e.area_id == a && e.other_area_id == b));
        assert!(events
            .iter()
            .any(|e| e.area_id == b && e.other_area_id == a));

        // Frame 2: no change ⇒ no events.
        let events = store.detect_area_overlaps();
        assert!(events.is_empty(), "stable overlap fires no events");

        // Move b away ⇒ both pairs exit.
        store.get_area_mut(b).unwrap().position = Vector2::new(100.0, 0.0);
        let events = store.detect_area_overlaps();
        assert_eq!(events.len(), 2, "both ordered pairs should exit");
        assert!(events.iter().all(|e| e.state == OverlapState::Exited));
    }

    /// `overlapping_areas` answers from the same set the enter/exit events were
    /// derived from, tracks moves, and honours the `monitorable` gate
    /// (pat-0q2kx.112). Backs the script-facing `get_overlapping_areas()`.
    #[test]
    fn overlapping_areas_reports_the_live_set_and_honours_monitorable() {
        let mut store = AreaStore::new();
        let a = store.add_area(Area2D::new(
            AreaId(0),
            Vector2::ZERO,
            Shape2D::Circle { radius: 5.0 },
        ));
        let b = store.add_area(Area2D::new(
            AreaId(0),
            Vector2::new(3.0, 0.0),
            Shape2D::Circle { radius: 5.0 },
        ));

        // Before any detection pass the set is empty rather than stale/absent —
        // Godot returns an empty array, never null.
        assert!(store.overlapping_areas(a).is_empty());

        store.detect_area_overlaps();
        assert_eq!(
            store.overlapping_areas(a),
            vec![b],
            "a must see b once they overlap"
        );
        assert_eq!(
            store.overlapping_areas(b),
            vec![a],
            "the relation is reported from both sides"
        );

        // Moving b out empties both answers — the set is live, not cumulative.
        store.get_area_mut(b).unwrap().position = Vector2::new(100.0, 0.0);
        store.detect_area_overlaps();
        assert!(store.overlapping_areas(a).is_empty());
        assert!(store.overlapping_areas(b).is_empty());

        // Back in range, but b opts out of being seen. Godot gates area-vs-area
        // detection on the TARGET being monitorable, so a no longer sees b —
        // while b, which is still monitoring, does see a.
        store.get_area_mut(b).unwrap().position = Vector2::new(3.0, 0.0);
        store.get_area_mut(b).unwrap().monitorable = false;
        store.detect_area_overlaps();
        assert!(
            store.overlapping_areas(a).is_empty(),
            "a non-monitorable area must not appear in another area's overlap set"
        );
        assert_eq!(
            store.overlapping_areas(b),
            vec![a],
            "monitorable gates being SEEN, not seeing: b still observes a"
        );
    }
}
