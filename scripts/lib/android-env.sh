#!/usr/bin/env bash

# Shared Android environment helpers for project scripts.

find_ndk() {
  if [ -n "${ANDROID_NDK_HOME:-}" ] && [ -d "${ANDROID_NDK_HOME}" ]; then
    echo "${ANDROID_NDK_HOME}"
    return 0
  fi

  if [ -n "${ANDROID_SDK_ROOT:-}" ] && [ -d "${ANDROID_SDK_ROOT}/ndk" ]; then
    local latest
    latest="$(ls -1 "${ANDROID_SDK_ROOT}/ndk" 2>/dev/null | sort -V | tail -n1)"
    if [ -n "$latest" ] && [ -d "${ANDROID_SDK_ROOT}/ndk/${latest}" ]; then
      echo "${ANDROID_SDK_ROOT}/ndk/${latest}"
      return 0
    fi
  fi

  if [ -d "$HOME/Android/Sdk/ndk" ]; then
    local latest_home
    latest_home="$(ls -1 "$HOME/Android/Sdk/ndk" 2>/dev/null | sort -V | tail -n1)"
    if [ -n "$latest_home" ] && [ -d "$HOME/Android/Sdk/ndk/${latest_home}" ]; then
      echo "$HOME/Android/Sdk/ndk/${latest_home}"
      return 0
    fi
  fi

  return 1
}

find_adb() {
  if command -v adb >/dev/null 2>&1; then
    command -v adb
    return 0
  fi

  if [ -n "${ADB:-}" ] && [ -x "${ADB}" ]; then
    echo "${ADB}"
    return 0
  fi

  if [ -n "${ANDROID_SDK_ROOT:-}" ] && [ -x "${ANDROID_SDK_ROOT}/platform-tools/adb" ]; then
    echo "${ANDROID_SDK_ROOT}/platform-tools/adb"
    return 0
  fi

  if [ -x "/mnt/c/Users/User/AppData/Local/Android/Sdk/platform-tools/adb.exe" ]; then
    echo "/mnt/c/Users/User/AppData/Local/Android/Sdk/platform-tools/adb.exe"
    return 0
  fi

  return 1
}

configure_android_cc_env() {
  local target="$1"
  local ndk_root="$2"
  local toolchain_bin="${ndk_root}/toolchains/llvm/prebuilt/linux-x86_64/bin"
  local api="${ANDROID_PLATFORM_API:-24}"
  local nosized="-fno-sized-deallocation"

  if [ ! -d "${toolchain_bin}" ]; then
    echo "error: could not find NDK llvm toolchain at ${toolchain_bin}" >&2
    echo "set ANDROID_NDK_HOME to a valid Linux NDK path" >&2
    return 1
  fi

  export AR="${toolchain_bin}/llvm-ar"
  case "$target" in
    aarch64-linux-android)
      export CC_aarch64_linux_android="${toolchain_bin}/aarch64-linux-android${api}-clang"
      export CXX_aarch64_linux_android="${toolchain_bin}/aarch64-linux-android${api}-clang++"
      export CXXFLAGS_aarch64_linux_android="${CXXFLAGS_aarch64_linux_android:-} ${nosized}"
      ;;
    x86_64-linux-android)
      export CC_x86_64_linux_android="${toolchain_bin}/x86_64-linux-android${api}-clang"
      export CXX_x86_64_linux_android="${toolchain_bin}/x86_64-linux-android${api}-clang++"
      export CXXFLAGS_x86_64_linux_android="${CXXFLAGS_x86_64_linux_android:-} ${nosized}"
      ;;
    *)
      echo "error: unsupported Android target for toolchain env: ${target}" >&2
      return 1
      ;;
  esac
}
