//! ALSA backend (Linux).

use crate::core::config::{SampleFormat, StreamConfig};
use crate::core::error::{AudioError, Result};
use crate::core::stream::Stream;

#[cfg(target_os = "linux")]
use crate::core::callback::CallbackInfo;
#[cfg(target_os = "linux")]
use crate::core::config::Direction;
#[cfg(target_os = "linux")]
use crate::core::metrics::StreamMetrics;
#[cfg(target_os = "linux")]
use crate::core::stream::{
    SharedCaptureCallback, SharedRenderCallback, StreamBackendOps, new_capture_callback_handle,
    new_render_callback_handle, render_from_callback_handle,
};
#[cfg(target_os = "linux")]
use std::sync::Arc;
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
#[cfg(target_os = "linux")]
use std::sync::mpsc::{self, Sender};
#[cfg(target_os = "linux")]
use std::thread::{self, JoinHandle};
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Create a stream backed by ALSA.
#[cfg(target_os = "linux")]
pub fn create_stream(config: StreamConfig) -> Result<Stream> {
    create_stream_with_preferred_device(config, None)
}

/// Create a stream backed by ALSA with a preferred device name.
#[cfg(target_os = "linux")]
pub fn create_stream_with_preferred_device(
    config: StreamConfig,
    preferred_device: Option<&'static str>,
) -> Result<Stream> {
    validate_requested_config(config)?;
    let render_callback: SharedRenderCallback = new_render_callback_handle();
    let capture_callback: SharedCaptureCallback = new_capture_callback_handle();
    let backend = AlsaBackendStream::new(
        config,
        preferred_device,
        render_callback.clone(),
        capture_callback.clone(),
    );
    Ok(Stream::with_backend_and_callback(
        config,
        Box::new(backend),
        render_callback,
        capture_callback,
    ))
}

/// Create a stream backed by ALSA.
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

#[cfg(target_os = "linux")]
#[derive(Default)]
struct AlsaMetrics {
    callback_time_ns: AtomicU64,
    frames_processed: AtomicI64,
    delay_frames: AtomicI64,
}

#[cfg(target_os = "linux")]
struct WorkerState {
    stop: Arc<AtomicBool>,
    join: JoinHandle<()>,
}

#[cfg(target_os = "linux")]
struct AlsaBackendStream {
    config: StreamConfig,
    preferred_device: Option<&'static str>,
    direction: Direction,
    render_callback: SharedRenderCallback,
    capture_callback: SharedCaptureCallback,
    metrics: Arc<AlsaMetrics>,
    worker: Option<WorkerState>,
}

#[cfg(target_os = "linux")]
impl AlsaBackendStream {
    fn new(
        config: StreamConfig,
        preferred_device: Option<&'static str>,
        render_callback: SharedRenderCallback,
        capture_callback: SharedCaptureCallback,
    ) -> Self {
        Self {
            config,
            preferred_device,
            direction: config.direction,
            render_callback,
            capture_callback,
            metrics: Arc::new(AlsaMetrics::default()),
            worker: None,
        }
    }
}

