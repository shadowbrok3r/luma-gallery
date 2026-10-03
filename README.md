# Luma Gallery

Android 10+ gallery built with EguiMobile 0.1.4. Package: `app.luma.gallery`.

- Stacked album covers, photo/video/RAW filters, search and persistent favorites.
- Multi-select photos, videos or whole albums: long-press (or the Select button), then tap,
  drag across the grid to pick a range, or select all.
- Manage selections: share, favorite, move or copy to an album (or a new one), rename
  (several files are numbered `Name_001`), and delete. Albums can be renamed, merged into
  another album or deleted. The viewer's menu offers the same actions for the open item.
- Deletes go to Android's trash on Android 11+, with Undo and a Trash view to restore or
  delete forever; Android purges trashed items after 30 days. Linked-folder items and
  Android 10 deletions are permanent and confirmed first.
- Compact AMOLED-black interface with ComfyUI's hot-pink/aqua palette and tight selection glows.
- Swipe between photos at fit zoom; zoomed drags pan. A scrollable thumbnail rail enlarges and
  centers the selected card, following the library's album, filters and date order.
- LibVLC demuxing handles Sony MP4 with PCM audio. Android hardware decoding feeds a GPU
  SurfaceTexture directly; no full-frame readback or egui pixel uploads during playback.
- Scrub-preview loupe, 5x/20x fine seeking by lifting the drag, timeline zoom, frame steps,
  playback speed, mute, in/out marks and selection looping.
- Fast trims copy the video from the preceding keyframe and encode audio to AAC. Exact trims
  re-encode with x264 CRF 18. Both create new files under `Movies/Luma`; originals are untouched.
- LibRaw develops ARWs; a separate Adobe DNG SDK 1.7.1 converter develops DNGs, including
  Galaxy S26 JPEG XL linear RAW, at native dimensions with DNG color/opcode processing and orientation.
  An 8-bit sRGB, high-quality 4:4:4 JPEG display cache
  feeds a bounded tile viewer with pinch/pan and 1:1 zoom. This is a viewer, not a RAW editor.
- Optional 1080p playback proxies. Clip export continues to use the original recording.
- MediaStore permissions and persistently authorized document folders; no network permission.
  Android confirms each batch of changes to media Luma did not create. Settings > Skip
  confirmations opens Android's Media management access (Android 12+), which removes them.

## Build

Requires Rust, cargo-egui-mobile/cargo-apk2, Android SDK/NDK r28, Java 17, curl, unzip, make,
pkg-config, NASM, CMake, Ninja, patch, jq and librsvg's `rsvg-convert`.
Native source versions and checksums are pinned; see `THIRD_PARTY.md` for the SDK's separate license.

```sh
bash scripts/prepare-native.sh
bash scripts/build-android.sh --emulator  # x86_64 emulator
bash scripts/package-sources.sh          # include corresponding native sources in releases
bash scripts/build-android.sh             # ARM64 phone release
cargo test --lib
```

UI regression: `python3 scripts/smoke-android.py --serial emulator-5554`.
Photo navigation/theme regression: `python3 scripts/smoke-navigation.py --serial emulator-5554`.
DNG viewer regression: `python3 scripts/smoke-dng.py --serial emulator-5554`.
Continuous album scroll regression: `python3 scripts/smoke-scroll.py --serial emulator-5554`.
Library management regression: `python3 scripts/smoke-manage.py --serial emulator-5554`
(creates, changes and removes its own `Luma-Manage-*` folders).
The fixture layout and OCR prerequisites are listed in `VERIFICATION.md`.
Export regression: `python3 scripts/smoke-export.py --serial emulator-5554 --original ~/Videos/Jewelry/C0038.MP4`.
This creates two new clips and verifies that nonzero trims from proxy playback retain the original
4K source. Fast mode also checks the exact first keyframe and every copied video packet.
Publish the ARM64 APK with `bash scripts/publish-appstore.sh` using `AS_URL` and `AS_KEY`.

APK: `target/release/apk/luma_gallery.apk`. Builds currently use the existing personal Android
debug signing key, consistently with the other apps in this private app store.

The reusable GPU renderer and platform SurfacePlayer live in EguiMobile's `video` module.
ComfyUI Android 0.23.2 also uses SurfacePlayer instead of the previous CPU frame-upload player.

## Limits

MediaStore keeps photos inside DCIM or Pictures and videos inside DCIM, Movies or Pictures,
so other folders are not offered as move targets. Moves keep each file on its storage volume;
an emptied source folder remains on disk but no longer appears as an album. Moving
linked-folder items into an album copies them, then deletes the originals. Folders linked
before 0.1.4 were granted read access only; add them again to delete or rename their files.

Hardware decode depends on the recording's codec, bit depth and chroma profile. Unsupported
profiles can fall back to software or use an explicitly requested playback proxy. The NPU is
not used for conventional video decoding. This build composites SDR; HDR passthrough and
HDR-to-SDR tone mapping are not implemented. Frame stepping uses the reported average rate;
variable-frame-rate footage is not guaranteed frame-index exact. Fast-cut boundaries can
include extra codec reorder frames; use Exact cut for tighter boundaries.

Full RAW development and exact export are CPU jobs and can take time. Keep the app open while
exporting; background job recovery is not implemented. The displayed RAW JPEG is not lossless,
although its pixel dimensions are retained. Cached media is disposable; originals stay intact.
DNGs render to SDR sRGB; XMP editing instructions and sidecar edits are not applied. The
DNG helper supports files up to 512 MiB via Android's descriptor pipe and images up to 200 MP.

See `VERIFICATION.md` for measured device coverage, and `THIRD_PARTY.md` for source notices.
