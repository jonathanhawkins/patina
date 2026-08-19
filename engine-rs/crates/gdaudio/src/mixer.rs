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

    /// Rebuilds the mixer's buses from a `default_bus_layout.tres`, effect
    /// chains included (pat-0q2kx.56).
    ///
    /// Godot loads this file at startup (`AudioServer::load_default_bus_layout`,
    /// `servers/audio/audio_server.cpp:1626`) from the project setting
    /// `audio/buses/default_bus_layout`. The resource carries the bus table in
    /// its `[resource]` section and each effect as a `[sub_resource]` block that
    /// a bus references by id:
    ///
    /// ```text
    /// [sub_resource type="AudioEffectAmplify" id="Amplify_1"]
    /// volume_db = 6.0
    ///
    /// [resource]
    /// bus/1/name = "Music"
    /// bus/1/effect/0/effect = SubResource("Amplify_1")
    /// bus/1/effect/0/enabled = true
    /// ```
    ///
    /// ⚠️ A slot with `enabled = false` is SKIPPED rather than inserted and
    /// bypassed — Patina's [`EffectChain`] has no per-slot enable flag, and
    /// inserting a disabled effect as if it were live would silently colour
    /// every bus that has one.
    ///
    /// Returns the resulting bus count. Unknown effect types are skipped (the
    /// bus still loads), so a project using an effect Patina has not
    /// implemented yet keeps its bus layout instead of failing to boot.
    pub fn load_bus_layout_tres(&mut self, text: &str) -> usize {
        let doc = parse_tres_sections(text);
        let entries: Vec<(&str, &str)> = doc
            .resource
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let count = self.load_bus_layout(entries);

        // Effects are attached after the buses exist, so a slot referring to a
        // bus the layout never declared simply finds no target.
        for (key, value) in &doc.resource {
            let Some((bus_idx, slot_field)) = parse_effect_key(key) else {
                continue;
            };
            if slot_field != "effect" {
                continue;
            }
            let Some(id) = parse_sub_resource_id(value) else {
                continue;
            };
            // The sibling `enabled` key governs this slot; Godot defaults it to
            // true when absent.
            let enabled_key = key
                .rsplit_once('/')
                .map(|(base, _)| format!("{base}/enabled"));
            let enabled = enabled_key
                .and_then(|k| {
                    doc.resource
                        .iter()
                        .find(|(dk, _)| *dk == k)
                        .map(|(_, v)| v.as_str())
                })
                .map(|v| v.trim() == "true")
                .unwrap_or(true);
            if !enabled {
                continue;
            }
            let Some(props) = doc.sub_resources.get(&id) else {
                continue;
            };
            let Some(effect) = effect_from_sub_resource(&props.type_name, &props.fields) else {
                continue;
            };
            if let Some(bus) = self.buses.get_mut(bus_idx) {
                bus.effects_mut().add(effect);
            }
        }
        count
    }

    /// Rebuilds the mixer's buses from a Godot audio-bus-layout description.
    ///
    /// Accepts the layout's `key → value` entries using Godot's
    /// `bus/<index>/<field>` naming — as found in `default_bus_layout.tres` or a
    /// project.godot `[audio_bus_layout]` block. Recognized fields: `name`,
    /// `volume_db`, `send`, `send_db`, `mute`, `solo`, `bypass_fx`. Buses are created in
    /// ascending index order (index 0 is the Master bus, defaulted if absent);
    /// string values may be quoted, and unrecognized keys/fields are ignored.
    ///
    /// An empty layout leaves the mixer unchanged. Returns the resulting bus
    /// count.
    pub fn load_bus_layout<'a, I>(&mut self, entries: I) -> usize
    where
        I: IntoIterator<Item = (&'a str, &'a str)>,
    {
        use std::collections::BTreeMap;

        // Group `bus/<idx>/<field>` entries by bus index (BTreeMap keeps the
        // indices ordered so buses rebuild in the layout's order).
        let mut layout: BTreeMap<usize, BTreeMap<String, String>> = BTreeMap::new();
        for (key, value) in entries {
            let Some(rest) = key.trim().strip_prefix("bus/") else {
                continue;
            };
            let mut parts = rest.splitn(2, '/');
            let Some(idx) = parts.next().and_then(|s| s.parse::<usize>().ok()) else {
                continue;
            };
            let Some(field) = parts.next() else {
                continue;
            };
            layout
                .entry(idx)
                .or_default()
                .insert(field.to_string(), unquote(value.trim()).to_string());
        }

        if layout.is_empty() {
            return self.bus_count();
        }

        let max_idx = *layout.keys().max().expect("layout is non-empty");
        let mut buses = Vec::with_capacity(max_idx + 1);
        for i in 0..=max_idx {
            let fields = layout.get(&i);
            let name = fields
                .and_then(|f| f.get("name"))
                .cloned()
                .unwrap_or_else(|| {
                    if i == 0 {
                        "Master".to_string()
                    } else {
                        format!("Bus{i}")
                    }
                });
            let mut bus = AudioBus::new(name);
            if let Some(f) = fields {
                if let Some(v) = f.get("volume_db").and_then(|s| s.parse::<f32>().ok()) {
                    bus.set_volume_db(v);
                }
                if let Some(v) = f.get("send_db").and_then(|s| s.parse::<f32>().ok()) {
                    bus.set_send_db(v);
                }
                if let Some(v) = f.get("send") {
                    bus.set_send(v.as_str());
                }
                if let Some(v) = f.get("mute") {
                    bus.set_mute(v.as_str() == "true");
                }
                if let Some(v) = f.get("solo") {
                    bus.set_solo(v.as_str() == "true");
                }
                if let Some(v) = f.get("bypass_fx") {
                    bus.set_bypass(v.as_str() == "true");
                }
            }
            buses.push(bus);
        }
        self.buses = buses;
        self.bus_count()
    }

    /// Convenience wrapper for [`Self::load_bus_layout`] that parses a raw
    /// `[audio_bus_layout]` section body: one `key = value` per line, with blank
    /// lines, `[section]` headers, and `;` comments ignored.
    pub fn load_bus_layout_str(&mut self, section: &str) -> usize {
        let entries: Vec<(&str, &str)> = section
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if line.is_empty() || line.starts_with('[') || line.starts_with(';') {
                    return None;
                }
                line.split_once('=').map(|(k, v)| (k.trim(), v.trim()))
            })
            .collect();
        self.load_bus_layout(entries)
    }
}

