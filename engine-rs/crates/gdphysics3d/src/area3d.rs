//! Area3D overlap detection and signal emission.
//!
//! Provides [`Area3D`] for detecting when 3D physics bodies enter or exit a
//! spatial region, without applying physics forces. Useful for triggers,
//! damage zones, and spatial queries.
//!
//! Mirrors Godot's `Area3D` node:
//! - `monitoring` / `monitorable` control detection
//! - `collision_layer` / `collision_mask` filter interactions
//! - `body_entered` / `body_exited` signals via [`OverlapEvent3D`]
//! - Area-to-area overlap tracking via [`AreaOverlapEvent3D`]

use std::collections::{HashMap, HashSet};

use gdcore::math::Vector3;

use crate::body::{BodyId3D, PhysicsBody3D};
use crate::collision;
use crate::shape::Shape3D;

/// Unique identifier for a 3D area within a [`PhysicsWorld3D`](crate::world::PhysicsWorld3D).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AreaId3D(pub u64);

/// Whether a body just entered or exited an area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlapState3D {
    /// The body entered the area this frame.
    Entered,
    /// The body exited the area this frame.
    Exited,
}

/// An overlap event between a 3D area and a body.
///
/// Maps to Godot's `body_entered` / `body_exited` signals.
#[derive(Debug, Clone, Copy)]
pub struct OverlapEvent3D {
    /// The area involved.
    pub area_id: AreaId3D,
    /// The body that entered or exited.
    pub body_id: BodyId3D,
    /// Whether the body entered or exited.
    pub state: OverlapState3D,
}

/// An overlap event between two 3D areas.
///
/// Maps to Godot's `area_entered` / `area_exited` signals.
#[derive(Debug, Clone, Copy)]
pub struct AreaOverlapEvent3D {
    /// The detecting area.
    pub area_id: AreaId3D,
    /// The other area that entered or exited.
    pub other_area_id: AreaId3D,
    /// Whether the other area entered or exited.
    pub state: OverlapState3D,
}

/// Space override mode for an Area3D (controls how it affects physics).
///
/// Maps to Godot's `Area3D.SpaceOverride` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpaceOverride {
    /// No gravity/damping override.
    #[default]
    Disabled = 0,
    /// Combine with default space parameters.
    Combine = 1,
    /// Replace default space parameters in overlapping region.
    Replace = 2,
    /// Combine then replace (for stacking areas).
    CombineReplace = 3,
    /// Replace then combine.
    ReplaceCombine = 4,
}

/// A 3D area used for overlap detection (no physics response).
///
/// Mirrors Godot's `Area3D` node. The area occupies a spatial region defined
/// by its `shape` and `position`, and emits enter/exit events when bodies
/// or other areas overlap.
#[derive(Debug, Clone)]
pub struct Area3D {
    /// Unique identifier.
    pub id: AreaId3D,
    /// World-space position.
    pub position: Vector3,
    /// Collision shape in local space.
    pub shape: Shape3D,
    /// Collision layer bitmask — which layers this area occupies.
    pub collision_layer: u32,
    /// Collision mask bitmask — which layers this area scans for overlaps.
    pub collision_mask: u32,
    /// Whether this area actively detects overlapping bodies/areas.
    pub monitoring: bool,
    /// Whether this area can be detected by other monitoring areas.
    pub monitorable: bool,
    /// Priority for overlapping area resolution (higher = processed first).
    pub priority: f32,
    /// Gravity override mode.
    pub gravity_space_override: SpaceOverride,
    /// Gravity strength when overriding (m/s²).
    pub gravity: f32,
    /// Gravity direction (normalized).
    pub gravity_direction: Vector3,
    /// Whether gravity is a point (radial) rather than directional.
    pub gravity_point: bool,
    /// Linear damp override.
    pub linear_damp: f32,
    /// Angular damp override.
    pub angular_damp: f32,
}

impl Area3D {
    /// Creates a new area with the given properties and Godot-compatible defaults.
    pub fn new(id: AreaId3D, position: Vector3, shape: Shape3D) -> Self {
        Self {
            id,
            position,
            shape,
            collision_layer: 1,
            collision_mask: 1,
            monitoring: true,
            monitorable: true,
            priority: 0.0,
            gravity_space_override: SpaceOverride::default(),
            gravity: 9.8,
            gravity_direction: Vector3::new(0.0, -1.0, 0.0),
            gravity_point: false,
            linear_damp: 0.1,
            angular_damp: 0.1,
        }
    }

