//! OpenSL ES backend.

use crate::core::config::{SampleFormat, StreamConfig};
use crate::core::error::{AudioError, Result};
use crate::core::stream::Stream;

#[cfg(target_os = "android")]
use crate::core::callback::CallbackInfo;
#[cfg(target_os = "android")]
use crate::core::config::Direction;
#[cfg(target_os = "android")]
use crate::core::metrics::{StreamMetrics, StreamTiming};
#[cfg(target_os = "android")]
use crate::core::stream::{
    SharedCaptureCallback, SharedRenderCallback, capture_from_callback_handle,
    new_capture_callback_handle, new_render_callback_handle, render_from_callback_handle,
};
#[cfg(target_os = "android")]
use core::ffi::c_void;
#[cfg(target_os = "android")]
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

/// Create a stream backed by OpenSL ES.
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
    match config.direction {
        Direction::Output => create_output_stream_with_fallback(config),
        Direction::Input => create_input_stream_with_fallback(config),
    }
}

#[cfg(target_os = "android")]
fn create_output_stream_with_fallback(config: StreamConfig) -> Result<Stream> {
    let mut first_error: Option<AudioError> = None;
    for candidate in fallback_candidates(config) {
        match create_output_stream(candidate) {
            Ok(stream) => return Ok(stream),
            Err(AudioError::BackendFailure {
                code: ffi::SL_RESULT_PARAMETER_INVALID,
            }) => {
                if first_error.is_none() {
                    first_error = Some(AudioError::BackendFailure {
                        code: ffi::SL_RESULT_PARAMETER_INVALID,
                    });
                }
                maybe_log_backend_fallback(
                    "output",
                    config.channels.get(),
                    config.sample_rate_hz.get(),
                    candidate.channels.get(),
                    candidate.sample_rate_hz.get(),
                );
            }
            Err(error) => return Err(error),
        }
    }

    Err(first_error.unwrap_or(AudioError::BackendFailure {
        code: ffi::SL_RESULT_PARAMETER_INVALID,
    }))
}

#[cfg(target_os = "android")]
fn create_input_stream_with_fallback(config: StreamConfig) -> Result<Stream> {
    let mut first_error: Option<AudioError> = None;
    for candidate in fallback_candidates(config) {
        match create_input_stream(candidate) {
            Ok(stream) => return Ok(stream),
            Err(AudioError::BackendFailure {
                code: ffi::SL_RESULT_PARAMETER_INVALID,
            }) => {
                if first_error.is_none() {
                    first_error = Some(AudioError::BackendFailure {
                        code: ffi::SL_RESULT_PARAMETER_INVALID,
                    });
                }
                maybe_log_backend_fallback(
                    "input",
                    config.channels.get(),
                    config.sample_rate_hz.get(),
                    candidate.channels.get(),
                    candidate.sample_rate_hz.get(),
                );
            }
            Err(error) => return Err(error),
        }
    }

    Err(first_error.unwrap_or(AudioError::BackendFailure {
        code: ffi::SL_RESULT_PARAMETER_INVALID,
    }))
}

#[cfg(target_os = "android")]
fn fallback_candidates(config: StreamConfig) -> [StreamConfig; 4] {
    let mono_channels = core::num::NonZeroU32::new(1).expect("literal is non-zero");
    let sr_44k1 = core::num::NonZeroU32::new(44_100).expect("literal is non-zero");
    [
        config,
        StreamConfig {
            channels: mono_channels,
            ..config
        },
        StreamConfig {
            sample_rate_hz: sr_44k1,
            ..config
        },
        StreamConfig {
            channels: mono_channels,
            sample_rate_hz: sr_44k1,
            ..config
        },
    ]
}

