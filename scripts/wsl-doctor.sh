#!/usr/bin/env bash
set -euo pipefail

ok() { echo "[OK]  $*"; }
warn() { echo "[WARN] $*"; }
err() { echo "[ERR] $*"; }

main() {
  local fail=0

  echo "== Trombone WSL Doctor =="
  echo "cwd: $(pwd)"
  echo

  if grep -qi microsoft /proc/version 2>/dev/null; then
    ok "Running inside WSL"
  else
    warn "Not running inside WSL (this script is WSL-focused)"
  fi

  if ps -p 1 -o comm= 2>/dev/null | grep -q "^systemd$"; then
    ok "systemd is enabled in WSL"
  else
    warn "systemd not detected (set /etc/wsl.conf with [boot] systemd=true)"
  fi

  if command -v rustc >/dev/null 2>&1; then
    ok "rustc: $(rustc --version)"
  else
    err "rustc not found"
    fail=1
  fi

  if command -v cargo >/dev/null 2>&1; then
    ok "cargo: $(cargo --version)"
  else
    err "cargo not found"
    fail=1
  fi

  if command -v pkg-config >/dev/null 2>&1; then
    ok "pkg-config found"
  else
    warn "pkg-config not found (sudo apt install pkg-config)"
  fi

  if pkg-config --exists alsa; then
    ok "ALSA dev package is available"
  else
    warn "ALSA dev package missing (sudo apt install libasound2-dev)"
  fi

  if command -v aplay >/dev/null 2>&1; then
    ok "aplay found"
    local has_sink=0
    if aplay -L 2>/dev/null | grep -Eiq '(^|\s)(pulse|pipewire|default)(\s|$)'; then
      has_sink=1
    fi
    if [ "$has_sink" -eq 1 ]; then
      ok "ALSA playback aliases include pulse/pipewire/default"
    else
      warn "No pulse/pipewire/default ALSA aliases found in 'aplay -L'"
      warn "Install plugins: sudo apt install libasound2-plugins"
    fi
  else
    warn "aplay not found (sudo apt install alsa-utils)"
  fi

  if [ -n "${PULSE_SERVER:-}" ]; then
    ok "PULSE_SERVER is set: ${PULSE_SERVER}"
  else
    warn "PULSE_SERVER is unset (WSLg often uses unix:/mnt/wslg/PulseServer)"
  fi

  if [ -S /mnt/wslg/PulseServer ]; then
    ok "WSLg Pulse socket exists: /mnt/wslg/PulseServer"
  else
    warn "WSLg Pulse socket missing at /mnt/wslg/PulseServer"
  fi

  if [ -f "${HOME}/.asoundrc" ]; then
    if grep -q "type pulse" "${HOME}/.asoundrc"; then
      ok "~/.asoundrc has pulse default"
    else
      warn "~/.asoundrc exists but no 'type pulse' default found"
    fi
  else
    warn "~/.asoundrc missing (recommended to route default ALSA to pulse)"
  fi

  if [ -n "${ANDROID_SDK_ROOT:-}" ]; then
    if [ -d "${ANDROID_SDK_ROOT}" ]; then
      ok "ANDROID_SDK_ROOT=${ANDROID_SDK_ROOT}"
    else
      warn "ANDROID_SDK_ROOT is set but directory is missing: ${ANDROID_SDK_ROOT}"
    fi
  else
    warn "ANDROID_SDK_ROOT is unset (needed for Android scripts)"
  fi

  if [ -n "${ANDROID_NDK_HOME:-}" ]; then
    if [ -d "${ANDROID_NDK_HOME}" ]; then
      ok "ANDROID_NDK_HOME=${ANDROID_NDK_HOME}"
    else
      warn "ANDROID_NDK_HOME is set but directory is missing: ${ANDROID_NDK_HOME}"
    fi
  else
    warn "ANDROID_NDK_HOME is unset (needed for Android build scripts)"
  fi

  if [ -n "${ADB:-}" ]; then
    if [ -x "${ADB}" ]; then
      ok "ADB=${ADB}"
    else
      warn "ADB is set but not executable: ${ADB}"
    fi
  else
    if command -v adb >/dev/null 2>&1; then
      ok "adb found in PATH: $(command -v adb)"
    else
      warn "adb not found in PATH and ADB env var is unset"
    fi
  fi

  echo
  echo "Suggested fixes:"
  echo "  1) sudo apt install -y libasound2-dev libasound2-plugins alsa-utils pkg-config"
  echo "  2) export PULSE_SERVER=unix:/mnt/wslg/PulseServer"
  echo "  3) set TROMBONE_ALSA_DEVICE=pulse when running Linux examples in WSL"
  echo

  if [ "$fail" -eq 0 ]; then
    ok "WSL doctor checks completed"
  else
    err "WSL doctor found blocking issues"
    exit 1
  fi
}

main "$@"

