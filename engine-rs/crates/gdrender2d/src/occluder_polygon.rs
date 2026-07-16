//! `OccluderPolygon2D` — the 2D shadow geometry resource for `LightOccluder2D`.
//!
//! Mirrors Godot's `OccluderPolygon2D` (inherits `Resource`): a polygon of
//! vertices, a `closed` flag, and a `cull_mode` selecting one-sided occlusion
//! by winding. Provides the occluding [`edges`](OccluderPolygon2D::edges) and a
//! winding test ([`is_clockwise`](OccluderPolygon2D::is_clockwise)) that the
//! shadow pass uses to honor `cull_mode`.
//!
//! Registering the class, `.tres` sub-resource parsing, and wiring it as
//! `LightOccluder2D.occluder` are done elsewhere.

use gdcore::math::Vector2;

/// How a polygon's winding gates one-sided occlusion
/// (Godot `OccluderPolygon2D.CullMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OccluderCullMode {
    /// Occlude regardless of winding (default).
    #[default]
    Disabled,
    /// Occlude only when the polygon winds clockwise.
    Clockwise,
    /// Occlude only when the polygon winds counter-clockwise.
    CounterClockwise,
}

impl OccluderCullMode {
    /// Maps Godot's integer enum value (`0`/`1`/`2`) to a cull mode; other
    /// values fall back to [`Disabled`](OccluderCullMode::Disabled).
    pub fn from_i64(value: i64) -> Self {
        match value {
            1 => OccluderCullMode::Clockwise,
            2 => OccluderCullMode::CounterClockwise,
            _ => OccluderCullMode::Disabled,
        }
    }

    /// The Godot integer enum value for this cull mode.
    pub fn as_i64(self) -> i64 {
        match self {
            OccluderCullMode::Disabled => 0,
            OccluderCullMode::Clockwise => 1,
            OccluderCullMode::CounterClockwise => 2,
        }
    }
}

/// A 2D polygon defining a `LightOccluder2D`'s shadow geometry
/// (Godot `OccluderPolygon2D`).
#[derive(Debug, Clone, PartialEq)]
pub struct OccluderPolygon2D {
    /// Polygon vertices.
    pub polygon: Vec<Vector2>,
    /// Whether the polygon is a closed loop (Godot default `true`).
    pub closed: bool,
    /// One-sided culling mode.
    pub cull_mode: OccluderCullMode,
}

impl Default for OccluderPolygon2D {
    fn default() -> Self {
        Self {
            polygon: Vec::new(),
            closed: true,
            cull_mode: OccluderCullMode::Disabled,
        }
    }
}

impl OccluderPolygon2D {
    /// Creates an empty, closed occluder polygon with culling disabled.
    pub fn new() -> Self {
        Self::default()
    }

    /// The occluding edges as `(start, end)` segment pairs. A `closed` polygon
    /// adds the edge from the last vertex back to the first; an open polyline
    /// omits it. Returns no edges for fewer than two vertices.
    pub fn edges(&self) -> Vec<(Vector2, Vector2)> {
        let n = self.polygon.len();
        if n < 2 {
            return Vec::new();
        }
        let mut edges = Vec::with_capacity(n);
        for i in 0..n - 1 {
            edges.push((self.polygon[i], self.polygon[i + 1]));
        }
        if self.closed && n >= 3 {
            edges.push((self.polygon[n - 1], self.polygon[0]));
        }
        edges
    }

    /// Whether the polygon winds clockwise, using Godot's exact
    /// `Geometry2D::is_polygon_clockwise` convention:
    /// `Σ (v2.x - v1.x) * (v2.y + v1.y) > 0` over each edge `v1 → v2`. Returns
    /// `false` for degenerate polygons (fewer than three vertices).
    pub fn is_clockwise(&self) -> bool {
        let n = self.polygon.len();
        if n < 3 {
            return false;
        }
        let mut sum = 0.0;
        for i in 0..n {
            let v1 = self.polygon[i];
            let v2 = self.polygon[(i + 1) % n];
            sum += (v2.x - v1.x) * (v2.y + v1.y);
        }
        sum > 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f32, y: f32) -> Vector2 {
        Vector2::new(x, y)
    }

    #[test]
    fn cull_mode_int_roundtrip() {
        assert_eq!(OccluderCullMode::from_i64(0), OccluderCullMode::Disabled);
        assert_eq!(OccluderCullMode::from_i64(1), OccluderCullMode::Clockwise);
        assert_eq!(
            OccluderCullMode::from_i64(2),
            OccluderCullMode::CounterClockwise
        );
        assert_eq!(OccluderCullMode::from_i64(99), OccluderCullMode::Disabled);
        assert_eq!(OccluderCullMode::CounterClockwise.as_i64(), 2);
        assert_eq!(OccluderCullMode::default(), OccluderCullMode::Disabled);
    }

    #[test]
    fn edges_open_vs_closed() {
        let mut o = OccluderPolygon2D::new();
        o.polygon = vec![v(0.0, 0.0), v(1.0, 0.0), v(1.0, 1.0)];
        // Closed (default): three edges including the closing one.
        assert!(o.closed);
        let e = o.edges();
        assert_eq!(e.len(), 3);
        assert_eq!(e[2], (v(1.0, 1.0), v(0.0, 0.0)));
        // Open: no closing edge.
        o.closed = false;
        assert_eq!(o.edges().len(), 2);
        // Fewer than two vertices: no edges.
        o.polygon = vec![v(0.0, 0.0)];
        assert!(o.edges().is_empty());
    }

    #[test]
    fn winding_matches_godot_formula() {
        // Values below are computed directly from Godot's summation formula,
        // not from visual intuition. For (0,0)->(1,0)->(1,1)->(0,1) the sum is
        // -2, so is_clockwise() is false; the reverse gives +2 -> true.
        let mut o = OccluderPolygon2D::new();
        o.polygon = vec![v(0.0, 0.0), v(1.0, 0.0), v(1.0, 1.0), v(0.0, 1.0)];
        assert!(!o.is_clockwise());
        o.polygon.reverse();
        assert!(o.is_clockwise());
        // Degenerate polygon (<3 vertices).
        o.polygon = vec![v(0.0, 0.0), v(1.0, 0.0)];
        assert!(!o.is_clockwise());
    }
}
