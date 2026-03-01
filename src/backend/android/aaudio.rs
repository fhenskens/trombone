//! AAudio backend.

#[cfg(target_os = "android")]
use crate::core::config::{PerformanceMode, SharingMode};
use crate::core::config::{SampleFormat, StreamConfig};
use crate::core::error::{AudioError, Result};
use crate::core::stream::Stream;

#[cfg(target_os = "android")]
use crate::core::callback::CallbackInfo;
#[cfg(target_os = "android")]
use crate::core::config::Direction;
#[cfg(target_os = "android")]
use crate::core::metrics::StreamMetrics;
#[cfg(target_os = "android")]
use crate::core::stream::{
    SharedCaptureCallback, SharedRenderCallback, capture_from_callback_handle,
    new_capture_callback_handle, new_render_callback_handle, render_from_callback_handle,
};
#[cfg(target_os = "android")]
use core::ffi::c_void;
#[cfg(target_os = "android")]
use core::slice;
#[cfg(target_os = "android")]
use std::sync::atomic::{AtomicU64, Ordering};

/// Create a stream backed by AAudio.
pub fn create_stream(config: StreamConfig) -> Result<Stream> {
    validate_requested_config(config)?;
    create_stream_impl(config)
}

fn validate_requested_config(config: StreamConfig) -> Result<()> {
    match config.format {
        SampleFormat::F32 => Ok(()),
        SampleFormat::I16 => Err(AudioError::UnsupportedConfig),
    }
}

#[cfg(not(target_os = "android"))]
fn create_stream_impl(_config: StreamConfig) -> Result<Stream> {
    Err(AudioError::NotImplemented)
}

#[cfg(target_os = "android")]
fn create_stream_impl(config: StreamConfig) -> Result<Stream> {
    let render_callback = new_render_callback_handle();
    let capture_callback = new_capture_callback_handle();
    let mut callback_ctx = Box::new(CallbackContext {
        direction: config.direction,
        render_callback: render_callback.clone(),
        capture_callback: capture_callback.clone(),
        channels: config.channels.get(),
        last_callback_time_ns: AtomicU64::new(0),
    });
    let mut stream_ptr: *mut ffi::AAudioStream = core::ptr::null_mut();
    let mut builder: *mut ffi::AAudioStreamBuilder = core::ptr::null_mut();

    unsafe {
        // Create builder first.
        check_result(ffi::AAudio_createStreamBuilder(&mut builder as *mut _))?;
        if builder.is_null() {
            return Err(AudioError::BackendFailure { code: -1 });
        }

        ffi::AAudioStreamBuilder_setDirection(builder, direction_to_aaudio(config.direction));
        ffi::AAudioStreamBuilder_setSampleRate(builder, config.sample_rate_hz.get() as i32);
        ffi::AAudioStreamBuilder_setChannelCount(builder, config.channels.get() as i32);
        ffi::AAudioStreamBuilder_setFormat(builder, sample_format_to_aaudio(config.format));
        ffi::AAudioStreamBuilder_setPerformanceMode(
            builder,
            performance_mode_to_aaudio(config.options.performance_mode),
        );
        ffi::AAudioStreamBuilder_setSharingMode(
            builder,
            sharing_mode_to_aaudio(config.options.sharing_mode),
        );
        ffi::AAudioStreamBuilder_setFramesPerDataCallback(
            builder,
            config.frames_per_burst.get() as i32,
        );
        ffi::AAudioStreamBuilder_setDataCallback(
            builder,
            Some(aaudio_data_callback),
            (&mut *callback_ctx) as *mut CallbackContext as *mut c_void,
        );

        let open_result = ffi::AAudioStreamBuilder_openStream(builder, &mut stream_ptr as *mut _);
        let _ = ffi::AAudioStreamBuilder_delete(builder);
        check_result(open_result)?;
    }

    if stream_ptr.is_null() {
        return Err(AudioError::BackendFailure { code: -1 });
    }

    let negotiated = match unsafe { negotiated_config_from_stream(stream_ptr, config) } {
        Ok(config) => config,
        Err(error) => {
            let _ = unsafe { ffi::AAudioStream_close(stream_ptr) };
            return Err(error);
        }
    };
    callback_ctx.channels = negotiated.channels.get();
    let backend = AAudioBackendStream {
        raw_stream: stream_ptr,
        is_started: false,
        callback_ctx,
        direction: negotiated.direction,
        sample_rate_hz: negotiated.sample_rate_hz.get(),
    };

    Ok(Stream::with_backend_and_callback(
        negotiated,
        Box::new(backend),
        render_callback,
        capture_callback,
    ))
}

