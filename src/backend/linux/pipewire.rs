//! Native PipeWire backend (Linux).

use crate::core::config::{SampleFormat, StreamConfig};
use crate::core::error::{AudioError, Result};
use crate::core::stream::Stream;

#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
use crate::core::callback::CallbackInfo;
#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
use crate::core::config::Direction;
#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
use crate::core::metrics::StreamMetrics;
#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
use crate::core::stream::{
    SharedCaptureCallback, SharedRenderCallback, StreamBackendOps, capture_from_callback_handle,
    new_capture_callback_handle, new_render_callback_handle, render_from_callback_handle,
};
#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
use pipewire as pw;
#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
use pw::{properties::properties, spa};
#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
use spa::pod::Pod;
#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
use std::sync::Arc;
#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
use std::sync::mpsc::{self, Sender};
#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
use std::thread::{self, JoinHandle};
#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Create a stream backed by native PipeWire.
#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
pub fn create_stream(config: StreamConfig) -> Result<Stream> {
    validate_requested_config(config)?;
    probe_runtime_support()?;

    let render_callback: SharedRenderCallback = new_render_callback_handle();
    let capture_callback: SharedCaptureCallback = new_capture_callback_handle();
    let backend =
        PipeWireBackendStream::new(config, render_callback.clone(), capture_callback.clone());
    Ok(Stream::with_backend_and_callback(
        config,
        Box::new(backend),
        render_callback,
        capture_callback,
    ))
}

/// Create a stream backed by native PipeWire.
#[cfg(all(target_os = "linux", not(feature = "linux-pipewire")))]
pub fn create_stream(config: StreamConfig) -> Result<Stream> {
    validate_requested_config(config)?;
    Err(AudioError::NotImplemented)
}

/// Create a stream backed by native PipeWire.
#[cfg(not(target_os = "linux"))]
pub fn create_stream(config: StreamConfig) -> Result<Stream> {
    validate_requested_config(config)?;
    Err(AudioError::NotImplemented)
}

fn validate_requested_config(config: StreamConfig) -> Result<()> {
    match config.format {
        SampleFormat::F32 | SampleFormat::I16 => Ok(()),
    }
}

#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
fn probe_runtime_support() -> Result<()> {
    if !pipewire_socket_exists() {
        return Err(AudioError::BackendFailure { code: -730 });
    }
    if !libpipewire_available() {
        return Err(AudioError::BackendFailure { code: -731 });
    }
    Ok(())
}

#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
fn pipewire_socket_exists() -> bool {
    pipewire_socket_path().is_some_and(|path| path.exists())
}

#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
fn pipewire_socket_path() -> Option<std::path::PathBuf> {
    if let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        let candidate = std::path::PathBuf::from(runtime_dir).join("pipewire-0");
        if candidate.exists() {
            return Some(candidate);
        }
    }
    let uid = unsafe { libc::geteuid() };
    let fallback = std::path::PathBuf::from(format!("/run/user/{uid}/pipewire-0"));
    if fallback.exists() {
        Some(fallback)
    } else {
        None
    }
}

#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
fn libpipewire_available() -> bool {
    use std::ffi::CString;

    let names = ["libpipewire-0.3.so.0", "libpipewire-0.3.so"];
    for name in names {
        let Ok(cname) = CString::new(name) else {
            continue;
        };
        let handle = unsafe { libc::dlopen(cname.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL) };
        if !handle.is_null() {
            let _ = unsafe { libc::dlclose(handle) };
            return true;
        }
    }
    false
}

#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
#[derive(Default)]
struct PipeWireMetrics {
    callback_time_ns: AtomicU64,
    frames_written: AtomicI64,
    frames_read: AtomicI64,
}

#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
struct WorkerState {
    stop: Arc<AtomicBool>,
    join: JoinHandle<()>,
}

#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
struct PipeWireBackendStream {
    config: StreamConfig,
    render_callback: SharedRenderCallback,
    capture_callback: SharedCaptureCallback,
    metrics: Arc<PipeWireMetrics>,
    worker: Option<WorkerState>,
}

#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
impl PipeWireBackendStream {
    fn new(
        config: StreamConfig,
        render_callback: SharedRenderCallback,
        capture_callback: SharedCaptureCallback,
    ) -> Self {
        Self {
            config,
            render_callback,
            capture_callback,
            metrics: Arc::new(PipeWireMetrics::default()),
            worker: None,
        }
    }
}

