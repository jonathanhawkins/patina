//! Path3D / PathFollow3D / Curve3D (pat-p0s82.15).

use gdcore::math::Vector3;

/// Cubic / linear curve sample in 3D (Godot Curve3D subset).
#[derive(Debug, Clone, Default)]
pub struct Curve3D {
    /// Control points (baked polyline for sampling).
    pub points: Vec<Vector3>,
}

impl Curve3D {
    /// Empty curve.
    pub fn new() -> Self {
        Self { points: Vec::new() }
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
            if o <= seg || seg < 1e-8 {
                let t = if seg > 1e-8 { o / seg } else { 0.0 };
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
            if o <= seg || seg < 1e-8 {
                let t = if seg > 1e-8 { o / seg } else { 0.0 };
                return Vector2::new(
                    w[0].x + (w[1].x - w[0].x) * t,
                    w[0].y + (w[1].y - w[0].y) * t,
                );
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
        assert!((t.x - 1.0).abs() < 1e-3, "tangent.x should be ~1, got {}", t.x);
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
}
