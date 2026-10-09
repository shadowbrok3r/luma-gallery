# Source And Licenses

Luma Gallery source is GPL-3.0-or-later, except the standalone converter wrapper in
`native/dng/`, which is MIT. Dependencies retain their respective licenses.
The launcher and toolbar SVGs are original project assets.

| Component | Version | License | Source |
| --- | --- | --- | --- |
| FFmpeg | 8.0.1, GPL configuration | GPL-2.0-or-later | https://ffmpeg.org/releases/ffmpeg-8.0.1.tar.xz |
| x264 | b35605ace3ddf7c1a5d67a2eb553f034aef41d55 | GPL-2.0-or-later | https://code.videolan.org/videolan/x264 |
| LibRaw | 0.22.0 | LGPL-2.1 or CDDL-1.0 | https://www.libraw.org/data/LibRaw-0.22.0.tar.gz |
| Adobe DNG SDK | 1.7.1 build 2724, 2026-09-08 | Adobe DNG SDK License Agreement | https://download.adobe.com/pub/adobe/dng/dng_sdk_1_7_1_2724_20260908.zip |
| libjxl | 0.11.2, Adobe SDK bundled sources | BSD-3-Clause | https://github.com/libjxl/libjxl |
| Highway / Brotli / skcms | Adobe SDK bundled libjxl dependencies | Apache-2.0 or BSD-3-Clause / MIT / BSD-3-Clause | Included under `libjxl/libjxl/third_party/` |
| IJG libjpeg | 9c, Adobe SDK bundled sources | IJG license in README | https://www.ijg.org/files/jpegsrc.v9c.tar.gz |
| LibVLC Android | 3.7.7, 0b8dc65e | LGPL-2.1-or-later | https://code.videolan.org/videolan/libvlcjni |
| VLC core | 84177b22 plus LibVLC build patches | LGPL configuration | https://code.videolan.org/videolan/vlc |
| Android C++ runtime | NDK distribution | Apache-2.0 with LLVM exception | https://android.googlesource.com/platform/ndk |
| EguiMobile | 0.1.4, 8098d9d3 | MIT or Apache-2.0 | https://github.com/shadowbrok3r/EguiMobile |
| egui and egui_extras | 0.36 | MIT or Apache-2.0 | https://github.com/emilk/egui |

`Cargo.lock` records all Rust dependencies. `scripts/prepare-native.sh` records native versions,
checksums, configuration, and toolchain. FFmpeg, LibRaw, and DNG conversion execute in separate processes;
LibVLC and the shared C++ runtime are dynamically linked and unmodified.

Release APK assets include `source/luma-native-source.tar.xz`, containing the application,
native bridge, build scripts, and FFmpeg, x264, LibRaw, LibVLC JNI, VLC core, DNG SDK,
libjxl and libjpeg sources,
including their license texts. The LibVLC build scripts record the core revision, patches,
and contrib recipes. NDK and toolchain notices are included as well.
Rust source dependencies are pinned to public Cargo/git sources.
The unmodified LibVLC AAR is available from Maven Central at
https://repo.maven.apache.org/maven2/org/videolan/android/libvlc-all/3.7.7/ .

## DNG Converter

This product includes DNG technology under license by Adobe.
This software is based in part on the work of the Independent JPEG Group.

The Adobe SDK has its own license agreement, not the application's GPL or the
wrapper's MIT license. Its complete license is included at
`native/vendor/dng_sdk_1_7_1/LICENSE.txt`, with the archive's copyright notices intact.
The helper is a standalone `dng_decode input output.ppm full|preview` executable;
the gallery launches it as a child process and reads the resulting PPM file.
Neither the GPL gallery library nor LibRaw links to the DNG SDK.

`native/dng/no-xmp.patch` adds two missing `qDNGUseXMP` guards to SDK build 2724's
JXL metadata glue. XMP and metadata-writing code are excluded; DNG decoding,
linearization, demosaic, opcodes, gain tables, color rendering and orientation
use the SDK. This patch is reapplied and checked by `prepare-native.sh`.
The archive contains the exact source and license texts for all SDK dependencies;
Adobe's sample photos and the user's camera files are not included in the APK.
