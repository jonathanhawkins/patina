//! Animation **interpolation & loop modes** (pat-6ua0i).
//!
//! A value track samples a float over time from its keyframes. Each key carries
//! an interpolation mode that governs the segment leaving it — nearest (step to
//! the closest key), linear, or cubic (smoothstep) — so changing a key's mode
//! changes how values are sampled between keys. The track's loop mode decides
//! whether sampling past the end wraps back to the start or holds the last key.

/// How a key's outgoing segment is interpolated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpolation {
    /// Snap to the nearest key (step).
    Nearest,
    /// Straight-line blend between keys.
    Linear,
    /// Smoothstep (eased) blend between keys.
    Cubic,
}

/// How the animation wraps at its boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopMode {
    /// No looping — sampling past the end holds the last key.
    None,
    /// Loop — sampling past the end wraps back to the start.
    Linear,
}

/// A keyframe: a value at a time, with the interpolation leaving it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ValueKey {
    /// Time in seconds.
    pub time: f32,
    /// Keyed value.
    pub value: f32,
    /// Interpolation mode for the segment after this key.
    pub interp: Interpolation,
}

/// A float value track that samples over time.
#[derive(Debug, Clone)]
pub struct ValueTrack {
    keys: Vec<ValueKey>,
    length: f32,
    loop_mode: LoopMode,
}

impl ValueTrack {
    /// Creates an empty track of `length` seconds, not looping.
    pub fn new(length: f32) -> Self {
        Self {
            keys: Vec::new(),
            length: length.max(0.0),
            loop_mode: LoopMode::None,
        }
    }

    /// Adds a key at `time` with `value` and interpolation `interp`, keeping
    /// keys sorted by time.
    pub fn add_key(&mut self, time: f32, value: f32, interp: Interpolation) {
        self.keys.push(ValueKey { time, value, interp });
        self.keys
            .sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap_or(std::cmp::Ordering::Equal));
    }

    /// The number of keys.
    pub fn key_count(&self) -> usize {
        self.keys.len()
    }

    /// Sets the interpolation mode of the key at `index`. Returns whether it
    /// exists.
    pub fn set_interp(&mut self, index: usize, interp: Interpolation) -> bool {
        match self.keys.get_mut(index) {
            Some(k) => {
                k.interp = interp;
                true
            }
            None => false,
        }
    }

    /// Sets the loop mode.
    pub fn set_loop_mode(&mut self, mode: LoopMode) {
        self.loop_mode = mode;
    }

    /// The loop mode.
    pub fn loop_mode(&self) -> LoopMode {
        self.loop_mode
    }

    /// Samples the track's value at `time`, honoring per-key interpolation and
    /// the loop mode.
    pub fn sample(&self, time: f32) -> f32 {
        if self.keys.is_empty() {
            return 0.0;
        }
        // Wrap the sample time when looping.
        let t = match self.loop_mode {
            LoopMode::Linear if self.length > 0.0 => time.rem_euclid(self.length),
            _ => time,
        };

        let last = self.keys.len() - 1;
        if t <= self.keys[0].time {
            return self.keys[0].value;
        }
        if t >= self.keys[last].time {
            return self.keys[last].value;
        }

        for i in 0..last {
            let a = self.keys[i];
            let b = self.keys[i + 1];
            if t >= a.time && t <= b.time {
                let span = b.time - a.time;
                let u = if span > 0.0 { (t - a.time) / span } else { 0.0 };
                return match a.interp {
                    Interpolation::Nearest => {
                        if u < 0.5 {
                            a.value
                        } else {
                            b.value
                        }
                    }
                    Interpolation::Linear => a.value + (b.value - a.value) * u,
                    Interpolation::Cubic => {
                        let s = u * u * (3.0 - 2.0 * u); // smoothstep
                        a.value + (b.value - a.value) * s
                    }
                };
            }
        }
        self.keys[last].value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-6
    }

    /// Acceptance (pat-6ua0i): setting a key's interpolation changes how values
    /// are sampled between keys, and toggling loop mode wraps playback at the
    /// animation boundaries.
    #[test]
    fn anim_interpolation_loop_modes() {
        // Track of length 2s with keys at t=0 (0.0) and t=1 (10.0).
        let mut track = ValueTrack::new(2.0);
        track.add_key(0.0, 0.0, Interpolation::Linear);
        track.add_key(1.0, 10.0, Interpolation::Linear);

        // Linear sampling between keys.
        assert!(approx(track.sample(0.5), 5.0));
        assert!(approx(track.sample(0.0), 0.0));
        assert!(approx(track.sample(1.0), 10.0));
        // Out of range (no loop) holds the boundary keys.
        assert!(approx(track.sample(-0.5), 0.0));
        assert!(approx(track.sample(1.5), 10.0));

        // Nearest steps to the closest key.
        assert!(track.set_interp(0, Interpolation::Nearest));
        assert!(approx(track.sample(0.4), 0.0)); // closer to start
        assert!(approx(track.sample(0.6), 10.0)); // closer to end

        // Cubic eases in: below the linear value before the midpoint, equal at it.
        assert!(track.set_interp(0, Interpolation::Cubic));
        assert!(track.sample(0.25) < 2.5); // linear would be 2.5
        assert!(approx(track.sample(0.5), 5.0));

        // Loop mode wraps sampling past the end back to the start.
        assert!(track.set_interp(0, Interpolation::Linear));
        track.set_loop_mode(LoopMode::Linear);
        assert_eq!(track.loop_mode(), LoopMode::Linear);
        assert!(approx(track.sample(2.5), 5.0)); // 2.5 wraps to 0.5 → 5.0

        // Loop off holds the last key past the end.
        track.set_loop_mode(LoopMode::None);
        assert!(approx(track.sample(2.5), 10.0));
    }
}
