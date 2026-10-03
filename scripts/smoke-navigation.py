#!/usr/bin/env python3
"""Photo gestures/theme regression using ARTEMIS/ADB-verified 0.1.2 interactions.

Requires the six-item Luma-Tests album and the same tools as smoke-android.py.
OCR locates text; SVG and native gesture fallbacks scale from 1440 x 3120.
Originals, favorites and permissions are not modified. Rotation is restored.
"""
import argparse
import json
import runpy
import time
from pathlib import Path

from PIL import ImageChops, ImageStat

Device = runpy.run_path(str(Path(__file__).with_name("smoke-android.py")))["Device"]


def swipe(device, start, end, duration=380):
    device.foreground()
    width, height = device.screen().size
    assert width < height
    points = [round(start[0] * width / 1440), round(start[1] * height / 3120),
              round(end[0] * width / 1440), round(end[1] * height / 3120)]
    device.shell("input", "swipe", *points, duration)
    time.sleep(0.5)  # Exceeds the 140 ms thumbnail transition and native event poll.


def rail(image):
    return image.crop((0, round(image.height * .85), image.width, round(image.height * .95)))


def selected_card(image):
    strip = rail(image)
    red, green, blue = strip.split()
    mask = ImageChops.multiply(red.point(lambda p: 255 if p > 210 else 0),
                              green.point(lambda p: 255 if p < 115 else 0))
    mask = ImageChops.multiply(mask, blue.point(lambda p: 255 if 90 < p < 210 else 0))
    bounds = mask.getbbox()
    assert bounds, "Selected thumbnail has no hot-pink rim"
    left, top, right, bottom = bounds
    assert abs((left + right) / 2 - image.width / 2) < image.width * .03
    assert .18 < (right - left) / image.width < .22

    def card_height(x):
        rows = [y for y in range(strip.height) if sum(strip.getpixel((x, y))) > 90]
        return max(rows) - min(rows) if rows else 0

    selected_height = card_height(round(image.width * .5))
    neighbor_height = card_height(round(image.width * 1030 / 1440))
    assert neighbor_height > 0 and selected_height > neighbor_height * 1.15


def run(args):
    d = Device(args.serial, args.evidence)
    auto = d.shell("settings", "get", "system", "accelerometer_rotation")
    rotation = d.shell("settings", "get", "system", "user_rotation")
    checks = []
    try:
        d.shell("settings", "put", "system", "accelerometer_rotation", 0)
        d.shell("settings", "put", "system", "user_rotation", 0)
        d.shell("am", "start", "-S", "-n", "app.luma.gallery/.GalleryActivity")
        _, title_y = d.wait_text("Luma Gallery")
        assert title_y < d.screen().height * .09, "Duplicate top inset returned"
        d.screen("compact-header")
        checks.append("compact-header")
        d.tap_text("Luma-Tests", (350, 1170))
        d.wait_text("6 items")
        shot = d.screen("amoled-album")
        empty = shot.crop((shot.width * .1, shot.height * .55, shot.width * .9, shot.height * .85))
        assert ImageStat.Stat(empty).mean == [0, 0, 0], "Album background is not AMOLED black"
        checks.append("amoled-black")
        d.tap(260, 670)
        d.wait_text("6024 x 4024", timeout=180)
        time.sleep(.5)
        selected_card(d.screen("selected-card"))
        checks.append("expanded-centered-neon-thumbnail")

        for start, end in [((280, 1420), (1160, 1420)),  # First item: do not wrap.
                           ((720, 1050), (790, 1820)),  # Vertical pan: do not navigate.
                           ((720, 1420), (660, 1420))]:  # Short drag: do not navigate.
            swipe(d, start, end)
            d.wait_text("DSC7596")
        checks.append("boundary-vertical-and-short-drags-stay-put")
        swipe(d, (1160, 1420), (280, 1420))
        d.wait_text("uncompressed")
        d.wait_text("7028 x 4688", timeout=180)
        selected_card(d.screen("swipe-next"))
        swipe(d, (280, 1420), (1160, 1420))
        d.wait_text("DSC7596")
        checks.append("swipe-next-and-previous")

        d.tap(1030, 2800)  # Next thumbnail, observed to the right of centered selection.
        d.wait_text("uncompressed")
        d.tap_text("1:1", (1180, 2580))
        d.wait_text("100%")
        before = d.media("zoom-before-pan")
        swipe(d, (1160, 1420), (280, 1420))
        d.wait_text("uncompressed")
        d.wait_text("100%")
        after = d.media("zoom-after-pan")
        assert sum(ImageStat.Stat(ImageChops.difference(before, after)).mean) > 5
        checks.append("zoomed-drag-pans-without-navigation")
        d.tap(1340, 2580)  # Fit SVG control.
        swipe(d, (1160, 1420), (280, 1420))
        d.wait_text("lossless")
        d.wait_text("7028 x 4688", timeout=180)
        selected_card(d.screen("thumbnail-selection"))
        checks.append("thumbnail-tap-and-fit-restore-navigation")
        before = rail(d.screen())
        swipe(d, (1120, 2800), (300, 2800), 650)
        d.wait_text("lossless")
        after = rail(d.screen("rail-scrolled"))
        assert sum(ImageStat.Stat(ImageChops.difference(before, after)).mean) > 5
        time.sleep(1)
        assert sum(ImageStat.Stat(ImageChops.difference(after, rail(d.screen()))).mean) < 1
        checks.append("rail-scroll-does-not-select-or-snap-back")

        d.back_to_album()
        d.tap(1170, 390)  # Reverse date order.
        d.tap(1180, 670)  # JPEG is now the third cell of the first row.
        d.wait_text("Jewelry-4K")
        swipe(d, (1160, 1420), (280, 1420))
        d.wait_text("lossless")
        swipe(d, (280, 1420), (1160, 1420))
        d.wait_text("Jewelry-4K")
        checks.append("swipes-follow-reversed-grid-order")
        d.shell("settings", "put", "system", "user_rotation", 1)
        time.sleep(2)
        d.wait_text("3840 x 2160")
        d.media("landscape-photo-sidebar")
        assert d.screen().width > d.screen().height
        checks.append("landscape-photo-controls-and-rail")
        d.shell("settings", "put", "system", "user_rotation", 0)
        time.sleep(2)
        # Reversed order: JPEG's previous neighbor is the 4K video.
        swipe(d, (280, 1420), (1160, 1420))
        d.wait_text("C0038")
        d.wait_text("2160p")
        time.sleep(1.5)
        d.media("swipe-photo-to-video")
        checks.append("photo-to-video-transition")
        d.back_to_album()
        d.tap(1170, 390)  # Restore newest first.
        d.tap(1010, 390)  # RAW filter.
        d.wait_text("3 items")
        d.tap(1180, 670)
        d.wait_text("lossless")
        swipe(d, (1160, 1420), (280, 1420))
        d.wait_text("lossless")
        checks.append("last-filtered-photo-does-not-wrap-or-open-video")
        print(json.dumps({"result": "passed", "checks": checks}, indent=2), flush=True)
        (d.evidence / "result.json").write_text(json.dumps(checks, indent=2))
    finally:
        d.shell("settings", "put", "system", "user_rotation", rotation)
        d.shell("settings", "put", "system", "accelerometer_rotation", auto)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--serial", required=True)
    parser.add_argument("--evidence", default="evidence/navigation")
    run(parser.parse_args())
