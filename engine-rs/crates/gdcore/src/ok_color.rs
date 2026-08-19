//! Oklab / OKHSL colour space (pat-s3rrv.58).
//!
//! A direct port of the two entry points Godot uses from
//! `upstream/godot/thirdparty/misc/ok_color.h` — `okhsl_to_srgb` and
//! `srgb_to_okhsl` — plus the chain they depend on. Godot wraps them in
//! `Color::set_ok_hsl` / `Color::get_ok_hsl_*` (`core/math/color.cpp:236,498`).
//!
//! # Why this is a transcription, not an implementation
//!
//! The polynomials in [`compute_max_saturation`] and [`get_st_mid`] were
//! produced by an offline optimisation process — their coefficients carry no
//! derivable meaning, so there is nothing to re-derive and nothing to
//! sanity-check by inspection. A single transposed digit yields colours that
//! are subtly wrong but still *look* like colours, and a round-trip test
//! (`okhsl -> srgb -> okhsl`) passes against a consistently-wrong pair.
//!
//! So the coefficients here were checked **mechanically** against the C++
//! rather than by eye: every float literal in this file was extracted in order
//! and diffed against the literals in the corresponding functions of
//! `ok_color.h`. The tests then assert values produced by an independent
//! transcription of the same C++ into Python, not values produced by this code.
//!
//! ⚠️ The RGB in this module is **sRGB-encoded** (the same 0..1 values a
//! `Color` carries), not linear. `okhsl_to_srgb` applies the transfer function
//! on the way out and `srgb_to_okhsl` applies its inverse on the way in, which
//! is why Godot hands `Color`'s `r`/`g`/`b` straight to them.

use std::f32::consts::PI;

/// An Oklab colour: perceptual lightness plus two opponent axes.
#[derive(Clone, Copy, Debug)]
struct Lab {
    l: f32,
    a: f32,
    b: f32,
}

/// A cusp of the sRGB gamut for one hue: the most chromatic point.
#[derive(Clone, Copy, Debug)]
struct Lc {
    l: f32,
    c: f32,
}

/// Saturation and "toe" values for a cusp.
#[derive(Clone, Copy, Debug)]
struct St {
    s: f32,
    t: f32,
}

/// The three chroma anchors OKHSL interpolates between.
#[derive(Clone, Copy, Debug)]
struct Cs {
    c_0: f32,
    c_mid: f32,
    c_max: f32,
}

/// sRGB transfer function (linear → encoded).
fn srgb_transfer_function(a: f32) -> f32 {
    if 0.0031308 >= a {
        12.92 * a
    } else {
        1.055 * a.powf(0.4166666666666667) - 0.055
    }
}

/// sRGB transfer function inverse (encoded → linear).
fn srgb_transfer_function_inv(a: f32) -> f32 {
    if 0.04045 < a {
        ((a + 0.055) / 1.055).powf(2.4)
    } else {
        a / 12.92
    }
}

fn linear_srgb_to_oklab(r: f32, g: f32, b: f32) -> Lab {
    let l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
    let m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
    let s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;

    let l_ = l.cbrt();
    let m_ = m.cbrt();
    let s_ = s.cbrt();

    Lab {
        l: 0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
        a: 1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
        b: 0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_,
    }
}

fn oklab_to_linear_srgb(lab: Lab) -> (f32, f32, f32) {
    let l_ = lab.l + 0.3963377774 * lab.a + 0.2158037573 * lab.b;
    let m_ = lab.l - 0.1055613458 * lab.a - 0.0638541728 * lab.b;
    let s_ = lab.l - 0.0894841775 * lab.a - 1.2914855480 * lab.b;

    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;

    (
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    )
}

