#!/usr/bin/env python3
"""Library management regression: selection, move, rename, copy, trash, undo, album rename.

Interactions were explored with ADB on emulator-5554 (1440 x 3120, density 560).
OCR reads egui text; SVG controls use verified coordinates scaled from 1440 x 3120;
Android's MediaStore consent dialogs are answered through UIAutomator.
The script only touches the Luma-Manage-* fixture folders it creates and removes them
afterwards. Requires Pillow, tesseract, adb and the installed APK with media access.
"""
import argparse
import io
import json
import re
import runpy
import subprocess
import time
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont, ImageOps

shared = runpy.run_path(str(Path(__file__).with_name("smoke-android.py")))
Device, normalized = shared["Device"], shared["normalized"]

SOURCE = "DCIM/Luma-Manage-Smoke"
TARGET = "Pictures/Luma-Manage-Target"
COPIES = "Pictures/Luma-Manage-Copies"
RENAMED = "Pictures/Luma-Manage-Copied"
FOLDERS = [SOURCE, TARGET, COPIES, RENAMED]
TILES = [(264, 708), (720, 708), (1176, 708), (264, 1162)]  # First album-grid cells.
BAR = 2850  # Action bar row; six media actions or three album actions.
MEDIA_ACTIONS = {"move": 607, "copy": 833, "rename": 1059, "delete": 1285}
ALBUM_ACTIONS = {"rename": 268}


def rows(d, where):
    output = d.shell("content", "query", "--uri", "content://media/external/file", "--projection",
                     "_display_name:relative_path:owner_package_name", "--where", f'"{where}"')
    return [dict(re.findall(r"(\w+)=([^,]*)", line)) for line in output.splitlines() if line.startswith("Row")]


def files(d, folder):
    names = d.shell("ls", "-a", f"/sdcard/{folder}", "2>/dev/null", "|| true").split()
    return sorted(name for name in names if name not in (".", ".."))


def trash_count(d):
    """OCR of the count label left of the Empty button."""
    shot = d.screen()
    label = shot.crop((0, round(shot.height * .14), round(shot.width * .13), round(shot.height * .175)))
    return normalized(" ".join(text for text, _, _ in d.lines(label)))


def scan(d):
    d.shell("content", "call", "--uri", "content://media", "--method", "scan_volume", "--arg", "external_primary")


def fixtures(d, work):
    font = ImageFont.load_default(size=320)
    for folder in FOLDERS:
        d.shell("rm", "-rf", f"/sdcard/{folder}")
    d.shell("mkdir", "-p", f"/sdcard/{SOURCE}", f"/sdcard/{TARGET}")
    for index, letter in enumerate("ABCDEFT"):
        image = Image.new("RGB", (1200, 900), (40 + index * 30, 90, 200 - index * 20))
        ImageDraw.Draw(image).text((600, 450), letter, fill="white", font=font, anchor="mm")
        path = work / f"Smoke-{letter}.jpg"
        image.save(path, quality=90)
        folder = TARGET if letter == "T" else SOURCE
        d.adb("push", "-q", str(path), f"/sdcard/{folder}/")
        # Distinct times keep A..F in newest-first grid order.
        d.shell("touch", "-m", "-t", f"20260901000{9 - index}", f"/sdcard/{folder}/Smoke-{letter}.jpg")
    scan(d)
    deadline = time.monotonic() + 30
    while len(rows(d, "relative_path LIKE 'DCIM/Luma-Manage-Smoke%'")) < 6:
        assert time.monotonic() < deadline, "MediaStore did not index the fixtures"
        time.sleep(1)


def wait_band(d, wanted, top, bottom, timeout=10):
    """Inverted OCR of a horizontal band; framed egui text defeats whole-screen OCR."""
    print(f"Read: {wanted}", flush=True)
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        shot = d.screen().convert("L")
        area = shot.crop((0, round(shot.height * top), shot.width, round(shot.height * bottom)))
        png = io.BytesIO()
        ImageOps.invert(area).point(lambda p: 0 if p < 128 else 255).save(png, format="PNG")
        text = subprocess.run(["tesseract", "stdin", "stdout", "--psm", "6"], input=png.getvalue(),
                              capture_output=True, check=True, timeout=20).stdout.decode()
        if normalized(wanted) in normalized(text):
            return
        time.sleep(0.3)
    d.screen("failure")
    raise AssertionError(f"Did not read {wanted!r} within {timeout}s")


