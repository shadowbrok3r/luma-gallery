#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
root="$PWD"
sdk="${ANDROID_HOME:-$HOME/Android/Sdk}"
ndk="${ANDROID_NDK_HOME:-$sdk/ndk/28.0.12674087}"
toolchain="$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin"
jobs="${JOBS:-8}"
mkdir -p .build/downloads native/vendor native/libs
fetch() {
    local url="$1" file="$2" sha="$3"
    if [ ! -f "$file" ]; then curl --fail --location --retry 3 "$url" -o "$file"; fi
    printf '%s  %s\n' "$sha" "$file" | sha256sum -c -
}
fetch https://ffmpeg.org/releases/ffmpeg-8.0.1.tar.xz .build/downloads/ffmpeg.tar.xz 05ee0b03119b45c0bdb4df654b96802e909e0a752f72e4fe3794f487229e5a41
fetch https://www.libraw.org/data/LibRaw-0.22.0.tar.gz .build/downloads/libraw.tar.gz 1071e6e8011593c366ffdadc3d3513f57c90202d526e133174945ec1dd53f2a1
fetch https://download.adobe.com/pub/adobe/dng/dng_sdk_1_7_1_2724_20260908.zip .build/downloads/dng-sdk.zip 740fbe95c69e09e9cd17654a5e4fef2d7021254b06fd2b8c5557b79a1496b50c
fetch https://repo.maven.apache.org/maven2/org/videolan/android/libvlc-all/3.7.7/libvlc-all-3.7.7.aar .build/downloads/libvlc.aar b48dab96e0e90e34cce3861963500c144a5aadb1b249d501d6a4b104d849e61b
fetch https://code.videolan.org/videolan/libvlcjni/-/archive/0b8dc65efb203c86a0476bc337ddd99ecf1c0ef6/libvlcjni-0b8dc65efb203c86a0476bc337ddd99ecf1c0ef6.tar.gz .build/downloads/libvlcjni-source.tar.gz bafba3f63830d7a049d753f8ffe6fecdcf0abcfed8e449355e87205da18854ec
fetch https://code.videolan.org/videolan/vlc/-/archive/84177b2273abc5c4300234e6e18b55a4d2dd5a03/vlc-84177b2273abc5c4300234e6e18b55a4d2dd5a03.tar.gz .build/downloads/vlc-source.tar.gz aeb337f3487011d5f826b360b47c32c28fa3dbca737ac638b91f8ce31147ad20
if [ ! -d native/vendor/libvlcjni-0b8dc65efb203c86a0476bc337ddd99ecf1c0ef6 ]; then tar -xf .build/downloads/libvlcjni-source.tar.gz -C native/vendor; fi
if [ ! -d native/vendor/vlc-84177b2273abc5c4300234e6e18b55a4d2dd5a03 ]; then tar -xf .build/downloads/vlc-source.tar.gz -C native/vendor; fi
cp "$ndk/NOTICE" native/vendor/ANDROID-NDK-NOTICE
cp "$ndk/NOTICE.toolchain" native/vendor/ANDROID-NDK-TOOLCHAIN-NOTICE
if [ ! -d native/vendor/ffmpeg-8.0.1 ]; then tar -xf .build/downloads/ffmpeg.tar.xz -C native/vendor; fi
if [ ! -d native/vendor/LibRaw-0.22.0 ]; then tar -xf .build/downloads/libraw.tar.gz -C native/vendor; fi
if [ ! -d native/vendor/dng_sdk_1_7_1 ]; then
    unzip -q .build/downloads/dng-sdk.zip 'dng_sdk_1_7_1/dng_sdk/source/*' \
        'dng_sdk_1_7_1/libjxl/*' 'dng_sdk_1_7_1/libjpeg/*' \
        'dng_sdk_1_7_1/LICENSE.txt' 'dng_sdk_1_7_1/JXL_ReadMe.txt' \
        'dng_sdk_1_7_1/JPEG_ReadMe.txt' -d native/vendor
fi
# SDK 2724 omits two qDNGUseXMP guards in its JXL metadata glue.
if patch --batch --forward --dry-run -p1 -d native/vendor/dng_sdk_1_7_1 < native/dng/no-xmp.patch >/dev/null; then
    patch --batch --forward -p1 -d native/vendor/dng_sdk_1_7_1 < native/dng/no-xmp.patch
else
    patch --batch --dry-run -R -p1 -d native/vendor/dng_sdk_1_7_1 < native/dng/no-xmp.patch >/dev/null
fi
if [ ! -d native/vendor/x264 ]; then
    git clone https://code.videolan.org/videolan/x264.git native/vendor/x264
fi
git -C native/vendor/x264 checkout --detach b35605ace3ddf7c1a5d67a2eb553f034aef41d55
unzip -oq .build/downloads/libvlc.aar 'jni/*' classes.jar -d .build/vlc

