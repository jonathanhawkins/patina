//! pat-s3rrv.54 — the `AudioEffectFilter` family reaches a bus and ClassDB.
//!
//! The biquad DSP has been written and unit-tested all along
//! (`gdaudio::effects::FilterMode` covers LowPass/HighPass/BandPass/Notch/
//! Peak/LowShelf/HighShelf). What was missing was every route TO it:
//!
//! * the bus-layout loader mapped only LowPass/HighPass/BandPass and returned
//!   `None` for the other five, and `None` means "skip this slot" — so a
//!   project whose Master bus ended in a notch filter booted with that insert
//!   silently gone;
//! * ClassDB registered nine `AudioEffect*` names and not one filter class, and
//!   the editor's Add-Effect list is `class_db::get_class_list()`, so none of
//!   the eight could be added to a bus from the UI.
//!
//! Godot ground truth: `AudioEffectFilter : AudioEffect` carries `cutoff_hz`
//! (2000), `resonance` (1.0), `gain` (1.0) and the `FilterDB` slope enum; the
//! seven concrete leaves inherit it and add **no members of their own**
//! (extension_api.json, doc/classes/AudioEffectFilter.xml).

use gdaudio::effects::AudioEffect;
use gdaudio::mixer::AudioMixer;
use gdobject::class_db::{class_exists, get_class_info};
use gdobject::register_2d_classes;

const SR: u32 = 48_000;

/// Every one of the eight filter classes on a single bus, in a known order.
///
/// `db = 1` on the low-pass is `DB_12`, i.e. two cascaded stages — the slope is
/// an enum INDEX, not a dB number.
const LAYOUT: &str = r#"[gd_resource type="AudioBusLayout" format=3]

[sub_resource type="AudioEffectLowPassFilter" id="LP_1"]
cutoff_hz = 500.0
db = 1

[sub_resource type="AudioEffectHighPassFilter" id="HP_1"]
cutoff_hz = 4000.0

[sub_resource type="AudioEffectBandPassFilter" id="BP_1"]
cutoff_hz = 1000.0

[sub_resource type="AudioEffectBandLimitFilter" id="BL_1"]
cutoff_hz = 1500.0

[sub_resource type="AudioEffectNotchFilter" id="NO_1"]
cutoff_hz = 2000.0

[sub_resource type="AudioEffectLowShelfFilter" id="LS_1"]
cutoff_hz = 300.0
gain = 2.0

[sub_resource type="AudioEffectHighShelfFilter" id="HS_1"]
cutoff_hz = 6000.0
gain = 0.5

[sub_resource type="AudioEffectFilter" id="BASE_1"]
cutoff_hz = 800.0

[resource]
bus/0/name = "Master"
bus/0/volume_db = 0.0
bus/1/name = "Filters"
bus/1/volume_db = 0.0
bus/1/send = "Master"
bus/1/effect/0/effect = SubResource("LP_1")
bus/1/effect/0/enabled = true
bus/1/effect/1/effect = SubResource("HP_1")
bus/1/effect/1/enabled = true
bus/1/effect/2/effect = SubResource("BP_1")
bus/1/effect/2/enabled = true
bus/1/effect/3/effect = SubResource("BL_1")
bus/1/effect/3/enabled = true
bus/1/effect/4/effect = SubResource("NO_1")
bus/1/effect/4/enabled = true
bus/1/effect/5/effect = SubResource("LS_1")
bus/1/effect/5/enabled = true
bus/1/effect/6/effect = SubResource("HS_1")
bus/1/effect/6/enabled = true
bus/1/effect/7/effect = SubResource("BASE_1")
bus/1/effect/7/enabled = true
"#;

/// A mono sine at `hz`, one second's worth of `frames`.
fn sine(hz: f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| (std::f32::consts::TAU * hz * i as f32 / SR as f32).sin())
        .collect()
}

