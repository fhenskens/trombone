# Trombone

Trombone is a Rust audio I/O library for low-latency audio.
The goal is to support all major platforms over time.

## Project Goals

- Provide a real-time-safe callback API.
- Support major platforms with one clear API.
- Use native backends on each platform.
- Keep stream states and errors easy to understand.

## Non-Goals (v0)

- A full DSP graph framework.
- Auto-tuning that hides backend limits.
- Full parity with every backend feature in v0.

## Current Status

Current code includes:

- Core types for stream config, callback, state, and errors.
- Backend trait and Android backend selection.
- AAudio backend for input and output on Android.
- OpenSL ES backend for input and output on Android.
- Auto backend mode: tries AAudio, then falls back to OpenSL ES.
- Runtime metrics with timing and latency estimates.
- Android runtime conformance script that checks both backends.

## Roadmap

1. M1: Android output with AAudio.
2. M2: Android input and OpenSL ES fallback.
3. M3: Apple backend (CoreAudio for iOS/macOS).
4. M4: Windows backend (WASAPI).
5. M5: Linux backend (PipeWire and/or ALSA).
6. M6: Cross-platform tests and latency profiling.

## First Technical Milestone (M1)

M1 is done when:

- Output stream can open, start, and stop on Android.
- Callback path avoids allocation in the real-time thread.
- Example tone generator plays without glitches.

## Development

Requirements:

- Rust stable toolchain with `rustfmt` and `clippy`.

Local quality checks:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo check --all-targets
```

Android build (emulator + device targets):

```bash
./scripts/build-android.sh
```

Check your local Android toolchain/device setup:

```bash
./scripts/android-doctor.sh
```

Build, push, and run on connected Android device/emulator:

```bash
./scripts/run-android.sh
```

Pick a specific device:

```bash
./scripts/run-android.sh --serial emulator-5554
```

Run tone example with custom settings:

```bash
./scripts/run-android.sh --example tone --serial emulator-5554 -- --freq 1000 --amp 0.5 --seconds 3
```

Force tone example to use OpenSL ES backend:

```bash
./scripts/run-android.sh --example tone --serial emulator-5554 -- --backend opensl --freq 440 --amp 0.2 --seconds 3
```

Run capture example with live level meter:

```bash
./scripts/run-android.sh --example capture --serial emulator-5554 -- --seconds 5
```

Run capture with a specific backend:

```bash
./scripts/run-android.sh --example capture --serial emulator-5554 -- --backend opensl --seconds 5
```

Run duplex passthrough (mic to speaker):

```bash
./scripts/run-android.sh --example duplex --serial emulator-5554 -- --seconds 5 --gain 1.0
```

Run duplex with a specific backend:

```bash
./scripts/run-android.sh --example duplex --serial emulator-5554 -- --backend aaudio --seconds 5 --gain 1.0
```

Run benchmark example (human-readable):

```bash
./scripts/run-android.sh --release --example bench --serial emulator-5554 -- --mode output --backend aaudio --seconds 10
```

Run benchmark example (CSV output for A/B comparison):

```bash
./scripts/run-android.sh --release --example bench --serial emulator-5554 -- --mode duplex --backend opensl --seconds 10 --format csv
```

Run `oboe-rs` benchmark example (same CSV columns for comparison):

```bash
./scripts/run-android.sh --release --example oboe_bench --serial emulator-5554 -- --mode output --backend aaudio --seconds 10 --channels 1 --format csv
```

Run side-by-side benchmark comparison (Trombone vs oboe-rs):

```bash
./scripts/bench-compare.sh --serial <serial> --mode output --backend aaudio --seconds 10 --channels 1 --frames-per-burst 192
```

Recent duplex AAudio comparison on a physical Android device showed near parity:
- output and input interval averages were within about 1% of `oboe-rs`
- output and input `p95` were also within about 1%
- zero outliers over `2x` and `5x` median interval in both libraries

Run Android runtime conformance tests (tone + capture + duplex on AAudio and OpenSL ES):

```bash
./scripts/test-android-runtime.sh
```

Require runtime tests to fail instead of skip when no device is available:

```bash
ANDROID_RUNTIME_REQUIRED=1 ./scripts/test-android-runtime.sh
```

WSL note:

- If you build from WSL, install an Android **Linux** NDK in WSL.
- Building from WSL with `windows-x86_64` NDK tools will fail at link time.
- Set `ANDROID_SDK_ROOT` and `ANDROID_NDK_HOME` in WSL, for example:

```bash
export ANDROID_SDK_ROOT="$HOME/Android/Sdk"
export ANDROID_NDK_HOME="$ANDROID_SDK_ROOT/ndk/<version>"
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for how to contribute.
See [KNOWN_LIMITATIONS.md](KNOWN_LIMITATIONS.md) for current gaps.
See [ALPHA_RELEASE_CHECKLIST.md](ALPHA_RELEASE_CHECKLIST.md) for release prep.
