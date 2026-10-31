//! Per-glyph BBCode text effects for `RichTextLabel` (pat-nzgdu.157).
//!
//! Godot's model: a `[wave]`/`[tornado]`/… tag (or a script-defined
//! `RichTextEffect`) is handed a mutable [`CharFXTransform`] once per covered
//! glyph, per frame. The effect writes `offset`/`color`/`visible` on it and the
//! label draws that glyph accordingly. This module is the engine-side half:
//! the [`CharFXTransform`] payload plus the six built-in effects, as pure
//! functions of `(relative_index, elapsed_time)` so they are deterministic and
//! testable without a renderer or a frame loop.
//!
//! **Deviation from Godot, deliberate:** Godot phases `wave`/`tornado` by the
//! glyph's *pixel x* (`sin(elapsed * freq + glyph_x / 50)`), which couples the
//! effect to font metrics and line wrapping. We phase by `relative_index`
//! instead — the same visual (a travelling wave), but reproducible from the
//! character index alone, which is what makes it assertable in a test. The
//! amplitude/frequency semantics and the tag names/params match Godot.
//!
//! Script-defined custom effects **are** dispatched, as of pat-fk17e.2. The
//! engine side runs as a PRE-PASS in
//! [`control_render`](crate::control_render): it seeds a phantom node with
//! [`CharFXTransform::to_properties`], calls the effect's
//! `_process_custom_fx` through `SceneTree::call_script_method`, then collects
//! the mutation with [`CharFXTransform::read_back`]. A handler that returns
//! `false` hides the glyph, which Godot treats as separate from clearing
//! `char_fx.visible`. [`match_custom_effect`] maps a BBCode tag to an entry of
//! `RichTextLabel.custom_effects`, first match wins.
//!
//! ⚠️ This paragraph previously said the opposite — that effects were "NOT
//! dispatched here" and that two of four blockers were "the real remaining
//! work". All four are resolved, and the note outlived them. It is recorded
//! here because a stale "remaining work" note in a module doc is worse than no
//! note: it reads as current, and it cost a worker most of a slice picking up
//! blocker 3 before discovering `to_properties`/`read_back` already existed.
//! The four, and what settled each:
//!
//! 1. `draw_control` takes `&SceneTree` — calling a script needs `&mut`.
//!    **Never a blocker**, and the reason is the design the implementation
//!    used. It reads as though the feature needs `&mut` threaded through
//!    `draw_control`, which would reach every caller of `control_draw_items`
//!    including the editor viewport. It does not: the sanctioned shape is a
//!    PRE-PASS that runs the scripts with `&mut self`, caches what they
//!    produced, and lets the immutable draw path read the cache. There were
//!    already two of them —
//!    [`SceneTree::flush_pending_redraws`](crate::scene_tree::SceneTree::flush_pending_redraws)
//!    for a node's own `_draw`, and
//!    [`SceneTree::flush_scripted_resources`](crate::scene_tree::SceneTree::flush_scripted_resources)
//!    for scripted `StyleBox`/`Texture2D` (pat-s3rrv.160). Custom effects took
//!    a third of the same shape, not a signature change.
//! 2. `RichTextLabel` had no `custom_effects` property and nothing mapped a
//!    BBCode tag to an effect's `bbcode`. **Resolved**: the property is
//!    registered in `class_db.rs` and [`match_custom_effect`] does the mapping.
//! 3. `CharFXTransform` had no `Variant` encoding, so it could not cross
//!    `ScriptInstance::call_method(&[Variant])` and come back mutated.
//!    **Resolved**: it rides as the property bag of a phantom node —
//!    [`CharFXTransform::to_properties`] out, [`CharFXTransform::read_back`]
//!    in — so no new `Variant` type was needed.
//! 4. There was no script-backed Resource to hang an effect on. **Resolved by
//!    pat-fk17e.1.** A GDScript that `extends Resource` instantiates as an
//!    object carrying a `ScriptInstance`, via the scene accessor's
//!    `instantiate_script_resource`, and `load("res://effect.tres")` on a
//!    `.tres` with `script = ExtResource(..)` yields that object. See
//!    `crate::scripting`'s module doc for why the instance id — not a `script`
//!    field on `ResourceRef` — is the representation. The object is still a
//!    phantom `Node` under the scene root, a carrier limitation rather than an
//!    absence of the thing.
//!
//! Remaining parity gap, small and real: `to_properties` seeds `glyph_flags`
//! and `glyph_index` at `0` because this struct models neither. Godot supplies
//! both per glyph, so a script reading `char_fx.glyph_index` sees `0` for every
//! glyph rather than its real index. They are seeded rather than omitted so
//! arithmetic on them does not fault on `Nil`.

use std::collections::HashMap;

use gdcore::math::{Color, Vector2, Vector2i};
use gdvariant::Variant;

