# Verification

## Video Frames Release 0.1.11 (2026-10-07)

The old scrub worker invalidated every decode whenever another time request arrived,
including repeated requests for the same time. Slow decodes could therefore never
appear, and the popup retained the previous video's bitmap. Preview jobs now carry
an immutable media/session identity, coalesce pending times, display completed progress
with its own timestamp, and clear on release, media changes and backgrounding.

On Android 16, WebM's `OPTION_CLOSEST` returned an earlier keyframe: a request around
6.780 seconds showed 4.267 seconds. WebM/Matroska previews and full-resolution stills
now decode forward with FFmpeg. Older cached thumbnails are invalidated.

The camera icon beside video crop saves a PNG from the original video to Pictures/Luma,
with rotation applied and no UI or pending crop. Capture pauses playback, honors pending
frame steps, and leaves the source unchanged. Passive save notices allow taps through;
notices containing Undo remain interactive. FFprobe JSON is written separately from
diagnostics. Android metadata provides a fallback, and optional metadata failures no
longer produce playback error notices or arrive from an already-closed video.

All 30 host tests and 14 video-frame checks pass on the existing `s26ultra` AVD
(`emulator-5554`, Android 16, x86_64, 1440 x 3120, density 560). The device checks cover
moving/continuous previews, 20x precision labels, warm switches from H.264 to silent VP9 WebM, full-resolution
960 x 540 PNGs, single-frame stepping, capture while playing and at the end, portrait
rotation to 540 x 960, unchanged originals, and Android metadata fallback on playable
silent MPEG-TS that the bundled FFprobe does not demux. Saved frames are compared to
independent host decodes, allowing neighboring presentation frames for millisecond rounding.
The user file that originally produced the probe error was not available; the fallback
was exercised with a synthetic file instead. Physical S26/OEM/HDR behavior is not certified.

The release APK's final build (preview-request dedupe, pre-Android 11 capture call) passed
the same 14 checks again, plus the C0038 export regression: the fast cut starts at the
original 3.570 s keyframe with 124 byte-identical packets, and the exact cut keeps 4K/AAC.
The export script now reads the selected bounds from the framed In/Out fields.

Evidence: `evidence/video-frames-0.1.11/final/` and `export-final/`; package/store
verification: `evidence/release-0.1.11/`.

```sh
cargo test --lib
TESSDATA_PREFIX=.build/tessdata python3 scripts/smoke-video-frames.py --serial emulator-5554 --evidence evidence/video-frames-0.1.11/final
TESSDATA_PREFIX=.build/tessdata python3 scripts/smoke-export.py --serial emulator-5554 --original ~/Videos/Jewelry/C0038.MP4 --device-uri file:///sdcard/DCIM/Luma-Export/C0038.MP4
```

## Keyboard Focus Release 0.1.10 (2026-10-05)

Two independent focus-loss paths were reproduced:

- The Qwen prompt used a parent-derived widget ID. A tall keyboard could change
  the editor from stacked to columns, replacing the focused widget and closing the
  keyboard. The prompt now has an explicit per-session ID. Columns also require at
  least 600 points of width so a short portrait viewport keeps full-width controls.
- EguiMobile immediately interpreted a hidden IME inset as external dismissal.
  During rotation, Android reattached its input connection and requested a show;
  the bridge's immediate hide cancelled that show. The shared Java bridge now checks
  the settled root insets after 300 ms. A visible inset, a new show, an explicit hide,
  or Activity destruction cancels the pending check. Back still dismisses editing.

The real-widget host regression failed before the ID fix and passes afterward,
including typed input through keyboard resize and layout-parent changes. All 30
library tests and five toolbar host tests pass. On the existing `s26ultra` AVD
(`emulator-5554`, Android 16, Gboard), the 0.1.9 APK immediately closed the keyboard
in a 1440 x 2200 viewport. The fixed build passes seven focus checks in that viewport
and the original 1440 x 3120 viewport: opening/typing, three Back/reopen cycles at
each size, typing through both rotations without retapping, and final dismissal.
The seven existing clipboard/caret/Enter checks also pass. Display overrides and
rotation settings are restored by the regression script.
The separately rebuilt EguiMobile Android Hello demo passes four checks of the
shared bridge: multiline typing, uninterrupted typing through each rotation, and Back.

