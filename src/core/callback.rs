//! Real-time callback types.

/// Timing info passed to callbacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallbackInfo {
    /// Monotonic time in nanoseconds at callback start.
    pub callback_time_ns: u64,
    /// Number of frames requested in this callback.
    pub frames: u32,
}

/// Output callback.
///
/// Avoid allocation, blocking locks, and syscalls.
pub trait RenderCallback: Send + 'static {
    /// Write interleaved floating-point PCM samples into `out`.
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

/// Input callback.
///
/// Avoid allocation, blocking locks, and syscalls.
pub trait CaptureCallback: Send + 'static {
    /// Consume interleaved floating-point PCM samples from `input`.
    ///
    /// `input` length is `frames * channels`.
    fn capture(&mut self, info: CallbackInfo, input: &[f32]);
}

impl<F> CaptureCallback for F
where
    F: FnMut(CallbackInfo, &[f32]) + Send + 'static,
{
    fn capture(&mut self, info: CallbackInfo, input: &[f32]) {
        (self)(info, input);
    }
}
