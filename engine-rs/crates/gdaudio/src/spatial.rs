//! AudioStreamPlayer3D — 3D positional audio with spatial attenuation.
//!
//! Provides [`AudioStreamPlayer3D`] which calculates volume attenuation and
//! stereo panning based on the distance and angle between the audio source
//! and a listener position.

use gdcore::math::{Vector2, Vector3};

/// Attenuation model for distance-based volume falloff.
///
/// Maps to Godot's `AudioStreamPlayer3D.AttenuationModel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AttenuationModel {
    /// Volume decreases proportionally with distance (Godot default).
    #[default]
    InverseDistance,
    /// Volume decreases proportionally with the square of distance.
    InverseSquareDistance,
    /// Volume decreases logarithmically with distance.
    Logarithmic,
    /// No distance attenuation — volume is constant regardless of distance.
    Disabled,
}

impl AttenuationModel {
    /// Converts from the Godot integer representation.
    pub fn from_godot_int(v: i64) -> Self {
        match v {
            1 => Self::InverseSquareDistance,
            2 => Self::Logarithmic,
            3 => Self::Disabled,
            _ => Self::InverseDistance,
        }
    }

    /// Converts to the Godot integer representation.
    pub fn to_godot_int(self) -> i64 {
        match self {
            Self::InverseDistance => 0,
            Self::InverseSquareDistance => 1,
            Self::Logarithmic => 2,
            Self::Disabled => 3,
        }
    }
}

/// Doppler tracking mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DopplerTracking {
    /// Doppler tracking disabled (default).
    #[default]
    Disabled,
    /// Track Doppler based on idle step.
    IdleStep,
    /// Track Doppler based on physics step.
    PhysicsStep,
}

/// A 3D positional audio source with spatial attenuation and stereo panning.
///
/// Maps to Godot's `AudioStreamPlayer3D`. Calculates volume gain and
/// left/right pan based on distance and angle to a listener.
#[derive(Debug, Clone)]
pub struct AudioStreamPlayer3D {
    /// World-space position of the audio source.
    pub position: Vector3,
    /// Volume in decibels (0 dB = unity).
    pub volume_db: f32,
    /// Unit size — distance at which volume is 0 dB (before attenuation).
    /// Godot default: 10.0.
    pub unit_size: f32,
    /// Maximum distance beyond which audio is silent. Godot default: 0 (unlimited).
    pub max_distance: f32,
    /// Maximum volume in dB (clamp to prevent clipping at close range).
    pub max_db: f32,
    /// Attenuation model for distance falloff.
    pub attenuation_model: AttenuationModel,
    /// Attenuation filter cutoff in Hz. 0 = disabled.
    pub attenuation_filter_cutoff_hz: f32,
    /// Attenuation filter dB reduction at max distance.
    pub attenuation_filter_db: f32,
    /// Bus name for routing.
    pub bus: String,
    /// Whether the player is currently playing.
    pub playing: bool,
    /// Whether the player is set to autoplay.
    pub autoplay: bool,
    /// Pitch scale multiplier.
    pub pitch_scale: f32,
    /// Doppler tracking mode.
    pub doppler_tracking: DopplerTracking,
    /// Emission angle in degrees (0 = omnidirectional, >0 = directional cone).
    pub emission_angle_degrees: f32,
    /// Emission angle filter attenuation in dB.
    pub emission_angle_filter_attenuation_db: f32,
    /// Whether emission angle filtering is enabled.
    pub emission_angle_enabled: bool,
    /// Panning strength (0 = center, 1 = full stereo pan). Godot default: 1.0.
    pub panning_strength: f32,
}

impl Default for AudioStreamPlayer3D {
    fn default() -> Self {
        Self {
            position: Vector3::ZERO,
            volume_db: 0.0,
            unit_size: 10.0,
            max_distance: 0.0,
            max_db: 3.0,
            attenuation_model: AttenuationModel::InverseDistance,
            attenuation_filter_cutoff_hz: 5000.0,
            attenuation_filter_db: -24.0,
            bus: "Master".to_string(),
            playing: false,
            autoplay: false,
            pitch_scale: 1.0,
            doppler_tracking: DopplerTracking::Disabled,
            emission_angle_degrees: 0.0,
            emission_angle_filter_attenuation_db: -12.0,
            emission_angle_enabled: false,
            panning_strength: 1.0,
        }
    }
}