#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
impl StreamBackendOps for PipeWireBackendStream {
    fn start(&mut self) -> Result<()> {
        if self.worker.is_some() {
            return Ok(());
        }

        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = stop.clone();
        let config = self.config;
        let render_callback = self.render_callback.clone();
        let capture_callback = self.capture_callback.clone();
        let metrics = self.metrics.clone();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<()>>();

        let join = thread::spawn(move || {
            let worker_result = match config.direction {
                Direction::Output => {
                    run_output_worker(config, render_callback, metrics, stop_for_thread, ready_tx)
                }
                Direction::Input => {
                    run_input_worker(config, capture_callback, metrics, stop_for_thread, ready_tx)
                }
            };
            if let Err(err) = worker_result {
                eprintln!("PipeWire worker error: {err:?}");
            }
        });

        match ready_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                stop.store(true, Ordering::Relaxed);
                let _ = join.join();
                return Err(err);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                stop.store(true, Ordering::Relaxed);
                let _ = join.join();
                return Err(AudioError::BackendFailure { code: -5 });
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                stop.store(true, Ordering::Relaxed);
                let _ = join.join();
                return Err(AudioError::BackendFailure { code: -6 });
            }
        }

        self.worker = Some(WorkerState { stop, join });
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        if let Some(worker) = self.worker.take() {
            worker.stop.store(true, Ordering::Relaxed);
            let _ = worker.join.join();
        }
        Ok(())
    }

    fn metrics(&self) -> StreamMetrics {
        let frames_written = self.metrics.frames_written.load(Ordering::Relaxed);
        let frames_read = self.metrics.frames_read.load(Ordering::Relaxed);
        StreamMetrics {
            xrun_count: 0,
            frames_written: (self.config.direction == Direction::Output).then_some(frames_written),
            frames_read: (self.config.direction == Direction::Input).then_some(frames_read),
            timing: crate::core::metrics::StreamTiming {
                callback_time_ns: match self.metrics.callback_time_ns.load(Ordering::Relaxed) {
                    0 => None,
                    value => Some(value),
                },
                ..Default::default()
            },
        }
    }

    fn close(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
struct OutputUserData {
    render_callback: SharedRenderCallback,
    metrics: Arc<PipeWireMetrics>,
    channels: usize,
    format: SampleFormat,
    scratch: Vec<f32>,
}

#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
struct InputUserData {
    capture_callback: SharedCaptureCallback,
    metrics: Arc<PipeWireMetrics>,
    channels: usize,
    format: SampleFormat,
    scratch: Vec<f32>,
}

#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
fn run_output_worker(
    config: StreamConfig,
    render_callback: SharedRenderCallback,
    metrics: Arc<PipeWireMetrics>,
    stop: Arc<AtomicBool>,
    ready_tx: Sender<Result<()>>,
) -> Result<()> {
    let channels = config.channels.get() as usize;
    let bytes_per_sample = sample_bytes(config.format);
    let stride = channels.saturating_mul(bytes_per_sample);
    let initial_samples = (config.frames_per_burst.get() as usize).saturating_mul(channels);

    pw::init();
    let mainloop = pw::main_loop::MainLoop::new(None).map_err(|e| AudioError::BackendFailure {
        code: map_pw_error(e),
    })?;
    let context = pw::context::Context::new(&mainloop).map_err(|e| AudioError::BackendFailure {
        code: map_pw_error(e),
    })?;
    let core = context
        .connect(None)
        .map_err(|e| AudioError::BackendFailure {
            code: map_pw_error(e),
        })?;

    let stream = pw::stream::Stream::new(
        &core,
        "trombone-output",
        properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Playback",
            *pw::keys::MEDIA_ROLE => "Music",
        },
    )
    .map_err(|e| AudioError::BackendFailure {
        code: map_pw_error(e),
    })?;

    let user_data = OutputUserData {
        render_callback,
        metrics: metrics.clone(),
        channels,
        format: config.format,
        scratch: vec![0.0_f32; initial_samples.max(1)],
    };

    let _listener = stream
        .add_local_listener_with_user_data(user_data)
        .process(move |stream, user_data| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let datas = buffer.datas_mut();
            if datas.is_empty() || stride == 0 {
                return;
            }
            let data = &mut datas[0];
            let n_frames = if let Some(slice) = data.data() {
                let n_frames = slice.len() / stride;
                let sample_count = n_frames.saturating_mul(user_data.channels);
                if sample_count == 0 {
                    return;
                }
                if sample_count > user_data.scratch.len() {
                    user_data.scratch.resize(sample_count, 0.0);
                }
                let out = &mut user_data.scratch[..sample_count];
                let callback_time_ns = unix_time_ns();
                user_data
                    .metrics
                    .callback_time_ns
                    .store(callback_time_ns, Ordering::Relaxed);
                let info = CallbackInfo {
                    callback_time_ns,
                    frames: n_frames as u32,
                };
                if render_from_callback_handle(&user_data.render_callback, info, out).is_err() {
                    out.fill(0.0);
                }

                match user_data.format {
                    SampleFormat::F32 => {
                        for (sample, bytes) in out.iter().zip(slice.chunks_exact_mut(4)) {
                            bytes.copy_from_slice(&sample.to_le_bytes());
                        }
                    }
                    SampleFormat::I16 => {
                        for (sample, bytes) in out.iter().zip(slice.chunks_exact_mut(2)) {
                            bytes.copy_from_slice(&f32_to_i16(*sample).to_le_bytes());
                        }
                    }
                }
                user_data
                    .metrics
                    .frames_written
                    .fetch_add(n_frames as i64, Ordering::Relaxed);
                n_frames
            } else {
                0
            };

            let chunk = data.chunk_mut();
            *chunk.offset_mut() = 0;
            *chunk.stride_mut() = stride as _;
            *chunk.size_mut() = (n_frames.saturating_mul(stride)) as _;
        })
        .register()
        .map_err(|e| AudioError::BackendFailure {
            code: map_pw_error(e),
        })?;

    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(match config.format {
        SampleFormat::F32 => spa::param::audio::AudioFormat::F32LE,
        SampleFormat::I16 => spa::param::audio::AudioFormat::S16LE,
    });
    audio_info.set_rate(config.sample_rate_hz.get());
    audio_info.set_channels(config.channels.get());
    let obj = pw::spa::pod::Object {
        type_: pw::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: pw::spa::param::ParamType::EnumFormat.as_raw(),
        properties: audio_info.into(),
    };
    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(obj),
    )
    .map_err(|_| AudioError::BackendFailure { code: -744 })?
    .0
    .into_inner();
    let mut params = [Pod::from_bytes(&values).ok_or(AudioError::BackendFailure { code: -745 })?];

    stream
        .connect(
            spa::utils::Direction::Output,
            None,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut params,
        )
        .map_err(|e| AudioError::BackendFailure {
            code: map_pw_error(e),
        })?;
    stream
        .set_active(true)
        .map_err(|e| AudioError::BackendFailure {
            code: map_pw_error(e),
        })?;
    let _ = ready_tx.send(Ok(()));

    while !stop.load(Ordering::Relaxed) {
        let _ = mainloop.loop_().iterate(Duration::from_millis(20));
    }

    let _ = stream.set_active(false);
    let _ = stream.disconnect();
    Ok(())
}

