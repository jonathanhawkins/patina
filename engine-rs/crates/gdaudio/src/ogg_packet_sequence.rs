//! `OggPacketSequence` — the demuxed form Godot stores an imported Ogg Vorbis
//! stream in (pat-nzgdu.188).
//!
//! Godot does not keep the original `.ogg` container. Its importer demuxes the
//! Ogg transport once, at import time, and stores the bare **Vorbis packets**
//! grouped by the **page** they arrived in, alongside each page's granule
//! position (`modules/ogg/ogg_packet_sequence.h`). `AudioStreamOggVorbis` then
//! holds a reference to that sequence, and playback feeds those packets straight
//! to the Vorbis decoder — the Ogg framing layer is never seen again.
//!
//! Patina previously had no representation for this form at all: [`crate::decode::decode_ogg`]
//! takes a whole `.ogg` container and hands it to `lewton`'s [`OggStreamReader`],
//! so a `.tres` carrying an `OggPacketSequence` sub-resource had nothing to load
//! into and its audio was silently dropped. This module supplies the missing
//! type, plus the two directions that make it useful:
//!
//! * [`OggPacketSequence::demux`] — `.ogg` container → packet sequence, mirroring
//!   `AudioStreamOggVorbis::load_from_buffer`.
//! * [`OggPacketSequence::decode`] — packet sequence → PCM, using `lewton`'s
//!   packet-level API rather than its container reader.
//!
//! The two paths are held together by a round-trip test: demuxing a `.ogg` and
//! decoding the resulting sequence must produce **byte-identical PCM** to
//! decoding the container directly.
//!
//! [`OggStreamReader`]: lewton::inside_ogg::OggStreamReader

use std::io::Cursor;

use lewton::audio::{read_audio_packet_generic, PreviousWindowRight};
use lewton::header::{read_header_comment, read_header_ident, read_header_setup};
use lewton::samples::InterleavedSamples;
use ogg::PacketReader;

use crate::decode::{AudioBuffer, DecodeError};

/// A demuxed Ogg Vorbis stream: pages of Vorbis packets, plus per-page granule
/// positions and the sampling rate read out of the identification header.
///
/// Mirrors Godot's `OggPacketSequence : Resource`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OggPacketSequence {
    /// `page -> packet -> bytes`. Godot's `Vector<Vector<PackedByteArray>>`.
    page_data: Vec<Vec<Vec<u8>>>,
    /// One granule position per **page** — not per packet, despite the
    /// `{set,get}_packet_granule_positions` accessors Godot binds.
    granule_positions: Vec<i64>,
    /// Sampling rate, as reported by the identification header.
    sampling_rate: f32,
}

impl OggPacketSequence {
    /// Creates an empty sequence.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends one page of packets and its granule position.
    pub fn push_page(&mut self, granule_position: i64, packets: Vec<Vec<u8>>) {
        self.page_data.push(packets);
        self.granule_positions.push(granule_position);
    }

    /// The pages, each a list of packets.
    pub fn page_data(&self) -> &[Vec<Vec<u8>>] {
        &self.page_data
    }

    /// One granule position per page.
    pub fn granule_positions(&self) -> &[i64] {
        &self.granule_positions
    }

    /// Sampling rate in Hz, or `0.0` if no identification header has been read.
    pub fn sampling_rate(&self) -> f32 {
        self.sampling_rate
    }

    /// Sets the sampling rate (Godot's `set_sampling_rate`).
    pub fn set_sampling_rate(&mut self, rate: f32) {
        self.sampling_rate = rate;
    }

    /// Number of pages.
    pub fn page_count(&self) -> usize {
        self.page_data.len()
    }

    /// Total number of packets across all pages.
    pub fn packet_count(&self) -> usize {
        self.page_data.iter().map(Vec::len).sum()
    }

