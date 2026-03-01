//! Stream runtime metrics.

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
