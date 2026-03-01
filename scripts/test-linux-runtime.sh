#!/usr/bin/env bash
set -euo pipefail

fail() {
  echo "error: $*" >&2
  exit 1
}

is_required() {
  [ "${LINUX_RUNTIME_REQUIRED:-0}" = "1" ]
}

ensure_linux_host() {
  if ! uname -s | grep -qi linux; then
    if is_required; then
      fail "linux runtime tests require a Linux host"
    fi
    echo "Linux runtime tests skipped: host is not Linux"
    exit 0
  fi
}

extract_pair() {
  sed -nE "s/.*$1: ([0-9]+), $2: ([0-9]+).*/\\1 \\2/p" | head -n1
}

require_command_or_skip() {
  local cmd="$1"
  if command -v "$cmd" >/dev/null 2>&1; then
    return 0
  fi
  if is_required; then
    fail "required command not found: $cmd"
  fi
  echo "Linux runtime tests skipped: missing command '$cmd'"
  exit 0
}

run_capture() {
  set +e
  local output
  output="$("$@" 2>&1)"
  local status=$?
  set -e
  printf '%s' "$output"
  return "$status"
}

run_with_timeout() {
  if command -v timeout >/dev/null 2>&1; then
    timeout "${LINUX_RUNTIME_TIMEOUT:-20s}" "$@"
  else
    "$@"
  fi
}

ensure_linux_host
require_command_or_skip cargo
require_command_or_skip rustup

if [ -z "${TROMBONE_ALSA_DEVICE:-}" ]; then
  export TROMBONE_ALSA_DEVICE="pulse"
fi

echo "Linux runtime tests using ALSA device: ${TROMBONE_ALSA_DEVICE}"
echo "Building Linux runtime examples..."
cargo build --example linux_tone --example linux_capture --example linux_duplex --example linux_bench

echo "Running linux_tone..."
tone_status=0
tone_output="$(run_capture run_with_timeout ./scripts/run-linux.sh --example linux_tone --no-build -- --backend alsa --seconds 2 --freq 440 --amp 0.2)" || tone_status=$?
echo "$tone_output"
if [ "$tone_status" -ne 0 ] || echo "$tone_output" | grep -q "Could not start stream"; then
  if is_required; then
    fail "linux_tone could not start stream"
  fi
  echo "Linux runtime tests skipped: tone stream could not start"
  exit 0
fi
read -r tone_calls tone_frames < <(echo "$tone_output" | extract_pair "Callback calls" "rendered frames")
[ "${tone_calls:-0}" -gt 0 ] || fail "linux_tone callback count is zero"
[ "${tone_frames:-0}" -gt 0 ] || fail "linux_tone rendered frames is zero"

echo "Running linux_capture..."
capture_status=0
capture_output="$(run_capture run_with_timeout ./scripts/run-linux.sh --example linux_capture --no-build -- --backend alsa --seconds 2)" || capture_status=$?
echo "$capture_output"
if [ "$capture_status" -ne 0 ] || echo "$capture_output" | grep -q "Could not start stream"; then
  if is_required; then
    fail "linux_capture could not start stream"
  fi
  echo "Linux runtime tests skipped: capture stream could not start"
  exit 0
fi
read -r capture_calls capture_samples < <(echo "$capture_output" | extract_pair "Callback calls" "captured samples")
[ "${capture_calls:-0}" -gt 0 ] || fail "linux_capture callback count is zero"
[ "${capture_samples:-0}" -gt 0 ] || fail "linux_capture sample count is zero"

echo "Running linux_duplex..."
duplex_status=0
duplex_output="$(run_capture run_with_timeout ./scripts/run-linux.sh --example linux_duplex --no-build -- --backend alsa --seconds 2 --gain 1.0)" || duplex_status=$?
echo "$duplex_output"
if [ "$duplex_status" -ne 0 ] || echo "$duplex_output" | grep -q "Could not start .*stream"; then
  if is_required; then
    fail "linux_duplex could not start stream"
  fi
  echo "Linux runtime tests skipped: duplex stream could not start"
  exit 0
fi
read -r duplex_in_calls duplex_in_samples < <(echo "$duplex_output" | extract_pair "Input callbacks" "captured samples")
read -r duplex_out_calls duplex_out_samples < <(echo "$duplex_output" | extract_pair "Output callbacks" "played samples")
[ "${duplex_in_calls:-0}" -gt 0 ] || fail "linux_duplex input callback count is zero"
[ "${duplex_in_samples:-0}" -gt 0 ] || fail "linux_duplex captured sample count is zero"
[ "${duplex_out_calls:-0}" -gt 0 ] || fail "linux_duplex output callback count is zero"
[ "${duplex_out_samples:-0}" -gt 0 ] || fail "linux_duplex played sample count is zero"

echo "Running linux_bench (csv)..."
bench_status=0
bench_output="$(run_capture run_with_timeout ./scripts/run-linux.sh --release --example linux_bench -- --mode duplex --backend alsa --seconds 3 --format csv)" || bench_status=$?
echo "$bench_output"
[ "$bench_status" -eq 0 ] || fail "linux_bench command failed"
echo "$bench_output" | grep -q "^mode,backend,seconds," || fail "linux_bench csv header missing"
echo "$bench_output" | grep -q "^Duplex,Alsa," || fail "linux_bench csv row missing"

echo "Linux runtime tests passed."
