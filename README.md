# Luma Gallery

Android 10+ gallery built with EguiMobile 0.1.4. Package: `app.luma.gallery`.

- Stacked album covers, photo/video/RAW filters, search and persistent favorites.
- Open photos and videos from Android's **Open with** menu or choose **Luma Gallery**
  in the share sheet. Multiple shared items stay in the sender's order for navigation.
  The sender's file grant is enough; full library access is optional. Shared photos use
  the same crop, color and Qwen tools, and shared videos use the same clip editor.
  A new share asks before closing an open editor; edits still save as new copies.
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
  playback speed, mute, in/out marks and selection looping. Speed, loop and sound share a row
  with the resolution and frame rate; play, frame steps and previous/next sit on the bottom row.
  Settings > Playback controls puts both on the right (default), left or center.
- Fast trims copy the video from the preceding keyframe and encode audio to AAC. Exact trims
  re-encode with x264 CRF 18. Both create new files under `Movies/Luma`; originals are untouched.
- Photo editor: draggable crop, aspect presets, rotate, mirror, exposure, contrast and color.
  Save a JPEG or PNG copy under `Pictures/Luma`. Supports regular Android-decodable photos
  (including JPEG, PNG, WebP, HEIF/AVIF and GIF first frames) and developed RAW previews.
- Video editing: numeric in/out seconds, draggable trim handles, crop presets and a separate
  audio waveform aligned with the frame strip, playhead and timeline zoom. Crop uses Exact
  cut; exports can retain or omit audio. These tools also work on non-Sony recordings.
- **Edit video** (pencil in the viewer, or the More menu) opens a multi-layer editor. Playback
  (start, frame back, play, frame forward, end) sits under the timeline on the Settings side, and
  tools are grouped under Video, Text, Overlay and Canvas tabs; selecting a clip opens its tab.
  The video track splits at the playhead; deleting a part ripples later captions with it.
  Segments fade in and out through black or white, and a cut can dip to black. Playback skips
  removed parts.
- **Speed** plays a segment from 0.25x to 4x with the sound's pitch kept, in the preview and the
  export; captions and overlays over the segment stretch with it and later ones move.
- **Adjust** grades a segment with exposure, brightness, contrast, highlights, shadows, whites,
  blacks, temperature, tint, vibrance and saturation. White balance and exposure scale linear
  light and tone works in stops around middle grey, as RAW developers do. The preview applies
  the same 33-point color table on the GPU that the export applies with FFmpeg's lut3d at 16 bits.
- **Add video** (or the + tile after the last clip) adds another library video to the track, at the
  cut nearest the playhead or at the end. Other sizes and rotations are letterboxed into the
  first video's frame; each clip keeps its own audio, and silent clips play silence.
- **Captions** transcribes the timeline's audio on the phone with whisper.cpp (base model; 57 MB
  downloaded once from Hugging Face and checked by SHA-256; audio never leaves the device) and
  fills a "Captions" layer: phrases of up to N words, one word at a time, or phrases with the
  spoken word in a highlight color. Pick the language or let whisper detect it. Position, size,
  look and highlight color apply to every caption; dragging or restyling one caption moves or
  restyles the whole linked layer, and tapping one edits its words. "Update captions" rebuilds
  the layer in another mode without transcribing again. Debug adds a sample transcript.
- Captions sit on text layers and photos on overlay layers, drawn as translucent clip boxes
  in each layer's color; colored bands on the video track show where layers overlap it. Drag a
  caption on the video (it snaps to the center lines), resize/rotate with its corner handle or
  two fingers, and trim or move clips with timeline handles that snap to the playhead and edges.
  Layers can be hidden (also left out of exports), locked, renamed, recolored and reordered.
- Text styles: bundled TikTok Sans and device fonts (Roboto, Noto Serif, Source Sans Pro, Dancing Script, ...), size,
  bold/italic/underline/caps, alignment, colors, outline, rounded box with opacity, drop shadow
  or glow, spacing and wrap width, plus presets (Classic, Subtitle, Glass, Yellow, Neon, ...).
  Captions and overlays fade or slide in and out. Canvas crop, audio volume/fades, undo/redo and
  per-video autosave are included; Debug shows mapping, render timing, JSON and the FFmpeg command.
