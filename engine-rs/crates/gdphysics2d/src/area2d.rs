//! Area2D overlap detection.
//!
//! Provides [`Area2D`] for detecting when physics bodies enter or exit a
//! spatial region, without applying physics forces. Useful for triggers,
//! damage zones, and spatial queries.

use std::collections::{HashMap, HashSet};

use gdcore::math::{Transform2D, Vector2};

use crate::body::{BodyId, PhysicsBody2D};
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

/// A 2D area used for overlap detection (no physics response).
#[derive(Debug, Clone)]
pub struct Area2D {
    /// Unique identifier.
    pub id: AreaId,
    /// World-space position.
    pub position: Vector2,
    /// Collision shape in local space.
    pub shape: Shape2D,
    /// Collision layer bitmask — which layers this area occupies.
    pub collision_layer: u32,
    /// Collision mask bitmask — which layers this area scans for overlaps.
    pub collision_mask: u32,
    /// Whether this area is actively detecting overlaps.
    pub monitoring: bool,
}

impl Area2D {
    /// Creates a new area with the given properties.
    pub fn new(id: AreaId, position: Vector2, shape: Shape2D) -> Self {
        Self {
            id,
            position,
            shape,
            collision_layer: 1,
            collision_mask: 1,
            monitoring: true,
        }
    }
}

/// Stores areas and tracks overlaps across frames.
pub struct AreaStore {
    areas: HashMap<AreaId, Area2D>,
    next_id: u64,
    /// Previous frame overlaps: (area_id, body_id) pairs.
    previous_overlaps: HashSet<(AreaId, BodyId)>,
}

impl AreaStore {
    /// Creates an empty area store.
    pub fn new() -> Self {
        Self {
            areas: HashMap::new(),
            next_id: 1,
            previous_overlaps: HashSet::new(),
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

            let tf_area = Transform2D::translated(area.position);

            for body in bodies.values() {
                // Layer/mask filtering
                if (area.collision_mask & body.collision_layer) == 0 {
                    continue;
                }

                let tf_body = Transform2D::translated(body.position);

                if let Some(result) =
                    collision::test_collision(&area.shape, &tf_area, &body.shape, &tf_body)
                {
                    if result.colliding && result.depth > 0.0 {
                        current_overlaps.insert((area.id, body.id));
                    }
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
}