/// RMS over the second half only — the first half is filter settling, and
/// including it would let a filter that never converges still look correct.
fn settled_rms(samples: &[f32]) -> f32 {
    let tail = &samples[samples.len() / 2..];
    (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt()
}

/// Runs a tone through one freshly built effect and reports its settled RMS.
fn rms_through(effect_type: &str, cutoff: f32, tone_hz: f32) -> f32 {
    let layout = format!(
        r#"[gd_resource type="AudioBusLayout" format=3]

[sub_resource type="{effect_type}" id="E_1"]
cutoff_hz = {cutoff}

[resource]
bus/0/name = "Master"
bus/0/volume_db = 0.0
bus/1/name = "Test"
bus/1/volume_db = 0.0
bus/1/send = "Master"
bus/1/effect/0/effect = SubResource("E_1")
bus/1/effect/0/enabled = true
"#
    );
    let mut mixer = AudioMixer::new();
    mixer.load_bus_layout_tres(&layout);
    let bus_index = mixer.get_bus_by_name("Test").expect("Test bus");
    let mut samples = sine(tone_hz, SR as usize / 4);
    let bus = mixer.get_bus_mut(bus_index).expect("Test bus");
    bus.effects_mut().process(&mut samples, SR, 1);
    settled_rms(&samples)
}

/// **The acceptance test.**
#[test]
fn audio_effect_filter_family_registered_and_wired() {
    // -- (1) ClassDB: all eight exist under Godot's inheritance --------------
    //
    // ⚠️ ClassDB is NOT populated implicitly in tests. The AudioEffect family
    // lives in `register_2d_classes` (an existing quirk — Amplify/Reverb/EQ are
    // registered there rather than in `register_audio_classes`), so that is the
    // registrar to call. Without it every `class_exists` below answers false
    // and the test would fail for the wrong reason.
    register_2d_classes();

    assert!(
        class_exists("AudioEffectFilter"),
        "the base filter class must be registered",
    );
    let base = get_class_info("AudioEffectFilter").expect("AudioEffectFilter info");
    assert_eq!(
        base.parent_class, "AudioEffect",
        "AudioEffectFilter : AudioEffect",
    );

    for leaf in [
        "AudioEffectLowPassFilter",
        "AudioEffectHighPassFilter",
        "AudioEffectBandPassFilter",
        "AudioEffectBandLimitFilter",
        "AudioEffectNotchFilter",
        "AudioEffectLowShelfFilter",
        "AudioEffectHighShelfFilter",
    ] {
        assert!(class_exists(leaf), "{leaf} must be registered");
        let info = get_class_info(leaf).unwrap_or_else(|| panic!("{leaf} info"));
        assert_eq!(
            info.parent_class, "AudioEffectFilter",
            "{leaf} inherits AudioEffectFilter, not AudioEffect directly — the \
             editor groups the family by that parent",
        );
    }

    // -- (2) the bus loader builds a slot for every one of the eight ---------
    let mut mixer = AudioMixer::new();
    let buses = mixer.load_bus_layout_tres(LAYOUT);
    assert_eq!(buses, 2, "the layout declares two buses");

    let filters = mixer.get_bus_by_name("Filters").expect("Filters bus");
    let chain = mixer.get_bus(filters).expect("Filters bus").effects();
    assert_eq!(
        chain.len(),
        8,
        "every filter class must produce a slot — the loader used to return \
         None for five of them, and None means the insert is silently dropped",
    );

    // Per-slot identity. `name()` is "Filter" for all seven Godot classes, so
    // this asserts through `godot_class()`; asserting on `name()` would pass
    // even if the loader built eight identical low-passes.
    let classes: Vec<&str> = (0..chain.len())
        .map(|i| chain.effect_at(i).expect("slot").godot_class())
        .collect();
    assert_eq!(
        classes,
        vec![
            "AudioEffectLowPassFilter",
            "AudioEffectHighPassFilter",
            "AudioEffectBandPassFilter",
            "AudioEffectBandLimitFilter",
            "AudioEffectNotchFilter",
            "AudioEffectLowShelfFilter",
            "AudioEffectHighShelfFilter",
            "AudioEffectFilter",
        ],
        "each slot must carry the class the layout named, in order",
    );

    // -- (3) the DSP actually filters, in the right direction ---------------
    //
    // A low-pass at 500 Hz must pass 100 Hz and attenuate 10 kHz; the high-pass
    // at 500 Hz must do the inverse. Ratios rather than absolute levels, so
    // this does not encode the biquad's exact gain.
    let lp_low = rms_through("AudioEffectLowPassFilter", 500.0, 100.0);
    let lp_high = rms_through("AudioEffectLowPassFilter", 500.0, 10_000.0);
    assert!(
        lp_low > lp_high * 10.0,
        "a 500 Hz low-pass must pass 100 Hz far more than 10 kHz; got \
         {lp_low} vs {lp_high}",
    );

    let hp_low = rms_through("AudioEffectHighPassFilter", 500.0, 100.0);
    let hp_high = rms_through("AudioEffectHighPassFilter", 500.0, 10_000.0);
    assert!(
        hp_high > hp_low * 10.0,
        "a 500 Hz high-pass must do the inverse; got {hp_low} vs {hp_high}",
    );

    // The five formerly-dropped classes are load-bearing too: a notch at
    // 2 kHz must cut a 2 kHz tone well below what it does to 100 Hz.
    let notch_at = rms_through("AudioEffectNotchFilter", 2000.0, 2000.0);
    let notch_off = rms_through("AudioEffectNotchFilter", 2000.0, 100.0);
    assert!(
        notch_off > notch_at * 2.0,
        "a 2 kHz notch must attenuate 2 kHz relative to 100 Hz; got \
         {notch_at} at the notch vs {notch_off} away from it",
    );
}

/// The `db` slope selects a cascade depth (pat-s3rrv.54).
///
/// `FilterDB` is an enum INDEX — `DB_24` is 3, not 24 — and one biquad is
/// 12 dB/oct, so Godot steepens the skirt by cascading identical stages.
/// Reading the field as a dB number would build 24 stages instead of 4.
#[test]
fn the_db_field_selects_a_cascade_depth_not_a_decibel_count() {
    // Same cutoff and tone; only the slope differs. A steeper skirt must
    // attenuate a well-past-cutoff tone more.
    let shallow = rms_through_with_db("AudioEffectLowPassFilter", 500.0, 4000.0, 0);
    let steep = rms_through_with_db("AudioEffectLowPassFilter", 500.0, 4000.0, 3);
    assert!(
        steep < shallow,
        "DB_24 must attenuate more than DB_6 at the same cutoff; got \
         steep={steep} shallow={shallow}",
    );
}

/// [`rms_through`] with an explicit `db` slope index.
fn rms_through_with_db(effect_type: &str, cutoff: f32, tone_hz: f32, db: i32) -> f32 {
    let layout = format!(
        r#"[gd_resource type="AudioBusLayout" format=3]

[sub_resource type="{effect_type}" id="E_1"]
cutoff_hz = {cutoff}
db = {db}

[resource]
bus/0/name = "Master"
bus/0/volume_db = 0.0
bus/1/name = "Test"
bus/1/volume_db = 0.0
bus/1/send = "Master"
bus/1/effect/0/effect = SubResource("E_1")
bus/1/effect/0/enabled = true
"#
    );
    let mut mixer = AudioMixer::new();
    mixer.load_bus_layout_tres(&layout);
    let bus_index = mixer.get_bus_by_name("Test").expect("Test bus");
    let mut samples = sine(tone_hz, SR as usize / 4);
    let bus = mixer.get_bus_mut(bus_index).expect("Test bus");
    bus.effects_mut().process(&mut samples, SR, 1);
    settled_rms(&samples)
}