The Java fix is identical in Luma's vendored backend and the shared EguiMobile
checkout. Other apps need rebuilding to receive it. Samsung Keyboard was reported
by the user but is unavailable on this AVD; these results do not certify its OEM
behavior. Evidence: `evidence/keyboard-focus-0.1.10/`; release package and store
verification: `evidence/release-0.1.10/`.

```sh
cargo test --lib
cargo test --manifest-path scripts/text-actions-tests/Cargo.toml --target-dir target
TESSDATA_PREFIX=.build/tessdata python3 scripts/smoke-keyboard-focus.py --serial emulator-5554
TESSDATA_PREFIX=.build/tessdata python3 scripts/smoke-text-menu.py --serial emulator-5554
```

## Text Toolbar Release 0.1.9 (2026-10-05)

EguiMobile's Paste/Copy/Cut/Select-all toolbar previously occupied app layout space,
covering long or bottom-aligned text fields. The Android adapter now reserves a
measured strip before app layout, above the actual keyboard inset. The toolbar is
clipped to that strip, reflows with window/style changes, and returns the space when
dismissed. No guessed keyboard fraction or per-app vertical anchor is needed.
The same implementation is in the shared EguiMobile checkout and Luma's vendored
backend. The shared checkout also includes the earlier subclass-IME and Enter fixes.

Luma's editor permits smaller scroll viewports in landscape. Its Qwen prompt also
follows the caret after text reflow: pasting into an empty field can change egui's
rendered text height one frame after the text-change event. Ordinary scrolling
remains free because caret following only runs when text/viewport geometry changes.

- 29 Luma library tests and five host tests of the actual toolbar widget pass.
  Host coverage includes portrait, landscape, hardware keyboards, changing keyboard
  height, large controls, narrow-to-wide reflow, anchors, dismissal and button taps.
- ARTEMIS/ADB reproduced the overlap on the existing `s26ultra` AVD, Android 16,
  x86_64, 1440 x 3120. `scripts/smoke-text-menu.py` passes seven checks: ten-line
  prompt bounds, Select all/Copy/Paste, Cut/Paste into empty text, 25-line caret
  scrolling, Back dismissal, landscape caret visibility and single-line Enter.
  OCR and detected field/menu outlines verify that the text stays above the menu.
- The shared EguiMobile Android Hello demo exposes a **Text actions** screen for
  direct checks independent of Luma's layout. Five device checks passed there too:
  multiline caret visibility, Select all/Copy/Paste, Cut/Paste, landscape typing
  and single-line Enter dismissal.

Evidence: `evidence/text-menu-0.1.9/`; packaging, signing and store verification:
`evidence/release-0.1.9/`. Other apps need rebuilding against the shared changes.

```sh
cargo test --lib
cargo test --manifest-path scripts/text-actions-tests/Cargo.toml --target-dir target
TESSDATA_PREFIX=.build/tessdata python3 scripts/smoke-text-menu.py --serial emulator-5554
```

## Sharing Release 0.1.8 (2026-10-05)

Android advertises Luma for `VIEW`, `SEND` and `SEND_MULTIPLE` with `image/*` and
`video/*`. Both cold launches and `onNewIntent` resolve content URIs off the UI
thread, using the sender's read grant. Shared items have their own ordered viewer
collection; library scans and filters do not remove them. A new share asks before
closing the photo editor or a video trim/crop. Unsupported attachments are skipped,
unreadable shares keep the current view, and a slow earlier provider cannot replace
a newer share. The 0.1.7 keyboard backend is unchanged.

- All 29 library tests pass, including shared navigation outside library filters,
  keeping an open photo editor, and preserving a pending video trim.
- ARTEMIS screenshots/hierarchy and ADB established the real Android chooser,
  share sheet, mixed-media navigation and editor paths on the existing `s26ultra`
  AVD (`emulator-5554`, Android 16 / x86_64).
