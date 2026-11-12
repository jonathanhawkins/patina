//! The canvas signed-distance field Godot builds from `sdf_collision` occluders
//! (pat-s3rrv.250).
//!
//! Godot rasterizes every `LightOccluder2D` whose `sdf_collision` is set into a
//! distance texture sized by the viewport's `sdf_oversize` and `sdf_scale`.
//! `GPUParticles2D` collision samples it, and canvas_item shaders reach it through
//! `texture_sdf` / `texture_sdf_normal` / `sdf_to_screen_uv` / `screen_uv_to_sdf`.
//! Patina carried `sdf_oversize`/`sdf_scale` on `CanvasQuality` with an explicit
//! "not yet consumed" comment, and nothing produced a field at all — so 2D particle
//! collision against level geometry was impossible.
//!
//! # Why the distance is exact rather than jump-flooded
//!
//! Godot runs a jump-flood pass on the GPU, which is fast and **approximate**.
//! This is a CPU path, where the exact distance to the nearest polygon edge is
//! simply a min over segments — O(texels x edges) instead of O(texels log n), but
//! with no error term to reason about. For the canvas sizes this serves that trade
//! is worth it, and an exact field means a test can assert a distance rather than a
//! tolerance band.
//!
//! ⚠️ The sign convention is Godot's: **negative inside** an occluder, positive
//! outside, zero on the boundary. A particle is colliding when its sample is `<= 0`.

use gdcore::math::Vector2;

/// Maps `Viewport.sdf_oversize` (an enum INDEX) to the multiplier it names.
///
/// ⚠️ The property is not a factor. Godot's `SDFOversize` is
/// `100/120/150/200 percent` at indices `0..=3`, and its **default is 1 — i.e.
/// 120%, not 1.0x**. Passing the raw integer through as a multiplier yields a
/// 100% field that looks entirely plausible and is silently too small, so every
/// caller must come through here.
pub fn sdf_oversize_factor(index: i64) -> f32 {
    match index {
        0 => 1.0,
        2 => 1.5,
        3 => 2.0,
        // 1 is Godot's default; an out-of-range value falls back to it rather than
        // to 100%, so a malformed .tscn still gets the shipped behaviour.
        _ => 1.2,
    }
}

/// Maps `Viewport.sdf_scale` (an enum INDEX) to texels per canvas unit.
///
/// ⚠️ Same trap as [`sdf_oversize_factor`]: `SDFScale` is `100/50/25 percent` at
/// indices `0..=2` and **defaults to 1, i.e. 50%**.
pub fn sdf_scale_factor(index: i64) -> f32 {
    match index {
        0 => 1.0,
        2 => 0.25,
        _ => 0.5,
    }
}

/// The canvas SDF for one frame.
///
/// Stores distances in **canvas units**, not texels, so a consumer never has to
/// know the scale to interpret a sample. `sdf_scale` only decides how finely the
/// field is sampled.
#[derive(Debug, Clone, PartialEq)]
pub struct CanvasSdf {
    width: usize,
    height: usize,
    /// Signed distance per texel, row-major. Negative inside an occluder.
    data: Vec<f32>,
    /// Canvas-space top-left of the covered rect (after oversize).
    origin: Vector2,
    /// Canvas-space extent of the covered rect (after oversize).
    size: Vector2,
    /// The frame rect the field was built for, before oversize — the space
    /// `screen_uv` addresses.
    frame: Vector2,
}

impl CanvasSdf {
    /// Builds the field from every occluder polygon passed in.
    ///
    /// `oversize` follows Godot's `Viewport.sdf_oversize` as a multiplier (1.0 =
    /// exactly the frame, 2.0 = double), and `scale` its `sdf_scale` as texels per
    /// canvas unit. Occluders are `(points, closed)` pairs; **open polylines still
    /// contribute distance** but never enclose, so they cannot make a sample
    /// negative — which is what `closed` means physically.
    pub fn generate(
        frame: Vector2,
        oversize: f32,
        scale: f32,
        occluders: &[(Vec<Vector2>, bool)],
    ) -> Self {
        // ⚠️ Guard the inputs rather than trusting them: `sdf_scale` arrives from a
        // .tscn and a zero or negative value would give a zero-sized grid, which
        // reads downstream as "the whole canvas is empty" rather than as an error.
        let oversize = if oversize.is_finite() && oversize >= 1.0 {
            oversize
        } else {
            1.0
        };
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };

