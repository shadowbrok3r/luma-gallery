#include <libraw/libraw.h>
#include <cstdio>
#include <cstring>
#include <vector>
#include <unistd.h>

// Runs out of process so a 33 MP demosaic cannot exhaust the Android UI heap.
int main(int argc, char **argv) {
    if (argc != 4) {
        std::fprintf(stderr, "usage: raw_decode input output preview|full\n");
        return 2;
    }
    LibRaw raw;
    raw.imgdata.params.use_camera_wb = 1;
    raw.imgdata.params.output_color = 1;
    raw.imgdata.params.output_bps = 8;
    raw.imgdata.params.no_auto_bright = 1;
    raw.imgdata.params.user_qual = 3;
    std::vector<unsigned char> input;
    int rc;
    if (!std::strcmp(argv[1], "-")) {
        unsigned char block[65536];
        ssize_t count;
        while ((count = read(STDIN_FILENO, block, sizeof(block))) > 0) {
            if (input.size() + count > 512u * 1024u * 1024u) return 2;
            input.insert(input.end(), block, block + count);
        }
        rc = count < 0 ? LIBRAW_IO_ERROR : raw.open_buffer(input.data(), input.size());
    } else rc = raw.open_file(argv[1]);
    if (rc == LIBRAW_SUCCESS) {
        if (!std::strcmp(argv[3], "preview")) {
            rc = raw.unpack_thumb();
            if (!rc) rc = raw.dcraw_thumb_writer(argv[2]);
        } else {
            rc = raw.unpack();
            if (!rc) rc = raw.dcraw_process();
            if (!rc) rc = raw.dcraw_ppm_tiff_writer(argv[2]);
        }
    }
    if (rc) std::fprintf(stderr, "LibRaw: %s\n", libraw_strerror(rc));
    return rc ? 1 : 0;
}
