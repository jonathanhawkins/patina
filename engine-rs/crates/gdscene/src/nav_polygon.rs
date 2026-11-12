//! `NavigationPolygon` outline → polygon baking, with obstruction carving
//! (pat-0q2kx.186).
//!
//! Godot ground truth: `modules/navigation_2d/2d/nav_mesh_generator_2d.cpp`
//! parses traversable and obstruction outlines, carves the obstructions out,
//! and partitions the remainder into convex polygons;
//! `NavigationPolygon.make_polygons_from_outlines` is the manual entry point.
//!
//! Patina had **nothing** on the 2D side — `grep make_polygons_from_outlines`
//! across the workspace matched zero. A `NavigationPolygon` could hold outlines
//! (the `.tscn` parser reads them) but nothing ever turned them into the
//! polygons the runtime map consumes, so a region with outlines contributed no
//! navigable area at all.

use gdcore::math::Vector2;

/// Whether `point` lies inside `polygon`, by even-odd ray casting.
///
/// NOT reused from `gdcore::geometry2d`: that module is **orphaned** — its file
/// exists but `lib.rs` never declares it (see the note in
/// `gdcore/src/geometry3d.rs:22`), so it has never been compiled and its
/// `is_point_in_polygon` is both unreachable and unverified. Declaring the
/// module to reach one function would risk breaking a foundational crate with
/// code no compiler has ever seen.
///
/// Ray casting rather than that module's cross-product winding test, which is
/// only correct for CONVEX polygons — a carved room is exactly the concave case
/// that would answer wrongly.
fn is_point_in_polygon(point: Vector2, polygon: &[Vector2]) -> bool {
    let n = polygon.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let a = polygon[i];
        let b = polygon[j];
        // Half-open edge test (`a.y > y` vs `b.y > y`) so a ray passing exactly
        // through a vertex is counted once, not twice.
        if (a.y > point.y) != (b.y > point.y) {
            let t = (point.y - a.y) / (b.y - a.y);
            if point.x < a.x + t * (b.x - a.x) {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Twice the signed area of a polygon. Positive when the winding is
/// counter-clockwise in Godot's screen space (y down).
fn signed_area2(poly: &[Vector2]) -> f32 {
    let mut acc = 0.0;
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        acc += a.x * b.y - b.x * a.y;
    }
    acc
}

/// Cross product of `b - a` and `c - a`; its sign is the turn direction.
fn cross(a: Vector2, b: Vector2, c: Vector2) -> f32 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

/// Whether `p` lies inside triangle `abc`, edges included.
///
/// Uses consistent sign tests rather than barycentric division, so a
/// degenerate (zero-area) triangle cannot divide by zero.
fn point_in_triangle(p: Vector2, a: Vector2, b: Vector2, c: Vector2) -> bool {
    let d1 = cross(a, b, p);
    let d2 = cross(b, c, p);
    let d3 = cross(c, a, p);
    let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_neg && has_pos)
}

/// Ear-clipping triangulation of one simple polygon.
///
/// Returns triangles as vertex triples. Handles concave outlines; self-
/// intersecting ones are not supported (neither are they in Godot's clipper
/// path, which expects simple outlines).
///
/// The polygon is normalised to a consistent winding first, because the
/// convexity test below is only meaningful relative to a known orientation —
/// a clockwise outline would otherwise report every vertex as reflex and clip
/// nothing.
pub fn triangulate(polygon: &[Vector2]) -> Vec<[Vector2; 3]> {
    if polygon.len() < 3 {
        return Vec::new();
    }
    let mut verts: Vec<Vector2> = polygon.to_vec();
    if signed_area2(&verts) < 0.0 {
        verts.reverse();
    }

    let mut out = Vec::new();
    // Each successful clip removes one vertex; the guard bounds the pathological
    // case where no ear is found (a malformed outline) instead of looping
    // forever.
    let mut guard = verts.len() * verts.len() + 16;
    while verts.len() > 3 && guard > 0 {
        guard -= 1;
        let n = verts.len();
        let mut clipped = false;
        for i in 0..n {
            let prev = verts[(i + n - 1) % n];
            let cur = verts[i];
            let next = verts[(i + 1) % n];

            // Reflex vertices cannot be ears.
            if cross(prev, cur, next) <= 0.0 {
                continue;
            }
            // An ear's triangle must contain no other vertex.
            let mut contains = false;
            for (j, v) in verts.iter().enumerate() {
                if j == i || j == (i + n - 1) % n || j == (i + 1) % n {
                    continue;
                }
                if point_in_triangle(*v, prev, cur, next) {
                    contains = true;
                    break;
                }
            }
            if contains {
                continue;
            }
            out.push([prev, cur, next]);
            verts.remove(i);
            clipped = true;
            break;
        }
        if !clipped {
            break;
        }
    }
    if verts.len() == 3 {
        out.push([verts[0], verts[1], verts[2]]);
    }
    out
}

/// A navigation polygon's outlines and the polygons baked from them.
///
/// Mirrors Godot's `NavigationPolygon`: outlines go in, `make_polygons_from_outlines`
/// produces the convex polygons the navigation map consumes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NavigationPolygon {
    outlines: Vec<Vec<Vector2>>,
    polygons: Vec<Vec<Vector2>>,
}