- `scripts/smoke-share.py`: 11 passing checks using a separate test app's unexported
  provider. Luma's three media-read permissions were disabled during the checks and
  restored afterward. Cold Open with, warm video sharing in the same Activity process,
  audio waveform generation, mixed multi-share ordering, ClipData-only streams and
  names without extensions all pass. Shell access to the provider is denied.
- A shared 900 x 600 photo crops to a pixel-exact 600 x 600 PNG copy. Receiving another
  share and choosing Keep editing preserves that crop; Open media accepts the next
  share. Invalid attachments, missing files and a deliberately slow provider also pass.

Evidence: `evidence/share-0.1.8/`; release package and store checks:
`evidence/release-0.1.8/`. The sender under `scripts/share-probe/` is test tooling,
not part of Luma's application classes.

```sh
cargo test --lib
TESSDATA_PREFIX=.build/tessdata python3 scripts/smoke-share.py --serial emulator-5554
```

## Keyboard Release 0.1.7 (2026-10-05)

The Android backend recognizes subclasses of `EguiNativeActivity`, enabling its IME
bridge in Luma's `GalleryActivity`. Enter in a single-line field also tears down the
keyboard session instead of restoring focus. The implemented backend sources from
EguiMobile revision `ad77e546` are retained unchanged in `vendor/egui-android`; its
manifest pins shared dependencies to the public `31d1cbeb` base. Builds no longer
depend on the implementation agent's temporary checkout.

- All 26 library tests pass.
- On the existing `s26ultra` AVD, the Qwen prompt opens the soft keyboard, accepts text,
  dismisses on Back, reopens on a second tap, and accepts deletion back to an empty prompt.
- Pen-only rejection, stylus painting, mask retention across tabs and Undo still pass.
- Entered 2 and 6 seconds in the video trim fields using the soft-keyboard editing
  session; Enter dismisses the keyboard. The exported square clip is 360 x 360,
  approximately four seconds long, with its first frame and audio pulses matching
  the selected source interval.

Five targeted device checks passed using `keyboard_check`, `mask_checks` and
`clip_check` from `scripts/smoke-editing.py`. Evidence: `evidence/keyboard-0.1.7/`;
release package and store checks: `evidence/release-0.1.7/`.

## Crop Release 0.1.6 (2026-10-05)

Photo/video crop gestures now retain their starting pointer position through release.
Previously, egui cleared `press_origin()` on release while still supplying the final
pointer position; the crop handler then used zero displacement and restored the original
rectangle. The shared handler now stores the origin with the starting crop.

- Both new egui gesture regressions failed before the fix and pass afterward. All 26
  library tests pass. Coverage includes all four corners, square-crop movement, clamping
  at the image boundary, release and idle frames, with both touch and mouse events.
- ARTEMIS screenshots/hierarchy and ADB established the paths on the existing `s26ultra`
  AVD (`emulator-5554`, Android 16 / x86_64, 1440 x 3120). The old photo and video selection
  visibly moved during contact and returned to its starting bounds on release.
- `scripts/smoke-crop.py`: 15 passing checks. Moved and resized bounds persist after touch
  release in photos, landscape MP4 and rotated MOV. A square moved against the photo's
  left edge exports a pixel-exact 600 x 600 source crop; shrinking it exports a pixel-exact
  302 x 301 crop. Switching photo-editor tabs retains the rectangle.
- Both 2–6 second video exports retain the smaller, moved crop: output frames match an
  independent decode/crop of the original, including display rotation. Audio remains
  present, duration stays within 160 ms of four seconds, and all seven original fixture
  hashes remain unchanged.

Evidence: `evidence/crop-fix-0.1.6/`; release package/store checks: `evidence/release-0.1.6/`.

```sh
cargo test --lib
TESSDATA_PREFIX=.build/tessdata python3 scripts/smoke-crop.py --serial emulator-5554
```

## Editing Release 0.1.5 (2026-10-05)