/// The mutable per-glyph payload an effect writes to — Godot's
/// `CharFXTransform`, minus the fields that need a live font/RID.
#[derive(Clone, Debug, PartialEq)]
pub struct CharFXTransform {
    /// The `[start, end)` glyph range the effect covers, in absolute indices.
    pub range: (usize, usize),
    /// Seconds the label has been animating. Effects are functions of this.
    pub elapsed_time: f32,
    /// Cleared by an effect to skip drawing this glyph entirely.
    pub visible: bool,
    /// True while processing an outline pass (never set by the built-ins).
    pub outline: bool,
    /// Pixel offset applied to this glyph. The whole point of `[wave]`.
    pub offset: Vector2,
    /// The glyph's colour; effects may tint or fade it.
    pub color: Color,
    /// Tag parameters, e.g. `[ghost speed=2]` -> `{"speed": "2"}`.
    pub env: HashMap<String, String>,
    /// Index of this glyph within the effect's range (0 at the range start).
    pub relative_index: usize,
    /// Index of this glyph within the whole text.
    pub absolute_index: usize,
    /// Total glyphs covered by this effect.
    pub glyph_count: usize,
}

impl CharFXTransform {
    /// A transform for one glyph, with the effect's defaults applied.
    pub fn new(
        absolute_index: usize,
        range: (usize, usize),
        elapsed_time: f32,
        color: Color,
    ) -> Self {
        Self {
            range,
            elapsed_time,
            visible: true,
            outline: false,
            offset: Vector2::ZERO,
            color,
            env: HashMap::new(),
            relative_index: absolute_index.saturating_sub(range.0),
            absolute_index,
            glyph_count: range.1.saturating_sub(range.0),
        }
    }

    // -- script-visible surface (pat-fk17e.2) -------------------------------

    /// The property bag a script sees as the `char_fx` argument of
    /// `_process_custom_fx`.
    ///
    /// This is how a `CharFXTransform` crosses into a script without a new
    /// `Variant` type: it rides as the property bag of a phantom node, so the
    /// script's `char_fx.offset = …` is serviced by the existing
    /// `set_node_property` path and the mutation lands in the tree, where
    /// [`read_back`](Self::read_back) collects it. See the module docs.
    ///
    /// ⚠️ The names come from the ClassDB registration, NOT from this struct's
    /// fields. `glyph_flags` and `glyph_index` are registered with no field
    /// here, and are seeded with their registered defaults so a script reading
    /// one sees `0` rather than `Nil` — a `Nil` would make
    /// `char_fx.glyph_index + 1` a hard runtime error in GDScript rather than
    /// an unused value.
    pub fn to_properties(&self) -> Vec<(String, Variant)> {
        vec![
            // Registered but not modelled: seeded at its default so arithmetic
            // on it does not fault.
            ("glyph_flags".to_string(), Variant::Int(0)),
            // Godot types `range` as Vector2i (start, end).
            (
                "range".to_string(),
                Variant::Vector2i(Vector2i::new(self.range.0 as i32, self.range.1 as i32)),
            ),
            // ⚠️ `Variant::Float` is f64 while this field is f32. The cast is
            // mandatory, not cosmetic — mixing the two silently is how a
            // previous sweep took the workspace down.
            (
                "elapsed_time".to_string(),
                Variant::Float(f64::from(self.elapsed_time)),
            ),
            ("visible".to_string(), Variant::Bool(self.visible)),
            ("outline".to_string(), Variant::Bool(self.outline)),
            ("offset".to_string(), Variant::Vector2(self.offset)),
            ("color".to_string(), Variant::Color(self.color)),
            (
                "env".to_string(),
                Variant::Dictionary(
                    self.env
                        .iter()
                        .map(|(k, v)| (k.clone(), Variant::String(v.clone())))
                        .collect::<HashMap<String, Variant>>()
                        .into(),
                ),
            ),
            ("glyph_index".to_string(), Variant::Int(0)),
            (
                "glyph_count".to_string(),
                Variant::Int(self.glyph_count as i64),
            ),
            (
                "relative_index".to_string(),
                Variant::Int(self.relative_index as i64),
            ),
            (
                "absolute_index".to_string(),
                Variant::Int(self.absolute_index as i64),
            ),
        ]
    }

    /// Collects what `_process_custom_fx` wrote back onto the phantom node.
    ///
    /// Only `offset`, `color` and `visible` are read: those are the three the
    /// draw path honours, and they are what Godot documents an effect as
    /// setting. `get` is the node-property reader.
    ///
    /// ⚠️ A property the script did not touch must keep the value it was
    /// SEEDED with, so each read falls through to the current field on a type
    /// mismatch or `Nil`. Defaulting to `Vector2::ZERO`/`visible = true`
    /// instead would silently discard a built-in effect's contribution when
    /// both apply to the same glyph.
    pub fn read_back(&mut self, get: impl Fn(&str) -> Variant) {
        if let Variant::Vector2(v) = get("offset") {
            self.offset = v;
        }
        if let Variant::Color(c) = get("color") {
            self.color = c;
        }
        if let Variant::Bool(b) = get("visible") {
            self.visible = b;
        }
    }
}

/// Resolves a BBCode opener to the installed custom effect that claims it.
///
/// `effects` is `(effect object id, its `bbcode` property)`, in the order they
/// appear in `RichTextLabel.custom_effects`; the FIRST match wins, which is
/// Godot's behaviour for two effects declaring the same tag.
///
/// ⚠️ Matched case-insensitively **because the BBCode parser lowercases the tag
/// before it gets here** (`parse_tag_params(&lower)` in `control_render`). An
/// exact compare would mean an effect whose `bbcode` is `"MyTag"` could never
/// be reached by `[MyTag]` — it would silently degrade to plain text, which is
/// indistinguishable from the tag not being registered at all.
pub fn match_custom_effect(tag: &str, effects: &[(u64, String)]) -> Option<u64> {
    effects
        .iter()
        .find(|(_, bbcode)| bbcode.eq_ignore_ascii_case(tag))
        .map(|(id, _)| *id)
}