#[cfg(target_os = "linux")]
impl StreamBackendOps for AlsaBackendStream {
    fn start(&mut self) -> Result<()> {
        if self.worker.is_some() {
            return Ok(());
        }

        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = stop.clone();
        let render_callback = self.render_callback.clone();
        let capture_callback = self.capture_callback.clone();
        let config = self.config;
        let preferred_device = self.preferred_device;
        let direction = self.direction;
        let metrics = self.metrics.clone();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<()>>();

        let join = thread::spawn(move || {
            let worker_result = match direction {
                Direction::Output => run_output_worker(
                    config,
                    preferred_device,
                    render_callback,
                    metrics,
                    stop_for_thread,
                    ready_tx,
                ),
                Direction::Input => run_input_worker(
                    config,
                    preferred_device,
                    capture_callback,
                    metrics,
                    stop_for_thread,
                    ready_tx,
                ),
            };
            if let Err(err) = worker_result {
                eprintln!("ALSA worker error: {err:?}");
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
        let frames = self.metrics.frames_processed.load(Ordering::Relaxed);
        let latency_frames =
            u32::try_from(self.metrics.delay_frames.load(Ordering::Relaxed).max(0)).ok();
        let latency_ns = latency_frames.map(|v| frames_to_ns(v, self.config.sample_rate_hz.get()));
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
                ..Default::default()
            },
        }
    }

    fn close(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(target_os = "linux")]
fn run_output_worker(
    config: StreamConfig,
    preferred_device: Option<&'static str>,
    render_callback: SharedRenderCallback,
    metrics: Arc<AlsaMetrics>,
    stop: Arc<AtomicBool>,
    ready_tx: Sender<Result<()>>,
) -> Result<()> {
    use alsa::ValueOr;
    use alsa::pcm::{Access, Format, HwParams, State};
    use libc::{EPIPE, ESTRPIPE};

    let pcm = open_alsa_pcm_for(Direction::Output, preferred_device)
        .map_err(|e| AudioError::BackendFailure { code: e.errno() })?;

    {
        let hwp =
            HwParams::any(&pcm).map_err(|e| AudioError::BackendFailure { code: e.errno() })?;
        hwp.set_access(Access::RWInterleaved)
            .map_err(|e| AudioError::BackendFailure { code: e.errno() })?;
        hwp.set_channels(config.channels.get())
            .map_err(|e| AudioError::BackendFailure { code: e.errno() })?;
        let actual_rate = hwp
            .set_rate_near(config.sample_rate_hz.get(), ValueOr::Nearest)
            .map_err(|e| AudioError::BackendFailure { code: e.errno() })?;
        if actual_rate == 0 {
            return Err(AudioError::BackendFailure { code: -1 });
        }
        let format = match config.format {
            SampleFormat::F32 => Format::float(),
            SampleFormat::I16 => Format::s16(),
        };
        hwp.set_format(format)
            .map_err(|e| AudioError::BackendFailure { code: e.errno() })?;
        let period_frames = config.frames_per_burst.get() as i64;
        let _ = hwp
            .set_period_size_near(period_frames, ValueOr::Nearest)
            .map_err(|e| AudioError::BackendFailure { code: e.errno() })?;
        let _ = hwp
            .set_buffer_size_near(period_frames.saturating_mul(4))
            .map_err(|e| AudioError::BackendFailure { code: e.errno() })?;
        pcm.hw_params(&hwp)
            .map_err(|e| AudioError::BackendFailure { code: e.errno() })?;
    }

    pcm.prepare()
        .map_err(|e| AudioError::BackendFailure { code: e.errno() })?;
    let _ = ready_tx.send(Ok(()));

    let channels = config.channels.get() as usize;
    let frames = config.frames_per_burst.get() as usize;
    let callback_period = Duration::from_nanos(frames_to_ns(
        config.frames_per_burst.get(),
        config.sample_rate_hz.get(),
    ));
    let mut next_wake = Instant::now();
    let sample_count = frames.saturating_mul(channels);
    let mut f32_buffer = vec![0.0_f32; sample_count];
    let mut i16_buffer = vec![0_i16; sample_count];

    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        sleep_until_or_stop(next_wake, &stop);
        if stop.load(Ordering::Relaxed) {
            break;
        }
        next_wake = next_wake
            .checked_add(callback_period)
            .unwrap_or_else(Instant::now);

        let callback_time_ns = unix_time_ns();
        metrics
            .callback_time_ns
            .store(callback_time_ns, Ordering::Relaxed);
        let info = CallbackInfo {
            callback_time_ns,
            frames: frames as u32,
        };

        if render_from_callback_handle(&render_callback, info, &mut f32_buffer).is_err() {
            f32_buffer.fill(0.0);
        }

        let mut written_frames = 0usize;
        while written_frames < frames {
            let wrote = match config.format {
                SampleFormat::F32 => {
                    let io = pcm
                        .io_f32()
                        .map_err(|e| AudioError::BackendFailure { code: e.errno() })?;
                    io.writei(&f32_buffer[written_frames * channels..])
                }
                SampleFormat::I16 => {
                    for (src, dst) in f32_buffer.iter().zip(i16_buffer.iter_mut()) {
                        *dst = f32_to_i16(*src);
                    }
                    let io = pcm
                        .io_i16()
                        .map_err(|e| AudioError::BackendFailure { code: e.errno() })?;
                    io.writei(&i16_buffer[written_frames * channels..])
                }
            };

            match wrote {
                Ok(frames_written_now) => {
                    if frames_written_now == 0 {
                        break;
                    }
                    written_frames = written_frames.saturating_add(frames_written_now);
                }
                Err(err) if err.errno() == EPIPE || err.errno() == ESTRPIPE => {
                    let _ = pcm.prepare();
                    if pcm.state() == State::XRun {
                        let _ = pcm.prepare();
                    }
                }
                Err(err) => {
                    let _ = pcm.try_recover(err, true);
                    return Err(AudioError::BackendFailure { code: err.errno() });
                }
            }
        }

        metrics
            .frames_processed
            .fetch_add(written_frames as i64, Ordering::Relaxed);
        if let Ok(delay) = pcm.delay() {
            metrics.delay_frames.store(delay.max(0), Ordering::Relaxed);
        }
    }

    let _ = pcm.drain();
    Ok(())
}

#[cfg(target_os = "linux")]
fn run_input_worker(
    config: StreamConfig,
    preferred_device: Option<&'static str>,
    capture_callback: SharedCaptureCallback,
    metrics: Arc<AlsaMetrics>,
    stop: Arc<AtomicBool>,
    ready_tx: Sender<Result<()>>,
) -> Result<()> {
    use alsa::ValueOr;
    use alsa::pcm::{Access, Format, HwParams, State};
    use libc::{EPIPE, ESTRPIPE};

    let pcm = open_alsa_pcm_for(Direction::Input, preferred_device)
        .map_err(|e| AudioError::BackendFailure { code: e.errno() })?;

    {
        let hwp =
            HwParams::any(&pcm).map_err(|e| AudioError::BackendFailure { code: e.errno() })?;
        hwp.set_access(Access::RWInterleaved)
            .map_err(|e| AudioError::BackendFailure { code: e.errno() })?;
        hwp.set_channels(config.channels.get())
            .map_err(|e| AudioError::BackendFailure { code: e.errno() })?;
        let actual_rate = hwp
            .set_rate_near(config.sample_rate_hz.get(), ValueOr::Nearest)
            .map_err(|e| AudioError::BackendFailure { code: e.errno() })?;
        if actual_rate == 0 {
            return Err(AudioError::BackendFailure { code: -1 });
        }
        let format = match config.format {
            SampleFormat::F32 => Format::float(),
            SampleFormat::I16 => Format::s16(),
        };
        hwp.set_format(format)
            .map_err(|e| AudioError::BackendFailure { code: e.errno() })?;
        let period_frames = config.frames_per_burst.get() as i64;
        let _ = hwp
            .set_period_size_near(period_frames, ValueOr::Nearest)
            .map_err(|e| AudioError::BackendFailure { code: e.errno() })?;
        let _ = hwp
            .set_buffer_size_near(period_frames.saturating_mul(4))
            .map_err(|e| AudioError::BackendFailure { code: e.errno() })?;
        pcm.hw_params(&hwp)
            .map_err(|e| AudioError::BackendFailure { code: e.errno() })?;
    }

    pcm.prepare()
        .map_err(|e| AudioError::BackendFailure { code: e.errno() })?;
    let _ = ready_tx.send(Ok(()));

    let channels = config.channels.get() as usize;
    let frames = config.frames_per_burst.get() as usize;
    let sample_count = frames.saturating_mul(channels);
    let mut f32_buffer = vec![0.0_f32; sample_count];
    let mut i16_buffer = vec![0_i16; sample_count];

    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if !wait_for_available_frames(&pcm, frames as i64, &stop)? {
            break;
        }

        let read_frames = match config.format {
            SampleFormat::F32 => {
                let io = pcm
                    .io_f32()
                    .map_err(|e| AudioError::BackendFailure { code: e.errno() })?;
                match io.readi(&mut f32_buffer) {
                    Ok(fr) => fr,
                    Err(err) if err.errno() == EPIPE || err.errno() == ESTRPIPE => {
                        let _ = pcm.prepare();
                        if pcm.state() == State::XRun {
                            let _ = pcm.prepare();
                        }
                        continue;
                    }
                    Err(err) => {
                        let _ = pcm.try_recover(err, true);
                        return Err(AudioError::BackendFailure { code: err.errno() });
                    }
                }
            }
            SampleFormat::I16 => {
                let io = pcm
                    .io_i16()
                    .map_err(|e| AudioError::BackendFailure { code: e.errno() })?;
                match io.readi(&mut i16_buffer) {
                    Ok(fr) => {
                        let samples = fr.saturating_mul(channels);
                        for (src, dst) in i16_buffer[..samples]
                            .iter()
                            .zip(f32_buffer[..samples].iter_mut())
                        {
                            *dst = i16_to_f32(*src);
                        }
                        fr
                    }
                    Err(err) if err.errno() == EPIPE || err.errno() == ESTRPIPE => {
                        let _ = pcm.prepare();
                        if pcm.state() == State::XRun {
                            let _ = pcm.prepare();
                        }
                        continue;
                    }
                    Err(err) => {
                        let _ = pcm.try_recover(err, true);
                        return Err(AudioError::BackendFailure { code: err.errno() });
                    }
                }
            }
        };

        let callback_time_ns = unix_time_ns();
        metrics
            .callback_time_ns
            .store(callback_time_ns, Ordering::Relaxed);
        let info = CallbackInfo {
            callback_time_ns,
            frames: read_frames as u32,
        };
        let samples = read_frames.saturating_mul(channels);
        let _ = crate::core::stream::capture_from_callback_handle(
            &capture_callback,
            info,
            &f32_buffer[..samples],
        );
        metrics
            .frames_processed
            .fetch_add(read_frames as i64, Ordering::Relaxed);
        if let Ok(delay) = pcm.delay() {
            metrics.delay_frames.store(delay.max(0), Ordering::Relaxed);
        }
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn wait_for_available_frames(pcm: &alsa::PCM, min_frames: i64, stop: &AtomicBool) -> Result<bool> {
    use alsa::pcm::State;
    use libc::{EPIPE, ESTRPIPE};

    while !stop.load(Ordering::Relaxed) {
        match pcm.avail_update() {
            Ok(available_frames) if available_frames >= min_frames => return Ok(true),
            Ok(_) => {
                let _ = pcm
                    .wait(Some(100))
                    .map_err(|e| AudioError::BackendFailure { code: e.errno() })?;
            }
            Err(err) if err.errno() == EPIPE || err.errno() == ESTRPIPE => {
                let _ = pcm.prepare();
                if pcm.state() == State::XRun {
                    let _ = pcm.prepare();
                }
            }
            Err(err) => {
                let code = err.errno();
                let _ = pcm.try_recover(err, true);
                return Err(AudioError::BackendFailure { code });
            }
        }
    }
    Ok(false)
}

#[cfg(target_os = "linux")]
fn sleep_until_or_stop(deadline: Instant, stop: &AtomicBool) {
    while !stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        let remaining = deadline.saturating_duration_since(now);
        std::thread::sleep(remaining.min(Duration::from_millis(2)));
    }
}

#[cfg(target_os = "linux")]
fn open_alsa_pcm_for(
    direction: Direction,
    preferred_device: Option<&'static str>,
) -> alsa::Result<alsa::PCM> {
    use alsa::{Direction, PCM};
    let alsa_direction = match direction {
        crate::core::config::Direction::Output => Direction::Playback,
        crate::core::config::Direction::Input => Direction::Capture,
    };

    if let Ok(device) = std::env::var("TROMBONE_ALSA_DEVICE") {
        let trimmed = device.trim();
        if !trimmed.is_empty() {
            return PCM::new(trimmed, alsa_direction, false);
        }
    }

    let mut last_err = None;
    let mut candidates = Vec::<&str>::new();
    if let Some(device) = preferred_device {
        candidates.push(device);
    }
    candidates.extend(["default", "pipewire", "pulse"]);
    for candidate in candidates {
        match PCM::new(candidate, alsa_direction, false) {
            Ok(pcm) => return Ok(pcm),
            Err(err) => last_err = Some(err),
        }
    }

    Err(last_err.unwrap_or_else(|| alsa::Error::unsupported("snd_pcm_open")))
}

#[cfg(target_os = "linux")]
fn f32_to_i16(sample: f32) -> i16 {
    let clamped = sample.clamp(-1.0, 1.0);
    (clamped * 32767.0).round() as i16
}

#[cfg(target_os = "linux")]
fn i16_to_f32(sample: i16) -> f32 {
    sample as f32 / 32768.0
}

#[cfg(target_os = "linux")]
fn frames_to_ns(frames: u32, sample_rate_hz: u32) -> u64 {
    if sample_rate_hz == 0 {
        return 0;
    }
    ((frames as u128) * 1_000_000_000_u128 / (sample_rate_hz as u128)) as u64
}

#[cfg(target_os = "linux")]
fn unix_time_ns() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(delta) => delta.as_nanos().min(u128::from(u64::MAX)) as u64,
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::create_stream;
    use crate::core::config::{Direction, SampleFormat, StreamConfig};

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn non_linux_returns_not_implemented() {
        let result = create_stream(StreamConfig::default());
        match result {
            Ok(_) => panic!("expected not implemented on non-linux"),
            Err(err) => assert_eq!(err, crate::core::error::AudioError::NotImplemented),
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn input_config_is_accepted_on_linux() {
        let config = StreamConfig {
            direction: Direction::Input,
            ..StreamConfig::default()
        };
        assert!(create_stream(config).is_ok());
    }

    #[test]
    fn f32_and_i16_configs_are_accepted_for_output() {
        for format in [SampleFormat::F32, SampleFormat::I16] {
            let config = StreamConfig {
                format,
                ..StreamConfig::default()
            };
            #[cfg(target_os = "linux")]
            {
                // Stream create should succeed on Linux even without starting.
                assert!(create_stream(config).is_ok());
            }
            #[cfg(not(target_os = "linux"))]
            {
                assert_eq!(
                    create_stream(config),
                    Err(crate::core::error::AudioError::NotImplemented)
                );
            }
        }
    }
}
