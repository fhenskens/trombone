#!/usr/bin/env bash
set -euo pipefail

if [ -z "${ANDROID_SDK_ROOT:-}" ] && [ -d "$HOME/Android/Sdk" ]; then
  export ANDROID_SDK_ROOT="$HOME/Android/Sdk"
fi

if [ -z "${ANDROID_NDK_HOME:-}" ]; then
  if [ -n "${ANDROID_SDK_ROOT:-}" ] && [ -d "$ANDROID_SDK_ROOT/ndk" ]; then
    newest_ndk="$(ls -1 "$ANDROID_SDK_ROOT/ndk" | sort -V | tail -n 1)"
    export ANDROID_NDK_HOME="$ANDROID_SDK_ROOT/ndk/$newest_ndk"
  fi
fi

if [ -z "${ANDROID_NDK_HOME:-}" ] || [ ! -d "$ANDROID_NDK_HOME" ]; then
  echo "error: ANDROID_NDK_HOME is not set to a valid Linux NDK path" >&2
  echo "Set ANDROID_NDK_HOME to something like: ~/Android/Sdk/ndk/<version>" >&2
  exit 1
fi

clang="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android26-clang"
if [ ! -x "$clang" ]; then
  echo "error: Linux NDK clang wrapper not found at: $clang" >&2
  echo "Your NDK looks like a Windows-only install. Install Linux NDK in WSL." >&2
  exit 1
fi

exec "$clang" "$@"
