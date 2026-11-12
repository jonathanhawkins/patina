//! pat-nzgdu.188 — `OggPacketSequence`: the demuxed form Godot stores imported
//! Ogg Vorbis in, and the two directions that make it useful.
//!
//! ⚠️ **This is the first real Ogg Vorbis data in the repo.** Before this bead
//! there was not a single `.ogg` file in the tree, so `gdaudio::decode::decode_ogg`
//! had only ever been exercised by `decode_ogg_invalid_data_returns_error` — the
//! error path. The happy path was untested.
//!
//! The fixture is a 0.35 s 440 Hz stereo tone. It is generated audio — our own
//! content — encoded with the **reference Xiph libVorbis** so it uses the
//! standard 256/2048 block switching that strict decoders accept. (An earlier
//! draft used ffmpeg's *experimental* native Vorbis encoder, whose non-standard
//! 2048/2048 stream `lewton` could not decode; that is what failed verification
//! the first time.) Its structure was verified independently by parsing the Ogg
//! pages, so the constants below are facts about the file:
//!
//! | property             | value                                    |
//! |----------------------|------------------------------------------|
//! | encoder              | `Xiph.Org libVorbis I 20200704`          |
//! | blocksizes           | 256 / 2048 (standard)                    |
//! | pages                | 3                                        |
//! | packets              | 20 (1 ident + 1 comment + 1 setup + 17)  |
//! | granule per page     | `[0, 0, 15434]`                          |
//! | ident header         | `channels=2`, `sample_rate=44100`        |
//! | length               | 15434 / 44100 = 0.3500 s                 |
//!
//! ⚠️ **The decoded frame count is deliberately not pinned to an exact value.**
//! A Vorbis decode does not land on the granule exactly: the stream opens with a
//! short block whose leading half is decoder ramp-up, so `ffmpeg` decodes this
//! file to 15306 frames — 128 *below* the granule — while a raw packet decode
//! that keeps the priming block lands elsewhere again. The granule is the true
//! length; the assertions below check that the decode reaches it within one long
//! block, not that it hits a specific number that depends on decoder internals.

use gdaudio::decode::decode_ogg;
use gdaudio::ogg_packet_sequence::OggPacketSequence;

static TONE_OGG: &[u8] = include_bytes!("fixtures/test_tone.ogg");

/// The stream's true length in frames: the granule position of its final page,
/// and what `ffprobe` reports as `duration_ts`.
const FINAL_GRANULE: i64 = 15434;
const SAMPLE_RATE: u32 = 44100;
const CHANNELS: u16 = 2;
/// Vorbis's long block. A faithful decode lands within one of these of the
/// granule (ramp-up trims a little off the front; a final block can run a little
/// past the end). Wider than needed, but the point is to pin "full-length
/// stream", not a decoder-specific frame count.
const LONG_BLOCK: i64 = 2048;

/// Demuxing must reproduce the page grouping, because the granule positions
/// index against *pages*, not packets.
#[test]
fn demux_groups_packets_by_page_and_records_granules() {
    let seq = OggPacketSequence::demux(TONE_OGG).expect("fixture is a valid Ogg Vorbis stream");

    assert_eq!(seq.page_count(), 3, "the fixture has three Ogg pages");
    assert_eq!(
        seq.packet_count(),
        20,
        "1 ident + 1 comment + 1 setup + 17 audio packets"
    );
    // ⚠️ One granule per PAGE — not per packet, despite Godot naming the
    // accessors `{set,get}_packet_granule_positions`.
    assert_eq!(
        seq.granule_positions(),
        &[0, 0, FINAL_GRANULE],
        "the two header pages carry no audio, so their granule is 0"
    );
    assert_eq!(seq.granule_positions().len(), seq.page_count());

    // The header packets are laid out ident | comment+setup | audio...
    assert_eq!(seq.page_data()[0].len(), 1, "page 0 holds the ident header");
    assert_eq!(
        seq.page_data()[1].len(),
        2,
        "page 1 holds the comment and setup headers"
    );
    assert_eq!(
        seq.page_data()[2].len(),
        17,
        "page 2 holds the audio packets"
    );

    // The ident header is the very first packet, and Vorbis marks it type 1.
    let ident = seq.packets().next().expect("at least one packet");
    assert_eq!(ident[0], 1, "packet 0 is the identification header");
    assert_eq!(&ident[1..7], b"vorbis");
}

/// `demux` reads the rate out of the ident header, mirroring what Godot's
/// `maybe_update_info()` does when a sequence is attached to a stream.
#[test]
fn demux_reads_sampling_rate_from_the_ident_header() {
    let seq = OggPacketSequence::demux(TONE_OGG).expect("valid stream");

    assert_eq!(seq.sampling_rate(), SAMPLE_RATE as f32);
    assert_eq!(seq.final_granule_pos(), FINAL_GRANULE);
    // 15434 / 44100
    assert!(
        (seq.length() - 0.3500).abs() < 0.001,
        "length was {}, expected ~0.35s",
        seq.length()
    );
}