/// Listener state for spatial audio calculations.
#[derive(Debug, Clone, Copy)]
pub struct AudioListener3D {
    /// World-space position of the listener.
    pub position: Vector3,
    /// Forward direction the listener is facing (normalized).
    pub forward: Vector3,
    /// Right direction of the listener (normalized).
    pub right: Vector3,
}

impl Default for AudioListener3D {
    fn default() -> Self {
        Self {
            position: Vector3::ZERO,
            forward: Vector3::new(0.0, 0.0, -1.0),
            right: Vector3::new(1.0, 0.0, 0.0),
        }
    }
}

/// Result of a spatial audio calculation.
#[derive(Debug, Clone, Copy)]
pub struct SpatialAudioResult {
    /// Linear volume gain [0.0, ...] after distance attenuation.
    pub volume_linear: f32,
    /// Left channel gain [0.0, 1.0].
    pub pan_left: f32,
    /// Right channel gain [0.0, 1.0].
    pub pan_right: f32,
}

impl AudioStreamPlayer3D {
    /// Creates a new 3D audio player at the given position.
    pub fn new(position: Vector3) -> Self {
        Self {
            position,
            ..Default::default()
        }
    }

    /// Computes the distance attenuation factor for a given distance.
    ///
    /// Returns a linear gain in [0.0, 1.0].
    pub fn compute_attenuation(&self, distance: f32) -> f32 {
        if distance <= 0.0 {
            return 1.0;
        }

        // If max_distance is set and exceeded, return 0.
        if self.max_distance > 0.0 && distance > self.max_distance {
            return 0.0;
        }

        let unit = self.unit_size.max(0.001);

        match self.attenuation_model {
            AttenuationModel::InverseDistance => {
                // gain = unit_size / (unit_size + distance)
                unit / (unit + distance)
            }
            AttenuationModel::InverseSquareDistance => {
                // gain = unit_size^2 / (unit_size^2 + distance^2)
                let u2 = unit * unit;
                u2 / (u2 + distance * distance)
            }
            AttenuationModel::Logarithmic => {
                // gain = 1 / (1 + log2(1 + distance / unit_size))
                let ratio = 1.0 + distance / unit;
                1.0 / (1.0 + ratio.log2())
            }
            AttenuationModel::Disabled => 1.0,
        }
    }

    /// Computes full spatial audio parameters relative to a listener.
    ///
    /// Returns volume gain (with distance attenuation) and stereo panning.
    pub fn compute_spatial(&self, listener: &AudioListener3D) -> SpatialAudioResult {
        let to_source = self.position - listener.position;
        let distance = to_source.length();

        // Volume: base dB → linear, then apply distance attenuation.
        let base_linear = db_to_linear(self.volume_db);
        let attenuation = self.compute_attenuation(distance);
        let max_linear = db_to_linear(self.max_db);
        let volume_linear = (base_linear * attenuation).min(max_linear);

        // Stereo panning based on angle to listener's right vector.
        let (pan_left, pan_right) = if distance < 1e-6 || self.panning_strength <= 0.0 {
            // Source is at listener position or panning disabled → center.
            (1.0, 1.0)
        } else {
            let dir = to_source * (1.0 / distance);
            let dot_right = dir.dot(listener.right);
            // dot_right: -1 = full left, +1 = full right, 0 = center.
            let pan = dot_right * self.panning_strength;
            // Equal-power panning.
            let angle = (pan + 1.0) * 0.5 * std::f32::consts::FRAC_PI_2;
            (angle.cos(), angle.sin())
        };

        SpatialAudioResult {
            volume_linear,
            pan_left,
            pan_right,
        }
    }
}

