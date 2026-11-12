//! Path3D / PathFollow3D / Curve3D (pat-p0s82.15).

use crate::bezier_bake::CurvePoint3;
use gdcore::math::Vector3;

/// Standard cubic bezier, Godot's `Vector3::bezier_interpolate`.
///
/// ⚠️ Local rather than shared with `bezier_bake`'s copy on purpose: that one is
/// private to its module, and `gdcore`'s `Vector3` has no `bezier_interpolate`
/// yet. Widening either to reach it would put an edit into a base crate for the
/// sake of four lines of arithmetic.
fn bezier3(p0: Vector3, p1: Vector3, p2: Vector3, p3: Vector3, t: f32) -> Vector3 {
    let u = 1.0 - t;
    p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t)
}

/// Godot's `Curve3D::_bake_segment3d` — records a midpoint only where the
/// direction turns by more than `tolerance_degrees`.
///
/// ⚠️ The recursion is **unconditional** to `max_depth`. It does NOT stop when
/// the angle test passes, the way a chord-length subdivision stops when the
/// chord is short enough: a segment can be locally straight at this depth and
/// bend sharply inside one of its halves, and an early return would miss that
/// corner entirely. Only whether the midpoint is RECORDED depends on the test.
#[allow(clippy::too_many_arguments)]
fn bake_segment3d(
    out: &mut Vec<(f32, Vector3)>,
    begin: f32,
    end: f32,
    a: Vector3,
    out_handle: Vector3,
    b: Vector3,
    in_handle: Vector3,
    depth: u32,
    max_depth: u32,
    tolerance_degrees: f32,
) {
    let (c1, c2) = (a + out_handle, b + in_handle);
    let mp = begin + (end - begin) * 0.5;
    let beg_p = bezier3(a, c1, c2, b, begin);
    let mid_p = bezier3(a, c1, c2, b, mp);
    let end_p = bezier3(a, c1, c2, b, end);

    let na = (mid_p - beg_p).normalized();
    let nb = (end_p - mid_p).normalized();
    if na.dot(nb) < tolerance_degrees.to_radians().cos() {
        out.push((mp, mid_p));
    }
    if depth < max_depth {
        bake_segment3d(
            out,
            begin,
            mp,
            a,
            out_handle,
            b,
            in_handle,
            depth + 1,
            max_depth,
            tolerance_degrees,
        );
        bake_segment3d(
            out,
            mp,
            end,
            a,
            out_handle,
            b,
            in_handle,
            depth + 1,
            max_depth,
            tolerance_degrees,
        );
    }
}

/// Cubic / linear curve sample in 3D (Godot Curve3D subset).
#[derive(Debug, Clone, Default)]
pub struct Curve3D {
    /// Control points (baked polyline for sampling).
    pub points: Vec<Vector3>,
    /// Per-sample tilt in radians, index-for-index with
    /// [`points`](Self::points) — Godot's `baked_tilt_cache`.
    ///
    /// Empty means "no tilt authored", which is distinct from all-zero only in
    /// that it lets a caller skip the twist entirely.
    pub tilts: Vec<f32>,
    /// Per-sample UP VECTORS, index-for-index with [`points`](Self::points) —
    /// Godot's `baked_up_vector_cache`, built by parallel transport.
    ///
    /// Empty when the curve has `up_vector_enabled = false`, which is Godot's
    /// own representation of that setting (`curve.cpp:1776-1779` clears the
    /// cache); callers then fall back to world up.
    pub up_vectors: Vec<Vector3>,
    /// The AUTHORED control points, with their bezier handles — Godot's
    /// `Curve3D::points` (pat-s3rrv.91 item 5).
    ///
    /// ⚠️ **Not the same array as [`points`](Self::points).** Patina's `points`
    /// is the BAKED polyline; Godot's `points` is this control array. Every
    /// method ported from upstream reads `control` where upstream reads
    /// `points`, and reading the wrong one silently samples the wrong curve:
    /// the baked polyline has far more entries, so an index that upstream means
    /// as "control point 2" lands a few percent along the path instead.
    ///
    /// Empty on a curve that was baked without retaining its handles, in which
    /// case `sample`/`samplef`/`tessellate` have nothing to evaluate and return
    /// the same zero/empty results Godot gives for a point-less curve.
    pub control: Vec<CurvePoint3>,
    /// Loops the last control point back to the first (`Curve3D.closed`).
    ///
    /// Read by [`sample`](Self::sample) and [`tessellate`](Self::tessellate),
    /// which both treat the wrap as one more interval.
    pub closed: bool,
}

impl Curve3D {
    /// Empty curve.
    pub fn new() -> Self {
        Self {
            points: Vec::new(),
            tilts: Vec::new(),
            up_vectors: Vec::new(),
            control: Vec::new(),
            closed: false,
        }
    }

    /// Add a point.
    pub fn add_point(&mut self, p: Vector3) {
        self.points.push(p);
    }

