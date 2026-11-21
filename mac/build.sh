#!/bin/sh
set -eu
cd "$(dirname "$0")/.."

if [ "$(uname -s)" != Darwin ]; then
  echo "Build SyncAiDync.app on macOS." >&2
  exit 1
fi

case "${1:-}" in
  ''|--build-only) ;;
  *) echo "Usage: mac/build.sh [--build-only]" >&2; exit 2 ;;
esac

BUILD="$PWD/target/mac"
APP="$BUILD/SyncAiDync.app"
CONTENTS="$APP/Contents"
RESOURCES="$CONTENTS/Resources"
ICONS="$BUILD/SyncAiDync.iconset"
SDK_CACHE="$BUILD/module-cache"
ARCH="$(uname -m)-apple-macosx13.0"

mkdir -p "$CONTENTS/MacOS" "$RESOURCES" "$ICONS" "$SDK_CACHE"
cargo build --release --locked --offline -p syncaidync
swiftc -parse-as-library -module-cache-path "$SDK_CACHE" -target "$ARCH" -typecheck mac/SyncAiDync.swift
swiftc -parse-as-library -module-cache-path "$SDK_CACHE" -target "$ARCH" -O mac/SyncAiDync.swift \
  -framework AppKit -framework WebKit -framework UserNotifications -framework Carbon -framework Security \
  -o "$CONTENTS/MacOS/SyncAiDync"

if ! sips -s format png mac/icon.svg --out "$BUILD/icon.png" >/dev/null 2>&1; then
  swiftc -module-cache-path "$SDK_CACHE" -target "$ARCH" -O mac/make-icon.swift -o "$BUILD/make-icon"
  "$BUILD/make-icon" "$BUILD/icon.png"
fi
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$BUILD/icon.png" --out "$ICONS/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" "$BUILD/icon.png" --out "$ICONS/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONS" -o "$RESOURCES/SyncAiDync.icns"
cp target/release/syncaidync "$RESOURCES/syncaidync"
cp mac/Info.plist "$CONTENTS/Info.plist"
version=$(sed -n 's/^version = "\([^"]*\)"$/\1/p' Cargo.toml)
if [ "$(target/release/syncaidync --version)" != "syncaidync $version" ]; then
  echo "Cargo and server versions differ." >&2
  exit 1
fi
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $version" "$CONTENTS/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $version" "$CONTENTS/Info.plist"
codesign --force --deep --sign - "$APP"
codesign --verify --deep --strict "$APP"

if [ "${1:-}" != "--build-only" ]; then
  mkdir -p "$HOME/Applications"
  ditto "$APP" "$HOME/Applications/SyncAiDync.app"
  APP="$HOME/Applications/SyncAiDync.app"
fi

printf '%s\n' "$APP"
