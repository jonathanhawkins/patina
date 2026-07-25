//! `VideoStreamPlayer` + a decode path from a video stream to frame textures
//! (pat-p0s82.28).
//!
//! Mirrors Godot's split between the `VideoStream` resource (the decoded frame
//! source) and `VideoStreamPlayer` (the Control node that walks it in time and
//! exposes the current frame via `get_video_texture`).
//!
//! **Codec scope:** Godot ships Theora (`VideoStreamTheora`). No Theora/VP8
//! decoder is vendored here and adding one is a dependency decision, so the
//! only decoder wired today reads the uncompressed `PATVID01` fixture
//! container — enough to drive real frames to a real [`Image`] end to end.
//! [`VideoStream`] is deliberately codec-agnostic: a Theora decoder only has to
//! produce the same `Vec<Image>` + fps to light up playback unchanged.

use gdresource::image::{Image, ImageFormat};
use thiserror::Error;

/// Magic bytes identifying the uncompressed fixture container.
const FIXTURE_MAGIC: &[u8; 8] = b"PATVID01";

/// `magic(8) + width(4) + height(4) + frame_count(4) + fps(f32, 4)`.
const HEADER_LEN: usize = 24;

/// Bytes per pixel in the fixture body (RGBA8).
const BYTES_PER_PIXEL: usize = 4;

/// Errors raised while decoding a video stream.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum VideoStreamError {
    /// The data does not start with the `PATVID01` magic.
    #[error("not a PATVID01 video fixture")]
    BadMagic,
    /// The header promises more frame data than the buffer holds.
    #[error("video fixture truncated: {0}")]
    Truncated(String),
    /// The header itself is nonsensical (zero-sized frames, non-positive fps…).
    #[error("invalid video fixture: {0}")]
    Invalid(String),
}

/// A decoded video stream: every frame as an [`Image`], plus its frame rate.
///
/// Analogous to Godot's `VideoStream` resource. Frames are decoded eagerly,
/// which is fine for fixtures; a real codec would stream them instead.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoStream {
    frames: Vec<Image>,
    fps: f64,
}

impl VideoStream {
    /// Decodes the uncompressed `PATVID01` fixture container.
    ///
    /// Layout: `"PATVID01"`, then little-endian `u32` width, `u32` height,
    /// `u32` frame_count, `f32` fps, followed by `frame_count` frames of
    /// `width × height` RGBA8 pixels.
    pub fn from_fixture_bytes(data: &[u8]) -> Result<Self, VideoStreamError> {
        if data.len() < HEADER_LEN || &data[..8] != FIXTURE_MAGIC {
            return Err(VideoStreamError::BadMagic);
        }
        let width = u32::from_le_bytes(data[8..12].try_into().unwrap());
        let height = u32::from_le_bytes(data[12..16].try_into().unwrap());
        let frame_count = u32::from_le_bytes(data[16..20].try_into().unwrap()) as usize;
        let fps = f32::from_le_bytes(data[20..24].try_into().unwrap()) as f64;

        if width == 0 || height == 0 {
            return Err(VideoStreamError::Invalid(format!(
                "frames must be non-empty, got {width}x{height}"
            )));
        }
        // Rejects NaN too, which would otherwise poison every time->frame map.
        if !(fps > 0.0) {
            return Err(VideoStreamError::Invalid(format!(
                "fps must be positive, got {fps}"
            )));
        }

        let frame_len = (width as usize)
            .checked_mul(height as usize)
            .and_then(|px| px.checked_mul(BYTES_PER_PIXEL))
            .ok_or_else(|| VideoStreamError::Invalid("frame size overflows".into()))?;
        let needed = frame_len
            .checked_mul(frame_count)
            .ok_or_else(|| VideoStreamError::Invalid("stream size overflows".into()))?;

        // Validate against the real byte count BEFORE decoding, so a corrupt
        // frame_count cannot drive a huge allocation.
        let body = &data[HEADER_LEN..];
        if body.len() < needed {
            return Err(VideoStreamError::Truncated(format!(
                "{frame_count} frame(s) need {needed} byte(s), have {}",
                body.len()
            )));
        }

        let frames = body[..needed]
            .chunks_exact(frame_len)
            .map(|chunk| Image::create_from_data(width, height, ImageFormat::Rgba8, chunk.to_vec()))
            .collect();
        Ok(Self { frames, fps })
    }

