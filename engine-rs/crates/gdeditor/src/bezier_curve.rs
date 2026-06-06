//! Bezier (value-curve) tracks for the animation editor.
//!
//! A bezier track stores keys, each with a value and an in/out tangent handle
//! (offsets in `(time, value)` from the key point). Between two keys the value
//! follows the cubic Bezier defined by the left key's point, its out handle, the
//! right key's in handle, and the right key's point. Dragging a handle reshapes
//! that interpolation. Evaluating at a time before the first key or after the
//! last clamps to the endpoint value.
//!
//! Evaluation solves for the Bezier parameter `u` whose x-coordinate (time)
//! matches the queried time via binary search — robust as long as the handles
//! keep the segment's x monotonically increasing (the editor clamps handle time
//! offsets to keep that invariant), then returns the y (value) at that `u`.

/// A single bezier key: a point plus in/out tangent handles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BezierKey {
    /// Key time (x).
    pub time: f64,
    /// Key value (y).
    pub value: f64,
    /// Incoming handle offset `(dtime, dvalue)` relative to the key point.
    pub in_handle: (f64, f64),
    /// Outgoing handle offset `(dtime, dvalue)` relative to the key point.
    pub out_handle: (f64, f64),
}

impl BezierKey {
    /// A key at `(time, value)` with zero (flat) handles.
    pub fn new(time: f64, value: f64) -> Self {
        Self {
            time,
            value,
            in_handle: (0.0, 0.0),
            out_handle: (0.0, 0.0),
        }
    }
}

/// An editable bezier value-curve: keys ordered by time.
#[derive(Debug, Clone, Default)]
pub struct BezierCurve {
    keys: Vec<BezierKey>,
}

impl BezierCurve {
    /// Creates an empty curve.
    pub fn new() -> Self {
        Self::default()
    }

    /// The keys, ordered by time.
    pub fn keys(&self) -> &[BezierKey] {
        &self.keys
    }

    /// Number of keys.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Whether the curve has no keys.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Inserts a key at `(time, value)` with flat handles, keeping keys ordered
    /// by time. Returns the index of the inserted key.
    pub fn add_key(&mut self, time: f64, value: f64) -> usize {
        let idx = self
            .keys
            .partition_point(|k| k.time < time);
        self.keys.insert(idx, BezierKey::new(time, value));
        idx
    }

    /// Sets the outgoing handle of key `i` (offset in time/value). No-op if out
    /// of range.
    pub fn set_out_handle(&mut self, i: usize, dtime: f64, dvalue: f64) {
        if let Some(k) = self.keys.get_mut(i) {
            k.out_handle = (dtime, dvalue);
        }
    }

    /// Sets the incoming handle of key `i` (offset in time/value). No-op if out
    /// of range.
    pub fn set_in_handle(&mut self, i: usize, dtime: f64, dvalue: f64) {
        if let Some(k) = self.keys.get_mut(i) {
            k.in_handle = (dtime, dvalue);
        }
    }

    /// Evaluates the curve at `time`, following the bezier segments between
    /// keys. Clamps to the endpoint value before the first / after the last key.
    /// Returns `0.0` for an empty curve.
    pub fn evaluate(&self, time: f64) -> f64 {
        match self.keys.len() {
            0 => 0.0,
            1 => self.keys[0].value,
            _ => {
                let first = &self.keys[0];
                let last = &self.keys[self.keys.len() - 1];
                if time <= first.time {
                    return first.value;
                }
                if time >= last.time {
                    return last.value;
                }
                // Find the segment [i, i+1] containing `time`.
                let i = self.keys.partition_point(|k| k.time <= time) - 1;
                let a = &self.keys[i];
                let b = &self.keys[i + 1];
                eval_segment(a, b, time)
            }
        }
    }
}