    /// Returns the list of bodies currently overlapping this area, given the
    /// current body set. This is a one-shot query (no enter/exit tracking).
    pub fn get_overlapping_bodies<'a>(
        &self,
        bodies: impl IntoIterator<Item = &'a PhysicsBody3D>,
    ) -> Vec<BodyId3D> {
        if !self.monitoring {
            return Vec::new();
        }
        let mut result = Vec::new();
        for body in bodies {
            if (self.collision_mask & body.collision_layer) == 0 {
                continue;
            }
            let cr = collision::test_collision(self.position, &self.shape, body.position, &body.shape);
            if cr.colliding {
                result.push(body.id);
            }
        }
        result
    }
}

/// Stores 3D areas and tracks body/area overlaps across frames.
pub struct AreaStore3D {
    areas: HashMap<AreaId3D, Area3D>,
    next_id: u64,
    /// Previous frame body overlaps: (area_id, body_id) pairs.
    previous_body_overlaps: HashSet<(AreaId3D, BodyId3D)>,
    /// Previous frame area-area overlaps: (area_id, other_area_id) pairs.
    previous_area_overlaps: HashSet<(AreaId3D, AreaId3D)>,
}

impl AreaStore3D {
    /// Creates an empty area store.
    pub fn new() -> Self {
        Self {
            areas: HashMap::new(),
            next_id: 1,
            previous_body_overlaps: HashSet::new(),
            previous_area_overlaps: HashSet::new(),
        }
    }

    /// Adds an area and returns its unique ID.
    pub fn add_area(&mut self, mut area: Area3D) -> AreaId3D {
        let id = AreaId3D(self.next_id);
        self.next_id += 1;
        area.id = id;
        self.areas.insert(id, area);
        id
    }

    /// Removes an area by ID.
    pub fn remove_area(&mut self, id: AreaId3D) -> Option<Area3D> {
        self.areas.remove(&id)
    }

    /// Returns a reference to an area by ID.
    pub fn get_area(&self, id: AreaId3D) -> Option<&Area3D> {
        self.areas.get(&id)
    }

    /// Returns a mutable reference to an area by ID.
    pub fn get_area_mut(&mut self, id: AreaId3D) -> Option<&mut Area3D> {
        self.areas.get_mut(&id)
    }

    /// Returns the number of areas in the store.
    pub fn area_count(&self) -> usize {
        self.areas.len()
    }

    /// Detects body overlaps between all areas and the given bodies.
    ///
    /// Returns enter/exit events by comparing against the previous frame.
    /// Maps to Godot's `body_entered` / `body_exited` signals.
    pub fn detect_body_overlaps(
        &mut self,
        bodies: &HashMap<BodyId3D, PhysicsBody3D>,
    ) -> Vec<OverlapEvent3D> {
        let mut current_overlaps = HashSet::new();
        let mut events = Vec::new();

        for area in self.areas.values() {
            if !area.monitoring {
                continue;
            }

            for body in bodies.values() {
                // Layer/mask filtering
                if (area.collision_mask & body.collision_layer) == 0 {
                    continue;
                }

                let result = collision::test_collision(
                    area.position,
                    &area.shape,
                    body.position,
                    &body.shape,
                );

                if result.colliding {
                    current_overlaps.insert((area.id, body.id));
                }
            }
        }

        // Detect entered
        for &pair in &current_overlaps {
            if !self.previous_body_overlaps.contains(&pair) {
                events.push(OverlapEvent3D {
                    area_id: pair.0,
                    body_id: pair.1,
                    state: OverlapState3D::Entered,
                });
            }
        }

        // Detect exited
        for &pair in &self.previous_body_overlaps {
            if !current_overlaps.contains(&pair) {
                events.push(OverlapEvent3D {
                    area_id: pair.0,
                    body_id: pair.1,
                    state: OverlapState3D::Exited,
                });
            }
        }

        self.previous_body_overlaps = current_overlaps;
        events
    }