The editing checks below ran on the development 0.1.4 build. Release 0.1.5 changes
only the package version and this verification record from that tested build;
the higher version code allows updates from the published 0.1.4 library release.

Used the existing `s26ultra` AVD, `emulator-5554`, Android 16 / x86_64, 1440 x 3120 at
density 560. ARTEMIS screenshots/hierarchy and ADB established the interaction paths;
`scripts/smoke-editing.py` uses OCR with verified icon-coordinate fallbacks and bounded
waits for rendered media, server results and completed exports. Originals are synthetic.

- `cargo test --lib`: 24 passing tests, including crop rounding after serialization,
  rotation/mirroring, transient-preserving waveform pooling, alpha-mask RGB preservation,
  circular brush geometry, pressure/undo and palm/pen input gates.
- Photo exports: a 900 x 600 PNG crops to exactly 600 x 600 with identical source pixels.
  Rotate/mirror output is pixel-exact; setting Color to zero produces a grayscale copy.
  The photo editor also renders correctly in landscape. WebP and GIF first-frame PNG copies match decoded
  sources; EXIF orientation 6 exports a correctly oriented 600 x 900 JPEG-derived copy.
- S Pen event simulation: pen-only rejects finger strokes; stylus strokes paint; undo
  restores the canvas; switching tabs retains the mask. A 600 ms stationary contact at
  pressure 0.2 affects 1,053 pixels, versus 4,560 at pressure 1.0. Eraser-tip and barrel-button
  contacts erase, including release; undo restores the erased region. The runnable
  `scripts/StylusStroke.java` probe is compiled only for testing, outside the app.
- Live Qwen: authenticated against `https://comfy.shadowbroker.app`, uploaded a synthetic
  painted PNG, ran 25 steps, stopped waiting locally and resumed the same server job,
  downloaded/reviewed/saved a 900 x 600 result, then resumed it after process restart.
  One measured result changed only bounds `(606,113)..(891,477)`;
  every pixel outside that region was identical to the upload. The correct gateway is
  configured on the emulator; credentials are private app data and absent from the APK.
- Video: the 2.000–6.000 s square crop exports H.264 360 x 360 with AAC, container duration
  4.021333 s. Decoded pulse peaks occur at 0.02–0.12 s and 2.02–2.12 s, matching the source
  beats plus AAC padding. The waveform displays these transients under the same time axis
  as video. Silent VP9 WebM reports no audio. Rotation fixtures use a verified display
  matrix; `-metadata rotate=90` on the host's FFmpeg did not create one. Portrait playback
  fills the correct aspect after LibVLC reports its track dimensions. A rotated MOV crops
  to 360 x 360 with no remaining rotation matrix; the first exported frame matches an
  independent decode/crop at 2 seconds, and disabling audio removes the audio stream.

Evidence: `evidence/editing-smoke/` and `evidence/editor-0.1.4/`. Run:

```sh
TESSDATA_PREFIX=.build/tessdata python3 scripts/smoke-editing.py --serial emulator-5554 --stylus
# Optional real server request with credentials already configured through Luma's UI:
TESSDATA_PREFIX=.build/tessdata python3 scripts/smoke-editing.py --serial emulator-5554 --qwen
```

Physical S26 Ultra pressure accuracy, palm rejection, latency, thermals, and Samsung AI
editor availability remain unverified. The external-editor action hands off a copy;
it does not expose Samsung's native image-generation models inside Luma. Photo exports
are SDR and capped at 32 MP; Qwen inputs are capped at 1600 pixels on the longest side.
HDR editing, animation editing, independently moving audio, and background video-export
recovery are not implemented. Existing release checks below are historical, not new runs.

## Previous Release Coverage

Release: 0.1.4. Test date: 2026-10-03. Device: `emulator-5554`, Android 16, x86_64,
1440 x 3120 at density 560, booted headless and read-only (`-read-only -no-window`), so
test media and app changes were discarded on exit. The Galaxy S26 Ultra was not connected.
Emulator results establish functionality, not phone decoder throughput or thermal behavior.
Sections without a 0.1.4 note record the 0.1.3 results of 2026-10-02.

