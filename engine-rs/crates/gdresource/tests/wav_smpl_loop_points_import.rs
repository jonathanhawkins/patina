//! pat-s3rrv.47 — a WAV's `smpl` loop chunk reaches the imported
//! `AudioStreamWAV` resource.
//!
//! Godot's `ResourceImporterWAV` reads the RIFF `smpl` chunk and carries its
//! first loop onto the imported stream as `loop_mode` / `loop_begin` /
//! `loop_end` (`editor/import/resource_importer_wav.cpp`). Patina's `import_wav`
//! read `fmt `+`data` and nothing else, so a looping sample authored in any
//! tracker or sampler imported as a one-shot: the loop points were sitting in
//! the file and no code ever looked at them.
//!
//! ⚠️ This lives in `gdresource`'s own test tree, NOT alongside the bead's
//! scene-level acceptance test in `gdscene`, deliberately: `gdresource` does not
//! depend on `gdscene`, so this half of the bead stays verifiable while that
//! crate is broken by unrelated in-flight work. The WAV bytes are hand-rolled
//! for the same reason — `gdresource` has no `gdaudio` dependency to borrow a
//! builder from.

use gdvariant::Variant;

/// The smallest RIFF/WAVE that `import_wav` accepts: `fmt ` + `data`.
fn wav_bytes(extra_chunks: &[u8]) -> Vec<u8> {
    let sample_rate: u32 = 44_100;
    let channels: u16 = 1;
    let bits: u16 = 16;
    let pcm = [0u8; 512];

    let mut body = Vec::new();
    body.extend_from_slice(b"WAVE");

    body.extend_from_slice(b"fmt ");
    body.extend_from_slice(&16u32.to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes()); // PCM
    body.extend_from_slice(&channels.to_le_bytes());
    body.extend_from_slice(&sample_rate.to_le_bytes());
    let byte_rate = sample_rate * channels as u32 * (bits / 8) as u32;
    body.extend_from_slice(&byte_rate.to_le_bytes());
    body.extend_from_slice(&(channels * (bits / 8)).to_le_bytes());
    body.extend_from_slice(&bits.to_le_bytes());

    body.extend_from_slice(b"data");
    body.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    body.extend_from_slice(&pcm);

    body.extend_from_slice(extra_chunks);

    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

/// A `smpl` chunk declaring exactly one loop.
///
/// Layout per the RIFF sampler spec: 36 bytes of header (manufacturer, product,
/// sample period, MIDI unity note, MIDI pitch fraction, SMPTE format, SMPTE
/// offset, loop count, sampler-data size) then 24 bytes per loop.
fn smpl_chunk(loop_type: u32, begin: u32, end: u32) -> Vec<u8> {
    let mut c = Vec::new();
    c.extend_from_slice(b"smpl");
    c.extend_from_slice(&60u32.to_le_bytes());
    for _ in 0..7 {
        c.extend_from_slice(&0u32.to_le_bytes());
    }
    c.extend_from_slice(&1u32.to_le_bytes()); // one loop
    c.extend_from_slice(&0u32.to_le_bytes()); // sampler data
    c.extend_from_slice(&0u32.to_le_bytes()); // cue point id
    c.extend_from_slice(&loop_type.to_le_bytes());
    c.extend_from_slice(&begin.to_le_bytes());
    c.extend_from_slice(&end.to_le_bytes());
    c.extend_from_slice(&0u32.to_le_bytes()); // fraction
    c.extend_from_slice(&0u32.to_le_bytes()); // play count
    c
}

/// A `smpl` chunk that carries MIDI data but declares ZERO loops — common in
/// real files, and not a malformed one.
fn smpl_chunk_no_loops() -> Vec<u8> {
    let mut c = Vec::new();
    c.extend_from_slice(b"smpl");
    c.extend_from_slice(&60u32.to_le_bytes());
    for _ in 0..7 {
        c.extend_from_slice(&0u32.to_le_bytes());
    }
    c.extend_from_slice(&0u32.to_le_bytes()); // ZERO loops
    c.extend_from_slice(&0u32.to_le_bytes());
    c.extend_from_slice(&[0u8; 24]); // trailing space, never read
    c
}

fn import(bytes: Vec<u8>, name: &str) -> std::sync::Arc<gdresource::resource::Resource> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    gdresource::importers::import_wav(&path).expect("wav imports")
}