    /// Arc length of the polyline.
    pub fn baked_length(&self) -> f32 {
        let mut len = 0.0f32;
        for w in self.points.windows(2) {
            len += (w[1] - w[0]).length();
        }
        len
    }

    /// Godot `Curve3D.sample(idx, t)` — the bezier between control point `idx`
    /// and the next one, at parameter `t` (`curve.cpp` `Curve3D::sample`).
    ///
    /// ⚠️ Reads [`control`](Self::control), NOT the baked polyline. `t` is the
    /// bezier parameter of one segment, not an arc-length offset and not a
    /// fraction of the whole curve — `sample(0, 0.5)` is the midpoint of the
    /// FIRST segment, which on an uneven curve is nowhere near the halfway
    /// point of the path.
    ///
    /// The out-of-range behaviour is upstream's and is deliberately not a
    /// clamp on both ends: past the last segment an OPEN curve returns its last
    /// position, while a CLOSED one wraps to the segment joining last to first.
    pub fn sample(&self, index: i32, t: f32) -> Vector3 {
        let pc = self.control.len();
        if pc == 0 {
            return Vector3::new(0.0, 0.0, 0.0);
        }
        let mut index = index;
        if index >= pc as i32 - 1 {
            if !self.closed {
                return self.control[pc - 1].position;
            }
            index = pc as i32 - 1;
        } else if index < 0 {
            return self.control[0].position;
        }
        let i = index as usize;
        let p0 = self.control[i].position;
        let p1 = p0 + self.control[i].out_handle;
        let (p3, p2) = if !self.closed || i < pc - 1 {
            let p3 = self.control[i + 1].position;
            (p3, p3 + self.control[i + 1].in_handle)
        } else {
            let p3 = self.control[0].position;
            (p3, p3 + self.control[0].in_handle)
        };
        bezier3(p0, p1, p2, p3, t)
    }

    /// Godot `Curve3D.samplef(findex)` — sample at a FRACTIONAL control-point
    /// index (`curve.cpp` `Curve3D::samplef`).
    ///
    /// ⚠️ The upper clamp is `control.len()`, not `len() - 1`. That looks like
    /// an off-by-one and is upstream's actual behaviour: the clamped index then
    /// trips `sample`'s own `index >= pc - 1` branch, so an open curve returns
    /// its final position. Clamping to `len() - 1` here would instead evaluate
    /// the last segment at a leftover fraction and return a point short of the
    /// end. Transcribed rather than tidied.
    pub fn samplef(&self, findex: f32) -> Vector3 {
        let pc = self.control.len() as f32;
        let findex = if findex < 0.0 {
            0.0
        } else if findex >= pc {
            pc
        } else {
            findex
        };
        self.sample(findex as i32, findex % 1.0)
    }

    /// Godot `Curve3D.tessellate(max_stages, tolerance_degrees)` — an
    /// ANGLE-adaptive polyline through the control points (`curve.cpp`
    /// `Curve3D::tessellate` / `_bake_segment3d`).
    ///
    /// ⚠️ **A different algorithm from the `bake_interval` subdivision**, not a
    /// variant of it. `_tessellate_even_length` splits while a sub-interval's
    /// CHORD is too long and stops as soon as it is short enough; this splits on
    /// how far the direction TURNS and always recurses to `max_stages`,
    /// recording a midpoint only where the turn exceeds the tolerance. So this
    /// one spends its samples on corners and leaves straight runs nearly bare —
    /// the opposite distribution. Using either where the other belongs gives a
    /// plausible polyline of roughly the right size that is wrong exactly where
    /// it matters.
    pub fn tessellate(&self, max_stages: u32, tolerance_degrees: f32) -> Vec<Vector3> {
        let pc = self.control.len();
        if pc == 0 {
            return Vec::new();
        }
        let num_intervals = if self.closed {
            pc
        } else {
            pc.saturating_sub(1)
        };
        let mut out = vec![self.control[0].position];
        for i in 0..num_intervals {
            let a = self.control[i];
            let b = if !self.closed || i < num_intervals - 1 {
                self.control[i + 1]
            } else {
                self.control[0]
            };
            let mut mids: Vec<(f32, Vector3)> = Vec::new();
            bake_segment3d(
                &mut mids,
                0.0,
                1.0,
                a.position,
                a.out_handle,
                b.position,
                b.in_handle,
                0,
                max_stages,
                tolerance_degrees,
            );
            // Upstream collects into an RBMap keyed by `t`: sorted, and one
            // entry per parameter value.
            mids.sort_by(|l, r| l.0.partial_cmp(&r.0).unwrap_or(std::cmp::Ordering::Equal));
            mids.dedup_by(|l, r| l.0 == r.0);
            out.extend(mids.into_iter().map(|(_, p)| p));
            out.push(b.position);
        }
        out
    }