## Library Management (0.1.4)

Fixtures were pushed with ADB, so MediaStore lists `com.android.shell` as their owner and
every change to them passes through Android's consent dialogs, as for camera media.

- `scripts/smoke-manage.py`: seven checks, passed on four runs, the last on the final build.
  Long-press, drag-range and tap selection without scrolling; moving two photos to another
  album after consent, with the files moved on disk; batch rename to `Smoke_001.jpg` and
  `Smoke_002.jpg`; trash followed by Undo from the notice; copying into a new album, which
  needs no consent because Luma owns the copy; delete forever from the Trash view; and an
  album rename.
- ADB-driven checks during development: Trash restore and Empty; viewer delete advancing to
  the next photo (re-checked through the More menu on the final build); viewer rename with
  the title updating; restoring a trashed photo from the
  viewer; the share sheet for two images; favoriting and unfavoriting a selection; Settings
  opening Android's Media management page (left disabled).
- A linked `.nomedia` folder, granted through the folder picker: subfolder album rename, file
  rename, permanent deletion after Luma's confirmation, moving a file into a MediaStore album
  (copied, then the original deleted), and refusal to rename the linked folder itself.
- Not exercised: Android 10's per-item consent path, changes with Media management access
  granted (no dialogs), and linked folders from providers other than device storage.
- The legacy regressions re-passed on 0.1.4: all 17 `smoke-android.py` checks and all 12
  `smoke-navigation.py` checks on two consecutive runs. The first navigation run after a
  fresh install failed its photo-to-video media check once: the 4K video had not started
  1.5 seconds after opening while RAW caches were still developing. 0.1.3 passed the same
  suite on this emulator, and opening the video directly played it on both builds.

Evidence: `evidence/manage-0.1.4/`, `evidence/smoke-0.1.4/`, `evidence/navigation-0.1.4/`.
The management regression needs Tesseract's English data, e.g.
`TESSDATA_PREFIX=.build/tessdata python3 scripts/smoke-manage.py --serial emulator-5554`.

## DNG And Scrolling

The supplied `20260916_183018.dng` is a Galaxy S26 Ultra DNG 1.7: 4080 x 3060,
14-bit three-channel linear RAW, JPEG XL compression (52546), orientation 6.
SHA-256: `459f994ed028c7118db3a5b8e83895ab5d978787e0c59ee7fbdb407f95a416da`.
The old build displayed a 1600 x 2134 preview followed by RAW development failed.
The new DNG converter and Android viewer produce 3060 x 4080 after rotation.

- `scripts/verify-dng.py`: 21 passing native checks. Path and stdin output are identical;
  removing the embedded JPEG does not change full development. Corrupting the JPEG XL
  RAW stream fails even with a valid JPEG preview. Invalid/truncated inputs fail cleanly.
  All 14 official SDK sample files decode, covering integer/float JPEG XL, Bayer JPEG XL,
  uncompressed integer/float data, lossless JPEG, gain tables and profile metadata.
- `scripts/smoke-dng.py`: four passing emulator checks for full-size portrait rendering,
  1:1 tiled viewing/panning, landscape orientation, and reopening the developed cache.
  Device log confirms `DNG SDK 1.7.1 (2724): compression=52546, rendered=3060x4080, orientation=6`.
- The supplied DNG develops in approximately 1.1 seconds on this host. This is not a
  phone performance measurement. The original 14-bit file is never changed; the display
  cache is the same 8-bit SDR sRGB format used for ARWs.
- A headless egui regression samples both grids across virtual row boundaries in both
  directions, at portrait/landscape sizes and density 3.5. Restoring the old row height
  reproduces a 15.84375-point album jump at offset 213; the corrected geometry passes.
- A second regression verifies scrolling on the first touch without a prior tap.
  The old touch-autodetection default yields zero movement; explicit gallery drag
  scrolling passes and does not open a card while dragging.
- `scripts/smoke-scroll.py`: nine-second continuous emulator drags in both directions,
  11 captured frames each. Pixel tracking reports no reverse jumps (down: -38..0;
  up: 0..37 in the scaled test image). Temporary album fixtures were removed afterward.

