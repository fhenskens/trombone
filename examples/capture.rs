use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use trombone::backend::AudioBackend;
use trombone::backend::android::{AndroidBackend, AndroidBackendKind};
use trombone::core::callback::CallbackInfo;
use trombone::core::config::{Direction, StreamConfig};

struct CaptureOptions {
    seconds: u64,
    backend: AndroidBackendKind,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            seconds: 5,
            backend: AndroidBackendKind::Auto,
        }
    }
}

fn print_help() {
    println!("Capture demo options:");
    println!("  --backend <auto|aaudio|opensl> backend choice (default: auto)");
    println!("  --seconds <n>    Capture length in seconds (default: 5)");
    println!("  --help           Show this help");
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
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }

    Ok(options)
}

fn peak_bar(peak: f32, width: usize) -> String {
    let clamped = peak.clamp(0.0, 1.0);
    let filled = (clamped * width as f32).round() as usize;
    let mut bar = String::with_capacity(width);
    for i in 0..width {
        if i < filled {
            bar.push('#');
        } else {
            bar.push('.');
        }
    }
    bar
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

    let config = StreamConfig {
        direction: Direction::Input,
        ..StreamConfig::default()
    };
    let backend = AndroidBackend::new(options.backend);

    match backend.create_stream(config) {
        Ok(mut stream) => {
            let callback_calls = Arc::new(AtomicU64::new(0));
            let captured_frames = Arc::new(AtomicU64::new(0));
            let peak_milli = Arc::new(AtomicU32::new(0));
            let callback_calls_cb = callback_calls.clone();
            let captured_frames_cb = captured_frames.clone();
            let peak_milli_cb = peak_milli.clone();

            if let Err(error) =
                stream.set_capture_callback(move |_info: CallbackInfo, input: &[f32]| {
                    callback_calls_cb.fetch_add(1, Ordering::Relaxed);
                    captured_frames_cb.fetch_add(input.len() as u64, Ordering::Relaxed);

                    let mut local_peak = 0.0_f32;
                    for sample in input {
                        let abs = sample.abs();
                        if abs > local_peak {
                            local_peak = abs;
                        }
                    }

                    let local_milli = (local_peak * 1000.0).round() as u32;
                    let mut current = peak_milli_cb.load(Ordering::Relaxed);
                    while local_milli > current {
                        match peak_milli_cb.compare_exchange_weak(
                            current,
                            local_milli,
                            Ordering::Relaxed,
                            Ordering::Relaxed,
                        ) {
                            Ok(_) => break,
                            Err(next) => current = next,
                        }
                    }
                })
            {
                eprintln!("Could not set capture callback: {error:?}");
                return;
            }

            println!(
                "Starting input stream @ {} Hz for {}s",
                stream.config().sample_rate_hz,
                options.seconds
            );
            if let Err(error) = stream.start() {
                eprintln!("Could not start stream: {error:?}");
                return;
            }

            let steps = options.seconds.saturating_mul(2);
            for _ in 0..steps {
                std::thread::sleep(std::time::Duration::from_millis(500));
                let milli = peak_milli.swap(0, Ordering::Relaxed);
                let peak = milli as f32 / 1000.0;
                println!("peak {:>5.3} [{}]", peak, peak_bar(peak, 20));
            }

            if let Err(error) = stream.stop() {
                eprintln!("Could not stop stream: {error:?}");
            }
            let metrics = stream.metrics();
            println!(
                "Callback calls: {}, captured samples: {}",
                callback_calls.load(Ordering::Relaxed),
                captured_frames.load(Ordering::Relaxed)
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
            println!("Could not create input stream: {error:?}");
        }
    }
}