/// Strips a single pair of surrounding double quotes from a Godot string value
/// (`"Master"` → `Master`), leaving unquoted values untouched.
/// A `[sub_resource]` block: its Godot type name and its `key = value` fields.
struct SubResourceBlock {
    type_name: String,
    fields: std::collections::HashMap<String, String>,
}

/// The parts of a `.tres` this loader needs.
struct TresDoc {
    /// `[resource]` entries, in file order — order matters because effect slots
    /// are indexed and must attach in sequence.
    resource: Vec<(String, String)>,
    sub_resources: std::collections::HashMap<String, SubResourceBlock>,
}

/// Splits a `.tres` into its `[sub_resource]` blocks and `[resource]` entries
/// (pat-0q2kx.56).
///
/// Deliberately minimal: a bus layout is a flat list of scalars, so this reads
/// section headers and `key = value` lines and ignores everything else. It does
/// NOT try to be a general Godot resource parser — `gdresource` owns that — and
/// a line it cannot make sense of is skipped rather than failing the boot.
fn parse_tres_sections(text: &str) -> TresDoc {
    let mut doc = TresDoc {
        resource: Vec::new(),
        sub_resources: std::collections::HashMap::new(),
    };
    // None = before any section we care about.
    let mut current: Option<String> = None;
    let mut in_resource = false;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') {
            in_resource = false;
            current = None;
            if line.starts_with("[resource") {
                in_resource = true;
            } else if line.starts_with("[sub_resource") {
                let type_name = section_attr(line, "type").unwrap_or_default();
                if let Some(id) = section_attr(line, "id") {
                    doc.sub_resources.insert(
                        id.clone(),
                        SubResourceBlock {
                            type_name,
                            fields: std::collections::HashMap::new(),
                        },
                    );
                    current = Some(id);
                }
            }
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim().to_string(), value.trim().to_string());
        if in_resource {
            doc.resource.push((key, value));
        } else if let Some(id) = current.as_ref() {
            if let Some(block) = doc.sub_resources.get_mut(id) {
                block.fields.insert(key, value);
            }
        }
    }
    doc
}