/// Converts decibels to linear gain.
fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

/// Builds equal-power stereo pan gains from a pan value in `[-1, 1]`
/// (`-1` = full left, `0` = center, `+1` = full right). Returns
/// `(pan_left, pan_right)` each in `[0, 1]` with constant power.
fn equal_power_pan(pan: f32) -> (f32, f32) {
    let pan = pan.clamp(-1.0, 1.0);
    let angle = (pan + 1.0) * 0.5 * std::f32::consts::FRAC_PI_2;
    (angle.cos(), angle.sin())
}

/// A 2D positional audio source (Godot's `AudioStreamPlayer2D`).
///
/// Panning follows the source's horizontal screen position relative to the
/// listener (a `Camera2D`/screen center); volume attenuates with distance up to
/// `max_distance`. Unlike the 3D player there is no elevation/forward axis.
#[derive(Debug, Clone, Copy)]
pub struct AudioStreamPlayer2D {
    /// Canvas-space position of the source.
    pub position: Vector2,
    /// Base volume in decibels.
    pub volume_db: f32,
    /// Distance (in the same units as `position`) beyond which the source is
    /// silent. `0.0` means unlimited (no distance attenuation).
    pub max_distance: f32,
    /// Attenuation exponent applied to the normalized distance falloff.
    pub attenuation: f32,
    /// Panning strength (`0` = no pan, `1` = full stereo). Godot default `1.0`.
    pub panning_strength: f32,
}

impl Default for AudioStreamPlayer2D {
    fn default() -> Self {
        Self {
            position: Vector2::ZERO,
            volume_db: 0.0,
            max_distance: 2000.0,
            attenuation: 1.0,
            panning_strength: 1.0,
        }
    }
}

impl AudioStreamPlayer2D {
    /// Creates a 2D audio source at `position` with Godot defaults.
    pub fn new(position: Vector2) -> Self {
        Self {
            position,
            ..Default::default()
        }
    }

    /// Computes spatial volume + equal-power pan relative to a listener.
    ///
    /// `screen_half_width` is half the viewport width (in `position` units); a
    /// source that far to the right of the listener pans fully right. Volume
    /// falls off with distance across `max_distance`.
    pub fn compute_spatial_2d(
        &self,
        listener: Vector2,
        screen_half_width: f32,
    ) -> SpatialAudioResult {
        let offset = self.position - listener;
        let distance = offset.length();

        // Distance attenuation: 1.0 at the listener, 0.0 at/after max_distance.
        let base_linear = db_to_linear(self.volume_db);
        let atten = if self.max_distance <= 0.0 {
            1.0
        } else if distance >= self.max_distance {
            0.0
        } else {
            (1.0 - distance / self.max_distance).powf(self.attenuation.max(0.0))
        };
        let volume_linear = base_linear * atten;

        // Pan from horizontal screen offset, scaled by panning_strength.
        let pan = if screen_half_width <= 1e-6 || self.panning_strength <= 0.0 {
            0.0
        } else {
            (offset.x / screen_half_width) * self.panning_strength
        };
        let (pan_left, pan_right) = equal_power_pan(pan);

        SpatialAudioResult {
            volume_linear,
            pan_left,
            pan_right,
        }
    }
}