#[cfg(target_os = "android")]
fn create_output_stream(config: StreamConfig) -> Result<Stream> {
    let render_callback = new_render_callback_handle();
    let capture_callback = new_capture_callback_handle();
    let channels = config.channels.get() as usize;
    let frames_per_burst = config.frames_per_burst.get() as usize;

    let mut engine_obj: ffi::SLObjectItf = core::ptr::null_mut();
    let mut output_mix_obj: ffi::SLObjectItf = core::ptr::null_mut();
    let mut player_obj: ffi::SLObjectItf = core::ptr::null_mut();

    unsafe {
        check_result_stage(
            ffi::slCreateEngine(
                &mut engine_obj as *mut _,
                0,
                core::ptr::null(),
                0,
                core::ptr::null(),
                core::ptr::null(),
            ),
            "create_engine",
        )?;
        check_object_stage(engine_obj, "check_engine_object")?;
        check_result_stage(
            call_realize(engine_obj, ffi::SL_BOOLEAN_FALSE),
            "realize_engine",
        )?;

        let mut engine_itf: ffi::SLEngineItf = core::ptr::null_mut();
        check_result_stage(
            call_get_interface(
                engine_obj,
                ffi::SL_IID_ENGINE,
                &mut engine_itf as *mut _ as *mut c_void,
            ),
            "get_engine_interface",
        )?;
        check_engine_stage(engine_itf, "check_engine_interface")?;

        check_result_stage(
            call_create_output_mix(
                engine_itf,
                &mut output_mix_obj as *mut _,
                0,
                core::ptr::null(),
                core::ptr::null(),
            ),
            "create_output_mix",
        )?;
        check_object_stage(output_mix_obj, "check_output_mix_object")?;
        check_result_stage(
            call_realize(output_mix_obj, ffi::SL_BOOLEAN_FALSE),
            "realize_output_mix",
        )?;

        let mut locator_queue = ffi::SLDataLocator_AndroidSimpleBufferQueue {
            locatorType: ffi::SL_DATALOCATOR_ANDROIDSIMPLEBUFFERQUEUE,
            numBuffers: 2,
        };
        let mut format_pcm = ffi::SLDataFormat_PCM {
            formatType: ffi::SL_DATAFORMAT_PCM,
            numChannels: channels as u32,
            samplesPerSec: config.sample_rate_hz.get() * 1000,
            bitsPerSample: 16,
            containerSize: 16,
            channelMask: channel_mask(channels as u32),
            endianness: ffi::SL_BYTEORDER_LITTLEENDIAN,
        };
        let audio_source = ffi::SLDataSource {
            pLocator: &mut locator_queue as *mut _ as *mut c_void,
            pFormat: &mut format_pcm as *mut _ as *mut c_void,
        };

        let mut locator_output_mix = ffi::SLDataLocator_OutputMix {
            locatorType: ffi::SL_DATALOCATOR_OUTPUTMIX,
            outputMix: output_mix_obj,
        };
        let audio_sink = ffi::SLDataSink {
            pLocator: &mut locator_output_mix as *mut _ as *mut c_void,
            pFormat: core::ptr::null_mut(),
        };

        // PLAY is a core interface on audio players and should not be requested
        // as an optional interface at creation time.
        let interface_ids = [ffi::SL_IID_ANDROIDSIMPLEBUFFERQUEUE];
        let interface_required = [ffi::SL_BOOLEAN_TRUE];

        check_result_stage(
            call_create_audio_player(
                engine_itf,
                &mut player_obj as *mut _,
                &audio_source as *const _,
                &audio_sink as *const _,
                interface_ids.len() as u32,
                interface_ids.as_ptr(),
                interface_required.as_ptr(),
            ),
            "create_audio_player",
        )?;
        check_object_stage(player_obj, "check_player_object")?;
        check_result_stage(
            call_realize(player_obj, ffi::SL_BOOLEAN_FALSE),
            "realize_player",
        )?;
    }

    let mut play_itf: ffi::SLPlayItf = core::ptr::null_mut();
    let mut queue_itf: ffi::SLAndroidSimpleBufferQueueItf = core::ptr::null_mut();
    unsafe {
        check_result_stage(
            call_get_interface(
                player_obj,
                ffi::SL_IID_PLAY,
                &mut play_itf as *mut _ as *mut c_void,
            ),
            "get_play_interface",
        )?;
        check_result_stage(
            call_get_interface(
                player_obj,
                ffi::SL_IID_ANDROIDSIMPLEBUFFERQUEUE,
                &mut queue_itf as *mut _ as *mut c_void,
            ),
            "get_player_queue_interface",
        )?;
    }
    check_play_stage(play_itf, "check_play_interface")?;
    check_queue_stage(queue_itf, "check_player_queue_interface_nonnull")?;

    let mut callback_state = Box::new(OutputCallbackState::new(
        render_callback.clone(),
        channels,
        frames_per_burst,
        config.sample_rate_hz.get(),
    ));
    unsafe {
        check_result_stage(
            call_queue_register_callback(
                queue_itf,
                Some(opensl_output_callback),
                (&mut *callback_state) as *mut OutputCallbackState as *mut c_void,
            ),
            "register_output_callback",
        )?;
    }

    let backend = OpenSlesOutputStream {
        engine_obj,
        output_mix_obj,
        player_obj,
        play_itf,
        queue_itf,
        callback_state,
        started: false,
    };

    Ok(Stream::with_backend_and_callback(
        config,
        Box::new(backend),
        render_callback,
        capture_callback,
    ))
}

#[cfg(target_os = "android")]
fn create_input_stream(config: StreamConfig) -> Result<Stream> {
    let render_callback = new_render_callback_handle();
    let capture_callback = new_capture_callback_handle();
    let channels = config.channels.get() as usize;
    let frames_per_burst = config.frames_per_burst.get() as usize;

    let mut engine_obj: ffi::SLObjectItf = core::ptr::null_mut();
    let mut recorder_obj: ffi::SLObjectItf = core::ptr::null_mut();

    unsafe {
        check_result_stage(
            ffi::slCreateEngine(
                &mut engine_obj as *mut _,
                0,
                core::ptr::null(),
                0,
                core::ptr::null(),
                core::ptr::null(),
            ),
            "create_engine",
        )?;
        check_object_stage(engine_obj, "check_engine_object")?;
        check_result_stage(
            call_realize(engine_obj, ffi::SL_BOOLEAN_FALSE),
            "realize_engine",
        )?;

        let mut engine_itf: ffi::SLEngineItf = core::ptr::null_mut();
        check_result_stage(
            call_get_interface(
                engine_obj,
                ffi::SL_IID_ENGINE,
                &mut engine_itf as *mut _ as *mut c_void,
            ),
            "get_engine_interface",
        )?;
        check_engine_stage(engine_itf, "check_engine_interface")?;

        let mut locator_input = ffi::SLDataLocator_IODevice {
            locatorType: ffi::SL_DATALOCATOR_IODEVICE,
            deviceType: ffi::SL_IODEVICE_AUDIOINPUT,
            deviceID: ffi::SL_DEFAULTDEVICEID_AUDIOINPUT,
            device: core::ptr::null_mut(),
        };
        let audio_source = ffi::SLDataSource {
            pLocator: &mut locator_input as *mut _ as *mut c_void,
            pFormat: core::ptr::null_mut(),
        };

        let mut locator_queue = ffi::SLDataLocator_AndroidSimpleBufferQueue {
            locatorType: ffi::SL_DATALOCATOR_ANDROIDSIMPLEBUFFERQUEUE,
            numBuffers: 2,
        };
        let mut format_pcm = ffi::SLDataFormat_PCM {
            formatType: ffi::SL_DATAFORMAT_PCM,
            numChannels: channels as u32,
            samplesPerSec: config.sample_rate_hz.get() * 1000,
            bitsPerSample: 16,
            containerSize: 16,
            channelMask: channel_mask(channels as u32),
            endianness: ffi::SL_BYTEORDER_LITTLEENDIAN,
        };
        let audio_sink = ffi::SLDataSink {
            pLocator: &mut locator_queue as *mut _ as *mut c_void,
            pFormat: &mut format_pcm as *mut _ as *mut c_void,
        };

        // RECORD is a core interface on audio recorders and should not be requested
        // as an optional interface at creation time.
        let interface_ids = [ffi::SL_IID_ANDROIDSIMPLEBUFFERQUEUE];
        let interface_required = [ffi::SL_BOOLEAN_TRUE];

        check_result_stage(
            call_create_audio_recorder(
                engine_itf,
                &mut recorder_obj as *mut _,
                &audio_source as *const _,
                &audio_sink as *const _,
                interface_ids.len() as u32,
                interface_ids.as_ptr(),
                interface_required.as_ptr(),
            ),
            "create_audio_recorder",
        )?;
        check_object_stage(recorder_obj, "check_recorder_object")?;
        check_result_stage(
            call_realize(recorder_obj, ffi::SL_BOOLEAN_FALSE),
            "realize_recorder",
        )?;
    }

    let mut record_itf: ffi::SLRecordItf = core::ptr::null_mut();
    let mut queue_itf: ffi::SLAndroidSimpleBufferQueueItf = core::ptr::null_mut();
    unsafe {
        check_result_stage(
            call_get_interface(
                recorder_obj,
                ffi::SL_IID_RECORD,
                &mut record_itf as *mut _ as *mut c_void,
            ),
            "get_record_interface",
        )?;
        check_result_stage(
            call_get_interface(
                recorder_obj,
                ffi::SL_IID_ANDROIDSIMPLEBUFFERQUEUE,
                &mut queue_itf as *mut _ as *mut c_void,
            ),
            "get_recorder_queue_interface",
        )?;
    }
    check_record_stage(record_itf, "check_record_interface")?;
    check_queue_stage(queue_itf, "check_recorder_queue_interface_nonnull")?;

    let mut callback_state = Box::new(InputCallbackState::new(
        capture_callback.clone(),
        channels,
        frames_per_burst,
        config.sample_rate_hz.get(),
    ));
    unsafe {
        check_result_stage(
            call_queue_register_callback(
                queue_itf,
                Some(opensl_input_callback),
                (&mut *callback_state) as *mut InputCallbackState as *mut c_void,
            ),
            "register_input_callback",
        )?;
    }

    let backend = OpenSlesInputStream {
        engine_obj,
        recorder_obj,
        record_itf,
        queue_itf,
        callback_state,
        started: false,
    };

    Ok(Stream::with_backend_and_callback(
        config,
        Box::new(backend),
        render_callback,
        capture_callback,
    ))
}

