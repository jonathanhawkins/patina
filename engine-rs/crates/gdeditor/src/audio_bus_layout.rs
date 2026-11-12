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

    /// Renames the bus at `index`. The `Master` bus (index 0) is fixed and
    /// cannot be renamed, matching Godot. Returns whether a rename happened.
    pub fn rename_bus(&mut self, index: usize, name: impl Into<String>) -> bool {
        if index == 0 {
            return false;
        }
        match self.buses.get_mut(index) {
            Some(bus) => {
                bus.name = name.into();
                true
            }
            None => false,
        }
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

    /// Toggles the `enabled` flag of the effect at `effect_index` on the bus at
    /// `index`, returning the new state.
    pub fn toggle_effect(&mut self, index: usize, effect_index: usize) -> Option<bool> {
        let bus = self.buses.get_mut(index)?;
        let effect = bus.effects.get_mut(effect_index)?;
        effect.enabled = !effect.enabled;
        Some(effect.enabled)
    }

    /// Sets the **output bus** that the bus at `index` routes to. This mirrors
    /// Godot's per-bus output dropdown: a bus has a single primary output (its
    /// first send). The `Master` bus (index 0) has no output and is rejected.
    /// Returns whether the output was set.
    pub fn set_output(&mut self, index: usize, target: impl Into<String>) -> bool {
        if index == 0 {
            return false;
        }
        let target = target.into();
        match self.buses.get_mut(index) {
            Some(bus) => {
                match bus.sends.first_mut() {
                    Some(send) => send.target = target,
                    None => bus.sends.push(BusSend {
                        target,
                        amount_db: 0.0,
                    }),
                }
                true
            }
            None => false,
        }
    }

    /// The output bus that the bus at `index` routes to (its first send's
    /// target), or `None` for `Master`/unrouted buses.
    pub fn output_of(&self, index: usize) -> Option<&str> {
        self.buses
            .get(index)
            .and_then(|bus| bus.sends.first())
            .map(|send| send.target.as_str())
    }

    /// Replaces this layout with the default (a single `Master` bus). Mirrors
    /// Godot's "Load Default" / "Create" actions.
    pub fn reset_to_default(&mut self) {
        *self = Self::default();
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

    /// Swaps two effects in a bus's chain, which is how the panel's
    /// drag-reorder lands (Godot's `AudioServer.swap_bus_effects`).
    ///
    /// Order is audible — a filter before a reverb is not the same sound as a
    /// filter after it — so this is a real edit, not a cosmetic list shuffle.
    /// Returns whether both slots existed; a no-op swap of a slot with itself
    /// still reports success, matching the drag that ends where it began.
    pub fn swap_effects(&mut self, index: usize, a: usize, b: usize) -> bool {
        match self.buses.get_mut(index) {
            Some(bus) if a < bus.effects.len() && b < bus.effects.len() => {
                bus.effects.swap(a, b);
                true
            }
            _ => false,
        }
    }

    /// Serializes the layout as a Godot `AudioBusLayout` `.tres`.
    ///
    /// ⚠️ The grammar here is not free-form: it is exactly what
    /// `gdaudio::mixer::AudioMixer::load_bus_layout_tres` parses, because the
    /// runner reads this same file at boot. The editor writing a layout the
    /// runtime cannot read back is the whole failure this replaces — the panel
    /// previously kept everything in memory and the Save button only logged a
    /// line.
    ///
    /// Effects become `[sub_resource]` blocks referenced by
    /// `bus/<i>/effect/<slot>/effect = SubResource("id")`, with the sibling
    /// `enabled` key. ⚠️ The loader SKIPS a slot whose `enabled` is false — it
    /// has no per-slot bypass — so a disabled effect round-trips as absent
    /// rather than as a silent one. That is the loader's deliberate choice, and
    /// writing the flag faithfully is what keeps the two halves agreeing.
    pub fn to_tres(&self) -> String {
        let mut subs = String::new();
        let mut body = String::new();
        let mut steps = 0usize;

        for (i, bus) in self.buses.iter().enumerate() {
            body.push_str(&format!("bus/{i}/name = \"{}\"\n", bus.name));
            body.push_str(&format!("bus/{i}/solo = {}\n", bus.soloed));
            body.push_str(&format!("bus/{i}/mute = {}\n", bus.muted));
            body.push_str(&format!("bus/{i}/bypass_fx = {}\n", bus.bypassed));
            body.push_str(&format!("bus/{i}/volume_db = {:?}\n", bus.volume_db));
            // Godot writes the primary output as `send`; Master has none.
            if let Some(send) = bus.sends.first() {
                body.push_str(&format!("bus/{i}/send = \"{}\"\n", send.target));
            }
            for (slot, fx) in bus.effects.iter().enumerate() {
                let class = godot_effect_class(&fx.name);
                // Unique per (bus, slot) so two buses carrying the same effect
                // type do not collide on one id and share a sub_resource.
                let id = format!("{class}_{i}_{slot}");
                subs.push_str(&format!("[sub_resource type=\"{class}\" id=\"{id}\"]\n\n"));
                steps += 1;
                body.push_str(&format!(
                    "bus/{i}/effect/{slot}/effect = SubResource(\"{id}\")\n"
                ));
                body.push_str(&format!("bus/{i}/effect/{slot}/enabled = {}\n", fx.enabled));
            }
        }

        format!(
            "[gd_resource type=\"AudioBusLayout\" load_steps={} format=3]\n\n{subs}[resource]\n{body}",
            steps + 1
        )
    }

    /// Parses a Godot `AudioBusLayout` `.tres` back into an editor layout.
    ///
    /// Deliberately tolerant, because this reads files Godot wrote as well as
    /// files this editor wrote: unknown keys are ignored, buses are sized by
    /// the highest index seen (so a sparse or out-of-order file still loads),
    /// and a document with no `bus/…` keys at all yields the default layout
    /// rather than an empty one — a layout with no Master bus is not a state
    /// the panel can represent.
    ///
    /// ⚠️ Written by hand rather than reused from `gdaudio`: `gdeditor` has no
    /// dependency on that crate, and its parser is private. The two must agree
    /// on the grammar, which is what `to_tres`'s round-trip test pins.
    pub fn from_tres(text: &str) -> Self {
        // effect slot -> (class, enabled), keyed by (bus, slot).
        let mut sub_types: Vec<(String, String)> = Vec::new(); // (id, type)
        let mut entries: Vec<(String, String)> = Vec::new();
        let mut in_resource = false;
        let mut pending_sub: Option<String> = None;

        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_resource = line.starts_with("[resource]");
                pending_sub = None;
                if line.starts_with("[sub_resource") {
                    let ty = attr(line, "type");
                    let id = attr(line, "id");
                    if let (Some(ty), Some(id)) = (ty, id) {
                        sub_types.push((id.clone(), ty));
                        pending_sub = Some(id);
                    }
                }
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim().to_string(), value.trim().to_string());
            // `[gd_resource]` header aside, only the [resource] block carries
            // the bus table; sub_resource fields are not bus keys.
            if in_resource && pending_sub.is_none() {
                entries.push((key, value));
            }
        }

        let mut layout = Self { buses: Vec::new() };
        let bus_count = entries
            .iter()
            .filter_map(|(k, _)| bus_index(k))
            .max()
            .map(|m| m + 1)
            .unwrap_or(0);
        if bus_count == 0 {
            return Self::default();
        }
        for _ in 0..bus_count {
            layout.buses.push(AudioBus::new(MASTER_BUS));
        }

        for (key, value) in &entries {
            let Some(idx) = bus_index(key) else { continue };
            let Some(bus) = layout.buses.get_mut(idx) else {
                continue;
            };
            let field = key.rsplit('/').next().unwrap_or("");
            let raw = unquote(value);
            match field {
                "name" => bus.name = raw.to_string(),
                "volume_db" => {
                    if let Ok(v) = raw.parse::<f32>() {
                        bus.volume_db = v;
                    }
                }
                "mute" => bus.muted = raw == "true",
                "solo" => bus.soloed = raw == "true",
                "bypass_fx" => bus.bypassed = raw == "true",
                "send" => {
                    bus.sends = vec![BusSend {
                        target: raw.to_string(),
                        amount_db: 0.0,
                    }]
                }
                "effect" => {
                    // `bus/<i>/effect/<slot>/effect = SubResource("id")`
                    let id = raw
                        .strip_prefix("SubResource(")
                        .and_then(|r| r.strip_suffix(')'))
                        .map(|r| unquote(r.trim()).to_string());
                    if let Some(id) = id {
                        let class = sub_types
                            .iter()
                            .find(|(sid, _)| *sid == id)
                            .map(|(_, ty)| ty.clone())
                            .unwrap_or_else(|| "AudioEffectAmplify".to_string());
                        bus.effects.push(AudioEffect {
                            name: class,
                            enabled: true,
                        });
                    }
                }
                "enabled" => {
                    // Applies to the slot just pushed, which is the last one:
                    // the writer always emits `effect` before `enabled`.
                    if let Some(last) = bus.effects.last_mut() {
                        last.enabled = raw == "true";
                    }
                }
                _ => {}
            }
        }
        layout
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

    // -- .tres round-trip (pat-s3rrv.52) ------------------------------------

    /// A layout with a second bus and an effect, as the panel would build it.
    fn music_layout() -> AudioBusLayout {
        let mut l = AudioBusLayout::new();
        let i = l.add_bus("Music");
        l.set_volume(i, -12.0);
        l.set_output(i, MASTER_BUS);
        l.add_effect(i, "AudioEffectAmplify");
        l
    }

    /// **The round-trip.** What the panel saves is what it loads back.
    #[test]
    fn tres_round_trips_through_from_tres() {
        let before = music_layout();
        let after = AudioBusLayout::from_tres(&before.to_tres());
        assert_eq!(
            after.len(),
            2,
            "both buses must survive the round-trip:\n{}",
            before.to_tres()
        );
        let bus = after.bus(1).expect("the Music bus");
        assert_eq!(bus.name, "Music");
        assert_eq!(bus.volume_db, -12.0);
        assert_eq!(bus.effects.len(), 1, "the effect slot must survive");
        assert_eq!(bus.effects[0].name, "AudioEffectAmplify");
        assert!(bus.effects[0].enabled);
        assert_eq!(after.output_of(1), Some(MASTER_BUS));
    }

    /// ⚠️ The emitted keys are a CONTRACT with `gdaudio`'s loader, which is a
    /// separate crate with its own private parser. Round-tripping through our
    /// own `from_tres` alone would happily pass on a grammar the runtime cannot
    /// read, so the exact key shapes are pinned here too.
    #[test]
    fn tres_emits_the_grammar_the_runtime_loader_parses() {
        let text = music_layout().to_tres();
        for expected in [
            "[gd_resource type=\"AudioBusLayout\"",
            "bus/0/name = \"Master\"",
            "bus/1/name = \"Music\"",
            "bus/1/volume_db = -12.0",
            "bus/1/send = \"Master\"",
            "bus/1/effect/0/effect = SubResource(",
            "bus/1/effect/0/enabled = true",
            "[sub_resource type=\"AudioEffectAmplify\"",
        ] {
            assert!(
                text.contains(expected),
                "missing {expected:?} in emitted layout:\n{text}"
            );
        }
    }

    /// Two buses carrying the SAME effect type must not share one sub_resource
    /// id — otherwise the second silently aliases the first.
    #[test]
    fn effect_sub_resource_ids_are_unique_per_bus_and_slot() {
        let mut l = AudioBusLayout::new();
        let a = l.add_bus("A");
        let b = l.add_bus("B");
        l.add_effect(a, "AudioEffectAmplify");
        l.add_effect(b, "AudioEffectAmplify");
        let text = l.to_tres();
        let ids: Vec<&str> = text
            .lines()
            .filter(|l| l.starts_with("[sub_resource"))
            .collect();
        assert_eq!(ids.len(), 2, "one sub_resource per effect slot:\n{text}");
        assert_ne!(ids[0], ids[1], "ids must differ:\n{text}");
    }

    /// A short effect name still serializes to the Godot class name, since the
    /// loader matches on the class.
    #[test]
    fn short_effect_names_serialize_as_godot_classes() {
        let mut l = AudioBusLayout::new();
        l.add_effect(0, "Amplify");
        assert!(l
            .to_tres()
            .contains("[sub_resource type=\"AudioEffectAmplify\""));
    }

    /// Reordering is audible, so it must reach the saved file.
    #[test]
    fn swap_effects_reorders_the_chain_and_the_saved_file() {
        let mut l = AudioBusLayout::new();
        l.add_effect(0, "AudioEffectAmplify");
        l.add_effect(0, "AudioEffectReverb");
        assert!(l.swap_effects(0, 0, 1));

        let bus = l.bus(0).expect("master");
        assert_eq!(bus.effects[0].name, "AudioEffectReverb");
        assert_eq!(bus.effects[1].name, "AudioEffectAmplify");

        let text = l.to_tres();
        let reverb = text.find("AudioEffectReverb").expect("reverb present");
        let amplify = text.find("AudioEffectAmplify").expect("amplify present");
        assert!(
            reverb < amplify,
            "slot 0 must be the reverb after the swap:\n{text}"
        );
        // …and it survives a load.
        let after = AudioBusLayout::from_tres(&text);
        assert_eq!(after.bus(0).unwrap().effects[0].name, "AudioEffectReverb");
    }

    /// Out-of-range slots are rejected rather than panicking on a stale drag.
    #[test]
    fn swap_effects_rejects_out_of_range_slots() {
        let mut l = AudioBusLayout::new();
        l.add_effect(0, "AudioEffectAmplify");
        assert!(!l.swap_effects(0, 0, 5));
        assert!(!l.swap_effects(9, 0, 0));
    }

    /// A disabled effect round-trips as disabled. ⚠️ The runtime loader SKIPS
    /// disabled slots entirely — it has no per-slot bypass — so this flag is
    /// the difference between "present but silent" and "not loaded".
    #[test]
    fn a_disabled_effect_keeps_its_flag() {
        let mut l = AudioBusLayout::new();
        l.add_effect(0, "AudioEffectAmplify");
        l.toggle_effect(0, 0);
        let text = l.to_tres();
        assert!(text.contains("bus/0/effect/0/enabled = false"), "{text}");
        let after = AudioBusLayout::from_tres(&text);
        assert!(!after.bus(0).unwrap().effects[0].enabled);
    }

    /// A document with no bus table is the default layout, not an empty one —
    /// a layout with no Master bus is not a state the panel can represent.
    #[test]
    fn an_empty_document_loads_the_default_layout() {
        let l = AudioBusLayout::from_tres(
            "[gd_resource type=\"AudioBusLayout\" format=3]\n\n[resource]\n",
        );
        assert_eq!(l.len(), 1);
        assert_eq!(l.bus(0).unwrap().name, MASTER_BUS);
    }

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
        assert_eq!(
            layout.bus(music).unwrap().sends,
            vec![BusSend {
                target: MASTER_BUS.to_string(),
                amount_db: 0.0,
            }]
        );

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

    /// Renaming, effect toggling, and output routing behave like Godot: Master
    /// is fixed (cannot be renamed and has no output), effects flip enabled, and
    /// a bus's output routes through its first send.
    #[test]
    fn rename_effect_toggle_and_output_routing() {
        let mut layout = AudioBusLayout::new();
        let music = layout.add_bus("Music");

        // Master (index 0) cannot be renamed; a real bus can.
        assert!(!layout.rename_bus(0, "NotMaster"));
        assert_eq!(layout.bus(0).unwrap().name, MASTER_BUS);
        assert!(layout.rename_bus(music, "BGM"));
        assert_eq!(layout.bus(music).unwrap().name, "BGM");

        // New buses route to Master by default; output can be re-routed.
        assert_eq!(layout.output_of(music), Some(MASTER_BUS));
        let sfx = layout.add_bus("SFX");
        assert!(layout.set_output(sfx, "BGM"));
        assert_eq!(layout.output_of(sfx), Some("BGM"));
        // Master has no output and rejects routing.
        assert!(!layout.set_output(0, "BGM"));
        assert_eq!(layout.output_of(0), None);

        // Effects flip their enabled flag.
        assert!(layout.add_effect(music, "Reverb"));
        assert_eq!(layout.toggle_effect(music, 0), Some(false));
        assert!(!layout.bus(music).unwrap().effects[0].enabled);
        assert_eq!(layout.toggle_effect(music, 0), Some(true));
        assert_eq!(layout.toggle_effect(music, 99), None);

        // Reset drops everything back to a lone Master bus.
        layout.reset_to_default();
        assert_eq!(layout.len(), 1);
        assert_eq!(layout.bus(0).unwrap().name, MASTER_BUS);
    }
}

/// The Godot class name for an effect the panel stores by short or full name.
///
/// The Add-Effect dropdown offers full class names, but older layouts (and the
/// module's own doc example) use the short form, so both must serialize to the
/// class name the `.tres` grammar expects.
fn godot_effect_class(name: &str) -> String {
    if name.starts_with("AudioEffect") {
        name.to_string()
    } else {
        format!("AudioEffect{name}")
    }
}

/// The bus index in a `bus/<i>/…` key, or `None` for any other key.
fn bus_index(key: &str) -> Option<usize> {
    key.strip_prefix("bus/")?.split_once('/')?.0.parse().ok()
}

/// Strips one layer of surrounding double quotes.
fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value)
}

/// Reads `key="value"` out of a section header line.
fn attr(line: &str, key: &str) -> Option<String> {
    let pat = format!("{key}=\"");
    let start = line.find(&pat)? + pat.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}
