#!/usr/bin/env python3
"""Video preview/frame regression explored with ARTEMIS on the s26ultra AVD.

Requires adb, ffmpeg, Pillow, numpy, Tesseract and an installed Luma APK with
media access. Creates synthetic videos in DCIM/Luma-Frames and photos in
Pictures/Luma; never deletes media. Uses OCR for text, pixel location for the
preview, and observed icon/timeline coordinates at 1440x3120, density 560.
Warm VIEW intents deliberately retain the activity between different videos.
"""
import argparse
import hashlib
import io
import json
import math
import re
import runpy
import subprocess
import time
from pathlib import Path

import numpy as np
from PIL import Image, ImageChops, ImageStat

editing = runpy.run_path(str(Path(__file__).with_name("smoke-editing.py")))
Device, run, names, collect = (editing[n] for n in ("Device", "run", "names", "collect"))


def fixtures(d, folder):
    folder.mkdir(parents=True, exist_ok=True)
    a = folder / "Frame-A.mp4"
    run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i",
        "testsrc2=size=960x540:rate=30:duration=20", "-f", "lavfi", "-i",
        "sine=frequency=440:duration=20", "-c:v", "libx264", "-crf", "18",
        "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", a)
    run("ffmpeg", "-v", "error", "-y", "-i", a, "-vf", "hue=h=100", "-an",
        "-c:v", "libvpx-vp9", "-deadline", "realtime", folder / "Frame-B.webm")
    run("ffmpeg", "-v", "error", "-y", "-display_rotation:v:0", "90", "-i", a,
        "-c", "copy", folder / "Frame-Rotated.mov")
    # Luma's small FFmpeg build excludes the TS demuxer. Android and VLC support
    # this container, exercising the metadata fallback with a real playable file.
    run("ffmpeg", "-v", "error", "-y", "-i", a, "-an", "-c:v", "copy",
        "-f", "mpegts", folder / "Frame-Fallback.mp4")
    d.shell("mkdir", "-p", "/sdcard/DCIM/Luma-Frames")
    for path in folder.iterdir():
        d.adb("push", "-q", str(path), "/sdcard/DCIM/Luma-Frames/" + path.name)
    d.shell("content", "call", "--uri", "content://media", "--method", "scan_volume", "--arg", "external_primary")


def open_video(d, name):
    d.shell("am", "start", "-a", "android.intent.action.VIEW", "-d",
            "file:///sdcard/DCIM/Luma-Frames/" + name, "-t", "video/*",
            "-n", "app.luma.gallery/.GalleryActivity")
    d.wait_text(name)
    d.wait_text("Export clip")
    d.tap(433, 2715)  # Pause transport, verified before tests were authored.


def reference(path, milliseconds):
    return Image.open(io.BytesIO(run("ffmpeg", "-v", "error", "-ss", max(0, milliseconds) / 1000,
        "-i", path, "-frames:v", "1", "-f", "image2pipe", "-c:v", "png", "-"))).convert("RGB")


def rms(first, second):
    return max(ImageStat.Stat(ImageChops.difference(first, second)).rms)


def loupe(d):
    shot = d.screen()
    pixels = np.asarray(shot)
    mask = (pixels[:, :, 0] > 210) & (pixels[:, :, 1] < 110) & (pixels[:, :, 2] > 100)
    mask[:round(shot.height * .52)] = False
    mask[round(shot.height * .71):] = False
    yy, xx = np.where(mask)
    assert len(xx) > 100, "No scrub preview border"
    left, right, top, bottom = int(xx.min()), int(xx.max()) + 1, int(yy.min()), int(yy.max()) + 1
    assert right - left > 400 and bottom - top > 300, (left, top, right, bottom)
    image = shot.crop((left + 15, top + 15, right - 15, top + 15 + round((right - left - 30) * 9 / 16)))
    label = shot.crop((left, bottom - 72, right, bottom)).resize(((right - left) * 2, 144))
    text = " ".join(line[0] for line in d.lines(label))
    match = re.search(r"00:(\d{2})[.,](\d{3})", text)
    assert match, "No decoded-frame timestamp: " + text
    return shot, image, int(match[1]) * 1000 + int(match[2])


