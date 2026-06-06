//! Animation **keyframe editing** (pat-3a2r0).
//!
//! A keyframe track holds keys positioned in time, plus a playhead and a
//! selection. Inserting adds a key at the playhead; keys can be selected singly
//! or as a group; dragging moves the whole selection in time; and delete removes
//! the selected keys. Keys are always kept sorted by time.

use std::collections::BTreeSet;

/// A stable keyframe id.
pub type KeyId = u64;

/// A single keyframe at a point in time.
#[derive(Debug, Clone, PartialEq)]
pub struct Keyframe {
    /// Stable identifier.
    pub id: KeyId,
    /// Time position in seconds.
    pub time: f32,
}

/// A track of keyframes with a playhead and selection.
#[derive(Debug, Clone, Default)]
pub struct KeyframeTrack {
    keys: Vec<Keyframe>,
    playhead: f32,
    selection: BTreeSet<KeyId>,
    next_id: KeyId,
}

impl KeyframeTrack {
    /// Creates an empty track with the playhead at 0.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the playhead time.
    pub fn set_playhead(&mut self, time: f32) {
        self.playhead = time.max(0.0);
    }

    /// The current playhead time.
    pub fn playhead(&self) -> f32 {
        self.playhead
    }

    /// The keys, sorted by time.
    pub fn keys(&self) -> &[Keyframe] {
        &self.keys
    }

    /// The number of keys.
    pub fn count(&self) -> usize {
        self.keys.len()
    }

    /// The time of the key with `id`, if present.
    pub fn time_of(&self, id: KeyId) -> Option<f32> {
        self.keys.iter().find(|k| k.id == id).map(|k| k.time)
    }

    /// The selected key ids, ascending.
    pub fn selection(&self) -> Vec<KeyId> {
        self.selection.iter().copied().collect()
    }

    fn sort_keys(&mut self) {
        self.keys
            .sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap_or(std::cmp::Ordering::Equal));
    }

    /// Inserts a key at the playhead on this (active) track, selecting it
    /// exclusively. Returns the new key's id.
    pub fn insert(&mut self) -> KeyId {
        let id = self.next_id;
        self.next_id += 1;
        self.keys.push(Keyframe {
            id,
            time: self.playhead,
        });
        self.sort_keys();
        self.select_only(id);
        id
    }

    /// Selects only `id` (clearing any prior selection). Returns whether the key
    /// exists.
    pub fn select_only(&mut self, id: KeyId) -> bool {
        self.selection.clear();
        self.add_to_selection(id)
    }

    /// Adds `id` to the selection. Returns whether the key exists.
    pub fn add_to_selection(&mut self, id: KeyId) -> bool {
        if self.keys.iter().any(|k| k.id == id) {
            self.selection.insert(id);
            true
        } else {
            false
        }
    }

    /// Clears the selection.
    pub fn clear_selection(&mut self) {
        self.selection.clear();
    }

    /// Moves every selected key by `delta` seconds (clamped at 0), re-sorting.
    /// Returns whether anything was selected.
    pub fn move_selected(&mut self, delta: f32) -> bool {
        if self.selection.is_empty() {
            return false;
        }
        for key in self.keys.iter_mut() {
            if self.selection.contains(&key.id) {
                key.time = (key.time + delta).max(0.0);
            }
        }
        self.sort_keys();
        true
    }

    /// Deletes the selected keys, clearing the selection. Returns how many were
    /// removed.
    pub fn delete_selected(&mut self) -> usize {
        let before = self.keys.len();
        self.keys.retain(|k| !self.selection.contains(&k.id));
        let removed = before - self.keys.len();
        self.selection.clear();
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-3a2r0): inserting a key adds it at the playhead, dragging
    /// moves it in time, multi-select moves a group, and delete removes the
    /// selection.
    #[test]
    fn anim_keyframe_edit() {
        let mut track = KeyframeTrack::new();

        // Insert keys at successive playhead positions.
        track.set_playhead(0.0);
        let k1 = track.insert();
        track.set_playhead(1.0);
        let k2 = track.insert();
        track.set_playhead(2.0);
        let k3 = track.insert();

        assert_eq!(track.count(), 3);
        assert_eq!(track.time_of(k1), Some(0.0));
        assert_eq!(track.time_of(k2), Some(1.0));
        assert_eq!(track.time_of(k3), Some(2.0));
        // Inserting selects just the new key.
        assert_eq!(track.selection(), vec![k3]);

        // Dragging a single selected key moves it in time.
        track.select_only(k2);
        assert!(track.move_selected(0.5));
        assert_eq!(track.time_of(k2), Some(1.5));

        // Multi-select moves the whole group together.
        track.clear_selection();
        track.add_to_selection(k1);
        track.add_to_selection(k3);
        assert!(track.move_selected(1.0));
        assert_eq!(track.time_of(k1), Some(1.0));
        assert_eq!(track.time_of(k3), Some(3.0));
        // The unselected key is unaffected.
        assert_eq!(track.time_of(k2), Some(1.5));

        // Keys stay sorted by time.
        let times: Vec<f32> = track.keys().iter().map(|k| k.time).collect();
        assert_eq!(times, vec![1.0, 1.5, 3.0]);

        // Moving clamps at the start of the track.
        track.select_only(k1);
        assert!(track.move_selected(-10.0));
        assert_eq!(track.time_of(k1), Some(0.0));

        // Delete removes the current selection.
        track.clear_selection();
        track.add_to_selection(k1);
        track.add_to_selection(k3);
        assert_eq!(track.delete_selected(), 2);
        assert_eq!(track.count(), 1);
        assert!(track.time_of(k1).is_none());
        assert!(track.time_of(k3).is_none());
        assert!(track.selection().is_empty());
        // Only k2 remains.
        assert_eq!(track.time_of(k2), Some(1.5));

        // Deleting with nothing selected removes nothing.
        assert_eq!(track.delete_selected(), 0);
        assert_eq!(track.count(), 1);
    }
}