/// Reads `name="value"` out of a section header line.
fn section_attr(line: &str, attr: &str) -> Option<String> {
    let needle = format!("{attr}=\"");
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Splits `bus/<i>/effect/<j>/<field>` into the bus index and the field name.
fn parse_effect_key(key: &str) -> Option<(usize, &str)> {
    let rest = key.strip_prefix("bus/")?;
    let (idx, rest) = rest.split_once('/')?;
    let idx = idx.parse::<usize>().ok()?;
    let rest = rest.strip_prefix("effect/")?;
    let (_slot, field) = rest.split_once('/')?;
    Some((idx, field))
}

/// Extracts the id from a `SubResource("id")` reference.
fn parse_sub_resource_id(value: &str) -> Option<String> {
    let inner = value
        .trim()
        .strip_prefix("SubResource(")?
        .strip_suffix(')')?;
    Some(unquote(inner.trim()).to_string())
}

/// Builds an effect from a `[sub_resource]` block's Godot type name
/// (pat-0q2kx.56).
///
/// Returns `None` for a type Patina has not implemented, which the caller
/// treats as "skip this slot" — a project using an unmodelled effect keeps its
/// bus layout rather than failing to boot.
fn effect_from_sub_resource(
    type_name: &str,
    fields: &std::collections::HashMap<String, String>,
) -> Option<Box<dyn crate::effects::AudioEffect>> {
    use crate::effects::{
        AudioEffectAmplify, AudioEffectCompressor, AudioEffectFilter, AudioEffectFilterCascade,
        AudioEffectHardLimiter, AudioEffectLimiter, AudioEffectReverb, FilterMode,
    };

    let f = |name: &str, default: f32| -> f32 {
        fields
            .get(name)
            .and_then(|v| v.trim().parse::<f32>().ok())
            .unwrap_or(default)
    };

    // pat-s3rrv.54 — Godot's documented filter defaults
    // (doc/classes/AudioEffectFilter.xml): cutoff_hz 2000, resonance 1.0,
    // gain 1.0, db DB_6.
    //
    // ⚠️ `resonance` was 0.5 here, which is NOT Godot's default. Nothing pinned
    // it, so a layout that omits the field has been building a differently
    // shaped filter than Godot would.
    let cutoff = f("cutoff_hz", 2000.0);
    let resonance = f("resonance", 1.0);
    // `gain` is a LINEAR multiplier in Godot while the shelf constructors take
    // dB, so 1.0 must mean 0 dB rather than 1 dB. A non-positive gain has no dB
    // value at all, so it clamps instead of producing -inf.
    let gain_linear = f("gain", 1.0);
    let gain_db = if gain_linear > 0.0 {
        20.0 * gain_linear.log10()
    } else {
        0.0
    };
    // `db` is an ENUM INDEX (DB_6=0 … DB_24=3), not a dB number. One biquad is
    // 12 dB/oct; Godot steepens the skirt by cascading identical stages.
    let stages = (f("db", 0.0) as usize).saturating_add(1).clamp(1, 4);

    // One stage stays a bare filter so the common case allocates nothing extra;
    // more becomes a cascade. Both carry the Godot class name, because `name()`
    // is "Filter" for all seven classes and a bus slot has to say which it is.
    let filter =
        |proto: AudioEffectFilter, class: &'static str| -> Box<dyn crate::effects::AudioEffect> {
            if stages > 1 {
                Box::new(AudioEffectFilterCascade::repeated(proto, stages, class))
            } else {
                Box::new(proto.with_godot_class(class))
            }
        };

    match type_name {
        "AudioEffectAmplify" => Some(Box::new(AudioEffectAmplify::new(f("volume_db", 0.0)))),
        "AudioEffectReverb" => Some(Box::new(AudioEffectReverb::new())),
        // pat-s3rrv.54: the whole filter family. The biquad DSP already existed
        // and was unit-tested; only three of the eight classes could be named
        // from a bus layout, and the other five were silently dropped.
        "AudioEffectLowPassFilter" => Some(filter(
            AudioEffectFilter::low_pass(cutoff, resonance),
            "AudioEffectLowPassFilter",
        )),
        "AudioEffectHighPassFilter" => Some(filter(
            AudioEffectFilter::high_pass(cutoff, resonance),
            "AudioEffectHighPassFilter",
        )),
        "AudioEffectBandPassFilter" => Some(filter(
            AudioEffectFilter::band_pass(cutoff, resonance),
            "AudioEffectBandPassFilter",
        )),
        "AudioEffectNotchFilter" => Some(filter(
            AudioEffectFilter::notch(cutoff, resonance),
            "AudioEffectNotchFilter",
        )),
        "AudioEffectLowShelfFilter" => Some(filter(
            AudioEffectFilter::low_shelf(cutoff, gain_db),
            "AudioEffectLowShelfFilter",
        )),
        "AudioEffectHighShelfFilter" => Some(filter(
            AudioEffectFilter::high_shelf(cutoff, gain_db),
            "AudioEffectHighShelfFilter",
        )),
        // Bounded from BOTH sides, so a high-pass and a low-pass in series
        // rather than any single response; `db` repeats the pair.
        //
        // ⚠️ APPROXIMATION, flagged rather than hidden: Godot's BANDLIMIT is its
        // own mode in `audio_filter_sw.h` and Patina's `FilterMode` has no
        // equivalent. HP+LP at one corner is the right SHAPE, not a bit-exact
        // match.
        "AudioEffectBandLimitFilter" => Some(Box::new(AudioEffectFilterCascade::new(
            (0..stages)
                .flat_map(|_| {
                    [
                        AudioEffectFilter::high_pass(cutoff, resonance),
                        AudioEffectFilter::low_pass(cutoff, resonance),
                    ]
                })
                .collect(),
            "AudioEffectBandLimitFilter",
        ))),
        // The base class is instantiable in Godot and defaults to a low-pass
        // response; a layout naming it should not lose its slot.
        "AudioEffectFilter" => Some(filter(
            AudioEffectFilter::low_pass(cutoff, resonance),
            "AudioEffectFilter",
        )),
        // pat-s3rrv.53: the dynamics family. The DSP existed and was unit-tested;
        // without these arms a bus layout naming them built an EMPTY chain, so a
        // Master bus ending in a compressor silently did nothing.
        "AudioEffectCompressor" => {
            // ⚠️ Godot stores attack in MICROSECONDS (`attack_us`, default 20.0)
            // while the sibling is `release_ms`. `AudioEffectCompressor::new`
            // takes milliseconds, so this MUST divide by 1000 — passing 20.0
            // straight through is a 1000x attack time that still runs, just
            // with an envelope that never catches transients.
            let mut c = AudioEffectCompressor::new(
                f("threshold", 0.0),
                f("ratio", 4.0),
                f("attack_us", 20.0) / 1000.0,
                f("release_ms", 250.0),
            );
            c.gain_db = f("gain", 0.0);
            Some(Box::new(c))
        }
        "AudioEffectLimiter" => Some(Box::new(AudioEffectLimiter::new(
            f("ceiling_db", -0.1),
            f("soft_clip_db", 2.0),
        ))),
        "AudioEffectHardLimiter" => Some(Box::new(AudioEffectHardLimiter::new(
            // -0.3 is upstream's HardLimiter default and differs from Limiter's
            // -0.1 (doc/classes/AudioEffectHardLimiter.xml).
            f("ceiling_db", -0.3),
            f("pre_gain_db", 0.0),
        ))),
        _ => None,
    }
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value)
}

