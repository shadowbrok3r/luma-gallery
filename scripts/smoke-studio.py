#!/usr/bin/env python3
"""Video editor regression on the s26ultra AVD (1440x3120, density 560).

Requires adb, ffmpeg/ffprobe, Pillow, numpy and Tesseract (TESSDATA_PREFIX=.build/tessdata).
Pushes a uniquely named synthetic clip to DCIM/Luma-Studio, opens it in the editor, adds a
caption at 0 s, splits at about 4 s and 6 s, deletes the middle part and exports. The export
in Movies/Luma is checked without the UI: duration, audio, the burned-in caption (OCR) and
the cut (frames after it match the source two seconds later). Controls are found by label and
lanes by position from the app's LumaWidgets log; coordinates measured with ADB screenshots on
this AVD are fallbacks. Never deletes media.
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
from PIL import Image, ImageOps

Device = runpy.run_path(str(Path(__file__).with_name("smoke-android.py")))["Device"]
FOLDER = "/sdcard/DCIM/Luma-Studio"
CENTER_X = 807
SCALE = 94.5  # Device pixels per second at the initial zoom of a 10 s clip.


def run(*args):
    return subprocess.check_output(list(map(str, args)), stderr=subprocess.PIPE, timeout=180)


def stamps(path, start, seconds):
    """The testsrc2 timecode corner of each 960x540 frame in a range."""
    raw = run("ffmpeg", "-v", "error", "-ss", f"{start:.3f}", "-t", f"{seconds:.3f}", "-i", path,
              "-f", "rawvideo", "-pix_fmt", "rgb24", "-")
    return np.frombuffer(raw, np.uint8).reshape(-1, 540, 960, 3)[:, :40, :130].astype(np.int16)


def frame(path, at):
    return Image.open(io.BytesIO(run(
        "ffmpeg", "-v", "error", "-ss", f"{at:.3f}", "-i", path, "-frames:v", "1",
        "-f", "image2pipe", "-c:v", "png", "-"))).convert("RGB")


def ocr(image, path):
    """Reads near-white caption pixels as dark text on white."""
    mask = image.point(lambda v: 255 if v > 235 else 0).convert("L").point(lambda v: 255 if v > 250 else 0)
    ImageOps.invert(mask).save(path)
    return subprocess.run(["tesseract", str(path), "-", "--psm", "7"], capture_output=True, text=True).stdout.upper()


def film_y(d):
    top, bottom = d.bounds("Film", [42, 2040, 1398, 2236])[1::2]
    return (top + bottom) // 2


def playhead(d):
    """Editor time from the playback row, left of its buttons."""
    left, top, _, bottom = d.bounds("Go to start", [621, 2427, 761, 2567])
    text = " ".join(row[0] for row in d.lines(d.screen().crop((0, top - 20, left, bottom + 20))))
    match = re.search(r"(\d{2}):(\d{2})[.,](\d{3})", text)
    assert match, "No playhead time: " + text
    return int(match[1]) * 60000 + int(match[2]) * 1000 + int(match[3])


def scrub(d, ms):
    """Moves the playhead by about `ms` with on-screen drags of the timeline."""
    remaining = round(ms * SCALE / 1000)
    while remaining:
        step = max(-1100, min(1100, remaining))
        start = 1300 if step > 0 else 200
        y = str(film_y(d))
        d.adb("shell", "input", "swipe", str(start), y, str(start - step), y, "600")
        remaining -= step
    time.sleep(1.2)


def main(args):
    d = Device(args.serial, args.evidence)
    assert d.screen().size == (1440, 3120), "Use the portrait s26ultra AVD"
    assert "560" in d.shell("wm", "density"), "Verified coordinates require density 560"
    work = Path(args.evidence)
    work.mkdir(parents=True, exist_ok=True)
    name = f"Studio-Smoke-{int(time.time())}.mp4"
    source = work / name
    success = False
    try:
        run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=960x540:rate=30:duration=10",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=10", "-c:v", "libx264", "-crf", "18",
            "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", source)
        d.shell("mkdir", "-p", FOLDER)
        d.adb("push", "-q", str(source), f"{FOLDER}/{name}")
        before = set(d.shell("ls", "/sdcard/Movies/Luma/").split())
        d.shell("am", "force-stop", "app.luma.gallery")
        d.shell("am", "start", "-a", "android.intent.action.VIEW", "-d", f"file://{FOLDER}/{name}",
                "-t", "video/mp4", "-n", "app.luma.gallery/.GalleryActivity")
        d.wait_text("Export clip")
        d.tap_icon("Pause", (999, 2868))
        d.tap_icon("Edit video", (434, 2714))
        d.wait_text("Edit video")
        d.screen("editor-open")
        scrub(d, -12000)
        assert playhead(d) == 0, "Timeline did not reach the start"
        d.checks.append("timeline-scrubs-to-start")
        d.tap_icon("Text", (550, 2878))
        d.tap_icon("Add text", (155, 2696))
        time.sleep(1)
        d.adb("shell", "input", "text", "LUMA%sSMOKE")
        time.sleep(.8)
        d.adb("shell", "input", "keyevent", "111")  # Closes the text panel.
        time.sleep(.8)
        picture = d.bounds("Video frame", [56, 671, 1384, 1418])
        d.tap(picture[2] - 134, picture[3] + 50)  # Empty stage deselects.
        d.screen("caption-added")
        start = playhead(d)
        scrub(d, 4000)
        first = playhead(d)
        assert 3500 < first < 4500, first
        d.tap_icon("Video", (211, 2878))
        d.tap_icon("Split", (381, 2696))
        scrub(d, 2000)
        second = playhead(d)
        assert second - first > 1500, (first, second)
        d.tap_icon("Split", (155, 2696))  # Splits the selected segment.
        d.tap(CENTER_X - round(SCALE), film_y(d))  # Middle segment, one second left of the playhead.
        d.tap_icon("Delete", (1285, 2696))
        time.sleep(.8)
        d.screen("middle-removed")
        d.wait_text("/ 00:0")
        d.checks.append(f"split-at-{first}-and-{second}-and-delete")
        d.tap(1229, 229)  # Export header button.
        d.tap_text("Export video", (384, 2220))
        deadline = time.monotonic() + 120
        exported = None
        while time.monotonic() < deadline and not exported:
            fresh = set(d.shell("ls", "/sdcard/Movies/Luma/").split()) - before
            exported = next((f for f in fresh if f.startswith(name[:-4] + "_edit_")), None)
            time.sleep(1)
        assert exported, "No export appeared in Movies/Luma"
        time.sleep(1)
        local = work / exported
        d.adb("pull", "-q", f"/sdcard/Movies/Luma/{exported}", str(local))
        probe = json.loads(run("ffprobe", "-v", "error", "-show_entries", "format=duration:stream=codec_type",
                               "-of", "json", local))
        duration = float(probe["format"]["duration"])
        expected = 10 - (second - first) / 1000
        assert abs(duration - expected) < 0.15, (duration, expected)
        assert {s["codec_type"] for s in probe["streams"]} == {"video", "audio"}
        d.checks.append(f"export-duration-{duration:.2f}s-with-audio")
        caption = frame(local, (start + 1500) / 1000)
        caption.save(work / "export-caption.png")
        read = ocr(caption.crop((280, 360, 680, 450)), work / "caption-ocr.png")
        assert "LUMA" in read and "SMOKE" in read, "Caption OCR: " + read
        d.checks.append("caption-burned-in-at-start")
        read = ocr(frame(local, (start + 3400) / 1000).crop((280, 360, 680, 450)), work / "after-ocr.png")
        assert "LUMA" not in read, "Caption still visible: " + read
        d.checks.append("caption-ends-after-three-seconds")
        after = stamps(local, first / 1000 + 0.5, 0.04)[0]
        window = second / 1000
        errors = [np.abs(after - c).mean() for c in stamps(source, window, 1.0)]
        best = int(np.argmin(errors))
        offset = window + best / 30
        assert errors[best] < 12 and abs(offset - (window + 0.5)) < 0.12, (errors[best], offset)
        d.checks.append(f"cut-skips-to-source-{offset:.2f}s")
        assert d.shell("ls", f"{FOLDER}/{name}").strip(), "Original missing"
        d.checks.append("original-kept")
        success = True
    finally:
        (work / "results.json").write_text(json.dumps({"success": success, "checks": d.checks}, indent=2) + "\n")
    print(json.dumps({"success": success, "checks": d.checks}, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serial", default="emulator-5554")
    parser.add_argument("--evidence", type=Path, default=Path("evidence/studio-smoke"))
    main(parser.parse_args())
