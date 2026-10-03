cmake_minimum_required(VERSION 3.16)
project(luma_dng LANGUAGES C CXX)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)
set(SDK "${CMAKE_CURRENT_SOURCE_DIR}/../vendor/dng_sdk_1_7_1")

set(BUILD_SHARED_LIBS OFF CACHE BOOL "" FORCE)
set(BUILD_TESTING OFF CACHE BOOL "" FORCE)
foreach(option TOOLS DEVTOOLS FUZZERS BENCHMARK EXAMPLES JNI JPEGLI JPEGLI_LIBJPEG
        DOXYGEN MANPAGES SJPEG OPENEXR TCMALLOC PLUGINS VIEWERS)
    set(JPEGXL_ENABLE_${option} OFF CACHE BOOL "" FORCE)
endforeach()
set(JPEGXL_ENABLE_SKCMS ON CACHE BOOL "" FORCE)
add_subdirectory("${SDK}/libjxl/libjxl" jxl EXCLUDE_FROM_ALL)

set(JPEG_SOURCES jaricom.c jcapimin.c jcapistd.c jcarith.c jccoefct.c jccolor.c
    jcdctmgr.c jchuff.c jcinit.c jcmainct.c jcmarker.c jcmaster.c jcomapi.c
    jcparam.c jcprepct.c jcsample.c jctrans.c jdapimin.c jdapistd.c jdarith.c
    jdatadst.c jdatasrc.c jdcoefct.c jdcolor.c jddctmgr.c jdhuff.c jdinput.c
    jdmainct.c jdmarker.c jdmaster.c jdmerge.c jdpostct.c jdsample.c jdtrans.c
    jerror.c jfdctflt.c jfdctfst.c jfdctint.c jidctflt.c jidctfst.c jidctint.c
    jquant1.c jquant2.c jutils.c jmemmgr.c jmemnobs.c)
list(TRANSFORM JPEG_SOURCES PREPEND "${SDK}/libjpeg/")
add_library(dng_jpeg STATIC ${JPEG_SOURCES})
target_include_directories(dng_jpeg PUBLIC "${SDK}/libjpeg")

file(GLOB DNG_SOURCES CONFIGURE_DEPENDS "${SDK}/dng_sdk/source/*.cpp")
list(FILTER DNG_SOURCES EXCLUDE REGEX "/dng_(validate|xmp|xmp_sdk|update_meta)\\.cpp$")
add_library(dng_sdk STATIC ${DNG_SOURCES})
target_include_directories(dng_sdk PUBLIC "${SDK}/dng_sdk/source")
target_compile_definitions(dng_sdk PUBLIC qDNGUseXMP=0 qDNGXMPFiles=0
    qDNGXMPDocOps=0 qDNGUseLibJPEG=1 qDNGThreadSafe=1
    qDNGBigEndian=0 qDNGValidate=0 qDNGValidateTarget=0 UNIX_ENV=1)
if(ANDROID)
    target_compile_definitions(dng_sdk PUBLIC qAndroid=1)
else()
    target_compile_definitions(dng_sdk PUBLIC qLinux=1)
endif()
target_link_libraries(dng_sdk PUBLIC dng_jpeg jxl jxl_threads z)

# Standalone file converter, separately licensed from the GPL gallery/LibRaw.
add_executable(dng_decode dng_decode.cpp)
target_link_libraries(dng_decode PRIVATE dng_sdk)
target_compile_options(dng_sdk PRIVATE -ffunction-sections -fdata-sections)
target_link_options(dng_decode PRIVATE -Wl,--gc-sections)
if(ANDROID)
    target_link_options(dng_decode PRIVATE -Wl,-z,max-page-size=16384)
endif()