    /// Sample a point at arc-length `offset` (clamped).
    pub fn sample_baked(&self, offset: f32) -> Vector3 {
        if self.points.is_empty() {
            return Vector3::ZERO;
        }
        if self.points.len() == 1 {
            return self.points[0];
        }
        let total = self.baked_length().max(1e-6);
        let mut o = offset.clamp(0.0, total);
        for w in self.points.windows(2) {
            let seg = (w[1] - w[0]).length();
            // ⚠️ A degenerate segment is SKIPPED, not returned from. The old
            // `|| seg < 1e-8` clause avoided the divide-by-zero but stopped the
            // walk at the first zero-length segment, so every offset past it
            // collapsed onto that point and the curve was effectively truncated
            // there — a PathFollow would advance normally and then stick.
            // Coincident control points bake to duplicate baked points, so this
            // is reachable from ordinary authoring. (Found by MagentaBrook,
            // pat-s3rrv.91.)
            if seg < 1e-8 {
                continue;
            }
            if o <= seg {
                let t = o / seg;
                return Vector3::new(
                    w[0].x + (w[1].x - w[0].x) * t,
                    w[0].y + (w[1].y - w[0].y) * t,
                    w[0].z + (w[1].z - w[0].z) * t,
                );
            }
            o -= seg;
        }
        *self.points.last().unwrap()
    }

    /// Cubic (Catmull-Rom) sample at arc-length `offset` — Godot's
    /// `sample_baked(offset, cubic = true)` (pat-s3rrv.91 item 2).
    ///
    /// Godot's `_sample_baked` cubic branch (`curve.cpp`) interpolates the
    /// segment's two endpoints against their neighbours:
    ///
    /// ```text
    /// pre  = idx > 0        ? r[idx - 1] : r[idx];
    /// post = idx < pc - 2   ? r[idx + 2] : r[idx + 1];
    /// r[idx].cubic_interpolate(r[idx + 1], pre, post, frac)
    /// ```
    ///
    /// ⚠️ Both neighbours CLAMP at the ends rather than wrapping. Clamping makes
    /// the first and last segments degrade to something close to the linear
    /// result, which is what keeps the curve from bulging outward past its own
    /// endpoints; wrapping would tie the two ends together on an open curve.
    ///
    /// ⚠️ Walks with the same zero-length SKIP as
    /// [`sample_baked`](Self::sample_baked). Reintroducing the old
    /// return-on-degenerate here would have put the truncation bug back on the
    /// cubic path only — present for `cubic_interp = true`, which is Godot's
    /// DEFAULT, and absent from the linear path anyone would test against.
    pub fn sample_baked_cubic(&self, offset: f32) -> Vector3 {
        if self.points.is_empty() {
            return Vector3::ZERO;
        }
        if self.points.len() == 1 {
            return self.points[0];
        }
        let count = self.points.len();
        let total = self.baked_length().max(1e-6);
        let mut o = offset.clamp(0.0, total);
        for (i, w) in self.points.windows(2).enumerate() {
            let seg = (w[1] - w[0]).length();
            if seg < 1e-8 {
                continue;
            }
            if o <= seg {
                let t = o / seg;
                let pre = if i > 0 { self.points[i - 1] } else { w[0] };
                let post = if i + 2 < count {
                    self.points[i + 2]
                } else {
                    w[1]
                };
                return w[0].cubic_interpolate(w[1], pre, post, t);
            }
            o -= seg;
        }
        *self.points.last().unwrap()
    }

    /// Tilt in radians at arc-length `offset`, mirroring Godot's
    /// `Curve3D::sample_baked_tilt`.
    ///
    /// Walks the polyline exactly as [`sample_baked`](Self::sample_baked) does
    /// and lerps between the two adjacent baked tilts with the same fraction,
    /// so the angle returned always belongs to the point that method returns
    /// (`curve.cpp:1936`: `Math::lerp(r[idx], r[idx + 1], frac)`).
    ///
    /// Returns `0.0` when no tilts were baked, which is Godot's untilted
    /// default rather than a failure value.
    pub fn sample_baked_tilt(&self, offset: f32) -> f32 {
        if self.tilts.is_empty() {
            return 0.0;
        }
        if self.tilts.len() == 1 || self.points.len() < 2 {
            return self.tilts[0];
        }
        let total = self.baked_length().max(1e-6);
        let mut o = offset.clamp(0.0, total);
        for (i, w) in self.points.windows(2).enumerate() {
            let seg = (w[1] - w[0]).length();
            // ⚠️ A degenerate segment is SKIPPED, not returned from. The old
            // `|| seg < 1e-8` clause avoided the divide-by-zero but stopped the
            // walk at the first zero-length segment, so every offset past it
            // collapsed onto that point and the curve was effectively truncated
            // there — a PathFollow would advance normally and then stick.
            // Coincident control points bake to duplicate baked points, so this
            // is reachable from ordinary authoring. (Found by MagentaBrook,
            // pat-s3rrv.91.)
            if seg < 1e-8 {
                continue;
            }
            if o <= seg {
                let t = o / seg;
                let a = self.tilts.get(i).copied().unwrap_or(0.0);
                let b = self.tilts.get(i + 1).copied().unwrap_or(a);
                return a + (b - a) * t;
            }
            o -= seg;
        }
        self.tilts.last().copied().unwrap_or(0.0)
    }