- Exports render every caption with the same rasterizer as the preview into one frame track per
  layer (animations are drawn per output frame), then FFmpeg composes cuts, fades, crop and the
  tracks into a new H.264/AAC file under `Movies/Luma`.
- The camera button beside video crop saves the selected frame as a full-resolution PNG
  under `Pictures/Luma`. It pauses playback and uses the original video, including rotation,
  without the app controls or pending clip crop. Scrub previews follow the current video;
  WebM/Matroska frames decode forward instead of snapping to an old keyframe.
- Qwen Image Edit: paint the region, describe the change, review Before/Result, then save a
  copy. S Pen input supports pressure, eraser/barrel-button erase, hover and pen-only painting;
  pinch zoom and two-finger pan follow ComfyUI Android's canvas conventions. Undo restores
  a complete stroke. Quick draft is optional; normal edits use 25 steps.
- Qwen server settings accept a comfy-gate API key or account login, stored in private app
  preferences. The default URL is `https://comfy.shadowbroker.app`; credentials are never
  bundled in the APK. A pending job/result can be resumed after reopening its original photo.
  Stop waiting detaches locally and does not interrupt other users' server jobs.
- The photo menu's **Open copy in another editor** hands a rendered copy to an installed
  Android editor. Samsung/Galaxy AI features depend on the editor offered by the device;
  Luma does not embed Samsung's private image-generation models.
- LibRaw develops ARWs; a separate Adobe DNG SDK 1.7.1 converter develops DNGs, including
  Galaxy S26 JPEG XL linear RAW, at native dimensions with DNG color/opcode processing and orientation.
  An 8-bit sRGB, high-quality 4:4:4 JPEG display cache
  feeds a bounded tile viewer with pinch/pan and 1:1 zoom. Photo adjustments operate on this
  rendered image, not the sensor data.
- Optional 1080p playback proxies. Clip export continues to use the original recording.
- MediaStore permissions and persistently authorized document folders. Network permission
  is used for user-triggered Qwen edits and server checks; ordinary gallery/editing stays local.
  Android confirms each batch of changes to media Luma did not create. Settings > Skip
  confirmations opens Android's Media management access (Android 12+), which removes them.

## Build

Requires Rust, cargo-egui-mobile/cargo-apk2, Android SDK/NDK r28, Java 17, curl, unzip, make,
pkg-config, NASM, CMake, Ninja, patch, jq and librsvg's `rsvg-convert`.
Native source versions and checksums are pinned; see `THIRD_PARTY.md` for the SDK's separate license.
The Android backend is EguiMobile's `egui-android` at the revision `Cargo.toml` pins, which
carries the subclass keyboard bridge, Enter dismissal and the composing-word fix.
The toolbar has its own space above the keyboard, leaving prompt text unobstructed.

```sh
bash scripts/prepare-native.sh
bash scripts/build-android.sh --emulator  # x86_64 emulator
bash scripts/package-sources.sh          # include corresponding native sources in releases
bash scripts/build-android.sh             # ARM64 phone release
cargo test --lib
```

