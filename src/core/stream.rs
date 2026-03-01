//! Stream state and lifecycle.

use crate::core::callback::{CallbackInfo, CaptureCallback, RenderCallback};
use crate::core::config::{Direction, StreamConfig};
use crate::core::error::{AudioError, Result};
use crate::core::metrics::{StreamMetrics, StreamTiming};
use std::sync::{Arc, Mutex};

pub(crate) type SharedRenderCallback = Arc<Mutex<Option<Box<dyn RenderCallback>>>>;
pub(crate) type SharedCaptureCallback = Arc<Mutex<Option<Box<dyn CaptureCallback>>>>;

pub(crate) fn new_render_callback_handle() -> SharedRenderCallback {
    Arc::new(Mutex::new(None))
}

pub(crate) fn new_capture_callback_handle() -> SharedCaptureCallback {
    Arc::new(Mutex::new(None))
}

pub(crate) fn render_from_callback_handle(
    handle: &SharedRenderCallback,
    info: CallbackInfo,
    out: &mut [f32],
) -> Result<()> {
    let mut guard = handle
        .lock()
        .map_err(|_| AudioError::BackendFailure { code: -3 })?;
    let callback = guard.as_mut().ok_or(AudioError::RenderCallbackNotSet)?;
    callback.render(info, out);
    Ok(())
}

pub(crate) fn capture_from_callback_handle(
    handle: &SharedCaptureCallback,
    info: CallbackInfo,
    input: &[f32],
) -> Result<()> {
    let mut guard = handle
        .lock()
        .map_err(|_| AudioError::BackendFailure { code: -4 })?;
    let callback = guard.as_mut().ok_or(AudioError::CaptureCallbackNotSet)?;
    callback.capture(info, input);
    Ok(())
}

/// Internal backend hooks used by `Stream`.
pub(crate) trait StreamBackendOps: Send {
    /// Start the backend stream.
    fn start(&mut self) -> Result<()>;
    /// Stop the backend stream.
    fn stop(&mut self) -> Result<()>;
    /// Return backend runtime metrics.
    fn metrics(&self) -> StreamMetrics;
    /// Close backend resources.
    fn close(&mut self);
}

/// Current stream state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    /// Created, not started.
    Stopped,
    /// Running callbacks.
    Running,
    /// Stream hit a recoverable runtime fault.
    XRun,
}

/// Stream handle used across backends.
///
/// Backends can wrap native handles behind this type.
pub struct Stream {
    config: StreamConfig,
    state: StreamState,
    render_callback: SharedRenderCallback,
    capture_callback: SharedCaptureCallback,
    backend: Option<Box<dyn StreamBackendOps>>,
}

impl Stream {
    /// Create a new stopped stream.
    pub fn new(config: StreamConfig) -> Self {
        Self {
            config,
            state: StreamState::Stopped,
            render_callback: new_render_callback_handle(),
            capture_callback: new_capture_callback_handle(),
            backend: None,
        }
    }

    /// Create a new stopped stream with backend hooks and callback handle.
    #[cfg(any(target_os = "android", target_os = "windows", target_os = "linux"))]
    pub(crate) fn with_backend_and_callback(
        config: StreamConfig,
        backend: Box<dyn StreamBackendOps>,
        render_callback: SharedRenderCallback,
        capture_callback: SharedCaptureCallback,
    ) -> Self {
        Self {
            config,
            state: StreamState::Stopped,
            render_callback,
            capture_callback,
            backend: Some(backend),
        }
    }

    /// Create a new stopped stream with backend hooks for tests.
    #[cfg(test)]
    pub(crate) fn with_backend_for_test(
        config: StreamConfig,
        backend: Box<dyn StreamBackendOps>,
    ) -> Self {
        Self {
            config,
            state: StreamState::Stopped,
            render_callback: new_render_callback_handle(),
            capture_callback: new_capture_callback_handle(),
            backend: Some(backend),
        }
    }

    /// Get stream config.
    pub fn config(&self) -> StreamConfig {
        self.config
    }

    /// Get current state.
    pub fn state(&self) -> StreamState {
        self.state
    }

