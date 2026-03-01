#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/lib/android-env.sh
source "${SCRIPT_DIR}/lib/android-env.sh"

TARGETS=(
  "x86_64-linux-android"
  "aarch64-linux-android"
)

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
