# Verification

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
