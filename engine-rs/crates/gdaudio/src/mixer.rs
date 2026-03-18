//! Audio mixing and output pipeline.

use crate::bus::AudioBus;

/// The audio mixer manages an ordered collection of [`AudioBus`] instances.
///
/// Index 0 is always the "Master" bus and cannot be removed.
#[derive(Debug, Clone)]
pub struct AudioMixer {
    buses: Vec<AudioBus>,
}

impl AudioMixer {
    /// Create a new mixer with a default "Master" bus at index 0.
    pub fn new() -> Self {
        Self {
            buses: vec![AudioBus::new("Master")],
        }
    }

    /// Add a new bus with the given name. Returns its index.
    pub fn add_bus(&mut self, name: impl Into<String>) -> usize {
        let idx = self.buses.len();
        self.buses.push(AudioBus::new(name));
        idx
    }

    /// Remove the bus at `idx`.
    ///
    /// # Panics
    /// Panics if `idx` is 0 (master bus cannot be removed) or out of range.
    pub fn remove_bus(&mut self, idx: usize) {
        assert!(idx != 0, "cannot remove the master bus");
        self.buses.remove(idx);
    }

    /// Return an immutable reference to the bus at `idx`, if it exists.
    pub fn get_bus(&self, idx: usize) -> Option<&AudioBus> {
        self.buses.get(idx)
    }

    /// Return a mutable reference to the bus at `idx`, if it exists.
    pub fn get_bus_mut(&mut self, idx: usize) -> Option<&mut AudioBus> {
        self.buses.get_mut(idx)
    }

    /// Find the index of the first bus with the given name.
    pub fn get_bus_by_name(&self, name: &str) -> Option<usize> {
        self.buses.iter().position(|b| b.name() == name)
    }

    /// Return the number of buses.
    pub fn bus_count(&self) -> usize {
        self.buses.len()
    }

    /// Move a bus from index `from` to index `to`.
    ///
    /// The master bus at index 0 is never affected — both `from` and `to`
    /// must be >= 1.
    ///
    /// # Panics
    /// Panics if `from` or `to` is 0 or out of range.
    pub fn move_bus(&mut self, from: usize, to: usize) {
        assert!(from != 0 && to != 0, "cannot move the master bus");
        assert!(from < self.buses.len(), "from index out of range");
        assert!(to < self.buses.len(), "to index out of range");
        let bus = self.buses.remove(from);
        self.buses.insert(to, bus);
    }
}

impl Default for AudioMixer {
    fn default() -> Self {
        Self::new()
    }
}
