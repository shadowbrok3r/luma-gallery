#!/usr/bin/env python3
"""Toolbar regression explored through ARTEMIS/ADB on the s26ultra AVD.

Uses the synthetic Photo-Grid fixture from smoke-editing.py. Does not submit Qwen
jobs or modify media. Requires Pillow, Tesseract and adb. Labels use OCR; custom
icons use positions relative to the detected toolbar border. Pixel bounds verify
that the focused text stays above the toolbar. Rotation settings are restored.
"""
import argparse
import io
import json
import re
import runpy
import subprocess
import time
from pathlib import Path
from PIL import ImageChops

editing = runpy.run_path(str(Path(__file__).with_name("smoke-editing.py")))
Device = editing["Device"]


def pink_boxes(image):
    """Connected outlines in Luma's observed accent colour, independent of position."""
    red, green, blue = image.split()
    mask = ImageChops.multiply(red.point(lambda x: 255 if x > 90 else 0),
        ImageChops.subtract(red, green).point(lambda x: 255 if x > 45 else 0))
    mask = ImageChops.multiply(mask, blue.point(lambda x: 255 if x > 45 else 0))
    mask = ImageChops.multiply(mask,
        ImageChops.subtract(red, blue).point(lambda x: 255 if x > 25 else 0))
    pixels = {i for i, value in enumerate(mask.tobytes()) if value}
    w, h = image.size
    boxes = []
    while pixels:
        start = pixels.pop()
        pending = [start]
        left = right = start % w
        top = bottom = start // w
        count = 0
        while pending:
            i = pending.pop()
            x, y = i % w, i // w
            left, right = min(left, x), max(right, x)
            top, bottom = min(top, y), max(bottom, y)
            count += 1
            for j in (i - w, i + w, i - 1 if x else -1, i + 1 if x + 1 < w else -1):
                if j in pixels:
                    pixels.remove(j)
                    pending.append(j)
        if count > 100:
            boxes.append((left, top, right + 1, bottom + 1))
    return boxes


def toolbar(image, boxes=None):
    short = min(image.size)
    matches = [b for b in (boxes if boxes is not None else pink_boxes(image))
        if .35 * short < b[2] - b[0] < .65 * short
        and 3 < (b[2] - b[0]) / (b[3] - b[1]) < 5]
    assert matches, "Text toolbar is not visible"
    return max(matches, key=lambda b: b[3])


def ocr(image):
    buffer = io.BytesIO()
    image.save(buffer, format="PNG")
    return subprocess.run(["tesseract", "stdin", "stdout", "--psm", "6"],
        input=buffer.getvalue(), capture_output=True, check=True, timeout=15).stdout.decode()


def normalized(s):
    return re.sub(r"[^a-z0-9]", "", s.lower())


def field_state(d):
    image = d.screen()
    boxes = pink_boxes(image)
    bar = toolbar(image, boxes)
    fields = [b for b in boxes if b[2] - b[0] > min(image.size) * .85 and b[1] < bar[1]]
    assert fields, "Focused prompt outline is not visible"
    field = max(fields, key=lambda b: b[3])
    assert field[3] < bar[1], f"Text field {field} overlaps toolbar {bar}"
    return image, field, bar


def wait_prompt(d, text, name=None, timeout=15):
    deadline = time.monotonic() + timeout
    last = ""
    while time.monotonic() < deadline:
        try:
            image, field, bar = field_state(d)
            last = ocr(image.crop((field[0] + 5, field[1] + 3, field[2] - 5, field[3] - 3)))
            if normalized(text) in normalized(last):
                if name:
                    image.save(d.evidence / (name + ".png"))
                return {"field": field, "toolbar": bar, "visible_text": last.strip()}
        except AssertionError as error:
            last = str(error)
        time.sleep(.2)
    d.screen("failure")
    raise AssertionError(f"Prompt did not show {text!r}: {last}")


def action(d, index):
    image = d.screen()
    box = toolbar(image)
    # Observed four equally spaced buttons. Prefer OCR for the only text label.
    if index == 3:
        try:
            hits = [(x, y) for text, x, y in d.lines(image.crop(box)) if text.strip() == "Aa"]
            assert hits
            x, y = hits[0][0] + box[0], hits[0][1] + box[1]
        except AssertionError:
            x, y = box[0] + (index + .5) * (box[2] - box[0]) / 4, (box[1] + box[3]) / 2
    else:
        x, y = box[0] + (index + .5) * (box[2] - box[0]) / 4, (box[1] + box[3]) / 2
    d.foreground()
    d.shell("input", "tap", round(x), round(y))
    time.sleep(.25)  # Release plus the next frame's queued clipboard event.
    editing["ime_state"](d, True)