#[cfg(target_os = "android")]
fn check_result(code: i32) -> Result<()> {
    if code == ffi::SL_RESULT_SUCCESS {
        Ok(())
    } else {
        Err(AudioError::BackendFailure { code })
    }
}

#[cfg(target_os = "android")]
fn check_object(obj: ffi::SLObjectItf) -> Result<()> {
    if obj.is_null() {
        Err(AudioError::BackendFailure { code: -21 })
    } else {
        Ok(())
    }
}

#[cfg(target_os = "android")]
fn check_engine(itf: ffi::SLEngineItf) -> Result<()> {
    if itf.is_null() {
        Err(AudioError::BackendFailure { code: -22 })
    } else {
        Ok(())
    }
}

#[cfg(target_os = "android")]
fn check_play(itf: ffi::SLPlayItf) -> Result<()> {
    if itf.is_null() {
        Err(AudioError::BackendFailure { code: -23 })
    } else {
        Ok(())
    }
}

#[cfg(target_os = "android")]
fn check_record(itf: ffi::SLRecordItf) -> Result<()> {
    if itf.is_null() {
        Err(AudioError::BackendFailure { code: -25 })
    } else {
        Ok(())
    }
}

#[cfg(target_os = "android")]
fn check_queue(itf: ffi::SLAndroidSimpleBufferQueueItf) -> Result<()> {
    if itf.is_null() {
        Err(AudioError::BackendFailure { code: -24 })
    } else {
        Ok(())
    }
}

#[cfg(target_os = "android")]
fn check_result_stage(code: i32, stage: &str) -> Result<()> {
    let result = check_result(code);
    annotate_stage(result, stage)
}

#[cfg(target_os = "android")]
fn check_object_stage(obj: ffi::SLObjectItf, stage: &str) -> Result<()> {
    annotate_stage(check_object(obj), stage)
}

#[cfg(target_os = "android")]
fn check_engine_stage(itf: ffi::SLEngineItf, stage: &str) -> Result<()> {
    annotate_stage(check_engine(itf), stage)
}

#[cfg(target_os = "android")]
fn check_play_stage(itf: ffi::SLPlayItf, stage: &str) -> Result<()> {
    annotate_stage(check_play(itf), stage)
}

#[cfg(target_os = "android")]
fn check_record_stage(itf: ffi::SLRecordItf, stage: &str) -> Result<()> {
    annotate_stage(check_record(itf), stage)
}

#[cfg(target_os = "android")]
fn check_queue_stage(itf: ffi::SLAndroidSimpleBufferQueueItf, stage: &str) -> Result<()> {
    annotate_stage(check_queue(itf), stage)
}

#[cfg(target_os = "android")]
fn annotate_stage(result: Result<()>, stage: &str) -> Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(AudioError::BackendFailure { code }) => {
            maybe_log_backend_failure(stage, code);
            Err(AudioError::BackendFailure { code })
        }
        Err(other) => Err(other),
    }
}

#[cfg(target_os = "android")]
fn maybe_log_backend_failure(stage: &str, code: i32) {
    if !backend_debug_enabled() {
        return;
    }
    eprintln!("OpenSL ES failure at stage '{stage}' with code {code}");
}

#[cfg(target_os = "android")]
fn maybe_log_backend_fallback(
    direction: &str,
    from_channels: u32,
    from_sample_rate_hz: u32,
    to_channels: u32,
    to_sample_rate_hz: u32,
) {
    if !backend_debug_enabled() {
        return;
    }
    if from_channels == to_channels && from_sample_rate_hz == to_sample_rate_hz {
        return;
    }
    eprintln!(
        "OpenSL ES fallback ({direction}): channels {from_channels} -> {to_channels}, rate {from_sample_rate_hz} -> {to_sample_rate_hz}"
    );
}

#[cfg(target_os = "android")]
fn backend_debug_enabled() -> bool {
    std::env::var_os("TROMBONE_BACKEND_DEBUG").is_some()
        || std::env::var_os("TROMBONE_DEBUG_BACKEND").is_some()
}

#[cfg(target_os = "android")]
fn channel_mask(channels: u32) -> u32 {
    if channels <= 1 {
        ffi::SL_SPEAKER_FRONT_LEFT
    } else {
        ffi::SL_SPEAKER_FRONT_LEFT | ffi::SL_SPEAKER_FRONT_RIGHT
    }
}

#[cfg(target_os = "android")]
struct OutputCallbackState {
    render_callback: SharedRenderCallback,
    frames_per_burst: usize,
    next_buffer: usize,
    f32_buffers: [Vec<f32>; 2],
    i16_buffers: [Vec<i16>; 2],
    xrun_count: AtomicU32,
    recovery_count: AtomicU32,
    frames_written: std::sync::atomic::AtomicI64,
    last_callback_time_ns: AtomicU64,
    queue_depth: AtomicUsize,
    sample_rate_hz: u32,
}