def await_loupe(d, source, name, minimum_time=None):
    deadline = time.monotonic() + 12
    last = None
    while time.monotonic() < deadline:
        try:
            shot, actual, at = loupe(d)
            if minimum_time is not None:
                assert at > minimum_time, (at, minimum_time)
            frame = round(at * 30 / 1000)
            delta = min(rms(actual, reference(source, math.floor(index * 1000 / 30)).resize(actual.size))
                        for index in range(max(0, frame - 1), frame + 2))
            assert delta < 28, ("Wrong or empty video preview", delta, at)
            shot.save(d.evidence / (name + ".png"))
            return actual, at
        except AssertionError as error:
            last = error
            time.sleep(.2)
    raise AssertionError(str(last))


def preview_checks(d, source, name):
    # Keep a single pointer down while moving, then verify the decoder settles.
    try:
        d.motion("DOWN", 300, 2320)
        d.motion("MOVE", 520, 2320)
        first, first_at = await_loupe(d, source, name + "-early")
        d.motion("MOVE", 1030, 2320)
        second, second_at = await_loupe(d, source, name + "-later", minimum_time=first_at + 5000)
        assert second_at > first_at + 5000, (first_at, second_at)
        assert rms(first.resize((128, 72)), second.resize((128, 72))) > 5, "Preview stayed static"
        d.checks.append(name + "-moving-preview-correct-video-and-time")
    finally:
        d.motion("UP", 1030, 2320)
    deadline = time.monotonic() + 5
    while True:
        try:
            loupe(d)
        except AssertionError:
            break
        assert time.monotonic() < deadline, "Preview did not close after releasing the drag"
        time.sleep(.1)


