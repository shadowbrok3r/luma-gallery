#!/usr/bin/env python3
"""Multi-video editor regression on the s26ultra AVD (1440x3120, density 560).

Requires adb, ffmpeg/ffprobe, Pillow, numpy and Tesseract (TESSDATA_PREFIX=.build/tessdata).
Pushes three uniquely named clips to DCIM/Luma-Multi: a 6 s 960x540 main clip with a 440 Hz
tone, a 4 s 960x540 clip with an 880 Hz tone tagged with a 90 degree rotation like a portrait
phone recording (shown 540x960) and a silent 3 s 640x360 clip. Their
modification times are set ahead of the library so they lead the editor's video picker.
Opens the main clip in the editor, adds the portrait clip before it and the silent clip
after it, plays across the first cut, reopens the editor and exports. The export in
Movies/Luma is checked without the UI: duration, size, letterboxing, the order of the
pictures and each section's audio. Controls and picker tiles are found by label (LumaWidgets
log) or OCR; coordinates measured with ADB screenshots on this AVD are fallbacks. Never deletes
media.
"""
import argparse
import io
import json
import re
import runpy
import subprocess
import time
from pathlib import Path

import numpy as np
from PIL import Image

Device = runpy.run_path(str(Path(__file__).with_name("smoke-android.py")))["Device"]
FOLDER = "/sdcard/DCIM/Luma-Multi"
FIRST_TILE = (226, 1910)
SECOND_TILE = (552, 1910)


def run(*args):
    return subprocess.check_output(list(map(str, args)), stderr=subprocess.PIPE, timeout=180)


def frame(path, at):
    return np.asarray(Image.open(io.BytesIO(run(
        "ffmpeg", "-v", "error", "-ss", f"{at:.3f}", "-i", path, "-frames:v", "1",
        "-f", "image2pipe", "-c:v", "png", "-"))).convert("RGB"), np.int16)


def tone(samples, start, seconds=0.5):
    part = samples[int(start * 8000):int((start + seconds) * 8000)]
    spectrum = np.abs(np.fft.rfft(part))
    return round(np.fft.rfftfreq(len(part), 1 / 8000)[spectrum.argmax()]), np.sqrt((part ** 2).mean())


