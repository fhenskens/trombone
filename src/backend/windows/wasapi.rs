//! WASAPI backend.

#[cfg(target_os = "windows")]
use crate::core::config::Direction;
use crate::core::config::{SampleFormat, StreamConfig};
use crate::core::error::{AudioError, Result};
use crate::core::stream::Stream;

#[cfg(target_os = "windows")]
use crate::core::callback::CallbackInfo;
#[cfg(target_os = "windows")]
use crate::core::metrics::{NegotiatedSampleFormat, NegotiatedShareMode, StreamMetrics};
#[cfg(target_os = "windows")]
use crate::core::stream::{
    SharedCaptureCallback, SharedRenderCallback, StreamBackendOps, capture_from_callback_handle,
    new_capture_callback_handle, new_render_callback_handle, render_from_callback_handle,
};
#[cfg(target_os = "windows")]
use std::sync::Arc;
#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU8, AtomicU64, Ordering};
#[cfg(target_os = "windows")]
use std::thread::{self, JoinHandle};
#[cfg(target_os = "windows")]
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(target_os = "windows")]
use windows::Win32::Media::Audio::{
    AUDCLNT_SHAREMODE_EXCLUSIVE, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
    IAudioClient, WAVE_FORMAT_PCM, WAVEFORMATEX,
};
#[cfg(target_os = "windows")]
use windows::Win32::Media::Multimedia::WAVE_FORMAT_IEEE_FLOAT;

/// Create a stream backed by WASAPI.
#[cfg(target_os = "windows")]
pub fn create_stream(config: StreamConfig) -> Result<Stream> {
    validate_requested_config(config)?;
    probe_default_endpoint(config.direction)?;

    let render_callback: SharedRenderCallback = new_render_callback_handle();
    let capture_callback: SharedCaptureCallback = new_capture_callback_handle();
    let backend =
        WasapiBackendStream::new(config, render_callback.clone(), capture_callback.clone());

    Ok(Stream::with_backend_and_callback(
        config,
        Box::new(backend),
        render_callback,
        capture_callback,
    ))
}

/// Create a stream backed by WASAPI.
#[cfg(not(target_os = "windows"))]
pub fn create_stream(config: StreamConfig) -> Result<Stream> {
    validate_requested_config(config)?;
    Err(AudioError::NotImplemented)
}

fn validate_requested_config(config: StreamConfig) -> Result<()> {
    match config.format {
        SampleFormat::F32 | SampleFormat::I16 => Ok(()),
    }
}

#[cfg(target_os = "windows")]
fn map_win_error(e: windows::core::Error) -> AudioError {
    AudioError::BackendFailure { code: e.code().0 }
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WasapiMode {
    Auto,
    Shared,
    Exclusive,
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShareModeChoice {
    Shared,
    Exclusive,
}

#[cfg(target_os = "windows")]
impl ShareModeChoice {
    fn label(self) -> &'static str {
        match self {
            ShareModeChoice::Shared => "shared",
            ShareModeChoice::Exclusive => "exclusive",
        }
    }

    fn as_metrics(self) -> NegotiatedShareMode {
        match self {
            ShareModeChoice::Shared => NegotiatedShareMode::Shared,
            ShareModeChoice::Exclusive => NegotiatedShareMode::Exclusive,
        }
    }
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug)]
struct ClientInitResult {
    buffer_frames: u32,
    share_mode: ShareModeChoice,
    sample_format: WasapiSampleFormat,
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WasapiSampleFormat {
    F32,
    I16,
}

#[cfg(target_os = "windows")]
impl WasapiSampleFormat {
    fn from_requested(format: SampleFormat) -> Self {
        match format {
            SampleFormat::F32 => WasapiSampleFormat::F32,
            SampleFormat::I16 => WasapiSampleFormat::I16,
        }
    }

    fn alternate(self) -> Self {
        match self {
            WasapiSampleFormat::F32 => WasapiSampleFormat::I16,
            WasapiSampleFormat::I16 => WasapiSampleFormat::F32,
        }
    }

    fn bytes_per_sample(self) -> usize {
        match self {
            WasapiSampleFormat::F32 => 4,
            WasapiSampleFormat::I16 => 2,
        }
    }