def capture(d, source, name):
    prefix = source.stem + "_frame_"
    before = names(d, "/sdcard/Pictures/Luma", prefix)
    d.tap(433, 2865)  # Camera SVG beside crop, verified by saving an actual frame.
    output = collect(d, "/sdcard/Pictures/Luma", prefix, before, d.evidence / (name + ".png"))
    added = names(d, "/sdcard/Pictures/Luma", prefix) - before
    assert len(added) == 1
    filename = added.pop()
    at = int(re.search(r"_frame_(\d+)ms_", filename)[1])
    actual = Image.open(output).convert("RGB")
    # OPTION_CLOSEST chooses the nearest presentation frame; FFmpeg seeks to
    # the first frame at/after the requested timestamp. Check neighboring frames.
    duration = json.loads(run("ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "json", source))
    last_frame = math.ceil(float(duration["format"]["duration"]) * 30) - 1
    frame = round(at * 30 / 1000)
    # Floor to milliseconds before the exact rational frame boundary so the
    # reference decoder cannot skip that frame due to decimal rounding.
    candidates = [reference(source, math.floor(index * 1000 / 30))
                  for index in range(max(0, frame - 1), min(last_frame, frame + 1) + 1)]
    assert all(expected.size == actual.size for expected in candidates), (actual.size, candidates[0].size)
    error = min(rms(actual, expected) for expected in candidates)
    assert error < 12, ("Captured wrong frame, orientation, or UI pixels", at, error)
    assert actual.size in ((960, 540), (540, 960)), actual.size
    d.screen(name + "-ui")
    d.checks.append(name + "-full-resolution-png-correct-frame")
    return at


def main(args):
    d = Device(args.serial, args.evidence)
    assert d.screen().size == (1440, 3120), "Use the existing portrait s26ultra AVD"
    assert "560" in d.shell("wm", "density"), "Verified control coordinates require density 560"
    work = Path(args.evidence)
    originals = work / "fixtures"
    success = False
    try:
        fixtures(d, originals)
        hashes = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in originals.iterdir()}
        open_video(d, "Frame-A.mp4")
        # Native helper children briefly inherit the Java process name before exec.
        pid = min(map(int, d.shell("pidof", "app.luma.gallery").split()))
        preview_checks(d, originals / "Frame-A.mp4", "audio")
        at = capture(d, originals / "Frame-A.mp4", "audio-capture")
        d.tap(592, 2715)  # Next-frame transport.
        step = capture(d, originals / "Frame-A.mp4", "stepped-capture")
        assert 0 < step - at <= 34, (at, step)  # The arbitrary scrub time advances to the next frame boundary.
        assert rms(Image.open(work / "audio-capture.png").convert("RGB"),
                   Image.open(work / "stepped-capture.png").convert("RGB")) > 10
        d.checks.append("capture-honors-a-pending-single-frame-step")
        d.tap(433, 2715)  # Resume, then capture during playback.
        time.sleep(.3)
        playing = capture(d, originals / "Frame-A.mp4", "playing-capture")
        assert playing > step
        first = d.screen().crop((45, 800, 1395, 1550))
        time.sleep(.4)
        assert rms(first, d.screen().crop((45, 800, 1395, 1550))) == 0, "Capture did not pause playback"
        d.checks.append("capture-during-playback-pauses-the-video")
        d.tap(1390, 2320)  # End of rail clamps to the duration.
        end = capture(d, originals / "Frame-A.mp4", "end-capture")
        assert end >= 19950, end
        open_video(d, "Frame-B.webm")
        assert pid in map(int, d.shell("pidof", "app.luma.gallery").split()), "Warm switch restarted the app"
        d.wait_text("No audio track")
        preview_checks(d, originals / "Frame-B.webm", "silent-warm-switch")
        capture(d, originals / "Frame-B.webm", "silent-capture")
        # Exercise sustained movement, not just a stationary request.
        swipe = subprocess.Popen(["adb", "-s", d.serial, "shell", "input", "swipe", "250", "2320", "1150", "2320", "8000"])
        try:
            time.sleep(1.5)
            first, at1 = await_loupe(d, originals / "Frame-B.webm", "continuous-early")
            time.sleep(2)
            second, at2 = await_loupe(d, originals / "Frame-B.webm", "continuous-later")
            assert at2 > at1 + 1000 and rms(first.resize((128, 72)), second.resize((128, 72))) > 5
        finally:
            swipe.wait(timeout=15)
        d.checks.append("continuous-drag-decodes-progress-without-starvation")
        try:
            d.motion("DOWN", 520, 2320)
            d.motion("MOVE", 520, 2290)
            _, at = await_loupe(d, originals / "Frame-B.webm", "precision-normal")
            d.motion("MOVE", 520, 1780)
            d.wait_text("20x")
            _, precise_at = await_loupe(d, originals / "Frame-B.webm", "precision-fine")
            assert abs(precise_at - at) <= 1, (at, precise_at)
        finally:
            d.motion("UP", 520, 1780)
        d.checks.append("precision-label-updates-without-a-time-change")
        open_video(d, "Frame-Rotated.mov")
        d.tap(720, 2320)
        capture(d, originals / "Frame-Rotated.mov", "rotated-capture")
        open_video(d, "Frame-Fallback.mp4")
        d.wait_text("No audio track")
        d.tap(1340, 232)  # File information icon.
        d.wait_text("video/avc")  # Android MediaExtractor codec, rather than FFprobe's h264.
        d.screen("android-metadata-fallback")
        text = " ".join(row[0] for row in d.lines(d.screen())).lower()
        assert "ffprobe failed" not in text and "additional video details unavailable" not in text
        d.checks.append("playable-silent-ts-android-metadata-fallback-no-error-toast")
        for name, digest in hashes.items():
            assert d.shell("sha256sum", "/sdcard/DCIM/Luma-Frames/" + name).split()[0] == digest
        d.checks.append("all-original-videos-unchanged")
        success = True
    finally:
        (work / "results.json").write_text(json.dumps({"success": success, "checks": d.checks}, indent=2) + "\n")
    print(json.dumps({"success": success, "checks": d.checks}, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serial", default="emulator-5554")
    parser.add_argument("--evidence", type=Path, required=True)
    main(parser.parse_args())