def film_y(d):
    top, bottom = d.bounds("Film", [42, 2040, 1398, 2236])[1::2]
    return str((top + bottom) // 2)


def stage_center(d):
    left, top, right, bottom = d.bounds("Video frame", [56, 671, 1384, 1418])
    return (left + right) // 2, (top + bottom) // 2


def times(d):
    """Editor time and length from the playback row, left of its buttons."""
    left, top, _, bottom = d.bounds("Go to start", [621, 2427, 761, 2567])
    text = " ".join(row[0] for row in d.lines(d.screen().crop((0, top - 20, left, bottom + 20))))
    found = [int(m[0]) * 60000 + int(m[1]) * 1000 + int(m[2] or 0) for m in re.findall(r"(\d{2}):(\d{2})(?:[.,](\d{3}))?", text)]
    assert len(found) == 2, "No playhead and length: " + text
    return found


def scrub(d, direction):
    """Drags the timeline twice across the screen, far enough to reach either end."""
    for _ in range(2):
        start, end = (1300, 220) if direction > 0 else (220, 1300)  # The track starts right of x=175.
        y = film_y(d)
        d.adb("shell", "input", "swipe", str(start), y, str(end), y, "400")
    time.sleep(1.2)


def pick(d, name, tile, label):
    d.tap_icon("Add video", (155, 2696))
    d.wait_text("Adds the whole video")
    d.screen(f"picker-{label}")
    d.tap_icon(name, tile)
    time.sleep(2.5)
    left, top, right, bottom = d.bounds("Video frame", [56, 671, 1384, 1418])
    d.tap(right - 134, bottom + 50)  # Empty stage deselects.
    time.sleep(0.8)


def main(args):
    d = Device(args.serial, args.evidence)
    assert d.screen().size == (1440, 3120), "Use the portrait s26ultra AVD"
    assert "560" in d.shell("wm", "density"), "Verified coordinates require density 560"
    work = Path(args.evidence)
    work.mkdir(parents=True, exist_ok=True)
    stamp = int(time.time())
    main_name, portrait, silent = (f"Multi-{kind}-{stamp}.mp4" for kind in ("Main", "Portrait", "Silent"))
    success = False
    try:
        run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=960x540:rate=30:duration=6",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=6", "-c:v", "libx264", "-pix_fmt", "yuv420p",
            "-c:a", "aac", "-shortest", work / main_name)
        flat = work / f"flat-{stamp}.mp4"
        run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", "smptebars=size=960x540:rate=30:duration=4",
            "-f", "lavfi", "-i", "sine=frequency=880:duration=4", "-c:v", "libx264", "-pix_fmt", "yuv420p",
            "-c:a", "aac", "-shortest", flat)
        run("ffmpeg", "-v", "error", "-y", "-display_rotation:v:0", "90", "-i", flat, "-c", "copy", work / portrait)
        run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", "mandelbrot=size=640x360:rate=25", "-t", "3",
            "-c:v", "libx264", "-pix_fmt", "yuv420p", work / silent)
        d.shell("mkdir", "-p", FOLDER)
        # The portrait clip is newest, then the silent one; both ahead of media and earlier runs.
        for name, ahead in ((main_name, 0), (silent, 172_799), (portrait, 172_800)):
            d.adb("push", "-q", str(work / name), f"{FOLDER}/{name}")
            if ahead:
                d.shell("touch", "-m", "-d", f"@{stamp + ahead}", f"{FOLDER}/{name}")
            d.shell("am", "broadcast", "-a", "android.intent.action.MEDIA_SCANNER_SCAN_FILE",
                    "-d", f"file://{FOLDER}/{name}")
        time.sleep(2)
        before = set(d.shell("ls", "/sdcard/Movies/Luma/").split())
        d.shell("am", "force-stop", "app.luma.gallery")
        d.shell("am", "start", "-a", "android.intent.action.VIEW", "-d", f"file://{FOLDER}/{main_name}",
                "-t", "video/mp4", "-n", "app.luma.gallery/.GalleryActivity")
        d.wait_text("Export clip")
        d.tap_icon("Pause", (999, 2868))
        d.tap_icon("Edit video", (434, 2714))
        d.wait_text("Edit video")

        pick(d, portrait, FIRST_TILE, "portrait")
        assert times(d) == [0, 10_000], times(d)
        x, y = stage_center(d)
        shot = np.asarray(d.screen("portrait-first"), np.int16)
        assert shot[y, 300].max() < 30 and shot[y, x].max() > 120, "Portrait clip letterboxed at the start"
        d.checks.append("portrait-inserted-before")

        scrub(d, 1)
        at, length = times(d)
        assert length == 10_000 and at >= 9900, (at, length)
        pick(d, silent, SECOND_TILE, "silent")
        assert times(d)[1] == 13_000, times(d)
        d.checks.append("silent-appended")

        scrub(d, -1)
        y = film_y(d)
        d.adb("shell", "input", "swipe", "900", y, "650", y, "500")
        time.sleep(1.2)
        start = times(d)[0]
        assert 500 < start < 3800, start
        d.tap_icon("Play", (999, 2497))
        time.sleep(3.0)
        d.tap_icon("Pause", (999, 2497))
        time.sleep(1.0)
        at = times(d)[0]
        x, y = stage_center(d)
        shot = np.asarray(d.screen("played-across-cut"), np.int16)
        assert at > 4300 and at - start > 2000, (start, at)
        assert shot[y, 300].max() > 120, "Main video fills the canvas after the cut"
        d.checks.append(f"played-{start}-to-{at}-across-cut")

        d.shell("input", "keyevent", "4")  # Closes the editor.
        d.wait_text("Export clip")
        d.tap_icon("Edit video", (434, 2714))
        d.wait_text("Edit video")
        assert times(d)[1] == 13_000, "Project restored: " + str(times(d))
        d.checks.append("project-restored")

        d.tap(1229, 229)  # Export header button.
        d.tap_text("Export video", (384, 2220))
        deadline = time.monotonic() + 150
        exported = None
        while time.monotonic() < deadline and not exported:
            fresh = set(d.shell("ls", "/sdcard/Movies/Luma/").split()) - before
            exported = next((f for f in fresh if f.startswith(main_name[:-4] + "_edit_")), None)
            time.sleep(1)
        assert exported, "No export appeared in Movies/Luma"
        time.sleep(1)
        local = work / exported
        d.adb("pull", "-q", f"/sdcard/Movies/Luma/{exported}", str(local))
        probe = json.loads(run("ffprobe", "-v", "error", "-show_entries",
                               "format=duration:stream=codec_type,width,height", "-of", "json", local))
        duration = float(probe["format"]["duration"])
        assert abs(duration - 13) < 0.15, duration
        video = next(s for s in probe["streams"] if s["codec_type"] == "video")
        assert (video["width"], video["height"]) == (960, 540), video
        assert any(s["codec_type"] == "audio" for s in probe["streams"])
        d.checks.append(f"export-{duration:.2f}s-960x540-with-audio")
        first, middle, last = frame(local, 1.0), frame(local, 6.0), frame(local, 11.5)
        assert first[270, 60].max() < 30 and first[270, 480].max() > 120, "Portrait letterboxed first"
        assert middle[270, 60].max() > 120, "Main clip fills the frame"
        assert last[270, 60].max() > 20 and np.abs(last - middle).mean() > 30, "Silent clip last"
        d.checks.append("export-pictures-in-order")
        samples = np.frombuffer(run("ffmpeg", "-v", "error", "-i", local, "-ac", "1", "-ar", "8000",
                                    "-f", "s16le", "-"), np.int16).astype(float)
        assert tone(samples, 1.5)[0] == 880 and tone(samples, 6.5)[0] == 440, (tone(samples, 1.5), tone(samples, 6.5))
        assert tone(samples, 11.5)[1] < 50, tone(samples, 11.5)
        d.checks.append("export-audio-880-440-silence")
        for name in (main_name, portrait, silent):
            assert d.shell("ls", f"{FOLDER}/{name}").strip(), f"{name} missing"
        d.checks.append("originals-kept")
        success = True
    finally:
        (work / "results.json").write_text(json.dumps({"success": success, "checks": d.checks}, indent=2) + "\n")
    print(json.dumps({"success": success, "checks": d.checks}, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serial", default="emulator-5554")
    parser.add_argument("--evidence", type=Path, default=Path("evidence/multivideo-smoke"))
    main(parser.parse_args())
