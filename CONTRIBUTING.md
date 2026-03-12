# Contributing to Trombone

Thanks for helping with Trombone.
This file explains how to contribute.

## Prerequisites

- Rust `1.85` or newer (MSRV).
- `rustfmt` and `clippy` components installed.

## Setup

```bash
git clone git@github.com:fhenskens/trombone.git
cd trombone
cargo check
```

## Development Standards

- Keep callback-path code real-time-safe:
  no allocation, no blocking locks, and no syscalls in the hot path.
- Keep PRs small and focused.
- Update docs when public APIs change.
- Add tests for behavior changes.

## Required Checks

Run these before opening a PR:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo check --all-targets
```

If you have an Android device/emulator available, also run:

```bash
./scripts/test-android-runtime.sh
```

In CI, runtime tests run with `ANDROID_RUNTIME_REQUIRED=1`, so they fail if no device is available.

## Pull Requests

- Link related issues.
- Explain what changed and which platforms are affected.
- Note any latency, glitch, or recovery impact.
- List follow-up work if something is intentionally left for later.

## Commit Guidance

- Use clear commit messages in imperative voice.
- Keep unrelated refactors out of feature/fix commits.

## Reporting Issues

When reporting a bug, include:

- Platform and device details.
- Steps to reproduce.
- Expected vs actual behavior.
- Logs and error codes when possible.

## Code of Conduct

By participating, you agree to follow [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