    /// Detects area-area overlaps between all monitoring areas and
    /// all monitorable areas.
    ///
    /// Returns enter/exit events by comparing against the previous frame.
    /// Maps to Godot's `area_entered` / `area_exited` signals.
    pub fn detect_area_overlaps(&mut self) -> Vec<AreaOverlapEvent3D> {
        let mut current_overlaps = HashSet::new();
        let mut events = Vec::new();

        let area_list: Vec<_> = self.areas.values().collect();

        for (i, a) in area_list.iter().enumerate() {
            for b in area_list.iter().skip(i + 1) {
                // Check if shapes actually overlap (shared for both directions).
                let a_can_see_b = a.monitoring && b.monitorable
                    && (a.collision_mask & b.collision_layer) != 0;
                let b_can_see_a = b.monitoring && a.monitorable
                    && (b.collision_mask & a.collision_layer) != 0;

                if !a_can_see_b && !b_can_see_a {
                    continue;
                }

                let result = collision::test_collision(
                    a.position, &a.shape,
                    b.position, &b.shape,
                );

                if result.colliding {
                    if a_can_see_b {
                        current_overlaps.insert((a.id, b.id));
                    }
                    if b_can_see_a {
                        current_overlaps.insert((b.id, a.id));
                    }
                }
            }
        }

        // Detect entered
        for &pair in &current_overlaps {
            if !self.previous_area_overlaps.contains(&pair) {
                events.push(AreaOverlapEvent3D {
                    area_id: pair.0,
                    other_area_id: pair.1,
                    state: OverlapState3D::Entered,
                });
            }
        }

        // Detect exited
        for &pair in &self.previous_area_overlaps {
            if !current_overlaps.contains(&pair) {
                events.push(AreaOverlapEvent3D {
                    area_id: pair.0,
                    other_area_id: pair.1,
                    state: OverlapState3D::Exited,
                });
            }
        }

        self.previous_area_overlaps = current_overlaps;
        events
    }
}

impl Default for AreaStore3D {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::BodyType3D;

    fn make_body(id: u64, pos: Vector3) -> PhysicsBody3D {
        PhysicsBody3D::new(
            BodyId3D(id),
            BodyType3D::Rigid,
            pos,
            Shape3D::Sphere { radius: 1.0 },
            1.0,
        )
    }

    fn make_area(pos: Vector3, radius: f32) -> Area3D {
        Area3D::new(AreaId3D(0), pos, Shape3D::Sphere { radius })
    }

    // -- Area3D construction --------------------------------------------------

    #[test]
    fn area3d_defaults() {
        let a = Area3D::new(AreaId3D(1), Vector3::ZERO, Shape3D::Sphere { radius: 5.0 });
        assert_eq!(a.collision_layer, 1);
        assert_eq!(a.collision_mask, 1);
        assert!(a.monitoring);
        assert!(a.monitorable);
        assert!((a.priority - 0.0).abs() < f32::EPSILON);
        assert_eq!(a.gravity_space_override, SpaceOverride::Disabled);
        assert!((a.gravity - 9.8).abs() < f32::EPSILON);
        assert!(!a.gravity_point);
    }

    #[test]
    fn area_id_equality() {
        assert_eq!(AreaId3D(10), AreaId3D(10));
        assert_ne!(AreaId3D(10), AreaId3D(20));
    }

    #[test]
    fn space_override_values() {
        assert_eq!(SpaceOverride::Disabled as u32, 0);
        assert_eq!(SpaceOverride::Combine as u32, 1);
        assert_eq!(SpaceOverride::Replace as u32, 2);
        assert_eq!(SpaceOverride::CombineReplace as u32, 3);
        assert_eq!(SpaceOverride::ReplaceCombine as u32, 4);
    }

    // -- AreaStore3D management -----------------------------------------------

    #[test]
    fn store_add_and_get() {
        let mut store = AreaStore3D::new();
        let id = store.add_area(make_area(Vector3::ZERO, 5.0));
        assert_eq!(store.area_count(), 1);
        assert!(store.get_area(id).is_some());
    }

    #[test]
    fn store_remove() {
        let mut store = AreaStore3D::new();
        let id = store.add_area(make_area(Vector3::ZERO, 5.0));
        assert!(store.remove_area(id).is_some());
        assert_eq!(store.area_count(), 0);
        assert!(store.get_area(id).is_none());
    }

    #[test]
    fn store_get_mut() {
        let mut store = AreaStore3D::new();
        let id = store.add_area(make_area(Vector3::ZERO, 5.0));
        store.get_area_mut(id).unwrap().monitoring = false;
        assert!(!store.get_area(id).unwrap().monitoring);
    }

    // -- Body overlap detection -----------------------------------------------