def wait_toast(d, wanted):
    wait_band(d, wanted, .78, .95, timeout=8)


def wait_dialog(d, wanted):
    wait_band(d, wanted, .03, .62)


def long_press(d, x, y):
    d.foreground()
    width, height = d.screen().size
    point = [round(x * width / 1440), round(y * height / 3120)]
    d.shell("input", "swipe", *point, *point, 900)
    time.sleep(0.8)


def drag_select(d, start, end):
    d.motion("DOWN", *start)
    time.sleep(0.9)  # Longer than the 0.5 s long-press threshold.
    d.motion("MOVE", (start[0] + end[0]) // 2, (start[1] + end[1]) // 2)
    d.motion("MOVE", *end)
    time.sleep(0.4)
    d.motion("UP", *end)
    time.sleep(0.6)


def allow(d, timeout=15):
    """Answers the MediaStore consent dialog; Luma pauses its change until then."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        d.shell("uiautomator", "dump", "/sdcard/luma-ui.xml")
        xml = d.shell("cat", "/sdcard/luma-ui.xml")
        match = re.search(r'text="Allow"[^>]*bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"', xml)
        if match:
            x1, y1, x2, y2 = map(int, match.groups())
            d.shell("input", "tap", (x1 + x2) // 2, (y1 + y2) // 2)
            time.sleep(1.5)
            return
        time.sleep(0.5)
    d.screen("failure")
    raise AssertionError("No MediaStore consent dialog appeared")


def type_text(d, text):
    d.shell("input", "text", text)
    time.sleep(0.5)


def tap_found(d, text):
    """Taps OCR-located text; no coordinate fallback, so a miss never touches other media."""
    x, y = d.wait_text(text)
    d.foreground()
    d.shell("input", "tap", x, y)
    time.sleep(0.6)


def open_album(d, name):
    d.tap(190, 384)  # Albums view.
    tap_found(d, name)
    d.wait_text("Luma-Manage")


def run(args):
    d = Device(args.serial, args.evidence)
    work = Path(args.evidence)
    auto = d.shell("settings", "get", "system", "accelerometer_rotation")
    rotation = d.shell("settings", "get", "system", "user_rotation")
    checks = []
    try:
        d.shell("settings", "put", "system", "accelerometer_rotation", 0)
        d.shell("settings", "put", "system", "user_rotation", 0)
        fixtures(d, work)
        d.shell("am", "force-stop", "app.luma.gallery")
        d.shell("am", "start", "-n", "app.luma.gallery/.GalleryActivity")
        d.wait_text("Luma Gallery")
        open_album(d, "Luma-Manage-Smoke")
        d.wait_text("6 items")

        long_press(d, *TILES[0])
        d.wait_text("1 selected")
        drag_select(d, TILES[1], TILES[3])
        d.wait_text("4 selected")
        d.tap(*TILES[0])
        d.wait_text("3 selected")
        d.screen("drag-selection")
        d.tap(111, 228)  # Cancel selection.
        d.wait_text("6 items")
        checks.append("long-press-drag-and-tap-selection")

        long_press(d, *TILES[0])
        d.tap(*TILES[1])
        d.wait_text("2 selected")
        d.tap(MEDIA_ACTIONS["move"], BAR)
        wait_dialog(d, "Move 2 items")
        tap_found(d, "Luma-Manage-Target")
        allow(d)
        wait_toast(d, "Moved 2 items")
        moved = {row["_display_name"] for row in rows(d, f"relative_path = '{TARGET}/'")}
        assert moved == {"Smoke-A.jpg", "Smoke-B.jpg", "Smoke-T.jpg"}, moved
        assert files(d, TARGET) == ["Smoke-A.jpg", "Smoke-B.jpg", "Smoke-T.jpg"], files(d, TARGET)
        checks.append("move-to-album-with-consent")

        long_press(d, *TILES[0])
        d.tap(*TILES[1])
        d.wait_text("2 selected")
        d.tap(MEDIA_ACTIONS["rename"], BAR)
        wait_dialog(d, "Rename 2 items")
        type_text(d, "Smoke")
        wait_dialog(d, "Smoke_001.jpg")
        d.shell("input", "keyevent", 66)
        allow(d)
        wait_toast(d, "Renamed 2 items")
        assert files(d, SOURCE) == ["Smoke-E.jpg", "Smoke-F.jpg", "Smoke_001.jpg", "Smoke_002.jpg"], files(d, SOURCE)
        checks.append("batch-rename-keeps-extensions")

        long_press(d, *TILES[0])
        d.tap(MEDIA_ACTIONS["delete"], BAR)
        allow(d)
        assert any(name.startswith(".trashed-") for name in files(d, SOURCE))
        wait_toast(d, "Undo")
        d.tap(596, 2817)  # Undo in the trash notice.
        allow(d)
        wait_toast(d, "Restored 1 item")
        assert not any(name.startswith(".trashed-") for name in files(d, SOURCE))
        checks.append("trash-and-undo")

        long_press(d, *TILES[0])
        d.tap(MEDIA_ACTIONS["copy"], BAR)
        wait_dialog(d, "Copy 1 item")
        d.tap(550, 429)  # New album field.
        type_text(d, "Luma-Manage-Copies")
        wait_dialog(d, f"Creates {COPIES}")
        d.tap(1200, 429)  # Create.
        wait_toast(d, "Copied 1 item")
        copies = rows(d, f"relative_path = '{COPIES}/'")
        assert len(copies) == 1 and copies[0]["owner_package_name"] == "app.luma.gallery", copies
        checks.append("copy-into-new-album-without-consent")

        long_press(d, *TILES[0])
        d.tap(MEDIA_ACTIONS["delete"], BAR)
        allow(d)
        wait_toast(d, "Moved 1 item")
        d.shell("input", "keyevent", 4)
        d.tap(1191, 384)  # Trash view.
        time.sleep(1)
        # Only the fixture may be deleted forever; other trashed media leaves this step out.
        if not trash_count(d).startswith("1items"):
            print("Skip: the device trash already holds other media", flush=True)
        else:
            long_press(d, 264, 846)  # First trash cell, below the view tabs.
            d.tap(1059, BAR)  # Delete, second of two trash actions.
            wait_dialog(d, "Delete forever")
            d.tap(732, 1674)  # Confirm.
            allow(d)
            wait_toast(d, "Deleted 1 item")
            d.wait_text("Trash is empty")
            assert len(files(d, SOURCE)) == 3, files(d, SOURCE)
            checks.append("trash-view-delete-forever")

        d.tap(190, 384)  # Albums view.
        x, y = d.wait_text("Luma-Manage-Copies")
        long_press(d, x * 1440 // d.screen().width, (y - 150) * 3120 // d.screen().height)
        d.wait_text("1 album")
        d.tap(ALBUM_ACTIONS["rename"], BAR)
        wait_dialog(d, "Rename album")
        d.shell("input", "keyevent", *[67] * 24)
        type_text(d, "Luma-Manage-Copied")
        wait_dialog(d, f"in {RENAMED}")
        d.shell("input", "keyevent", 66)
        allow(d)
        wait_toast(d, "Renamed the album")
        assert len(rows(d, f"relative_path = '{RENAMED}/'")) == 1
        checks.append("rename-album")
        print(json.dumps({"result": "passed", "checks": checks}, indent=2), flush=True)
        (d.evidence / "result.json").write_text(json.dumps(checks, indent=2))
    finally:
        for folder in FOLDERS:
            d.shell("rm", "-rf", f"/sdcard/{folder}")
        d.shell("rm", "-f", "/sdcard/luma-ui.xml")
        scan(d)
        d.shell("settings", "put", "system", "user_rotation", rotation)
        d.shell("settings", "put", "system", "accelerometer_rotation", auto)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--serial", required=True)
    parser.add_argument("--evidence", default="evidence/manage")
    run(parser.parse_args())
