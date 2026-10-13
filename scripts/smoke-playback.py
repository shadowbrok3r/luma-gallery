#!/usr/bin/env python3
"""Viewer playback regression on the s26ultra AVD (1440x3120, density 560).

Requires adb, ffmpeg, Pillow, numpy and Tesseract (TESSDATA_PREFIX=.build/tessdata).
Pushes a uniquely named 6 s clip to DCIM/Luma-Playback and checks the cases where LibVLC
reached the end of its input: scrubbing and playing after the clip ended, playing again from
the end, scrubbing while playing, scrubbing to the end while playing, and looping the whole
clip. Control coordinates were verified with ADB screenshots on this AVD. Never deletes media.
"""
import argparse
import json
import re
import runpy
import subprocess
import time
from pathlib import Path

import numpy as np

Device = runpy.run_path(str(Path(__file__).with_name("smoke-android.py")))["Device"]
FOLDER = "/sdcard/DCIM/Luma-Playback"
PLAY = (434, 2714)
LOOP = (1181, 2714)
RAIL_Y = 2324
RAIL = (84, 1370)  # Timeline x range for the whole clip at 1x zoom.
LENGTH = 6000


def position(d):
    text = " ".join(row[0] for row in d.lines(d.screen().crop((0, 2040, 760, 2150))))
    match = re.search(r"(\d{2}):(\d{2})[.,](\d{3})", text)
    assert match, "No playhead time: " + text
    return int(match[1]) * 60000 + int(match[2]) * 1000 + int(match[3])


def stage(d):
    return np.asarray(d.screen().crop((100, 850, 1350, 1500)), np.int16)


def moving(d):
    first = stage(d)
    time.sleep(0.6)
    return np.abs(stage(d) - first).mean() > 2


def no_error(d):
    text = " ".join(row[0] for row in d.lines(d.screen().crop((0, 2600, 1440, 3000)))).lower()
    assert "could not play" not in text, "Playback error notice: " + text


def x_for(ms):
    return round(RAIL[0] + (RAIL[1] - RAIL[0]) * ms / LENGTH)


def wait_end(d, timeout=12):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if not moving(d) and position(d) > LENGTH - 600:
            return
    raise AssertionError("Playback did not stop at the end")


def plays_from(d, low, high):
    time.sleep(1.2)
    at = position(d)
    assert moving(d), f"Not playing at {at}"
    assert low <= at <= high, (at, low, high)
    return at


def main(args):
    d = Device(args.serial, args.evidence)
    assert d.screen().size == (1440, 3120), "Use the portrait s26ultra AVD"
    assert "560" in d.shell("wm", "density"), "Verified coordinates require density 560"
    work = Path(args.evidence)
    work.mkdir(parents=True, exist_ok=True)
    name = f"Playback-Smoke-{int(time.time())}.mp4"
    success = False
    try:
        subprocess.check_call(
            ["ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=960x540:rate=30:duration=6",
             "-f", "lavfi", "-i", "sine=frequency=440:duration=6", "-c:v", "libx264", "-crf", "20",
             "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", str(work / name)], timeout=120)
        d.shell("mkdir", "-p", FOLDER)
        d.adb("push", "-q", str(work / name), f"{FOLDER}/{name}")
        d.shell("am", "force-stop", "app.luma.gallery")
        d.shell("am", "start", "-a", "android.intent.action.VIEW", "-d", f"file://{FOLDER}/{name}",
                "-t", "video/mp4", "-n", "app.luma.gallery/.GalleryActivity")
        d.wait_text("Export clip")

        wait_end(d)
        d.screen("ended")
        d.checks.append("stops-at-end")

        d.tap(x_for(3000), RAIL_Y)
        time.sleep(1.5)
        at = position(d)
        assert abs(at - 3000) < 250, at
        assert not moving(d), "Seek after the end should stay paused"
        d.tap(*PLAY)
        plays_from(d, 3600, 5200)
        d.screen("plays-after-scrub")
        no_error(d)
        d.checks.append("scrub-after-end-then-play")

        wait_end(d)
        d.tap(*PLAY)
        plays_from(d, 600, 2600)
        no_error(d)
        d.checks.append("play-at-end-restarts")

        d.adb("shell", "input", "swipe", str(x_for(800)), str(RAIL_Y), str(x_for(2500)), str(RAIL_Y), "500")
        plays_from(d, 2900, 4700)
        no_error(d)
        d.checks.append("scrub-while-playing-resumes")

        d.adb("shell", "input", "swipe", str(x_for(2500)), str(RAIL_Y), "1435", str(RAIL_Y), "500")
        wait_end(d, 6)
        d.tap(*PLAY)
        plays_from(d, 600, 2600)
        no_error(d)
        d.checks.append("scrub-to-end-while-playing")

        d.tap(*LOOP)
        samples = []
        deadline = time.monotonic() + 9
        while time.monotonic() < deadline:
            samples.append(position(d))
        assert any(b < a - 2000 for a, b in zip(samples, samples[1:])), f"No wrap: {samples}"
        assert moving(d), "Loop stopped playing"
        no_error(d)
        d.screen("looping")
        d.checks.append(f"loop-wraps-{len(samples)}-samples")
        d.tap(*LOOP)
        success = True
    finally:
        (work / "results.json").write_text(json.dumps({"success": success, "checks": d.checks}, indent=2) + "\n")
    print(json.dumps({"success": success, "checks": d.checks}, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serial", default="emulator-5554")
    parser.add_argument("--evidence", type=Path, default=Path("evidence/playback-smoke"))
    main(parser.parse_args())
