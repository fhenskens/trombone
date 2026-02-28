//! Stream configuration types.

use core::num::NonZeroU32;

/// Audio sample format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormat {
    /// 32-bit floating-point samples in the `[-1.0, 1.0]` range.
    F32,
    /// Signed 16-bit integer PCM.
    I16,
}

/// Audio direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Render/playback stream.
    Output,
    /// Capture/recording stream.
    Input,
}

/// Core stream configuration requested by users.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamConfig {
    /// Number of channels. Stereo is `2`.
    pub channels: NonZeroU32,
    /// Requested sample rate in Hz.
    pub sample_rate_hz: NonZeroU32,
    /// Requested frames per callback quantum.
    pub frames_per_burst: NonZeroU32,
    /// Sample representation.
    pub format: SampleFormat,
    /// Input or output.
    pub direction: Direction,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            channels: NonZeroU32::new(2).expect("literal is non-zero"),
            sample_rate_hz: NonZeroU32::new(48_000).expect("literal is non-zero"),
            frames_per_burst: NonZeroU32::new(192).expect("literal is non-zero"),
            format: SampleFormat::F32,
            direction: Direction::Output,
        }
    }
}
