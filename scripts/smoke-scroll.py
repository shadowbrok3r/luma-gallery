#!/usr/bin/env python3
"""Pixel-tracked continuous album drags, explored with ARTEMIS and ADB.

Requires a portrait device with at least seven rows of albums, Pillow, numpy,
adb, and Tesseract. No files, permissions or favorites are changed.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import json
import runpy
import time
from pathlib import Path

import numpy as np

Device = runpy.run_path(str(Path(__file__).with_name("smoke-android.py")))["Device"]


def pixels(image):
    # Exclude the fixed header/nav bar; retain image detail and moving card edges.
    return np.asarray(image.resize((288, 624)).convert("L"), dtype=np.float32)[150:578, 18:268]


def displacement(before, after):
    height = before.shape[0]
    scores = []
    for dy in range(-45, 46):
        if dy >= 0:
            error = np.abs(after[dy:] - before[:height - dy]).mean()
        else:
            error = np.abs(after[:height + dy] - before[-dy:]).mean()
        scores.append((float(error), dy))
    return min(scores)


def run(args):
    d = Device(args.serial, args.evidence)
    auto = d.shell("settings", "get", "system", "accelerometer_rotation")
    rotation = d.shell("settings", "get", "system", "user_rotation")
    reports = []
    try:
        d.shell("settings", "put", "system", "accelerometer_rotation", 0)
        d.shell("settings", "put", "system", "user_rotation", 0)
        d.shell("am", "start", "-S", "-n", "app.luma.gallery/.GalleryActivity")
        d.wait_text("Luma Gallery")
        time.sleep(1)
        width, height = d.screen().size
        assert width < height
        for direction, ys in [(-1, [2700, 800]), (1, [800, 2700])]:
            d.foreground()
            before = pixels(d.screen())
            deltas = []
            with ThreadPoolExecutor(max_workers=1) as pool:
                x = round(650 * width / 1440)
                pending = pool.submit(d.shell, "input", "swipe", x,
                    round(ys[0] * height / 3120), x, round(ys[1] * height / 3120), 9000)
                while not pending.done():
                    shot = d.screen()
                    after = pixels(shot)
                    score, dy = displacement(before, after)
                    deltas.append({"dy": dy, "error": round(score, 3)})
                    shot.resize((720, 1560)).save(d.evidence / f"{direction}-{len(deltas):03}.png")
                    before = after
                    time.sleep(.06)
                pending.result(timeout=15)
            reliable = [item["dy"] for item in deltas if item["error"] < 8]
            (d.evidence / f"{direction}-measurements.json").write_text(json.dumps(deltas, indent=2))
            assert len(reliable) >= 10, "Insufficient matching frames"
            assert sum(direction * dy for dy in reliable) > 120, "Albums did not scroll far enough"
            assert min(direction * dy for dy in reliable) >= -1, "Visible reverse jump during drag"
            reports.append({"direction": direction, "frames": deltas})
            time.sleep(1)  # Allow release inertia to settle before the reverse drag.
        d.wait_text("Luma Gallery")
        report = {"result": "passed", "drags": reports}
        (d.evidence / "result.json").write_text(json.dumps(report, indent=2) + "\n")
        print("Passed pixel-tracked continuous down/up album drags")
    finally:
        d.shell("settings", "put", "system", "user_rotation", rotation)
        d.shell("settings", "put", "system", "accelerometer_rotation", auto)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--serial", required=True)
    parser.add_argument("--evidence", default="evidence/scroll-ui")
    run(parser.parse_args())
