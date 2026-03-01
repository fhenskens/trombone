use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use trombone::backend::AudioBackend;
use trombone::backend::windows::{WindowsBackend, WindowsBackendKind};
use trombone::core::callback::CallbackInfo;
use trombone::core::config::{Direction, StreamConfig};

struct DuplexOptions {
    seconds: u64,
    gain: f32,
}

impl Default for DuplexOptions {
    fn default() -> Self {
        Self {
            seconds: 5,
            gain: 1.0,
        }
    }
}

fn print_help() {
    println!("Windows duplex demo options:");
    println!("  --seconds <n>    Duplex length in seconds (default: 5)");
    println!("  --gain <0..4>    Output gain (default: 1.0)");
    println!("  --help           Show this help");
}

fn parse_args() -> Result<DuplexOptions, String> {
    let mut options = DuplexOptions::default();
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

    let backend = WindowsBackend::new(WindowsBackendKind::Wasapi);
    let input_config = StreamConfig {
        direction: Direction::Input,
        ..StreamConfig::default()
    };
    let output_config = StreamConfig {
        direction: Direction::Output,
        ..StreamConfig::default()
    };

    let mut input_stream = match backend.create_stream(input_config) {
        Ok(v) => v,
        Err(error) => {
            eprintln!("Could not create input stream: {error:?}");
            return;
        }
    };
    let mut output_stream = match backend.create_stream(output_config) {
        Ok(v) => v,
        Err(error) => {
            eprintln!("Could not create output stream: {error:?}");
            return;
        }
    };

    let ring = Arc::new(Mutex::new(VecDeque::<f32>::with_capacity(48_000)));
    let in_callbacks = Arc::new(AtomicU64::new(0));
    let in_samples = Arc::new(AtomicU64::new(0));
    let out_callbacks = Arc::new(AtomicU64::new(0));
    let out_played = Arc::new(AtomicU64::new(0));
    let out_zero_filled = Arc::new(AtomicU64::new(0));
    let gain = options.gain;

    {
        let ring_cb = ring.clone();
        let in_callbacks_cb = in_callbacks.clone();
        let in_samples_cb = in_samples.clone();
        if let Err(error) =
            input_stream.set_capture_callback(move |_info: CallbackInfo, input: &[f32]| {
                in_callbacks_cb.fetch_add(1, Ordering::Relaxed);
                in_samples_cb.fetch_add(input.len() as u64, Ordering::Relaxed);
                if let Ok(mut rb) = ring_cb.lock() {
                    for sample in input {
                        if rb.len() >= 48_000 {
                            let _ = rb.pop_front();
                        }
                        rb.push_back(*sample);
                    }
                }
            })
        {
            eprintln!("Could not set input callback: {error:?}");
            return;
        }
    }

    {
        let ring_cb = ring.clone();
        let out_callbacks_cb = out_callbacks.clone();
        let out_played_cb = out_played.clone();
        let out_zero_filled_cb = out_zero_filled.clone();
        if let Err(error) =
            output_stream.set_render_callback(move |_info: CallbackInfo, out: &mut [f32]| {
                out_callbacks_cb.fetch_add(1, Ordering::Relaxed);
                let mut played = 0_u64;
                let mut zeroed = 0_u64;
                if let Ok(mut rb) = ring_cb.lock() {
                    for sample in out {
                        if let Some(v) = rb.pop_front() {
                            *sample = v * gain;
                            played += 1;
                        } else {
                            *sample = 0.0;
                            zeroed += 1;
                        }
                    }
                } else {
                    out.fill(0.0);
                    zeroed += out.len() as u64;
                }
                out_played_cb.fetch_add(played, Ordering::Relaxed);
                out_zero_filled_cb.fetch_add(zeroed, Ordering::Relaxed);
            })
        {
            eprintln!("Could not set output callback: {error:?}");
            return;
        }
    }

    println!(
        "Starting Windows duplex stream for {}s (gain: {})",
        options.seconds, options.gain
    );
    if let Err(error) = output_stream.start() {
        eprintln!("Could not start output stream: {error:?}");
        return;
    }
    if let Err(error) = input_stream.start() {
        let _ = output_stream.stop();
        eprintln!("Could not start input stream: {error:?}");
        return;
    }

    std::thread::sleep(std::time::Duration::from_secs(options.seconds));

    if let Err(error) = input_stream.stop() {
        eprintln!("Could not stop input stream: {error:?}");
    }
    if let Err(error) = output_stream.stop() {
        eprintln!("Could not stop output stream: {error:?}");
    }

    let input_metrics = input_stream.metrics();
    let output_metrics = output_stream.metrics();

    println!(
        "Input callbacks: {}, captured samples: {}",
        in_callbacks.load(Ordering::Relaxed),
        in_samples.load(Ordering::Relaxed)
    );
    println!(
        "Output callbacks: {}, played samples: {}, zero-filled samples: {}",
        out_callbacks.load(Ordering::Relaxed),
        out_played.load(Ordering::Relaxed),
        out_zero_filled.load(Ordering::Relaxed)
    );
    println!(
        "Input metrics: xruns={}, frames_written={:?}, frames_read={:?}",
        input_metrics.xrun_count, input_metrics.frames_written, input_metrics.frames_read
    );
    println!(
        "Output metrics: xruns={}, frames_written={:?}, frames_read={:?}",
        output_metrics.xrun_count, output_metrics.frames_written, output_metrics.frames_read
    );
    println!("Done.");
}