    /// Start the stream.
    pub fn start(&mut self) -> Result<()> {
        match self.state {
            StreamState::Stopped => {
                match self.config.direction {
                    Direction::Output if !self.has_render_callback()? => {
                        return Err(AudioError::RenderCallbackNotSet);
                    }
                    Direction::Input if !self.has_capture_callback()? => {
                        return Err(AudioError::CaptureCallbackNotSet);
                    }
                    Direction::Output | Direction::Input => {}
                }
                if let Some(backend) = &mut self.backend {
                    backend.start()?;
                }
                self.state = StreamState::Running;
                Ok(())
            }
            StreamState::Running | StreamState::XRun => Err(AudioError::InvalidStateTransition),
        }
    }

    /// Stop the stream.
    pub fn stop(&mut self) -> Result<()> {
        match self.state {
            StreamState::Running | StreamState::XRun => {
                if let Some(backend) = &mut self.backend {
                    backend.stop()?;
                }
                self.state = StreamState::Stopped;
                Ok(())
            }
            StreamState::Stopped => Err(AudioError::InvalidStateTransition),
        }
    }

    /// Set the output callback.
    ///
    /// Callback can only be changed while stopped.
    pub fn set_render_callback<C>(&mut self, callback: C) -> Result<()>
    where
        C: RenderCallback,
    {
        if self.state != StreamState::Stopped {
            return Err(AudioError::InvalidStateTransition);
        }
        let mut guard = self
            .render_callback
            .lock()
            .map_err(|_| AudioError::BackendFailure { code: -3 })?;
        *guard = Some(Box::new(callback));
        Ok(())
    }

    /// Set the input callback.
    ///
    /// Callback can only be changed while stopped.
    pub fn set_capture_callback<C>(&mut self, callback: C) -> Result<()>
    where
        C: CaptureCallback,
    {
        if self.state != StreamState::Stopped {
            return Err(AudioError::InvalidStateTransition);
        }
        let mut guard = self
            .capture_callback
            .lock()
            .map_err(|_| AudioError::BackendFailure { code: -4 })?;
        *guard = Some(Box::new(callback));
        Ok(())
    }

    /// Render one buffer with the registered callback.
    ///
    /// Returns an error if no callback is set.
    pub fn render_into(&mut self, info: CallbackInfo, out: &mut [f32]) -> Result<()> {
        render_from_callback_handle(&self.render_callback, info, out)
    }

    /// Consume one input buffer with the registered callback.
    ///
    /// Returns an error if no callback is set.
    pub fn capture_from(&mut self, info: CallbackInfo, input: &[f32]) -> Result<()> {
        capture_from_callback_handle(&self.capture_callback, info, input)
    }

    /// Get runtime metrics.
    pub fn metrics(&self) -> StreamMetrics {
        self.backend
            .as_ref()
            .map_or(StreamMetrics::default(), |backend| backend.metrics())
    }

    /// Get runtime timing and latency values.
    pub fn timing(&self) -> StreamTiming {
        self.metrics().timing
    }

    fn has_render_callback(&self) -> Result<bool> {
        let guard = self
            .render_callback
            .lock()
            .map_err(|_| AudioError::BackendFailure { code: -3 })?;
        Ok(guard.is_some())
    }