    /// Up vector at arc-length `offset`, mirroring Godot's
    /// `Curve3D::sample_baked_up_vector`.
    ///
    /// Walks the polyline exactly as [`sample_baked`](Self::sample_baked) does
    /// and interpolates between the two adjacent baked vectors, renormalizing —
    /// a lerp between two unit vectors is shorter than unit, and feeding an
    /// unnormalized up into a frame skews it.
    ///
    /// Returns world up when nothing was baked, which is Godot's documented
    /// fallback for a curve with `up_vector_enabled = false` rather than a
    /// failure value.
    pub fn sample_baked_up_vector(&self, offset: f32) -> Vector3 {
        let world_up = Vector3::new(0.0, 1.0, 0.0);
        if self.up_vectors.is_empty() {
            return world_up;
        }
        if self.up_vectors.len() == 1 || self.points.len() < 2 {
            return self.up_vectors[0];
        }
        let total = self.baked_length().max(1e-6);
        let mut o = offset.clamp(0.0, total);
        for (i, w) in self.points.windows(2).enumerate() {
            let seg = (w[1] - w[0]).length();
            // ⚠️ A degenerate segment is SKIPPED, not returned from. The old
            // `|| seg < 1e-8` clause avoided the divide-by-zero but stopped the
            // walk at the first zero-length segment, so every offset past it
            // collapsed onto that point and the curve was effectively truncated
            // there — a PathFollow would advance normally and then stick.
            // Coincident control points bake to duplicate baked points, so this
            // is reachable from ordinary authoring. (Found by MagentaBrook,
            // pat-s3rrv.91.)
            if seg < 1e-8 {
                continue;
            }
            if o <= seg {
                let t = o / seg;
                let a = self.up_vectors.get(i).copied().unwrap_or(world_up);
                let b = self.up_vectors.get(i + 1).copied().unwrap_or(a);
                let lerped = a + (b - a) * t;
                return if lerped.length() > 1e-6 {
                    lerped.normalized()
                } else {
                    a
                };
            }
            o -= seg;
        }
        self.up_vectors.last().copied().unwrap_or(world_up)
    }

    /// Closest point on the curve to `to_point`.
    pub fn get_closest_point(&self, to_point: Vector3) -> Vector3 {
        if self.points.is_empty() {
            return Vector3::ZERO;
        }
        let mut best = self.points[0];
        let mut best_d = f32::MAX;
        let steps = 32.max(self.points.len() * 4);
        let total = self.baked_length();
        for i in 0..=steps {
            let o = total * (i as f32) / (steps as f32);
            let p = self.sample_baked(o);
            let d = (p - to_point).length_squared();
            if d < best_d {
                best_d = d;
                best = p;
            }
        }
        best
    }

    /// Normalized tangent (direction of travel) at arc-length `offset`.
    ///
    /// Uses a small central finite difference along the baked polyline. Falls
    /// back to +Z for degenerate curves so callers always get a unit vector.
    pub fn sample_tangent(&self, offset: f32) -> Vector3 {
        let len = self.baked_length();
        if len <= 1e-6 || self.points.len() < 2 {
            return Vector3::new(0.0, 0.0, 1.0);
        }
        let eps = (len * 0.001).max(1e-3);
        let a = self.sample_baked((offset - eps).clamp(0.0, len));
        let b = self.sample_baked((offset + eps).clamp(0.0, len));
        let d = b - a;
        let l = d.length();
        if l <= 1e-8 {
            Vector3::new(0.0, 0.0, 1.0)
        } else {
            Vector3::new(d.x / l, d.y / l, d.z / l)
        }
    }
}

/// Path3D node data: owns a Curve3D.
#[derive(Debug, Clone, Default)]
pub struct Path3D {
    pub curve: Curve3D,
}

/// PathFollow3D: samples Path3D at an offset.
#[derive(Debug, Clone)]
pub struct PathFollow3D {
    pub progress: f32,
    pub loop_path: bool,
    pub rotation_mode: PathFollowRotation,
}

/// How PathFollow3D orients along the path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PathFollowRotation {
    #[default]
    None,
    Y,
    Xy,
    Xyz,
    /// Godot's `ROTATION_ORIENTED` (4): the full frame from
    /// `Basis::looking_at(tangent)` with no axis locked.
    ///
    /// ⚠️ Added because the property is an INT on the node and mode 4 used to
    /// fall through to [`None`](Self::None) — a follower authored as ORIENTED
    /// silently stopped rotating at all, which reads as "rotation is broken"
    /// rather than as an unhandled mode.
    Oriented,
}