    /// Every packet in stream order, flattened across pages.
    ///
    /// Page grouping matters for seeking (granules are per page), but the Vorbis
    /// decoder only wants a flat packet stream.
    pub fn packets(&self) -> impl Iterator<Item = &[u8]> + '_ {
        self.page_data
            .iter()
            .flat_map(|page| page.iter().map(Vec::as_slice))
    }

    /// The granule position of the last page, or `-1` when there are no pages.
    ///
    /// Mirrors `OggPacketSequence::get_final_granule_pos`.
    pub fn final_granule_pos(&self) -> i64 {
        self.granule_positions.last().copied().unwrap_or(-1)
    }

    /// Stream length in seconds (`final_granule_pos / sampling_rate`).
    ///
    /// Mirrors `OggPacketSequence::get_length`, including its `granule < 0 -> 0`
    /// guard. Godot does **not** guard the divisor, so a sequence carrying pages
    /// but no sampling rate yields `inf` there; that state is unreachable through
    /// `load_from_buffer` (which always sets the rate from the ident header), but
    /// it is reachable from a hand-written `.tres`, so we return `0.0` rather
    /// than leak a non-finite length into the mixer.
    pub fn length(&self) -> f32 {
        let granule = self.final_granule_pos();
        if granule < 0 || self.sampling_rate <= 0.0 {
            return 0.0;
        }
        granule as f32 / self.sampling_rate
    }

    /// Demuxes an `.ogg` container into a packet sequence.
    ///
    /// Mirrors `AudioStreamOggVorbis::load_from_buffer`: packets are grouped by
    /// the page they arrived in, each page is tagged with that page's granule
    /// position, empty packets are dropped, and a stream that yields no pages at
    /// all is an error. The sampling rate is read from the identification header,
    /// which is what Godot's `maybe_update_info` does when the sequence is
    /// attached to a stream.
    pub fn demux(data: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = PacketReader::new(Cursor::new(data));
        let mut sequence = Self::new();
        let mut current_page: Vec<Vec<u8>> = Vec::new();

        while let Some(packet) = reader
            .read_packet()
            .map_err(|e| DecodeError::Ogg(format!("{e:?}")))?
        {
            let ends_page = packet.last_in_page();
            let granule = packet.absgp_page();
            // Godot skips zero-length packets rather than storing them.
            if !packet.data.is_empty() {
                current_page.push(packet.data);
            }
            if ends_page {
                // A page whose packets were all empty is not pushed, matching
                // Godot's `packet_data.size() > 0` guard.
                if !current_page.is_empty() {
                    sequence.push_page(granule as i64, std::mem::take(&mut current_page));
                } else {
                    current_page.clear();
                }
            }
        }

        if sequence.granule_positions.is_empty() {
            return Err(DecodeError::Ogg(
                "no Ogg pages found — not a valid Ogg Vorbis stream".into(),
            ));
        }

        // The rate lives in the ident header, which is always the first packet.
        let ident_packet = sequence
            .packets()
            .next()
            .ok_or_else(|| DecodeError::Ogg("stream has pages but no packets".into()))?;
        let ident =
            read_header_ident(ident_packet).map_err(|e| DecodeError::Ogg(format!("{e:?}")))?;
        sequence.sampling_rate = ident.audio_sample_rate as f32;

        Ok(sequence)
    }

    /// Decodes the sequence to interleaved PCM.
    ///
    /// Uses `lewton`'s packet-level API: the first three packets are the Vorbis
    /// identification, comment and setup headers, and every packet after them is
    /// audio. This is the same decode the container path performs — it simply
    /// skips the Ogg framing layer, which the sequence has already stripped.
    ///
    /// The output is capped at the final granule position — the stream's true
    /// length by the Ogg spec, and what `ffprobe` reports as `duration_ts`. A
    /// raw packet decode has no framing to tell it where the audio ends, so a
    /// final long block can run past the granule; the cap trims that padding.
    /// (It is only a cap: standard streams begin with a short block whose
    /// leading half is decoder ramp-up, so a faithful decode often lands a little
    /// *below* the granule — for `tests/fixtures/test_tone.ogg`, granule 15434 vs
    /// ~15306 decoded frames — and the cap then does nothing. Either way the
    /// buffer never exceeds the stream length.) Unlike lewton's container reader
    /// this does not discard the priming packet or trim the leading ramp, so the
    /// PCM is not sample-identical to the container path; it is the same audio.
    pub fn decode(&self) -> Result<AudioBuffer, DecodeError> {
        let mut packets = self.packets();

        let ident_packet = packets
            .next()
            .ok_or_else(|| DecodeError::Ogg("missing Vorbis identification header".into()))?;
        let comment_packet = packets
            .next()
            .ok_or_else(|| DecodeError::Ogg("missing Vorbis comment header".into()))?;
        let setup_packet = packets
            .next()
            .ok_or_else(|| DecodeError::Ogg("missing Vorbis setup header".into()))?;

        let ident =
            read_header_ident(ident_packet).map_err(|e| DecodeError::Ogg(format!("{e:?}")))?;
        // Parsed for validation only; tags are the stream's business, not ours.
        read_header_comment(comment_packet).map_err(|e| DecodeError::Ogg(format!("{e:?}")))?;
        let setup = read_header_setup(
            setup_packet,
            ident.audio_channels,
            (ident.blocksize_0, ident.blocksize_1),
        )
        .map_err(|e| DecodeError::Ogg(format!("{e:?}")))?;

        let mut previous_window = PreviousWindowRight::new();
        let mut samples: Vec<f32> = Vec::new();
        for packet in packets {
            let decoded: InterleavedSamples<i16> =
                read_audio_packet_generic(&ident, &setup, packet, &mut previous_window)
                    .map_err(|e| DecodeError::Ogg(format!("{e:?}")))?;
            samples.extend(decoded.samples.into_iter().map(|s| s as f32 / 32768.0));
        }

        let channels = ident.audio_channels as usize;
        // Cap at the stream's real length. The `<` guard makes this a cap, never
        // an extend: a decode that legitimately lands below the granule (leading
        // ramp) is left untouched, and a sequence whose granule outruns the
        // packets it carries (a truncated file) cannot grow the buffer.
        let final_granule = self.final_granule_pos();
        if final_granule > 0 && channels > 0 {
            let target = (final_granule as usize).saturating_mul(channels);
            if target < samples.len() {
                samples.truncate(target);
            }
        }

        Ok(AudioBuffer {
            samples,
            sample_rate: ident.audio_sample_rate,
            channels: ident.audio_channels as u16,
        })
    }
}
