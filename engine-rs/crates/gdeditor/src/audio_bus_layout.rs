//! The **Audio bus layout editor**.
//!
//! The bottom-panel Audio editor lets the user shape the project's audio bus
//! layout: add and remove buses, set each bus's volume, toggle mute / solo /
//! bypass, build an effect chain per bus, and route sends to other buses. The
//! whole layout serializes to and from a resource so it persists across editor
//! sessions.
//!
//! There is always a `Master` bus (index 0); it cannot be removed. New buses
//! default to routing their output to `Master`.

use serde::{Deserialize, Serialize};

/// The default name of the always-present root bus.
pub const MASTER_BUS: &str = "Master";

/// A single effect in a bus's effect chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioEffect {
    /// Effect type name (e.g. `"Reverb"`).
    pub name: String,
    /// Whether the effect is active.
    pub enabled: bool,
}

/// A send routing some of a bus's signal to another bus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BusSend {
    /// Name of the destination bus.
    pub target: String,
    /// Send level in decibels.
    pub amount_db: f32,
}

/// A single audio bus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioBus {
    /// Display name.
    pub name: String,
    /// Output volume in decibels (0.0 = unity).
    pub volume_db: f32,
    /// Whether the bus is muted.
    pub muted: bool,
    /// Whether the bus is soloed.
    pub soloed: bool,
    /// Whether the bus's effects are bypassed.
    pub bypassed: bool,
    /// The effect chain, processed in order.
    pub effects: Vec<AudioEffect>,
    /// Sends to other buses.
    pub sends: Vec<BusSend>,
}

impl AudioBus {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            volume_db: 0.0,
            muted: false,
            soloed: false,
            bypassed: false,
            effects: Vec::new(),
            sends: Vec::new(),
        }
    }
}

/// The full bus layout: the ordered list of buses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioBusLayout {
    buses: Vec<AudioBus>,
}

impl Default for AudioBusLayout {
    fn default() -> Self {
        Self {
            buses: vec![AudioBus::new(MASTER_BUS)],
        }
    }
}

impl AudioBusLayout {
    /// Creates a layout with just the `Master` bus.
    pub fn new() -> Self {
        Self::default()
    }

    /// The number of buses.
    pub fn len(&self) -> usize {
        self.buses.len()
    }

    /// Whether the layout has no buses (never true in practice — Master always
    /// exists — but provided for API completeness).
    pub fn is_empty(&self) -> bool {
        self.buses.is_empty()
    }

    /// All buses, in order (Master first).
    pub fn buses(&self) -> &[AudioBus] {
        &self.buses
    }

    /// A bus by index.
    pub fn bus(&self, index: usize) -> Option<&AudioBus> {
        self.buses.get(index)
    }

    /// Adds a new bus named `name`, routed to `Master` by default. Returns its
    /// index.
    pub fn add_bus(&mut self, name: impl Into<String>) -> usize {
        let mut bus = AudioBus::new(name);
        bus.sends.push(BusSend {
            target: MASTER_BUS.to_string(),
            amount_db: 0.0,
        });
        self.buses.push(bus);
        self.buses.len() - 1
    }

    /// Removes the bus at `index`. The `Master` bus (index 0) cannot be removed.
    /// Returns whether a bus was removed.
    pub fn remove_bus(&mut self, index: usize) -> bool {
        if index == 0 || index >= self.buses.len() {
            return false;
        }
        self.buses.remove(index);
        true
    }

    /// Sets the volume (in dB) of the bus at `index`.
    pub fn set_volume(&mut self, index: usize, volume_db: f32) -> bool {
        match self.buses.get_mut(index) {
            Some(bus) => {
                bus.volume_db = volume_db;
                true
            }
            None => false,
        }
    }

    /// Toggles mute on the bus at `index`, returning the new state.
    pub fn toggle_mute(&mut self, index: usize) -> Option<bool> {
        let bus = self.buses.get_mut(index)?;
        bus.muted = !bus.muted;
        Some(bus.muted)
    }

    /// Toggles solo on the bus at `index`, returning the new state.
    pub fn toggle_solo(&mut self, index: usize) -> Option<bool> {
        let bus = self.buses.get_mut(index)?;
        bus.soloed = !bus.soloed;
        Some(bus.soloed)
    }