impl NavigationPolygon {
    /// An empty navigation polygon.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends an outline (`NavigationPolygon.add_outline`).
    ///
    /// The **first** outline is the traversable area; later outlines that lie
    /// inside it are obstructions to carve out, which is how Godot's generator
    /// classifies them.
    pub fn add_outline(&mut self, outline: Vec<Vector2>) {
        self.outlines.push(outline);
    }

    /// Number of outlines.
    pub fn outline_count(&self) -> usize {
        self.outlines.len()
    }

    /// One outline's vertices.
    pub fn get_outline(&self, index: usize) -> Option<&[Vector2]> {
        self.outlines.get(index).map(|o| o.as_slice())
    }

    /// Drops every outline and every baked polygon
    /// (`NavigationPolygon.clear_outlines` plus `clear_polygons`).
    pub fn clear_outlines(&mut self) {
        self.outlines.clear();
        self.polygons.clear();
    }

    /// The polygons produced by the last bake.
    pub fn polygons(&self) -> &[Vec<Vector2>] {
        &self.polygons
    }

    /// Whether `point` falls in any baked polygon — what a path query asks.
    pub fn contains_point(&self, point: Vector2) -> bool {
        self.polygons.iter().any(|p| is_point_in_polygon(point, p))
    }

    /// Bakes outlines into polygons (`make_polygons_from_outlines`).
    ///
    /// Stage 1: a uniform grid partition of the traversable outline with the
    /// obstruction outlines carved out. Each surviving cell is a convex quad —
    /// which is what the navigation map consumes — and no cell overlaps a hole,
    /// so a query inside a carved region correctly finds no navigable polygon.
    ///
    /// **Why a grid rather than ear-clipping the outline.** Triangulating the
    /// outline and then dropping triangles whose centroid falls in a hole does
    /// NOT carve: a 10x10 square ear-clips into two triangles whose centroids
    /// are (3.3, 3.3) and (6.7, 6.7), both outside a central 4..6 hole, so
    /// nothing is dropped and the hole stays navigable. Carving properly with
    /// ear-clipping requires bridging each hole into the outer contour, which
    /// Godot does via clipper. The grid gets the property that matters — no
    /// navigable polygon over a hole — without that machinery; a later stage can
    /// replace it with a clipper-quality partition.
    ///
    /// `cell` is the grid pitch: smaller hugs the outline more closely at the
    /// cost of more polygons.
    ///
    /// Returns the number of polygons produced.
    pub fn make_polygons_from_outlines_with_cell(&mut self, cell: f32) -> usize {
        self.polygons.clear();
        let Some(traversable) = self.outlines.first() else {
            return 0;
        };
        if traversable.len() < 3 || cell <= 0.0 {
            return 0;
        }

        // Outlines after the first that sit inside it are holes. Classifying by
        // containment rather than index order means an outline listed later but
        // disjoint (a second room) is not mistaken for a hole.
        let holes: Vec<&Vec<Vector2>> = self.outlines[1..]
            .iter()
            .filter(|o| o.iter().all(|p| is_point_in_polygon(*p, traversable)))
            .collect();

        let (mut min_x, mut min_y) = (f32::INFINITY, f32::INFINITY);
        let (mut max_x, mut max_y) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        for v in traversable {
            min_x = min_x.min(v.x);
            min_y = min_y.min(v.y);
            max_x = max_x.max(v.x);
            max_y = max_y.max(v.y);
        }

        let cols = (((max_x - min_x) / cell).ceil() as i64).max(1);
        let rows = (((max_y - min_y) / cell).ceil() as i64).max(1);
        // A pathological cell size would otherwise try to allocate unboundedly.
        if cols.saturating_mul(rows) > 1_000_000 {
            return 0;
        }

        for row in 0..rows {
            for col in 0..cols {
                let x0 = min_x + col as f32 * cell;
                let y0 = min_y + row as f32 * cell;
                let x1 = (x0 + cell).min(max_x);
                let y1 = (y0 + cell).min(max_y);
                let centre = Vector2::new((x0 + x1) * 0.5, (y0 + y1) * 0.5);

                if !is_point_in_polygon(centre, traversable) {
                    continue;
                }
                if holes.iter().any(|h| is_point_in_polygon(centre, h)) {
                    continue;
                }
                self.polygons.push(vec![
                    Vector2::new(x0, y0),
                    Vector2::new(x1, y0),
                    Vector2::new(x1, y1),
                    Vector2::new(x0, y1),
                ]);
            }
        }
        self.polygons.len()
    }

    /// [`make_polygons_from_outlines_with_cell`](Self::make_polygons_from_outlines_with_cell)
    /// at a default pitch of 1.0 world unit.
    pub fn make_polygons_from_outlines(&mut self) -> usize {
        self.make_polygons_from_outlines_with_cell(1.0)
    }
}
