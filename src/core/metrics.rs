//! Stream runtime metrics.

/// Negotiated stream sharing mode, when backend exposes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NegotiatedShareMode {
    /// Shared with system mixer or other clients.
    Shared,
    /// Exclusive device access.
    Exclusive,
}

/// Negotiated sample format, when backend exposes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NegotiatedSampleFormat {
    /// 32-bit float PCM.
    F32,
    /// Signed 16-bit PCM.
    I16,
}

/// Stream timing and latency snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StreamTiming {
    /// Most recent callback timestamp in nanoseconds, if available.
    pub callback_time_ns: Option<u64>,
    /// Backend frame position timestamp in nanoseconds, if available.
    pub backend_time_ns: Option<i64>,
    /// Frame position paired with `backend_time_ns`, if available.
    pub frame_position: Option<i64>,
    /// Estimated end-to-end stream latency in frames, if available.
    pub estimated_latency_frames: Option<u32>,
    /// Estimated end-to-end stream latency in nanoseconds, if available.
    pub estimated_latency_ns: Option<u64>,
    /// Backend-negotiated sharing mode, if available.
    pub negotiated_share_mode: Option<NegotiatedShareMode>,
    /// Backend-negotiated sample format, if available.
    pub negotiated_sample_format: Option<NegotiatedSampleFormat>,
}

/// Runtime counters reported by a stream backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StreamMetrics {
    /// Number of xruns reported by backend.
    pub xrun_count: u32,
    /// Frames written by backend, if available.
    pub frames_written: Option<i64>,
    /// Frames read by backend, if available.
    pub frames_read: Option<i64>,
    /// Timing and latency values.
    pub timing: StreamTiming,
}
