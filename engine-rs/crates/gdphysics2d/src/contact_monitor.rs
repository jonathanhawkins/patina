//! Per-body contact tracking for `RigidBody2D`'s `contact_monitor`.
//!
//! [`ContactMonitor`] maintains the set of bodies currently touching one
//! `RigidBody2D` and reports enter/exit transitions between frames, mirroring
//! Godot's `contact_monitor` + `max_contacts_reported` bookkeeping that backs
//! the `body_entered` / `body_exited` signals and
//! `get_colliding_bodies()` / `get_contact_count()`.
//!
//! This is the reusable core: a physics-server pass feeds each monitored body
//! the set of bodies it overlaps this step and emits signals from the returned
//! [`ContactChanges`]. Routing the world's collision events to nodes and
//! emitting the actual signals is wired elsewhere.

use std::collections::HashSet;

use crate::body::BodyId;

/// Bodies that began or ended contact between two [`ContactMonitor::update`]
/// calls. Both lists are sorted by body id for deterministic reporting.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContactChanges {
    /// Bodies newly in contact this update (→ `body_entered`).
    pub entered: Vec<BodyId>,
    /// Bodies no longer in contact this update (→ `body_exited`).
    pub exited: Vec<BodyId>,
}

/// Tracks the bodies currently in contact with one `RigidBody2D` and reports
/// enter/exit transitions (Godot `contact_monitor` + `max_contacts_reported`).
#[derive(Debug, Clone)]
pub struct ContactMonitor {
    /// Whether contact reporting is enabled (`contact_monitor`).
    pub enabled: bool,
    /// Maximum number of contacts tracked; `0` disables reporting, as in Godot.
    pub max_contacts_reported: usize,
    current: HashSet<BodyId>,
}

impl Default for ContactMonitor {
    fn default() -> Self {
        Self {
            enabled: false,
            max_contacts_reported: 0,
            current: HashSet::new(),
        }
    }
}

impl ContactMonitor {
    /// Creates a disabled monitor (Godot's default).
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an enabled monitor reporting up to `max_contacts_reported`
    /// contacts.
    pub fn active(max_contacts_reported: usize) -> Self {
        Self {
            enabled: true,
            max_contacts_reported,
            current: HashSet::new(),
        }
    }

    /// Whether contact reporting is currently active (enabled with a positive
    /// cap). When inactive, [`update`](Self::update) tracks nothing.
    pub fn is_active(&self) -> bool {
        self.enabled && self.max_contacts_reported > 0
    }

    /// Number of bodies currently in contact (`get_contact_count`).
    pub fn contact_count(&self) -> usize {
        self.current.len()
    }

    /// Whether `body` is currently in contact.
    pub fn is_colliding(&self, body: BodyId) -> bool {
        self.current.contains(&body)
    }

    /// The bodies currently in contact, sorted by id (`get_colliding_bodies`).
    pub fn colliding_bodies(&self) -> Vec<BodyId> {
        let mut bodies: Vec<BodyId> = self.current.iter().copied().collect();
        bodies.sort_by_key(|b| b.0);
        bodies
    }

    /// Replaces the contact set with `touching` (the bodies overlapping this
    /// frame) and returns the enter/exit transitions since the previous update.
    ///
    /// When [`is_active`](Self::is_active) is false, nothing is tracked: any
    /// previously-tracked contacts are reported as exited and then cleared. At
    /// most `max_contacts_reported` contacts are retained (excess is ignored,
    /// as Godot stops reporting past the cap).
    pub fn update<I>(&mut self, touching: I) -> ContactChanges
    where
        I: IntoIterator<Item = BodyId>,
    {
        let mut next: HashSet<BodyId> = HashSet::new();
        if self.is_active() {
            for body in touching {
                if next.len() >= self.max_contacts_reported {
                    break;
                }
                next.insert(body);
            }
        }
        let mut entered: Vec<BodyId> = next.difference(&self.current).copied().collect();
        let mut exited: Vec<BodyId> = self.current.difference(&next).copied().collect();
        entered.sort_by_key(|b| b.0);
        exited.sort_by_key(|b| b.0);
        self.current = next;
        ContactChanges { entered, exited }
    }

    /// Clears all tracked contacts, returning the bodies that were exited
    /// (sorted by id).
    pub fn clear(&mut self) -> Vec<BodyId> {
        let mut exited: Vec<BodyId> = self.current.drain().collect();
        exited.sort_by_key(|b| b.0);
        exited
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(values: &[u64]) -> Vec<BodyId> {
        values.iter().map(|&n| BodyId(n)).collect()
    }

    #[test]
    fn disabled_monitor_tracks_nothing() {
        let mut m = ContactMonitor::new();
        assert!(!m.is_active());
        let changes = m.update(ids(&[1, 2, 3]));
        assert!(changes.entered.is_empty());
        assert_eq!(m.contact_count(), 0);
    }

    #[test]
    fn enter_and_exit_transitions() {
        let mut m = ContactMonitor::active(8);
        assert!(m.is_active());

        // Frame 1: bodies 1 and 2 enter.
        let c = m.update(ids(&[1, 2]));
        assert_eq!(c.entered, ids(&[1, 2]));
        assert!(c.exited.is_empty());
        assert_eq!(m.contact_count(), 2);
        assert!(m.is_colliding(BodyId(1)));

        // Frame 2: 2 persists, 3 enters, 1 exits.
        let c = m.update(ids(&[2, 3]));
        assert_eq!(c.entered, ids(&[3]));
        assert_eq!(c.exited, ids(&[1]));
        assert_eq!(m.colliding_bodies(), ids(&[2, 3]));

        // Frame 3: everything separates.
        let c = m.update(ids(&[]));
        assert!(c.entered.is_empty());
        assert_eq!(c.exited, ids(&[2, 3]));
        assert_eq!(m.contact_count(), 0);
    }

    #[test]
    fn max_contacts_cap_is_honored() {
        let mut m = ContactMonitor::active(2);
        let c = m.update(ids(&[1, 2, 3, 4]));
        assert_eq!(m.contact_count(), 2);
        assert_eq!(c.entered.len(), 2);
    }

    #[test]
    fn clear_reports_exited() {
        let mut m = ContactMonitor::active(8);
        m.update(ids(&[5, 6]));
        let exited = m.clear();
        assert_eq!(exited, ids(&[5, 6]));
        assert_eq!(m.contact_count(), 0);
    }
}