    fn bits_per_sample(self) -> u16 {
        (self.bytes_per_sample() * 8) as u16
    }

    fn wave_format_tag(self) -> u16 {
        match self {
            WasapiSampleFormat::F32 => WAVE_FORMAT_IEEE_FLOAT as u16,
            WasapiSampleFormat::I16 => WAVE_FORMAT_PCM as u16,
        }
    }

    fn label(self) -> &'static str {
        match self {
            WasapiSampleFormat::F32 => "f32",
            WasapiSampleFormat::I16 => "i16",
        }
    }

    fn as_metrics(self) -> NegotiatedSampleFormat {
        match self {
            WasapiSampleFormat::F32 => NegotiatedSampleFormat::F32,
            WasapiSampleFormat::I16 => NegotiatedSampleFormat::I16,
        }
    }
}

#[cfg(target_os = "windows")]
const NEGOTIATED_UNKNOWN: u8 = 0;
#[cfg(target_os = "windows")]
const NEGOTIATED_SHARED: u8 = 1;
#[cfg(target_os = "windows")]
const NEGOTIATED_EXCLUSIVE: u8 = 2;
#[cfg(target_os = "windows")]
const NEGOTIATED_F32: u8 = 1;
#[cfg(target_os = "windows")]
const NEGOTIATED_I16: u8 = 2;

#[cfg(target_os = "windows")]
fn backend_debug_enabled() -> bool {
    std::env::var_os("TROMBONE_BACKEND_DEBUG").is_some()
        || std::env::var_os("TROMBONE_DEBUG_BACKEND").is_some()
}

#[cfg(target_os = "windows")]
fn parse_wasapi_mode() -> WasapiMode {
    match std::env::var("TROMBONE_WASAPI_MODE") {
        Ok(raw) => match raw.trim().to_ascii_lowercase().as_str() {
            "shared" => WasapiMode::Shared,
            "exclusive" => WasapiMode::Exclusive,
            _ => WasapiMode::Auto,
        },
        Err(_) => WasapiMode::Auto,
    }
}

#[cfg(target_os = "windows")]
fn frames_to_hns(frames: u32, sample_rate_hz: u32) -> i64 {
    if sample_rate_hz == 0 {
        return 0;
    }
    ((frames as i64) * 10_000_000 / (sample_rate_hz as i64)).max(1)
}

#[cfg(target_os = "windows")]
fn initialize_audio_client(
    audio_client: &IAudioClient,
    config: StreamConfig,
    direction: Direction,
) -> Result<ClientInitResult> {
    let mode = parse_wasapi_mode();
    let attempts: &[ShareModeChoice] = match mode {
        WasapiMode::Exclusive => &[ShareModeChoice::Exclusive, ShareModeChoice::Shared],
        WasapiMode::Shared => &[ShareModeChoice::Shared],
        WasapiMode::Auto => &[ShareModeChoice::Exclusive, ShareModeChoice::Shared],
    };

    let mut last_error = AudioError::BackendFailure { code: -1 };
    let requested_format = WasapiSampleFormat::from_requested(config.format);

    for attempt in attempts {
        for sample_format in [requested_format, requested_format.alternate()] {
            match initialize_audio_client_for_mode(audio_client, config, *attempt, sample_format) {
                Ok(buffer_frames) => {
                    if backend_debug_enabled() {
                        eprintln!(
                            "WASAPI {direction:?} initialized in {} mode, format={}, buffer_frames={buffer_frames}",
                            attempt.label(),
                            sample_format.label()
                        );
                    }
                    return Ok(ClientInitResult {
                        buffer_frames,
                        share_mode: *attempt,
                        sample_format,
                    });
                }
                Err(err) => {
                    if backend_debug_enabled() {
                        eprintln!(
                            "WASAPI {direction:?} init failed in {} mode, format={}: {err:?}",
                            attempt.label(),
                            sample_format.label()
                        );
                    }
                    last_error = err;
                }
            }
        }
    }

    Err(last_error)
}

