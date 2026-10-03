// SPDX-License-Identifier: MIT
// This standalone converter exchanges ordinary files, not SDK objects, with Luma.
#include "dng_color_space.h"
#include "dng_exceptions.h"
#include "dng_file_stream.h"
#include "dng_host.h"
#include "dng_image.h"
#include "dng_info.h"
#include "dng_memory_stream.h"
#include "dng_negative.h"
#include "dng_pixel_buffer.h"
#include "dng_render.h"
#include "dng_tag_types.h"
#include <cstdio>
#include <cstring>
#include <stdexcept>
#include <vector>
#include <unistd.h>

static void write_ppm(const char *path, const dng_image &image) {
    const auto bounds = image.Bounds();
    const auto width = bounds.W();
    const auto height = bounds.H();
    if (image.Planes() != 3 || image.PixelType() != ttByte)
        throw std::runtime_error("Expected rendered sRGB pixels");
    FILE *file = std::fopen(path, "wb");
    if (!file) throw std::runtime_error("Cannot create rendered image");
    try {
        if (std::fprintf(file, "P6\n%u %u\n255\n", width, height) < 0)
            throw std::runtime_error("Cannot write image header");
        std::vector<unsigned char> row(static_cast<size_t>(width) * 3);
        for (int32 y = bounds.t; y < bounds.b; ++y) {
            dng_pixel_buffer pixels;
            pixels.fArea = dng_rect(y, bounds.l, y + 1, bounds.r);
            pixels.fPlane = 0;
            pixels.fPlanes = 3;
            pixels.fRowStep = width * 3;
            pixels.fColStep = 3;
            pixels.fPlaneStep = 1;
            pixels.fPixelType = ttByte;
            pixels.fPixelSize = 1;
            pixels.fData = row.data();
            image.Get(pixels);
            if (std::fwrite(row.data(), 1, row.size(), file) != row.size())
                throw std::runtime_error("Cannot write image pixels");
        }
    } catch (...) {
        std::fclose(file);
        std::remove(path);
        throw;
    }
    if (std::fclose(file)) {
        std::remove(path);
        throw std::runtime_error("Cannot finish rendered image");
    }
}

int main(int argc, char **argv) {
    if (argc != 4 || (std::strcmp(argv[3], "full") && std::strcmp(argv[3], "preview"))) {
        std::fprintf(stderr, "usage: dng_decode input|- output.ppm full|preview\n");
        return 2;
    }
    try {
        dng_host host;
        AutoPtr<dng_stream> input;
        if (!std::strcmp(argv[1], "-")) {
            auto *memory = new dng_memory_stream(host.Allocator());
            input.Reset(memory);
            unsigned char block[65536];
            ssize_t count;
            uint64 total = 0;
            while ((count = read(STDIN_FILENO, block, sizeof(block))) > 0) {
                total += count;
                if (total > 512u * 1024u * 1024u)
                    throw std::runtime_error("DNG exceeds the 512 MiB input limit");
                memory->Put(block, static_cast<uint32>(count));
            }
            if (count < 0) throw std::runtime_error("Cannot read DNG");
            memory->SetReadPosition(0);
        } else input.Reset(new dng_file_stream(argv[1]));

        dng_info info;
        info.Parse(host, *input);
        info.PostParse(host);
        if (!info.IsValidDNG()) throw std::runtime_error("Not a valid DNG");
        const auto &main = *info.fIFD.at(info.fMainIndex);
        if (static_cast<uint64>(main.fImageWidth) * main.fImageLength > 200000000)
            throw std::runtime_error("DNG exceeds the 200 MP image limit");
        AutoPtr<dng_negative> negative(host.Make_dng_negative());
        negative->Parse(host, *input, info);
        negative->PostParse(host, *input, info);
        if (info.fEnhancedIndex != -1)
            negative->ReadEnhancedImage(host, *input, info);
        else
            negative->ReadStage1Image(host, *input, info);
        if (negative->Stage1Image()) negative->BuildStage2Image(host);
        if (negative->Stage2Image()) negative->BuildStage3Image(host);

        dng_render render(host, *negative);
        render.SetFinalSpace(dng_space_sRGB::Get());
        render.SetFinalPixelType(ttByte);
        if (!std::strcmp(argv[3], "preview")) render.SetMaximumSize(1600);
        AutoPtr<dng_image> image(render.Render());
        image->Rotate(negative->Orientation());
        write_ppm(argv[2], *image);
        std::fprintf(stderr, "DNG SDK %s: compression=%u, rendered=%ux%u, orientation=%u\n",
            kDNGSDK_GetInfoVersion, main.fCompression, image->Bounds().W(),
            image->Bounds().H(), negative->Orientation().GetTIFF());
        return 0;
    } catch (const dng_exception &error) {
        std::fprintf(stderr, "DNG decode error %d\n", error.ErrorCode());
    } catch (const std::exception &error) {
        std::fprintf(stderr, "DNG: %s\n", error.what());
    } catch (...) {
        std::fprintf(stderr, "DNG: unexpected decode failure\n");
    }
    return 1;
}