        let size = Vector2::new(frame.x * oversize, frame.y * oversize);
        // Centred on the frame, so the extra margin is shared on both sides — an
        // occluder just off the left edge still has field around it.
        let origin = Vector2::new((frame.x - size.x) * 0.5, (frame.y - size.y) * 0.5);

        let width = ((size.x * scale).ceil() as usize).max(1);
        let height = ((size.y * scale).ceil() as usize).max(1);

        let mut data = vec![f32::MAX; width * height];
        for (ty, row) in data.chunks_mut(width).enumerate() {
            for (tx, cell) in row.iter_mut().enumerate() {
                // Texel CENTRE, not corner: a corner-sampled field is biased half a
                // texel toward the origin and puts the zero-crossing in the wrong
                // place by exactly that much.
                let p = Vector2::new(
                    origin.x + (tx as f32 + 0.5) / scale,
                    origin.y + (ty as f32 + 0.5) / scale,
                );
                *cell = signed_distance(p, occluders);
            }
        }

        Self {
            width,
            height,
            data,
            origin,
            size,
            frame,
        }
    }

    /// An empty field — no occluders, so every sample is "far outside".
    ///
    /// Used when a frame has nothing marked `sdf_collision`, so consumers do not
    /// need an `Option` and particle collision simply never triggers.
    pub fn empty(frame: Vector2) -> Self {
        Self::generate(frame, 1.0, 1.0, &[])
    }

    /// Width of the field in texels.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Height of the field in texels.
    pub fn height(&self) -> usize {
        self.height
    }

    /// Canvas-space rect the field covers, as `(origin, size)`.
    pub fn rect(&self) -> (Vector2, Vector2) {
        (self.origin, self.size)
    }

    /// Whether any occluder contributed — a field with no geometry samples as
    /// `f32::MAX` everywhere.
    pub fn is_empty(&self) -> bool {
        self.data.iter().all(|d| *d == f32::MAX)
    }

    /// `texture_sdf(sdf_uv)` — the signed distance at a canvas-space point, in
    /// canvas units. Negative inside an occluder.
    ///
    /// Bilinear across texels so the field is continuous: a nearest-texel lookup
    /// stair-steps, and a particle integrating against a stepped field jitters at
    /// the boundary instead of resting on it.
    pub fn sample(&self, canvas_point: Vector2) -> f32 {
        if self.data.is_empty() {
            return f32::MAX;
        }
        // Texel-space coordinates of the sample, offset by the half-texel the
        // centres are stored at.
        let fx = (canvas_point.x - self.origin.x) / self.texel_size().x - 0.5;
        let fy = (canvas_point.y - self.origin.y) / self.texel_size().y - 0.5;

        let x0 = fx.floor();
        let y0 = fy.floor();
        let tx = fx - x0;
        let ty = fy - y0;
        let (x0, y0) = (x0 as isize, y0 as isize);

        let d00 = self.texel(x0, y0);
        let d10 = self.texel(x0 + 1, y0);
        let d01 = self.texel(x0, y0 + 1);
        let d11 = self.texel(x0 + 1, y0 + 1);

        let top = d00 + (d10 - d00) * tx;
        let bottom = d01 + (d11 - d01) * tx;
        top + (bottom - top) * ty
    }

    /// `texture_sdf_normal(sdf_uv)` — the outward gradient at a point.
    ///
    /// This is the direction a colliding particle must be pushed. Central
    /// differences one texel apart; the result is normalised, and a zero gradient
    /// (a perfectly flat region, or an empty field) yields `(0, 0)` rather than a
    /// NaN from dividing by zero.
    pub fn normal_at(&self, canvas_point: Vector2) -> Vector2 {
        let t = self.texel_size();
        let dx = self.sample(Vector2::new(canvas_point.x + t.x, canvas_point.y))
            - self.sample(Vector2::new(canvas_point.x - t.x, canvas_point.y));
        let dy = self.sample(Vector2::new(canvas_point.x, canvas_point.y + t.y))
            - self.sample(Vector2::new(canvas_point.x, canvas_point.y - t.y));
        let len = (dx * dx + dy * dy).sqrt();
        if len <= f32::EPSILON || !len.is_finite() {
            return Vector2::new(0.0, 0.0);
        }
        Vector2::new(dx / len, dy / len)
    }

    /// `screen_uv_to_sdf(uv)` — screen UV in `[0,1]` over the frame, to a canvas
    /// point inside the (possibly oversized) SDF rect.
    pub fn screen_uv_to_sdf(&self, uv: Vector2) -> Vector2 {
        Vector2::new(uv.x * self.frame.x, uv.y * self.frame.y)
    }

    /// `sdf_to_screen_uv(p)` — the inverse of [`Self::screen_uv_to_sdf`].
    ///
    /// ⚠️ A point in the oversize margin maps OUTSIDE `[0,1]`, which is correct and
    /// deliberate: the margin exists precisely to hold geometry that is off-screen,
    /// and clamping here would silently pull it back onto the edge.
    pub fn sdf_to_screen_uv(&self, canvas_point: Vector2) -> Vector2 {
        let x = if self.frame.x != 0.0 {
            canvas_point.x / self.frame.x
        } else {
            0.0
        };
        let y = if self.frame.y != 0.0 {
            canvas_point.y / self.frame.y
        } else {
            0.0
        };
        Vector2::new(x, y)
    }

    /// Canvas-space size of one texel.
    fn texel_size(&self) -> Vector2 {
        Vector2::new(
            self.size.x / self.width as f32,
            self.size.y / self.height as f32,
        )
    }

    /// A texel with edge clamping, so sampling at the border does not wrap.
    fn texel(&self, x: isize, y: isize) -> f32 {
        let x = x.clamp(0, self.width as isize - 1) as usize;
        let y = y.clamp(0, self.height as isize - 1) as usize;
        self.data[y * self.width + x]
    }
}