    /// Toggles effect bypass on the bus at `index`, returning the new state.
    pub fn toggle_bypass(&mut self, index: usize) -> Option<bool> {
        let bus = self.buses.get_mut(index)?;
        bus.bypassed = !bus.bypassed;
        Some(bus.bypassed)
    }

    /// Appends an effect to the bus at `index`'s effect chain.
    pub fn add_effect(&mut self, index: usize, effect_name: impl Into<String>) -> bool {
        match self.buses.get_mut(index) {
            Some(bus) => {
                bus.effects.push(AudioEffect {
                    name: effect_name.into(),
                    enabled: true,
                });
                true
            }
            None => false,
        }
    }

    /// Removes the effect at `effect_index` from the bus at `index`.
    pub fn remove_effect(&mut self, index: usize, effect_index: usize) -> bool {
        match self.buses.get_mut(index) {
            Some(bus) if effect_index < bus.effects.len() => {
                bus.effects.remove(effect_index);
                true
            }
            _ => false,
        }
    }

    /// Adds a send from the bus at `index` to `target` at `amount_db`.
    pub fn add_send(&mut self, index: usize, target: impl Into<String>, amount_db: f32) -> bool {
        match self.buses.get_mut(index) {
            Some(bus) => {
                bus.sends.push(BusSend {
                    target: target.into(),
                    amount_db,
                });
                true
            }
            None => false,
        }
    }

    /// Serializes the layout to a JSON resource string.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("AudioBusLayout serializes")
    }

    /// Loads a layout from a JSON resource string.
    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-k56d8): adding a bus, adjusting its volume, toggling
    /// mute, and adding an effect update the bus layout, and the layout saves to
    /// and loads from a resource.
    #[test]
    fn bottom_audio_buses_edit_and_persist() {
        let mut layout = AudioBusLayout::new();

        // Starts with just the Master bus.
        assert_eq!(layout.len(), 1);
        assert_eq!(layout.bus(0).unwrap().name, MASTER_BUS);

        // Add a "Music" bus — routed to Master by default.
        let music = layout.add_bus("Music");
        assert_eq!(music, 1);
        assert_eq!(layout.len(), 2);
        assert_eq!(layout.bus(music).unwrap().sends, vec![BusSend {
            target: MASTER_BUS.to_string(),
            amount_db: 0.0,
        }]);

        // Adjust its volume.
        assert!(layout.set_volume(music, -6.0));
        assert_eq!(layout.bus(music).unwrap().volume_db, -6.0);

        // Toggle mute.
        assert_eq!(layout.toggle_mute(music), Some(true));
        assert!(layout.bus(music).unwrap().muted);

        // Add an effect.
        assert!(layout.add_effect(music, "Reverb"));
        let fx = &layout.bus(music).unwrap().effects;
        assert_eq!(fx.len(), 1);
        assert_eq!(fx[0].name, "Reverb");
        assert!(fx[0].enabled);

        // Master (index 0) cannot be removed; a real bus can.
        assert!(!layout.remove_bus(0), "Master is protected");
        assert_eq!(layout.len(), 2);

        // Save to a resource, then load it back — round-trips exactly.
        let resource = layout.to_json();
        let loaded = AudioBusLayout::from_json(&resource).expect("layout loads");
        assert_eq!(loaded, layout, "saved layout reloads identically");

        // The reloaded layout preserves the edits.
        let reloaded_music = loaded.bus(music).unwrap();
        assert_eq!(reloaded_music.volume_db, -6.0);
        assert!(reloaded_music.muted);
        assert_eq!(reloaded_music.effects[0].name, "Reverb");

        // Removing the bus drops it from the layout.
        assert!(layout.remove_bus(music));
        assert_eq!(layout.len(), 1);
    }

    /// Solo and bypass toggles flip and report their state; out-of-range indices
    /// are rejected.
    #[test]
    fn solo_bypass_and_bounds() {
        let mut layout = AudioBusLayout::new();
        let bus = layout.add_bus("SFX");

        assert_eq!(layout.toggle_solo(bus), Some(true));
        assert_eq!(layout.toggle_solo(bus), Some(false));
        assert_eq!(layout.toggle_bypass(bus), Some(true));
        assert!(layout.bus(bus).unwrap().bypassed);

        assert_eq!(layout.toggle_mute(99), None);
        assert!(!layout.set_volume(99, 1.0));
        assert!(!layout.add_effect(99, "Delay"));
    }
}