UI regression: `python3 scripts/smoke-android.py --serial emulator-5554`.
The scripts set `debug.luma.automation` to 1, so the app logs its icon controls, timeline lanes
and video frame with pixel bounds on `LumaWidgets` logcat lines; scripts tap those by label and
fall back to measured coordinates.
Photo navigation/theme regression: `python3 scripts/smoke-navigation.py --serial emulator-5554`.
DNG viewer regression: `python3 scripts/smoke-dng.py --serial emulator-5554`.
Continuous album scroll regression: `python3 scripts/smoke-scroll.py --serial emulator-5554`.
Library management regression: `python3 scripts/smoke-manage.py --serial emulator-5554`
(creates, changes and removes its own `Luma-Manage-*` folders).
Editor regression: `TESSDATA_PREFIX=.build/tessdata python3 scripts/smoke-editing.py --serial emulator-5554`.
It creates seven synthetic fixtures and exports copies; add `--qwen` to test one real edit
using the credentials configured in the app. It checks pixel-exact crop/rotation, EXIF and
WebP/GIF exports, pen-only input/undo, silent WebM, clip timing/cropping and audio alignment.
Add `--stylus` on the 1440 x 3120 s26ultra AVD to compile `StylusStroke.java` with the Android
SDK and inject held pressure, eraser-tip and barrel-button contacts through `app_process`.
This probe is test tooling only and is not included in the application's classes.
The fixture layout and OCR prerequisites are listed in `VERIFICATION.md`.
Crop gesture regression: `TESSDATA_PREFIX=.build/tessdata python3 scripts/smoke-crop.py --serial emulator-5554`.
Checks that moving/resizing survives release and that photo/video copies use the selected bounds.
Video editor regression: `TESSDATA_PREFIX=.build/tessdata python3 scripts/smoke-studio.py --serial emulator-5554`.
Adds a caption, splits twice, deletes the middle part and exports; checks duration, audio, the
burned-in caption (OCR) and the cut against the source.
Multi-video regression: `TESSDATA_PREFIX=.build/tessdata python3 scripts/smoke-multivideo.py --serial emulator-5554`.
Adds a rotated portrait clip before and a silent clip after a video, plays across the cut,
reopens the project and checks the export's order, letterboxing and per-clip audio.
Playback regression: `TESSDATA_PREFIX=.build/tessdata python3 scripts/smoke-playback.py --serial emulator-5554`.
Scrubs and plays after the end, restarts from the end, scrubs while playing and loops.
Caption regression: `TESSDATA_PREFIX=.build/tessdata python3 scripts/smoke-captions.py --serial emulator-5554`.
Transcribes the JFK sample in the editor (downloading the model on first run), adds one-word
captions, drags them to the top and checks by OCR that the export shows each word on time.
Video frame regression: `TESSDATA_PREFIX=.build/tessdata python3 scripts/smoke-video-frames.py --serial emulator-5554 --evidence evidence/video-frames`.
Checks moving previews across warm video switches, silent WebM, full-resolution PNG frames,
rotation, Android metadata fallback and unchanged originals against generated video fixtures.
Android share/Open with regression: `TESSDATA_PREFIX=.build/tessdata python3 scripts/smoke-share.py --serial emulator-5554`.
This builds a separate sender with private synthetic files, temporarily revokes Luma's
library permissions, and restores its original grants afterward.
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

The video editor joins videos on one track; it does not place video picture-in-picture. The
preview switches files at a cut between videos with a short pause, and volume and audio fades
are heard only in the export. Speech recognition uses whisper's base model: word times come
from its token timestamps and are usually within a tenth of a second; there is no speaker
separation, and editing the timeline after transcribing needs a new transcription. Slowed
segments repeat frames; there is no frame interpolation. Adjust works on the decoded SDR
picture; HDR, log and RAW video are not developed from their wider range yet. Color emoji render from bitmap (CBDT/sbix) emoji fonts;
COLRv1-only emoji fonts, such as the Android 16 emulator's, fall back to monochrome Noto Emoji.
Editor exports re-encode with x264 and use the cropped source size or a 1080p/720p limit.

Full RAW development and exact export are CPU jobs and can take time. Keep the app open while
exporting; background job recovery is not implemented. The displayed RAW JPEG is not lossless,
although its pixel dimensions are retained. Cached media is disposable; originals stay intact.
DNGs render to SDR sRGB; XMP editing instructions and sidecar edits are not applied. The
DNG helper supports files up to 512 MiB via Android's descriptor pipe and images up to 200 MP.

Photo copies use SDR sRGB and up to 32 MP; the editor displays their export dimensions.
Qwen edits use the displayed working image (up to 1600 pixels on its longest side), upload
that image and its painted mask to the configured server, and save at that resolution.
Photo exports do not preserve original EXIF/GPS or animation. GIF editing saves a still frame.
The waveform analyzes the first audio stream with bounded time bins (20 ms for short clips);
it is an aid to cutting, not a separate audio mixer. Media codec support is bounded by Android,
LibVLC and the bundled FFmpeg build. Native Samsung AI and physical S Pen pressure/latency
require validation on the actual phone; emulator checks do not establish those capabilities.

See `VERIFICATION.md` for measured device coverage, and `THIRD_PARTY.md` for source notices.
