#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
export JAVA_HOME=/usr/lib/jvm/java-17-openjdk
export PATH="$JAVA_HOME/bin:$PATH"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/28.0.12674087}"
profile=release
if [ "${1:-}" = '--debug' ]; then profile=debug; fi
mkdir -p "target/$profile/apk/classes"
# cargo-apk2 compiles against this directory and dexes it with the application's classes.
unzip -oq .build/vlc/classes.jar -d "target/$profile/apk/classes"
for spec in mdpi:48 hdpi:72 xhdpi:96 xxhdpi:144 xxxhdpi:192; do
    mkdir -p "res/mipmap-${spec%:*}"
    rsvg-convert -w "${spec#*:}" -h "${spec#*:}" icon/ic_launcher.svg -o "res/mipmap-${spec%:*}/ic_launcher.png"
done
if [ "${1:-}" = '--emulator' ]; then
    mkdir -p java/com/github/egui_mobile
    framework=$(cargo metadata --format-version 1 --filter-platform x86_64-linux-android | jq -r '.packages[] | select(.name == "egui-android") | .manifest_path | rtrimstr("/Cargo.toml")')
    cp "$framework/java/com/github/egui_mobile/"*.java java/com/github/egui_mobile/
    cargo apk2 build --target x86_64-linux-android --release
elif [ "$profile" = release ]; then cargo egui-mobile build -a --release
else cargo egui-mobile build -a
fi