/// The maximum saturation (S = C/L) for a hue that still fits in sRGB.
///
/// `a` and `b` must be normalised so `a² + b² == 1`. The branch picks which
/// channel goes negative first; the coefficients are an optimisation artefact
/// with no closed form.
fn compute_max_saturation(a: f32, b: f32) -> f32 {
    let (k0, k1, k2, k3, k4, wl, wm, ws) = if -1.88170328 * a - 0.80936493 * b > 1.0 {
        // Red component
        (
            1.19086277,
            1.76576728,
            0.59662641,
            0.75515197,
            0.56771245,
            4.0767416621,
            -3.3077115913,
            0.2309699292,
        )
    } else if 1.81444104 * a - 1.19445276 * b > 1.0 {
        // Green component
        (
            0.73956515,
            -0.45954404,
            0.08285427,
            0.12541070,
            0.14503204,
            -1.2684380046,
            2.6097574011,
            -0.3413193965,
        )
    } else {
        // Blue component
        (
            1.35733652,
            -0.00915799,
            -1.15130210,
            -0.50559606,
            0.00692167,
            -0.0041960863,
            -0.7034186147,
            1.7076147010,
        )
    };

    // Polynomial approximation, then one Halley step to refine it.
    let mut s = k0 + k1 * a + k2 * b + k3 * a * a + k4 * a * b;

    let k_l = 0.3963377774 * a + 0.2158037573 * b;
    let k_m = -0.1055613458 * a - 0.0638541728 * b;
    let k_s = -0.0894841775 * a - 1.2914855480 * b;

    {
        let l_ = 1.0 + s * k_l;
        let m_ = 1.0 + s * k_m;
        let s_ = 1.0 + s * k_s;

        let l = l_ * l_ * l_;
        let m = m_ * m_ * m_;
        let s3 = s_ * s_ * s_;

        let l_ds = 3.0 * k_l * l_ * l_;
        let m_ds = 3.0 * k_m * m_ * m_;
        let s_ds = 3.0 * k_s * s_ * s_;

        let l_ds2 = 6.0 * k_l * k_l * l_;
        let m_ds2 = 6.0 * k_m * k_m * m_;
        let s_ds2 = 6.0 * k_s * k_s * s_;

        let f = wl * l + wm * m + ws * s3;
        let f1 = wl * l_ds + wm * m_ds + ws * s_ds;
        let f2 = wl * l_ds2 + wm * m_ds2 + ws * s_ds2;

        s -= f * f1 / (f1 * f1 - 0.5 * f * f2);
    }

    s
}

/// `L_cusp` and `C_cusp` for a hue. `a`/`b` normalised.
fn find_cusp(a: f32, b: f32) -> Lc {
    let s_cusp = compute_max_saturation(a, b);

    let (r, g, bl) = oklab_to_linear_srgb(Lab {
        l: 1.0,
        a: s_cusp * a,
        b: s_cusp * b,
    });
    let l_cusp = (1.0 / r.max(g).max(bl)).cbrt();
    let c_cusp = l_cusp * s_cusp;

    Lc {
        l: l_cusp,
        c: c_cusp,
    }
}

/// Intersection of `L = L0*(1-t) + t*L1`, `C = t*C1` with the sRGB gamut.
///
/// ⚠️ The upstream C++ REUSES its `b` parameter as scratch for the blue
/// residual partway through (`b = -0.0041960863f * l - …`). That is safe there
/// only because the hue's `b` is never read again afterwards. This port keeps
/// them separate (`b_resid`) rather than reproducing the shadowing — the
/// arithmetic is identical, and reusing the name in Rust would invite a later
/// edit to read `b` expecting the hue.
fn find_gamut_intersection(a: f32, b: f32, l1: f32, c1: f32, l0: f32, cusp: Lc) -> f32 {
    if ((l1 - l0) * cusp.c - (cusp.l - l0) * c1) <= 0.0 {
        // Lower half
        cusp.c * l0 / (c1 * cusp.l + cusp.c * (l0 - l1))
    } else {
        // Upper half: intersect the triangle, then one Halley step.
        let mut t = cusp.c * (l0 - 1.0) / (c1 * (cusp.l - 1.0) + cusp.c * (l0 - l1));

        let dl = l1 - l0;
        let dc = c1;

        let k_l = 0.3963377774 * a + 0.2158037573 * b;
        let k_m = -0.1055613458 * a - 0.0638541728 * b;
        let k_s = -0.0894841775 * a - 1.2914855480 * b;

        let l_dt = dl + dc * k_l;
        let m_dt = dl + dc * k_m;
        let s_dt = dl + dc * k_s;

        {
            let big_l = l0 * (1.0 - t) + t * l1;
            let big_c = t * c1;

            let l_ = big_l + big_c * k_l;
            let m_ = big_l + big_c * k_m;
            let s_ = big_l + big_c * k_s;

            let l = l_ * l_ * l_;
            let m = m_ * m_ * m_;
            let s = s_ * s_ * s_;

            let ldt = 3.0 * l_dt * l_ * l_;
            let mdt = 3.0 * m_dt * m_ * m_;
            let sdt = 3.0 * s_dt * s_ * s_;

            let ldt2 = 6.0 * l_dt * l_dt * l_;
            let mdt2 = 6.0 * m_dt * m_dt * m_;
            let sdt2 = 6.0 * s_dt * s_dt * s_;

            let r = 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s - 1.0;
            let r1 = 4.0767416621 * ldt - 3.3077115913 * mdt + 0.2309699292 * sdt;
            let r2 = 4.0767416621 * ldt2 - 3.3077115913 * mdt2 + 0.2309699292 * sdt2;

            let u_r = r1 / (r1 * r1 - 0.5 * r * r2);
            let mut t_r = -r * u_r;

            let g = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s - 1.0;
            let g1 = -1.2684380046 * ldt + 2.6097574011 * mdt - 0.3413193965 * sdt;
            let g2 = -1.2684380046 * ldt2 + 2.6097574011 * mdt2 - 0.3413193965 * sdt2;

            let u_g = g1 / (g1 * g1 - 0.5 * g * g2);
            let mut t_g = -g * u_g;

            let b_resid = -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s - 1.0;
            let b1 = -0.0041960863 * ldt - 0.7034186147 * mdt + 1.7076147010 * sdt;
            let b2 = -0.0041960863 * ldt2 - 0.7034186147 * mdt2 + 1.7076147010 * sdt2;

            let u_b = b1 / (b1 * b1 - 0.5 * b_resid * b2);
            let mut t_b = -b_resid * u_b;

            t_r = if u_r >= 0.0 { t_r } else { f32::MAX };
            t_g = if u_g >= 0.0 { t_g } else { f32::MAX };
            t_b = if u_b >= 0.0 { t_b } else { f32::MAX };

            t += t_r.min(t_g.min(t_b));
        }

        t
    }
}

