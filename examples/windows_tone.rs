use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use trombone::backend::AudioBackend;
use trombone::backend::windows::{WindowsBackend, WindowsBackendKind};
use trombone::core::callback::CallbackInfo;
use trombone::core::config::StreamConfig;

struct ToneOptions {
    freq_hz: f32,
    amp: f32,
    seconds: u64,
}

impl Default for ToneOptions {
    fn default() -> Self {
        Self {
            freq_hz: 440.0,
            amp: 0.15,
            seconds: 3,
        }
    }
}

fn print_help() {
    println!("Windows tone demo options:");
    println!("  --freq <hz>      Tone frequency in Hz (default: 440)");
    println!("  --amp <0..1>     Tone amplitude (default: 0.15)");
    println!("  --seconds <n>    Playback length in seconds (default: 3)");
    println!("  --help           Show this help");
}

fn parse_args() -> Result<ToneOptions, String> {
    let mut options = ToneOptions::default();
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
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

fn main() {
    let options = match parse_args() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error}");
            print_help();
            std::process::exit(2);
        }
    };

    let config = StreamConfig::default();
    let backend = WindowsBackend::new(WindowsBackendKind::Wasapi);

    match backend.create_stream(config) {
        Ok(mut stream) => {
            let channels = stream.config().channels.get() as usize;
            let sample_rate = stream.config().sample_rate_hz.get() as f32;
            let phase_step = 2.0_f32 * core::f32::consts::PI * options.freq_hz / sample_rate;
            let mut phase = 0.0_f32;
            let callback_calls = Arc::new(AtomicU64::new(0));
            let rendered_frames = Arc::new(AtomicU64::new(0));
            let callback_calls_cb = callback_calls.clone();
            let rendered_frames_cb = rendered_frames.clone();
            let amplitude = options.amp;

            if let Err(error) =
                stream.set_render_callback(move |_info: CallbackInfo, out: &mut [f32]| {
                    callback_calls_cb.fetch_add(1, Ordering::Relaxed);
                    rendered_frames_cb.fetch_add((out.len() / channels) as u64, Ordering::Relaxed);
                    for frame in out.chunks_exact_mut(channels) {
                        let sample = phase.sin() * amplitude;
                        phase += phase_step;
                        if phase >= 2.0_f32 * core::f32::consts::PI {
                            phase -= 2.0_f32 * core::f32::consts::PI;
                        }
                        frame.fill(sample);
                    }
                })
            {
                eprintln!("Could not set callback: {error:?}");
                return;
            }

            println!(
                "Starting Windows stream: {:?} @ {} Hz, tone {} Hz, amp {}, {}s",
                stream.config().direction,
                stream.config().sample_rate_hz,
                options.freq_hz,
                options.amp,
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
            println!(
                "Callback calls: {}, rendered frames: {}",
                callback_calls.load(Ordering::Relaxed),
                rendered_frames.load(Ordering::Relaxed)
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
            println!("Could not create Windows stream: {error:?}");
        }
    }
}
