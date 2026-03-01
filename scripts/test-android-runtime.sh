#!/usr/bin/env bash
set -euo pipefail

fail() {
  echo "error: $*" >&2
  exit 1
}

is_required() {
  [ "${ANDROID_RUNTIME_REQUIRED:-0}" = "1" ]
}

find_adb() {
  if command -v adb >/dev/null 2>&1; then
    command -v adb
    return
  fi
  if [ -n "${ANDROID_SDK_ROOT:-}" ] && [ -x "${ANDROID_SDK_ROOT}/platform-tools/adb" ]; then
    echo "${ANDROID_SDK_ROOT}/platform-tools/adb"
    return
  fi
  if [ -x "/mnt/c/Users/User/AppData/Local/Android/Sdk/platform-tools/adb.exe" ]; then
    echo "/mnt/c/Users/User/AppData/Local/Android/Sdk/platform-tools/adb.exe"
    return
  fi
  return 1
}

adb_devices_snapshot() {
  local adb_bin="$1"
  "$adb_bin" devices 2>&1 || true
}

list_online_devices() {
  local adb_bin="$1"
  "$adb_bin" devices | awk 'NR>1 { gsub(/\r/, "", $2); if ($2=="device") print $1 }'
}

pick_serial_once() {
  local adb_bin="$1"
  mapfile -t devices < <(list_online_devices "$adb_bin")
  if [ "${#devices[@]}" -eq 1 ]; then
    echo "${devices[0]}"
    return 0
  fi
  if [ "${#devices[@]}" -eq 0 ]; then
    return 1
  fi
  fail "multiple adb devices detected; set ANDROID_TEST_SERIAL
adb path: ${adb_bin}
adb devices:
$(adb_devices_snapshot "$adb_bin")"
}

pick_serial() {
  local adb_bin="$1"
  if [ -n "${ANDROID_TEST_SERIAL:-}" ]; then
    if list_online_devices "$adb_bin" | awk -v serial="${ANDROID_TEST_SERIAL}" '$1==serial {found=1} END {exit(found?0:1)}'; then
      echo "${ANDROID_TEST_SERIAL}"
      return
    fi
    fail "ANDROID_TEST_SERIAL is set but not online as a device: ${ANDROID_TEST_SERIAL}
adb path: ${adb_bin}
adb devices:
$(adb_devices_snapshot "$adb_bin")"
  fi

  # Emulator/device state can race for a moment right after startup.
  local serial=""
  for _ in 1 2 3; do
    serial="$(pick_serial_once "$adb_bin" || true)"
    if [ -n "$serial" ]; then
      echo "$serial"
      return 0
    fi
    sleep 1
  done
  return 1
}

target_from_abi() {
  case "$1" in
    x86_64) echo "x86_64-linux-android" ;;
    arm64-v8a) echo "aarch64-linux-android" ;;
    *) fail "unsupported ABI for tests: $1" ;;
  esac
}

extract_pair() {
  # Reads first line matching pattern and returns "a b" where a and b are numbers.
  # shellcheck disable=SC2001
  sed -nE "s/.*$1: ([0-9]+), $2: ([0-9]+).*/\\1 \\2/p" | head -n1
}

ADB_BIN="$(find_adb || true)"
if [ -z "$ADB_BIN" ]; then
  if is_required; then
    fail "adb not found but ANDROID_RUNTIME_REQUIRED=1"
  fi
  echo "Android runtime tests skipped: adb not found (set ANDROID_SDK_ROOT or install adb)"
  exit 0
fi

SERIAL="$(pick_serial "$ADB_BIN" || true)"
if [ -z "$SERIAL" ]; then
  if is_required; then
    fail "no single online adb device found but ANDROID_RUNTIME_REQUIRED=1
adb path: ${ADB_BIN}
adb devices:
$(adb_devices_snapshot "$ADB_BIN")"
  fi
  echo "Android runtime tests skipped: no single online device"
  echo "adb path: ${ADB_BIN}"
  echo "adb devices:"
  adb_devices_snapshot "$ADB_BIN"
  echo "Tip: set ANDROID_TEST_SERIAL=<serial> if a device is online."
  exit 0
fi

ABI="$("$ADB_BIN" -s "$SERIAL" shell getprop ro.product.cpu.abi | tr -d '\r\n')"
TARGET="$(target_from_abi "$ABI")"

echo "Android runtime test device: ${SERIAL} (${ABI})"
echo "Building runtime examples for ${TARGET}..."
cargo build --target "$TARGET" --example tone --example capture --example duplex

