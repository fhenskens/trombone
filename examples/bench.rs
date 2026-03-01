use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use trombone::backend::AudioBackend;
use trombone::backend::android::{AndroidBackend, AndroidBackendKind};
use trombone::core::callback::CallbackInfo;
use trombone::core::config::{Direction, StreamConfig};
use trombone::core::metrics::StreamMetrics;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BenchMode {
    Output,
    Input,
    Duplex,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputFormat {
    Human,
    Csv,
}

#[derive(Debug, Clone, Copy)]
struct BenchOptions {
    mode: BenchMode,
    backend: AndroidBackendKind,
    seconds: u64,
    sample_rate_hz: NonZeroU32,
    channels: NonZeroU32,
    frames_per_burst: NonZeroU32,
    freq_hz: f32,
    amp: f32,
    gain: f32,
    format: OutputFormat,
}

impl Default for BenchOptions {
    fn default() -> Self {
        Self {
            mode: BenchMode::Output,
            backend: AndroidBackendKind::Auto,
            seconds: 10,
            sample_rate_hz: NonZeroU32::new(48_000).expect("literal is non-zero"),
            channels: NonZeroU32::new(2).expect("literal is non-zero"),
            frames_per_burst: NonZeroU32::new(192).expect("literal is non-zero"),
            freq_hz: 440.0,
            amp: 0.2,
            gain: 1.0,
            format: OutputFormat::Human,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct IntervalSummary {
    samples: usize,
    avg_us: f64,
    min_us: f64,
    max_us: f64,
    p50_us: f64,
    p95_us: f64,
    p99_us: f64,
    trimmed_p95_us: f64,
    outliers_over_2x_median: usize,
    outliers_over_5x_median: usize,
}

#[derive(Default)]
struct TimingRecorder {
    last_instant: Option<Instant>,
    last_ns_tick: Option<u64>,
    deltas_ns: Vec<u64>,
}

impl TimingRecorder {
    fn percentile_ns(sorted_values: &[u64], percentile: f64) -> f64 {
        if sorted_values.is_empty() {
            return 0.0;
        }
        let count = sorted_values.len();
        let index = (((count as f64) * percentile).ceil() as usize)
            .saturating_sub(1)
            .min(count - 1);
        sorted_values[index] as f64
    }

    fn record_now(&mut self) {
        let now = Instant::now();
        if let Some(last) = self.last_instant {
            let delta = now
                .duration_since(last)
                .as_nanos()
                .min(u128::from(u64::MAX)) as u64;
            self.deltas_ns.push(delta);
        }
        self.last_instant = Some(now);
    }

    fn record_ns(&mut self, tick_ns: u64) {
        if let Some(last_tick) = self.last_ns_tick {
            let delta = tick_ns.saturating_sub(last_tick);
            if delta > 0 {
                self.deltas_ns.push(delta);
            }
        }
        self.last_ns_tick = Some(tick_ns);
    }

    fn summary(&self) -> Option<IntervalSummary> {
        if self.deltas_ns.is_empty() {
            return None;
        }
        let mut values = self.deltas_ns.clone();
        values.sort_unstable();
        let count = values.len();
        let sum: u128 = values.iter().map(|value| *value as u128).sum();
        let avg_ns = (sum as f64) / (count as f64);
        let min_ns = values[0] as f64;
        let max_ns = values[count - 1] as f64;
        let p50_ns = Self::percentile_ns(&values, 0.50);
        let p95_ns = Self::percentile_ns(&values, 0.95);
        let p99_ns = Self::percentile_ns(&values, 0.99);

        let two_x_median_ns = p50_ns * 2.0;
        let five_x_median_ns = p50_ns * 5.0;
        let outliers_over_2x_median = values
            .iter()
            .filter(|delta_ns| (**delta_ns as f64) > two_x_median_ns)
            .count();
        let outliers_over_5x_median = values
            .iter()
            .filter(|delta_ns| (**delta_ns as f64) > five_x_median_ns)
            .count();

        let trimmed_values: Vec<u64> = values
            .iter()
            .copied()
            .filter(|delta_ns| (*delta_ns as f64) <= two_x_median_ns)
            .collect();
        let trimmed_p95_ns = if trimmed_values.is_empty() {
            p95_ns
        } else {
            Self::percentile_ns(&trimmed_values, 0.95)
        };

        Some(IntervalSummary {
            samples: count,
            avg_us: avg_ns / 1_000.0,
            min_us: min_ns / 1_000.0,
            max_us: max_ns / 1_000.0,
            p50_us: p50_ns / 1_000.0,
            p95_us: p95_ns / 1_000.0,
            p99_us: p99_ns / 1_000.0,
            trimmed_p95_us: trimmed_p95_ns / 1_000.0,
            outliers_over_2x_median,
            outliers_over_5x_median,
        })
    }
}

#[derive(Debug)]
struct StreamBenchResult {
    callbacks: u64,
    samples: u64,
    callback_samples: u64,
    channels: u32,
    interval: Option<IntervalSummary>,
    backend_interval: Option<IntervalSummary>,
    metrics: StreamMetrics,
}

impl StreamBenchResult {
    fn observed_frames_per_callback(&self) -> Option<f64> {
        if self.callbacks == 0 || self.channels == 0 {
            return None;
        }
        Some(self.callback_samples as f64 / self.callbacks as f64 / self.channels as f64)
    }
}

#[derive(Debug)]
struct BenchResult {
    requested: StreamConfig,
    actual_output: Option<StreamConfig>,
    actual_input: Option<StreamConfig>,
    output: Option<StreamBenchResult>,
    input: Option<StreamBenchResult>,
    zero_filled_samples: Option<u64>,
}

fn print_help() {
    println!("Android audio benchmark options:");
    println!("  --mode <output|input|duplex>     benchmark mode (default: output)");
    println!("  --backend <auto|aaudio|opensl>   backend choice (default: auto)");
    println!("  --seconds <n>                    run time in seconds (default: 10)");
    println!("  --sample-rate <hz>               sample rate (default: 48000)");
    println!("  --channels <n>                   channels (default: 2)");
    println!("  --frames-per-burst <n>           callback frames (default: 192)");
    println!("  --freq <hz>                      output tone frequency (default: 440)");
    println!("  --amp <0..1>                     output tone amplitude (default: 0.2)");
    println!("  --gain <0..4>                    duplex output gain (default: 1.0)");
    println!("  --format <human|csv>             output format (default: human)");
    println!("  --help                           show this help");
}

fn parse_nonzero_u32(flag: &str, value: &str) -> Result<NonZeroU32, String> {
    let parsed = value
        .parse::<u32>()
        .map_err(|_| format!("invalid {flag} value: {value}"))?;
    NonZeroU32::new(parsed).ok_or_else(|| format!("{flag} must be > 0"))
}

fn parse_args() -> Result<BenchOptions, String> {
    let mut options = BenchOptions::default();
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            "--mode" => {
                let value = args
                    .next()
                    .ok_or_else(|| String::from("--mode needs a value"))?;
                options.mode = match value.as_str() {
                    "output" => BenchMode::Output,
                    "input" => BenchMode::Input,
                    "duplex" => BenchMode::Duplex,
                    _ => return Err(format!("invalid --mode value: {value}")),
                };
            }
            "--backend" => {
                let value = args
                    .next()
                    .ok_or_else(|| String::from("--backend needs a value"))?;
                options.backend = match value.as_str() {
                    "auto" => AndroidBackendKind::Auto,
                    "aaudio" => AndroidBackendKind::AAudio,
                    "opensl" => AndroidBackendKind::OpenSLES,
                    _ => return Err(format!("invalid --backend value: {value}")),
                };
            }
            "--seconds" => {
                let value = args
                    .next()
                    .ok_or_else(|| String::from("--seconds needs a value"))?;
                options.seconds = value
                    .parse::<u64>()
                    .map_err(|_| format!("invalid --seconds value: {value}"))?;
                if options.seconds == 0 {
                    return Err(String::from("--seconds must be > 0"));
                }
            }
            "--sample-rate" => {
                let value = args
                    .next()
                    .ok_or_else(|| String::from("--sample-rate needs a value"))?;
                options.sample_rate_hz = parse_nonzero_u32("--sample-rate", &value)?;
            }
            "--channels" => {
                let value = args
                    .next()
                    .ok_or_else(|| String::from("--channels needs a value"))?;
                options.channels = parse_nonzero_u32("--channels", &value)?;
            }
            "--frames-per-burst" => {
                let value = args
                    .next()
                    .ok_or_else(|| String::from("--frames-per-burst needs a value"))?;
                options.frames_per_burst = parse_nonzero_u32("--frames-per-burst", &value)?;
            }
            "--freq" => {
                let value = args
                    .next()
                    .ok_or_else(|| String::from("--freq needs a value"))?;
                options.freq_hz = value
                    .parse::<f32>()
                    .map_err(|_| format!("invalid --freq value: {value}"))?;
                if options.freq_hz <= 0.0 {
                    return Err(String::from("--freq must be > 0"));
                }
            }
            "--amp" => {
                let value = args
                    .next()
                    .ok_or_else(|| String::from("--amp needs a value"))?;
                options.amp = value
                    .parse::<f32>()
                    .map_err(|_| format!("invalid --amp value: {value}"))?;
                if !(0.0..=1.0).contains(&options.amp) {
                    return Err(String::from("--amp must be in range 0.0..=1.0"));
                }
            }
            "--gain" => {
                let value = args
                    .next()
                    .ok_or_else(|| String::from("--gain needs a value"))?;
                options.gain = value
                    .parse::<f32>()
                    .map_err(|_| format!("invalid --gain value: {value}"))?;
                if !(0.0..=4.0).contains(&options.gain) {
                    return Err(String::from("--gain must be in range 0.0..=4.0"));
                }
            }
            "--format" => {
                let value = args
                    .next()
                    .ok_or_else(|| String::from("--format needs a value"))?;
                options.format = match value.as_str() {
                    "human" => OutputFormat::Human,
                    "csv" => OutputFormat::Csv,
                    _ => return Err(format!("invalid --format value: {value}")),
                };
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }

    Ok(options)
}

fn base_config(options: BenchOptions, direction: Direction) -> StreamConfig {
    StreamConfig {
        sample_rate_hz: options.sample_rate_hz,
        channels: options.channels,
        frames_per_burst: options.frames_per_burst,
        direction,
        ..StreamConfig::default()
    }
}

fn run_output(options: BenchOptions, backend: AndroidBackend) -> Result<BenchResult, String> {
    let requested = base_config(options, Direction::Output);
    let mut stream = backend
        .create_stream(requested)
        .map_err(|error| format!("Could not create output stream: {error:?}"))?;
    let actual = stream.config();

    let channels = actual.channels.get() as usize;
    let sample_rate = actual.sample_rate_hz.get() as f32;
    let phase_step = 2.0_f32 * core::f32::consts::PI * options.freq_hz / sample_rate;
    let amplitude = options.amp;

    let callbacks = Arc::new(AtomicU64::new(0));
    let samples = Arc::new(AtomicU64::new(0));
    let timing = Arc::new(Mutex::new(TimingRecorder::default()));
    let backend_timing = Arc::new(Mutex::new(TimingRecorder::default()));

    let callbacks_cb = callbacks.clone();
    let samples_cb = samples.clone();
    let timing_cb = timing.clone();
    let backend_timing_cb = backend_timing.clone();
    let mut phase = 0.0_f32;

    stream
        .set_render_callback(move |info: CallbackInfo, out: &mut [f32]| {
            callbacks_cb.fetch_add(1, Ordering::Relaxed);
            samples_cb.fetch_add(out.len() as u64, Ordering::Relaxed);
            if let Ok(mut recorder) = timing_cb.lock() {
                recorder.record_now();
            }
            if let Ok(mut recorder) = backend_timing_cb.lock() {
                recorder.record_ns(info.callback_time_ns);
            }

            for frame in out.chunks_exact_mut(channels) {
                let sample = phase.sin() * amplitude;
                phase += phase_step;
                if phase >= 2.0_f32 * core::f32::consts::PI {
                    phase -= 2.0_f32 * core::f32::consts::PI;
                }
                frame.fill(sample);
            }
        })
        .map_err(|error| format!("Could not set output callback: {error:?}"))?;

    stream
        .start()
        .map_err(|error| format!("Could not start output stream: {error:?}"))?;
    std::thread::sleep(std::time::Duration::from_secs(options.seconds));
    stream
        .stop()
        .map_err(|error| format!("Could not stop output stream: {error:?}"))?;

    let output = StreamBenchResult {
        callbacks: callbacks.load(Ordering::Relaxed),
        samples: samples.load(Ordering::Relaxed),
        callback_samples: samples.load(Ordering::Relaxed),
        channels: actual.channels.get(),
        interval: timing.lock().ok().and_then(|r| r.summary()),
        backend_interval: backend_timing.lock().ok().and_then(|r| r.summary()),
        metrics: stream.metrics(),
    };

    Ok(BenchResult {
        requested,
        actual_output: Some(actual),
        actual_input: None,
        output: Some(output),
        input: None,
        zero_filled_samples: None,
    })
}

fn run_input(options: BenchOptions, backend: AndroidBackend) -> Result<BenchResult, String> {
    let requested = base_config(options, Direction::Input);
    let mut stream = backend
        .create_stream(requested)
        .map_err(|error| format!("Could not create input stream: {error:?}"))?;
    let actual = stream.config();

    let callbacks = Arc::new(AtomicU64::new(0));
    let samples = Arc::new(AtomicU64::new(0));
    let timing = Arc::new(Mutex::new(TimingRecorder::default()));
    let backend_timing = Arc::new(Mutex::new(TimingRecorder::default()));

    let callbacks_cb = callbacks.clone();
    let samples_cb = samples.clone();
    let timing_cb = timing.clone();
    let backend_timing_cb = backend_timing.clone();

    stream
        .set_capture_callback(move |info: CallbackInfo, input: &[f32]| {
            callbacks_cb.fetch_add(1, Ordering::Relaxed);
            samples_cb.fetch_add(input.len() as u64, Ordering::Relaxed);
            if let Ok(mut recorder) = timing_cb.lock() {
                recorder.record_now();
            }
            if let Ok(mut recorder) = backend_timing_cb.lock() {
                recorder.record_ns(info.callback_time_ns);
            }
        })
        .map_err(|error| format!("Could not set input callback: {error:?}"))?;

    stream
        .start()
        .map_err(|error| format!("Could not start input stream: {error:?}"))?;
    std::thread::sleep(std::time::Duration::from_secs(options.seconds));
    stream
        .stop()
        .map_err(|error| format!("Could not stop input stream: {error:?}"))?;

    let input = StreamBenchResult {
        callbacks: callbacks.load(Ordering::Relaxed),
        samples: samples.load(Ordering::Relaxed),
        callback_samples: samples.load(Ordering::Relaxed),
        channels: actual.channels.get(),
        interval: timing.lock().ok().and_then(|r| r.summary()),
        backend_interval: backend_timing.lock().ok().and_then(|r| r.summary()),
        metrics: stream.metrics(),
    };

    Ok(BenchResult {
        requested,
        actual_output: None,
        actual_input: Some(actual),
        output: None,
        input: Some(input),
        zero_filled_samples: None,
    })
}

struct SampleRing {
    buf: Vec<f32>,
    read: usize,
    write: usize,
    len: usize,
}

impl SampleRing {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: vec![0.0; capacity],
            read: 0,
            write: 0,
            len: 0,
        }
    }

    fn push_slice(&mut self, input: &[f32]) {
        for &sample in input {
            if self.len == self.buf.len() {
                self.read = (self.read + 1) % self.buf.len();
                self.len -= 1;
            }
            self.buf[self.write] = sample;
            self.write = (self.write + 1) % self.buf.len();
            self.len += 1;
        }
    }

    fn pop_into(&mut self, output: &mut [f32]) -> usize {
        let mut read_count = 0;
        for sample in output.iter_mut() {
            if self.len == 0 {
                break;
            }
            *sample = self.buf[self.read];
            self.read = (self.read + 1) % self.buf.len();
            self.len -= 1;
            read_count += 1;
        }
        read_count
    }
}

fn run_duplex(options: BenchOptions, backend: AndroidBackend) -> Result<BenchResult, String> {
    let requested_input = base_config(options, Direction::Input);
    let requested_output = base_config(options, Direction::Output);

    let mut input_stream = backend
        .create_stream(requested_input)
        .map_err(|error| format!("Could not create input stream: {error:?}"))?;
    let mut output_stream = backend
        .create_stream(requested_output)
        .map_err(|error| format!("Could not create output stream: {error:?}"))?;

    let actual_input = input_stream.config();
    let actual_output = output_stream.config();
    let ring_capacity = (actual_input.sample_rate_hz.get() * actual_input.channels.get()) as usize;
    let ring = Arc::new(Mutex::new(SampleRing::with_capacity(
        ring_capacity.max(1024),
    )));

    let in_callbacks = Arc::new(AtomicU64::new(0));
    let in_samples = Arc::new(AtomicU64::new(0));
    let in_timing = Arc::new(Mutex::new(TimingRecorder::default()));
    let in_backend_timing = Arc::new(Mutex::new(TimingRecorder::default()));

    let out_callbacks = Arc::new(AtomicU64::new(0));
    let out_samples = Arc::new(AtomicU64::new(0));
    let out_callback_samples = Arc::new(AtomicU64::new(0));
    let out_timing = Arc::new(Mutex::new(TimingRecorder::default()));
    let out_backend_timing = Arc::new(Mutex::new(TimingRecorder::default()));
    let zero_filled = Arc::new(AtomicU64::new(0));

    {
        let ring = ring.clone();
        let in_callbacks_cb = in_callbacks.clone();
        let in_samples_cb = in_samples.clone();
        let in_timing_cb = in_timing.clone();
        let in_backend_timing_cb = in_backend_timing.clone();
        input_stream
            .set_capture_callback(move |info: CallbackInfo, input: &[f32]| {
                in_callbacks_cb.fetch_add(1, Ordering::Relaxed);
                in_samples_cb.fetch_add(input.len() as u64, Ordering::Relaxed);
                if let Ok(mut recorder) = in_timing_cb.lock() {
                    recorder.record_now();
                }
                if let Ok(mut recorder) = in_backend_timing_cb.lock() {
                    recorder.record_ns(info.callback_time_ns);
                }
                if let Ok(mut rb) = ring.lock() {
                    rb.push_slice(input);
                }
            })
            .map_err(|error| format!("Could not set input callback: {error:?}"))?;
    }

    {
        let ring = ring.clone();
        let out_callbacks_cb = out_callbacks.clone();
        let out_samples_cb = out_samples.clone();
        let out_callback_samples_cb = out_callback_samples.clone();
        let out_timing_cb = out_timing.clone();
        let out_backend_timing_cb = out_backend_timing.clone();
        let zero_filled_cb = zero_filled.clone();
        let gain = options.gain;
        output_stream
            .set_render_callback(move |info: CallbackInfo, out: &mut [f32]| {
                out_callbacks_cb.fetch_add(1, Ordering::Relaxed);
                out_callback_samples_cb.fetch_add(out.len() as u64, Ordering::Relaxed);
                if let Ok(mut recorder) = out_timing_cb.lock() {
                    recorder.record_now();
                }
                if let Ok(mut recorder) = out_backend_timing_cb.lock() {
                    recorder.record_ns(info.callback_time_ns);
                }

                let mut read_count = 0_usize;
                if let Ok(mut rb) = ring.lock() {
                    read_count = rb.pop_into(out);
                }
                for sample in out.iter_mut().take(read_count) {
                    *sample *= gain;
                }
                if read_count < out.len() {
                    out[read_count..].fill(0.0);
                    zero_filled_cb.fetch_add(
                        (out.len().saturating_sub(read_count)) as u64,
                        Ordering::Relaxed,
                    );
                }
                out_samples_cb.fetch_add(read_count as u64, Ordering::Relaxed);
            })
            .map_err(|error| format!("Could not set output callback: {error:?}"))?;
    }

    output_stream
        .start()
        .map_err(|error| format!("Could not start output stream: {error:?}"))?;
    input_stream
        .start()
        .map_err(|error| format!("Could not start input stream: {error:?}"))?;
    std::thread::sleep(std::time::Duration::from_secs(options.seconds));
    input_stream
        .stop()
        .map_err(|error| format!("Could not stop input stream: {error:?}"))?;
    output_stream
        .stop()
        .map_err(|error| format!("Could not stop output stream: {error:?}"))?;

    let input = StreamBenchResult {
        callbacks: in_callbacks.load(Ordering::Relaxed),
        samples: in_samples.load(Ordering::Relaxed),
        callback_samples: in_samples.load(Ordering::Relaxed),
        channels: actual_input.channels.get(),
        interval: in_timing.lock().ok().and_then(|r| r.summary()),
        backend_interval: in_backend_timing.lock().ok().and_then(|r| r.summary()),
        metrics: input_stream.metrics(),
    };
    let output = StreamBenchResult {
        callbacks: out_callbacks.load(Ordering::Relaxed),
        samples: out_samples.load(Ordering::Relaxed),
        callback_samples: out_callback_samples.load(Ordering::Relaxed),
        channels: actual_output.channels.get(),
        interval: out_timing.lock().ok().and_then(|r| r.summary()),
        backend_interval: out_backend_timing.lock().ok().and_then(|r| r.summary()),
        metrics: output_stream.metrics(),
    };

    Ok(BenchResult {
        requested: requested_output,
        actual_output: Some(actual_output),
        actual_input: Some(actual_input),
        output: Some(output),
        input: Some(input),
        zero_filled_samples: Some(zero_filled.load(Ordering::Relaxed)),
    })
}

fn interval_to_fields(interval: Option<IntervalSummary>) -> [String; 5] {
    match interval {
        Some(v) => [
            format!("{:.3}", v.avg_us),
            format!("{:.3}", v.min_us),
            format!("{:.3}", v.max_us),
            format!("{:.3}", v.p95_us),
            format!("{}", v.samples),
        ],
        None => [
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::from("0"),
        ],
    }
}

fn interval_to_extra_fields(interval: Option<IntervalSummary>) -> [String; 5] {
    match interval {
        Some(v) => [
            format!("{:.3}", v.p50_us),
            format!("{:.3}", v.p99_us),
            format!("{:.3}", v.trimmed_p95_us),
            format!("{}", v.outliers_over_2x_median),
            format!("{}", v.outliers_over_5x_median),
        ],
        None => [
            String::new(),
            String::new(),
            String::new(),
            String::from("0"),
            String::from("0"),
        ],
    }
}

fn interval_to_backend_fields(interval: Option<IntervalSummary>) -> [String; 5] {
    match interval {
        Some(v) => [
            format!("{:.3}", v.avg_us),
            format!("{:.3}", v.min_us),
            format!("{:.3}", v.max_us),
            format!("{:.3}", v.p95_us),
            format!("{}", v.samples),
        ],
        None => [
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::from("0"),
        ],
    }
}

fn print_human(options: BenchOptions, result: &BenchResult) {
    println!(
        "Bench: mode={:?}, backend={:?}, seconds={}",
        options.mode, options.backend, options.seconds
    );
    println!(
        "Requested config: rate={}Hz, channels={}, burst={}",
        result.requested.sample_rate_hz.get(),
        result.requested.channels.get(),
        result.requested.frames_per_burst.get()
    );
    if let Some(cfg) = result.actual_output {
        println!(
            "Actual output: rate={}Hz, channels={}, burst={}",
            cfg.sample_rate_hz.get(),
            cfg.channels.get(),
            cfg.frames_per_burst.get()
        );
    }
    if let Some(cfg) = result.actual_input {
        println!(
            "Actual input:  rate={}Hz, channels={}, burst={}",
            cfg.sample_rate_hz.get(),
            cfg.channels.get(),
            cfg.frames_per_burst.get()
        );
    }

    if let Some(output) = &result.output {
        println!(
            "Output: callbacks={}, samples={}, xruns={}, observed_frames_per_callback={}",
            output.callbacks,
            output.samples,
            output.metrics.xrun_count,
            output
                .observed_frames_per_callback()
                .map(|v| format!("{v:.3}"))
                .unwrap_or_else(|| String::from("n/a"))
        );
        if let Some(interval) = output.interval {
            println!(
                "Output interval (us): avg={:.3}, min={:.3}, max={:.3}, p50={:.3}, p95={:.3}, p99={:.3}, p95_trimmed_2x={:.3}, outliers_>2x={}, outliers_>5x={}, samples={}",
                interval.avg_us,
                interval.min_us,
                interval.max_us,
                interval.p50_us,
                interval.p95_us,
                interval.p99_us,
                interval.trimmed_p95_us,
                interval.outliers_over_2x_median,
                interval.outliers_over_5x_median,
                interval.samples
            );
        }
        if let Some(interval) = output.backend_interval {
            println!(
                "Output backend interval (us): avg={:.3}, min={:.3}, max={:.3}, p95={:.3}, samples={}",
                interval.avg_us,
                interval.min_us,
                interval.max_us,
                interval.p95_us,
                interval.samples
            );
        }
    }
    if let Some(input) = &result.input {
        println!(
            "Input: callbacks={}, samples={}, xruns={}, observed_frames_per_callback={}",
            input.callbacks,
            input.samples,
            input.metrics.xrun_count,
            input
                .observed_frames_per_callback()
                .map(|v| format!("{v:.3}"))
                .unwrap_or_else(|| String::from("n/a"))
        );
        if let Some(interval) = input.interval {
            println!(
                "Input interval (us): avg={:.3}, min={:.3}, max={:.3}, p50={:.3}, p95={:.3}, p99={:.3}, p95_trimmed_2x={:.3}, outliers_>2x={}, outliers_>5x={}, samples={}",
                interval.avg_us,
                interval.min_us,
                interval.max_us,
                interval.p50_us,
                interval.p95_us,
                interval.p99_us,
                interval.trimmed_p95_us,
                interval.outliers_over_2x_median,
                interval.outliers_over_5x_median,
                interval.samples
            );
        }
        if let Some(interval) = input.backend_interval {
            println!(
                "Input backend interval (us): avg={:.3}, min={:.3}, max={:.3}, p95={:.3}, samples={}",
                interval.avg_us,
                interval.min_us,
                interval.max_us,
                interval.p95_us,
                interval.samples
            );
        }
    }
    if let Some(zero) = result.zero_filled_samples {
        println!("Duplex zero-filled samples: {zero}");
    }
}

fn print_csv(options: BenchOptions, result: &BenchResult) {
    println!(
        "mode,backend,seconds,req_rate,req_channels,req_burst,neg_out_rate,neg_out_channels,neg_out_burst,neg_in_rate,neg_in_channels,neg_in_burst,out_callbacks,out_samples,out_xruns,out_observed_frames_per_callback,out_avg_us,out_min_us,out_max_us,out_p95_us,out_interval_samples,in_callbacks,in_samples,in_xruns,in_observed_frames_per_callback,in_avg_us,in_min_us,in_max_us,in_p95_us,in_interval_samples,duplex_zero_filled,out_p50_us,out_p99_us,out_p95_trimmed_2x_us,out_outliers_over_2x_median,out_outliers_over_5x_median,in_p50_us,in_p99_us,in_p95_trimmed_2x_us,in_outliers_over_2x_median,in_outliers_over_5x_median,out_backend_avg_us,out_backend_min_us,out_backend_max_us,out_backend_p95_us,out_backend_interval_samples,in_backend_avg_us,in_backend_min_us,in_backend_max_us,in_backend_p95_us,in_backend_interval_samples"
    );

    let out_cfg = result.actual_output.unwrap_or(result.requested);
    let in_cfg = result.actual_input.unwrap_or(result.requested);

    let (
        out_callbacks,
        out_samples,
        out_xruns,
        out_obs_frames,
        out_interval,
        out_interval_extra,
        out_backend_interval,
    ) = match &result.output {
        Some(v) => (
            v.callbacks,
            v.samples,
            v.metrics.xrun_count,
            v.observed_frames_per_callback()
                .map(|x| format!("{x:.3}"))
                .unwrap_or_default(),
            interval_to_fields(v.interval),
            interval_to_extra_fields(v.interval),
            interval_to_backend_fields(v.backend_interval),
        ),
        None => (
            0,
            0,
            0,
            String::new(),
            interval_to_fields(None),
            interval_to_extra_fields(None),
            interval_to_backend_fields(None),
        ),
    };
    let (
        in_callbacks,
        in_samples,
        in_xruns,
        in_obs_frames,
        in_interval,
        in_interval_extra,
        in_backend_interval,
    ) = match &result.input {
        Some(v) => (
            v.callbacks,
            v.samples,
            v.metrics.xrun_count,
            v.observed_frames_per_callback()
                .map(|x| format!("{x:.3}"))
                .unwrap_or_default(),
            interval_to_fields(v.interval),
            interval_to_extra_fields(v.interval),
            interval_to_backend_fields(v.backend_interval),
        ),
        None => (
            0,
            0,
            0,
            String::new(),
            interval_to_fields(None),
            interval_to_extra_fields(None),
            interval_to_backend_fields(None),
        ),
    };

    let negotiated_out_burst = out_obs_frames
        .parse::<f64>()
        .ok()
        .map(|v| v.round() as u32)
        .unwrap_or(out_cfg.frames_per_burst.get());
    let negotiated_in_burst = in_obs_frames
        .parse::<f64>()
        .ok()
        .map(|v| v.round() as u32)
        .unwrap_or(in_cfg.frames_per_burst.get());

    let mut row = vec![
        format!("{:?}", options.mode),
        format!("{:?}", options.backend),
        format!("{}", options.seconds),
        format!("{}", result.requested.sample_rate_hz.get()),
        format!("{}", result.requested.channels.get()),
        format!("{}", result.requested.frames_per_burst.get()),
        format!("{}", out_cfg.sample_rate_hz.get()),
        format!("{}", out_cfg.channels.get()),
        format!("{}", negotiated_out_burst),
        format!("{}", in_cfg.sample_rate_hz.get()),
        format!("{}", in_cfg.channels.get()),
        format!("{}", negotiated_in_burst),
        format!("{}", out_callbacks),
        format!("{}", out_samples),
        format!("{}", out_xruns),
        out_obs_frames,
    ];
    row.extend_from_slice(&out_interval);
    row.extend([
        format!("{}", in_callbacks),
        format!("{}", in_samples),
        format!("{}", in_xruns),
        in_obs_frames,
    ]);
    row.extend_from_slice(&in_interval);
    row.push(format!("{}", result.zero_filled_samples.unwrap_or(0)));
    row.extend_from_slice(&out_interval_extra);
    row.extend_from_slice(&in_interval_extra);
    row.extend_from_slice(&out_backend_interval);
    row.extend_from_slice(&in_backend_interval);
    println!("{}", row.join(","));
}

fn main() {
    let options = match parse_args() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error}");
            print_help();
            std::process::exit(2);
        }
    };
    let backend = AndroidBackend::new(options.backend);

    let result = match options.mode {
        BenchMode::Output => run_output(options, backend),
        BenchMode::Input => run_input(options, backend),
        BenchMode::Duplex => run_duplex(options, backend),
    };

    let result = match result {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    match options.format {
        OutputFormat::Human => print_human(options, &result),
        OutputFormat::Csv => print_csv(options, &result),
    }
}
