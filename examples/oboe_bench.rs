#[cfg(not(target_os = "android"))]
fn main() {
    eprintln!("oboe_bench is only supported on Android.");
    std::process::exit(1);
}

#[cfg(target_os = "android")]
mod android {
    use std::num::NonZeroU32;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Instant;

    use oboe::{
        AudioApi, AudioInputCallback, AudioInputStreamSafe, AudioOutputCallback,
        AudioOutputStreamSafe, AudioStream, AudioStreamBuilder, AudioStreamSafe,
        DataCallbackResult, Input, Mono, Output, PerformanceMode, SharingMode,
    };

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

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum BenchBackend {
        Auto,
        AAudio,
        OpenSLES,
    }

    #[derive(Debug, Clone, Copy)]
    struct BenchOptions {
        mode: BenchMode,
        backend: BenchBackend,
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
                backend: BenchBackend::Auto,
                seconds: 10,
                sample_rate_hz: NonZeroU32::new(48_000).expect("literal is non-zero"),
                channels: NonZeroU32::new(1).expect("literal is non-zero"),
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

    #[derive(Debug, Clone, Copy)]
    struct StreamResult {
        callbacks: u64,
        samples: u64,
        callback_samples: u64,
        negotiated_frames_per_burst: i32,
        channels: u32,
        interval: Option<IntervalSummary>,
        xruns: u32,
    }

    impl StreamResult {
        fn observed_frames_per_callback(&self) -> Option<f64> {
            if self.callbacks == 0 || self.channels == 0 {
                return None;
            }
            Some(self.callback_samples as f64 / self.callbacks as f64 / self.channels as f64)
        }
    }

    #[derive(Debug)]
    struct BenchResult {
        output: Option<StreamResult>,
        input: Option<StreamResult>,
        zero_filled_samples: Option<u64>,
    }

    struct ToneCallback {
        callbacks: Arc<AtomicU64>,
        samples: Arc<AtomicU64>,
        timing: Arc<Mutex<TimingRecorder>>,
        phase: f32,
        phase_step: f32,
        amp: f32,
    }

    impl AudioOutputCallback for ToneCallback {
        type FrameType = (f32, Mono);

        fn on_audio_ready(
            &mut self,
            _stream: &mut dyn AudioOutputStreamSafe,
            audio_data: &mut [f32],
        ) -> DataCallbackResult {
            self.callbacks.fetch_add(1, Ordering::Relaxed);
            self.samples
                .fetch_add(audio_data.len() as u64, Ordering::Relaxed);
            if let Ok(mut recorder) = self.timing.lock() {
                recorder.record_now();
            }

            for sample in audio_data.iter_mut() {
                *sample = self.phase.sin() * self.amp;
                self.phase += self.phase_step;
                if self.phase >= 2.0_f32 * core::f32::consts::PI {
                    self.phase -= 2.0_f32 * core::f32::consts::PI;
                }
            }
            DataCallbackResult::Continue
        }
    }

    struct CaptureCallback {
        callbacks: Arc<AtomicU64>,
        samples: Arc<AtomicU64>,
        timing: Arc<Mutex<TimingRecorder>>,
    }

    impl AudioInputCallback for CaptureCallback {
        type FrameType = (f32, Mono);

        fn on_audio_ready(
            &mut self,
            _stream: &mut dyn AudioInputStreamSafe,
            audio_data: &[f32],
        ) -> DataCallbackResult {
            self.callbacks.fetch_add(1, Ordering::Relaxed);
            self.samples
                .fetch_add(audio_data.len() as u64, Ordering::Relaxed);
            if let Ok(mut recorder) = self.timing.lock() {
                recorder.record_now();
            }
            DataCallbackResult::Continue
        }
    }