echo "Running tone example..."
tone_output="$(
  ./scripts/run-android.sh \
    --serial "$SERIAL" \
    --target "$TARGET" \
    --example tone \
    --no-build \
    -- \
    --seconds 2 --freq 440 --amp 0.2 2>&1
)"
echo "$tone_output"
read -r tone_calls tone_frames < <(echo "$tone_output" | extract_pair "Callback calls" "rendered frames")
[ "${tone_calls:-0}" -gt 0 ] || fail "tone callback count is zero"
[ "${tone_frames:-0}" -gt 0 ] || fail "tone rendered frames is zero"

for backend in aaudio opensl; do
  debug_env=()
  if [ "$backend" = "opensl" ]; then
    debug_env=("TROMBONE_BACKEND_DEBUG=1")
  fi

  echo "Running tone example via ${backend}..."
  backend_tone_output="$(
    env "${debug_env[@]}" ./scripts/run-android.sh \
      --serial "$SERIAL" \
      --target "$TARGET" \
      --example tone \
      --no-build \
      -- \
      --seconds 2 --freq 440 --amp 0.2 --backend "$backend" 2>&1
  )"
  echo "$backend_tone_output"
  if [ "$backend" = "opensl" ] && echo "$backend_tone_output" | grep -q "Could not create stream: BackendFailure { code: 2 }"; then
    echo "OpenSL runtime tests skipped on this device/emulator: create_audio_player returned parameter invalid (code 2)."
    continue
  fi
  read -r backend_calls backend_frames < <(echo "$backend_tone_output" | extract_pair "Callback calls" "rendered frames")
  [ "${backend_calls:-0}" -gt 0 ] || fail "${backend} tone callback count is zero"
  [ "${backend_frames:-0}" -gt 0 ] || fail "${backend} tone rendered frames is zero"
  echo "$backend_tone_output" | grep -q "latency_frames=Some(" || fail "${backend} tone timing is missing latency"

  echo "Running capture example via ${backend}..."
  backend_capture_output="$(
    env "${debug_env[@]}" ./scripts/run-android.sh \
      --serial "$SERIAL" \
      --target "$TARGET" \
      --example capture \
      --no-build \
      -- \
      --seconds 2 --backend "$backend" 2>&1
  )"
  echo "$backend_capture_output"
  if [ "$backend" = "opensl" ] && echo "$backend_capture_output" | grep -q "Could not create input stream: BackendFailure { code: 2 }"; then
    echo "OpenSL runtime tests skipped on this device/emulator: recorder creation returned parameter invalid (code 2)."
    continue
  fi
  read -r backend_capture_calls backend_capture_samples < <(echo "$backend_capture_output" | extract_pair "Callback calls" "captured samples")
  [ "${backend_capture_calls:-0}" -gt 0 ] || fail "${backend} capture callback count is zero"
  [ "${backend_capture_samples:-0}" -gt 0 ] || fail "${backend} capture sample count is zero"
  echo "$backend_capture_output" | grep -q "latency_frames=Some(" || fail "${backend} capture timing is missing latency"

  echo "Running duplex example via ${backend}..."
  backend_duplex_output="$(
    env "${debug_env[@]}" ./scripts/run-android.sh \
      --serial "$SERIAL" \
      --target "$TARGET" \
      --example duplex \
      --no-build \
      -- \
      --seconds 2 --gain 1.0 --backend "$backend" 2>&1
  )"
  echo "$backend_duplex_output"
  if [ "$backend" = "opensl" ] && echo "$backend_duplex_output" | grep -q "Could not create .*stream: BackendFailure { code: 2 }"; then
    echo "OpenSL runtime tests skipped on this device/emulator: duplex stream creation returned parameter invalid (code 2)."
    continue
  fi
  read -r backend_duplex_in_calls backend_duplex_in_samples < <(echo "$backend_duplex_output" | extract_pair "Input callbacks" "captured samples")
  read -r backend_duplex_out_calls backend_duplex_out_samples < <(echo "$backend_duplex_output" | extract_pair "Output callbacks" "played samples")
  [ "${backend_duplex_in_calls:-0}" -gt 0 ] || fail "${backend} duplex input callback count is zero"
  [ "${backend_duplex_in_samples:-0}" -gt 0 ] || fail "${backend} duplex captured sample count is zero"
  [ "${backend_duplex_out_calls:-0}" -gt 0 ] || fail "${backend} duplex output callback count is zero"
  [ "${backend_duplex_out_samples:-0}" -gt 0 ] || fail "${backend} duplex played sample count is zero"
  echo "$backend_duplex_output" | grep -q "Input timing: .*latency_frames=Some(" || fail "${backend} duplex input timing is missing latency"
  echo "$backend_duplex_output" | grep -q "Output timing: .*latency_frames=Some(" || fail "${backend} duplex output timing is missing latency"
done

echo "Android runtime tests passed."