impl Default for PathFollow3D {
    fn default() -> Self {
        Self {
            progress: 0.0,
            loop_path: true,
            rotation_mode: PathFollowRotation::None,
        }
    }
}

impl PathFollow3D {
    /// Advance progress by `delta` along `curve`, optionally looping.
    pub fn advance(&mut self, curve: &Curve3D, delta: f32) {
        let len = curve.baked_length();
        if len <= 1e-6 {
            return;
        }
        self.progress += delta;
        if self.loop_path {
            self.progress = self.progress.rem_euclid(len);
        } else {
            self.progress = self.progress.clamp(0.0, len);
        }
    }

    /// World sample position.
    pub fn position_on(&self, curve: &Curve3D) -> Vector3 {
        curve.sample_baked(self.progress)
    }
}

/// Curve2D for Path2D runtime (same as Curve3D but Vector2).
#[derive(Debug, Clone, Default)]
pub struct Curve2D {
    pub points: Vec<gdcore::math::Vector2>,
}

impl Curve2D {
    pub fn add_point(&mut self, p: gdcore::math::Vector2) {
        self.points.push(p);
    }
    pub fn baked_length(&self) -> f32 {
        let mut len = 0.0f32;
        for w in self.points.windows(2) {
            len += (w[1] - w[0]).length();
        }
        len
    }
    pub fn sample_baked(&self, offset: f32) -> gdcore::math::Vector2 {
        use gdcore::math::Vector2;
        if self.points.is_empty() {
            return Vector2::ZERO;
        }
        if self.points.len() == 1 {
            return self.points[0];
        }
        let total = self.baked_length().max(1e-6);
        let mut o = offset.clamp(0.0, total);
        for w in self.points.windows(2) {
            let seg = (w[1] - w[0]).length();
            // ⚠️ A degenerate segment is SKIPPED, not returned from. The old
            // `|| seg < 1e-8` clause avoided the divide-by-zero but stopped the
            // walk at the first zero-length segment, so every offset past it
            // collapsed onto that point and the curve was effectively truncated
            // there — a PathFollow would advance normally and then stick.
            // Coincident control points bake to duplicate baked points, so this
            // is reachable from ordinary authoring. (Found by MagentaBrook,
            // pat-s3rrv.91.)
            if seg < 1e-8 {
                continue;
            }
            if o <= seg {
                let t = o / seg;
                return Vector2::new(
                    w[0].x + (w[1].x - w[0].x) * t,
                    w[0].y + (w[1].y - w[0].y) * t,
                );
            }
            o -= seg;
        }
        *self.points.last().unwrap()
    }

    /// Cubic (Catmull-Rom) sample at arc-length `offset` — Godot's
    /// `sample_baked(offset, cubic = true)`, which is `PathFollow2D`'s DEFAULT
    /// (pat-s3rrv.241).
    ///
    /// Mirrors [`Curve3D::sample_baked_cubic`] exactly; the 3D path has had this
    /// since pat-s3rrv.91 while the 2D one sampled linearly no matter what
    /// `cubic_interp` said.
    ///
    /// ⚠️ Both neighbours CLAMP at the ends rather than wrapping. Clamping makes
    /// the first and last segments degrade toward the linear result, which keeps
    /// the curve from bulging past its own endpoints; wrapping would tie the two
    /// ends of an open curve together.
    ///
    /// ⚠️ Uses the same zero-length SKIP as [`sample_baked`](Self::sample_baked)
    /// rather than returning early. Returning on a degenerate segment truncates
    /// the curve there — a bug that would live on the cubic path ONLY, which is
    /// the default, and be absent from the linear path anyone would compare
    /// against.
    pub fn sample_baked_cubic(&self, offset: f32) -> gdcore::math::Vector2 {
        use gdcore::math::Vector2;
        if self.points.is_empty() {
            return Vector2::ZERO;
        }
        if self.points.len() == 1 {
            return self.points[0];
        }
        let count = self.points.len();
        let total = self.baked_length().max(1e-6);
        let mut o = offset.clamp(0.0, total);
        for (i, w) in self.points.windows(2).enumerate() {
            let seg = (w[1] - w[0]).length();
            if seg < 1e-8 {
                continue;
            }
            if o <= seg {
                let t = o / seg;
                let pre = if i > 0 { self.points[i - 1] } else { w[0] };
                let post = if i + 2 < count {
                    self.points[i + 2]
                } else {
                    w[1]
                };
                return w[0].cubic_interpolate(w[1], pre, post, t);
            }
            o -= seg;
        }
        *self.points.last().unwrap()
    }