#[cfg(target_os = "android")]
unsafe fn negotiated_config_from_stream(
    stream: *mut ffi::AAudioStream,
    requested: StreamConfig,
) -> Result<StreamConfig> {
    let sample_rate = unsafe { ffi::AAudioStream_getSampleRate(stream) };
    let channels = unsafe { ffi::AAudioStream_getChannelCount(stream) };
    let frames_per_burst = unsafe { ffi::AAudioStream_getFramesPerBurst(stream) };
    let format = unsafe { ffi::AAudioStream_getFormat(stream) };

    if sample_rate <= 0 || channels <= 0 || frames_per_burst <= 0 {
        return Err(AudioError::BackendFailure { code: -1 });
    }

    let negotiated_format =
        sample_format_from_aaudio(format).ok_or(AudioError::UnsupportedConfig)?;

    Ok(StreamConfig {
        channels: core::num::NonZeroU32::new(channels as u32)
            .ok_or(AudioError::BackendFailure { code: -1 })?,
        sample_rate_hz: core::num::NonZeroU32::new(sample_rate as u32)
            .ok_or(AudioError::BackendFailure { code: -1 })?,
        frames_per_burst: core::num::NonZeroU32::new(frames_per_burst as u32)
            .ok_or(AudioError::BackendFailure { code: -1 })?,
        format: negotiated_format,
        direction: requested.direction,
        options: requested.options,
    })
}

#[cfg(target_os = "android")]
fn check_result(code: i32) -> Result<()> {
    if code == ffi::AAUDIO_OK {
        Ok(())
    } else {
        Err(AudioError::BackendFailure { code })
    }
}

#[cfg(target_os = "android")]
fn sample_format_to_aaudio(format: SampleFormat) -> i32 {
    match format {
        SampleFormat::F32 => ffi::AAUDIO_FORMAT_PCM_FLOAT,
        SampleFormat::I16 => ffi::AAUDIO_FORMAT_PCM_I16,
    }
}

#[cfg(target_os = "android")]
fn direction_to_aaudio(direction: Direction) -> i32 {
    match direction {
        Direction::Output => ffi::AAUDIO_DIRECTION_OUTPUT,
        Direction::Input => ffi::AAUDIO_DIRECTION_INPUT,
    }
}

#[cfg(target_os = "android")]
fn sample_format_from_aaudio(value: i32) -> Option<SampleFormat> {
    match value {
        ffi::AAUDIO_FORMAT_PCM_FLOAT => Some(SampleFormat::F32),
        ffi::AAUDIO_FORMAT_PCM_I16 => Some(SampleFormat::I16),
        _ => None,
    }
}

#[cfg(target_os = "android")]
fn performance_mode_to_aaudio(mode: PerformanceMode) -> i32 {
    match mode {
        PerformanceMode::None => ffi::AAUDIO_PERFORMANCE_MODE_NONE,
        PerformanceMode::PowerSaving => ffi::AAUDIO_PERFORMANCE_MODE_POWER_SAVING,
        PerformanceMode::LowLatency => ffi::AAUDIO_PERFORMANCE_MODE_LOW_LATENCY,
    }
}

#[cfg(target_os = "android")]
fn sharing_mode_to_aaudio(mode: SharingMode) -> i32 {
    match mode {
        SharingMode::Exclusive => ffi::AAUDIO_SHARING_MODE_EXCLUSIVE,
        SharingMode::Shared => ffi::AAUDIO_SHARING_MODE_SHARED,
    }
}

#[cfg(target_os = "android")]
struct AAudioBackendStream {
    raw_stream: *mut ffi::AAudioStream,
    is_started: bool,
    callback_ctx: Box<CallbackContext>,
    direction: Direction,
    sample_rate_hz: u32,
}

#[cfg(target_os = "android")]
struct CallbackContext {
    direction: Direction,
    render_callback: SharedRenderCallback,
    capture_callback: SharedCaptureCallback,
    channels: u32,
    last_callback_time_ns: AtomicU64,
}