const TOE_K1: f32 = 0.206;
const TOE_K2: f32 = 0.03;
const TOE_K3: f32 = (1.0 + TOE_K1) / (1.0 + TOE_K2);

/// Oklab lightness → OKHSL lightness.
fn toe(x: f32) -> f32 {
    0.5 * (TOE_K3 * x - TOE_K1
        + ((TOE_K3 * x - TOE_K1) * (TOE_K3 * x - TOE_K1) + 4.0 * TOE_K2 * TOE_K3 * x).sqrt())
}

/// OKHSL lightness → Oklab lightness.
fn toe_inv(x: f32) -> f32 {
    (x * x + TOE_K1 * x) / (TOE_K3 * (x + TOE_K2))
}

fn to_st(cusp: Lc) -> St {
    St {
        s: cusp.c / cusp.l,
        t: cusp.c / (1.0 - cusp.l),
    }
}

/// A smooth approximation of the cusp location.
///
/// Designed so `S_mid < S_max` and `T_mid < T_max`. Another optimisation
/// artefact — the nesting is Horner form, and the parenthesisation matters.
fn get_st_mid(a_: f32, b_: f32) -> St {
    let s = 0.11516993
        + 1.0
            / (7.44778970
                + 4.15901240 * b_
                + a_ * (-2.19557347
                    + 1.75198401 * b_
                    + a_ * (-2.13704948 - 10.02301043 * b_
                        + a_ * (-4.24894561 + 5.38770819 * b_ + 4.69891013 * a_))));

    let t = 0.11239642
        + 1.0
            / (1.61320320 - 0.68124379 * b_
                + a_ * (0.40370612
                    + 0.90148123 * b_
                    + a_ * (-0.27087943
                        + 0.61223990 * b_
                        + a_ * (0.00299215 - 0.45399568 * b_ - 0.14661872 * a_))));

    St { s, t }
}

fn get_cs(l: f32, a_: f32, b_: f32) -> Cs {
    let cusp = find_cusp(a_, b_);

    let c_max = find_gamut_intersection(a_, b_, l, 1.0, l, cusp);
    let st_max = to_st(cusp);

    // Compensates for the curved part of the gamut shape.
    let k = c_max / ((l * st_max.s).min((1.0 - l) * st_max.t));

    let c_mid = {
        let st_mid = get_st_mid(a_, b_);

        // A soft minimum rather than a sharp triangle, so chroma stays smooth.
        let c_a = l * st_mid.s;
        let c_b = (1.0 - l) * st_mid.t;
        0.9 * k
            * (1.0 / (1.0 / (c_a * c_a * c_a * c_a) + 1.0 / (c_b * c_b * c_b * c_b)))
                .sqrt()
                .sqrt()
    };

    let c_0 = {
        // Hue-independent, so S and T are constants picked as rough averages.
        let c_a = l * 0.4;
        let c_b = (1.0 - l) * 0.8;
        (1.0 / (1.0 / (c_a * c_a) + 1.0 / (c_b * c_b))).sqrt()
    };

    Cs { c_0, c_mid, c_max }
}