Native fixtures come from the checksum-pinned Adobe SDK archive under
`.build/dng-fixtures/dng_sdk_1_7_1/sample_files/`. They and the user's DNG are excluded
from the release source archive. Device fixture: `DCIM/Luma-DNG-Tests`, one DNG.
Evidence: `evidence/dng-native/`, `evidence/dng-ui/`, `evidence/scroll-fixed/`.

```sh
cmake -S native/dng -B .build/dng-host -DCMAKE_BUILD_TYPE=Release
cmake --build .build/dng-host --target dng_decode -j 8
python3 scripts/verify-dng.py --sample 20260916_183018.dng --sdk-samples .build/dng-fixtures/dng_sdk_1_7_1/sample_files
python3 scripts/smoke-dng.py --serial emulator-5554
python3 scripts/smoke-scroll.py --serial emulator-5554
```

Scroll capture additionally requires numpy and at least seven album rows. Test fixtures
used 14 temporary `DCIM/Luma-Scroll-01` through `DCIM/Luma-Scroll-14` folders, each containing
a copy of `Jewelry-4K.jpg`, scanned into MediaStore. The script itself does not add/delete media.

## Fixtures

The emulator album `DCIM/Luma-Tests` contains these six files. Personal media is
not included in the source bundle or APK.

| File | Format | Full-resolution result |
| --- | --- | --- |
| `_DSC7596.ARW` | User-supplied Sony ZV-E10 RAW | 6024 x 4024, fit and 1:1 |
| `A7IV-uncompressed.ARW` | Sony ILCE-7M4, uncompressed | 7028 x 4688, fit and 1:1 |
| `A7IV-lossless.ARW` | Sony ILCE-7M4, lossless compressed | 7028 x 4688, fit and 1:1 |
| `Jewelry-4K.jpg` | Still extracted from the user's video | 3840 x 2160, portrait and landscape |
| `C0038.MP4` | 3840 x 2160, H.264 High 8-bit 4:2:0, 29.97 fps, PCM audio | GPU-surface playback, seek loupe, trims |
| `C0010.MP4` | 1920 x 1080, H.264, 59.94 fps, PCM audio | Playback fixture |

