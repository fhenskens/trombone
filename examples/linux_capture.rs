#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("linux_capture is only supported on Linux.");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
mod app {
    use std::num::NonZeroU32;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use trombone::backend::AudioBackend;
    use trombone::backend::linux::{LinuxBackend, LinuxBackendKind};
    use trombone::core::callback::CallbackInfo;
    use trombone::core::config::{Direction, StreamConfig};

    struct CaptureOptions {
        backend: LinuxBackendKind,
        frames_per_burst: NonZeroU32,
        seconds: u64,
    }

    impl Default for CaptureOptions {
        fn default() -> Self {
            Self {
                backend: LinuxBackendKind::Auto,
                frames_per_burst: default_frames_per_burst(),
                seconds: 5,
            }
        }
    }

    fn print_help() {
        println!("Linux capture demo options:");
        println!("  --backend <auto|pipewire|alsa> backend choice (default: auto)");
        println!("  --frames-per-burst <n> callback frames (default: 192, or 1920 on WSL)");
        println!("  --seconds <n>    capture length in seconds (default: 5)");
        println!("  --help           show this help");
    }

    fn parse_args() -> Result<CaptureOptions, String> {
        let mut options = CaptureOptions::default();
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                "--backend" => {
                    let value = args
                        .next()
                        .ok_or_else(|| String::from("--backend needs a value"))?;
                    options.backend = match value.as_str() {
                        "auto" => LinuxBackendKind::Auto,
                        "pipewire" => LinuxBackendKind::PipeWire,
                        "alsa" => LinuxBackendKind::Alsa,
                        _ => return Err(format!("invalid --backend value: {value}")),
                    };
                }
                "--frames-per-burst" => {
                    let value = args
                        .next()
                        .ok_or_else(|| String::from("--frames-per-burst needs a value"))?;
                    let parsed = value
                        .parse::<u32>()
                        .map_err(|_| format!("invalid --frames-per-burst value: {value}"))?;
                    options.frames_per_burst = NonZeroU32::new(parsed)
                        .ok_or_else(|| String::from("--frames-per-burst must be > 0"))?;
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
                _ => return Err(format!("unknown argument: {arg}")),
            }
        }
        Ok(options)
    }

    fn update_peak_bits(bits: &AtomicUsize, value: f32) {
        let candidate = value.abs().to_bits() as usize;
        loop {
            let current = bits.load(Ordering::Relaxed);
            if candidate <= current {
                break;
            }
            if bits
                .compare_exchange(current, candidate, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }
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

        let config = StreamConfig {
            direction: Direction::Input,
            frames_per_burst: options.frames_per_burst,
            ..StreamConfig::default()
        };
        let backend = LinuxBackend::new(options.backend);

        match backend.create_stream(config) {
            Ok(mut stream) => {
                let callbacks = Arc::new(AtomicU64::new(0));
                let samples = Arc::new(AtomicU64::new(0));
                let peak_bits = Arc::new(AtomicUsize::new(0));

                let callbacks_cb = callbacks.clone();
                let samples_cb = samples.clone();
                let peak_bits_cb = peak_bits.clone();
                if let Err(error) =
                    stream.set_capture_callback(move |_info: CallbackInfo, input: &[f32]| {
                        callbacks_cb.fetch_add(1, Ordering::Relaxed);
                        samples_cb.fetch_add(input.len() as u64, Ordering::Relaxed);
                        let mut local_peak = 0.0_f32;
                        for sample in input {
                            local_peak = local_peak.max(sample.abs());
                        }
                        update_peak_bits(&peak_bits_cb, local_peak);
                    })
                {
                    eprintln!("Could not set capture callback: {error:?}");
                    return;
                }

                println!(
                    "Starting Linux capture stream: {} Hz, burst {}, {}s",
                    stream.config().sample_rate_hz,
                    stream.config().frames_per_burst,
                    options.seconds
                );
                if let Err(error) = stream.start() {
                    eprintln!("Could not start stream: {error:?}");
                    return;
                }
                std::thread::sleep(std::time::Duration::from_secs(options.seconds));
                if let Err(error) = stream.stop() {
                    eprintln!("Could not stop stream: {error:?}");
                }

                let metrics = stream.metrics();
                let peak = f32::from_bits(peak_bits.load(Ordering::Relaxed) as u32);
                println!(
                    "Callback calls: {}, captured samples: {}, peak: {:.3}",
                    callbacks.load(Ordering::Relaxed),
                    samples.load(Ordering::Relaxed),
                    peak
                );
                println!(
                    "Metrics: xruns={}, frames_written={:?}, frames_read={:?}",
                    metrics.xrun_count, metrics.frames_written, metrics.frames_read
                );
                println!(
                    "Timing: callback_time_ns={:?}, backend_time_ns={:?}, frame_position={:?}, latency_frames={:?}, latency_ns={:?}",
                    metrics.timing.callback_time_ns,
                    metrics.timing.backend_time_ns,
                    metrics.timing.frame_position,
                    metrics.timing.estimated_latency_frames,
                    metrics.timing.estimated_latency_ns
                );
                println!("Done.");
            }
            Err(error) => {
                println!("Could not create Linux capture stream: {error:?}");
            }
        }
    }

    fn default_frames_per_burst() -> NonZeroU32 {
        let is_wsl = std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .map(|v| v.to_ascii_lowercase().contains("microsoft"))
            .unwrap_or(false);
        if is_wsl {
            NonZeroU32::new(1920).expect("non-zero literal")
        } else {
            NonZeroU32::new(192).expect("non-zero literal")
        }
    }
}

#[cfg(target_os = "linux")]
fn main() {
    app::main();
}