/// Computes a Doppler pitch-scale multiplier for a moving source and listener.
///
/// Uses the textbook Doppler relation `f' = f * (c + v_o) / (c - v_s)`, where
/// `c` is the speed of sound, `v_s` is the source's velocity component toward
/// the listener, and `v_o` is the listener's velocity component toward the
/// source. Approaching raises the pitch (`> 1.0`), receding lowers it. The
/// result is clamped to `[0.25, 4.0]` to stay stable near/above the sound
/// barrier. Returns `1.0` when coincident or `c <= 0`.
pub fn doppler_pitch_scale(
    source_pos: Vector3,
    source_velocity: Vector3,
    listener_pos: Vector3,
    listener_velocity: Vector3,
    speed_of_sound: f32,
) -> f32 {
    let to_listener = listener_pos - source_pos;
    let distance = to_listener.length();
    if distance < 1e-6 || speed_of_sound <= 0.0 {
        return 1.0;
    }
    let dir = to_listener * (1.0 / distance); // unit source -> listener
    let v_s = source_velocity.dot(dir); // source moving toward listener (+)
    let v_o = -listener_velocity.dot(dir); // listener moving toward source (+)
    let denom = speed_of_sound - v_s;
    if denom <= 1e-6 {
        return 4.0; // source at/above sound speed toward listener
    }
    ((speed_of_sound + v_o) / denom).clamp(0.25, 4.0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn default_listener() -> AudioListener3D {
        AudioListener3D::default()
    }

    // -- AttenuationModel --

    #[test]
    fn attenuation_model_roundtrip() {
        for (int_val, expected) in [
            (0, AttenuationModel::InverseDistance),
            (1, AttenuationModel::InverseSquareDistance),
            (2, AttenuationModel::Logarithmic),
            (3, AttenuationModel::Disabled),
        ] {
            let m = AttenuationModel::from_godot_int(int_val);
            assert_eq!(m, expected);
            assert_eq!(m.to_godot_int(), int_val);
        }
    }

    #[test]
    fn attenuation_model_unknown_defaults_to_inverse() {
        assert_eq!(
            AttenuationModel::from_godot_int(99),
            AttenuationModel::InverseDistance
        );
    }

    // -- Attenuation calculation --

    #[test]
    fn attenuation_zero_distance_is_unity() {
        let player = AudioStreamPlayer3D::default();
        assert!((player.compute_attenuation(0.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn attenuation_inverse_distance_at_unit_size() {
        let player = AudioStreamPlayer3D {
            unit_size: 10.0,
            ..Default::default()
        };
        // At distance = unit_size: gain = 10/(10+10) = 0.5
        let gain = player.compute_attenuation(10.0);
        assert!((gain - 0.5).abs() < 1e-5, "got {gain}");
    }

    #[test]
    fn attenuation_inverse_square_at_unit_size() {
        let player = AudioStreamPlayer3D {
            unit_size: 10.0,
            attenuation_model: AttenuationModel::InverseSquareDistance,
            ..Default::default()
        };
        // At distance = unit_size: gain = 100/(100+100) = 0.5
        let gain = player.compute_attenuation(10.0);
        assert!((gain - 0.5).abs() < 1e-5, "got {gain}");
    }

    #[test]
    fn attenuation_logarithmic_at_unit_size() {
        let player = AudioStreamPlayer3D {
            unit_size: 10.0,
            attenuation_model: AttenuationModel::Logarithmic,
            ..Default::default()
        };
        // At distance = unit_size: gain = 1/(1+log2(2)) = 1/2 = 0.5
        let gain = player.compute_attenuation(10.0);
        assert!((gain - 0.5).abs() < 1e-5, "got {gain}");
    }

    #[test]
    fn attenuation_disabled_always_unity() {
        let player = AudioStreamPlayer3D {
            attenuation_model: AttenuationModel::Disabled,
            ..Default::default()
        };
        assert!((player.compute_attenuation(100.0) - 1.0).abs() < 1e-6);
        assert!((player.compute_attenuation(1000.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn attenuation_beyond_max_distance_is_zero() {
        let player = AudioStreamPlayer3D {
            max_distance: 50.0,
            ..Default::default()
        };
        assert!((player.compute_attenuation(51.0)).abs() < 1e-6);
    }

    #[test]
    fn attenuation_increases_with_distance() {
        let player = AudioStreamPlayer3D::default();
        let g1 = player.compute_attenuation(5.0);
        let g2 = player.compute_attenuation(20.0);
        let g3 = player.compute_attenuation(100.0);
        assert!(g1 > g2, "closer should be louder");
        assert!(g2 > g3, "farther should be quieter");
    }

    // -- Spatial computation --

    #[test]
    fn spatial_at_listener_position_is_centered() {
        let player = AudioStreamPlayer3D::new(Vector3::ZERO);
        let listener = default_listener();
        let result = player.compute_spatial(&listener);
        assert!((result.pan_left - 1.0).abs() < 1e-5);
        assert!((result.pan_right - 1.0).abs() < 1e-5);
    }

    #[test]
    fn spatial_source_to_right_pans_right() {
        let player = AudioStreamPlayer3D::new(Vector3::new(10.0, 0.0, 0.0));
        let listener = default_listener();
        let result = player.compute_spatial(&listener);
        assert!(
            result.pan_right > result.pan_left,
            "source to the right should pan right, L={} R={}",
            result.pan_left,
            result.pan_right
        );
    }

    #[test]
    fn spatial_source_to_left_pans_left() {
        let player = AudioStreamPlayer3D::new(Vector3::new(-10.0, 0.0, 0.0));
        let listener = default_listener();
        let result = player.compute_spatial(&listener);
        assert!(
            result.pan_left > result.pan_right,
            "source to the left should pan left, L={} R={}",
            result.pan_left,
            result.pan_right
        );
    }

    #[test]
    fn spatial_source_directly_ahead_is_centered() {
        // Listener faces -Z, source is directly ahead at (0,0,-10)
        let player = AudioStreamPlayer3D::new(Vector3::new(0.0, 0.0, -10.0));
        let listener = default_listener();
        let result = player.compute_spatial(&listener);
        assert!(
            (result.pan_left - result.pan_right).abs() < 0.01,
            "ahead should be centered, L={} R={}",
            result.pan_left,
            result.pan_right
        );
    }

    #[test]
    fn spatial_volume_decreases_with_distance() {
        let listener = default_listener();
        let near = AudioStreamPlayer3D::new(Vector3::new(0.0, 0.0, -5.0));
        let far = AudioStreamPlayer3D::new(Vector3::new(0.0, 0.0, -50.0));
        let r_near = near.compute_spatial(&listener);
        let r_far = far.compute_spatial(&listener);
        assert!(
            r_near.volume_linear > r_far.volume_linear,
            "near={} should be louder than far={}",
            r_near.volume_linear,
            r_far.volume_linear
        );
    }

    #[test]
    fn spatial_volume_clamped_by_max_db() {
        let player = AudioStreamPlayer3D {
            position: Vector3::new(0.0, 0.0, -0.001),
            volume_db: 20.0, // very loud
            max_db: 3.0,
            ..Default::default()
        };
        let listener = default_listener();
        let result = player.compute_spatial(&listener);
        let max_linear = db_to_linear(3.0);
        assert!(
            result.volume_linear <= max_linear + 1e-5,
            "volume {} should be clamped to max_db linear {}",
            result.volume_linear,
            max_linear
        );
    }

    #[test]
    fn spatial_zero_panning_strength_is_centered() {
        let player = AudioStreamPlayer3D {
            position: Vector3::new(100.0, 0.0, 0.0),
            panning_strength: 0.0,
            ..Default::default()
        };
        let listener = default_listener();
        let result = player.compute_spatial(&listener);
        assert!((result.pan_left - 1.0).abs() < 1e-5);
        assert!((result.pan_right - 1.0).abs() < 1e-5);
    }

    // -- Defaults --

    #[test]
    fn player_defaults() {
        let p = AudioStreamPlayer3D::default();
        assert_eq!(p.volume_db, 0.0);
        assert!((p.unit_size - 10.0).abs() < 1e-5);
        assert!((p.max_distance - 0.0).abs() < 1e-5);
        assert_eq!(p.attenuation_model, AttenuationModel::InverseDistance);
        assert_eq!(p.bus, "Master");
        assert!(!p.playing);
        assert!((p.pitch_scale - 1.0).abs() < 1e-5);
        assert!((p.panning_strength - 1.0).abs() < 1e-5);
    }

    #[test]
    fn listener_defaults() {
        let l = AudioListener3D::default();
        assert_eq!(l.position, Vector3::ZERO);
        assert!((l.forward.z - (-1.0)).abs() < 1e-5);
        assert!((l.right.x - 1.0).abs() < 1e-5);
    }

    // -- db_to_linear --

    #[test]
    fn db_to_linear_zero_is_unity() {
        assert!((db_to_linear(0.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn db_to_linear_minus20_is_tenth() {
        assert!((db_to_linear(-20.0) - 0.1).abs() < 1e-5);
    }

    #[test]
    fn db_to_linear_plus6_is_about_two() {
        let val = db_to_linear(6.0);
        assert!((val - 1.995).abs() < 0.01, "got {val}");
    }

    // -- AudioStreamPlayer2D panning --

    #[test]
    fn player2d_pans_right_for_source_on_the_right() {
        let src = AudioStreamPlayer2D::new(Vector2::new(400.0, 0.0));
        let res = src.compute_spatial_2d(Vector2::ZERO, 400.0); // half-width 400
                                                                // Source at +half_width -> full right: right gain dominates.
        assert!(res.pan_right > res.pan_left, "{res:?}");
        assert!(res.pan_right > 0.98, "near full right: {res:?}");
    }

    #[test]
    fn player2d_pans_left_for_source_on_the_left() {
        let src = AudioStreamPlayer2D::new(Vector2::new(-400.0, 0.0));
        let res = src.compute_spatial_2d(Vector2::ZERO, 400.0);
        assert!(res.pan_left > res.pan_right, "{res:?}");
        assert!(res.pan_left > 0.98, "near full left: {res:?}");
    }

    #[test]
    fn player2d_center_is_equal_power() {
        let src = AudioStreamPlayer2D::new(Vector2::ZERO);
        let res = src.compute_spatial_2d(Vector2::ZERO, 400.0);
        assert!((res.pan_left - res.pan_right).abs() < 1e-5, "{res:?}");
    }

    #[test]
    fn player2d_attenuates_with_distance() {
        let mut src = AudioStreamPlayer2D::new(Vector2::new(1000.0, 0.0));
        src.max_distance = 2000.0;
        let near = AudioStreamPlayer2D::new(Vector2::new(100.0, 0.0))
            .compute_spatial_2d(Vector2::ZERO, 2000.0)
            .volume_linear;
        let far = src.compute_spatial_2d(Vector2::ZERO, 2000.0).volume_linear;
        assert!(far < near, "far {far} should attenuate below near {near}");
        // At/after max_distance -> silent.
        let edge = AudioStreamPlayer2D::new(Vector2::new(2000.0, 0.0))
            .compute_spatial_2d(Vector2::ZERO, 2000.0)
            .volume_linear;
        assert!(edge.abs() < 1e-6, "silent at max_distance: {edge}");
    }

    // -- Doppler --

    #[test]
    fn doppler_stationary_is_unity() {
        let p = doppler_pitch_scale(
            Vector3::new(10.0, 0.0, 0.0),
            Vector3::ZERO,
            Vector3::ZERO,
            Vector3::ZERO,
            343.0,
        );
        assert!((p - 1.0).abs() < 1e-5, "got {p}");
    }

    #[test]
    fn doppler_approaching_source_raises_pitch() {
        // Source at +x moving toward listener at origin (-x velocity).
        let p = doppler_pitch_scale(
            Vector3::new(10.0, 0.0, 0.0),
            Vector3::new(-34.3, 0.0, 0.0),
            Vector3::ZERO,
            Vector3::ZERO,
            343.0,
        );
        assert!(p > 1.0, "approaching should raise pitch, got {p}");
    }

    #[test]
    fn doppler_receding_source_lowers_pitch() {
        // Source at +x moving away from listener (+x velocity).
        let p = doppler_pitch_scale(
            Vector3::new(10.0, 0.0, 0.0),
            Vector3::new(34.3, 0.0, 0.0),
            Vector3::ZERO,
            Vector3::ZERO,
            343.0,
        );
        assert!(p < 1.0, "receding should lower pitch, got {p}");
    }
}