/// Signed distance from `p` to the nearest occluder edge.
///
/// Positive outside, negative inside any CLOSED occluder, `f32::MAX` when there is
/// no geometry at all.
fn signed_distance(p: Vector2, occluders: &[(Vec<Vector2>, bool)]) -> f32 {
    let mut best = f32::MAX;
    let mut inside = false;
    for (points, closed) in occluders {
        if points.len() < 2 {
            continue;
        }
        let d = distance_to_polyline(p, points, *closed);
        if d < best {
            best = d;
        }
        // ⚠️ Only a CLOSED occluder has an interior. An open polyline is a wall
        // with no inside, so it must never flip the sign — otherwise a stray
        // unclosed occluder would mark half the canvas as solid.
        if *closed && point_in_polygon(p, points) {
            inside = true;
        }
    }
    if best == f32::MAX {
        return f32::MAX;
    }
    if inside {
        -best
    } else {
        best
    }
}

/// Distance from `p` to the nearest segment of a polyline.
fn distance_to_polyline(p: Vector2, points: &[Vector2], closed: bool) -> f32 {
    let mut best = f32::MAX;
    let n = points.len();
    let last = if closed { n } else { n - 1 };
    for i in 0..last {
        let a = points[i];
        let b = points[(i + 1) % n];
        let d = distance_to_segment(p, a, b);
        if d < best {
            best = d;
        }
    }
    best
}

/// Distance from `p` to segment `ab`.
fn distance_to_segment(p: Vector2, a: Vector2, b: Vector2) -> f32 {
    let abx = b.x - a.x;
    let aby = b.y - a.y;
    let len_sq = abx * abx + aby * aby;
    // A degenerate segment is a point; without this guard `t` divides by zero and
    // the whole field becomes NaN from one duplicated vertex.
    if len_sq <= f32::EPSILON {
        return ((p.x - a.x).powi(2) + (p.y - a.y).powi(2)).sqrt();
    }
    let t = (((p.x - a.x) * abx + (p.y - a.y) * aby) / len_sq).clamp(0.0, 1.0);
    let cx = a.x + abx * t;
    let cy = a.y + aby * t;
    ((p.x - cx).powi(2) + (p.y - cy).powi(2)).sqrt()
}

/// Even-odd containment test.
fn point_in_polygon(p: Vector2, points: &[Vector2]) -> bool {
    let mut inside = false;
    let n = points.len();
    let mut j = n - 1;
    for i in 0..n {
        let (pi, pj) = (points[i], points[j]);
        // The `!=` on the y-comparisons is the standard half-open rule: it counts a
        // vertex exactly once when the ray passes through it, where `<=` on both
        // sides would count it twice and report a point outside its own polygon.
        if (pi.y > p.y) != (pj.y > p.y) {
            let t = (p.y - pi.y) / (pj.y - pi.y);
            if p.x < pi.x + t * (pj.x - pi.x) {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}
