#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

OUT=ios/Generated
rm -rf "$OUT" ios/LocalDropCore.xcframework
mkdir -p "$OUT/tmp" "$OUT/headers"

cargo build -p ld-ffi --release --target aarch64-apple-ios
cargo build -p ld-ffi --release --target aarch64-apple-ios-sim
cargo build -p ld-ffi --target aarch64-apple-ios-sim
cargo run -p ld-ffi --features cli --bin uniffi-bindgen generate \
  --library target/aarch64-apple-ios-sim/debug/libld_ffi.a \
  --language swift --out-dir "$OUT/tmp"

mv "$OUT/tmp/ld_ffi.swift" "$OUT/ld_ffi.swift"
cp "$OUT/tmp/ld_ffiFFI.h" "$OUT/headers/"
cp "$OUT/tmp/ld_ffiFFI.modulemap" "$OUT/headers/module.modulemap"
rm -rf "$OUT/tmp"

xcodebuild -create-xcframework \
  -library target/aarch64-apple-ios/release/libld_ffi.a -headers "$OUT/headers" \
  -library target/aarch64-apple-ios-sim/release/libld_ffi.a -headers "$OUT/headers" \
  -output ios/LocalDropCore.xcframework

echo "Готово: ios/LocalDropCore.xcframework и ios/Generated/ld_ffi.swift"