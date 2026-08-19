//! pat-s3rrv.55 — 3D attenuation matches Godot's formulas, and the emission
//! cone is no longer a dead struct field.
//!
//! Patina implemented three plausible falloff curves that were **not Godot's**:
//! `unit/(unit+d)`, `u²/(u²+d²)` and `1/(1+log2(1+d/unit))`. They agree with
//! Godot at `d = 0` and diverge everywhere else, so every 3D source in every
//! project mixed at the wrong level with nothing looking broken — and the unit
//! tests pinned the wrong values, which is why it survived.
//!
//! Godot ground truth (`upstream/godot/scene/3d/audio_stream_player_3d.cpp`):
//!
//! * `_get_attenuation_db` :232-259 — `linear_to_db(1/(d/unit + eps))`,
//!   `linear_to_db(1/((d/unit)² + eps))`, `-20·ln(d/unit + eps)`, `0`;
//! * `_update_panning` :444-465 — a `max(0, 1 - d/max_distance)` multiplier
//!   (a linear fade, NOT the hard cut Patina used), a high-shelf cut that grows
//!   with distance, and the emission-cone attenuation.

use gdaudio::spatial::{AttenuationModel, AudioListener3D, AudioStreamPlayer3D};
use gdcore::math::Vector3;

/// Godot's `linear_to_db`.
fn linear_to_db(x: f32) -> f32 {
    20.0 * x.log10()
}