/// **The acceptance for the WAV half.** The loop points land on the resource,
/// and the loop TYPE is translated into Godot's numbering rather than copied.
#[test]
fn wav_smpl_loop_points_import() {
    let res = import(wav_bytes(&smpl_chunk(0, 128, 4096)), "forward.wav");

    // ⚠️ The two numberings differ and overlap, which is what makes a raw copy
    // dangerous: WAV 0 = forward / 1 = alternating / 2 = backward, Godot
    // 0 = DISABLED / 1 = FORWARD / 2 = PINGPONG / 3 = BACKWARD. Copying the
    // value across turns a forward loop into a DISABLED one — and a disabled
    // loop is indistinguishable from the old unlooped behaviour, so the bug
    // would look exactly like "the feature was never implemented".
    assert_eq!(
        res.get_property("loop_mode"),
        Some(&Variant::Int(1)),
        "WAV loop type 0 (forward) must import as Godot LOOP_FORWARD (1)",
    );
    assert_eq!(res.get_property("loop_begin"), Some(&Variant::Int(128)));
    assert_eq!(res.get_property("loop_end"), Some(&Variant::Int(4096)));
}

/// Each WAV loop type maps to its own distinct Godot mode — the table is a
/// translation, not an accidental identity.
#[test]
fn every_wav_loop_type_maps_to_its_godot_mode() {
    for (wav_type, godot_mode, label) in [
        (0u32, 1i64, "forward"),
        (1, 2, "alternating/pingpong"),
        (2, 3, "backward"),
    ] {
        let res = import(wav_bytes(&smpl_chunk(wav_type, 0, 64)), "loop.wav");
        assert_eq!(
            res.get_property("loop_mode"),
            Some(&Variant::Int(godot_mode)),
            "WAV loop type {wav_type} ({label}) must import as Godot mode {godot_mode}",
        );
    }
}

/// No `smpl` chunk at all: `LOOP_DISABLED`, so every WAV that imported
/// correctly before this change still does.
#[test]
fn wav_without_smpl_chunk_stays_unlooped() {
    let res = import(wav_bytes(&[]), "plain.wav");
    assert_eq!(
        res.get_property("loop_mode"),
        Some(&Variant::Int(0)),
        "a WAV with no smpl chunk must import as LOOP_DISABLED",
    );
    assert_eq!(res.get_property("loop_begin"), Some(&Variant::Int(0)));
    assert_eq!(res.get_property("loop_end"), Some(&Variant::Int(0)));
}

/// A `smpl` chunk declaring zero loops is NOT a loop. Reading the first loop
/// struct unconditionally would pull 24 bytes of unrelated trailing data in as
/// loop points.
#[test]
fn smpl_chunk_with_zero_loops_is_not_a_loop() {
    let res = import(wav_bytes(&smpl_chunk_no_loops()), "noloops.wav");
    assert_eq!(
        res.get_property("loop_mode"),
        Some(&Variant::Int(0)),
        "a smpl chunk declaring 0 loops must stay LOOP_DISABLED",
    );
}

/// The fields the importer already produced are untouched — this change adds
/// three properties, it does not reshape the resource.
#[test]
fn existing_wav_properties_are_unchanged() {
    let res = import(wav_bytes(&smpl_chunk(0, 8, 16)), "meta.wav");
    assert_eq!(res.get_property("sample_rate"), Some(&Variant::Int(44100)));
    assert_eq!(res.get_property("channels"), Some(&Variant::Int(1)));
    assert_eq!(res.get_property("bit_depth"), Some(&Variant::Int(16)));
}