    /// Builds a stream directly from decoded frames (the seam a real codec
    /// would use). Errors if `fps` is not positive.
    pub fn from_frames(frames: Vec<Image>, fps: f64) -> Result<Self, VideoStreamError> {
        if !(fps > 0.0) {
            return Err(VideoStreamError::Invalid(format!(
                "fps must be positive, got {fps}"
            )));
        }
        Ok(Self { frames, fps })
    }

    /// Number of decoded frames.
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Frames per second.
    pub fn fps(&self) -> f64 {
        self.fps
    }

    /// Stream duration in seconds.
    pub fn length_secs(&self) -> f64 {
        self.frames.len() as f64 / self.fps
    }

    /// Frame by index.
    pub fn frame(&self, index: usize) -> Option<&Image> {
        self.frames.get(index)
    }

    /// The frame visible at `time` seconds, clamped to the last frame.
    /// Returns `None` for an empty stream or a negative time.
    pub fn frame_at(&self, time: f64) -> Option<&Image> {
        if self.frames.is_empty() || !(time >= 0.0) {
            return None;
        }
        let index = (time * self.fps) as usize;
        self.frames.get(index.min(self.frames.len() - 1))
    }
}

/// The `VideoStreamPlayer` Control node: walks a [`VideoStream`] in time and
/// exposes the current frame as its video texture.
#[derive(Debug, Clone, Default)]
pub struct VideoStreamPlayer {
    /// Whether playback is running (`is_playing` also accounts for `paused`).
    pub playing: bool,
    /// Playback head, in seconds.
    pub position_secs: f64,
    /// Stream duration; kept in sync by [`set_stream`](Self::set_stream).
    pub length_secs: f64,
    /// `res://` path of the assigned stream, for inspector parity.
    pub stream_path: String,
    /// When true, [`advance`](Self::advance) is a no-op (`VideoStreamPlayer.paused`).
    pub paused: bool,
    /// When true, playback wraps instead of stopping at the end (`loop`).
    pub loop_stream: bool,
    stream: Option<VideoStream>,
}

impl VideoStreamPlayer {
    /// Assigns a stream, adopting its length and rewinding to the start.
    pub fn set_stream(&mut self, stream: VideoStream) {
        self.length_secs = stream.length_secs();
        self.position_secs = 0.0;
        self.stream = Some(stream);
    }

    /// The assigned stream, if any.
    pub fn stream(&self) -> Option<&VideoStream> {
        self.stream.as_ref()
    }

    /// Starts playback (and clears `paused`, as Godot's `play()` does).
    pub fn play(&mut self) {
        self.playing = true;
        self.paused = false;
    }

    /// Stops playback and rewinds to the start.
    pub fn stop(&mut self) {
        self.playing = false;
        self.position_secs = 0.0;
    }

    /// Pauses/resumes without rewinding.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// True only when running and not paused.
    pub fn is_playing(&self) -> bool {
        self.playing && !self.paused
    }

    /// Moves the playback head, clamped to the stream.
    pub fn seek(&mut self, time: f64) {
        self.position_secs = time.clamp(0.0, self.length_secs.max(0.0));
    }

    /// The frame currently on screen (`VideoStreamPlayer.get_video_texture`).
    pub fn get_video_texture(&self) -> Option<&Image> {
        self.stream.as_ref()?.frame_at(self.position_secs)
    }

