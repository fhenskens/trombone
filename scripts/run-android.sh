#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat << 'EOF'
Usage:
  ./scripts/run-android.sh [--serial <adb-serial>] [--target <rust-target>] [--bin <name> | --example <name>] [--release] [--no-build] [--] [app-args...]

Examples:
  ./scripts/run-android.sh
  ./scripts/run-android.sh --serial emulator-5554
  ./scripts/run-android.sh --example tone --serial emulator-5554
  ./scripts/run-android.sh --target aarch64-linux-android --release
  ./scripts/run-android.sh -- --help
EOF
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

  echo "error: adb not found in PATH or common SDK locations" >&2
  exit 1
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

pick_serial() {
  local adb_bin="$1"
  local serial_override="$2"

  if [ -n "$serial_override" ]; then
    echo "$serial_override"
    return
  fi

  mapfile -t devices < <("$adb_bin" devices | awk 'NR>1 { gsub(/\r/, "", $2); if ($2=="device") print $1 }')
  if [ "${#devices[@]}" -eq 0 ]; then
    echo "error: no online adb device found" >&2
    exit 1
  fi
  if [ "${#devices[@]}" -gt 1 ]; then
    echo "error: multiple adb devices found, pass --serial" >&2
    printf 'devices:\n'
    printf '  %s\n' "${devices[@]}"
    exit 1
  fi

  echo "${devices[0]}"
}

target_from_abi() {
  local abi="$1"
  case "$abi" in
    x86_64) echo "x86_64-linux-android" ;;
    arm64-v8a) echo "aarch64-linux-android" ;;
    *)
      echo "error: unsupported device ABI: $abi" >&2
      echo "supported ABIs: x86_64, arm64-v8a" >&2
      exit 1
      ;;
  esac
}

SERIAL=""
TARGET=""
PROFILE="debug"
NO_BUILD=0
ARTIFACT_KIND="bin"
ARTIFACT_NAME="trombone"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --serial)
      [ "$#" -ge 2 ] || { echo "error: --serial needs a value" >&2; exit 1; }
      SERIAL="$2"
      shift 2
      ;;
    --target)
      [ "$#" -ge 2 ] || { echo "error: --target needs a value" >&2; exit 1; }
      TARGET="$2"
      shift 2
      ;;
    --release)
      PROFILE="release"
      shift
      ;;
    --bin)
      [ "$#" -ge 2 ] || { echo "error: --bin needs a value" >&2; exit 1; }
      ARTIFACT_KIND="bin"
      ARTIFACT_NAME="$2"
      shift 2
      ;;
    --example)
      [ "$#" -ge 2 ] || { echo "error: --example needs a value" >&2; exit 1; }
      ARTIFACT_KIND="example"
      ARTIFACT_NAME="$2"
      shift 2
      ;;
    --no-build)
      NO_BUILD=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --)
      shift
      break
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      usage
      exit 1
      ;;
  esac
done

APP_ARGS=("$@")
ADB_BIN="$(find_adb)"
SERIAL="$(pick_serial "$ADB_BIN" "$SERIAL")"

if [ -z "$TARGET" ]; then
  ABI="$("$ADB_BIN" -s "$SERIAL" shell getprop ro.product.cpu.abi | tr -d '\r\n')"
  TARGET="$(target_from_abi "$ABI")"
fi

if [ "$NO_BUILD" -eq 0 ]; then
  if ! rustup target list --installed | grep -q "^${TARGET}$"; then
    echo "error: missing Rust target ${TARGET}" >&2
    echo "install with: rustup target add ${TARGET}" >&2
    exit 1
  fi

  NDK_ROOT="$(find_ndk || true)"
  if [ -n "${NDK_ROOT}" ]; then
    configure_android_cc_env "$TARGET" "$NDK_ROOT"
  fi

  CARGO_EXTRA_ENV=()
  CARGO_FEATURE_ARGS=()
  if [ "$ARTIFACT_KIND" = "example" ] && [ "$ARTIFACT_NAME" = "oboe_bench" ]; then
    CARGO_FEATURE_ARGS+=("--features" "oboe-bench")
    # oboe-sys links against libc++_static; force static section for that library.
    # This avoids unresolved operator new/delete on some NDK + linker combinations.
    CARGO_EXTRA_ENV+=(
      "RUSTFLAGS=-Clink-arg=-Wl,-Bstatic -Clink-arg=-lc++_static -Clink-arg=-lc++abi -Clink-arg=-Wl,-Bdynamic"
    )
  fi

  echo "Building ${ARTIFACT_KIND} ${ARTIFACT_NAME} for ${TARGET} (${PROFILE})..."
  if [ "$PROFILE" = "release" ]; then
    if [ "$ARTIFACT_KIND" = "example" ]; then
      env "${CARGO_EXTRA_ENV[@]}" cargo build --target "$TARGET" --release --example "$ARTIFACT_NAME" "${CARGO_FEATURE_ARGS[@]}"
    else
      env "${CARGO_EXTRA_ENV[@]}" cargo build --target "$TARGET" --release --bin "$ARTIFACT_NAME"
    fi
  else
    if [ "$ARTIFACT_KIND" = "example" ]; then
      env "${CARGO_EXTRA_ENV[@]}" cargo build --target "$TARGET" --example "$ARTIFACT_NAME" "${CARGO_FEATURE_ARGS[@]}"
    else
      env "${CARGO_EXTRA_ENV[@]}" cargo build --target "$TARGET" --bin "$ARTIFACT_NAME"
    fi
  fi
fi

if [ "$ARTIFACT_KIND" = "example" ]; then
  LOCAL_BIN="target/${TARGET}/${PROFILE}/examples/${ARTIFACT_NAME}"
else
  LOCAL_BIN="target/${TARGET}/${PROFILE}/${ARTIFACT_NAME}"
fi
REMOTE_BIN="/data/local/tmp/${ARTIFACT_NAME}"

if [ ! -f "$LOCAL_BIN" ]; then
  echo "error: built binary not found: $LOCAL_BIN" >&2
  exit 1
fi

echo "Pushing binary to ${SERIAL}..."
"$ADB_BIN" -s "$SERIAL" push "$LOCAL_BIN" "$REMOTE_BIN" >/dev/null
"$ADB_BIN" -s "$SERIAL" shell chmod 755 "$REMOTE_BIN"

echo "Running on ${SERIAL} (${TARGET})..."
REMOTE_ENV=()
if [ -n "${TROMBONE_BACKEND_DEBUG:-}" ]; then
  REMOTE_ENV+=("TROMBONE_BACKEND_DEBUG=${TROMBONE_BACKEND_DEBUG}")
fi
if [ -n "${TROMBONE_DEBUG_BACKEND:-}" ]; then
  REMOTE_ENV+=("TROMBONE_DEBUG_BACKEND=${TROMBONE_DEBUG_BACKEND}")
fi

if [ "${#REMOTE_ENV[@]}" -gt 0 ]; then
  "$ADB_BIN" -s "$SERIAL" shell env "${REMOTE_ENV[@]}" "$REMOTE_BIN" "${APP_ARGS[@]}"
else
  "$ADB_BIN" -s "$SERIAL" shell "$REMOTE_BIN" "${APP_ARGS[@]}"
fi
