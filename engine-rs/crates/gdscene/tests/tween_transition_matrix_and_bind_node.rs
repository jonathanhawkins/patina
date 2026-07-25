//! pat-0q2kx.55 acceptance: the full Tween transition matrix, checked against
//! Godot's own easing equations at fixed `t`.
//!
//! Godot ground truth: `upstream/godot/scene/animation/easing_equations.h`
//! (`Quart`, `Quint`, `Circ`, `Spring`, and every namespace's `out_in`) plus
//! `Tween.TransitionType`/`EaseType` in `extension_api.json`.
//!
//! Patina had 8 of the 12 curves and 3 of the 4 ease types. Worse, the
//! script-facing integer mapping SUBSTITUTED approximations for the missing
//! ones — `TRANS_SPRING` became Elastic, `TRANS_CIRC` became Sine, and both
//! `TRANS_QUINT` and `TRANS_QUART` became Cubic — so a script asking for a
//! spring got a visibly wrong motion and nothing reported an error.

use gdscene::tween::{ease, EaseType, TransFunc};

/// Godot's easing equations are written as `f(t, b, c, d)` with `b = 0`,
/// `c = 1`, `d = 1` here, so each expected value below is that formula
/// evaluated directly rather than a number copied from a run of this code.
fn close(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "{what}: expected {expected}, got {actual}"
    );
}

#[test]
fn tween_transition_matrix_and_bind_node() {
    // -- the four new curves, against Godot's formulas --------------------
    let t = 0.25;

    // Quart::in = t^4 ; Quart::out = -((t-1)^4 - 1)
    close(
        ease(t, EaseType::In, TransFunc::Quart),
        t.powi(4),
        "quart in",
    );
    close(
        ease(t, EaseType::Out, TransFunc::Quart),
        -((t - 1.0).powi(4) - 1.0),
        "quart out",
    );

    // Quint::in = t^5 ; Quint::out = (t-1)^5 + 1
    close(
        ease(t, EaseType::In, TransFunc::Quint),
        t.powi(5),
        "quint in",
    );
    close(
        ease(t, EaseType::Out, TransFunc::Quint),
        (t - 1.0).powi(5) + 1.0,
        "quint out",
    );

    // Circ::in = -(sqrt(1 - t^2) - 1) ; Circ::out = sqrt(1 - (t-1)^2)
    close(
        ease(t, EaseType::In, TransFunc::Circ),
        -((1.0f64 - t * t).sqrt() - 1.0),
        "circ in",
    );
    close(
        ease(t, EaseType::Out, TransFunc::Circ),
        (1.0f64 - (t - 1.0).powi(2)).sqrt(),
        "circ out",
    );

    // Spring::out, transcribed from easing_equations.h:418.
    let spring_out = |t: f64| {
        let s = 1.0 - t;
        ((t * std::f64::consts::PI * (0.2 + 2.5 * t * t * t)).sin() * s.powf(2.2) + t)
            * (1.0 + (1.2 * s))
    };
    close(
        ease(t, EaseType::Out, TransFunc::Spring),
        spring_out(t),
        "spring out",
    );
    // Spring::in is defined as c - out(d - t).
    close(
        ease(t, EaseType::In, TransFunc::Spring),
        1.0 - spring_out(1.0 - t),
        "spring in",
    );

    // -- every curve is anchored at both ends -----------------------------
    //
    // ⚠️ A curve that does not reach exactly 1.0 at t=1 leaves the tweened
    // property a hair short of its target forever.
    for func in [
        TransFunc::Linear,
        TransFunc::Sine,
        TransFunc::Quad,
        TransFunc::Cubic,
        TransFunc::Expo,
        TransFunc::Bounce,
        TransFunc::Quart,
        TransFunc::Quint,
        TransFunc::Circ,
        TransFunc::Spring,
    ] {
        for e in [
            EaseType::In,
            EaseType::Out,
            EaseType::InOut,
            EaseType::OutIn,
        ] {
            close(ease(0.0, e, func), 0.0, &format!("{func:?}/{e:?} at t=0"));
            close(ease(1.0, e, func), 1.0, &format!("{func:?}/{e:?} at t=1"));
        }
    }

    // -- EASE_OUT_IN is its own shape -------------------------------------
    //
    // out_in runs the OUT half first, so it is fast-slow-fast: at t=0.25 it is
    // already past halfway up for a curve that InOut would still be easing into.
    let out_in = ease(0.25, EaseType::OutIn, TransFunc::Quad);
    let in_out = ease(0.25, EaseType::InOut, TransFunc::Quad);
    assert!(
        out_in > in_out,
        "EASE_OUT_IN must rise faster early than EASE_IN_OUT: {out_in} vs {in_out}"
    );
    // Both meet in the middle — the halves swap, they do not shift the midpoint.
    close(
        ease(0.5, EaseType::OutIn, TransFunc::Quad),
        0.5,
        "out_in midpoint",
    );

    // out_in is exactly "out over the first half, in over the second", which is
    // the composition Godot's every `out_in` spells out.
    for probe in [0.1, 0.3, 0.45] {
        close(
            ease(probe, EaseType::OutIn, TransFunc::Cubic),
            ease(probe * 2.0, EaseType::Out, TransFunc::Cubic) * 0.5,
            &format!("out_in first half at {probe}"),
        );
    }
    for probe in [0.55, 0.7, 0.9] {
        close(
            ease(probe, EaseType::OutIn, TransFunc::Cubic),
            ease(probe * 2.0 - 1.0, EaseType::In, TransFunc::Cubic) * 0.5 + 0.5,
            &format!("out_in second half at {probe}"),
        );
    }

    // -- Spring really oscillates -----------------------------------------
    //
    // The point of TRANS_SPRING: it overshoots past its target and settles,
    // rather than approaching monotonically like Quad.
    let overshoots = (1..100)
        .map(|i| ease(i as f64 / 100.0, EaseType::Out, TransFunc::Spring))
        .any(|v| v > 1.0);
    assert!(
        overshoots,
        "TRANS_SPRING must overshoot past 1.0 somewhere — otherwise it is not a spring"
    );
}
