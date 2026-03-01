use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use trombone::backend::AudioBackend;
use trombone::backend::android::{AndroidBackend, AndroidBackendKind};
use trombone::core::callback::CallbackInfo;
use trombone::core::config::{Direction, StreamConfig};

struct DuplexOptions {
    seconds: u64,
    gain: f32,
    backend: AndroidBackendKind,
}

impl Default for DuplexOptions {
    fn default() -> Self {
        Self {
            seconds: 5,
            gain: 1.0,
            backend: AndroidBackendKind::Auto,
        }
    }
}

fn print_help() {
    println!("Duplex demo options:");
    println!("  --backend <auto|aaudio|opensl> backend choice (default: auto)");
    println!("  --seconds <n>    Run time in seconds (default: 5)");
    println!("  --gain <value>   Playback gain (default: 1.0)");
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
    let mono = NonZeroU32::new(1).expect("literal is non-zero");

    let input_config = StreamConfig {
        channels: mono,
        direction: Direction::Input,
        ..StreamConfig::default()
    };
    let output_config = StreamConfig {
        channels: mono,
        direction: Direction::Output,
        ..StreamConfig::default()
    };

    let mut input_stream = match backend.create_stream(input_config) {
        Ok(stream) => stream,
        Err(error) => {
            eprintln!("Could not create input stream: {error:?}");
            return;
        }
    };

    let mut output_stream = match backend.create_stream(output_config) {
        Ok(stream) => stream,
        Err(error) => {
            eprintln!("Could not create output stream: {error:?}");
            return;
        }
    };

    let ring = Arc::new(Mutex::new(SampleRing::with_capacity(48_000)));
    let input_calls = Arc::new(AtomicU64::new(0));
    let output_calls = Arc::new(AtomicU64::new(0));
    let dropped_output_samples = Arc::new(AtomicU64::new(0));
    let captured_samples = Arc::new(AtomicU64::new(0));
    let played_samples = Arc::new(AtomicU64::new(0));

    {
        let ring = ring.clone();
        let input_calls = input_calls.clone();
        let captured_samples = captured_samples.clone();
        if let Err(error) =
            input_stream.set_capture_callback(move |_info: CallbackInfo, input: &[f32]| {
                input_calls.fetch_add(1, Ordering::Relaxed);
                captured_samples.fetch_add(input.len() as u64, Ordering::Relaxed);
                if let Ok(mut rb) = ring.lock() {
                    rb.push_slice(input);
                }
            })
        {
            eprintln!("Could not set input callback: {error:?}");
            return;
        }
    }

    {
        let ring = ring.clone();
        let output_calls = output_calls.clone();
        let dropped_output_samples = dropped_output_samples.clone();
        let played_samples = played_samples.clone();
        let gain = options.gain;
        if let Err(error) =
            output_stream.set_render_callback(move |_info: CallbackInfo, out: &mut [f32]| {
                output_calls.fetch_add(1, Ordering::Relaxed);
                let mut read_count = 0_usize;
                if let Ok(mut rb) = ring.lock() {
                    read_count = rb.pop_into(out);
                }
                for sample in out.iter_mut().take(read_count) {
                    *sample *= gain;
                }
                if read_count < out.len() {
                    out[read_count..].fill(0.0);
                    dropped_output_samples.fetch_add(
                        (out.len().saturating_sub(read_count)) as u64,
                        Ordering::Relaxed,
                    );
                }
                played_samples.fetch_add(read_count as u64, Ordering::Relaxed);
            })
        {
            eprintln!("Could not set output callback: {error:?}");
            return;
        }
    }

    println!(
        "Starting duplex stream for {}s (gain: {})",
        options.seconds, options.gain
    );
    if let Err(error) = output_stream.start() {
        eprintln!("Could not start output stream: {error:?}");
        return;
    }
    if let Err(error) = input_stream.start() {
        eprintln!("Could not start input stream: {error:?}");
        let _ = output_stream.stop();
        return;
    }

    std::thread::sleep(std::time::Duration::from_secs(options.seconds));

    if let Err(error) = input_stream.stop() {
        eprintln!("Could not stop input stream: {error:?}");
    }
    if let Err(error) = output_stream.stop() {
        eprintln!("Could not stop output stream: {error:?}");
    }

    let in_metrics = input_stream.metrics();
    let out_metrics = output_stream.metrics();
    println!(
        "Input callbacks: {}, captured samples: {}",
        input_calls.load(Ordering::Relaxed),
        captured_samples.load(Ordering::Relaxed)
    );
    println!(
        "Output callbacks: {}, played samples: {}, zero-filled samples: {}",
        output_calls.load(Ordering::Relaxed),
        played_samples.load(Ordering::Relaxed),
        dropped_output_samples.load(Ordering::Relaxed)
    );
    println!(
        "Input metrics: xruns={}, frames_written={:?}, frames_read={:?}",
        in_metrics.xrun_count, in_metrics.frames_written, in_metrics.frames_read
    );
    println!(
        "Input timing: callback_time_ns={:?}, backend_time_ns={:?}, frame_position={:?}, latency_frames={:?}, latency_ns={:?}",
        in_metrics.timing.callback_time_ns,
        in_metrics.timing.backend_time_ns,
        in_metrics.timing.frame_position,
        in_metrics.timing.estimated_latency_frames,
        in_metrics.timing.estimated_latency_ns
    );
    println!(
        "Output metrics: xruns={}, frames_written={:?}, frames_read={:?}",
        out_metrics.xrun_count, out_metrics.frames_written, out_metrics.frames_read
    );
    println!(
        "Output timing: callback_time_ns={:?}, backend_time_ns={:?}, frame_position={:?}, latency_frames={:?}, latency_ns={:?}",
        out_metrics.timing.callback_time_ns,
        out_metrics.timing.backend_time_ns,
        out_metrics.timing.frame_position,
        out_metrics.timing.estimated_latency_frames,
        out_metrics.timing.estimated_latency_ns
    );
    println!("Done.");
}