    /// Normalized tangent (direction of travel) at arc-length `offset`.
    pub fn sample_tangent(&self, offset: f32) -> gdcore::math::Vector2 {
        use gdcore::math::Vector2;
        let len = self.baked_length();
        if len <= 1e-6 || self.points.len() < 2 {
            return Vector2::new(1.0, 0.0);
        }
        let eps = (len * 0.001).max(1e-3);
        let a = self.sample_baked((offset - eps).clamp(0.0, len));
        let b = self.sample_baked((offset + eps).clamp(0.0, len));
        let d = b - a;
        let l = d.length();
        if l <= 1e-8 {
            Vector2::new(1.0, 0.0)
        } else {
            Vector2::new(d.x / l, d.y / l)
        }
    }
}

/// PathFollow2D: samples a Path2D curve at an offset and optionally rotates the
/// node to face the direction of travel.
#[derive(Debug, Clone)]
pub struct PathFollow2D {
    pub progress: f32,
    pub loop_path: bool,
    pub rotates: bool,
}

impl Default for PathFollow2D {
    fn default() -> Self {
        Self {
            progress: 0.0,
            loop_path: true,
            rotates: true,
        }
    }
}

impl PathFollow2D {
    /// Advance progress by `delta` along `curve`, optionally looping.
    pub fn advance(&mut self, curve: &Curve2D, delta: f32) {
        let len = curve.baked_length();
        if len <= 1e-6 {
            return;
        }
        self.progress += delta;
        if self.loop_path {
            self.progress = self.progress.rem_euclid(len);
        } else {
            self.progress = self.progress.clamp(0.0, len);
        }
    }

