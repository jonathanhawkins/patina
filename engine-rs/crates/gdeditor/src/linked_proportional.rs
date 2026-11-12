//! Proportional ("ratio lock") editing for multi-component Vector/Rect
//! properties in the inspector.
//!
//! When a property's proportional lock is enabled (tracked by
//! [`crate::inspector::LinkedValues`]), editing one component scales the
//! remaining components so the original ratios between them are preserved —
//! mirroring Godot's linked vector editing. When the lock is disabled, only the
//! edited component changes.
//!
//! The existing `LinkedValues::apply_vec2`/`apply_vec3` perform *uniform*
//! linking (all components take the edited value); these helpers perform the
//! *proportional* variant the inspector's linked Vector/Rect editors use.

use crate::inspector::LinkedValues;
use gdcore::math::{Vector2, Vector3};

/// Applies a proportional linked edit to a `Vector2` property.
///
/// `old` is the component vector before the edit and `new_component_value` is
/// the value the user entered for `changed_component` (`0` = x, `1` = y).
///
/// - If `property` is **not** linked, only `changed_component` is updated.
/// - If it **is** linked, every component is scaled by the ratio
///   `new_component_value / old[changed_component]` so the original ratios are
///   preserved. When the edited component was `0.0` (no ratio can be derived),
///   the edit falls back to changing only that component.
pub fn apply_proportional_vec2(
    linked: &LinkedValues,
    property: &str,
    old: Vector2,
    new_component_value: f32,
    changed_component: usize,
) -> Vector2 {
    let mut result = old;
    match changed_component {
        0 => result.x = new_component_value,
        _ => result.y = new_component_value,
    }
    if !linked.is_linked(property) {
        return result;
    }
    let old_component = match changed_component {
        0 => old.x,
        _ => old.y,
    };
    if old_component == 0.0 {
        return result;
    }
    let factor = new_component_value / old_component;
    Vector2::new(old.x * factor, old.y * factor)
}

/// Applies a proportional linked edit to a `Vector3` property.
///
/// Behaves like [`apply_proportional_vec2`] across three components
/// (`0` = x, `1` = y, `2` = z).
pub fn apply_proportional_vec3(
    linked: &LinkedValues,
    property: &str,
    old: Vector3,
    new_component_value: f32,
    changed_component: usize,
) -> Vector3 {
    let mut result = old;
    match changed_component {
        0 => result.x = new_component_value,
        1 => result.y = new_component_value,
        _ => result.z = new_component_value,
    }
    if !linked.is_linked(property) {
        return result;
    }
    let old_component = match changed_component {
        0 => old.x,
        1 => old.y,
        _ => old.z,
    };
    if old_component == 0.0 {
        return result;
    }
    let factor = new_component_value / old_component;
    Vector3::new(old.x * factor, old.y * factor, old.z * factor)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-7zlfn): with the proportional lock enabled, editing one
    /// component scales the remaining components by the original ratio; with it
    /// disabled, only the edited component changes.
    #[test]
    fn inspector_linked_proportional_components() {
        let mut linked = LinkedValues::new();
        let prop = "scale";
        let old = Vector2::new(2.0, 4.0);

        // Disabled: only the edited component changes.
        let unlinked = apply_proportional_vec2(&linked, prop, old, 6.0, 0);
        assert_eq!(
            unlinked,
            Vector2::new(6.0, 4.0),
            "unlinked edit only changes the edited component"
        );

        // Enabled: editing x (2 -> 6, ratio 3x) scales y proportionally
        // (4 -> 12), preserving the original y/x ratio.
        linked.link(prop);
        let scaled = apply_proportional_vec2(&linked, prop, old, 6.0, 0);
        assert_eq!(
            scaled,
            Vector2::new(6.0, 12.0),
            "linked edit scales the other component by the original ratio"
        );
        assert!(
            (scaled.y / scaled.x - old.y / old.x).abs() < 1e-6,
            "the component ratio is preserved after a linked edit"
        );

        // Editing the other component (y) is symmetric: y 4 -> 2 (0.5x) scales
        // x 2 -> 1.
        let scaled_y = apply_proportional_vec2(&linked, prop, old, 2.0, 1);
        assert_eq!(scaled_y, Vector2::new(1.0, 2.0));
    }

    #[test]
    fn proportional_vec3_and_zero_fallback() {
        let mut linked = LinkedValues::new();
        linked.link("scale");

        // 3-component proportional scaling: edit y (2 -> 4, 2x) scales all.
        let old = Vector3::new(1.0, 2.0, 3.0);
        let scaled = apply_proportional_vec3(&linked, "scale", old, 4.0, 1);
        assert_eq!(scaled, Vector3::new(2.0, 4.0, 6.0));

        // A zero edited component yields no ratio, so only that component
        // changes even when linked.
        let from_zero = Vector3::new(0.0, 5.0, 7.0);
        let edited = apply_proportional_vec3(&linked, "scale", from_zero, 9.0, 0);
        assert_eq!(edited, Vector3::new(9.0, 5.0, 7.0));
    }
}
