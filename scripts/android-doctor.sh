#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/lib/android-env.sh
source "${SCRIPT_DIR}/lib/android-env.sh"

ok() { echo "[OK]  $*"; }
warn() { echo "[WARN] $*"; }
err() { echo "[ERR] $*"; }

main() {
  local fail=0

  echo "== Trombone Android Doctor =="
  echo "cwd: $(pwd)"
  echo

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

  if rustup target list --installed | grep -q '^x86_64-linux-android$'; then
    ok "rust target installed: x86_64-linux-android"
  else
    warn "missing rust target: x86_64-linux-android (run: rustup target add x86_64-linux-android)"
  fi

  if rustup target list --installed | grep -q '^aarch64-linux-android$'; then
    ok "rust target installed: aarch64-linux-android"
  else
    warn "missing rust target: aarch64-linux-android (run: rustup target add aarch64-linux-android)"
  fi

  echo "ANDROID_SDK_ROOT=${ANDROID_SDK_ROOT:-<unset>}"
  local ndk_root
  ndk_root="$(find_ndk || true)"
  if [ -n "$ndk_root" ]; then
    ok "NDK: ${ndk_root}"
    local tc="${ndk_root}/toolchains/llvm/prebuilt/linux-x86_64/bin"
    if [ -x "${tc}/aarch64-linux-android26-clang" ] && [ -x "${tc}/x86_64-linux-android26-clang" ]; then
      ok "NDK clang wrappers found"
    else
      warn "NDK clang wrappers not found at expected path: ${tc}"
    fi
  else
    err "ANDROID_NDK_HOME/SDK NDK not found"
    fail=1
  fi

  local adb_bin
  adb_bin="$(find_adb || true)"
  if [ -n "$adb_bin" ]; then
    ok "adb: ${adb_bin}"
    echo "adb devices:"
    "$adb_bin" devices || true
  else
    err "adb not found"
    fail=1
  fi

  echo
  if [ "$fail" -eq 0 ]; then
    ok "doctor checks completed"
  else
    err "doctor found blocking issues"
    exit 1
  fi
}

main "$@"