#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
fn run_input_worker(
    config: StreamConfig,
    capture_callback: SharedCaptureCallback,
    metrics: Arc<PipeWireMetrics>,
    stop: Arc<AtomicBool>,
    ready_tx: Sender<Result<()>>,
) -> Result<()> {
    let channels = config.channels.get() as usize;
    let bytes_per_sample = sample_bytes(config.format);
    let stride = channels.saturating_mul(bytes_per_sample);
    let initial_samples = (config.frames_per_burst.get() as usize).saturating_mul(channels);

    pw::init();
    let mainloop = pw::main_loop::MainLoop::new(None).map_err(|e| AudioError::BackendFailure {
        code: map_pw_error(e),
    })?;
    let context = pw::context::Context::new(&mainloop).map_err(|e| AudioError::BackendFailure {
        code: map_pw_error(e),
    })?;
    let core = context
        .connect(None)
        .map_err(|e| AudioError::BackendFailure {
            code: map_pw_error(e),
        })?;

    let stream = pw::stream::Stream::new(
        &core,
        "trombone-input",
        properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Music",
        },
    )
    .map_err(|e| AudioError::BackendFailure {
        code: map_pw_error(e),
    })?;

    let user_data = InputUserData {
        capture_callback,
        metrics: metrics.clone(),
        channels,
        format: config.format,
        scratch: vec![0.0_f32; initial_samples.max(1)],
    };

    let _listener = stream
        .add_local_listener_with_user_data(user_data)
        .process(move |stream, user_data| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let datas = buffer.datas_mut();
            if datas.is_empty() || stride == 0 {
                return;
            }
            let data = &mut datas[0];
            let chunk_size = data.chunk().size() as usize;
            let Some(slice) = data.data() else {
                return;
            };
            let bytes_len = chunk_size.min(slice.len());
            let n_frames = bytes_len / stride;
            if n_frames == 0 {
                return;
            }
            let sample_count = n_frames.saturating_mul(user_data.channels);
            if sample_count > user_data.scratch.len() {
                user_data.scratch.resize(sample_count, 0.0);
            }
            let input = &mut user_data.scratch[..sample_count];
            match user_data.format {
                SampleFormat::F32 => {
                    for (dst, bytes) in input.iter_mut().zip(slice[..bytes_len].chunks_exact(4)) {
                        *dst = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                    }
                }
                SampleFormat::I16 => {
                    for (dst, bytes) in input.iter_mut().zip(slice[..bytes_len].chunks_exact(2)) {
                        let sample = i16::from_le_bytes([bytes[0], bytes[1]]);
                        *dst = i16_to_f32(sample);
                    }
                }
            }

            let callback_time_ns = unix_time_ns();
            user_data
                .metrics
                .callback_time_ns
                .store(callback_time_ns, Ordering::Relaxed);
            let info = CallbackInfo {
                callback_time_ns,
                frames: n_frames as u32,
            };
            let _ = capture_from_callback_handle(&user_data.capture_callback, info, input);
            user_data
                .metrics
                .frames_read
                .fetch_add(n_frames as i64, Ordering::Relaxed);
        })
        .register()
        .map_err(|e| AudioError::BackendFailure {
            code: map_pw_error(e),
        })?;

    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(match config.format {
        SampleFormat::F32 => spa::param::audio::AudioFormat::F32LE,
        SampleFormat::I16 => spa::param::audio::AudioFormat::S16LE,
    });
    audio_info.set_rate(config.sample_rate_hz.get());
    audio_info.set_channels(config.channels.get());
    let obj = pw::spa::pod::Object {
        type_: pw::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: pw::spa::param::ParamType::EnumFormat.as_raw(),
        properties: audio_info.into(),
    };
    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(obj),
    )
    .map_err(|_| AudioError::BackendFailure { code: -744 })?
    .0
    .into_inner();
    let mut params = [Pod::from_bytes(&values).ok_or(AudioError::BackendFailure { code: -745 })?];

    stream
        .connect(
            spa::utils::Direction::Input,
            None,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut params,
        )
        .map_err(|e| AudioError::BackendFailure {
            code: map_pw_error(e),
        })?;
    stream
        .set_active(true)
        .map_err(|e| AudioError::BackendFailure {
            code: map_pw_error(e),
        })?;
    let _ = ready_tx.send(Ok(()));

    while !stop.load(Ordering::Relaxed) {
        let _ = mainloop.loop_().iterate(Duration::from_millis(20));
    }

    let _ = stream.set_active(false);
    let _ = stream.disconnect();
    Ok(())
}

