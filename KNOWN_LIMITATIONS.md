# Known Limitations

This file lists current known limitations for Trombone alpha.

## Project Stage

- Trombone is currently in **alpha** stage.
- API and behavior can still change.

## Platform Coverage

- Android and Windows runtime paths are implemented.
- Linux backend has initial ALSA runtime support for output/input/duplex.
- Linux `Auto` preference order is set to `PipeWire` first, then `ALSA`.
- Native PipeWire output and input paths are implemented behind Cargo feature `linux-pipewire`.
- Native PipeWire duplex path has not yet been validated on a real native Linux installation.
- Building with native PipeWire feature requires `libpipewire-0.3-dev` (or equivalent) on Linux.
- Native PipeWire output/input paths also still need native Linux validation runs.
- WSL audio path is best-effort for development and may sound choppy despite stable callback timing.
- Linux latency and quality should be judged on native Linux, not WSL audio bridging.
- Current focus is Android parity and Windows latency tuning.
- Other platforms are planned but not complete yet.

## Windows Backend Notes

- WASAPI supports event-driven `shared` and `exclusive` init paths.
- Default mode is `auto` (`exclusive` first, then fallback to `shared`).
- Some devices may reject exclusive mode depending on current system audio usage.
- Shared mode callback periods often stay near the Windows engine period (~10ms at 48kHz).

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
