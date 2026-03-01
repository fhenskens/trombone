# Known Limitations

This file lists current known limitations for Trombone alpha.

## Project Stage

- Trombone is currently in **alpha** stage.
- API and behavior can still change.

## Platform Coverage

- Current focus is Android.
- Other platforms are planned but not complete yet.

## Android Backend Notes

- AAudio is the main backend path right now.
- OpenSL ES support is present, but behavior can vary by device/emulator.
- Some emulator setups reject OpenSL stream creation with `BackendFailure { code: 2 }`.
- Runtime test script may skip OpenSL checks on those unsupported environments.

## Emulator vs Real Device

- Emulator audio can be stuttery even when xruns are zero.
- Audio quality should be judged on a real physical device.

## Benchmarking Notes

- `examples/bench.rs` (Trombone benchmark) supports output/input/duplex.
- `examples/oboe_bench.rs` supports output/input/duplex.
- On physical Android devices, recent AAudio duplex runs show near-parity with `oboe-rs` for callback timing.
- Keep comparing on real devices because emulator behavior can differ a lot.

## Build and Tooling Notes

- Android builds require a Linux NDK path when using WSL.
- `oboe_bench` uses extra C++ linker/toolchain settings in scripts.
- `oboe` dependency is feature-gated and used only for `oboe_bench`.

## Metrics and Reporting Notes

- Observed callback frame size is the best "actual callback size" signal.
- Negotiated burst values from backends may not always map cleanly to callback buffer sizes.
- Use observed metrics for practical comparison and performance judgment.

## Not Yet Ready for Stable Public Release

- Full cross-platform support.
- Long-run soak validation across many device models.
- Complete oboe parity testing in all modes.