#[cfg(target_os = "windows")]
fn initialize_audio_client_for_mode(
    audio_client: &IAudioClient,
    config: StreamConfig,
    mode: ShareModeChoice,
    sample_format: WasapiSampleFormat,
) -> Result<u32> {
    let sample_rate = config.sample_rate_hz.get();
    let channels = config.channels.get();
    let period_hns = frames_to_hns(config.frames_per_burst.get(), sample_rate).max(10_000);
    let wave = build_wave_format(sample_format, channels, sample_rate);

    let (share_mode, buffer_hns, periodicity_hns) = match mode {
        ShareModeChoice::Shared => (
            AUDCLNT_SHAREMODE_SHARED,
            (period_hns * 4).max(20_000),
            0_i64,
        ),
        ShareModeChoice::Exclusive => (AUDCLNT_SHAREMODE_EXCLUSIVE, period_hns, period_hns),
    };

    if mode == ShareModeChoice::Exclusive {
        probe_format_support(audio_client, share_mode, &wave)?;
    }

    // SAFETY: `audio_client` is a valid activated IAudioClient, and `wave`
    // points to a stack-local format structure alive across the call.
    unsafe {
        audio_client
            .Initialize(
                share_mode,
                AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                buffer_hns,
                periodicity_hns,
                &wave,
                None,
            )
            .map_err(map_win_error)?;
        audio_client.GetBufferSize().map_err(map_win_error)
    }
}

#[cfg(target_os = "windows")]
fn build_wave_format(format: WasapiSampleFormat, channels: u32, sample_rate: u32) -> WAVEFORMATEX {
    let bytes_per_sample = format.bytes_per_sample() as u32;
    let block_align = (channels * bytes_per_sample) as u16;
    let avg_bytes_per_sec = sample_rate * channels * bytes_per_sample;
    WAVEFORMATEX {
        wFormatTag: format.wave_format_tag(),
        nChannels: channels as u16,
        nSamplesPerSec: sample_rate,
        nAvgBytesPerSec: avg_bytes_per_sec,
        nBlockAlign: block_align,
        wBitsPerSample: format.bits_per_sample(),
        cbSize: 0,
    }
}

#[cfg(target_os = "windows")]
fn probe_format_support(
    audio_client: &IAudioClient,
    share_mode: windows::Win32::Media::Audio::AUDCLNT_SHAREMODE,
    wave: &WAVEFORMATEX,
) -> Result<()> {
    let mut closest_match: *mut WAVEFORMATEX = std::ptr::null_mut();
    // SAFETY: `audio_client` is valid and pointers are valid for the call.
    let hr = unsafe {
        audio_client.IsFormatSupported(
            share_mode,
            wave as *const _,
            Some(&mut closest_match as *mut *mut WAVEFORMATEX),
        )
    };
    if !closest_match.is_null() {
        // SAFETY: Memory was allocated by COM for IsFormatSupported closest match.
        unsafe { windows::Win32::System::Com::CoTaskMemFree(Some(closest_match as *const _)) };
    }
    if hr.is_ok() {
        Ok(())
    } else {
        Err(AudioError::BackendFailure { code: hr.0 })
    }
}

#[cfg(target_os = "windows")]
fn probe_default_endpoint(direction: Direction) -> Result<()> {
    use windows::Win32::Media::Audio::{
        IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eConsole, eRender,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    };

    let flow = match direction {
        Direction::Output => eRender,
        Direction::Input => eCapture,
    };

    unsafe {
        let init_hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        if init_hr.is_err() {
            return Err(AudioError::BackendFailure { code: init_hr.0 });
        }

        let result = (|| {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(map_win_error)?;
            let _device = enumerator
                .GetDefaultAudioEndpoint(flow, eConsole)
                .map_err(map_win_error)?;
            Ok(())
        })();

        CoUninitialize();
        result
    }
}

#[cfg(target_os = "windows")]
struct WasapiMetrics {
    callback_time_ns: AtomicU64,
    frames_processed: AtomicI64,
    last_padding_frames: AtomicI64,
    negotiated_share_mode: AtomicU8,
    negotiated_sample_format: AtomicU8,
}

#[cfg(target_os = "windows")]
impl Default for WasapiMetrics {
    fn default() -> Self {
        Self {
            callback_time_ns: AtomicU64::new(0),
            frames_processed: AtomicI64::new(0),
            last_padding_frames: AtomicI64::new(0),
            negotiated_share_mode: AtomicU8::new(NEGOTIATED_UNKNOWN),
            negotiated_sample_format: AtomicU8::new(NEGOTIATED_UNKNOWN),
        }
    }
}