    fn print_help() {
        println!("oboe-rs benchmark options:");
        println!("  --mode <output|input|duplex>     benchmark mode (default: output)");
        println!("  --backend <auto|aaudio|opensl>   backend choice (default: auto)");
        println!("  --seconds <n>                    run time in seconds (default: 10)");
        println!("  --sample-rate <hz>               sample rate (default: 48000)");
        println!("  --channels <n>                   channels (default: 1)");
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
                        "auto" => BenchBackend::Auto,
                        "aaudio" => BenchBackend::AAudio,
                        "opensl" => BenchBackend::OpenSLES,
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

    fn to_audio_api(backend: BenchBackend) -> AudioApi {
        match backend {
            BenchBackend::Auto => AudioApi::Unspecified,
            BenchBackend::AAudio => AudioApi::AAudio,
            BenchBackend::OpenSLES => AudioApi::OpenSLES,
        }
    }

    fn run_output(options: BenchOptions) -> Result<BenchResult, String> {
        if options.channels.get() != 1 {
            return Err(String::from(
                "oboe_bench currently supports --channels 1 only",
            ));
        }

        let callbacks = Arc::new(AtomicU64::new(0));
        let samples = Arc::new(AtomicU64::new(0));
        let timing = Arc::new(Mutex::new(TimingRecorder::default()));

        let phase_step =
            2.0_f32 * core::f32::consts::PI * options.freq_hz / options.sample_rate_hz.get() as f32;
        let callback = ToneCallback {
            callbacks: callbacks.clone(),
            samples: samples.clone(),
            timing: timing.clone(),
            phase: 0.0,
            phase_step,
            amp: options.amp,
        };

        let mut stream = AudioStreamBuilder::default()
            .set_direction::<Output>()
            .set_audio_api(to_audio_api(options.backend))
            .set_performance_mode(PerformanceMode::LowLatency)
            .set_sharing_mode(SharingMode::Shared)
            .set_sample_rate(options.sample_rate_hz.get() as i32)
            .set_frames_per_callback(options.frames_per_burst.get() as i32)
            .set_channel_count::<Mono>()
            .set_f32()
            .set_callback(callback)
            .open_stream()
            .map_err(|error| format!("Could not create oboe output stream: {error:?}"))?;

        stream
            .start()
            .map_err(|error| format!("Could not start oboe output stream: {error:?}"))?;
        std::thread::sleep(std::time::Duration::from_secs(options.seconds));
        stream
            .stop()
            .map_err(|error| format!("Could not stop oboe output stream: {error:?}"))?;

        let xruns = stream.get_xrun_count().unwrap_or(0).max(0) as u32;
        let negotiated_frames_per_burst = stream.get_frames_per_burst();

        Ok(BenchResult {
            output: Some(StreamResult {
                callbacks: callbacks.load(Ordering::Relaxed),
                samples: samples.load(Ordering::Relaxed),
                callback_samples: samples.load(Ordering::Relaxed),
                negotiated_frames_per_burst,
                channels: options.channels.get(),
                interval: timing.lock().ok().and_then(|r| r.summary()),
                xruns,
            }),
            input: None,
            zero_filled_samples: None,
        })
    }

    fn run_input(options: BenchOptions) -> Result<BenchResult, String> {
        if options.channels.get() != 1 {
            return Err(String::from(
                "oboe_bench currently supports --channels 1 only",
            ));
        }

        let callbacks = Arc::new(AtomicU64::new(0));
        let samples = Arc::new(AtomicU64::new(0));
        let timing = Arc::new(Mutex::new(TimingRecorder::default()));

        let callback = CaptureCallback {
            callbacks: callbacks.clone(),
            samples: samples.clone(),
            timing: timing.clone(),
        };

        let mut stream = AudioStreamBuilder::default()
            .set_direction::<Input>()
            .set_audio_api(to_audio_api(options.backend))
            .set_performance_mode(PerformanceMode::LowLatency)
            .set_sharing_mode(SharingMode::Shared)
            .set_sample_rate(options.sample_rate_hz.get() as i32)
            .set_frames_per_callback(options.frames_per_burst.get() as i32)
            .set_channel_count::<Mono>()
            .set_f32()
            .set_callback(callback)
            .open_stream()
            .map_err(|error| format!("Could not create oboe input stream: {error:?}"))?;

        stream
            .start()
            .map_err(|error| format!("Could not start oboe input stream: {error:?}"))?;
        std::thread::sleep(std::time::Duration::from_secs(options.seconds));
        stream
            .stop()
            .map_err(|error| format!("Could not stop oboe input stream: {error:?}"))?;

        let xruns = stream.get_xrun_count().unwrap_or(0).max(0) as u32;
        let negotiated_frames_per_burst = stream.get_frames_per_burst();

        Ok(BenchResult {
            output: None,
            input: Some(StreamResult {
                callbacks: callbacks.load(Ordering::Relaxed),
                samples: samples.load(Ordering::Relaxed),
                callback_samples: samples.load(Ordering::Relaxed),
                negotiated_frames_per_burst,
                channels: options.channels.get(),
                interval: timing.lock().ok().and_then(|r| r.summary()),
                xruns,
            }),
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

    struct DuplexInputCallback {
        callbacks: Arc<AtomicU64>,
        samples: Arc<AtomicU64>,
        timing: Arc<Mutex<TimingRecorder>>,
        ring: Arc<Mutex<SampleRing>>,
    }

    impl AudioInputCallback for DuplexInputCallback {
        type FrameType = (f32, Mono);

        fn on_audio_ready(
            &mut self,
            _stream: &mut dyn AudioInputStreamSafe,
            audio_data: &[f32],
        ) -> DataCallbackResult {
            self.callbacks.fetch_add(1, Ordering::Relaxed);
            self.samples
                .fetch_add(audio_data.len() as u64, Ordering::Relaxed);
            if let Ok(mut recorder) = self.timing.lock() {
                recorder.record_now();
            }
            if let Ok(mut ring) = self.ring.lock() {
                ring.push_slice(audio_data);
            }
            DataCallbackResult::Continue
        }
    }

    struct DuplexOutputCallback {
        callbacks: Arc<AtomicU64>,
        callback_samples: Arc<AtomicU64>,
        played_samples: Arc<AtomicU64>,
        zero_filled_samples: Arc<AtomicU64>,
        timing: Arc<Mutex<TimingRecorder>>,
        ring: Arc<Mutex<SampleRing>>,
        gain: f32,
    }

    impl AudioOutputCallback for DuplexOutputCallback {
        type FrameType = (f32, Mono);

        fn on_audio_ready(
            &mut self,
            _stream: &mut dyn AudioOutputStreamSafe,
            audio_data: &mut [f32],
        ) -> DataCallbackResult {
            self.callbacks.fetch_add(1, Ordering::Relaxed);
            self.callback_samples
                .fetch_add(audio_data.len() as u64, Ordering::Relaxed);
            if let Ok(mut recorder) = self.timing.lock() {
                recorder.record_now();
            }

            let mut read_count = 0_usize;
            if let Ok(mut ring) = self.ring.lock() {
                read_count = ring.pop_into(audio_data);
            }
            for sample in audio_data.iter_mut().take(read_count) {
                *sample *= self.gain;
            }
            if read_count < audio_data.len() {
                audio_data[read_count..].fill(0.0);
                self.zero_filled_samples
                    .fetch_add((audio_data.len() - read_count) as u64, Ordering::Relaxed);
            }
            self.played_samples
                .fetch_add(read_count as u64, Ordering::Relaxed);
            DataCallbackResult::Continue
        }
    }

    fn run_duplex(options: BenchOptions) -> Result<BenchResult, String> {
        if options.channels.get() != 1 {
            return Err(String::from(
                "oboe_bench currently supports --channels 1 only",
            ));
        }

        let ring = Arc::new(Mutex::new(SampleRing::with_capacity(
            options.sample_rate_hz.get() as usize,
        )));

        let in_callbacks = Arc::new(AtomicU64::new(0));
        let in_samples = Arc::new(AtomicU64::new(0));
        let in_timing = Arc::new(Mutex::new(TimingRecorder::default()));

        let out_callbacks = Arc::new(AtomicU64::new(0));
        let out_callback_samples = Arc::new(AtomicU64::new(0));
        let out_played_samples = Arc::new(AtomicU64::new(0));
        let out_zero_filled = Arc::new(AtomicU64::new(0));
        let out_timing = Arc::new(Mutex::new(TimingRecorder::default()));

        let input_callback = DuplexInputCallback {
            callbacks: in_callbacks.clone(),
            samples: in_samples.clone(),
            timing: in_timing.clone(),
            ring: ring.clone(),
        };
        let output_callback = DuplexOutputCallback {
            callbacks: out_callbacks.clone(),
            callback_samples: out_callback_samples.clone(),
            played_samples: out_played_samples.clone(),
            zero_filled_samples: out_zero_filled.clone(),
            timing: out_timing.clone(),
            ring: ring.clone(),
            gain: options.gain,
        };

        let mut input_stream = AudioStreamBuilder::default()
            .set_direction::<Input>()
            .set_audio_api(to_audio_api(options.backend))
            .set_performance_mode(PerformanceMode::LowLatency)
            .set_sharing_mode(SharingMode::Shared)
            .set_sample_rate(options.sample_rate_hz.get() as i32)
            .set_frames_per_callback(options.frames_per_burst.get() as i32)
            .set_channel_count::<Mono>()
            .set_f32()
            .set_callback(input_callback)
            .open_stream()
            .map_err(|error| format!("Could not create oboe input stream: {error:?}"))?;

        let mut output_stream = AudioStreamBuilder::default()
            .set_direction::<Output>()
            .set_audio_api(to_audio_api(options.backend))
            .set_performance_mode(PerformanceMode::LowLatency)
            .set_sharing_mode(SharingMode::Shared)
            .set_sample_rate(options.sample_rate_hz.get() as i32)
            .set_frames_per_callback(options.frames_per_burst.get() as i32)
            .set_channel_count::<Mono>()
            .set_f32()
            .set_callback(output_callback)
            .open_stream()
            .map_err(|error| format!("Could not create oboe output stream: {error:?}"))?;

        output_stream
            .start()
            .map_err(|error| format!("Could not start oboe output stream: {error:?}"))?;
        input_stream
            .start()
            .map_err(|error| format!("Could not start oboe input stream: {error:?}"))?;

        std::thread::sleep(std::time::Duration::from_secs(options.seconds));

        input_stream
            .stop()
            .map_err(|error| format!("Could not stop oboe input stream: {error:?}"))?;
        output_stream
            .stop()
            .map_err(|error| format!("Could not stop oboe output stream: {error:?}"))?;

        let input = StreamResult {
            callbacks: in_callbacks.load(Ordering::Relaxed),
            samples: in_samples.load(Ordering::Relaxed),
            callback_samples: in_samples.load(Ordering::Relaxed),
            negotiated_frames_per_burst: input_stream.get_frames_per_burst(),
            channels: options.channels.get(),
            interval: in_timing.lock().ok().and_then(|r| r.summary()),
            xruns: input_stream.get_xrun_count().unwrap_or(0).max(0) as u32,
        };
        let output = StreamResult {
            callbacks: out_callbacks.load(Ordering::Relaxed),
            samples: out_played_samples.load(Ordering::Relaxed),
            callback_samples: out_callback_samples.load(Ordering::Relaxed),
            negotiated_frames_per_burst: output_stream.get_frames_per_burst(),
            channels: options.channels.get(),
            interval: out_timing.lock().ok().and_then(|r| r.summary()),
            xruns: output_stream.get_xrun_count().unwrap_or(0).max(0) as u32,
        };

        Ok(BenchResult {
            output: Some(output),
            input: Some(input),
            zero_filled_samples: Some(out_zero_filled.load(Ordering::Relaxed)),
        })
    }

    fn run(options: BenchOptions) -> Result<BenchResult, String> {
        match options.mode {
            BenchMode::Output => run_output(options),
            BenchMode::Input => run_input(options),
            BenchMode::Duplex => run_duplex(options),
        }
    }

    fn print_human(options: BenchOptions, result: &BenchResult) {
        println!(
            "oboe bench: mode={:?}, backend={:?}, seconds={}, sample_rate={}, channels={}, burst={}",
            options.mode,
            options.backend,
            options.seconds,
            options.sample_rate_hz.get(),
            options.channels.get(),
            options.frames_per_burst.get(),
        );
        if let Some(out) = result.output {
            println!(
                "Output: callbacks={}, samples={}, xruns={}, observed_frames_per_callback={}",
                out.callbacks,
                out.samples,
                out.xruns,
                out.observed_frames_per_callback()
                    .map(|v| format!("{v:.3}"))
                    .unwrap_or_else(|| String::from("n/a"))
            );
            if let Some(interval) = out.interval {
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
        }
        if let Some(input) = result.input {
            println!(
                "Input: callbacks={}, samples={}, xruns={}, observed_frames_per_callback={}",
                input.callbacks,
                input.samples,
                input.xruns,
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
        }
        if let Some(zero_filled) = result.zero_filled_samples {
            println!("Duplex zero-filled samples: {zero_filled}");
        }
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

    fn print_csv(options: BenchOptions, result: &BenchResult) {
        println!(
            "mode,backend,seconds,req_rate,req_channels,req_burst,neg_out_rate,neg_out_channels,neg_out_burst,neg_in_rate,neg_in_channels,neg_in_burst,out_callbacks,out_samples,out_xruns,out_observed_frames_per_callback,out_avg_us,out_min_us,out_max_us,out_p95_us,out_interval_samples,in_callbacks,in_samples,in_xruns,in_observed_frames_per_callback,in_avg_us,in_min_us,in_max_us,in_p95_us,in_interval_samples,duplex_zero_filled,out_p50_us,out_p99_us,out_p95_trimmed_2x_us,out_outliers_over_2x_median,out_outliers_over_5x_median,in_p50_us,in_p99_us,in_p95_trimmed_2x_us,in_outliers_over_2x_median,in_outliers_over_5x_median"
        );

        let (
            out_callbacks,
            out_samples,
            out_xruns,
            out_obs_frames,
            out_interval,
            out_interval_extra,
        ) = match result.output {
            Some(v) => (
                v.callbacks,
                v.samples,
                v.xruns,
                v.observed_frames_per_callback()
                    .map(|x| format!("{x:.3}"))
                    .unwrap_or_default(),
                interval_to_fields(v.interval),
                interval_to_extra_fields(v.interval),
            ),
            None => (
                0,
                0,
                0,
                String::new(),
                interval_to_fields(None),
                interval_to_extra_fields(None),
            ),
        };
        let (in_callbacks, in_samples, in_xruns, in_obs_frames, in_interval, in_interval_extra) =
            match result.input {
                Some(v) => (
                    v.callbacks,
                    v.samples,
                    v.xruns,
                    v.observed_frames_per_callback()
                        .map(|x| format!("{x:.3}"))
                        .unwrap_or_default(),
                    interval_to_fields(v.interval),
                    interval_to_extra_fields(v.interval),
                ),
                None => (
                    0,
                    0,
                    0,
                    String::new(),
                    interval_to_fields(None),
                    interval_to_extra_fields(None),
                ),
            };
        let negotiated_out_burst = result
            .output
            .map(|v| v.negotiated_frames_per_burst)
            .unwrap_or(options.frames_per_burst.get() as i32);
        let negotiated_in_burst = result
            .input
            .map(|v| v.negotiated_frames_per_burst)
            .unwrap_or(options.frames_per_burst.get() as i32);

        let mut row = vec![
            format!("{:?}", options.mode),
            format!("{:?}", options.backend),
            format!("{}", options.seconds),
            format!("{}", options.sample_rate_hz.get()),
            format!("{}", options.channels.get()),
            format!("{}", options.frames_per_burst.get()),
            format!("{}", options.sample_rate_hz.get()),
            format!("{}", options.channels.get()),
            format!("{}", negotiated_out_burst),
            format!("{}", options.sample_rate_hz.get()),
            format!("{}", options.channels.get()),
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
        println!("{}", row.join(","));
    }

    pub fn main() {
        let options = match parse_args() {
            Ok(value) => value,
            Err(error) => {
                eprintln!("{error}");
                print_help();
                std::process::exit(2);
            }
        };

        let result = match run(options) {
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
}

#[cfg(target_os = "android")]
fn main() {
    android::main();
}