/// Cubic Bezier control points for the segment between `a` and `b`.
fn segment_points(a: &BezierKey, b: &BezierKey) -> [(f64, f64); 4] {
    let p0 = (a.time, a.value);
    let c0 = (a.time + a.out_handle.0, a.value + a.out_handle.1);
    let c1 = (b.time + b.in_handle.0, b.value + b.in_handle.1);
    let p3 = (b.time, b.value);
    [p0, c0, c1, p3]
}

/// Cubic Bezier scalar at parameter `u` for control values `(p0,p1,p2,p3)`.
fn cubic(p0: f64, p1: f64, p2: f64, p3: f64, u: f64) -> f64 {
    let mu = 1.0 - u;
    mu * mu * mu * p0 + 3.0 * mu * mu * u * p1 + 3.0 * mu * u * u * p2 + u * u * u * p3
}

/// Evaluates the bezier segment between `a` and `b` at the given `time` by
/// solving x(u) = time via binary search, then returning y(u).
fn eval_segment(a: &BezierKey, b: &BezierKey, time: f64) -> f64 {
    let [p0, c0, c1, p3] = segment_points(a, b);
    // Binary search for u in [0, 1] where x(u) == time (x assumed monotonic).
    let mut lo = 0.0_f64;
    let mut hi = 1.0_f64;
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        let x = cubic(p0.0, c0.0, c1.0, p3.0, mid);
        if x < time {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let u = 0.5 * (lo + hi);
    cubic(p0.1, c0.1, c1.1, p3.1, u)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-e0qjh): a bezier track evaluates a curve between keys,
    /// dragging in/out handles reshapes the interpolation, and the value follows
    /// the curve (clamping outside the key range).
    #[test]
    fn anim_bezier_editing() {
        let mut curve = BezierCurve::new();
        curve.add_key(0.0, 0.0);
        curve.add_key(1.0, 1.0);
        assert_eq!(curve.len(), 2);

        // With flat handles and symmetric endpoints, x(u) and y(u) are the same
        // function of u, so the curve evaluates to the identity time->value.
        assert!((curve.evaluate(0.0) - 0.0).abs() < 1e-9);
        assert!((curve.evaluate(1.0) - 1.0).abs() < 1e-9);
        assert!((curve.evaluate(0.5) - 0.5).abs() < 1e-6);
        assert!((curve.evaluate(0.25) - 0.25).abs() < 1e-6);

        // Times outside the key range clamp to the endpoint values.
        assert_eq!(curve.evaluate(-1.0), 0.0);
        assert_eq!(curve.evaluate(2.0), 1.0);

        // Dragging key 0's out handle reshapes the interpolation: the mid value
        // changes away from the identity baseline.
        let mid_before = curve.evaluate(0.5);
        curve.set_out_handle(0, 0.3, 0.5);
        let mid_after = curve.evaluate(0.5);
        assert!((mid_after - mid_before).abs() > 1e-3);
        // The curve still passes through its keys exactly.
        assert!((curve.evaluate(0.0) - 0.0).abs() < 1e-9);
        assert!((curve.evaluate(1.0) - 1.0).abs() < 1e-9);
    }

    /// add_key keeps keys ordered by time; a single key evaluates to its value.
    #[test]
    fn bezier_add_key_ordering_and_single() {
        let mut curve = BezierCurve::new();
        curve.add_key(1.0, 10.0);
        curve.add_key(0.0, 0.0); // inserted before
        curve.add_key(0.5, 5.0); // inserted between
        let times: Vec<f64> = curve.keys().iter().map(|k| k.time).collect();
        assert_eq!(times, vec![0.0, 0.5, 1.0]);

        let mut single = BezierCurve::new();
        single.add_key(2.0, 42.0);
        assert_eq!(single.evaluate(0.0), 42.0);
        assert_eq!(single.evaluate(5.0), 42.0);

        // An empty curve evaluates to 0.0.
        assert_eq!(BezierCurve::new().evaluate(1.0), 0.0);
    }
}