impl Default for AudioMixer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod bus_layout_tests {
    use super::*;
    use crate::effects::AudioEffectAmplify;

    const LAYOUT: &str = r#"
[audio_bus_layout]

bus/0/name = "Master"
bus/0/volume_db = 0.0
bus/0/mute = false
bus/1/name = "Music"
bus/1/volume_db = -6.0
bus/1/send = "Master"
bus/1/mute = false
bus/2/name = "SFX"
bus/2/volume_db = -3.0
bus/2/send = "Master"
bus/2/solo = true
"#;

    #[test]
    fn load_bus_layout_str_builds_named_buses() {
        let mut mixer = AudioMixer::new();
        let count = mixer.load_bus_layout_str(LAYOUT);
        assert_eq!(count, 3);
        assert_eq!(mixer.bus_count(), 3);
        assert_eq!(mixer.get_bus(0).unwrap().name(), "Master");
        assert_eq!(mixer.get_bus(1).unwrap().name(), "Music");
        assert_eq!(mixer.get_bus(2).unwrap().name(), "SFX");
        assert!((mixer.get_bus(1).unwrap().volume_db() - (-6.0)).abs() < 1e-6);
        assert_eq!(mixer.get_bus(1).unwrap().send(), "Master");
        assert!(mixer.get_bus(2).unwrap().is_solo());
        assert!(!mixer.get_bus(1).unwrap().is_solo());
    }