#[cfg(target_os = "android")]
unsafe extern "C" fn aaudio_data_callback(
    _stream: *mut ffi::AAudioStream,
    user_data: *mut c_void,
    audio_data: *mut c_void,
    num_frames: i32,
) -> i32 {
    if user_data.is_null() || audio_data.is_null() || num_frames <= 0 {
        return ffi::AAUDIO_CALLBACK_RESULT_CONTINUE;
    }

    let callback_ctx = unsafe { &mut *(user_data as *mut CallbackContext) };
    let channels = callback_ctx.channels as usize;
    let frame_count = num_frames as usize;
    let sample_count = frame_count.saturating_mul(channels);
    let callback_time_ns = unix_time_ns();
    callback_ctx
        .last_callback_time_ns
        .store(callback_time_ns, Ordering::Relaxed);

    let info = CallbackInfo {
        callback_time_ns,
        frames: num_frames as u32,
    };

    match callback_ctx.direction {
        Direction::Output => {
            let out_buffer =
                unsafe { slice::from_raw_parts_mut(audio_data as *mut f32, sample_count) };
            if render_from_callback_handle(&callback_ctx.render_callback, info, out_buffer).is_err()
            {
                out_buffer.fill(0.0);
            }
        }
        Direction::Input => {
            let input_buffer =
                unsafe { slice::from_raw_parts(audio_data as *const f32, sample_count) };
            let _ =
                capture_from_callback_handle(&callback_ctx.capture_callback, info, input_buffer);
        }
    }

    ffi::AAUDIO_CALLBACK_RESULT_CONTINUE
}

#[cfg(target_os = "android")]
unsafe impl Send for AAudioBackendStream {}

#[cfg(target_os = "android")]
impl crate::core::stream::StreamBackendOps for AAudioBackendStream {
    fn start(&mut self) -> Result<()> {
        if self.is_started {
            return Ok(());
        }
        let code = unsafe { ffi::AAudioStream_requestStart(self.raw_stream) };
        check_result(code)?;
        self.is_started = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        if !self.is_started {
            return Ok(());
        }
        let code = unsafe { ffi::AAudioStream_requestStop(self.raw_stream) };
        check_result(code)?;
        self.is_started = false;
        Ok(())
    }

    fn metrics(&self) -> StreamMetrics {
        if self.raw_stream.is_null() {
            return StreamMetrics::default();
        }

        let xrun_count = unsafe { ffi::AAudioStream_getXRunCount(self.raw_stream) };
        let frames_written = unsafe { ffi::AAudioStream_getFramesWritten(self.raw_stream) };
        let frames_read = unsafe { ffi::AAudioStream_getFramesRead(self.raw_stream) };
        let callback_time_ns = self
            .callback_ctx
            .last_callback_time_ns
            .load(Ordering::Relaxed);
        let callback_time_ns = (callback_time_ns > 0).then_some(callback_time_ns);

        let mut frame_position: i64 = 0;
        let mut backend_time_ns: i64 = 0;
        let timestamp_ok = unsafe {
            ffi::AAudioStream_getTimestamp(
                self.raw_stream,
                ffi::CLOCK_MONOTONIC,
                &mut frame_position as *mut _,
                &mut backend_time_ns as *mut _,
            )
        } == ffi::AAUDIO_OK;

        let queued_frames = match self.direction {
            Direction::Output => frames_written.saturating_sub(frames_read),
            Direction::Input => frames_read.saturating_sub(frames_written),
        };
        let latency_frames = u32::try_from(queued_frames).ok();
        let latency_ns = latency_frames.map(|frames| frames_to_ns(frames, self.sample_rate_hz));

        StreamMetrics {
            xrun_count: xrun_count.max(0) as u32,
            frames_written: Some(frames_written),
            frames_read: Some(frames_read),
            timing: crate::core::metrics::StreamTiming {
                callback_time_ns,
                backend_time_ns: timestamp_ok.then_some(backend_time_ns),
                frame_position: timestamp_ok.then_some(frame_position),
                estimated_latency_frames: latency_frames,
                estimated_latency_ns: latency_ns,
            },
        }
    }

    fn close(&mut self) {
        if self.raw_stream.is_null() {
            return;
        }

        if self.is_started {
            let _ = unsafe { ffi::AAudioStream_requestStop(self.raw_stream) };
            self.is_started = false;
        }

        let _ = unsafe { ffi::AAudioStream_close(self.raw_stream) };
        self.raw_stream = core::ptr::null_mut();
    }
}

#[cfg(target_os = "android")]
fn unix_time_ns() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(delta) => delta.as_nanos().min(u128::from(u64::MAX)) as u64,
        Err(_) => 0,
    }
}

#[cfg(target_os = "android")]
fn frames_to_ns(frames: u32, sample_rate_hz: u32) -> u64 {
    if sample_rate_hz == 0 {
        return 0;
    }
    ((frames as u128) * 1_000_000_000_u128 / (sample_rate_hz as u128)) as u64
}

#[cfg(target_os = "android")]
mod ffi {
    #[repr(C)]
    pub struct AAudioStreamBuilder {
        _private: [u8; 0],
    }

