//! Stream settings.

use core::num::NonZeroU32;

/// Sample format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormat {
    /// 32-bit float samples in the `[-1.0, 1.0]` range.
    F32,
    /// Signed 16-bit integer PCM.
    I16,
}

/// Stream direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Playback stream.
    Output,
    /// Recording stream.
    Input,
}

/// Performance hint for backend stream creation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerformanceMode {
    /// No specific performance preference.
    None,
    /// Prefer lower latency.
    LowLatency,
    /// Prefer lower power usage.
    PowerSaving,
}

/// Sharing mode for the audio device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharingMode {
    /// Shared with other clients.
    Shared,
    /// Exclusive access when possible.
    Exclusive,
}

/// Usage category for policy routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Usage {
    /// Unspecified usage.
    Unknown,
    /// Media playback/recording usage.
    Media,
    /// Voice communication usage.
    VoiceCommunication,
    /// Alarm usage.
    Alarm,
}

/// Content category hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    /// Unspecified content type.
    Unknown,
    /// Speech-focused content.
    Speech,
    /// Music-focused content.
    Music,
}

/// Additional stream options commonly exposed by audio APIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamOptions {
    /// Backend performance hint.
    pub performance_mode: PerformanceMode,
    /// Shared or exclusive stream request.
    pub sharing_mode: SharingMode,
    /// Usage hint for routing/policy.
    pub usage: Usage,
    /// Content type hint.
    pub content_type: ContentType,
}

impl Default for StreamOptions {
    fn default() -> Self {
        Self {
            performance_mode: PerformanceMode::LowLatency,
            sharing_mode: SharingMode::Shared,
            usage: Usage::Media,
            content_type: ContentType::Music,
        }
    }
}

/// Requested stream settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamConfig {
    /// Number of channels. Stereo is `2`.
    pub channels: NonZeroU32,
    /// Sample rate in Hz.
    pub sample_rate_hz: NonZeroU32,
    /// Frames per callback.
    pub frames_per_burst: NonZeroU32,
    /// Sample format.
    pub format: SampleFormat,
    /// Input or output stream.
    pub direction: Direction,
    /// Extra backend options.
    pub options: StreamOptions,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            channels: NonZeroU32::new(2).expect("literal is non-zero"),
            sample_rate_hz: NonZeroU32::new(48_000).expect("literal is non-zero"),
            frames_per_burst: NonZeroU32::new(192).expect("literal is non-zero"),
            format: SampleFormat::F32,
            direction: Direction::Output,
            options: StreamOptions::default(),
        }
    }
}
