# Alpha Release Checklist

This checklist is for an **alpha** release of Trombone.
It is not for a stable production release.

## 1. Scope and Message

- [ ] State clearly that this is an alpha release.
- [ ] State what is supported now (mainly Android).
- [ ] State what is not ready yet.
- [ ] Add a short "who should use this" note.

## 2. Versioning

- [ ] Bump `Cargo.toml` version for the alpha tag.
- [ ] Use a clear pre-release tag (example: `v0.2.0-alpha.1`).
- [ ] Add release notes for new features and known limits.

## 3. Quality Gates

Run these before tagging:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Android runtime checks:

```bash
./scripts/test-android-runtime.sh
```

- [ ] All commands above pass on the release commit.
- [ ] CI is green on the release commit.

## 4. Device Validation (Minimum)

Test on at least one physical Android device:

- [ ] Output (`tone`) works.
- [ ] Input (`capture`) works.
- [ ] Duplex works.
- [ ] No xruns in short runs under normal load.

Recommended command examples:

```bash
./scripts/run-android.sh --release --example tone --serial <serial> -- --backend aaudio --seconds 5
./scripts/run-android.sh --release --example capture --serial <serial> -- --backend aaudio --seconds 5
./scripts/run-android.sh --release --example duplex --serial <serial> -- --backend aaudio --seconds 5
```

## 5. Benchmark Snapshot (Optional but Useful)

Run both benchmarks with matching settings:

```bash
./scripts/run-android.sh --release --example bench --serial <serial> -- --mode output --backend aaudio --seconds 10 --channels 1 --sample-rate 48000 --frames-per-burst 192 --format csv
./scripts/run-android.sh --release --example oboe_bench --serial <serial> -- --mode output --backend aaudio --seconds 10 --channels 1 --sample-rate 48000 --frames-per-burst 192 --format csv
```

- [ ] Save the CSV output in release notes or an issue link.

## 6. Documentation

- [ ] `README.md` reflects current real status.
- [ ] `KNOWN_LIMITATIONS.md` is up to date.
- [ ] `CONTRIBUTING.md` includes current test commands.

## 7. Release Steps

- [ ] Merge release commit to `prod`.
- [ ] Confirm CI passes on `prod`.
- [ ] Confirm GitHub release workflow runs.
- [ ] Verify release tag and notes are published.

## 8. After Release

- [ ] Open a tracking issue for alpha feedback.
- [ ] Add top issues found during release testing.
- [ ] Prioritize blockers for beta milestone.
