#!/usr/bin/env bash
set -euo pipefail

TARGETS=(
  "x86_64-linux-android"
  "aarch64-linux-android"
)

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

configure_android_cc_env() {
  local target="$1"
  local ndk_root="$2"
  local toolchain_bin="${ndk_root}/toolchains/llvm/prebuilt/linux-x86_64/bin"
  local api="${ANDROID_PLATFORM_API:-24}"
  local nosized="-fno-sized-deallocation"

  if [ ! -d "${toolchain_bin}" ]; then
    echo "error: could not find NDK llvm toolchain at ${toolchain_bin}" >&2
    echo "set ANDROID_NDK_HOME to a valid Linux NDK path" >&2
    exit 1
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
  esac
}

missing_targets=()
for target in "${TARGETS[@]}"; do
  if ! rustup target list --installed | grep -q "^${target}$"; then
    missing_targets+=("${target}")
  fi
done

if [ "${#missing_targets[@]}" -gt 0 ]; then
  echo "Missing Rust targets:"
  for target in "${missing_targets[@]}"; do
    echo "  - ${target}"
  done
  echo
  echo "Install them with:"
  echo "  rustup target add ${missing_targets[*]}"
  exit 1
fi

NDK_ROOT="$(find_ndk || true)"
if [ -n "${NDK_ROOT}" ]; then
  echo "Using Android NDK: ${NDK_ROOT}"
fi

for target in "${TARGETS[@]}"; do
  echo "Building ${target}..."
  if [ -n "${NDK_ROOT}" ]; then
    configure_android_cc_env "$target" "$NDK_ROOT"
  fi
  cargo build --target "${target}"
done

echo "Android build finished for: ${TARGETS[*]}"
