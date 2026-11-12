//! pat-nzgdu.186: `Tween.tween_subtween` — a nested tween runs to completion as
//! one step before the parent's next chained step begins, and the nested
//! property animates to its end value as the parent is advanced.

use gdscene::tween::{Tween, TweenStep};
use gdvariant::Variant;

fn find(values: &[(String, Variant)], key: &str) -> Option<Variant> {
    values
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
}

#[test]
fn subtween_tweener_nested_test() {
    // Nested tween: animate "scale" 0 -> 10 over 1.0s.
    let mut nested = Tween::new();
    nested.steps.push(TweenStep::new(
        "scale",
        Variant::Float(0.0),
        Variant::Float(10.0),
        1.0,
    ));

    // Parent: a subtween step (1.0s), then a chained "phase" step (0.5s).
    let mut outer = Tween::new();
    outer.steps.push(TweenStep::subtween(nested)); // duration = nested total (1.0s)
    outer.steps.push(TweenStep::new(
        "phase",
        Variant::Float(0.0),
        Variant::Float(1.0),
        0.5,
    ));
    outer.start();

    // Total duration is subtween (1.0) + phase (0.5) = 1.5s.
    assert!((outer.total_duration() - 1.5).abs() < 1e-9);

    // Advance halfway through the nested tween. The nested "scale" is mid-flight,
    // and the parent's "phase" step must NOT have started yet.
    let done = outer.advance(0.5);
    assert!(!done, "outer tween is not finished mid-subtween");
    let mid = outer.get_current_values();
    let scale_mid = find(&mid, "scale");
    assert!(
        matches!(scale_mid, Some(Variant::Float(f)) if f > 0.0 && f < 10.0),
        "nested 'scale' should be mid-animation, got {scale_mid:?}"
    );
    assert_eq!(
        find(&mid, "phase"),
        Some(Variant::Float(0.0)),
        "the chained 'phase' step must not advance until the subtween completes"
    );

    // Advance past the nested tween's end (0.5 + 0.6 = 1.1s > 1.0s): the nested
    // property reaches its end value, and the chained step has now begun.
    outer.advance(0.6);
    let after = outer.get_current_values();
    assert_eq!(
        find(&after, "scale"),
        Some(Variant::Float(10.0)),
        "nested 'scale' should reach its end value once the subtween completes"
    );
    match find(&after, "phase") {
        Some(Variant::Float(p)) => {
            assert!(
                p > 0.0,
                "the chained 'phase' step should have started, got {p}"
            );
        }
        other => panic!("expected a Float 'phase' value, got {other:?}"),
    }
}