for abi in "${@:-arm64-v8a x86_64}"; do
    # The default list is split below; explicit ABI arguments stay individual.
    for archabi in $abi; do
        case "$archabi" in
            arm64-v8a) triple=aarch64-linux-android; arch=aarch64 ;;
            x86_64) triple=x86_64-linux-android; arch=x86_64 ;;
            *) echo "Unknown ABI: $archabi" >&2; exit 2 ;;
        esac
        prefix="$root/.build/$archabi/prefix"
        mkdir -p "$prefix" "native/libs/$archabi" ".build/$archabi/x264" ".build/$archabi/ffmpeg" ".build/$archabi/raw"
        cp .build/vlc/jni/"$archabi"/*.so "native/libs/$archabi/"
        export CC="$toolchain/${triple}29-clang"
        export CXX="$toolchain/${triple}29-clang++"
        export AR="$toolchain/llvm-ar"
        export RANLIB="$toolchain/llvm-ranlib"
        export STRIP="$toolchain/llvm-strip"
        export PATH="$toolchain:$PATH"
        if [ ! -f "$prefix/lib/libx264.a" ]; then
            (
                cd ".build/$archabi/x264"
                "$root/native/vendor/x264/configure" --host="$triple" --prefix="$prefix" --enable-static --enable-pic --disable-cli --disable-opencl --extra-cflags='-O3' --extra-ldflags='-Wl,-z,max-page-size=16384'
                make -j"$jobs"
                make install-lib-static
            )
        fi
        if [ ! -f ".build/$archabi/ffmpeg-editor-v1-ready" ]; then
            (
                cd ".build/$archabi/ffmpeg"
                export PKG_CONFIG_LIBDIR="$prefix/lib/pkgconfig"
                "$root/native/vendor/ffmpeg-8.0.1/configure" --prefix="$prefix" \
                    --target-os=android --arch="$arch" --enable-cross-compile \
                    --cc="$CC" --cxx="$CXX" --ar="$AR" --ranlib="$RANLIB" --strip="$STRIP" \
                    --pkg-config=pkg-config --enable-gpl --enable-libx264 --enable-zlib --enable-pic \
                    --disable-shared --enable-static --disable-debug --disable-doc --disable-network \
                    --disable-autodetect --disable-everything --enable-ffmpeg --enable-ffprobe \
                    --enable-avcodec --enable-avformat --enable-avfilter --enable-swscale --enable-swresample \
                    --enable-protocol=file,pipe,fd --enable-demuxer=mov,matroska,avi,image2,image_jpeg_pipe,image_png_pipe,image_ppm_pipe,wav \
                    --enable-muxer=mp4,mov,image2,null \
                    --enable-decoder=h264,hevc,aac,pcm_s16be,pcm_s16le,pcm_s24be,pcm_s24le,pcm_s32le,mjpeg,png,ppm,rawvideo,flac,mp3,opus,vorbis,vp8,vp9,av1 \
                    --enable-encoder=libx264,aac,mjpeg,png,pcm_s16le --enable-parser=h264,hevc,aac,mjpeg,vp8,vp9,av1 \
                    --enable-bsf=aac_adtstoasc,h264_mp4toannexb,hevc_mp4toannexb,extract_extradata \
                    --enable-filter=scale,format,fps,trim,atrim,setpts,asetpts,aresample,aformat,null,anull,transpose,rotate,crop,volume,astats,ametadata,asetnsamples \
                    --extra-cflags="-I$prefix/include -O3" \
                    --extra-ldflags="-L$prefix/lib -pie -Wl,-z,max-page-size=16384"
                make -j"$jobs"
                cp ffmpeg "$root/native/libs/$archabi/libffmpeg_exec.so"
                cp ffprobe "$root/native/libs/$archabi/libffprobe_exec.so"
                touch "$root/.build/$archabi/ffmpeg-editor-v1-ready"
            )
        fi
        if [ ! -f "$prefix/lib/libraw.a" ]; then
            (
                cd ".build/$archabi/raw"
                "$root/native/vendor/LibRaw-0.22.0/configure" --host="$triple" --prefix="$prefix" \
                    --disable-shared --enable-static --disable-examples --disable-openmp \
                    --disable-jpeg --disable-jasper --disable-lcms --disable-lcms2 --disable-rawspeed \
                    CXXFLAGS='-O3 -fPIC' CFLAGS='-O3 -fPIC' LDFLAGS='-Wl,-z,max-page-size=16384'
                make -j"$jobs"
                make install
            )
        fi
        "$CXX" -O3 -fPIE -pie -Wl,-z,max-page-size=16384 -I"$prefix/include" \
            native/raw_decode.cpp "$prefix/lib/libraw.a" -static-libstdc++ -lm -lz \
            -o "native/libs/$archabi/libraw_exec.so"
        cmake -S native/dng -B ".build/$archabi/dng" -G Ninja \
            -DCMAKE_BUILD_TYPE=Release -DCMAKE_TOOLCHAIN_FILE="$ndk/build/cmake/android.toolchain.cmake" \
            -DANDROID_ABI="$archabi" -DANDROID_PLATFORM=android-29 -DANDROID_STL=c++_static
        cmake --build ".build/$archabi/dng" --target dng_decode -j "$jobs"
        cp ".build/$archabi/dng/dng_decode" "native/libs/$archabi/libdng_exec.so"
        "$CXX" -O2 -shared -fPIC -Wl,-z,max-page-size=16384 native/process.cpp -static-libstdc++ \
            -o "native/libs/$archabi/libluma_process.so"
        "$STRIP" native/libs/"$archabi"/*.so
    done
done