def rotate(d, value):
    d.shell("settings", "put", "system", "user_rotation", value)
    deadline = time.monotonic() + 8
    while time.monotonic() < deadline:
        image = d.screen()
        if min(image.size) > 1 and (image.width > image.height) == (value == 1):
            time.sleep(.5)  # Wait for the Android rotation animation and new insets.
            return
        time.sleep(.1)
    raise AssertionError("Rotation did not complete")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serial", required=True)
    parser.add_argument("--evidence", default="evidence/text-menu-0.1.9/regression")
    args = parser.parse_args()
    d = Device(args.serial, Path(args.evidence))
    settings = {name: d.shell("settings", "get", "system", name)
        for name in ("user_rotation", "accelerometer_rotation")}
    checks = []
    bounds = {}
    try:
        d.shell("settings", "put", "system", "accelerometer_rotation", 0)
        rotate(d, 0)
        editing["open_fixture"](d, "Photo-Grid", photo=True)
        d.tap_text("Qwen edit", (570, 1986))
        d.tap_text("Paint an area", (489, 2405))
        editing["ime_state"](d, True)
        d.shell("input", "text", "menu%soverlap%stest")
        for n in range(2, 11):
            d.shell("input", "keyevent", 66)
            d.shell("input", "text", f"prompt_line_{n}")
        bounds["portrait"] = wait_prompt(d, "prompt_line_10", "long-prompt")
        checks.append("ten-line-prompt-above-menu")
        action(d, 3); action(d, 1)  # Select all, Copy.
        d.shell("input", "text", "replacement_text")
        wait_prompt(d, "replacement_text", "copy-replaced")
        action(d, 3); action(d, 0)
        wait_prompt(d, "prompt_line_10", "copy-pasted")
        checks.append("select-all-copy-paste-retains-focus")
        action(d, 3); action(d, 2)
        wait_prompt(d, "Paint an area", "cut-empty")
        action(d, 0)
        wait_prompt(d, "prompt_line_10", "cut-pasted")
        checks.append("cut-paste-retains-focus")
        for n in range(11, 26):
            d.shell("input", "keyevent", 66)
            d.shell("input", "text", f"prompt_line_{n}")
        bounds["scrolled"] = wait_prompt(d, "prompt_line_25", "scrolled-caret")
        checks.append("long-prompt-scrolls-caret-above-menu")
        d.shell("input", "keyevent", 4)
        editing["ime_state"](d, False)
        time.sleep(.4)
        try:
            toolbar(d.screen())
        except AssertionError:
            pass
        else:
            raise AssertionError("Toolbar survived keyboard dismissal")
        checks.append("back-dismisses-toolbar")
        rotate(d, 1)
        # Long prompt is visible in the right-hand editor column after rotation.
        image = d.screen()
        try:
            x, y = d.wait_text("prompt_line_", timeout=3)
        except AssertionError:
            # Landscape prompt rectangle verified during ARTEMIS exploration.
            x, y = round(image.width * 2060 / 3120), round(image.height * 900 / 1440)
        d.foreground()
        d.shell("input", "tap", x, y)
        editing["ime_state"](d, True)
        d.shell("input", "keycombination", 113, 123)  # Ctrl+End, discovered on device.
        d.shell("input", "text", "landscape_marker")
        bounds["landscape"] = wait_prompt(d, "landscape_marker", "landscape-caret")
        checks.append("landscape-caret-above-menu")
        d.shell("input", "keyevent", 4)
        editing["ime_state"](d, False)
        rotate(d, 0)
        # Album search exercises single-line Enter without touching any Qwen credentials.
        d.shell("am", "force-stop", "app.luma.gallery")
        d.shell("am", "start", "-n", "app.luma.gallery/.GalleryActivity")
        d.wait_text("Luma Gallery")
        d.tap_text("Luma-Editor", (337, 944)); d.wait_text("7 items")
        d.tap(1326, 389)
        d.tap_text("Search files or albums", (400, 535))
        editing["ime_state"](d, True)
        d.shell("input", "text", "Photo-Grid")
        d.wait_text("1 items")
        d.shell("input", "keyevent", 66)
        editing["ime_state"](d, False)
        d.screen("single-line-enter-dismissed")
        checks.append("single-line-enter-dismisses-keyboard")
        (d.evidence / "results.json").write_text(json.dumps({"success": True,
            "device": args.serial, "checks": checks, "bounds": bounds}, indent=2) + "\n")
        print(json.dumps({"success": True, "checks": checks}, indent=2))
    finally:
        d.shell("input", "keyevent", 4)
        for name, value in settings.items():
            d.shell("settings", "put", "system", name, value)


if __name__ == "__main__":
    main()
