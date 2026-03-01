#!/usr/bin/env bash
set -euo pipefail

ok() { echo "[OK]  $*"; }
warn() { echo "[WARN] $*"; }
err() { echo "[ERR] $*"; }

find_adb() {
  if command -v adb >/dev/null 2>&1; then
    command -v adb
    return
  fi
  if [ -n "${ADB:-}" ] && [ -x "${ADB}" ]; then
    echo "${ADB}"
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

find_ndk() {
  if [ -n "${ANDROID_NDK_HOME:-}" ] && [ -d "${ANDROID_NDK_HOME}" ]; then
    echo "${ANDROID_NDK_HOME}"
    return
  fi
  if [ -n "${ANDROID_SDK_ROOT:-}" ] && [ -d "${ANDROID_SDK_ROOT}/ndk" ]; then
    local latest
    latest="$(ls -1 "${ANDROID_SDK_ROOT}/ndk" 2>/dev/null | sort -V | tail -n1)"
    if [ -n "$latest" ] && [ -d "${ANDROID_SDK_ROOT}/ndk/${latest}" ]; then
      echo "${ANDROID_SDK_ROOT}/ndk/${latest}"
      return
    fi
  fi
  if [ -d "$HOME/Android/Sdk/ndk" ]; then
    local latest_home
    latest_home="$(ls -1 "$HOME/Android/Sdk/ndk" 2>/dev/null | sort -V | tail -n1)"
    if [ -n "$latest_home" ] && [ -d "$HOME/Android/Sdk/ndk/${latest_home}" ]; then
      echo "$HOME/Android/Sdk/ndk/${latest_home}"
      return
    fi
  fi
  return 1
}

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