#[cfg(target_os = "android")]
impl OutputCallbackState {
    fn new(
        render_callback: SharedRenderCallback,
        channels: usize,
        frames_per_burst: usize,
        sample_rate_hz: u32,
    ) -> Self {
        let samples = channels.saturating_mul(frames_per_burst);
        Self {
            render_callback,
            frames_per_burst,
            next_buffer: 0,
            f32_buffers: [vec![0.0; samples], vec![0.0; samples]],
            i16_buffers: [vec![0; samples], vec![0; samples]],
            xrun_count: AtomicU32::new(0),
            recovery_count: AtomicU32::new(0),
            frames_written: std::sync::atomic::AtomicI64::new(0),
            last_callback_time_ns: AtomicU64::new(0),
            queue_depth: AtomicUsize::new(0),
            sample_rate_hz,
        }
    }

    fn fill_and_enqueue(&mut self, queue: ffi::SLAndroidSimpleBufferQueueItf) -> Result<()> {
        let idx = self.next_buffer % 2;
        self.next_buffer = self.next_buffer.wrapping_add(1);
        let f32_buf = &mut self.f32_buffers[idx];
        let i16_buf = &mut self.i16_buffers[idx];
        let callback_time_ns = unix_time_ns();
        self.last_callback_time_ns
            .store(callback_time_ns, Ordering::Relaxed);

        let info = CallbackInfo {
            callback_time_ns,
            frames: self.frames_per_burst as u32,
        };
        if render_from_callback_handle(&self.render_callback, info, f32_buf).is_err() {
            f32_buf.fill(0.0);
        }
        for (src, dst) in f32_buf.iter().zip(i16_buf.iter_mut()) {
            let clamped = src.clamp(-1.0, 1.0);
            *dst = (clamped * i16::MAX as f32) as i16;
        }

        unsafe {
            check_result(call_queue_enqueue(
                queue,
                i16_buf.as_ptr() as *const c_void,
                (i16_buf.len() * core::mem::size_of::<i16>()) as u32,
            ))?;
        }
        self.queue_depth.fetch_add(1, Ordering::Relaxed);

        self.frames_written
            .fetch_add(self.frames_per_burst as i64, Ordering::Relaxed);
        Ok(())
    }

