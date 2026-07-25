//! pat-0q2kx.57 — `AudioStreamPlayer2D`/`3D` are driven through the audio
//! bridge with listener-relative pan and distance attenuation.
//!
//! The spatial math already existed and was unit-tested inside `gdaudio`
//! (`spatial.rs`), and the mixer already consumed `set_pan`/`set_linear_gain`.
//! What was missing was any call path between them: `AudioServerBridge` scanned
//! for the class name `AudioStreamPlayer` *exactly*, so a positional player was
//! never collected and produced no sound at all — however correct its
//! properties, and however well-tested the math.
//!
//! These tests drive a real scene through `MainLoop::step` and assert on the
//! mixed stereo output, so a pass means the whole chain is connected: tree →
//! global transform → listener resolution → spatial calculation → voice → mix.
//! Nothing here calls the spatial API directly.

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// One second of full-scale stereo DC, so channel energy is a direct readout of
/// the pan/attenuation gains applied to the voice.
fn dc_buffer() -> gdaudio::AudioBuffer {
    gdaudio::AudioBuffer {
        samples: vec![1.0; 44100 * 2],
        sample_rate: 44100,
        channels: 2,
    }
}

/// Sums |L| and |R| over the interleaved mix.
fn channel_energy(mix: &[f32]) -> (f32, f32) {
    let mut left = 0.0;
    let mut right = 0.0;
    for frame in mix.chunks_exact(2) {
        left += frame[0].abs();
        right += frame[1].abs();
    }
    (left, right)
}

/// Peak absolute sample in the mix, as a fraction of unity.
fn peak(mix: &[f32]) -> f32 {
    mix.iter().fold(0.0_f32, |acc, s| acc.max(s.abs()))
}

/// Runs the loop a few frames and returns the last mixed block.
fn run(ml: &mut MainLoop, frames: usize) -> Vec<f32> {
    for _ in 0..frames {
        ml.step(1.0 / 60.0);
    }
    ml.audio_bridge().last_mix().to_vec()
}

/// Builds a tree with a current `Camera3D` at the origin and an autoplaying
/// `AudioStreamPlayer3D` at `pos`.
fn scene_3d(pos: (f32, f32, f32), extra: &[(&str, Variant)]) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut cam = Node::new("Camera", "Camera3D");
    cam.set_property("current", Variant::Bool(true));
    tree.add_child(root, cam).unwrap();

    let mut sfx = Node::new("Sfx", "AudioStreamPlayer3D");
    sfx.set_property("autoplay", Variant::Bool(true));
    sfx.set_property(
        "position",
        Variant::Vector3(gdcore::math::Vector3::new(pos.0, pos.1, pos.2)),
    );
    for (key, value) in extra {
        sfx.set_property(key, value.clone());
    }
    let sfx_id = tree.add_child(root, sfx).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.audio_bridge_mut().set_stream_buffer(sfx_id, dc_buffer());
    (ml, sfx_id)
}

/// The headline: a 3D source to the right of the listener is heard on the
/// right, quieter than unity because it is 30 units away.
#[test]
fn audio_spatial_players_wiring() {
    let (mut ml, sfx) = scene_3d((30.0, 0.0, 0.0), &[]);
    let mix = run(&mut ml, 3);

    assert!(
        !mix.is_empty(),
        "an autoplaying AudioStreamPlayer3D must reach the mixer at all — \
         before this bead the bridge never collected it"
    );
    let (left, right) = channel_energy(&mix);
    assert!(
        right > left,
        "a source 30 units to the right must be louder on the right: L{left} R{right}"
    );
    assert!(
        peak(&mix) < 0.5,
        "a source 30 units away must attenuate well below unity, got peak {}",
        peak(&mix)
    );

    // Moving the source to the other side flips the pan, proving the pan is
    // recomputed from the live transform every frame rather than latched once.
    gdscene::node3d::set_position(
        ml.tree_mut(),
        sfx,
        gdcore::math::Vector3::new(-30.0, 0.0, 0.0),
    );
    let mix = run(&mut ml, 3);
    let (left, right) = channel_energy(&mix);
    assert!(
        left > right,
        "after moving the source to -X the left channel must lead: L{left} R{right}"
    );
}