#[cfg(target_os = "windows")]
struct WorkerState {
    stop: Arc<AtomicBool>,
    join: JoinHandle<()>,
}

#[cfg(target_os = "windows")]
struct WasapiBackendStream {
    config: StreamConfig,
    direction: Direction,
    render_callback: SharedRenderCallback,
    capture_callback: SharedCaptureCallback,
    metrics: Arc<WasapiMetrics>,
    worker: Option<WorkerState>,
}

#[cfg(target_os = "windows")]
impl WasapiBackendStream {
    fn new(
        config: StreamConfig,
        render_callback: SharedRenderCallback,
        capture_callback: SharedCaptureCallback,
    ) -> Self {
        Self {
            direction: config.direction,
            config,
            render_callback,
            capture_callback,
            metrics: Arc::new(WasapiMetrics::default()),
            worker: None,
        }
    }
}

#[cfg(target_os = "windows")]
impl StreamBackendOps for WasapiBackendStream {
    fn start(&mut self) -> Result<()> {
        if self.worker.is_some() {
            return Ok(());
        }

        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = stop.clone();
        let render_callback = self.render_callback.clone();
        let capture_callback = self.capture_callback.clone();
        let metrics = self.metrics.clone();
        let config = self.config;
        let direction = self.direction;

        let join = thread::spawn(move || {
            let worker_result = match direction {
                Direction::Output => {
                    run_output_worker(config, render_callback, metrics, stop_for_thread)
                }
                Direction::Input => {
                    run_input_worker(config, capture_callback, metrics, stop_for_thread)
                }
            };
            if let Err(err) = worker_result {
                eprintln!("WASAPI worker error ({direction:?}): {err:?}");
            }
        });

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
        let frames = self.metrics.frames_processed.load(Ordering::Relaxed);
        let latency_frames_i64 = self
            .metrics
            .last_padding_frames
            .load(Ordering::Relaxed)
            .max(0);
        let latency_frames = u32::try_from(latency_frames_i64).ok();
        let latency_ns = latency_frames.map(|v| frames_to_ns(v, self.config.sample_rate_hz.get()));
        let negotiated_share_mode = match self.metrics.negotiated_share_mode.load(Ordering::Relaxed)
        {
            NEGOTIATED_SHARED => Some(NegotiatedShareMode::Shared),
            NEGOTIATED_EXCLUSIVE => Some(NegotiatedShareMode::Exclusive),
            _ => None,
        };
        let negotiated_sample_format = match self
            .metrics
            .negotiated_sample_format
            .load(Ordering::Relaxed)
        {
            NEGOTIATED_F32 => Some(NegotiatedSampleFormat::F32),
            NEGOTIATED_I16 => Some(NegotiatedSampleFormat::I16),
            _ => None,
        };
        StreamMetrics {
            xrun_count: 0,
            frames_written: (self.direction == Direction::Output).then_some(frames),
            frames_read: (self.direction == Direction::Input).then_some(frames),
            timing: crate::core::metrics::StreamTiming {
                callback_time_ns: match self.metrics.callback_time_ns.load(Ordering::Relaxed) {
                    0 => None,
                    value => Some(value),
                },
                estimated_latency_frames: latency_frames,
                estimated_latency_ns: latency_ns,
                negotiated_share_mode,
                negotiated_sample_format,
                ..Default::default()
            },
        }
    }

