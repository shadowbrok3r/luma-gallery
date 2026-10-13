#!/usr/bin/env python3
"""Speech caption regression on the s26ultra AVD (1440x3120, density 560).

Requires adb, ffmpeg/ffprobe, Pillow, numpy, Tesseract (TESSDATA_PREFIX=.build/tessdata) and
native/vendor/whisper.cpp-1.9.5/samples/jfk.wav. The first run needs network access: the app
downloads its 57 MB speech model. Pushes a uniquely named 11 s clip of the public-domain JFK sample
over a plain background, transcribes it in the editor, adds one-word captions, drags them to the
top of the frame and exports. Checks the transcript, the word on the stage at the playhead, and in
the export the word shown where whisper's base model places it, the empty gap after "Americans,"
and the captions' new place. Controls are found by label (LumaWidgets log) or OCR; coordinates
measured with ADB screenshots on this AVD are fallbacks. Never deletes media.
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
FOLDER = "/sdcard/DCIM/Luma-Captions"
SAMPLE = Path(__file__).resolve().parent.parent / "native/vendor/whisper.cpp-1.9.5/samples/jfk.wav"
# Video frame of a 16:9 clip with the panel closed and one caption layer, in screen pixels.
STAGE = (56, 768, 1384, 1515)
TRANSCRIPT = "and so my fellow americans ask not what your country can do for you ask what you can do for your country"
# Word on screen in the middle of its span, from whisper base q5_1 offsets for jfk.wav.
EXPECTED = [(0.95, "fellow"), (2.8, None), (3.55, "ask"), (4.5, "not"), (6.0, "country")]


def run(*args):
    return subprocess.check_output(list(map(str, args)), stderr=subprocess.PIPE, timeout=180)


def words(text):
    return re.sub(r"[^a-z ]", "", text.lower()).split()


def read_white(image):
    """OCR of white caption letters only, as black text on white."""
    pixels = np.asarray(image.convert("RGB"), np.int16)
    mask = (pixels.min(axis=2) > 215)
    if mask.sum() < 40:
        return ""
    page = Image.fromarray(np.where(mask, 0, 255).astype(np.uint8)).resize((image.width * 2, image.height * 2))
    png = io.BytesIO()
    page.save(png, format="PNG")
    out = subprocess.run(["tesseract", "stdin", "stdout", "--psm", "7"], input=png.getvalue(),
                         capture_output=True, check=True, timeout=20).stdout.decode()
    return " ".join(words(out))


def frame(path, at):
    return Image.open(io.BytesIO(run("ffmpeg", "-v", "error", "-ss", f"{at:.3f}", "-i", path, "-frames:v", "1",
                                     "-f", "image2pipe", "-c:v", "png", "-")))


def playhead(d):
    """Editor time from the playback row, left of its buttons."""
    left, top, _, bottom = d.bounds("Go to start", [621, 2427, 761, 2567])
    text = " ".join(row[0] for row in d.lines(d.screen().crop((0, top - 20, left, bottom + 20))))
    match = re.search(r"(\d{2}):(\d{2})[.,](\d{3})", text)
    assert match, "No playhead time: " + text
    return int(match[1]) * 60000 + int(match[2]) * 1000 + int(match[3])


def main(args):
    d = Device(args.serial, args.evidence)
    assert d.screen().size == (1440, 3120), "Use the portrait s26ultra AVD"
    assert "560" in d.shell("wm", "density"), "Verified coordinates require density 560"
    work = Path(args.evidence)
    work.mkdir(parents=True, exist_ok=True)
    name = f"Captions-Smoke-{int(time.time())}.mp4"
    success = False
    try:
        run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", "color=c=0x1d2b53:size=960x540:rate=30:duration=11",
            "-i", SAMPLE, "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", work / name)
        d.shell("mkdir", "-p", FOLDER)
        d.adb("push", "-q", str(work / name), f"{FOLDER}/{name}")
        d.shell("am", "broadcast", "-a", "android.intent.action.MEDIA_SCANNER_SCAN_FILE", "-d", f"file://{FOLDER}/{name}")
        time.sleep(1)
        before = set(d.shell("ls", "/sdcard/Movies/Luma/").split())
        d.shell("am", "force-stop", "app.luma.gallery")
        d.shell("am", "start", "-a", "android.intent.action.VIEW", "-d", f"file://{FOLDER}/{name}",
                "-t", "video/mp4", "-n", "app.luma.gallery/.GalleryActivity")
        d.wait_text("Export clip")
        d.tap_icon("Pause", (999, 2868))
        d.tap_icon("Edit video", (434, 2714))
        d.wait_text("Edit video")

        d.tap_icon("Text", (550, 2878))
        d.tap_icon("Captions", (381, 2696))
        d.wait_text("Spoken language")
        d.screen("captions-panel")
        d.tap_text("ranscribe", (660, 1880))
        deadline = time.monotonic() + args.timeout
        count = None
        while time.monotonic() < deadline and count is None:
            time.sleep(2)
            text = " ".join(row[0] for row in d.lines(d.screen().crop((0, 1950, 1440, 2900))))
            assert "failed" not in text.lower() and "could not" not in text.lower(), text
            found = re.search(r"(\d+) words", text)
            count = found and int(found[1])
        assert count, "No transcript within the timeout"
        shot = d.screen("transcribed")
        transcript = " ".join(words(" ".join(row[0] for row in d.lines(shot.crop((0, 2380, 1440, 2800))))))
        assert TRANSCRIPT[:40] in transcript, transcript
        d.checks.append(f"transcribed-{count}-words")

        d.tap_text("One word", (452, 2114))
        time.sleep(0.5)
        d.tap_text("Add captions", (332, 2358))
        time.sleep(1)
        d.screen("captions-added")
        d.tap_icon("Done", (1300, 1640))
        time.sleep(0.8)
        d.tap_icon("Play", (999, 2497))
        time.sleep(3.4)
        d.tap_icon("Pause", (999, 2497))
        time.sleep(1.2)
        at = playhead(d)
        stage = d.screen("stage-caption")
        left, top, right, bottom = d.bounds("Video frame", STAGE)
        y = top + round(0.78 * (bottom - top))
        shown = read_white(stage.crop((left, y - 90, right, y + 90)))
        assert len(shown.split()) == 1 and shown in TRANSCRIPT.split(), f"One word on the stage at {at} ms: {shown!r}"
        d.checks.append(f"stage-word-{shown}-at-{at}")

        d.adb("shell", "input", "swipe", str((left + right) // 2), str(y), str((left + right) // 2), str(y - 470), "700")
        time.sleep(1.0)
        d.tap(left + 200, bottom - 120)  # Empty stage deselects.
        time.sleep(0.8)
        moved = d.screen("dragged")
        assert read_white(moved.crop((left, y - 90 - 470, right, y + 90 - 470))) == shown, "Caption follows the drag"
        assert read_white(moved.crop((left, y - 90, right, y + 90))) == "", "Nothing left at the old place"
        d.checks.append("drag-moves-captions")

        d.tap(1229, 229)  # Export header button.
        d.tap_text("Export video", (384, 2220))
        deadline = time.monotonic() + 150
        exported = None
        while time.monotonic() < deadline and not exported:
            fresh = set(d.shell("ls", "/sdcard/Movies/Luma/").split()) - before
            exported = next((f for f in fresh if f.startswith(name[:-4] + "_edit_")), None)
            time.sleep(1)
        assert exported, "No export appeared in Movies/Luma"
        time.sleep(1.5)
        local = work / exported
        d.adb("pull", "-q", f"/sdcard/Movies/Luma/{exported}", str(local))
        duration = float(run("ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", local))
        assert abs(duration - 11) < 0.2, duration
        for seconds, word in EXPECTED:
            image = frame(local, seconds)
            image.save(work / f"export-{seconds}.png")
            upper = read_white(image.crop((0, 0, 960, 220)))
            lower = read_white(image.crop((0, 330, 960, 540)))
            assert upper == (word or ""), f"{seconds} s shows {upper!r}, expected {word!r}"
            assert lower == "", f"{seconds} s still has a caption at the old place: {lower!r}"
        d.checks.append("export-words-on-time-at-the-top")
        assert d.shell("ls", f"{FOLDER}/{name}").strip(), "Original missing"
        d.checks.append("original-kept")
        success = True
    finally:
        (work / "results.json").write_text(json.dumps({"success": success, "checks": d.checks}, indent=2) + "\n")
    print(json.dumps({"success": success, "checks": d.checks}, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serial", default="emulator-5554")
    parser.add_argument("--evidence", type=Path, default=Path("evidence/captions-smoke"))
    parser.add_argument("--timeout", type=int, default=300, help="Seconds allowed for the model download and recognition")
    main(parser.parse_args())