/// A 2D source at the right screen edge pans right, and one past `max_distance`
/// is silent.
#[test]
fn audio_stream_player_2d_pans_and_attenuates() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut cam = Node::new("Camera", "Camera2D");
    cam.set_property("current", Variant::Bool(true));
    tree.add_child(root, cam).unwrap();

    let mut sfx = Node::new("Sfx", "AudioStreamPlayer2D");
    sfx.set_property("autoplay", Variant::Bool(true));
    sfx.set_property("max_distance", Variant::Float(2000.0));
    sfx.set_property(
        "position",
        Variant::Vector2(gdcore::math::Vector2::new(500.0, 0.0)),
    );
    let sfx_id = tree.add_child(root, sfx).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.audio_bridge_mut().set_stream_buffer(sfx_id, dc_buffer());

    let mix = run(&mut ml, 3);
    let (left, right) = channel_energy(&mix);
    assert!(
        right > left,
        "a 2D source right of the listener must pan right: L{left} R{right}"
    );

    // Beyond max_distance Godot silences the source entirely.
    gdscene::node2d::set_position(
        ml.tree_mut(),
        sfx_id,
        gdcore::math::Vector2::new(5000.0, 0.0),
    );
    let mix = run(&mut ml, 3);
    assert!(
        peak(&mix) < 1e-3,
        "past max_distance the 2D source must be silent, got peak {}",
        peak(&mix)
    );
}

/// The listener is the *current* `AudioListener3D` when one exists, overriding
/// the camera — Godot resolves the listener before falling back to the camera.
#[test]
fn audio_listener_3d_overrides_the_camera() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Camera at the origin: from here the source at +X would pan right.
    let mut cam = Node::new("Camera", "Camera3D");
    cam.set_property("current", Variant::Bool(true));
    tree.add_child(root, cam).unwrap();

    // A current listener further right than the source flips that.
    let mut listener = Node::new("Ears", "AudioListener3D");
    listener.set_property("current", Variant::Bool(true));
    listener.set_property(
        "position",
        Variant::Vector3(gdcore::math::Vector3::new(60.0, 0.0, 0.0)),
    );
    tree.add_child(root, listener).unwrap();

    let mut sfx = Node::new("Sfx", "AudioStreamPlayer3D");
    sfx.set_property("autoplay", Variant::Bool(true));
    sfx.set_property(
        "position",
        Variant::Vector3(gdcore::math::Vector3::new(30.0, 0.0, 0.0)),
    );
    let sfx_id = tree.add_child(root, sfx).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.audio_bridge_mut().set_stream_buffer(sfx_id, dc_buffer());

    let mix = run(&mut ml, 3);
    let (left, right) = channel_energy(&mix);
    assert!(
        left > right,
        "with a current AudioListener3D at +60 the source at +30 is to its LEFT, \
         so the camera at the origin must not be what we panned against: L{left} R{right}"
    );
}

/// `attenuation_model = DISABLED` (Godot enum 3) keeps the source at full
/// volume regardless of distance — proof the model property is mirrored, not
/// just the position.
#[test]
fn attenuation_model_is_mirrored_from_the_node() {
    let (mut ml, _) = scene_3d((30.0, 0.0, 0.0), &[("attenuation_model", Variant::Int(3))]);
    let disabled_peak = peak(&run(&mut ml, 3));

    let (mut ml, _) = scene_3d((30.0, 0.0, 0.0), &[("attenuation_model", Variant::Int(0))]);
    let inverse_peak = peak(&run(&mut ml, 3));

    assert!(
        disabled_peak > inverse_peak,
        "DISABLED attenuation must be louder at 30 units than INVERSE_DISTANCE: \
         {disabled_peak} vs {inverse_peak}"
    );
}
