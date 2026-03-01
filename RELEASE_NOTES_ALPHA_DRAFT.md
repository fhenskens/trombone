# Trombone Alpha Release Notes (Draft)

Trombone is in alpha. The current focus is Android.

## Highlights

- Android AAudio output, input, and duplex paths are working.
- OpenSL ES backend is available as a fallback.
- Runtime test script is included for Android device checks.
- Side-by-side benchmark script is included for Trombone vs `oboe-rs`.

## Benchmark Snapshot (Android Device)

Setup:

- mode: duplex
- backend: AAudio
- sample rate: 48000
- channels: 1
- frames per burst requested: 192
- duration: 10 seconds
- device serial: `3B010DLJG001GR`

Result summary:

- Trombone and `oboe-rs` are near parity for callback timing.
- Output and input average interval are within about 1%.
- Output and input p95 are within about 1%.
- Outlier rates over `2x` and `5x` median are 0% for both.
- Xruns are 0 for both.

Raw comparison output:

```text
=== Benchmark Comparison ===
mode=duplex backend=aaudio serial=3B010DLJG001GR

metric                         trombone         oboe             delta
out_callbacks                  2541             2540             +0.04%
out_samples                    479808           480000           -0.04%
out_xruns                      0                0                n/a
out_observed_frames            192.000          192.000          +0.00%
out_avg_us                     3994.470         3999.045         -0.11%
out_p95_us                     4028.280         4021.932         +0.16%
out_p50_us                     3999.674         3999.756         -0.00%
out_p99_us                     4316.243         4272.949         +1.01%
out_p95_trimmed_2x_us          4028.280         4021.932         +0.16%
out_outliers_over_2x           0                0                n/a
out_outlier_rate_over_2x       0.00%            0.00%            n/a
out_outliers_over_5x           0                0                n/a
out_outlier_rate_over_5x       0.00%            0.00%            n/a
in_callbacks                   2499             2500             -0.04%
in_samples                     479808           480000           -0.04%
in_xruns                       0                0                n/a
in_observed_frames             192.000          192.000          +0.00%
in_avg_us                      4000.357         4000.256         +0.00%
in_p95_us                      4053.467         4022.176         +0.78%
in_p50_us                      3999.959         3999.797         +0.00%
in_p99_us                      4155.395         4132.975         +0.54%
in_p95_trimmed_2x_us           4053.467         4022.176         +0.78%
in_outliers_over_2x            0                0                n/a
in_outlier_rate_over_2x        0.00%            0.00%            n/a
in_outliers_over_5x            0                0                n/a
in_outlier_rate_over_5x        0.00%            0.00%            n/a
duplex_zero_filled             8064             7680             +5.00%
```

## Known Limits (Alpha)

- Main validation has been on Android so far.
- Emulator behavior can differ from real devices.
- API and behavior may still change between alpha versions.

## Who Should Use This Alpha

- People testing Android audio behavior and reporting issues.
- Contributors who want to help build cross-platform support.

## Next Focus

- Expand device coverage for Android runtime and benchmark checks.
- Continue backend and callback-path hardening.
- Build out macOS/iOS, Windows, and Linux backends.