/// Godot's `db_to_linear`.
fn db_to_linear(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

fn source(model: AttenuationModel, unit: f32) -> AudioStreamPlayer3D {
    AudioStreamPlayer3D {
        unit_size: unit,
        attenuation_model: model,
        ..Default::default()
    }
}

/// **The acceptance test**: the gain matches Godot's own arithmetic, computed
/// here from the upstream formula rather than from a table of magic numbers.
#[test]
fn audio3d_attenuation_matches_godot_and_cone_attenuates() {
    const UNIT: f32 = 10.0;

    for d in [1.0f32, 5.0, 10.0, 25.0, 100.0] {
        let ratio = d / UNIT;

        let inv = source(AttenuationModel::InverseDistance, UNIT).compute_attenuation(d);
        let expect_inv = db_to_linear(linear_to_db(1.0 / ratio));
        assert!(
            (inv - expect_inv).abs() < 1e-3 * expect_inv.max(1.0),
            "inverse-distance at d={d}: got {inv}, Godot gives {expect_inv}",
        );

        let sq = source(AttenuationModel::InverseSquareDistance, UNIT).compute_attenuation(d);
        let expect_sq = db_to_linear(linear_to_db(1.0 / (ratio * ratio)));
        assert!(
            (sq - expect_sq).abs() < 1e-3 * expect_sq.max(1.0),
            "inverse-square at d={d}: got {sq}, Godot gives {expect_sq}",
        );

        let log = source(AttenuationModel::Logarithmic, UNIT).compute_attenuation(d);
        let expect_log = db_to_linear(-20.0 * ratio.ln());
        assert!(
            (log - expect_log).abs() < 1e-3 * expect_log.max(1.0),
            "logarithmic at d={d}: got {log}, Godot gives {expect_log}",
        );
    }

    // Disabled is exactly unity at any distance.
    let off = source(AttenuationModel::Disabled, UNIT);
    assert!((off.compute_attenuation(1000.0) - 1.0).abs() < 1e-6);
}

/// `max_distance` FADES rather than cutting (pat-s3rrv.55).
///
/// Patina returned 0 the instant `d > max_distance`, so a source crossing the
/// boundary popped out of the mix. Godot multiplies by
/// `max(0, 1 - d/max_distance)`, which reaches zero exactly AT the boundary
/// after fading the whole way in.
#[test]
fn max_distance_fades_linearly_instead_of_cutting() {
    let src = AudioStreamPlayer3D {
        unit_size: 10.0,
        max_distance: 100.0,
        ..Default::default()
    };
    // Halfway to max_distance the multiplier is 0.5 — the old hard cut left it
    // at full level right up to the edge, so this is the assertion that
    // separates the two behaviours.
    let expected_half = db_to_linear(linear_to_db(1.0 / 5.0)) * 0.5;
    let got = src.compute_attenuation(50.0);
    assert!(
        (got - expected_half).abs() < 1e-3,
        "at half of max_distance the rolloff must be 0.5: got {got}, want {expected_half}",
    );
    assert!(
        src.compute_attenuation(100.0).abs() < 1e-6,
        "silent AT max_distance",
    );
    assert!(src.compute_attenuation(150.0).abs() < 1e-6, "and beyond it",);
}

/// `max_db` clamps in DECIBELS, before the conversion (pat-s3rrv.55).
///
/// Godot sums attenuation + `volume_db`, clamps that at `max_db`, then converts
/// once. Clamping the linear product instead is a different operation whenever
/// the attenuation is a gain above unity — which, inside `unit_size`, it is.
#[test]
fn max_db_clamps_in_decibels() {
    let src = AudioStreamPlayer3D {
        position: Vector3::new(0.0, 0.0, -0.01), // very close → huge gain
        unit_size: 10.0,
        volume_db: 20.0,
        max_db: 3.0,
        ..Default::default()
    };
    let result = src.compute_spatial(&AudioListener3D::default());
    let ceiling = db_to_linear(3.0);
    assert!(
        result.volume_linear <= ceiling + 1e-4,
        "max_db must bound a near-field source: got {} vs {ceiling}",
        result.volume_linear,
    );
}

/// A listener outside the emission cone is attenuated; one inside is not
/// (pat-s3rrv.55).
///
/// The cone fields existed and `compute_spatial` never read them, so a
/// directional speaker authored in a scene behaved as omnidirectional.
#[test]
fn a_listener_outside_the_emission_cone_is_attenuated() {
    // Source at the origin facing -Z, with a 30-degree HALF-angle cone.
    let cone = |listener_pos: Vector3| {
        let src = AudioStreamPlayer3D {
            position: Vector3::ZERO,
            forward: Vector3::new(0.0, 0.0, -1.0),
            emission_angle_enabled: true,
            emission_angle_degrees: 30.0,
            emission_angle_filter_attenuation_db: -12.0,
            // Isolate the cone: no distance-driven filtering in the result.
            max_distance: 0.0,
            attenuation_filter_db: 0.0,
            ..Default::default()
        };
        let listener = AudioListener3D {
            position: listener_pos,
            ..Default::default()
        };
        src.compute_spatial(&listener).highshelf_db
    };

    // Straight down the source's forward axis → inside the cone.
    assert_eq!(
        cone(Vector3::new(0.0, 0.0, -10.0)),
        0.0,
        "a listener on-axis must not be cone-attenuated",
    );
    // Directly behind → 180 degrees off-axis, well outside.
    assert_eq!(
        cone(Vector3::new(0.0, 0.0, 10.0)),
        -12.0,
        "a listener behind the source must take the cone attenuation",
    );
    // 90 degrees off-axis is outside a 30-degree half-angle cone.
    assert_eq!(
        cone(Vector3::new(10.0, 0.0, 0.0)),
        -12.0,
        "90 degrees off-axis is outside a 30-degree cone",
    );

    // Disabling the cone silences the effect entirely, so the flag is real
    // rather than the angle merely never matching.
    let mut src = AudioStreamPlayer3D {
        forward: Vector3::new(0.0, 0.0, -1.0),
        emission_angle_enabled: false,
        emission_angle_degrees: 30.0,
        emission_angle_filter_attenuation_db: -12.0,
        attenuation_filter_db: 0.0,
        ..Default::default()
    };
    let behind = AudioListener3D {
        position: Vector3::new(0.0, 0.0, 10.0),
        ..Default::default()
    };
    assert_eq!(
        src.compute_spatial(&behind).highshelf_db,
        0.0,
        "with the cone disabled, direction must not matter",
    );
    src.emission_angle_enabled = true;
    assert_eq!(
        src.compute_spatial(&behind).highshelf_db,
        -12.0,
        "…and enabling it must bring the attenuation back",
    );
}

/// The distance high-shelf grows toward `max_distance` (pat-s3rrv.55).
///
/// Godot muffles a distant source as well as quietening it:
/// `db_att = (1 - min(1, multiplier)) * attenuation_filter_db`.
#[test]
fn the_distance_highshelf_grows_with_distance() {
    let src = AudioStreamPlayer3D {
        unit_size: 10.0,
        max_distance: 100.0,
        attenuation_filter_db: -24.0,
        attenuation_filter_cutoff_hz: 5000.0,
        ..Default::default()
    };
    let at = |d: f32| {
        src.compute_spatial(&AudioListener3D {
            position: Vector3::new(0.0, 0.0, -d),
            ..Default::default()
        })
    };
    // The listener moves away from a source at the origin.
    let near = at(10.0).highshelf_db;
    let far = at(90.0).highshelf_db;
    assert!(
        far < near,
        "the shelf must cut harder with distance: near={near} far={far}",
    );
    assert_eq!(
        at(10.0).highshelf_cutoff_hz,
        5000.0,
        "the cutoff must be carried from the source",
    );
}

/// The cone and filter properties are ClassDB-registered so the editor can
/// author them (pat-s3rrv.55).
#[test]
fn the_cone_and_filter_properties_are_registered() {
    // ClassDB starts EMPTY in tests. AudioStreamPlayer3D lives in
    // `register_audio_classes` — NOT `register_3d_classes`, despite the class
    // being a Node3D; calling the wrong registrar leaves the DB empty and the
    // assertions below fail for the wrong reason.
    gdobject::register_audio_classes();
    let info =
        gdobject::class_db::get_class_info("AudioStreamPlayer3D").expect("AudioStreamPlayer3D");
    let names: Vec<&str> = info.properties.iter().map(|p| p.name.as_str()).collect();
    for prop in [
        "emission_angle_enabled",
        "emission_angle_degrees",
        "emission_angle_filter_attenuation_db",
        "attenuation_filter_cutoff_hz",
        "attenuation_filter_db",
    ] {
        assert!(
            names.contains(&prop),
            "{prop} must be registered — the editor's inspector is built from \
             ClassDB, so an unregistered property cannot be authored at all. \
             Registered: {names:?}",
        );
    }
}
