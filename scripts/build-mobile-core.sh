#!/usr/bin/env bash
# Build and stage the shared Rust store/peer protocol. Does not launch an app or tests.
set -euo pipefail
cd "$(dirname "$0")/.."
platform="${1:-ios-simulator}"
case "$platform" in
  ios-simulator) triple=aarch64-apple-ios-sim; export SDKROOT="$(xcrun --sdk iphonesimulator --show-sdk-path)"; export IPHONEOS_DEPLOYMENT_TARGET=17.0 ;;
  ios-device) triple=aarch64-apple-ios; export SDKROOT="$(xcrun --sdk iphoneos --show-sdk-path)"; export IPHONEOS_DEPLOYMENT_TARGET=17.0 ;;
  android)
    triple=aarch64-linux-android
    : "${ANDROID_NDK_HOME:?Set ANDROID_NDK_HOME to an installed NDK}"
    case "$(uname -s)" in Darwin) host=darwin-x86_64;; Linux) host=linux-x86_64;; *) echo 'Use a macOS or Linux host' >&2; exit 1;; esac
    bin="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$host/bin"
    export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$bin/aarch64-linux-android31-clang"
    export CC_aarch64_linux_android="$bin/aarch64-linux-android31-clang"
    export AR_aarch64_linux_android="$bin/llvm-ar"
    ;;
  *) echo 'Usage: build-mobile-core.sh ios-simulator|ios-device|android' >&2; exit 1;;
esac
args=(build -p command-center-mobile-peer --target "$triple" --locked)
# Normal Rust installations: rustup target add <triple>. Homebrew Rust may instead
# provide matching standard-library sources; opt in explicitly to rebuilding std.
if [[ "${PCC_BUILD_STD:-0}" == 1 ]]; then export RUSTC_BOOTSTRAP=1; args+=(-Z build-std=std,panic_abort); fi
if [[ "${PCC_OFFLINE:-0}" == 1 ]]; then args+=(--offline); fi
cargo "${args[@]}"
if [[ "$platform" == android ]]; then
  mkdir -p apps/mobile-android/app/src/main/jniLibs/arm64-v8a
  cp "target/$triple/debug/libpcc_mobile_peer.so" apps/mobile-android/app/src/main/jniLibs/arm64-v8a/
else
  mkdir -p target/mobile-ios-core
  cp "target/$triple/debug/libpcc_mobile_peer.a" target/mobile-ios-core/
  printf '%s\n' "$triple" > target/mobile-ios-core/target-triple.txt
fi
printf 'Staged shared core for %s. Build the matching native app next.\n' "$platform"