    #[repr(C)]
    pub struct AAudioStream {
        _private: [u8; 0],
    }

    pub const AAUDIO_OK: i32 = 0;
    pub const AAUDIO_DIRECTION_OUTPUT: i32 = 0;
    pub const AAUDIO_DIRECTION_INPUT: i32 = 1;
    pub const AAUDIO_FORMAT_PCM_I16: i32 = 1;
    pub const AAUDIO_FORMAT_PCM_FLOAT: i32 = 2;
    pub const AAUDIO_CALLBACK_RESULT_CONTINUE: i32 = 0;
    pub const AAUDIO_SHARING_MODE_EXCLUSIVE: i32 = 0;
    pub const AAUDIO_SHARING_MODE_SHARED: i32 = 1;
    pub const AAUDIO_PERFORMANCE_MODE_NONE: i32 = 10;
    pub const AAUDIO_PERFORMANCE_MODE_POWER_SAVING: i32 = 11;
    pub const AAUDIO_PERFORMANCE_MODE_LOW_LATENCY: i32 = 12;
    pub const CLOCK_MONOTONIC: i32 = 1;
    pub type AAudioDataCallback = Option<
        unsafe extern "C" fn(
            stream: *mut AAudioStream,
            user_data: *mut core::ffi::c_void,
            audio_data: *mut core::ffi::c_void,
            num_frames: i32,
        ) -> i32,
    >;

    #[link(name = "aaudio")]
    unsafe extern "C" {
        pub fn AAudio_createStreamBuilder(builder: *mut *mut AAudioStreamBuilder) -> i32;
        pub fn AAudioStreamBuilder_delete(builder: *mut AAudioStreamBuilder) -> i32;
        pub fn AAudioStreamBuilder_setDirection(builder: *mut AAudioStreamBuilder, direction: i32);
        pub fn AAudioStreamBuilder_setSampleRate(
            builder: *mut AAudioStreamBuilder,
            sample_rate: i32,
        );
        pub fn AAudioStreamBuilder_setChannelCount(
            builder: *mut AAudioStreamBuilder,
            channel_count: i32,
        );
        pub fn AAudioStreamBuilder_setFormat(builder: *mut AAudioStreamBuilder, format: i32);
        pub fn AAudioStreamBuilder_setPerformanceMode(builder: *mut AAudioStreamBuilder, mode: i32);
        pub fn AAudioStreamBuilder_setSharingMode(builder: *mut AAudioStreamBuilder, mode: i32);
        pub fn AAudioStreamBuilder_setFramesPerDataCallback(
            builder: *mut AAudioStreamBuilder,
            num_frames: i32,
        );
        pub fn AAudioStreamBuilder_setDataCallback(
            builder: *mut AAudioStreamBuilder,
            callback: AAudioDataCallback,
            user_data: *mut core::ffi::c_void,
        );
        pub fn AAudioStreamBuilder_openStream(
            builder: *mut AAudioStreamBuilder,
            stream: *mut *mut AAudioStream,
        ) -> i32;

        pub fn AAudioStream_close(stream: *mut AAudioStream) -> i32;
        pub fn AAudioStream_requestStart(stream: *mut AAudioStream) -> i32;
        pub fn AAudioStream_requestStop(stream: *mut AAudioStream) -> i32;

        pub fn AAudioStream_getSampleRate(stream: *mut AAudioStream) -> i32;
        pub fn AAudioStream_getChannelCount(stream: *mut AAudioStream) -> i32;
        pub fn AAudioStream_getFormat(stream: *mut AAudioStream) -> i32;
        pub fn AAudioStream_getFramesPerBurst(stream: *mut AAudioStream) -> i32;
        pub fn AAudioStream_getXRunCount(stream: *mut AAudioStream) -> i32;
        pub fn AAudioStream_getFramesRead(stream: *mut AAudioStream) -> i64;
        pub fn AAudioStream_getFramesWritten(stream: *mut AAudioStream) -> i64;
        pub fn AAudioStream_getTimestamp(
            stream: *mut AAudioStream,
            clockid: i32,
            frame_position: *mut i64,
            time_nanoseconds: *mut i64,
        ) -> i32;
    }
}

#[cfg(test)]
mod tests {
    use super::create_stream;
    use crate::core::config::{SampleFormat, StreamConfig};

    #[test]
    fn rejects_i16_format_for_now() {
        let config = StreamConfig {
            format: SampleFormat::I16,
            ..StreamConfig::default()
        };
        assert!(create_stream(config).is_err());
    }
}
