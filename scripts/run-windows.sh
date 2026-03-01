#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat << 'EOF'
Usage:
  ./scripts/run-windows.sh [--target <rust-target>] [--bin <name> | --example <name>] [--wasapi-mode <auto|shared|exclusive>] [--release] [--no-build] [--] [app-args...]

Examples:
  ./scripts/run-windows.sh
  ./scripts/run-windows.sh --example windows_capture -- --seconds 5
  ./scripts/run-windows.sh --example windows_duplex --release -- --seconds 5 --gain 1.0
  ./scripts/run-windows.sh --example windows_bench --wasapi-mode exclusive -- --mode duplex --seconds 10 --format csv
  ./scripts/run-windows.sh --target x86_64-pc-windows-msvc --example windows_tone -- --freq 880 --seconds 2
EOF
}

TARGET="${WINDOWS_TARGET:-x86_64-pc-windows-msvc}"
PROFILE="debug"
NO_BUILD=0
ARTIFACT_KIND="example"
ARTIFACT_NAME="windows_tone"
WASAPI_MODE="${TROMBONE_WASAPI_MODE:-}"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --target)
      [ "$#" -ge 2 ] || { echo "error: --target needs a value" >&2; exit 1; }
      TARGET="$2"
      shift 2
      ;;
    --release)
      PROFILE="release"
      shift
      ;;
    --wasapi-mode)
      [ "$#" -ge 2 ] || { echo "error: --wasapi-mode needs a value" >&2; exit 1; }
      WASAPI_MODE="$2"
      shift 2
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

if [ -n "$WASAPI_MODE" ]; then
  case "$WASAPI_MODE" in
    auto|shared|exclusive) ;;
    *)
      echo "error: invalid --wasapi-mode value: $WASAPI_MODE (expected auto|shared|exclusive)" >&2
      exit 1
      ;;
  esac
fi

if ! rustup target list --installed | grep -q "^${TARGET}$"; then
  echo "error: missing Rust target ${TARGET}" >&2
  echo "install with: rustup target add ${TARGET}" >&2
  exit 1
fi

if [ "$NO_BUILD" -eq 0 ]; then
  echo "Building ${ARTIFACT_KIND} ${ARTIFACT_NAME} for ${TARGET} (${PROFILE})..."
  if [ "$PROFILE" = "release" ]; then
    if [ "$ARTIFACT_KIND" = "example" ]; then
      cargo build --target "$TARGET" --release --example "$ARTIFACT_NAME"
    else
      cargo build --target "$TARGET" --release --bin "$ARTIFACT_NAME"
    fi
  else
    if [ "$ARTIFACT_KIND" = "example" ]; then
      cargo build --target "$TARGET" --example "$ARTIFACT_NAME"
    else
      cargo build --target "$TARGET" --bin "$ARTIFACT_NAME"
    fi
  fi
fi

if [ "$ARTIFACT_KIND" = "example" ]; then
  LOCAL_BIN="target/${TARGET}/${PROFILE}/examples/${ARTIFACT_NAME}.exe"
else
  LOCAL_BIN="target/${TARGET}/${PROFILE}/${ARTIFACT_NAME}.exe"
fi

if [ ! -f "$LOCAL_BIN" ]; then
  echo "error: built executable not found: $LOCAL_BIN" >&2
  exit 1
fi

echo "Running Windows executable: ${LOCAL_BIN}"
if [ -n "$WASAPI_MODE" ]; then
  echo "WASAPI mode: ${WASAPI_MODE}"
  TROMBONE_WASAPI_MODE="$WASAPI_MODE" "$LOCAL_BIN" "${APP_ARGS[@]}"
else
  "$LOCAL_BIN" "${APP_ARGS[@]}"
fi
