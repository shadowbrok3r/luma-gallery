#!/usr/bin/env python3
"""Crop-release regression explored with ARTEMIS and ADB on the s26ultra AVD.

Requires the dependencies of smoke-editing.py. Uses its synthetic fixtures and
OCR-first controls; locates crop borders by their rendered color. Saves copies,
checks their pixels/frames, and verifies originals remain unchanged. No AI calls.
"""
import argparse
import hashlib
import io
import json
import runpy
import time
from pathlib import Path

from PIL import Image, ImageChops, ImageStat

editing = runpy.run_path(str(Path(__file__).with_name("smoke-editing.py")))
Device = editing["Device"]
run = editing["run"]


class PortraitDevice(Device):
    def screen(self, name=None):
        deadline = time.monotonic() + 10
        while True:
            shot = super().screen()
            if 100 < shot.width < shot.height:
                if name:
                    shot.save(self.evidence / (name + ".png"))
                return shot
            assert time.monotonic() < deadline, ("Portrait screenshot not ready", shot.size)
            time.sleep(.1)


def bounds(d, name=None):
    shot = d.screen(name)
    w, h = shot.size
    assert w < h, "This regression uses the explored portrait layout"
    # The explored canvas is above the timeline and controls on both editors.
    top, bottom = round(h * .09), round(h * .55)
    region = shot.crop((0, top, w, bottom))
    diff = ImageChops.difference(region, Image.new("RGB", region.size, (43, 226, 214)))
    r, g, b = diff.split()
    mask = ImageChops.lighter(ImageChops.lighter(r, g), b).point(lambda v: 255 if v < 4 else 0)
    raw = mask.tobytes()
    columns = [sum(raw[x::w]) // 255 for x in range(w)]
    rows = [sum(raw[y*w:(y+1)*w]) // 255 for y in range(bottom-top)]
    assert max(columns) > 80 and max(rows) > 80, "No crop rectangle visible"
    # Long border lines exclude the handles' overhang and image-color noise.
    xs = [i for i, n in enumerate(columns) if n > max(columns) * .5]
    ys = [i + top for i, n in enumerate(rows) if n > max(rows) * .5]
    return min(xs), min(ys), max(xs), max(ys)


def distance(a, b):
    return max(abs(x-y) for x, y in zip(a, b))


def gesture(d, name, start, end):
    before = bounds(d)
    d.foreground()
    def motion(action, point):
        d.shell("input", "motionevent", action, *map(round, point))
    try:
        motion("DOWN", start)
        motion("MOVE", tuple((a+b)/2 for a, b in zip(start, end)))
        motion("MOVE", end)
        deadline = time.monotonic() + 10
        while True:
            held = bounds(d)
            if distance(held, before) > 30:
                break
            assert time.monotonic() < deadline, "Crop did not respond to drag"
            time.sleep(.1)
        d.screen(name + "-held")
    finally:
        motion("UP", end)
    # Include multiple idle frames: the regression happened on finger release.
    deadline = time.monotonic() + 1
    while time.monotonic() < deadline:
        released = bounds(d)
        assert distance(released, held) <= 2, (name, "crop reset on release", held, released)
        time.sleep(.1)
    d.screen(name + "-released")
    d.checks.append(name + "-persists-after-release")
    return released


def move_to_start(d, name, horizontal=True):
    left, top, right, bottom = bounds(d)
    center = ((left+right)/2, (top+bottom)/2)
    end = (left+20, center[1]) if horizontal else (center[0], top+20)
    moved = gesture(d, name, center, end)
    assert abs((moved[2]-moved[0])-(right-left)) <= 2
    assert abs((moved[3]-moved[1])-(bottom-top)) <= 2
    return moved


def shrink(d, name):
    left, top, right, bottom = bounds(d)
    resized = gesture(d, name, (right-2, bottom-2), ((left+right)/2, (top+bottom)/2))
    assert abs(resized[0]-left) <= 2 and abs(resized[1]-top) <= 2
    assert .45 < (resized[2]-resized[0])/(right-left) < .55
    assert .45 < (resized[3]-resized[1])/(bottom-top) < .55
    return resized


def photo_checks(d, work):
    editing["open_fixture"](d, "Photo-Grid", photo=True)
    d.tap_text("1:1", (257, 2100))
    move_to_start(d, "photo-square-left")
    output = Image.open(editing["save_photo"](
        d, "Photo-Grid_edit_", work / "photo-left.png")).convert("RGB")
    source = Image.open(work / "fixtures/Photo-Grid.png").convert("RGB")
    assert output.size == (600, 600)
    assert ImageChops.difference(output, source.crop((0, 0, 600, 600))).getbbox() is None
    d.checks.append("photo-moved-square-export-pixel-exact")

    resized = shrink(d, "photo-smaller")
    # PNG is still selected from the first export; avoid toggling it again.
    output = Image.open(editing["save_photo"](
        d, "Photo-Grid_edit_", work / "photo-small.png", ai=True)).convert("RGB")
    w, h = output.size
    assert 295 <= w <= 305 and 295 <= h <= 305, output.size
    assert ImageChops.difference(output, source.crop((0, 0, w, h))).getbbox() is None
    d.checks.append("photo-resized-export-pixel-exact")

    d.tap_text("Adjust", (315, 1986)); d.wait_text("Exposure")
    d.tap_text("Crop", (110, 1986)); d.wait_text("Drag a corner")
    deadline = time.monotonic() + 10
    while distance(bounds(d), resized) > 2:
        assert time.monotonic() < deadline, "Switching tabs changed the crop"
        time.sleep(.1)
    d.checks.append("photo-crop-survives-tab-switch")


def video_checks(d, work, name):
    editing["open_fixture"](d, name)
    d.tap(111, 2865); d.tap(267, 2865)  # Explored Trim and Crop SVG controls.
    d.wait_text("Keep audio in clip")
    editing["numeric"](d, 300, 2440, 2)
    editing["numeric"](d, 700, 2440, 6)
    d.tap_text("1:1", (257, 2610))
    move_to_start(d, name + "-square-moved", horizontal=name == "Clip-Beats")
    shrink(d, name + "-smaller")
    before = editing["names"](d, "/sdcard/Movies/Luma", name + "_clip_")
    d.tap_text("Export clip", (1200, 2820)); d.wait_text("Save clip")
    d.tap_text("Save clip", (522, 1941))
    path = editing["collect"](d, "/sdcard/Movies/Luma", name + "_clip_", before, work / (name + ".mp4"))
    probe = json.loads(run("ffprobe", "-v", "error", "-show_streams", "-show_format", "-of", "json", path))
    video = next(s for s in probe["streams"] if s["codec_type"] == "video")
    w, h = video["width"], video["height"]
    assert 176 <= w <= 184 and 176 <= h <= 184, (w, h)
    assert not any(s.get("rotation", 0) for s in video.get("side_data_list", []))
    assert abs(float(probe["format"]["duration"]) - 4) < .16
    assert any(s["codec_type"] == "audio" for s in probe["streams"])
    ext = ".mp4" if name == "Clip-Beats" else ".mov"
    expected = Image.open(io.BytesIO(run("ffmpeg", "-v", "error", "-ss", "2", "-i",
        work / "fixtures" / (name + ext), "-vf", f"crop={w}:{h}:0:0", "-frames:v", "1",
        "-f", "image2pipe", "-c:v", "png", "-"))).convert("RGB")
    actual = Image.open(io.BytesIO(run("ffmpeg", "-v", "error", "-i", path, "-frames:v", "1",
        "-f", "image2pipe", "-c:v", "png", "-"))).convert("RGB")
    assert max(ImageStat.Stat(ImageChops.difference(expected, actual)).rms) < 12
    d.checks.append(name + "-moved-resized-export-matches-decoded-crop")


def main(args):
    d = PortraitDevice(args.serial, args.evidence)
    work = Path(args.evidence)
    rotation = d.shell("settings", "get", "system", "user_rotation")
    auto = d.shell("settings", "get", "system", "accelerometer_rotation")
    complete = False
    try:
        d.shell("settings", "put", "system", "accelerometer_rotation", 0)
        d.shell("settings", "put", "system", "user_rotation", 0)
        editing["fixtures"](d, work / "fixtures")
        original_hashes = {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                           for p in (work / "fixtures").iterdir()}
        photo_checks(d, work)
        for name in ["Clip-Beats", "Clip-Rotated"]:
            video_checks(d, work, name)
        for name, digest in original_hashes.items():
            assert d.shell("sha256sum", "/sdcard/DCIM/Luma-Editor/" + name).split()[0] == digest
        d.checks.append("all-seven-originals-unchanged")
        complete = True
    finally:
        d.shell("settings", "put", "system", "user_rotation", rotation)
        d.shell("settings", "put", "system", "accelerometer_rotation", auto)
        result = {"success": complete, "checks": d.checks}
        (work / "results.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serial", required=True)
    parser.add_argument("--evidence", default="evidence/crop-fix-0.1.6/after")
    main(parser.parse_args())
