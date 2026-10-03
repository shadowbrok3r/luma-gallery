#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
: "${AS_URL:?AS_URL is required}" "${AS_KEY:?AS_KEY is required}"
apk=target/release/apk/luma_gallery.apk
version=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].version')
IFS=. read -r major minor patch <<< "$version"
code=$(( (1 << 24) | (major << 16) | (minor << 8) | patch ))
notes=$(jq -rn --arg notes "${AS_NOTES:-GPU video, scrub loupe, trimming and full-resolution RAW}" '$notes|@uri')
aapt=$(find "$HOME/Android/Sdk/build-tools" -name aapt2 | sort -V | tail -1)
badging=$("$aapt" dump badging "$apk")
[[ "$badging" == *"versionCode='$code'"* && "$badging" == *"native-code: 'arm64-v8a'"* ]] || { echo 'Wrong APK version or ABI' >&2; exit 1; }
if ! curl -fsS -H "x-api-key: $AS_KEY" "$AS_URL/api/apps" | jq -e '.apps[] | select(.slug=="luma-gallery")' >/dev/null; then
    curl -fsS -X POST -H "x-api-key: $AS_KEY" -H 'Content-Type: application/json' \
        --data '{"slug":"luma-gallery","name":"Luma Gallery","package":"app.luma.gallery","notes":"Sony video and RAW gallery"}' \
        "$AS_URL/api/apps/create"
fi
curl -fsS --fail-with-body -X POST -H "x-api-key: $AS_KEY" -H 'Accept: application/json' \
    --data-binary "@$apk" \
    "$AS_URL/luma-gallery/upload-apk?version_code=$code&version_name=$version&notes=$notes"
curl -fsS -X POST -H "x-api-key: $AS_KEY" --data-binary @res/mipmap-xxxhdpi/ic_launcher.png "$AS_URL/luma-gallery/icon"
printf '\nLocal APK: '
sha256sum "$apk"