    #[test]
    fn load_bus_layout_ignores_unknown_and_unquotes() {
        let mut mixer = AudioMixer::new();
        let entries = vec![
            ("bus/0/name", "\"Master\""),
            ("bus/1/name", "\"Ambience\""),
            ("bus/1/send", "\"Master\""),
            ("bus/1/pitch_scale", "2.0"), // unrecognized field → ignored
            ("some/other/key", "5"),      // non-bus key → ignored
        ];
        let count = mixer.load_bus_layout(entries);
        assert_eq!(count, 2);
        assert_eq!(mixer.get_bus(1).unwrap().name(), "Ambience"); // quotes stripped
        assert_eq!(mixer.get_bus(1).unwrap().send(), "Master");
    }

    #[test]
    fn missing_master_index_still_defaults_to_master() {
        // A layout that only defines bus/1 must still yield a Master at index 0.
        let mut mixer = AudioMixer::new();
        let count = mixer.load_bus_layout(vec![("bus/1/name", "\"Voice\"")]);
        assert_eq!(count, 2);
        assert_eq!(mixer.get_bus(0).unwrap().name(), "Master");
        assert_eq!(mixer.get_bus(1).unwrap().name(), "Voice");
    }

    #[test]
    fn empty_layout_keeps_existing_master() {
        let mut mixer = AudioMixer::new();
        let count = mixer.load_bus_layout_str("[audio_bus_layout]\n; nothing here\n");
        assert_eq!(count, 1); // just the default Master
        assert_eq!(mixer.get_bus(0).unwrap().name(), "Master");
    }

    #[test]
    fn scene_can_add_effect_to_loaded_bus() {
        // Bead ACCEPT: "scene can add effect to bus."
        let mut mixer = AudioMixer::new();
        mixer.load_bus_layout_str(LAYOUT);
        let sfx = mixer.get_bus_by_name("SFX").expect("SFX bus from layout");
        let bus = mixer.get_bus_mut(sfx).unwrap();
        assert!(bus.effects().is_empty());
        bus.effects_mut()
            .add(Box::new(AudioEffectAmplify::new(6.0)));
        assert_eq!(bus.effects().len(), 1);
    }
}