    /// Advances playback by `dt` seconds, honoring `paused` and `loop_stream`.
    pub fn advance(&mut self, dt: f64) {
        if !self.playing || self.paused {
            return;
        }
        let next = self.position_secs + dt;
        if self.length_secs > 0.0 && next >= self.length_secs {
            if self.loop_stream {
                self.position_secs = next % self.length_secs;
            } else {
                self.position_secs = self.length_secs;
                self.playing = false;
            }
        } else {
            self.position_secs = next.min(self.length_secs.max(0.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];

    /// Builds a `PATVID01` fixture of 1×1 frames, one per colour.
    fn fixture(fps: f32, colors: &[[u8; 4]]) -> Vec<u8> {
        fixture_sized(1, 1, fps, colors)
    }

    fn fixture_sized(width: u32, height: u32, fps: f32, colors: &[[u8; 4]]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(FIXTURE_MAGIC);
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&(colors.len() as u32).to_le_bytes());
        out.extend_from_slice(&fps.to_le_bytes());
        for color in colors {
            for _ in 0..(width * height) {
                out.extend_from_slice(color);
            }
        }
        out
    }

    #[test]
    fn play_advances() {
        let mut v = VideoStreamPlayer {
            length_secs: 1.0,
            ..Default::default()
        };
        v.play();
        v.advance(0.5);
        assert!((v.position_secs - 0.5).abs() < 1e-9);
        assert!(v.playing);
    }

    #[test]
    fn fixture_decodes_to_frames() {
        let data = fixture_sized(2, 3, 10.0, &[RED, GREEN]);
        let stream = VideoStream::from_fixture_bytes(&data).expect("decodes");
        assert_eq!(stream.frame_count(), 2);
        assert_eq!(stream.fps(), 10.0);
        assert!((stream.length_secs() - 0.2).abs() < 1e-9);

        let first = stream.frame(0).expect("frame 0");
        assert_eq!(first.get_width(), 2);
        assert_eq!(first.get_height(), 3);
        assert_eq!(first.get_format(), ImageFormat::Rgba8);
        // 2×3 pixels of red.
        assert_eq!(first.get_data(), &RED.repeat(6)[..]);
        assert_eq!(
            stream.frame(1).expect("frame 1").get_data(),
            &GREEN.repeat(6)[..]
        );
        assert!(stream.frame(2).is_none());
    }

    #[test]
    fn frame_at_maps_time_to_frame_and_clamps() {
        // 2 fps => each frame shows for 0.5s.
        let stream =
            VideoStream::from_fixture_bytes(&fixture(2.0, &[RED, GREEN, BLUE])).expect("decodes");
        assert_eq!(stream.frame_at(0.0).unwrap().get_data(), &RED[..]);
        assert_eq!(stream.frame_at(0.49).unwrap().get_data(), &RED[..]);
        assert_eq!(stream.frame_at(0.5).unwrap().get_data(), &GREEN[..]);
        assert_eq!(stream.frame_at(1.0).unwrap().get_data(), &BLUE[..]);
        // Past the end clamps to the last frame rather than vanishing.
        assert_eq!(stream.frame_at(99.0).unwrap().get_data(), &BLUE[..]);
        assert!(stream.frame_at(-1.0).is_none());
    }

    #[test]
    fn playing_a_fixture_walks_frames_to_the_video_texture() {
        let stream =
            VideoStream::from_fixture_bytes(&fixture(2.0, &[RED, GREEN, BLUE])).expect("decodes");
        let mut player = VideoStreamPlayer::default();
        player.set_stream(stream);
        assert!((player.length_secs - 1.5).abs() < 1e-9);

        player.play();
        assert_eq!(player.get_video_texture().unwrap().get_data(), &RED[..]);
        player.advance(0.5);
        assert_eq!(player.get_video_texture().unwrap().get_data(), &GREEN[..]);
        player.advance(0.5);
        assert_eq!(player.get_video_texture().unwrap().get_data(), &BLUE[..]);

        // Running past the end stops on the final frame.
        player.advance(0.5);
        assert!(!player.playing);
        assert_eq!(player.get_video_texture().unwrap().get_data(), &BLUE[..]);
    }

    #[test]
    fn paused_player_does_not_advance() {
        let stream = VideoStream::from_fixture_bytes(&fixture(2.0, &[RED, GREEN])).unwrap();
        let mut player = VideoStreamPlayer::default();
        player.set_stream(stream);
        player.play();
        player.set_paused(true);
        assert!(!player.is_playing());

        player.advance(0.5);
        assert_eq!(player.position_secs, 0.0);
        assert_eq!(player.get_video_texture().unwrap().get_data(), &RED[..]);

        player.set_paused(false);
        assert!(player.is_playing());
        player.advance(0.5);
        assert_eq!(player.get_video_texture().unwrap().get_data(), &GREEN[..]);
    }

    #[test]
    fn looping_wraps_instead_of_stopping() {
        let stream = VideoStream::from_fixture_bytes(&fixture(2.0, &[RED, GREEN])).unwrap();
        let mut player = VideoStreamPlayer::default();
        player.set_stream(stream);
        player.loop_stream = true;
        player.play();

        player.advance(1.25); // length is 1.0 => wraps to 0.25
        assert!(player.playing, "looping playback must not stop");
        assert!((player.position_secs - 0.25).abs() < 1e-9);
        assert_eq!(player.get_video_texture().unwrap().get_data(), &RED[..]);
    }

    #[test]
    fn seek_clamps_and_stop_rewinds() {
        let stream = VideoStream::from_fixture_bytes(&fixture(2.0, &[RED, GREEN])).unwrap();
        let mut player = VideoStreamPlayer::default();
        player.set_stream(stream);

        player.seek(0.75);
        assert_eq!(player.get_video_texture().unwrap().get_data(), &GREEN[..]);
        player.seek(99.0);
        assert_eq!(player.position_secs, 1.0);
        player.seek(-5.0);
        assert_eq!(player.position_secs, 0.0);

        player.seek(0.75);
        player.stop();
        assert_eq!(player.position_secs, 0.0);
        assert!(!player.playing);
    }

    #[test]
    fn player_without_a_stream_has_no_texture() {
        let player = VideoStreamPlayer::default();
        assert!(player.get_video_texture().is_none());
        assert!(player.stream().is_none());
    }

    #[test]
    fn from_frames_is_the_codec_seam() {
        let frames = vec![Image::create(1, 1, ImageFormat::Rgba8)];
        let stream = VideoStream::from_frames(frames, 4.0).expect("builds");
        assert_eq!(stream.frame_count(), 1);
        assert!((stream.length_secs() - 0.25).abs() < 1e-9);
        assert!(VideoStream::from_frames(Vec::new(), 0.0).is_err());
    }

    #[test]
    fn bad_magic_is_rejected() {
        assert_eq!(
            VideoStream::from_fixture_bytes(b"nope").unwrap_err(),
            VideoStreamError::BadMagic
        );
        let mut data = fixture(2.0, &[RED]);
        data[0] = b'X';
        assert_eq!(
            VideoStream::from_fixture_bytes(&data).unwrap_err(),
            VideoStreamError::BadMagic
        );
    }

    #[test]
    fn truncated_body_is_rejected() {
        let mut data = fixture(2.0, &[RED, GREEN]);
        data.truncate(data.len() - 1);
        let err = VideoStream::from_fixture_bytes(&data).unwrap_err();
        assert!(
            matches!(err, VideoStreamError::Truncated(_)),
            "got: {err:?}"
        );
    }

    #[test]
    fn absurd_frame_count_is_rejected_without_allocating() {
        let mut data = fixture(2.0, &[RED]);
        data[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        let err = VideoStream::from_fixture_bytes(&data).unwrap_err();
        assert!(
            matches!(err, VideoStreamError::Truncated(_)),
            "got: {err:?}"
        );
    }

    #[test]
    fn zero_sized_frames_and_bad_fps_are_rejected() {
        let mut zero_w = fixture(2.0, &[RED]);
        zero_w[8..12].copy_from_slice(&0u32.to_le_bytes());
        assert!(matches!(
            VideoStream::from_fixture_bytes(&zero_w).unwrap_err(),
            VideoStreamError::Invalid(_)
        ));

        let mut bad_fps = fixture(0.0, &[RED]);
        assert!(matches!(
            VideoStream::from_fixture_bytes(&bad_fps).unwrap_err(),
            VideoStreamError::Invalid(_)
        ));

        // NaN fps must not slip through and poison time->frame mapping.
        bad_fps[20..24].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(matches!(
            VideoStream::from_fixture_bytes(&bad_fps).unwrap_err(),
            VideoStreamError::Invalid(_)
        ));
    }
}