#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
fn map_pw_error(error: pw::Error) -> i32 {
    match error {
        pw::Error::CreationFailed => -740,
        pw::Error::NoMemory => -741,
        pw::Error::WrongProxyType => -742,
        pw::Error::SpaError(_) => -743,
    }
}

#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
fn sample_bytes(format: SampleFormat) -> usize {
    match format {
        SampleFormat::F32 => 4,
        SampleFormat::I16 => 2,
    }
}

#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
fn f32_to_i16(sample: f32) -> i16 {
    let clamped = sample.clamp(-1.0, 1.0);
    (clamped * 32767.0).round() as i16
}

#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
fn i16_to_f32(sample: i16) -> f32 {
    sample as f32 / 32768.0
}

#[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
fn unix_time_ns() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(delta) => delta.as_nanos().min(u128::from(u64::MAX)) as u64,
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::validate_requested_config;
    use crate::core::config::{SampleFormat, StreamConfig};

    #[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
    use super::{create_stream, libpipewire_available};
    #[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
    use crate::core::config::Direction;

    #[test]
    fn validate_accepts_supported_formats() {
        for format in [SampleFormat::F32, SampleFormat::I16] {
            let config = StreamConfig {
                format,
                ..StreamConfig::default()
            };
            assert!(validate_requested_config(config).is_ok());
        }
    }

    #[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
    #[test]
    fn input_stream_can_be_constructed() {
        let config = StreamConfig {
            direction: Direction::Input,
            ..StreamConfig::default()
        };
        let _ = create_stream(config);
    }

    #[cfg(all(target_os = "linux", feature = "linux-pipewire"))]
    #[test]
    fn lib_probe_is_safe_to_call() {
        let _ = libpipewire_available();
    }
}
