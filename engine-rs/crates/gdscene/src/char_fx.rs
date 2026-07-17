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
//! Script-defined custom effects are NOT dispatched here. The ClassDB surface
//! for `RichTextEffect`/`CharFXTransform` is registered
//! (`gdobject::register_2d_classes`) and this module is shaped to accept that
//! call, but four things are missing before `_process_custom_fx` can run:
//!
//! 1. `draw_control` takes `&SceneTree` — calling a script needs `&mut`.
//! 2. `RichTextLabel` has no `custom_effects` property to hold effect
//!    instances, and nothing maps a BBCode tag to an effect's `bbcode`.
//! 3. `CharFXTransform` has no `Variant` encoding, so it cannot cross
//!    `ScriptInstance::call_method(&[Variant])` and come back mutated.
//! 4. The one script-attachment path that exists (`instantiate_script`) models
//!    a scripted "Resource" as a phantom `Node` parented under the scene root —
//!    `ResourceRef { path, class_name, properties }` has no script slot of its
//!    own — so there is no true Resource to hang an effect on.
//!
//! Any of those alone would block it; together they are a separate bead.

use std::collections::HashMap;

use gdcore::math::{Color, Vector2};

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
    pub fn new(absolute_index: usize, range: (usize, usize), elapsed_time: f32, color: Color) -> Self {
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
    let mut h = (step as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (index as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
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
}