const MID: f32 = 0.8;
const MID_INV: f32 = 1.25;

/// OKHSL → sRGB-encoded RGB, each 0..1 (Godot `ok_color::okhsl_to_srgb`).
pub fn okhsl_to_srgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    if l == 1.0 {
        return (1.0, 1.0, 1.0);
    } else if l == 0.0 {
        return (0.0, 0.0, 0.0);
    }

    let a_ = (2.0 * PI * h).cos();
    let b_ = (2.0 * PI * h).sin();
    let big_l = toe_inv(l);

    let cs = get_cs(big_l, a_, b_);

    let c = if s < MID {
        let t = MID_INV * s;
        let k_1 = MID * cs.c_0;
        let k_2 = 1.0 - k_1 / cs.c_mid;
        t * k_1 / (1.0 - k_2 * t)
    } else {
        let t = (s - MID) / (1.0 - MID);
        let k_0 = cs.c_mid;
        let k_1 = (1.0 - MID) * cs.c_mid * cs.c_mid * MID_INV * MID_INV / cs.c_0;
        let k_2 = 1.0 - k_1 / (cs.c_max - cs.c_mid);
        k_0 + t * k_1 / (1.0 - k_2 * t)
    };

    let (r, g, b) = oklab_to_linear_srgb(Lab {
        l: big_l,
        a: c * a_,
        b: c * b_,
    });
    (
        srgb_transfer_function(r),
        srgb_transfer_function(g),
        srgb_transfer_function(b),
    )
}