// ---------------------------------------------------------------------------
// Pre-pass result cache (pat-fk17e.2)
// ---------------------------------------------------------------------------

/// Node-property key: per-glyph offsets the pre-pass produced.
pub const CHARFX_CACHE_OFFSET: &str = "__charfx_offset__";
/// Node-property key: per-glyph colors the pre-pass produced.
pub const CHARFX_CACHE_COLOR: &str = "__charfx_color__";
/// Node-property key: per-glyph visibility, `1` visible and `0` hidden.
pub const CHARFX_CACHE_VISIBLE: &str = "__charfx_visible__";

/// Packs the pre-pass's per-glyph results into node properties on the label.
///
/// The three arrays are DENSE and indexed by absolute glyph index, so the draw
/// path can index straight in without carrying a map. `results[i]` is glyph `i`
/// of the label's tag-stripped text.
///
/// ⚠️ Why node properties rather than a field on `SceneTree` (which is where
/// `flush_pending_redraws` puts `custom_draw_commands`): the engine-internal
/// `__`-prefixed node property is an established carrier in this crate — the
/// `.tres` loader already hands the scripted-resource pre-pass its input as a
/// `__resscript__<prop>` companion — and it keeps this change out of
/// `scene_tree.rs`. A `SceneTree` field is the tidier home and is worth
/// revisiting; it is avoided here only to stay out of a contended file.
pub fn cache_to_properties(results: &[CharFXTransform]) -> Vec<(String, Variant)> {
    vec![
        (
            CHARFX_CACHE_OFFSET.to_string(),
            Variant::PackedVector2Array(results.iter().map(|fx| fx.offset).collect()),
        ),
        (
            CHARFX_CACHE_COLOR.to_string(),
            Variant::PackedColorArray(results.iter().map(|fx| fx.color).collect()),
        ),
        (
            CHARFX_CACHE_VISIBLE.to_string(),
            Variant::PackedByteArray(results.iter().map(|fx| u8::from(fx.visible)).collect()),
        ),
    ]
}

/// What the pre-pass produced, as the draw path sees it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CharFXCache {
    offsets: Vec<Vector2>,
    colors: Vec<Color>,
    visible: Vec<u8>,
}

impl CharFXCache {
    /// Reads the cache off a node. `get` is the node-property reader.
    ///
    /// A missing or wrongly-typed property yields an EMPTY cache rather than a
    /// partial one, so a label with no custom effects — the overwhelmingly
    /// common case — costs three reads and nothing else.
    pub fn from_properties(get: impl Fn(&str) -> Variant) -> Self {
        let offsets = match get(CHARFX_CACHE_OFFSET) {
            Variant::PackedVector2Array(v) => v,
            _ => Vec::new(),
        };
        let colors = match get(CHARFX_CACHE_COLOR) {
            Variant::PackedColorArray(v) => v,
            _ => Vec::new(),
        };
        let visible = match get(CHARFX_CACHE_VISIBLE) {
            Variant::PackedByteArray(v) => v,
            _ => Vec::new(),
        };
        Self {
            offsets,
            colors,
            visible,
        }
    }

    /// Whether anything was cached at all.
    pub fn is_empty(&self) -> bool {
        self.offsets.is_empty()
    }

    /// One glyph's result, or `None` when this glyph had no custom effect.
    ///
    /// ⚠️ `None` on a short or ragged cache is the load-bearing behaviour, not
    /// a convenience. The draw path falls back to drawing the glyph normally,
    /// so a cache that is missing, stale or shorter than the text leaves the
    /// text VISIBLE and unmoved. Returning a default `(ZERO, white, true)`
    /// instead would silently repaint every glyph white, and defaulting
    /// `visible` the other way would make a stale cache erase the label.
    pub fn get(&self, glyph: usize) -> Option<(Vector2, Color, bool)> {
        let offset = *self.offsets.get(glyph)?;
        let color = *self.colors.get(glyph)?;
        let visible = *self.visible.get(glyph)? != 0;
        Some((offset, color, visible))
    }
}