The two A7 IV samples are CC0 files from [raw.pixls.us](https://raw.pixls.us/):
records 6928 and 6929. SHA-256 values:

```text
_DSC7596.ARW        614fcf4df4f41aeba5944a7cbbf1571077e6a5452296cce044852f3efe2bf57e
A7IV-uncompressed  626e1f3235e28f2c642814e0aefad404ea767c06943934db87d2b0072ba8e8b9
A7IV-lossless      851b43c2116c4139104a5036f83ac3b6a148789b2142214dd7192c13972b25b6
```

## Executable Checks

`cargo test --lib`: 18 passing tests for fractional-rate frame stepping,
trim bounds, zoom bounds, timecode formatting, thumbnail invalidation, shared
grid/rail/navigation ordering, navigation boundaries, search, favorites, virtual row
geometry and first-touch scrolling. Added in 0.1.4: drag-range and select-all selection,
rename numbering and name validation, MediaStore album folder rules, size labels, change
summaries (favorites following renamed linked files, Undo limited to trashable items, the
viewer advancing past removed items), trash versus confirmed permanent deletion, and a
headless egui run that long-presses, drags and taps grid cells in both grids without
scrolling or opening an item.

`scripts/smoke-navigation.py`: 12 checks re-passed on 0.1.3, covering the compact header,
pixel-verified AMOLED black, enlarged centered pink selection, first/last item
boundaries, short/vertical drags, next/previous swipes, 1:1 panning without file
changes, thumbnail taps, free rail scrolling without selection or snap-back,
reversed ordering, landscape controls, photo-to-video transitions and RAW filtering.

`scripts/smoke-android.py` uses the interaction paths explored through ARTEMIS
and ADB. It uses OCR for labels, verified scaled coordinate fallbacks for the
custom-painted controls, explicit state waits, and screenshot pixel assertions.
It preserves app data and restores rotation settings. Requires Pillow,
Tesseract with English data, adb, the installed x86_64 APK and the fixture album.

The complete 17-check regression re-passed on 0.1.3 and 0.1.4: all three RAW fit/1:1 views, JPEG rotation,
moving 4K video, 1x/20x scrub loupe, portrait and landscape trim controls, 1080p60
playback and background pause. Playback proxy creation is covered by the export
regression; proxy cancellation was also exercised in 0.1.1.

`scripts/smoke-export.py` additionally builds a proxy and exports the original's
4.007 to 7.402 second range in both modes. The fast-cut regression checks that
the first video packet is the nearest preceding keyframe, not the earlier
interleave block returned by a coarse MP4 seek. Sony's reordered presentation
and decode timestamps are handled separately. Both exports retain 3840 x 2160
resolution despite playback using a 1920 x 1080 proxy.

```sh
python3 scripts/smoke-android.py --serial emulator-5554
python3 scripts/smoke-navigation.py --serial emulator-5554
python3 scripts/smoke-export.py --serial emulator-5554 --original ~/Videos/Jewelry/C0038.MP4
python3 scripts/verify-export.py ~/Videos/Jewelry/C0038.MP4 .build/fast-clip.mp4 --duration 3.117 --fast
python3 scripts/verify-export.py ~/Videos/Jewelry/C0038.MP4 .build/exact-clip.mp4 --duration 3.117
```

Current evidence screenshots are in `evidence/smoke-0.1.4/`, `evidence/navigation-0.1.4/` and
`evidence/manage-0.1.4/`;
export evidence is in `evidence/export-0.1.2/`, and build/test logs are in `.build/`.
They are deliberately excluded from Git and the APK because they contain personal media.

## Export Measurements

Exports were created through the Android UI and pulled from `Movies/Luma`.
Original files were left unchanged. The zero-start rows are the 0.1.1 baseline;
the nonzero, proxy-backed regression is repeated for 0.1.2.

| Mode | Selected range | Container duration | Resolution/audio | Video integrity |
| --- | --- | --- | --- | --- |
| Fast | 0 to 3.117 s | 3.224467 s | 3840 x 2160, AAC 48 kHz stereo | All 95 packets byte-identical and contiguous in the original |
| Exact | 0 to 3.117 s | 3.157467 s | 3840 x 2160, AAC 48 kHz stereo | Re-encoded H.264 |
| Fast, from proxy | 4.007 to 7.402 s | 4.058267 s | 3840 x 2160, AAC 48 kHz stereo | 118 unchanged packets; first keyframe PTS 3.570233 s |
| Exact, from proxy | 4.007 to 7.402 s | 3.424033 s | 3840 x 2160, AAC 48 kHz stereo | Re-encoded H.264 |

The measured duration difference includes codec reorder/audio padding. Fast mode
also snaps the start to a preceding keyframe. Neither mode promises arbitrary
millisecond-exact MP4 container duration.

ComfyUI Android 0.23.2's actual `src/player.rs` passed its isolated GPU harness:
moving rendered pixels, playback-clock advancement, paused seek, resume, loop,
and audio-track detection. Its server-backed flow could not be exercised because
the emulator connection received certificate errors. TLS verification was not disabled.

## Release Boundaries

- No S26 Ultra performance, battery, temperature, or sustained playback benchmark yet.
- These video fixtures are 8-bit 4:2:0. Sony 10-bit 4:2:2 and HEVC profiles still
  require physical-device validation; software fallback or a proxy may be needed.
- SDR compositor only; no HDR passthrough or tone mapping.
- RAW display uses full-size 8-bit sRGB JPEG caches, not a lossless RAW editing pipeline.
- Audio routing is implemented and export audio is validated; Bluetooth latency
  and subjective audio/video synchronization are not measured on the emulator.
- Exports are cancellable in-process, but do not survive Android killing the app.
- ARM64 native libraries were checked for 16 KB load-segment alignment. No 16 KB
  page-size device was available for an installation test.
