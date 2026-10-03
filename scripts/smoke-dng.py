#!/usr/bin/env python3
"""ARTEMIS/ADB-verified S26 DNG viewer regression; preserves media and settings.

Requires DCIM/Luma-DNG-Tests/20260916_183018.dng and smoke-android.py's tools.
"""
import argparse
import json
import runpy
import time
from pathlib import Path

from PIL import ImageChops, ImageStat

Device = runpy.run_path(str(Path(__file__).with_name("smoke-android.py")))["Device"]


def run(args):
    d = Device(args.serial, args.evidence)
    auto = d.shell("settings", "get", "system", "accelerometer_rotation")
    rotation = d.shell("settings", "get", "system", "user_rotation")
    checks = []
    try:
        d.shell("settings", "put", "system", "accelerometer_rotation", 0)
        d.shell("settings", "put", "system", "user_rotation", 0)
        d.shell("am", "start", "-S", "-n", "app.luma.gallery/.GalleryActivity")
        d.wait_text("Luma Gallery")
        d.tap_text("Luma-DNG-Tests", (350, 1170))
        d.wait_text("1 items")
        d.tap(260, 670)
        d.wait_text("20260916_183018")
        d.wait_text("Full-resolution RAW", timeout=180)
        d.wait_text("3060 x 4080")
        d.media("full-resolution-portrait")
        checks.append("jpeg-xl-dng-full-resolution-and-orientation")
        d.tap_text("1:1", (1180, 2580))
        d.wait_text("100%")
        before = d.media("one-to-one")
        w, h = d.screen().size
        d.shell("input", "swipe", round(w * .8), round(h * .45),
                round(w * .2), round(h * .45), 380)
        time.sleep(.4)
        d.wait_text("20260916_183018")
        d.wait_text("100%")
        after = d.media("zoomed-pan")
        assert sum(ImageStat.Stat(ImageChops.difference(before, after)).mean) > 5
        checks.append("one-to-one-tiles-and-pan")
        d.tap(1340, 2580)
        d.shell("settings", "put", "system", "user_rotation", 1)
        time.sleep(1)
        d.wait_text("3060 x 4080")
        assert d.screen().width > d.screen().height
        d.media("full-resolution-landscape")
        checks.append("landscape-retains-portrait-image-orientation")
        d.shell("settings", "put", "system", "user_rotation", 0)
        time.sleep(1)
        d.shell("input", "keyevent", 4)
        d.wait_text("1 items")
        d.tap(260, 670)
        d.wait_text("Full-resolution RAW")
        d.wait_text("3060 x 4080")
        d.media("cached-reopen")
        checks.append("developed-cache-reopens-at-full-resolution")
        report = {"result": "passed", "checks": checks}
        (d.evidence / "result.json").write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps(report, indent=2))
    finally:
        d.shell("settings", "put", "system", "user_rotation", rotation)
        d.shell("settings", "put", "system", "accelerometer_rotation", auto)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--serial", required=True)
    parser.add_argument("--evidence", default="evidence/dng-ui")
    run(parser.parse_args())