/// Godot's built-in BBCode effects. Each is a pure function of the glyph's
/// `relative_index` and the label's `elapsed_time`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BuiltinEffect {
    /// `[wave amp=50 freq=5]` — vertical sine, phase-shifted along the run.
    Wave { amplitude: f32, frequency: f32 },
    /// `[tornado radius=10 freq=1]` — circular orbit per glyph.
    Tornado { radius: f32, frequency: f32 },
    /// `[shake rate=20 level=5]` — pseudo-random jitter, re-rolled `rate` times
    /// a second. Deterministic: derived from `(relative_index, time step)`.
    Shake { rate: f32, level: f32 },
    /// `[fade start=4 length=14]` — alpha ramps to 0 across `length` glyphs.
    Fade { start: usize, length: usize },
    /// `[rainbow freq=1 sat=0.8 val=0.8]` — hue cycles along the run and in time.
    Rainbow {
        frequency: f32,
        saturation: f32,
        value: f32,
    },
    /// `[pulse freq=1 ease=-2]` — alpha breathes between 1 and `1 - height`.
    Pulse { frequency: f32, height: f32 },
    /// A script-defined effect from `RichTextLabel.custom_effects`, carrying
    /// the effect object's id (pat-fk17e.2).
    ///
    /// ⚠️ Two things about this variant are deliberate and easy to get wrong.
    ///
    /// **It widens the enum past its name.** `BuiltinEffect` is now "an effect
    /// a span can carry", not only a built-in one. Renaming the type would
    /// touch every call site and all of this module's tests for no behavioural
    /// gain, so the name is left and this note carries the truth instead.
    ///
    /// **[`apply`](Self::apply) does NOTHING for it, by construction.** Every
    /// other variant is a pure function of `(relative_index, elapsed_time)`;
    /// a script-defined effect cannot be, because its result comes from running
    /// GDScript, which needs `&mut SceneTree` and so happens in a pre-pass
    /// before the draw. The draw path reads that pre-pass's cached result
    /// instead of calling `apply`. Anything new that calls `apply` on a
    /// `Custom` and expects a mutation will silently get none — which is why
    /// `apply`'s arm for it says so rather than being an unremarked `{}`.
    Custom { effect: u64 },
}

/// `2*PI` as f32 — the period of the trig-driven effects.
const TAU: f32 = std::f32::consts::TAU;

/// How much phase each successive glyph adds, in radians. Godot uses the pixel
/// x divided by 50; a fixed per-index step is the index-space equivalent and
/// keeps a visible travelling wave for normal text sizes.
const PHASE_PER_GLYPH: f32 = 0.5;

impl BuiltinEffect {
    /// Parses a BBCode effect tag, e.g. `wave` with `{"amp": "50"}`.
    ///
    /// Returns `None` for any tag that is not a built-in effect, so the caller
    /// can fall through to custom effects (or strip the tag).
    pub fn parse(name: &str, params: &HashMap<String, String>) -> Option<Self> {
        let num = |key: &str, default: f32| -> f32 {
            params
                .get(key)
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(default)
        };
        let idx = |key: &str, default: usize| -> usize {
            params
                .get(key)
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(default)
        };
        // Defaults mirror Godot's documented tag defaults.
        match name {
            "wave" => Some(BuiltinEffect::Wave {
                amplitude: num("amp", 50.0) / 10.0,
                frequency: num("freq", 5.0),
            }),
            "tornado" => Some(BuiltinEffect::Tornado {
                radius: num("radius", 10.0) / 10.0,
                frequency: num("freq", 1.0),
            }),
            "shake" => Some(BuiltinEffect::Shake {
                rate: num("rate", 20.0),
                level: num("level", 5.0) / 5.0,
            }),
            "fade" => Some(BuiltinEffect::Fade {
                start: idx("start", 0),
                length: idx("length", 10),
            }),
            "rainbow" => Some(BuiltinEffect::Rainbow {
                frequency: num("freq", 1.0),
                saturation: num("sat", 0.8),
                value: num("val", 0.8),
            }),
            "pulse" => Some(BuiltinEffect::Pulse {
                frequency: num("freq", 1.0),
                height: num("height", 0.0).max(0.3),
            }),
            _ => None,
        }
    }

    /// Applies this effect to one glyph's transform, mutating `offset`/`color`.
    pub fn apply(&self, fx: &mut CharFXTransform) {
        let rel = fx.relative_index as f32;
        let t = fx.elapsed_time;
        match *self {
            BuiltinEffect::Wave {
                amplitude,
                frequency,
            } => {
                fx.offset.y += (t * frequency + rel * PHASE_PER_GLYPH).sin() * amplitude;
            }
            BuiltinEffect::Tornado { radius, frequency } => {
                let phase = t * frequency + rel * PHASE_PER_GLYPH;
                fx.offset.x += phase.cos() * radius;
                fx.offset.y += phase.sin() * radius;
            }
            BuiltinEffect::Shake { rate, level } => {
                // Re-roll `rate` times a second: quantise time into steps, then
                // hash (step, index) so the jitter is stable within a step and
                // reproducible across runs — Godot uses an RNG, but a test
                // cannot assert on that.
                let step = (t * rate).floor() as i64;
                let (dx, dy) = shake_offset(step, fx.relative_index);
                fx.offset.x += dx * level;
                fx.offset.y += dy * level;
            }
            BuiltinEffect::Fade { start, length } => {
                // Glyphs before `start` are untouched; alpha ramps to 0 across
                // the next `length` glyphs and stays there.
                let i = fx.relative_index;
                if i >= start {
                    let past = (i - start) as f32;
                    let alpha = if length == 0 {
                        0.0
                    } else {
                        1.0 - (past / length as f32).clamp(0.0, 1.0)
                    };
                    fx.color.a *= alpha;
                }
            }
            BuiltinEffect::Rainbow {
                frequency,
                saturation,
                value,
            } => {
                // Hue cycles along the run and with time; wraps to [0,1).
                let hue = (t * frequency + rel * 0.1).rem_euclid(1.0);
                let rgb = hsv_to_rgb(hue, saturation.clamp(0.0, 1.0), value.clamp(0.0, 1.0));
                fx.color = Color::new(rgb.0, rgb.1, rgb.2, fx.color.a);
            }
            BuiltinEffect::Pulse { frequency, height } => {
                // Breathe alpha between full and `1 - height`, never inverting.
                let wave = ((t * frequency * TAU).sin() + 1.0) * 0.5; // [0,1]
                fx.color.a *= 1.0 - height * (1.0 - wave);
            }
            // ⚠️ Intentionally no mutation. A script-defined effect's result
            // cannot be computed here: it comes from running GDScript, which
            // needs `&mut SceneTree`, so it is produced by the pre-pass and the
            // draw path reads the cache instead of calling this. Left as an
            // explicit arm rather than a `_ => {}` so that a future variant
            // cannot fall into it by accident, and so this silence is on the
            // record rather than looking like an omission (pat-fk17e.2).
            BuiltinEffect::Custom { effect: _ } => {}
        }
    }
}