/// sRGB-encoded RGB → OKHSL (Godot `ok_color::srgb_to_okhsl`).
///
/// ⚠️ Returns NaN components for a pure grey that is not black: the hue axis
/// is `lab.a / C` with `C == 0`. Upstream keeps that, and `Color::get_ok_hsl_*`
/// is what maps NaN to 0 (`color.cpp:504`). Callers must do the same rather
/// than assume a number comes back.
pub fn srgb_to_okhsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    // Fixes some precision errors.
    if r == 0.0 && g == 0.0 && b == 0.0 {
        return (0.0, 0.0, 0.0);
    }

    let lab = linear_srgb_to_oklab(
        srgb_transfer_function_inv(r),
        srgb_transfer_function_inv(g),
        srgb_transfer_function_inv(b),
    );

    let c = (lab.a * lab.a + lab.b * lab.b).sqrt();
    let a_ = lab.a / c;
    let b_ = lab.b / c;

    let big_l = lab.l;
    let h = 0.5 + 0.5 * (-lab.b).atan2(-lab.a) / PI;

    let cs = get_cs(big_l, a_, b_);

    // The inverse of the interpolation in `okhsl_to_srgb`.
    let s = if c < cs.c_mid {
        let k_1 = MID * cs.c_0;
        let k_2 = 1.0 - k_1 / cs.c_mid;
        let t = c / (k_1 + k_2 * c);
        t * MID
    } else {
        let k_0 = cs.c_mid;
        let k_1 = (1.0 - MID) * cs.c_mid * cs.c_mid * MID_INV * MID_INV / cs.c_0;
        let k_2 = 1.0 - k_1 / (cs.c_max - cs.c_mid);
        let t = (c - k_0) / (k_1 + k_2 * (c - k_0));
        MID + (1.0 - MID) * t
    };

    (h, s, toe(big_l))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every expected value below comes from an INDEPENDENT transcription of
    /// `ok_color.h` into Python, run to produce these vectors — not from this
    /// code. A round-trip test alone would pass against a consistently-wrong
    /// pair of functions, which is the failure mode this module is most
    /// exposed to.
    fn close(got: f32, want: f32, what: &str) {
        assert!(
            (got - want).abs() < 2e-4,
            "{what}: got {got}, want {want} (delta {})",
            (got - want).abs()
        );
    }

    /// 🔑 The check that does not depend on my transcription at all.
    ///
    /// The Oklab hues of the sRGB primaries are published values — red 29.23°,
    /// green 142.50°, blue 264.05°. Hitting all three pins the whole chain:
    /// the sRGB transfer function, both Oklab matrices, and the `atan2(-b, -a)`
    /// hue convention (which is off by half a turn if the negations are
    /// dropped). A wrong matrix coefficient moves these by degrees.
    #[test]
    fn okhsl_hues_of_the_srgb_primaries_match_published_oklch_values() {
        for (name, (r, g, b), degrees) in [
            ("red", (1.0, 0.0, 0.0), 29.23),
            ("green", (0.0, 1.0, 0.0), 142.50),
            ("blue", (0.0, 0.0, 1.0), 264.05),
        ] {
            let (h, _, _) = srgb_to_okhsl(r, g, b);
            let got = h * 360.0;
            assert!(
                (got - degrees).abs() < 0.01,
                "{name}: Oklab hue {got}° must match the published {degrees}°"
            );
        }
    }

    #[test]
    fn okhsl_from_srgb_matches_the_reference_transcription() {
        for ((r, g, b), (h, s, l)) in [
            ((1.0, 0.0, 0.0), (0.081205, 1.000000, 0.568085)),
            ((0.0, 1.0, 0.0), (0.395820, 1.000000, 0.844529)),
            ((0.0, 0.0, 1.0), (0.733478, 1.000000, 0.366565)),
            ((1.0, 1.0, 0.0), (0.304915, 1.000000, 0.962704)),
            ((0.25, 0.5, 0.75), (0.695923, 0.668595, 0.519219)),
        ] {
            let (gh, gs, gl) = srgb_to_okhsl(r, g, b);
            close(gh, h, "hue");
            close(gs, s, "saturation");
            close(gl, l, "lightness");
        }
    }

    /// Covers BOTH interpolation branches: `s < 0.8` takes the `C_0`/`C_mid`
    /// leg, `s >= 0.8` the `C_mid`/`C_max` leg. A port that mixed the two up
    /// still produces plausible colours on one side.
    #[test]
    fn okhsl_to_srgb_matches_the_reference_transcription() {
        for ((h, s, l), (r, g, b)) in [
            ((0.0, 0.0, 0.5), (0.466181, 0.466181, 0.466181)),
            ((0.25, 0.5, 0.5), (0.532501, 0.459829, 0.260123)),
            ((0.5, 1.0, 0.5), (0.000000, 0.542896, 0.475653)),
            ((0.75, 0.9, 0.25), (0.117783, 0.164009, 0.542353)),
            // The two sides of the `mid` split at one hue and lightness.
            ((0.3, 0.3, 0.6), (0.578519, 0.579234, 0.432820)),
            ((0.3, 0.8, 0.6), (0.598166, 0.588113, 0.173808)),
            ((0.3, 0.95, 0.6), (0.602498, 0.588992, 0.056301)),
        ] {
            let (gr, gg, gb) = okhsl_to_srgb(h, s, l);
            close(gr, r, "red");
            close(gg, g, "green");
            close(gb, b, "blue");
        }
    }

    /// The pair really are inverses: red's own OKHSL rebuilds red.
    ///
    /// ⚠️ This is a WEAK check on its own — it passes for any consistently
    /// wrong pair — so it sits alongside the reference vectors above rather
    /// than standing in for them.
    #[test]
    fn okhsl_round_trips_through_srgb() {
        let (h, s, l) = srgb_to_okhsl(1.0, 0.0, 0.0);
        let (r, g, b) = okhsl_to_srgb(h, s, l);
        close(r, 1.0, "red round-trip r");
        close(g, 0.0, "red round-trip g");
        close(b, 0.0, "red round-trip b");
    }

    /// The two short-circuits upstream added for precision.
    #[test]
    fn okhsl_lightness_extremes_and_black_short_circuit() {
        assert_eq!(okhsl_to_srgb(0.3, 0.5, 1.0), (1.0, 1.0, 1.0));
        assert_eq!(okhsl_to_srgb(0.3, 0.5, 0.0), (0.0, 0.0, 0.0));
        assert_eq!(srgb_to_okhsl(0.0, 0.0, 0.0), (0.0, 0.0, 0.0));
    }

    /// A neutral grey has (almost) no chroma, so its saturation collapses —
    /// but nothing here may produce a NaN that a caller would paint with.
    #[test]
    fn okhsl_grey_has_no_saturation_and_stays_finite() {
        let (h, s, l) = srgb_to_okhsl(0.5, 0.5, 0.5);
        assert!(s < 1e-3, "grey must be unsaturated, got {s}");
        assert!(h.is_finite() && l.is_finite(), "h={h} l={l}");
        assert!((l - 0.53376).abs() < 2e-4, "grey lightness {l}");
    }
}
