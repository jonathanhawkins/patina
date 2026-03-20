//! Audio sample buffer and output types.
//!
//! Provides [`AudioSampleBuffer`] for storing decoded PCM samples and
//! [`AudioOutputStream`] trait for audio output backends.

/// Channel layout of an audio buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelLayout {
    /// Single channel.
    Mono,
    /// Two channels (left, right).
    Stereo,
    /// Custom number of channels.
    Custom(u16),
}

/// A buffer of decoded audio samples in normalized f32 format.
///
/// Samples are interleaved for multi-channel audio:
/// `[L0, R0, L1, R1, ...]` for stereo.
#[derive(Debug, Clone)]
pub struct AudioSampleBuffer {
    /// Sample rate in Hz (e.g., 44100, 48000).
    pub sample_rate: u32,
    /// Number of audio channels.
    pub channels: u16,
    /// Channel layout.
    pub channel_layout: ChannelLayout,
    /// Interleaved samples in [-1.0, 1.0] range.
    pub samples: Vec<f32>,
}

impl AudioSampleBuffer {
    /// Creates a new empty sample buffer.
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        let layout = match channels {
            1 => ChannelLayout::Mono,
            2 => ChannelLayout::Stereo,
            n => ChannelLayout::Custom(n),
        };
        Self {
            sample_rate,
            channels,
            channel_layout: layout,
            samples: Vec::new(),
        }
    }

    /// Returns the number of frames (samples per channel).
    pub fn frame_count(&self) -> usize {
        if self.channels == 0 {
            return 0;
        }
        self.samples.len() / self.channels as usize
    }

    /// Returns the duration in seconds.
    pub fn duration_secs(&self) -> f32 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.frame_count() as f32 / self.sample_rate as f32
    }
}

/// Trait for audio output backends.
///
/// Implementations receive decoded audio samples and send them to the
/// platform's audio system (or discard them for headless testing).
pub trait AudioOutputStream {
    /// Writes a buffer of samples to the output.
    ///
    /// Returns the number of frames actually written.
    fn write_samples(&mut self, buffer: &AudioSampleBuffer) -> usize;

    /// Returns the sample rate expected by this output.
    fn sample_rate(&self) -> u32;

    /// Returns the number of output channels.
    fn channels(&self) -> u16;
}

/// A null audio output that accepts but discards all samples.
///
/// Useful for headless testing where no audio hardware is available.
#[derive(Debug)]
pub struct NullAudioOutput {
    sample_rate: u32,
    channels: u16,
    /// Total frames received (for verification in tests).
    pub frames_written: usize,
}

impl NullAudioOutput {
    /// Creates a null output with the given configuration.
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            sample_rate,
            channels,
            frames_written: 0,
        }
    }
}

impl AudioOutputStream for NullAudioOutput {
    fn write_samples(&mut self, buffer: &AudioSampleBuffer) -> usize {
        let frames = buffer.frame_count();
        self.frames_written += frames;
        frames
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn channels(&self) -> u16 {
        self.channels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_buffer_new_empty() {
        let buf = AudioSampleBuffer::new(44100, 2);
        assert_eq!(buf.sample_rate, 44100);
        assert_eq!(buf.channels, 2);
        assert_eq!(buf.channel_layout, ChannelLayout::Stereo);
        assert!(buf.samples.is_empty());
        assert_eq!(buf.frame_count(), 0);
        assert_eq!(buf.duration_secs(), 0.0);
    }

    #[test]
    fn sample_buffer_mono_frame_count() {
        let mut buf = AudioSampleBuffer::new(44100, 1);
        buf.samples = vec![0.0; 44100];
        assert_eq!(buf.frame_count(), 44100);
        assert!((buf.duration_secs() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn sample_buffer_stereo_frame_count() {
        let mut buf = AudioSampleBuffer::new(48000, 2);
        buf.samples = vec![0.0; 96000]; // 48000 frames * 2 channels
        assert_eq!(buf.frame_count(), 48000);
        assert!((buf.duration_secs() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn null_output_accepts_samples() {
        let mut output = NullAudioOutput::new(44100, 2);
        let mut buf = AudioSampleBuffer::new(44100, 2);
        buf.samples = vec![0.5, -0.5, 0.3, -0.3]; // 2 frames

        let written = output.write_samples(&buf);
        assert_eq!(written, 2);
        assert_eq!(output.frames_written, 2);
    }

    #[test]
    fn null_output_accumulates_frames() {
        let mut output = NullAudioOutput::new(44100, 1);

        let mut buf1 = AudioSampleBuffer::new(44100, 1);
        buf1.samples = vec![0.0; 100];
        output.write_samples(&buf1);

        let mut buf2 = AudioSampleBuffer::new(44100, 1);
        buf2.samples = vec![0.0; 200];
        output.write_samples(&buf2);

        assert_eq!(output.frames_written, 300);
    }

    #[test]
    fn null_output_properties() {
        let output = NullAudioOutput::new(48000, 2);
        assert_eq!(output.sample_rate(), 48000);
        assert_eq!(output.channels(), 2);
    }

    #[test]
    fn channel_layout_auto_detection() {
        assert_eq!(
            AudioSampleBuffer::new(44100, 1).channel_layout,
            ChannelLayout::Mono
        );
        assert_eq!(
            AudioSampleBuffer::new(44100, 2).channel_layout,
            ChannelLayout::Stereo
        );
        assert_eq!(
            AudioSampleBuffer::new(44100, 6).channel_layout,
            ChannelLayout::Custom(6)
        );
    }
}