    /// World sample position.
    pub fn position_on(&self, curve: &Curve2D) -> gdcore::math::Vector2 {
        curve.sample_baked(self.progress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2-point curve with ASYMMETRIC handles, and a deliberately bogus baked
    /// polyline so that reading the wrong array cannot pass.
    fn handled_curve() -> Curve3D {
        let mut c = Curve3D::new();
        c.control = vec![
            CurvePoint3 {
                position: Vector3::new(0.0, 0.0, 0.0),
                in_handle: Vector3::new(0.0, 0.0, 0.0),
                out_handle: Vector3::new(0.0, 9.0, 0.0),
            },
            CurvePoint3 {
                position: Vector3::new(10.0, 0.0, 0.0),
                in_handle: Vector3::new(-6.0, 0.0, 3.0),
                out_handle: Vector3::new(0.0, 0.0, 0.0),
            },
        ];
        // 🔑 The trap. `sample`/`samplef`/`tessellate` are ports of methods
        // whose upstream `points` is the CONTROL array, while Patina's `points`
        // is the baked polyline. Anything reading this field lands on 999.
        c.points = vec![Vector3::new(999.0, 999.0, 999.0)];
        c
    }

    fn near(a: Vector3, b: Vector3) -> bool {
        (a - b).length() < 1e-4
    }

    /// 🔑 `sample` evaluates the bezier of one segment, from the CONTROL points.
    ///
    /// Expected values derived from the cubic bezier by hand, not read back from
    /// the implementation: with `c1 = (0,9,0)` and `c2 = (4,0,3)`, `B(0.5)` is
    /// `0.125*p0 + 0.375*c1 + 0.375*c2 + 0.125*p3 = (2.75, 3.375, 1.125)`.
    #[test]
    fn curve3d_sample_reads_control_points_not_the_baked_polyline() {
        let c = handled_curve();
        assert!(
            near(c.sample(0, 0.5), Vector3::new(2.75, 3.375, 1.125)),
            "sample(0, 0.5) must be the bezier midpoint of the first segment; \
             got {:?}. A value near (999, 999, 999) means it read the baked \
             polyline instead of the control points",
            c.sample(0, 0.5),
        );
        assert!(
            near(
                c.sample(0, 0.25),
                Vector3::new(0.718_75, 3.796_875, 0.421_875)
            ),
            "sample(0, 0.25) must follow the same bezier; got {:?}",
            c.sample(0, 0.25),
        );
        // The handles are what bend it: a straight read of the control
        // positions would put t = 0.5 at (5, 0, 0).
        assert!(
            !near(c.sample(0, 0.5), Vector3::new(5.0, 0.0, 0.0)),
            "the out/in handles must actually curve the segment, or the fixture \
             is not exercising the bezier at all",
        );
    }

    /// 🔑 `samplef` takes a FRACTIONAL control-point index, and its upper clamp
    /// is upstream's `len()` rather than `len() - 1`.
    #[test]
    fn curve3d_samplef_clamps_past_the_end_to_the_final_position() {
        let c = handled_curve();
        assert!(
            near(c.samplef(0.5), c.sample(0, 0.5)),
            "samplef(0.5) must equal sample(0, 0.5)",
        );
        let last = Vector3::new(10.0, 0.0, 0.0);
        for beyond in [2.0f32, 5.0, 99.0] {
            assert!(
                near(c.samplef(beyond), last),
                "samplef({beyond}) past the last control point must give the \
                 final position {last:?}; got {:?}. Clamping to len() - 1 \
                 instead would evaluate the last segment at a leftover \
                 fraction and land short of the end",
                c.samplef(beyond),
            );
        }
        assert!(
            near(c.samplef(-3.0), Vector3::new(0.0, 0.0, 0.0)),
            "a negative index must give the first position, got {:?}",
            c.samplef(-3.0),
        );
    }

    /// 🔑 `tessellate` spends its samples on CURVATURE, not on length.
    ///
    /// ⚠️ This is the assertion that separates it from the `bake_interval`
    /// subdivision. A 10-unit straight run is the longest thing in this test and
    /// gets exactly two points; a bend spanning the same distance gets dozens. A
    /// chord-length subdivision would do the reverse on the straight run — many
    /// evenly spaced points — so an implementation that reused the even-length
    /// recursion here fails on the straight case, not the curved one.
    #[test]
    fn curve3d_tessellate_spends_points_on_curvature_not_length() {
        let mut straight = Curve3D::new();
        straight.control = vec![
            CurvePoint3 {
                position: Vector3::new(0.0, 0.0, 0.0),
                in_handle: Vector3::new(0.0, 0.0, 0.0),
                out_handle: Vector3::new(0.0, 0.0, 0.0),
            },
            CurvePoint3 {
                position: Vector3::new(10.0, 0.0, 0.0),
                in_handle: Vector3::new(0.0, 0.0, 0.0),
                out_handle: Vector3::new(0.0, 0.0, 0.0),
            },
        ];
        let flat = straight.tessellate(5, 4.0);
        assert_eq!(
            flat.len(),
            2,
            "a perfectly straight segment never turns, so tessellate records no \
             midpoints and returns just its endpoints; got {flat:?}. A \
             chord-length subdivision would return dozens here",
        );
        assert!(
            near(flat[0], Vector3::new(0.0, 0.0, 0.0))
                && near(flat[1], Vector3::new(10.0, 0.0, 0.0)),
            "…and those endpoints are the control positions, got {flat:?}",
        );

        let mut bent = Curve3D::new();
        bent.control = vec![
            CurvePoint3 {
                position: Vector3::new(0.0, 0.0, 0.0),
                in_handle: Vector3::new(0.0, 0.0, 0.0),
                out_handle: Vector3::new(0.0, 10.0, 0.0),
            },
            CurvePoint3 {
                position: Vector3::new(10.0, 0.0, 0.0),
                in_handle: Vector3::new(0.0, 10.0, 0.0),
                out_handle: Vector3::new(0.0, 0.0, 0.0),
            },
        ];
        let curved = bent.tessellate(5, 4.0);
        assert!(
            curved.len() > 10,
            "a strongly bent segment must be subdivided; got {} points",
            curved.len(),
        );
        assert!(
            curved.len() > flat.len(),
            "the bend must get more points than the straight run of the same \
             span ({} vs {})",
            curved.len(),
            flat.len(),
        );
        // Endpoints are still the control positions, whatever the depth.
        assert!(
            near(curved[0], Vector3::new(0.0, 0.0, 0.0))
                && near(curved[curved.len() - 1], Vector3::new(10.0, 0.0, 0.0)),
            "tessellate must start and end on the control points, got {:?} .. {:?}",
            curved[0],
            curved[curved.len() - 1],
        );
        // A coarser tolerance means fewer recorded corners.
        assert!(
            bent.tessellate(5, 40.0).len() < curved.len(),
            "a looser tolerance must record fewer midpoints ({} vs {})",
            bent.tessellate(5, 40.0).len(),
            curved.len(),
        );
    }

    /// An empty curve has nothing to evaluate, matching upstream's guards.
    #[test]
    fn curve3d_sample_family_is_safe_on_an_empty_curve() {
        let c = Curve3D::new();
        assert!(near(c.sample(0, 0.5), Vector3::new(0.0, 0.0, 0.0)));
        assert!(c.tessellate(5, 4.0).is_empty());
    }

    #[test]
    fn curve3d_sample_midpoint() {
        let mut c = Curve3D::new();
        c.add_point(Vector3::new(0.0, 0.0, 0.0));
        c.add_point(Vector3::new(10.0, 0.0, 0.0));
        assert!((c.baked_length() - 10.0).abs() < 1e-4);
        let p = c.sample_baked(5.0);
        assert!((p.x - 5.0).abs() < 1e-3);
    }

    #[test]
    fn path_follow_loops() {
        let mut c = Curve3D::new();
        c.add_point(Vector3::new(0.0, 0.0, 0.0));
        c.add_point(Vector3::new(4.0, 0.0, 0.0));
        let mut f = PathFollow3D::default();
        f.advance(&c, 5.0); // past end with loop
        assert!(f.progress < 4.0 + 1e-3);
        let p = f.position_on(&c);
        assert!(p.x >= 0.0 && p.x <= 4.0);
    }

    #[test]
    fn closest_point() {
        let mut c = Curve3D::new();
        c.add_point(Vector3::new(0.0, 0.0, 0.0));
        c.add_point(Vector3::new(10.0, 0.0, 0.0));
        let p = c.get_closest_point(Vector3::new(5.0, 2.0, 0.0));
        assert!((p.x - 5.0).abs() < 0.5);
    }

    #[test]
    fn curve3d_tangent_points_along_x() {
        let mut c = Curve3D::new();
        c.add_point(Vector3::new(0.0, 0.0, 0.0));
        c.add_point(Vector3::new(10.0, 0.0, 0.0));
        let t = c.sample_tangent(5.0);
        assert!(
            (t.x - 1.0).abs() < 1e-3,
            "tangent.x should be ~1, got {}",
            t.x
        );
        assert!(t.y.abs() < 1e-3 && t.z.abs() < 1e-3);
    }

    #[test]
    fn pathfollow2d_advances_and_loops() {
        use gdcore::math::Vector2;
        let mut c = Curve2D::default();
        c.add_point(Vector2::new(0.0, 0.0));
        c.add_point(Vector2::new(0.0, 4.0));
        let mut f = PathFollow2D::default();
        f.advance(&c, 5.0); // past end -> loops back into [0, 4]
        assert!(f.progress < 4.0 + 1e-3);
        let p = f.position_on(&c);
        assert!(p.y >= 0.0 && p.y <= 4.0);
    }

    /// 🔑 A ZERO-LENGTH BAKED SEGMENT MUST BE SKIPPED, NOT RETURNED FROM.
    ///
    /// The walk used to read `if o <= seg || seg < 1e-8` — which avoided the
    /// divide-by-zero but STOPPED at the first degenerate segment, so every
    /// offset past it collapsed onto that point. A PathFollow advanced normally
    /// and then stuck. Coincident control points bake to duplicate baked points,
    /// so it is reachable from ordinary authoring.
    ///
    /// ⚠️ Asserted on ALL THREE Curve3D samplers together, because they are
    /// index-aligned: fixing only `sample_baked` would advance the position past
    /// the duplicate while tilt and up vector still truncated at it, so the three
    /// would describe different points on the curve. (Bug found by MagentaBrook.)
    #[test]
    fn a_duplicate_baked_point_does_not_truncate_the_curve() {
        let mut curve = Curve3D::new();
        // A, B, B, C along +X — the duplicate B is the degenerate segment.
        for x in [0.0, 1.0, 1.0, 3.0] {
            curve.add_point(Vector3::new(x, 0.0, 0.0));
        }
        curve.tilts = vec![0.0, 1.0, 1.0, 3.0];
        curve.up_vectors = vec![Vector3::UP; 4];

        assert!(
            (curve.baked_length() - 3.0).abs() < 1e-4,
            "the duplicate contributes no arc length"
        );

        // Past the duplicate: 1.5 of 3.0 is a quarter along the B->C segment.
        let p = curve.sample_baked(1.5);
        assert!(
            (p.x - 1.5).abs() < 1e-4,
            "position must advance past the duplicate, not stick at 1.0; got {}",
            p.x
        );
        // The tilt array is index-aligned with the points, so it must land on the
        // same segment: lerp(tilts[2]=1, tilts[3]=3, 0.25) = 1.5.
        let tilt = curve.sample_baked_tilt(1.5);
        assert!(
            (tilt - 1.5).abs() < 1e-4,
            "tilt must come from the same segment as the position; got {tilt}"
        );
        // And the up vector must still be sampled rather than truncated.
        let up = curve.sample_baked_up_vector(1.5);
        assert!(
            (up - Vector3::UP).length() < 1e-4,
            "up vector must stay valid past the duplicate; got {up:?}"
        );

        // The far end still resolves, and the near side is unaffected.
        assert!((curve.sample_baked(3.0).x - 3.0).abs() < 1e-4);
        assert!((curve.sample_baked(0.5).x - 0.5).abs() < 1e-4);
    }

    /// The same truncation existed in `Curve2D`, so PathFollow2D had it too.
    ///
    /// ⚠️ Filed and fixed together deliberately: treating this as a 3D-only
    /// defect would have left the 2D half sticking in exactly the same way.
    #[test]
    fn a_duplicate_baked_point_does_not_truncate_a_curve2d() {
        use gdcore::math::Vector2;
        let mut curve = Curve2D::default();
        for x in [0.0, 1.0, 1.0, 3.0] {
            curve.add_point(Vector2::new(x, 0.0));
        }
        let p = curve.sample_baked(1.5);
        assert!(
            (p.x - 1.5).abs() < 1e-4,
            "Curve2D must advance past the duplicate too; got {}",
            p.x
        );
    }
}