    fn on_callback(&mut self, queue: ffi::SLAndroidSimpleBufferQueueItf) -> Result<()> {
        self.queue_depth
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |depth| {
                Some(depth.saturating_sub(1))
            })
            .ok();
        self.fill_and_enqueue(queue)
    }

    fn recover_queue(&mut self, queue: ffi::SLAndroidSimpleBufferQueueItf) {
        if unsafe { check_result(call_queue_clear(queue)) }.is_err() {
            return;
        }
        self.queue_depth.store(0, Ordering::Relaxed);
        if self.fill_and_enqueue(queue).is_ok() && self.fill_and_enqueue(queue).is_ok() {
            self.recovery_count.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(target_os = "android")]
struct InputCallbackState {
    capture_callback: SharedCaptureCallback,
    frames_per_burst: usize,
    next_buffer: usize,
    i16_buffers: [Vec<i16>; 2],
    f32_buffer: Vec<f32>,
    xrun_count: AtomicU32,
    recovery_count: AtomicU32,
    frames_read: std::sync::atomic::AtomicI64,
    last_callback_time_ns: AtomicU64,
    queue_depth: AtomicUsize,
    sample_rate_hz: u32,
}

#[cfg(target_os = "android")]
impl InputCallbackState {
    fn new(
        capture_callback: SharedCaptureCallback,
        channels: usize,
        frames_per_burst: usize,
        sample_rate_hz: u32,
    ) -> Self {
        let samples = channels.saturating_mul(frames_per_burst);
        Self {
            capture_callback,
            frames_per_burst,
            next_buffer: 0,
            i16_buffers: [vec![0; samples], vec![0; samples]],
            f32_buffer: vec![0.0; samples],
            xrun_count: AtomicU32::new(0),
            recovery_count: AtomicU32::new(0),
            frames_read: std::sync::atomic::AtomicI64::new(0),
            last_callback_time_ns: AtomicU64::new(0),
            queue_depth: AtomicUsize::new(0),
            sample_rate_hz,
        }
    }

    fn enqueue_buffer(
        &mut self,
        queue: ffi::SLAndroidSimpleBufferQueueItf,
        idx: usize,
    ) -> Result<()> {
        let buf = &self.i16_buffers[idx];
        unsafe {
            check_result(call_queue_enqueue(
                queue,
                buf.as_ptr() as *const c_void,
                (buf.len() * core::mem::size_of::<i16>()) as u32,
            ))
        }
        .map(|_| {
            self.queue_depth.fetch_add(1, Ordering::Relaxed);
        })
    }

    fn process_and_requeue(&mut self, queue: ffi::SLAndroidSimpleBufferQueueItf) -> Result<()> {
        let idx = self.next_buffer % 2;
        self.next_buffer = self.next_buffer.wrapping_add(1);
        let i16_buf = &self.i16_buffers[idx];
        self.queue_depth
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |depth| {
                Some(depth.saturating_sub(1))
            })
            .ok();

        for (src, dst) in i16_buf.iter().zip(self.f32_buffer.iter_mut()) {
            *dst = *src as f32 / i16::MAX as f32;
        }
        let callback_time_ns = unix_time_ns();
        self.last_callback_time_ns
            .store(callback_time_ns, Ordering::Relaxed);

        let info = CallbackInfo {
            callback_time_ns,
            frames: self.frames_per_burst as u32,
        };
        let _ = capture_from_callback_handle(&self.capture_callback, info, &self.f32_buffer);

        self.frames_read
            .fetch_add(self.frames_per_burst as i64, Ordering::Relaxed);
        self.enqueue_buffer(queue, idx)
    }

    fn recover_queue(&mut self, queue: ffi::SLAndroidSimpleBufferQueueItf) {
        if unsafe { check_result(call_queue_clear(queue)) }.is_err() {
            return;
        }
        self.queue_depth.store(0, Ordering::Relaxed);
        self.next_buffer = 0;
        if self.enqueue_buffer(queue, 0).is_ok() && self.enqueue_buffer(queue, 1).is_ok() {
            self.recovery_count.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(target_os = "android")]
unsafe extern "C" fn opensl_output_callback(
    caller: ffi::SLAndroidSimpleBufferQueueItf,
    context: *mut c_void,
) {
    if caller.is_null() || context.is_null() {
        return;
    }
    let state = unsafe { &mut *(context as *mut OutputCallbackState) };
    if state.on_callback(caller).is_err() {
        state.xrun_count.fetch_add(1, Ordering::Relaxed);
        state.recover_queue(caller);
    }
}

#[cfg(target_os = "android")]
unsafe extern "C" fn opensl_input_callback(
    caller: ffi::SLAndroidSimpleBufferQueueItf,
    context: *mut c_void,
) {
    if caller.is_null() || context.is_null() {
        return;
    }
    let state = unsafe { &mut *(context as *mut InputCallbackState) };
    if state.process_and_requeue(caller).is_err() {
        state.xrun_count.fetch_add(1, Ordering::Relaxed);
        state.recover_queue(caller);
    }
}

#[cfg(target_os = "android")]
struct OpenSlesOutputStream {
    engine_obj: ffi::SLObjectItf,
    output_mix_obj: ffi::SLObjectItf,
    player_obj: ffi::SLObjectItf,
    play_itf: ffi::SLPlayItf,
    queue_itf: ffi::SLAndroidSimpleBufferQueueItf,
    callback_state: Box<OutputCallbackState>,
    started: bool,
}

#[cfg(target_os = "android")]
unsafe impl Send for OpenSlesOutputStream {}

#[cfg(target_os = "android")]
impl crate::core::stream::StreamBackendOps for OpenSlesOutputStream {
    fn start(&mut self) -> Result<()> {
        if self.started {
            return Ok(());
        }
        unsafe {
            check_result(call_queue_clear(self.queue_itf))?;
        }
        self.callback_state.queue_depth.store(0, Ordering::Relaxed);

        if let Err(error) = self.callback_state.fill_and_enqueue(self.queue_itf) {
            unsafe {
                let _ = check_result(call_queue_clear(self.queue_itf));
            }
            self.callback_state.queue_depth.store(0, Ordering::Relaxed);
            return Err(error);
        }
        if let Err(error) = self.callback_state.fill_and_enqueue(self.queue_itf) {
            unsafe {
                let _ = check_result(call_queue_clear(self.queue_itf));
            }
            self.callback_state.queue_depth.store(0, Ordering::Relaxed);
            return Err(error);
        }

        if let Err(error) = unsafe {
            check_result(call_set_play_state(
                self.play_itf,
                ffi::SL_PLAYSTATE_PLAYING,
            ))
        } {
            unsafe {
                let _ = check_result(call_queue_clear(self.queue_itf));
            }
            self.callback_state.queue_depth.store(0, Ordering::Relaxed);
            return Err(error);
        }

        self.started = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        if !self.started {
            return Ok(());
        }
        let mut first_error: Option<AudioError> = None;
        unsafe {
            if let Err(error) = check_result(call_set_play_state(
                self.play_itf,
                ffi::SL_PLAYSTATE_STOPPED,
            )) && first_error.is_none()
            {
                first_error = Some(error);
            }
            if let Err(error) = check_result(call_queue_clear(self.queue_itf))
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        self.started = false;
        self.callback_state.queue_depth.store(0, Ordering::Relaxed);
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(())
    }

    fn metrics(&self) -> StreamMetrics {
        let queue_depth = self.callback_state.queue_depth.load(Ordering::Relaxed) as u32;
        let latency_frames =
            queue_depth.saturating_mul(self.callback_state.frames_per_burst as u32);
        StreamMetrics {
            xrun_count: self.callback_state.xrun_count.load(Ordering::Relaxed),
            frames_written: Some(self.callback_state.frames_written.load(Ordering::Relaxed)),
            frames_read: None,
            timing: StreamTiming {
                callback_time_ns: {
                    let value = self
                        .callback_state
                        .last_callback_time_ns
                        .load(Ordering::Relaxed);
                    (value > 0).then_some(value)
                },
                backend_time_ns: None,
                frame_position: None,
                estimated_latency_frames: Some(latency_frames),
                estimated_latency_ns: Some(frames_to_ns(
                    latency_frames,
                    self.callback_state.sample_rate_hz,
                )),
            },
        }
    }

    fn close(&mut self) {
        let _ = self.stop();
        unsafe {
            if !self.player_obj.is_null() {
                call_destroy(self.player_obj);
                self.player_obj = core::ptr::null_mut();
            }
            if !self.output_mix_obj.is_null() {
                call_destroy(self.output_mix_obj);
                self.output_mix_obj = core::ptr::null_mut();
            }
            if !self.engine_obj.is_null() {
                call_destroy(self.engine_obj);
                self.engine_obj = core::ptr::null_mut();
            }
        }
    }
}

#[cfg(target_os = "android")]
struct OpenSlesInputStream {
    engine_obj: ffi::SLObjectItf,
    recorder_obj: ffi::SLObjectItf,
    record_itf: ffi::SLRecordItf,
    queue_itf: ffi::SLAndroidSimpleBufferQueueItf,
    callback_state: Box<InputCallbackState>,
    started: bool,
}

#[cfg(target_os = "android")]
unsafe impl Send for OpenSlesInputStream {}

#[cfg(target_os = "android")]
impl crate::core::stream::StreamBackendOps for OpenSlesInputStream {
    fn start(&mut self) -> Result<()> {
        if self.started {
            return Ok(());
        }
        unsafe {
            check_result(call_queue_clear(self.queue_itf))?;
        }
        self.callback_state.queue_depth.store(0, Ordering::Relaxed);
        self.callback_state.next_buffer = 0;
        if let Err(error) = self.callback_state.enqueue_buffer(self.queue_itf, 0) {
            unsafe {
                let _ = check_result(call_queue_clear(self.queue_itf));
            }
            self.callback_state.queue_depth.store(0, Ordering::Relaxed);
            return Err(error);
        }
        if let Err(error) = self.callback_state.enqueue_buffer(self.queue_itf, 1) {
            unsafe {
                let _ = check_result(call_queue_clear(self.queue_itf));
            }
            self.callback_state.queue_depth.store(0, Ordering::Relaxed);
            return Err(error);
        }
        if let Err(error) = unsafe {
            check_result(call_set_record_state(
                self.record_itf,
                ffi::SL_RECORDSTATE_RECORDING,
            ))
        } {
            unsafe {
                let _ = check_result(call_queue_clear(self.queue_itf));
            }
            self.callback_state.queue_depth.store(0, Ordering::Relaxed);
            return Err(error);
        }
        self.started = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        if !self.started {
            return Ok(());
        }
        let mut first_error: Option<AudioError> = None;
        unsafe {
            if let Err(error) = check_result(call_set_record_state(
                self.record_itf,
                ffi::SL_RECORDSTATE_STOPPED,
            )) && first_error.is_none()
            {
                first_error = Some(error);
            }
            if let Err(error) = check_result(call_queue_clear(self.queue_itf))
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        self.started = false;
        self.callback_state.queue_depth.store(0, Ordering::Relaxed);
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(())
    }

    fn metrics(&self) -> StreamMetrics {
        let queue_depth = self.callback_state.queue_depth.load(Ordering::Relaxed) as u32;
        let latency_frames =
            queue_depth.saturating_mul(self.callback_state.frames_per_burst as u32);
        StreamMetrics {
            xrun_count: self.callback_state.xrun_count.load(Ordering::Relaxed),
            frames_written: None,
            frames_read: Some(self.callback_state.frames_read.load(Ordering::Relaxed)),
            timing: StreamTiming {
                callback_time_ns: {
                    let value = self
                        .callback_state
                        .last_callback_time_ns
                        .load(Ordering::Relaxed);
                    (value > 0).then_some(value)
                },
                backend_time_ns: None,
                frame_position: None,
                estimated_latency_frames: Some(latency_frames),
                estimated_latency_ns: Some(frames_to_ns(
                    latency_frames,
                    self.callback_state.sample_rate_hz,
                )),
            },
        }
    }

    fn close(&mut self) {
        let _ = self.stop();
        unsafe {
            if !self.recorder_obj.is_null() {
                call_destroy(self.recorder_obj);
                self.recorder_obj = core::ptr::null_mut();
            }
            if !self.engine_obj.is_null() {
                call_destroy(self.engine_obj);
                self.engine_obj = core::ptr::null_mut();
            }
        }
    }
}

#[cfg(target_os = "android")]
unsafe fn call_realize(obj: ffi::SLObjectItf, async_: u32) -> i32 {
    if obj.is_null() {
        return ffi::SL_RESULT_PARAMETER_INVALID;
    }
    let table = unsafe { *obj };
    if table.is_null() {
        return ffi::SL_RESULT_PARAMETER_INVALID;
    }
    match unsafe { (*table).Realize } {
        Some(func) => unsafe { func(obj, async_) },
        None => ffi::SL_RESULT_PARAMETER_INVALID,
    }
}

#[cfg(target_os = "android")]
unsafe fn call_get_interface(
    obj: ffi::SLObjectItf,
    iid: ffi::SLInterfaceID,
    itf: *mut c_void,
) -> i32 {
    if obj.is_null() {
        return ffi::SL_RESULT_PARAMETER_INVALID;
    }
    let table = unsafe { *obj };
    if table.is_null() {
        return ffi::SL_RESULT_PARAMETER_INVALID;
    }
    match unsafe { (*table).GetInterface } {
        Some(func) => unsafe { func(obj, iid, itf) },
        None => ffi::SL_RESULT_PARAMETER_INVALID,
    }
}

#[cfg(target_os = "android")]
unsafe fn call_destroy(obj: ffi::SLObjectItf) {
    if obj.is_null() {
        return;
    }
    let table = unsafe { *obj };
    if table.is_null() {
        return;
    }
    if let Some(func) = unsafe { (*table).Destroy } {
        unsafe { func(obj) };
    }
}

#[cfg(target_os = "android")]
unsafe fn call_create_output_mix(
    engine: ffi::SLEngineItf,
    mix_obj: *mut ffi::SLObjectItf,
    num_interfaces: u32,
    interface_ids: *const ffi::SLInterfaceID,
    interface_required: *const u32,
) -> i32 {
    if engine.is_null() {
        return ffi::SL_RESULT_PARAMETER_INVALID;
    }
    let table = unsafe { *engine };
    if table.is_null() {
        return ffi::SL_RESULT_PARAMETER_INVALID;
    }
    match unsafe { (*table).CreateOutputMix } {
        Some(func) => unsafe {
            func(
                engine,
                mix_obj,
                num_interfaces,
                interface_ids,
                interface_required,
            )
        },
        None => ffi::SL_RESULT_PARAMETER_INVALID,
    }
}

#[cfg(target_os = "android")]
unsafe fn call_create_audio_player(
    engine: ffi::SLEngineItf,
    player_obj: *mut ffi::SLObjectItf,
    audio_src: *const ffi::SLDataSource,
    audio_sink: *const ffi::SLDataSink,
    num_interfaces: u32,
    interface_ids: *const ffi::SLInterfaceID,
    interface_required: *const u32,
) -> i32 {
    if engine.is_null() {
        return ffi::SL_RESULT_PARAMETER_INVALID;
    }
    let table = unsafe { *engine };
    if table.is_null() {
        return ffi::SL_RESULT_PARAMETER_INVALID;
    }
    match unsafe { (*table).CreateAudioPlayer } {
        Some(func) => unsafe {
            func(
                engine,
                player_obj,
                audio_src,
                audio_sink,
                num_interfaces,
                interface_ids,
                interface_required,
            )
        },
        None => ffi::SL_RESULT_PARAMETER_INVALID,
    }
}

#[cfg(target_os = "android")]
unsafe fn call_create_audio_recorder(
    engine: ffi::SLEngineItf,
    recorder_obj: *mut ffi::SLObjectItf,
    audio_src: *const ffi::SLDataSource,
    audio_sink: *const ffi::SLDataSink,
    num_interfaces: u32,
    interface_ids: *const ffi::SLInterfaceID,
    interface_required: *const u32,
) -> i32 {
    if engine.is_null() {
        return ffi::SL_RESULT_PARAMETER_INVALID;
    }
    let table = unsafe { *engine };
    if table.is_null() {
        return ffi::SL_RESULT_PARAMETER_INVALID;
    }
    match unsafe { (*table).CreateAudioRecorder } {
        Some(func) => unsafe {
            func(
                engine,
                recorder_obj,
                audio_src,
                audio_sink,
                num_interfaces,
                interface_ids,
                interface_required,
            )
        },
        None => ffi::SL_RESULT_PARAMETER_INVALID,
    }
}

#[cfg(target_os = "android")]
unsafe fn call_set_play_state(play: ffi::SLPlayItf, state: u32) -> i32 {
    if play.is_null() {
        return ffi::SL_RESULT_PARAMETER_INVALID;
    }
    let table = unsafe { *play };
    if table.is_null() {
        return ffi::SL_RESULT_PARAMETER_INVALID;
    }
    match unsafe { (*table).SetPlayState } {
        Some(func) => unsafe { func(play, state) },
        None => ffi::SL_RESULT_PARAMETER_INVALID,
    }
}

#[cfg(target_os = "android")]
unsafe fn call_set_record_state(record: ffi::SLRecordItf, state: u32) -> i32 {
    if record.is_null() {
        return ffi::SL_RESULT_PARAMETER_INVALID;
    }
    let table = unsafe { *record };
    if table.is_null() {
        return ffi::SL_RESULT_PARAMETER_INVALID;
    }
    match unsafe { (*table).SetRecordState } {
        Some(func) => unsafe { func(record, state) },
        None => ffi::SL_RESULT_PARAMETER_INVALID,
    }
}

#[cfg(target_os = "android")]
unsafe fn call_queue_register_callback(
    queue: ffi::SLAndroidSimpleBufferQueueItf,
    callback: ffi::slAndroidSimpleBufferQueueCallback,
    context: *mut c_void,
) -> i32 {
    if queue.is_null() {
        return ffi::SL_RESULT_PARAMETER_INVALID;
    }
    let table = unsafe { *queue };
    if table.is_null() {
        return ffi::SL_RESULT_PARAMETER_INVALID;
    }
    match unsafe { (*table).RegisterCallback } {
        Some(func) => unsafe { func(queue, callback, context) },
        None => ffi::SL_RESULT_PARAMETER_INVALID,
    }
}

#[cfg(target_os = "android")]
unsafe fn call_queue_enqueue(
    queue: ffi::SLAndroidSimpleBufferQueueItf,
    buffer: *const c_void,
    size: u32,
) -> i32 {
    if queue.is_null() {
        return ffi::SL_RESULT_PARAMETER_INVALID;
    }
    let table = unsafe { *queue };
    if table.is_null() {
        return ffi::SL_RESULT_PARAMETER_INVALID;
    }
    match unsafe { (*table).Enqueue } {
        Some(func) => unsafe { func(queue, buffer, size) },
        None => ffi::SL_RESULT_PARAMETER_INVALID,
    }
}

#[cfg(target_os = "android")]
unsafe fn call_queue_clear(queue: ffi::SLAndroidSimpleBufferQueueItf) -> i32 {
    if queue.is_null() {
        return ffi::SL_RESULT_PARAMETER_INVALID;
    }
    let table = unsafe { *queue };
    if table.is_null() {
        return ffi::SL_RESULT_PARAMETER_INVALID;
    }
    match unsafe { (*table).Clear } {
        Some(func) => unsafe { func(queue) },
        None => ffi::SL_RESULT_PARAMETER_INVALID,
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
    #![allow(non_camel_case_types, non_snake_case, dead_code)]

    use core::ffi::c_void;

    pub type SLresult = i32;
    pub type SLboolean = u32;
    pub type SLuint32 = u32;
    pub type SLInterfaceID = *const c_void;
    pub type SLObjectItf = *mut *const SLObjectItf_;
    pub type SLEngineItf = *mut *const SLEngineItf_;
    pub type SLPlayItf = *mut *const SLPlayItf_;
    pub type SLRecordItf = *mut *const SLRecordItf_;
    pub type SLAndroidSimpleBufferQueueItf = *mut *const SLAndroidSimpleBufferQueueItf_;
    pub type slAndroidSimpleBufferQueueCallback =
        Option<unsafe extern "C" fn(caller: SLAndroidSimpleBufferQueueItf, context: *mut c_void)>;

    #[repr(C)]
    pub struct SLObjectItf_ {
        pub Realize:
            Option<unsafe extern "C" fn(self_: SLObjectItf, async_: SLboolean) -> SLresult>,
        pub Resume: Option<unsafe extern "C" fn(self_: SLObjectItf, async_: SLboolean) -> SLresult>,
        pub GetState:
            Option<unsafe extern "C" fn(self_: SLObjectItf, state: *mut SLuint32) -> SLresult>,
        pub GetInterface: Option<
            unsafe extern "C" fn(
                self_: SLObjectItf,
                iid: SLInterfaceID,
                iface: *mut c_void,
            ) -> SLresult,
        >,
        pub RegisterCallback: Option<
            unsafe extern "C" fn(
                self_: SLObjectItf,
                callback: *const c_void,
                context: *mut c_void,
            ) -> SLresult,
        >,
        pub AbortAsyncOperation: Option<unsafe extern "C" fn(self_: SLObjectItf) -> SLresult>,
        pub Destroy: Option<unsafe extern "C" fn(self_: SLObjectItf)>,
    }

    #[repr(C)]
    pub struct SLEngineItf_ {
        pub CreateLEDDevice: *const c_void,
        pub CreateVibraDevice: *const c_void,
        pub CreateAudioPlayer: Option<
            unsafe extern "C" fn(
                self_: SLEngineItf,
                player: *mut SLObjectItf,
                audio_src: *const SLDataSource,
                audio_sink: *const SLDataSink,
                num_interfaces: SLuint32,
                interface_ids: *const SLInterfaceID,
                interface_required: *const SLboolean,
            ) -> SLresult,
        >,
        pub CreateAudioRecorder: Option<
            unsafe extern "C" fn(
                self_: SLEngineItf,
                recorder: *mut SLObjectItf,
                audio_src: *const SLDataSource,
                audio_sink: *const SLDataSink,
                num_interfaces: SLuint32,
                interface_ids: *const SLInterfaceID,
                interface_required: *const SLboolean,
            ) -> SLresult,
        >,
        pub CreateMidiPlayer: *const c_void,
        pub CreateListener: *const c_void,
        pub Create3DGroup: *const c_void,
        pub CreateOutputMix: Option<
            unsafe extern "C" fn(
                self_: SLEngineItf,
                mix: *mut SLObjectItf,
                num_interfaces: SLuint32,
                interface_ids: *const SLInterfaceID,
                interface_required: *const SLboolean,
            ) -> SLresult,
        >,
        pub CreateMetadataExtractor: *const c_void,
        pub CreateExtensionObject: *const c_void,
        pub QueryNumSupportedInterfaces: *const c_void,
        pub QuerySupportedInterfaces: *const c_void,
        pub QueryNumSupportedExtensions: *const c_void,
        pub QuerySupportedExtension: *const c_void,
        pub IsExtensionSupported: *const c_void,
    }

    #[repr(C)]
    pub struct SLPlayItf_ {
        pub SetPlayState:
            Option<unsafe extern "C" fn(self_: SLPlayItf, state: SLuint32) -> SLresult>,
        pub GetPlayState: *const c_void,
        pub GetDuration: *const c_void,
        pub GetPosition: *const c_void,
        pub RegisterCallback: *const c_void,
        pub SetCallbackEventsMask: *const c_void,
        pub GetCallbackEventsMask: *const c_void,
        pub SetMarkerPosition: *const c_void,
        pub ClearMarkerPosition: *const c_void,
        pub SetPositionUpdatePeriod: *const c_void,
    }

    #[repr(C)]
    pub struct SLRecordItf_ {
        pub SetRecordState:
            Option<unsafe extern "C" fn(self_: SLRecordItf, state: SLuint32) -> SLresult>,
        pub GetRecordState: *const c_void,
        pub SetDurationLimit: *const c_void,
        pub GetPosition: *const c_void,
        pub RegisterCallback: *const c_void,
        pub SetCallbackEventsMask: *const c_void,
        pub GetCallbackEventsMask: *const c_void,
        pub SetMarkerPosition: *const c_void,
        pub ClearMarkerPosition: *const c_void,
        pub SetPositionUpdatePeriod: *const c_void,
    }

    #[repr(C)]
    pub struct SLAndroidSimpleBufferQueueItf_ {
        pub Enqueue: Option<
            unsafe extern "C" fn(
                self_: SLAndroidSimpleBufferQueueItf,
                buffer: *const c_void,
                size: SLuint32,
            ) -> SLresult,
        >,
        pub Clear: Option<unsafe extern "C" fn(self_: SLAndroidSimpleBufferQueueItf) -> SLresult>,
        pub GetState: *const c_void,
        pub RegisterCallback: Option<
            unsafe extern "C" fn(
                self_: SLAndroidSimpleBufferQueueItf,
                callback: slAndroidSimpleBufferQueueCallback,
                context: *mut c_void,
            ) -> SLresult,
        >,
    }

    #[repr(C)]
    pub struct SLDataSource {
        pub pLocator: *mut c_void,
        pub pFormat: *mut c_void,
    }

    #[repr(C)]
    pub struct SLDataSink {
        pub pLocator: *mut c_void,
        pub pFormat: *mut c_void,
    }

    #[repr(C)]
    pub struct SLDataLocator_AndroidSimpleBufferQueue {
        pub locatorType: SLuint32,
        pub numBuffers: SLuint32,
    }

    #[repr(C)]
    pub struct SLDataLocator_OutputMix {
        pub locatorType: SLuint32,
        pub outputMix: SLObjectItf,
    }

    #[repr(C)]
    pub struct SLDataLocator_IODevice {
        pub locatorType: SLuint32,
        pub deviceType: SLuint32,
        pub deviceID: SLuint32,
        pub device: *mut c_void,
    }

    #[repr(C)]
    pub struct SLDataFormat_PCM {
        pub formatType: SLuint32,
        pub numChannels: SLuint32,
        pub samplesPerSec: SLuint32,
        pub bitsPerSample: SLuint32,
        pub containerSize: SLuint32,
        pub channelMask: SLuint32,
        pub endianness: SLuint32,
    }

    pub const SL_RESULT_SUCCESS: SLresult = 0;
    pub const SL_RESULT_PARAMETER_INVALID: SLresult = 2;
    pub const SL_BOOLEAN_FALSE: SLboolean = 0;
    pub const SL_BOOLEAN_TRUE: SLboolean = 1;
    pub const SL_DATALOCATOR_IODEVICE: SLuint32 = 0x00000003;
    pub const SL_DATALOCATOR_OUTPUTMIX: SLuint32 = 0x0004;
    pub const SL_DATALOCATOR_ANDROIDSIMPLEBUFFERQUEUE: SLuint32 = 0x800007;
    pub const SL_DATAFORMAT_PCM: SLuint32 = 0x00000001;
    pub const SL_BYTEORDER_LITTLEENDIAN: SLuint32 = 0x00000002;
    pub const SL_PLAYSTATE_STOPPED: SLuint32 = 0x00000001;
    pub const SL_PLAYSTATE_PLAYING: SLuint32 = 0x00000003;
    pub const SL_RECORDSTATE_STOPPED: SLuint32 = 0x00000001;
    pub const SL_RECORDSTATE_RECORDING: SLuint32 = 0x00000003;
    pub const SL_IODEVICE_AUDIOINPUT: SLuint32 = 0x00000001;
    pub const SL_DEFAULTDEVICEID_AUDIOINPUT: SLuint32 = 0xFFFFFFFF;
    pub const SL_SPEAKER_FRONT_LEFT: SLuint32 = 0x00000001;
    pub const SL_SPEAKER_FRONT_RIGHT: SLuint32 = 0x00000002;
    pub const SL_SPEAKER_FRONT_CENTER: SLuint32 = 0x00000004;

    #[link(name = "OpenSLES")]
    unsafe extern "C" {
        pub static SL_IID_ENGINE: SLInterfaceID;
        pub static SL_IID_PLAY: SLInterfaceID;
        pub static SL_IID_RECORD: SLInterfaceID;
        pub static SL_IID_ANDROIDSIMPLEBUFFERQUEUE: SLInterfaceID;
        pub fn slCreateEngine(
            engine: *mut SLObjectItf,
            num_options: SLuint32,
            engine_options: *const c_void,
            num_interfaces: SLuint32,
            interface_ids: *const SLInterfaceID,
            interface_required: *const SLboolean,
        ) -> SLresult;
    }
}

#[cfg(test)]
mod tests {
    use super::create_stream;
    use crate::core::config::{SampleFormat, StreamConfig};

    #[test]
    fn accepts_input_config_shape() {
        let config = StreamConfig {
            direction: crate::core::config::Direction::Input,
            ..StreamConfig::default()
        };
        let _ = create_stream(config);
    }

    #[test]
    fn rejects_i16_format_for_now() {
        let config = StreamConfig {
            format: SampleFormat::I16,
            ..StreamConfig::default()
        };
        assert!(create_stream(config).is_err());
    }
}