/// A stable pseudo-random offset in `[-1, 1]^2` for a `(step, index)` pair.
///
/// A hash rather than an RNG so `[shake]` is reproducible: the same frame and
/// glyph always jitter the same way, which is what lets a test assert it moved
/// *and* assert it held still within a step.
fn shake_offset(step: i64, index: usize) -> (f32, f32) {
    // A small integer avalanche (splitmix-style); quality is irrelevant here,
    // decorrelation between neighbouring glyphs is not.
    let mut h = (step as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (index as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 30;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 31;
    let unit = |v: u32| (v as f32 / u32::MAX as f32) * 2.0 - 1.0;
    (unit(h as u32), unit((h >> 32) as u32))
}

/// HSV -> RGB, all channels in `[0, 1]`. Used by `[rainbow]`.
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    if s <= 0.0 {
        return (v, v, v);
    }
    let h6 = (h.rem_euclid(1.0)) * 6.0;
    let sector = h6.floor();
    let f = h6 - sector;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    match sector as i32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fx_at(index: usize, len: usize, t: f32) -> CharFXTransform {
        CharFXTransform::new(index, (0, len), t, Color::WHITE)
    }

    #[test]
    fn char_fx_transform_derives_relative_index_from_the_range() {
        let fx = CharFXTransform::new(7, (5, 11), 0.0, Color::WHITE);
        assert_eq!(fx.relative_index, 2, "relative to the range start, not 0");
        assert_eq!(fx.absolute_index, 7);
        assert_eq!(fx.glyph_count, 6);
        assert!(fx.visible);
        assert_eq!(fx.offset, Vector2::ZERO, "an untouched glyph has no offset");
    }

    #[test]
    fn wave_offsets_vertically_and_changes_over_time() {
        let e = BuiltinEffect::Wave {
            amplitude: 5.0,
            frequency: 5.0,
        };
        // At t=0 the first glyph sits at sin(0) == 0 — the wave's own zero.
        let mut a = fx_at(0, 4, 0.0);
        e.apply(&mut a);
        assert!(a.offset.y.abs() < 1e-6, "phase 0 -> no displacement");

        // The same glyph, a moment later, has moved: this is the "per frame" bit.
        let mut b = fx_at(0, 4, 0.1);
        e.apply(&mut b);
        assert!(
            (b.offset.y - a.offset.y).abs() > 1e-3,
            "wave must animate: y(t=0)={} y(t=0.1)={}",
            a.offset.y,
            b.offset.y
        );
        // Vertical only.
        assert_eq!(b.offset.x, 0.0, "wave displaces y, never x");
    }

    #[test]
    fn wave_phase_shifts_along_the_run() {
        let e = BuiltinEffect::Wave {
            amplitude: 5.0,
            frequency: 5.0,
        };
        // Neighbouring glyphs differ at a fixed instant — that is what makes it
        // a travelling wave rather than the whole run bobbing together.
        let mut first = fx_at(0, 4, 0.0);
        let mut second = fx_at(1, 4, 0.0);
        e.apply(&mut first);
        e.apply(&mut second);
        assert!(
            (first.offset.y - second.offset.y).abs() > 1e-3,
            "adjacent glyphs must be out of phase"
        );
    }

    #[test]
    fn wave_amplitude_bounds_the_offset() {
        let e = BuiltinEffect::Wave {
            amplitude: 3.0,
            frequency: 5.0,
        };
        for i in 0..32 {
            let mut fx = fx_at(i, 32, i as f32 * 0.07);
            e.apply(&mut fx);
            assert!(
                fx.offset.y.abs() <= 3.0 + 1e-5,
                "|offset.y| must never exceed the amplitude, got {}",
                fx.offset.y
            );
        }
    }

    #[test]
    fn tornado_orbits_both_axes_within_its_radius() {
        let e = BuiltinEffect::Tornado {
            radius: 2.0,
            frequency: 1.0,
        };
        let mut fx = fx_at(0, 4, 0.0);
        e.apply(&mut fx);
        // cos(0)=1, sin(0)=0 at the first glyph, t=0.
        assert!((fx.offset.x - 2.0).abs() < 1e-5);
        assert!(fx.offset.y.abs() < 1e-5);
        // The orbit stays on its circle.
        for i in 0..16 {
            let mut f = fx_at(i, 16, i as f32 * 0.1);
            e.apply(&mut f);
            let r = (f.offset.x * f.offset.x + f.offset.y * f.offset.y).sqrt();
            assert!((r - 2.0).abs() < 1e-4, "radius drifted to {r}");
        }
    }

    #[test]
    fn shake_is_stable_within_a_step_and_rerolls_across_steps() {
        let e = BuiltinEffect::Shake {
            rate: 10.0,
            level: 1.0,
        };
        // Same 1/10s window -> identical jitter (no per-call RNG churn).
        let mut a = fx_at(3, 8, 0.00);
        let mut b = fx_at(3, 8, 0.09);
        e.apply(&mut a);
        e.apply(&mut b);
        assert_eq!(a.offset, b.offset, "jitter must hold still within a step");

        // Next window -> different jitter.
        let mut c = fx_at(3, 8, 0.11);
        e.apply(&mut c);
        assert!(c.offset != a.offset, "jitter must re-roll on the next step");

        // Neighbouring glyphs jitter independently.
        let mut d = fx_at(4, 8, 0.00);
        e.apply(&mut d);
        assert!(d.offset != a.offset, "adjacent glyphs must decorrelate");
    }

    #[test]
    fn shake_offset_is_bounded_by_level() {
        let e = BuiltinEffect::Shake {
            rate: 20.0,
            level: 2.0,
        };
        for i in 0..64 {
            let mut fx = fx_at(i, 64, i as f32 * 0.013);
            e.apply(&mut fx);
            assert!(fx.offset.x.abs() <= 2.0 + 1e-5 && fx.offset.y.abs() <= 2.0 + 1e-5);
        }
    }

    #[test]
    fn fade_ramps_alpha_down_across_the_run() {
        let e = BuiltinEffect::Fade {
            start: 2,
            length: 4,
        };
        let alpha = |i: usize| {
            let mut fx = fx_at(i, 10, 0.0);
            e.apply(&mut fx);
            fx.color.a
        };
        assert_eq!(alpha(0), 1.0, "glyphs before `start` are untouched");
        assert_eq!(alpha(1), 1.0);
        assert_eq!(alpha(2), 1.0, "the ramp begins at `start`, still opaque");
        assert!((alpha(4) - 0.5).abs() < 1e-6, "halfway through the ramp");
        assert!(alpha(6).abs() < 1e-6, "fully faded at start+length");
        assert!(alpha(9).abs() < 1e-6, "and stays faded past the ramp");
        // Monotonically decreasing.
        for i in 2..9 {
            assert!(alpha(i) >= alpha(i + 1), "fade must not brighten at {i}");
        }
    }

    #[test]
    fn rainbow_cycles_hue_and_preserves_alpha() {
        let e = BuiltinEffect::Rainbow {
            frequency: 1.0,
            saturation: 1.0,
            value: 1.0,
        };
        let mut fx = fx_at(0, 8, 0.0);
        fx.color = Color::new(1.0, 1.0, 1.0, 0.5);
        e.apply(&mut fx);
        assert_eq!(fx.color.a, 0.5, "rainbow retints, it must not touch alpha");
        // Hue 0 -> red.
        assert!((fx.color.r - 1.0).abs() < 1e-5 && fx.color.g.abs() < 1e-5);

        // A third of a cycle later the colour has moved.
        let mut later = fx_at(0, 8, 0.33);
        e.apply(&mut later);
        assert!(later.color != fx.color, "hue must advance with time");
    }

    #[test]
    fn pulse_breathes_alpha_without_inverting_it() {
        let e = BuiltinEffect::Pulse {
            frequency: 1.0,
            height: 0.5,
        };
        for i in 0..40 {
            let mut fx = fx_at(0, 4, i as f32 * 0.05);
            e.apply(&mut fx);
            assert!(
                (0.5 - 1e-5..=1.0 + 1e-5).contains(&fx.color.a),
                "alpha left [1-height, 1]: {}",
                fx.color.a
            );
        }
    }

    #[test]
    fn parse_reads_godot_tag_names_and_params() {
        let p = |pairs: &[(&str, &str)]| -> HashMap<String, String> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        assert_eq!(
            BuiltinEffect::parse("wave", &p(&[("amp", "20"), ("freq", "3")])),
            Some(BuiltinEffect::Wave {
                amplitude: 2.0,
                frequency: 3.0
            })
        );
        assert_eq!(
            BuiltinEffect::parse("fade", &p(&[("start", "4"), ("length", "14")])),
            Some(BuiltinEffect::Fade {
                start: 4,
                length: 14
            })
        );
        // Unknown params fall back to Godot's defaults rather than failing.
        assert_eq!(
            BuiltinEffect::parse("wave", &p(&[("bogus", "1")])),
            Some(BuiltinEffect::Wave {
                amplitude: 5.0,
                frequency: 5.0
            })
        );
        // A non-built-in tag is not ours — the caller decides what to do.
        assert_eq!(BuiltinEffect::parse("ghost", &p(&[])), None);
        assert_eq!(BuiltinEffect::parse("b", &p(&[])), None);
    }

    // -- script-visible surface (pat-fk17e.2) -------------------------------

    /// Helper: the property bag as a lookup, the way the phantom node holds it.
    fn bag(fx: &CharFXTransform) -> HashMap<String, Variant> {
        fx.to_properties().into_iter().collect()
    }

    /// The seeded names are exactly ClassDB's, including the two registered
    /// properties this struct has no field for.
    #[test]
    fn the_property_bag_covers_every_registered_name() {
        let b = bag(&fx_at(3, 10, 0.5));
        for name in [
            "glyph_flags",
            "range",
            "elapsed_time",
            "visible",
            "outline",
            "offset",
            "color",
            "env",
            "glyph_index",
            "glyph_count",
            "relative_index",
            "absolute_index",
        ] {
            assert!(b.contains_key(name), "{name} must be seeded");
            assert_ne!(
                b[name],
                Variant::Nil,
                "{name} must be seeded with a value, not Nil — \
                 arithmetic on a Nil property is a hard error in GDScript"
            );
        }
        assert_eq!(
            b.len(),
            12,
            "no extra names, so a script sees Godot's surface"
        );
    }

    /// ⚠️ `Variant::Float` is f64 and `elapsed_time` is f32. This pins the cast
    /// rather than the type: a value that does not survive the trip is the bug.
    #[test]
    fn elapsed_time_crosses_as_a_float_without_losing_its_value() {
        let b = bag(&fx_at(0, 4, 0.25));
        assert_eq!(b["elapsed_time"], Variant::Float(0.25));
    }

    #[test]
    fn indices_and_range_cross_with_godots_types() {
        let b = bag(&fx_at(7, 10, 0.0));
        assert_eq!(b["range"], Variant::Vector2i(Vector2i::new(0, 10)));
        assert_eq!(b["relative_index"], Variant::Int(7));
        assert_eq!(b["absolute_index"], Variant::Int(7));
        assert_eq!(b["glyph_count"], Variant::Int(10));
    }

    /// A script writing `char_fx.offset` is read back; this is the whole point
    /// of the round trip.
    #[test]
    fn read_back_collects_what_the_script_wrote() {
        let mut fx = fx_at(0, 4, 0.0);
        let written: HashMap<String, Variant> = [
            (
                "offset".to_string(),
                Variant::Vector2(Vector2::new(3.0, -4.0)),
            ),
            ("visible".to_string(), Variant::Bool(false)),
            (
                "color".to_string(),
                Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0)),
            ),
        ]
        .into_iter()
        .collect();

        fx.read_back(|k| written.get(k).cloned().unwrap_or(Variant::Nil));

        assert_eq!(fx.offset, Vector2::new(3.0, -4.0));
        assert!(!fx.visible, "returning false must hide the glyph");
        assert_eq!(fx.color, Color::new(1.0, 0.0, 0.0, 1.0));
    }

    /// ⚠️ The control that makes the test above mean something: a property the
    /// script never touched must KEEP its seeded value, not snap to a default.
    /// A read-back that zeroed `offset` would silently erase a built-in
    /// effect's contribution to the same glyph, and the assertion above would
    /// still pass.
    #[test]
    fn read_back_leaves_untouched_properties_alone() {
        let mut fx = fx_at(0, 4, 0.0);
        fx.offset = Vector2::new(9.0, 9.0);
        fx.visible = false;
        fx.color = Color::new(0.0, 1.0, 0.0, 1.0);

        // A script that wrote nothing at all.
        fx.read_back(|_| Variant::Nil);

        assert_eq!(fx.offset, Vector2::new(9.0, 9.0), "offset survived");
        assert!(!fx.visible, "visible survived");
        assert_eq!(fx.color, Color::new(0.0, 1.0, 0.0, 1.0), "color survived");
    }

    /// A wrong-typed write is ignored rather than coerced — the seeded value is
    /// more trustworthy than a guess at what the script meant.
    #[test]
    fn read_back_ignores_a_wrongly_typed_write() {
        let mut fx = fx_at(0, 4, 0.0);
        fx.offset = Vector2::new(1.0, 2.0);
        fx.read_back(|k| match k {
            "offset" => Variant::Int(5),
            _ => Variant::Nil,
        });
        assert_eq!(fx.offset, Vector2::new(1.0, 2.0));
    }

    #[test]
    fn env_params_reach_the_script_as_a_dictionary() {
        let mut fx = fx_at(0, 4, 0.0);
        fx.env.insert("speed".to_string(), "2".to_string());
        let b = bag(&fx);
        let Variant::Dictionary(d) = &b["env"] else {
            panic!("env must cross as a Dictionary, got {:?}", b["env"]);
        };
        assert_eq!(
            d.get(Variant::String("speed".to_string())),
            Some(Variant::String("2".to_string()))
        );
    }

    #[test]
    fn a_custom_effect_is_matched_by_its_bbcode_tag() {
        let effects = vec![(11u64, "mytag".to_string()), (22u64, "other".to_string())];
        assert_eq!(match_custom_effect("mytag", &effects), Some(11));
        assert_eq!(match_custom_effect("other", &effects), Some(22));
        assert_eq!(match_custom_effect("absent", &effects), None);
        assert_eq!(match_custom_effect("mytag", &[]), None);
    }

    /// ⚠️ The parser lowercases the tag before this is called, so an effect
    /// declaring `bbcode = "MyTag"` must still be reachable by `[MyTag]`.
    /// An exact compare would degrade it to plain text, indistinguishable from
    /// the tag never having been registered.
    #[test]
    fn tag_matching_survives_the_parsers_lowercasing() {
        let effects = vec![(7u64, "MyTag".to_string())];
        assert_eq!(match_custom_effect("mytag", &effects), Some(7));
    }

    /// Two effects claiming one tag: first wins, as in Godot.
    #[test]
    fn the_first_effect_claiming_a_tag_wins() {
        let effects = vec![(1u64, "dup".to_string()), (2u64, "dup".to_string())];
        assert_eq!(match_custom_effect("dup", &effects), Some(1));
    }

    // -- pre-pass result cache (pat-fk17e.2) --------------------------------

    /// Round trip: what the pre-pass packed is what the draw path reads.
    #[test]
    fn the_cache_round_trips_per_glyph_results() {
        let mut a = fx_at(0, 2, 0.0);
        a.offset = Vector2::new(1.0, 2.0);
        a.color = Color::new(1.0, 0.0, 0.0, 1.0);
        let mut b = fx_at(1, 2, 0.0);
        b.offset = Vector2::new(-3.0, 4.0);
        b.visible = false;

        let packed: HashMap<String, Variant> = cache_to_properties(&[a, b]).into_iter().collect();
        let cache =
            CharFXCache::from_properties(|k| packed.get(k).cloned().unwrap_or(Variant::Nil));

        assert!(!cache.is_empty());
        assert_eq!(
            cache.get(0),
            Some((Vector2::new(1.0, 2.0), Color::new(1.0, 0.0, 0.0, 1.0), true))
        );
        assert_eq!(
            cache.get(1),
            Some((Vector2::new(-3.0, 4.0), Color::WHITE, false)),
            "an effect returning false must survive as hidden"
        );
    }

    /// ⚠️ The most important cache test: a label with no custom effects reads
    /// an EMPTY cache and every `get` says `None`, so the draw path takes its
    /// normal route. If this returned a default instead, every ordinary label
    /// in the engine would get repainted.
    #[test]
    fn a_label_with_no_custom_effects_has_an_empty_cache() {
        let cache = CharFXCache::from_properties(|_| Variant::Nil);
        assert!(cache.is_empty());
        assert_eq!(cache.get(0), None);
        assert_eq!(cache.get(99), None);
    }

    /// A wrongly-typed property is treated as absent, not coerced.
    #[test]
    fn a_wrongly_typed_cache_property_reads_as_absent() {
        let cache = CharFXCache::from_properties(|_| Variant::Int(7));
        assert!(cache.is_empty());
        assert_eq!(cache.get(0), None);
    }

    /// ⚠️ A glyph past the end of the cache yields `None`, so a stale or short
    /// cache leaves that glyph drawn normally — VISIBLE and unmoved. The
    /// alternative (defaulting `visible`) would let a stale cache blank the
    /// label, which is the worst available failure for a text control.
    #[test]
    fn a_glyph_past_the_end_of_the_cache_falls_back_rather_than_vanishing() {
        let one = fx_at(0, 1, 0.0);
        let packed: HashMap<String, Variant> = cache_to_properties(&[one]).into_iter().collect();
        let cache =
            CharFXCache::from_properties(|k| packed.get(k).cloned().unwrap_or(Variant::Nil));

        assert!(cache.get(0).is_some(), "the cached glyph is there");
        assert_eq!(cache.get(1), None, "the uncached one falls back");
    }

    /// A ragged cache — arrays of different lengths — must not panic, and must
    /// fall back for any glyph it cannot fully answer.
    #[test]
    fn a_ragged_cache_falls_back_instead_of_panicking() {
        let packed: HashMap<String, Variant> = [
            (
                CHARFX_CACHE_OFFSET.to_string(),
                Variant::PackedVector2Array(vec![Vector2::ZERO, Vector2::ZERO]),
            ),
            (
                CHARFX_CACHE_COLOR.to_string(),
                Variant::PackedColorArray(vec![Color::WHITE]),
            ),
            (
                CHARFX_CACHE_VISIBLE.to_string(),
                Variant::PackedByteArray(vec![1]),
            ),
        ]
        .into_iter()
        .collect();
        let cache =
            CharFXCache::from_properties(|k| packed.get(k).cloned().unwrap_or(Variant::Nil));

        assert!(cache.get(0).is_some());
        assert_eq!(cache.get(1), None, "offset alone is not an answer");
    }

    /// `apply` is deliberately inert for a script-defined effect; the draw path
    /// reads the cache instead. Pinned so a later refactor that quietly makes
    /// it mutate — or that removes the arm — is caught here.
    #[test]
    fn apply_does_nothing_for_a_script_defined_effect() {
        let mut fx = fx_at(3, 10, 0.75);
        let before = fx.clone();
        BuiltinEffect::Custom { effect: 42 }.apply(&mut fx);
        assert_eq!(
            fx, before,
            "a custom effect's result comes from the pre-pass"
        );
    }
}