    fn has_capture_callback(&self) -> Result<bool> {
        let guard = self
            .capture_callback
            .lock()
            .map_err(|_| AudioError::BackendFailure { code: -4 })?;
        Ok(guard.is_some())
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        if let Some(backend) = &mut self.backend {
            backend.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Stream, StreamBackendOps, StreamState};
    use crate::core::callback::CallbackInfo;
    use crate::core::config::{Direction, StreamConfig};
    use crate::core::error::{AudioError, Result};
    use crate::core::metrics::{StreamMetrics, StreamTiming};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct MockBackend {
        start_calls: Arc<AtomicUsize>,
        stop_calls: Arc<AtomicUsize>,
        close_calls: Arc<AtomicUsize>,
        fail_start: bool,
        fail_stop: bool,
    }

    impl StreamBackendOps for MockBackend {
        fn start(&mut self) -> Result<()> {
            self.start_calls.fetch_add(1, Ordering::SeqCst);
            if self.fail_start {
                return Err(AudioError::BackendFailure { code: -100 });
            }
            Ok(())
        }

        fn stop(&mut self) -> Result<()> {
            self.stop_calls.fetch_add(1, Ordering::SeqCst);
            if self.fail_stop {
                return Err(AudioError::BackendFailure { code: -200 });
            }
            Ok(())
        }

        fn close(&mut self) {
            self.close_calls.fetch_add(1, Ordering::SeqCst);
        }

        fn metrics(&self) -> StreamMetrics {
            StreamMetrics {
                xrun_count: 7,
                frames_written: Some(12_345),
                frames_read: Some(67),
                timing: StreamTiming {
                    callback_time_ns: Some(999),
                    backend_time_ns: Some(321),
                    frame_position: Some(654),
                    estimated_latency_frames: Some(64),
                    estimated_latency_ns: Some(1_333_333),
                    negotiated_share_mode: None,
                    negotiated_sample_format: None,
                },
            }
        }
    }

    #[test]
    fn start_stop_transitions_update_state() {
        let mut stream = Stream::new(StreamConfig::default());
        assert_eq!(stream.state(), StreamState::Stopped);

        stream
            .set_render_callback(|_info, out: &mut [f32]| out.fill(0.0))
            .expect("callback should set");
        stream.start().expect("start should work");
        assert_eq!(stream.state(), StreamState::Running);

        stream.stop().expect("stop should work");
        assert_eq!(stream.state(), StreamState::Stopped);
    }

    #[test]
    fn invalid_transitions_return_error() {
        let mut stream = Stream::new(StreamConfig::default());

        let stop_err = stream.stop().expect_err("stop before start should fail");
        assert_eq!(stop_err, AudioError::InvalidStateTransition);

        stream
            .set_render_callback(|_info, out: &mut [f32]| out.fill(0.0))
            .expect("callback should set");
        stream.start().expect("first start should work");
        let start_err = stream.start().expect_err("second start should fail");
        assert_eq!(start_err, AudioError::InvalidStateTransition);
    }

    #[test]
    fn backend_hooks_are_called_for_start_stop_and_drop() {
        let start_calls = Arc::new(AtomicUsize::new(0));
        let stop_calls = Arc::new(AtomicUsize::new(0));
        let close_calls = Arc::new(AtomicUsize::new(0));

        {
            let backend = MockBackend {
                start_calls: start_calls.clone(),
                stop_calls: stop_calls.clone(),
                close_calls: close_calls.clone(),
                fail_start: false,
                fail_stop: false,
            };
            let mut stream =
                Stream::with_backend_for_test(StreamConfig::default(), Box::new(backend));
            stream
                .set_render_callback(|_info, out: &mut [f32]| out.fill(0.0))
                .expect("callback should set");
            stream.start().expect("start should call backend start");
            stream.stop().expect("stop should call backend stop");
            let metrics = stream.metrics();
            assert_eq!(metrics.xrun_count, 7);
            assert_eq!(metrics.frames_written, Some(12_345));
            assert_eq!(metrics.frames_read, Some(67));
            assert_eq!(metrics.timing.callback_time_ns, Some(999));
            assert_eq!(stream.timing().estimated_latency_frames, Some(64));
        }

        assert_eq!(start_calls.load(Ordering::SeqCst), 1);
        assert_eq!(stop_calls.load(Ordering::SeqCst), 1);
        assert_eq!(close_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn backend_start_failure_keeps_stream_stopped() {
        let backend = MockBackend {
            start_calls: Arc::new(AtomicUsize::new(0)),
            stop_calls: Arc::new(AtomicUsize::new(0)),
            close_calls: Arc::new(AtomicUsize::new(0)),
            fail_start: true,
            fail_stop: false,
        };
        let mut stream = Stream::with_backend_for_test(StreamConfig::default(), Box::new(backend));
        stream
            .set_render_callback(|_info, out: &mut [f32]| out.fill(0.0))
            .expect("callback should set");

        let err = stream
            .start()
            .expect_err("backend failure should bubble up");
        assert_eq!(err, AudioError::BackendFailure { code: -100 });
        assert_eq!(stream.state(), StreamState::Stopped);
    }

    #[test]
    fn callback_can_only_be_set_while_stopped() {
        let mut stream = Stream::new(StreamConfig::default());
        stream
            .set_render_callback(|_info, out: &mut [f32]| out.fill(0.25))
            .expect("setting callback while stopped should work");
        stream
            .set_capture_callback(|_info, _input: &[f32]| {})
            .expect("setting capture callback while stopped should work");

        stream.start().expect("start should work");
        let err = stream
            .set_render_callback(|_info, out: &mut [f32]| out.fill(0.5))
            .expect_err("changing callback while running should fail");
        assert_eq!(err, AudioError::InvalidStateTransition);
        let capture_err = stream
            .set_capture_callback(|_info, _input: &[f32]| {})
            .expect_err("changing capture callback while running should fail");
        assert_eq!(capture_err, AudioError::InvalidStateTransition);
    }

    #[test]
    fn start_without_output_callback_fails() {
        let mut stream = Stream::new(StreamConfig::default());
        let err = stream
            .start()
            .expect_err("output stream start should fail without callback");
        assert_eq!(err, AudioError::RenderCallbackNotSet);
    }

    #[test]
    fn start_without_input_callback_fails() {
        let config = StreamConfig {
            direction: Direction::Input,
            ..StreamConfig::default()
        };
        let mut stream = Stream::new(config);
        let err = stream
            .start()
            .expect_err("input stream start should fail without callback");
        assert_eq!(err, AudioError::CaptureCallbackNotSet);
    }

    #[test]
    fn capture_from_uses_registered_callback() {
        let seen = Arc::new(AtomicBool::new(false));
        let seen_clone = seen.clone();
        let config = StreamConfig {
            direction: Direction::Input,
            ..StreamConfig::default()
        };
        let mut stream = Stream::new(config);
        stream
            .set_capture_callback(move |_info, input: &[f32]| {
                if !input.is_empty() {
                    seen_clone.store(true, Ordering::SeqCst);
                }
            })
            .expect("callback should set");

        let input = [0.1_f32; 8];
        stream
            .capture_from(
                CallbackInfo {
                    callback_time_ns: 1,
                    frames: 4,
                },
                &input,
            )
            .expect("capture should work");

        assert!(seen.load(Ordering::SeqCst));
    }

    #[test]
    fn capture_from_without_callback_returns_error() {
        let config = StreamConfig {
            direction: Direction::Input,
            ..StreamConfig::default()
        };
        let mut stream = Stream::new(config);
        let input = [0.0_f32; 4];
        let err = stream
            .capture_from(
                CallbackInfo {
                    callback_time_ns: 1,
                    frames: 2,
                },
                &input,
            )
            .expect_err("capture without callback should fail");
        assert_eq!(err, AudioError::CaptureCallbackNotSet);
    }

    #[test]
    fn render_into_uses_registered_callback() {
        let seen = Arc::new(AtomicBool::new(false));
        let seen_clone = seen.clone();
        let mut stream = Stream::new(StreamConfig::default());
        stream
            .set_render_callback(move |_info, out: &mut [f32]| {
                out.fill(0.75);
                seen_clone.store(true, Ordering::SeqCst);
            })
            .expect("callback should set");

        let mut out = [0.0_f32; 8];
        stream
            .render_into(
                CallbackInfo {
                    callback_time_ns: 1,
                    frames: 4,
                },
                &mut out,
            )
            .expect("render should work");

        assert!(seen.load(Ordering::SeqCst));
        assert_eq!(out, [0.75_f32; 8]);
    }

    #[test]
    fn render_into_without_callback_returns_error() {
        let mut stream = Stream::new(StreamConfig::default());
        let mut out = [0.0_f32; 4];
        let err = stream
            .render_into(
                CallbackInfo {
                    callback_time_ns: 1,
                    frames: 2,
                },
                &mut out,
            )
            .expect_err("render without callback should fail");
        assert_eq!(err, AudioError::RenderCallbackNotSet);
    }
}