    fn close(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(target_os = "windows")]
fn run_output_worker(
    config: StreamConfig,
    render_callback: SharedRenderCallback,
    metrics: Arc<WasapiMetrics>,
    stop: Arc<AtomicBool>,
) -> Result<()> {
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows::Win32::Media::Audio::{
        IAudioRenderClient, IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    };
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

    unsafe {
        let init_hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        if init_hr.is_err() {
            return Err(AudioError::BackendFailure { code: init_hr.0 });
        }

        let result = (|| {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(map_win_error)?;
            let device = enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(map_win_error)?;
            let audio_client: IAudioClient =
                device.Activate(CLSCTX_ALL, None).map_err(map_win_error)?;

            let channels = config.channels.get();
            let init = initialize_audio_client(&audio_client, config, Direction::Output)?;
            let buffer_frames = init.buffer_frames;
            let share_mode = init.share_mode;
            let sample_format = init.sample_format;
            metrics.negotiated_share_mode.store(
                match share_mode.as_metrics() {
                    NegotiatedShareMode::Shared => NEGOTIATED_SHARED,
                    NegotiatedShareMode::Exclusive => NEGOTIATED_EXCLUSIVE,
                },
                Ordering::Relaxed,
            );
            metrics.negotiated_sample_format.store(
                match sample_format.as_metrics() {
                    NegotiatedSampleFormat::F32 => NEGOTIATED_F32,
                    NegotiatedSampleFormat::I16 => NEGOTIATED_I16,
                },
                Ordering::Relaxed,
            );
            let render_client: IAudioRenderClient =
                audio_client.GetService().map_err(map_win_error)?;
            let event = CreateEventW(None, false, false, None).map_err(map_win_error)?;
            audio_client.SetEventHandle(event).map_err(map_win_error)?;
            let mut convert_scratch = Vec::<f32>::new();

            // Prime buffer before starting.
            fill_available(
                &audio_client,
                &render_client,
                channels as usize,
                share_mode,
                sample_format,
                &render_callback,
                &metrics,
                buffer_frames,
                &mut convert_scratch,
            )?;

            audio_client.Start().map_err(map_win_error)?;

            while !stop.load(Ordering::Relaxed) {
                let wait = WaitForSingleObject(event, 50);
                if wait == WAIT_TIMEOUT {
                    continue;
                }
                if wait != WAIT_OBJECT_0 {
                    let _ = audio_client.Stop();
                    let _ = CloseHandle(event);
                    return Err(AudioError::BackendFailure { code: -2 });
                }
                fill_available(
                    &audio_client,
                    &render_client,
                    channels as usize,
                    share_mode,
                    sample_format,
                    &render_callback,
                    &metrics,
                    buffer_frames,
                    &mut convert_scratch,
                )?;
            }

            let _ = audio_client.Stop();
            let _ = CloseHandle(event);
            Ok(())
        })();

        CoUninitialize();
        result
    }
}

#[cfg(target_os = "windows")]
fn run_input_worker(
    config: StreamConfig,
    capture_callback: SharedCaptureCallback,
    metrics: Arc<WasapiMetrics>,
    stop: Arc<AtomicBool>,
) -> Result<()> {
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows::Win32::Media::Audio::{
        IAudioCaptureClient, IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eConsole,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    };
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

    unsafe {
        let init_hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        if init_hr.is_err() {
            return Err(AudioError::BackendFailure { code: init_hr.0 });
        }

        let result = (|| {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(map_win_error)?;
            let device = enumerator
                .GetDefaultAudioEndpoint(eCapture, eConsole)
                .map_err(map_win_error)?;
            let audio_client: IAudioClient =
                device.Activate(CLSCTX_ALL, None).map_err(map_win_error)?;

            let channels = config.channels.get();
            let init = initialize_audio_client(&audio_client, config, Direction::Input)?;
            let sample_format = init.sample_format;
            metrics.negotiated_share_mode.store(
                match init.share_mode.as_metrics() {
                    NegotiatedShareMode::Shared => NEGOTIATED_SHARED,
                    NegotiatedShareMode::Exclusive => NEGOTIATED_EXCLUSIVE,
                },
                Ordering::Relaxed,
            );
            metrics.negotiated_sample_format.store(
                match sample_format.as_metrics() {
                    NegotiatedSampleFormat::F32 => NEGOTIATED_F32,
                    NegotiatedSampleFormat::I16 => NEGOTIATED_I16,
                },
                Ordering::Relaxed,
            );
            if matches!(init.share_mode, ShareModeChoice::Exclusive) {
                // In exclusive capture, buffer size can differ from requested frames.
                metrics
                    .last_padding_frames
                    .store(init.buffer_frames as i64, Ordering::Relaxed);
            }

            let capture_client: IAudioCaptureClient =
                audio_client.GetService().map_err(map_win_error)?;
            let event = CreateEventW(None, false, false, None).map_err(map_win_error)?;
            audio_client.SetEventHandle(event).map_err(map_win_error)?;
            let mut convert_scratch = Vec::<f32>::new();

            audio_client.Start().map_err(map_win_error)?;

            while !stop.load(Ordering::Relaxed) {
                let wait = WaitForSingleObject(event, 50);
                if wait == WAIT_TIMEOUT {
                    continue;
                }
                if wait != WAIT_OBJECT_0 {
                    let _ = audio_client.Stop();
                    let _ = CloseHandle(event);
                    return Err(AudioError::BackendFailure { code: -2 });
                }
                pull_available_capture(
                    &capture_client,
                    channels as usize,
                    sample_format,
                    &capture_callback,
                    &metrics,
                    &mut convert_scratch,
                )?;
            }

            let _ = audio_client.Stop();
            let _ = CloseHandle(event);
            Ok(())
        })();

        CoUninitialize();
        result
    }
}

#[cfg(target_os = "windows")]
fn fill_available(
    audio_client: &windows::Win32::Media::Audio::IAudioClient,
    render_client: &windows::Win32::Media::Audio::IAudioRenderClient,
    channels: usize,
    share_mode: ShareModeChoice,
    sample_format: WasapiSampleFormat,
    render_callback: &SharedRenderCallback,
    metrics: &WasapiMetrics,
    buffer_frames: u32,
    scratch: &mut Vec<f32>,
) -> Result<()> {
    let available_frames = match share_mode {
        ShareModeChoice::Exclusive => {
            metrics.last_padding_frames.store(0, Ordering::Relaxed);
            buffer_frames
        }
        ShareModeChoice::Shared => {
            // SAFETY: `audio_client` is a valid initialized COM interface for this thread.
            let padding = unsafe { audio_client.GetCurrentPadding() }.map_err(map_win_error)?;
            metrics
                .last_padding_frames
                .store(padding as i64, Ordering::Relaxed);
            buffer_frames.saturating_sub(padding)
        }
    };
    if available_frames == 0 {
        return Ok(());
    }

    let frame_count = available_frames as usize;
    let sample_count = frame_count.saturating_mul(channels);
    // SAFETY: `render_client` is valid and `available_frames` is from WASAPI buffer accounting.
    let data =
        unsafe { render_client.GetBuffer(available_frames) }.map_err(map_win_error)? as *mut u8;

    if data.is_null() {
        return Err(AudioError::BackendFailure { code: -1 });
    }

    let callback_time_ns = unix_time_ns();
    metrics
        .callback_time_ns
        .store(callback_time_ns, Ordering::Relaxed);
    let info = CallbackInfo {
        callback_time_ns,
        frames: available_frames,
    };

    match sample_format {
        WasapiSampleFormat::F32 => {
            // SAFETY: WASAPI buffer is interleaved f32 PCM in this format mode.
            let out = unsafe { std::slice::from_raw_parts_mut(data as *mut f32, sample_count) };
            if render_from_callback_handle(render_callback, info, out).is_err() {
                out.fill(0.0);
            }
        }
        WasapiSampleFormat::I16 => {
            scratch.resize(sample_count, 0.0);
            if render_from_callback_handle(render_callback, info, scratch).is_err() {
                scratch.fill(0.0);
            }
            // SAFETY: WASAPI buffer is interleaved i16 PCM in this format mode.
            let out_i16 = unsafe { std::slice::from_raw_parts_mut(data as *mut i16, sample_count) };
            for (src, dst) in scratch.iter().zip(out_i16.iter_mut()) {
                *dst = f32_to_i16(*src);
            }
        }
    }

    // SAFETY: Buffer was acquired with `GetBuffer(available_frames)` above.
    unsafe { render_client.ReleaseBuffer(available_frames, 0) }.map_err(map_win_error)?;
    metrics
        .frames_processed
        .fetch_add(available_frames as i64, Ordering::Relaxed);
    Ok(())
}

#[cfg(target_os = "windows")]
fn pull_available_capture(
    capture_client: &windows::Win32::Media::Audio::IAudioCaptureClient,
    channels: usize,
    sample_format: WasapiSampleFormat,
    capture_callback: &SharedCaptureCallback,
    metrics: &WasapiMetrics,
    scratch: &mut Vec<f32>,
) -> Result<()> {
    use windows::Win32::Media::Audio::AUDCLNT_BUFFERFLAGS_SILENT;

    loop {
        // SAFETY: Valid capture client on this thread.
        let packet_frames = unsafe { capture_client.GetNextPacketSize() }.map_err(map_win_error)?;
        metrics
            .last_padding_frames
            .store(packet_frames as i64, Ordering::Relaxed);
        if packet_frames == 0 {
            break;
        }

        let mut data_ptr: *mut u8 = std::ptr::null_mut();
        let mut num_frames: u32 = 0;
        let mut flags: u32 = 0;
        // SAFETY: Valid pointers and capture client; packet data is released below.
        unsafe {
            capture_client
                .GetBuffer(
                    &mut data_ptr as *mut _,
                    &mut num_frames as *mut _,
                    &mut flags as *mut _,
                    None,
                    None,
                )
                .map_err(map_win_error)?;
        }

        let sample_count = (num_frames as usize).saturating_mul(channels);
        let callback_time_ns = unix_time_ns();
        metrics
            .callback_time_ns
            .store(callback_time_ns, Ordering::Relaxed);
        let info = CallbackInfo {
            callback_time_ns,
            frames: num_frames,
        };

        let input: &[f32] = if (flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32)) != 0 {
            scratch.resize(sample_count, 0.0);
            &scratch
        } else if data_ptr.is_null() {
            &[]
        } else {
            match sample_format {
                WasapiSampleFormat::F32 => {
                    // SAFETY: Buffer is valid for `num_frames * channels` f32 samples until ReleaseBuffer.
                    unsafe { std::slice::from_raw_parts(data_ptr as *const f32, sample_count) }
                }
                WasapiSampleFormat::I16 => {
                    scratch.resize(sample_count, 0.0);
                    // SAFETY: Buffer is valid for `num_frames * channels` i16 samples until ReleaseBuffer.
                    let raw_i16 =
                        unsafe { std::slice::from_raw_parts(data_ptr as *const i16, sample_count) };
                    for (src, dst) in raw_i16.iter().zip(scratch.iter_mut()) {
                        *dst = i16_to_f32(*src);
                    }
                    &scratch
                }
            }
        };

        let _ = capture_from_callback_handle(capture_callback, info, input);
        // SAFETY: Matches previous `GetBuffer` call for this packet.
        unsafe { capture_client.ReleaseBuffer(num_frames) }.map_err(map_win_error)?;
        metrics
            .frames_processed
            .fetch_add(num_frames as i64, Ordering::Relaxed);
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn f32_to_i16(sample: f32) -> i16 {
    let clamped = sample.clamp(-1.0, 1.0);
    let scaled = clamped * 32767.0;
    scaled.round() as i16
}

#[cfg(target_os = "windows")]
fn i16_to_f32(sample: i16) -> f32 {
    sample as f32 / 32768.0
}

#[cfg(target_os = "windows")]
fn frames_to_ns(frames: u32, sample_rate_hz: u32) -> u64 {
    if sample_rate_hz == 0 {
        return 0;
    }
    ((frames as u128) * 1_000_000_000_u128 / (sample_rate_hz as u128)) as u64
}

#[cfg(target_os = "windows")]
fn unix_time_ns() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(delta) => delta.as_nanos().min(u128::from(u64::MAX)) as u64,
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::create_stream;
    use crate::core::config::{SampleFormat, StreamConfig};
    use crate::core::error::AudioError;

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn i16_config_is_accepted_on_non_windows_stub() {
        let config = StreamConfig {
            format: SampleFormat::I16,
            ..StreamConfig::default()
        };
        let result = create_stream(config);
        match result {
            Ok(_) => panic!("expected not implemented on non-windows"),
            Err(err) => assert_eq!(err, AudioError::NotImplemented),
        }
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn input_direction_is_not_implemented_on_non_windows() {
        let config = StreamConfig {
            direction: crate::core::config::Direction::Input,
            ..StreamConfig::default()
        };
        let result = create_stream(config);
        match result {
            Ok(_) => panic!("expected input path to be unimplemented on non-windows"),
            Err(err) => assert_eq!(err, AudioError::NotImplemented),
        }
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn non_windows_returns_not_implemented() {
        let result = create_stream(StreamConfig::default());
        match result {
            Ok(_) => panic!("expected not implemented on non-windows"),
            Err(err) => assert_eq!(err, AudioError::NotImplemented),
        }
    }
}