    #[test]
    fn detects_body_enter() {
        let mut store = AreaStore3D::new();
        let area_id = store.add_area(make_area(Vector3::ZERO, 5.0));

        let mut bodies = HashMap::new();
        bodies.insert(BodyId3D(1), make_body(1, Vector3::new(3.0, 0.0, 0.0)));

        let events = store.detect_body_overlaps(&bodies);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].area_id, area_id);
        assert_eq!(events[0].body_id, BodyId3D(1));
        assert_eq!(events[0].state, OverlapState3D::Entered);
    }

    #[test]
    fn detects_body_exit() {
        let mut store = AreaStore3D::new();
        store.add_area(make_area(Vector3::ZERO, 5.0));

        let mut bodies = HashMap::new();
        bodies.insert(BodyId3D(1), make_body(1, Vector3::new(3.0, 0.0, 0.0)));

        // Frame 1: body enters
        let events = store.detect_body_overlaps(&bodies);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].state, OverlapState3D::Entered);

        // Frame 2: body moves far away → exits
        bodies.get_mut(&BodyId3D(1)).unwrap().position = Vector3::new(100.0, 0.0, 0.0);
        let events = store.detect_body_overlaps(&bodies);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].state, OverlapState3D::Exited);
    }

    #[test]
    fn no_event_when_body_stays_inside() {
        let mut store = AreaStore3D::new();
        store.add_area(make_area(Vector3::ZERO, 5.0));

        let mut bodies = HashMap::new();
        bodies.insert(BodyId3D(1), make_body(1, Vector3::new(1.0, 0.0, 0.0)));

        // Frame 1: enter
        let _ = store.detect_body_overlaps(&bodies);
        // Frame 2: still inside — no events
        let events = store.detect_body_overlaps(&bodies);
        assert!(events.is_empty());
    }

    #[test]
    fn no_event_when_body_far_away() {
        let mut store = AreaStore3D::new();
        store.add_area(make_area(Vector3::ZERO, 5.0));

        let mut bodies = HashMap::new();
        bodies.insert(BodyId3D(1), make_body(1, Vector3::new(100.0, 0.0, 0.0)));

        let events = store.detect_body_overlaps(&bodies);
        assert!(events.is_empty());
    }

    #[test]
    fn monitoring_false_skips_detection() {
        let mut store = AreaStore3D::new();
        let mut area = make_area(Vector3::ZERO, 5.0);
        area.monitoring = false;
        store.add_area(area);

        let mut bodies = HashMap::new();
        bodies.insert(BodyId3D(1), make_body(1, Vector3::new(1.0, 0.0, 0.0)));

        let events = store.detect_body_overlaps(&bodies);
        assert!(events.is_empty());
    }

    #[test]
    fn layer_mask_filtering() {
        let mut store = AreaStore3D::new();
        let mut area = make_area(Vector3::ZERO, 5.0);
        area.collision_mask = 0b0010; // only detects layer 2
        store.add_area(area);

        let mut body = make_body(1, Vector3::new(1.0, 0.0, 0.0));
        body.collision_layer = 0b0001; // on layer 1 (not matched)

        let mut bodies = HashMap::new();
        bodies.insert(body.id, body);

        let events = store.detect_body_overlaps(&bodies);
        assert!(events.is_empty());
    }

    #[test]
    fn layer_mask_match_detects() {
        let mut store = AreaStore3D::new();
        let mut area = make_area(Vector3::ZERO, 5.0);
        area.collision_mask = 0b0010;
        store.add_area(area);

        let mut body = make_body(1, Vector3::new(1.0, 0.0, 0.0));
        body.collision_layer = 0b0010; // matches

        let mut bodies = HashMap::new();
        bodies.insert(body.id, body);

        let events = store.detect_body_overlaps(&bodies);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].state, OverlapState3D::Entered);
    }

    #[test]
    fn multiple_bodies_multiple_events() {
        let mut store = AreaStore3D::new();
        store.add_area(make_area(Vector3::ZERO, 10.0));

        let mut bodies = HashMap::new();
        bodies.insert(BodyId3D(1), make_body(1, Vector3::new(1.0, 0.0, 0.0)));
        bodies.insert(BodyId3D(2), make_body(2, Vector3::new(0.0, 1.0, 0.0)));
        bodies.insert(BodyId3D(3), make_body(3, Vector3::new(100.0, 0.0, 0.0)));

        let events = store.detect_body_overlaps(&bodies);
        // Bodies 1 and 2 overlap, body 3 does not
        assert_eq!(events.len(), 2);
        let entered_ids: HashSet<_> = events.iter().map(|e| e.body_id).collect();
        assert!(entered_ids.contains(&BodyId3D(1)));
        assert!(entered_ids.contains(&BodyId3D(2)));
    }

    // -- Area-area overlap detection ------------------------------------------

    #[test]
    fn detects_area_area_enter() {
        let mut store = AreaStore3D::new();
        let a1 = store.add_area(make_area(Vector3::ZERO, 5.0));
        let a2 = store.add_area(make_area(Vector3::new(3.0, 0.0, 0.0), 5.0));

        let events = store.detect_area_overlaps();
        // Symmetric — both directions
        assert_eq!(events.len(), 2);
        let ids: HashSet<_> = events.iter().map(|e| (e.area_id, e.other_area_id)).collect();
        assert!(ids.contains(&(a1, a2)));
        assert!(ids.contains(&(a2, a1)));
    }

    #[test]
    fn detects_area_area_exit() {
        let mut store = AreaStore3D::new();
        store.add_area(make_area(Vector3::ZERO, 5.0));
        let a2 = store.add_area(make_area(Vector3::new(3.0, 0.0, 0.0), 5.0));

        // Frame 1: enter
        let _ = store.detect_area_overlaps();

        // Frame 2: move area2 far away
        store.get_area_mut(a2).unwrap().position = Vector3::new(100.0, 0.0, 0.0);
        let events = store.detect_area_overlaps();
        assert_eq!(events.len(), 2); // symmetric exit
        assert!(events.iter().all(|e| e.state == OverlapState3D::Exited));
    }

    #[test]
    fn non_monitorable_area_not_detected_by_other() {
        let mut store = AreaStore3D::new();
        let _a1 = store.add_area(make_area(Vector3::ZERO, 5.0));
        let mut a2 = make_area(Vector3::new(1.0, 0.0, 0.0), 5.0);
        a2.monitorable = false;
        a2.monitoring = false; // also disable monitoring so a2 doesn't detect a1
        store.add_area(a2);

        let events = store.detect_area_overlaps();
        assert!(events.is_empty(), "non-monitorable+non-monitoring area should produce no events");
    }

    #[test]
    fn non_monitorable_but_monitoring_area_detects_others() {
        let mut store = AreaStore3D::new();
        let a1 = store.add_area(make_area(Vector3::ZERO, 5.0));
        let mut a2 = make_area(Vector3::new(1.0, 0.0, 0.0), 5.0);
        a2.monitorable = false; // a1 cannot see a2
        // a2.monitoring remains true, so a2 can see a1
        let a2_id = store.add_area(a2);

        let events = store.detect_area_overlaps();
        // Only a2 sees a1 (not symmetric because a2 is non-monitorable)
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].area_id, a2_id);
        assert_eq!(events[0].other_area_id, a1);
    }

    // -- get_overlapping_bodies one-shot query --------------------------------

    #[test]
    fn get_overlapping_bodies_returns_nearby() {
        let area = make_area(Vector3::ZERO, 5.0);
        let bodies = vec![
            make_body(1, Vector3::new(1.0, 0.0, 0.0)),  // inside
            make_body(2, Vector3::new(100.0, 0.0, 0.0)), // outside
        ];
        let result = area.get_overlapping_bodies(bodies.iter());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], BodyId3D(1));
    }

    #[test]
    fn get_overlapping_bodies_respects_monitoring() {
        let mut area = make_area(Vector3::ZERO, 5.0);
        area.monitoring = false;
        let bodies = vec![make_body(1, Vector3::new(1.0, 0.0, 0.0))];
        let result = area.get_overlapping_bodies(bodies.iter());
        assert!(result.is_empty());
    }

    // -- Removed area clears state -------------------------------------------

    #[test]
    fn remove_area_clears_overlap_state() {
        let mut store = AreaStore3D::new();
        let aid = store.add_area(make_area(Vector3::ZERO, 5.0));

        let mut bodies = HashMap::new();
        bodies.insert(BodyId3D(1), make_body(1, Vector3::new(1.0, 0.0, 0.0)));

        // Body enters
        let events = store.detect_body_overlaps(&bodies);
        assert_eq!(events.len(), 1);

        // Remove area → next frame should emit exit
        store.remove_area(aid);
        let events = store.detect_body_overlaps(&bodies);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].state, OverlapState3D::Exited);
    }
}
