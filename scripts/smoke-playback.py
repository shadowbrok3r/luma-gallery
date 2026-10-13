#!/usr/bin/env python3
"""Viewer playback regression on the s26ultra AVD (1440x3120, density 560).

Requires adb, ffmpeg, Pillow, numpy and Tesseract (TESSDATA_PREFIX=.build/tessdata).
Pushes a uniquely named 6 s clip to DCIM/Luma-Playback and checks the cases where LibVLC
reached the end of its input: scrubbing and playing after the clip ended, playing again from
the end, scrubbing while playing, scrubbing to the end while playing, and looping the whole
clip. Controls and lanes are found from the app's LumaWidgets log; coordinates measured with
ADB screenshots on this AVD are fallbacks. Never deletes media.
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
PLAY = (999, 2868)
LOOP = (1160, 2560)
LENGTH = 6000


def rail(d):
    """Video lane x range for the whole clip at 1x zoom, and its middle y."""
    left, top, right, bottom = d.bounds("Video lane", [84, 2093, 1356, 2254])
    return left, right, (top + bottom) // 2


def toggle(d):
    """Taps Play or Pause, whichever the viewer shows."""
    shown = {name for name, _ in d.controls()}
    d.tap_icon("Pause" if "Pause" in shown else "Play", PLAY)


def position(d):
    top = d.bounds("Video lane", [84, 2093, 1356, 2254])[1]
    text = " ".join(row[0] for row in d.lines(d.screen().crop((0, top - 180, 760, top - 10))))
    match = re.search(r"(\d{2}):(\d{2})[.,](\d{3})", text)
    assert match, "No playhead time: " + text
    return int(match[1]) * 60000 + int(match[2]) * 1000 + int(match[3])


def stage(d):
    left, top, right, bottom = d.bounds("Media stage", [42, 313, 1398, 1880])
    middle = (top + bottom) // 2
    return np.asarray(d.screen().crop((left + 60, middle - 320, right - 60, middle + 320)), np.int16)


def moving(d):
    first = stage(d)
    time.sleep(0.6)
    return np.abs(stage(d) - first).mean() > 2


def no_error(d):
    text = " ".join(row[0] for row in d.lines(d.screen().crop((0, 2600, 1440, 3000)))).lower()
    assert "could not play" not in text, "Playback error notice: " + text


def x_for(d, ms):
    left, right, _ = rail(d)
    return round(left + (right - left) * ms / LENGTH)


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

        y = rail(d)[2]
        d.tap(x_for(d, 3000), y)
        time.sleep(1.5)
        at = position(d)
        assert abs(at - 3000) < 250, at
        assert not moving(d), "Seek after the end should stay paused"
        toggle(d)
        plays_from(d, 3600, 5200)
        d.screen("plays-after-scrub")
        no_error(d)
        d.checks.append("scrub-after-end-then-play")

        wait_end(d)
        toggle(d)
        plays_from(d, 600, 2600)
        no_error(d)
        d.checks.append("play-at-end-restarts")

        d.adb("shell", "input", "swipe", str(x_for(d, 800)), str(y), str(x_for(d, 2500)), str(y), "500")
        plays_from(d, 2900, 4700)
        no_error(d)
        d.checks.append("scrub-while-playing-resumes")

        d.adb("shell", "input", "swipe", str(x_for(d, 2500)), str(y), "1435", str(y), "500")
        wait_end(d, 6)
        toggle(d)
        plays_from(d, 600, 2600)
        no_error(d)
        d.checks.append("scrub-to-end-while-playing")

        d.tap_icon("Loop selection", LOOP)
        samples = []
        deadline = time.monotonic() + 9
        while time.monotonic() < deadline:
            samples.append(position(d))
        assert any(b < a - 2000 for a, b in zip(samples, samples[1:])), f"No wrap: {samples}"
        assert moving(d), "Loop stopped playing"
        no_error(d)
        d.screen("looping")
        d.checks.append(f"loop-wraps-{len(samples)}-samples")
        d.tap_icon("Loop selection", LOOP)
        success = True
    finally:
        (work / "results.json").write_text(json.dumps({"success": success, "checks": d.checks}, indent=2) + "\n")
    print(json.dumps({"success": success, "checks": d.checks}, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serial", default="emulator-5554")
    parser.add_argument("--evidence", type=Path, default=Path("evidence/playback-smoke"))
    main(parser.parse_args())