/// The point of the whole bead: a packet sequence decodes to real PCM without
/// ever seeing an Ogg container again.
#[test]
fn packet_sequence_decodes_to_non_empty_pcm() {
    let seq = OggPacketSequence::demux(TONE_OGG).expect("valid stream");
    let buf = seq.decode().expect("packet sequence decodes");

    assert_eq!(buf.sample_rate, SAMPLE_RATE);
    assert_eq!(buf.channels, CHANNELS);
    assert!(!buf.samples.is_empty(), "decoded to non-empty PCM");
    assert_eq!(
        buf.samples.len() % CHANNELS as usize,
        0,
        "interleaved stereo: an even number of samples"
    );

    // ⚠️ A band, not an exact count. The decode reaches the full ~0.35 s stream
    // (never a fragment), landing within one long block of the granule — the
    // cap keeps it from exceeding the granule, and leading ramp-up keeps it from
    // falling more than a block short. The exact frame count depends on decoder
    // internals this test deliberately does not assume.
    let frames = buf.frame_count() as i64;
    assert!(
        frames > 0 && frames <= FINAL_GRANULE,
        "decode is capped at the granule; got {frames}, granule {FINAL_GRANULE}"
    );
    assert!(
        frames >= FINAL_GRANULE - LONG_BLOCK,
        "decode must reach essentially the whole stream; got {frames} of {FINAL_GRANULE}"
    );

    // It is a 440 Hz tone, not silence.
    let peak = buf.samples.iter().fold(0.0f32, |a, s| a.max(s.abs()));
    assert!(
        peak > 0.1,
        "a 440Hz tone should be audible; peak was {peak}"
    );
    assert!(
        buf.samples.iter().all(|s| s.is_finite() && s.abs() <= 1.0),
        "samples stay normalized to [-1, 1]"
    );
}

/// The packet path and the container path must agree about the stream they are
/// both describing.
///
/// ⚠️ Deliberately *not* asserting sample-for-sample equality with `decode_ogg`:
/// lewton's `OggStreamReader` does its own granule bookkeeping (it discards the
/// priming packet and trims the leading ramp), so the two paths need not agree
/// on the tail frame-for-frame. What must agree is the stream's identity and
/// that both decode to a full-length, audible signal.
#[test]
fn packet_path_and_container_path_describe_the_same_stream() {
    let via_packets = OggPacketSequence::demux(TONE_OGG)
        .expect("valid stream")
        .decode()
        .expect("packet decode");
    let via_container = decode_ogg(TONE_OGG).expect("container decode");

    assert_eq!(via_packets.sample_rate, via_container.sample_rate);
    assert_eq!(via_packets.channels, via_container.channels);
    assert!(
        !via_container.samples.is_empty(),
        "the container path decodes this fixture — the first real .ogg the repo has had"
    );

    // Both land within a long block of the true length.
    let container_frames = via_container.frame_count() as i64;
    assert!(
        (container_frames - FINAL_GRANULE).abs() <= LONG_BLOCK,
        "container path gave {container_frames} frames, granule says {FINAL_GRANULE}",
    );
    let packet_frames = via_packets.frame_count() as i64;
    assert!(
        (packet_frames - FINAL_GRANULE).abs() <= LONG_BLOCK,
        "packet path gave {packet_frames} frames, granule says {FINAL_GRANULE}",
    );
}

/// An empty sequence must not divide by zero.
#[test]
fn empty_sequence_has_no_length_and_no_final_granule() {
    let seq = OggPacketSequence::new();

    assert_eq!(seq.page_count(), 0);
    assert_eq!(seq.packet_count(), 0);
    // ⚠️ -1, not 0 — mirrors Godot's `get_final_granule_pos`, which returns -1
    // for an empty sequence so that "no pages" is distinguishable from "a page
    // at granule 0" (the fixture's header pages are exactly that).
    assert_eq!(seq.final_granule_pos(), -1);
    assert_eq!(seq.length(), 0.0);
    assert!(seq.length().is_finite());
}

/// A sequence carrying pages but no sampling rate must not produce `inf`.
#[test]
fn length_is_zero_without_a_sampling_rate_rather_than_infinite() {
    let mut seq = OggPacketSequence::new();
    seq.push_page(15434, vec![vec![1, 2, 3]]);

    assert_eq!(seq.sampling_rate(), 0.0, "the default rate");
    assert_eq!(seq.final_granule_pos(), 15434);
    // Godot divides unguarded here and yields inf; that state is unreachable via
    // load_from_buffer but reachable from a hand-written .tres.
    assert_eq!(seq.length(), 0.0);
    assert!(
        seq.length().is_finite(),
        "must never leak inf into the mixer"
    );

    seq.set_sampling_rate(44100.0);
    assert!((seq.length() - 0.3500).abs() < 0.001);
}

/// Garbage in must be an error, not a panic or an empty success.
#[test]
fn demux_rejects_data_that_is_not_an_ogg_stream() {
    assert!(OggPacketSequence::demux(b"not an ogg file at all").is_err());
    assert!(OggPacketSequence::demux(&[]).is_err());
    // A RIFF/WAV header is the realistic confusion, and it is not Ogg.
    assert!(OggPacketSequence::demux(b"RIFF\x00\x00\x00\x00WAVEfmt ").is_err());
}

/// Decoding a sequence with no header packets must error rather than panic.
#[test]
fn decode_without_header_packets_is_an_error() {
    let mut seq = OggPacketSequence::new();
    seq.set_sampling_rate(44100.0);
    seq.push_page(0, vec![vec![0xff; 8]]);

    assert!(seq.decode().is_err(), "a bogus ident header cannot decode");

    let empty = OggPacketSequence::new();
    assert!(empty.decode().is_err(), "no packets at all cannot decode");
}
