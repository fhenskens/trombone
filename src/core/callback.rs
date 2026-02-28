//! Real-time callback contracts.

/// Timing context passed into audio callbacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallbackInfo {
    /// Monotonic time in nanoseconds for the start of the callback.
    pub callback_time_ns: u64,
    /// Number of frames requested for this render quantum.
    pub frames: u32,
}

/// Output audio callback.
///
/// Implementations should avoid allocation, locks, and syscalls.
pub trait RenderCallback: Send + 'static {
    /// Produce `frames` worth of interleaved floating-point PCM in `out`.
    ///
    /// `out` length is `frames * channels`.
    fn render(&mut self, info: CallbackInfo, out: &mut [f32]);
}

impl<F> RenderCallback for F
where
    F: FnMut(CallbackInfo, &mut [f32]) + Send + 'static,
{
    fn render(&mut self, info: CallbackInfo, out: &mut [f32]) {
        (self)(info, out);
    }
}
