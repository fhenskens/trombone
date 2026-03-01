#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat << 'EOF'
Usage:
  ./scripts/run-linux.sh [--target <rust-target>] [--bin <name> | --example <name>] [--release] [--no-build] [--] [app-args...]

Examples:
  ./scripts/run-linux.sh
  ./scripts/run-linux.sh --example linux_capture -- --seconds 5
  ./scripts/run-linux.sh --example linux_duplex --release -- --seconds 5 --gain 1.0
  ./scripts/run-linux.sh --example linux_bench --release -- --mode duplex --seconds 10 --format csv
EOF
}

TARGET="${LINUX_TARGET:-}"
PROFILE="debug"
NO_BUILD=0
ARTIFACT_KIND="example"
ARTIFACT_NAME="linux_tone"

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

if [ -n "$TARGET" ]; then
  if ! rustup target list --installed | grep -q "^${TARGET}$"; then
    echo "error: missing Rust target ${TARGET}" >&2
    echo "install with: rustup target add ${TARGET}" >&2
    exit 1
  fi
fi

if [ "$NO_BUILD" -eq 0 ]; then
  if [ -n "$TARGET" ]; then
    echo "Building ${ARTIFACT_KIND} ${ARTIFACT_NAME} for ${TARGET} (${PROFILE})..."
  else
    echo "Building ${ARTIFACT_KIND} ${ARTIFACT_NAME} for host (${PROFILE})..."
  fi
  if [ "$PROFILE" = "release" ]; then
    if [ "$ARTIFACT_KIND" = "example" ]; then
      if [ -n "$TARGET" ]; then
        cargo build --target "$TARGET" --release --example "$ARTIFACT_NAME"
      else
        cargo build --release --example "$ARTIFACT_NAME"
      fi
    else
      if [ -n "$TARGET" ]; then
        cargo build --target "$TARGET" --release --bin "$ARTIFACT_NAME"
      else
        cargo build --release --bin "$ARTIFACT_NAME"
      fi
    fi
  else
    if [ "$ARTIFACT_KIND" = "example" ]; then
      if [ -n "$TARGET" ]; then
        cargo build --target "$TARGET" --example "$ARTIFACT_NAME"
      else
        cargo build --example "$ARTIFACT_NAME"
      fi
    else
      if [ -n "$TARGET" ]; then
        cargo build --target "$TARGET" --bin "$ARTIFACT_NAME"
      else
        cargo build --bin "$ARTIFACT_NAME"
      fi
    fi
  fi
fi

if [ "$ARTIFACT_KIND" = "example" ]; then
  if [ -n "$TARGET" ]; then
    LOCAL_BIN="target/${TARGET}/${PROFILE}/examples/${ARTIFACT_NAME}"
  else
    LOCAL_BIN="target/${PROFILE}/examples/${ARTIFACT_NAME}"
  fi
else
  if [ -n "$TARGET" ]; then
    LOCAL_BIN="target/${TARGET}/${PROFILE}/${ARTIFACT_NAME}"
  else
    LOCAL_BIN="target/${PROFILE}/${ARTIFACT_NAME}"
  fi
fi

if [ ! -f "$LOCAL_BIN" ]; then
  echo "error: built executable not found: $LOCAL_BIN" >&2
  exit 1
fi

echo "Running Linux executable: ${LOCAL_BIN}"
"$LOCAL_BIN" "${APP_ARGS[@]}"
