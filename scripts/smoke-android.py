#!/usr/bin/env python3
"""UI regression for the six Luma-Tests fixtures documented in VERIFICATION.md.

Interactions were explored with ARTEMIS and ADB on emulator-5554. Egui does not
expose its labels through UIAutomator; use OCR first, then verified scaled
coordinates for SVG controls. Requires Pillow, tesseract, adb and an installed APK.
Does not clear application data, change permissions, or export/delete media.
"""
import argparse
import csv
import io
import json
import re
import subprocess
import time
from pathlib import Path

from PIL import Image, ImageChops, ImageStat


def normalized(text):
    return re.sub(r"[^a-z0-9]", "", text.lower())


class Device:
    def __init__(self, serial, evidence):
        self.serial = serial
        self.evidence = Path(evidence)
        self.evidence.mkdir(parents=True, exist_ok=True)
        self.checks = []

    def adb(self, *args):
        return subprocess.check_output(["adb", "-s", self.serial, *args], timeout=30)

    def shell(self, *args):
        return self.adb("shell", *map(str, args)).decode().strip()

    def screen(self, name=None):
        image = Image.open(io.BytesIO(self.adb("exec-out", "screencap", "-p"))).convert("RGB")
        if name:
            image.save(self.evidence / f"{name}.png")
        return image

    def lines(self, image):
        png = io.BytesIO()
        image.save(png, format="PNG")
        output = subprocess.run(["tesseract", "stdin", "stdout", "--psm", "11", "-c", "tessedit_create_tsv=1"],
                                input=png.getvalue(), capture_output=True, check=True, timeout=20)
        groups = {}
        for row in csv.DictReader(io.StringIO(output.stdout.decode()), delimiter="\t", quoting=csv.QUOTE_NONE):
            if not row["text"].strip():
                continue
            key = (row["block_num"], row["par_num"], row["line_num"])
            groups.setdefault(key, []).append(row)
        result = []
        for group in groups.values():
            text = " ".join(row["text"] for row in group)
            x = min(int(row["left"]) for row in group)
            y = min(int(row["top"]) for row in group)
            right = max(int(row["left"]) + int(row["width"]) for row in group)
            bottom = max(int(row["top"]) + int(row["height"]) for row in group)
            result.append((text, (x + right) // 2, (y + bottom) // 2))
        return result

    def wait_text(self, wanted, timeout=20, exact=False):
        print(f"Wait: {wanted}", flush=True)
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            shot = self.screen()
            # Dense 1:1 RAW detail confuses whole-screen OCR. Inspect the observed
            # photo footer for its zoom labels while retaining text-based location.
            top = round(shot.height * .8) if shot.width < shot.height and wanted in ("1:1", "100%") else 0
            left = round(shot.width * .63) if top and wanted == "100%" else 0
            right = round(shot.width * .77) if left else shot.width
            region = shot.crop((left, top, right, round(shot.height * .86))) if top else shot
            for text, x, y in self.lines(region):
                matches = (text.strip() == wanted if wanted == "1:1" else
                           normalized(wanted) == normalized(text) if exact else
                           normalized(wanted) in normalized(text))
                if matches:
                    return x + left, y + top
            time.sleep(0.3)
        self.screen("failure")
        raise AssertionError(f"Did not see {wanted!r} within {timeout}s")

    def tap_text(self, text, fallback):
        try:
            x, y = self.wait_text(text, timeout=6)
            self.foreground()
            self.shell("input", "tap", x, y)
        except AssertionError:
            self.tap(*fallback)

    def tap(self, x, y):
        self.foreground()
        width, height = self.screen().size
        assert width < height, "SVG coordinate fallback is verified for portrait only"
        self.shell("input", "tap", round(x * width / 1440), round(y * height / 3120))

    def motion(self, action, x, y):
        self.foreground()
        width, height = self.screen().size
        self.shell("input", "motionevent", action, round(x * width / 1440), round(y * height / 3120))

    def foreground(self):
        resumed = re.findall(r"topResumedActivity=[^\n]*", self.shell("dumpsys", "activity", "activities"))
        assert any("app.luma.gallery/" in line for line in resumed), "Another app took over the emulator"

    def media(self, name):
        shot = self.screen(name)
        w, h = shot.size
        crop = shot.crop((w * 0.2, h * 0.4, w * 0.8, h * 0.62))
        assert max(ImageStat.Stat(crop).stddev) > 12, f"Blank media in {name}"
        self.checks.append(name)
        return crop

    def back_to_album(self):
        self.shell("input", "keyevent", 4)
        self.wait_text("6 items")

    def run(self):
        rotation = self.shell("settings", "get", "system", "user_rotation")
        auto = self.shell("settings", "get", "system", "accelerometer_rotation")
        try:
            self.shell("settings", "put", "system", "accelerometer_rotation", 0)
            self.shell("settings", "put", "system", "user_rotation", 0)
            self.shell("am", "force-stop", "app.luma.gallery")
            self.shell("am", "start", "-n", "app.luma.gallery/.GalleryActivity")
            self.wait_text("Luma Gallery")
            self.wait_text("Luma-Tests")
            self.screen("albums")
            self.tap_text("Luma-Tests", (350, 1170))
            self.wait_text("6 items")
            self.screen("album-grid")

            for name, point, dimensions in [
                ("DSC7596", (260, 670), "6024 x 4024"),
                ("uncompressed", (720, 670), "7028 x 4688"),
                ("lossless", (1180, 670), "7028 x 4688"),
            ]:
                self.tap_text(name, point)
                self.wait_text("Full-resolution RAW", timeout=180)
                self.wait_text(dimensions)
                fit = self.media(f"raw-{name}-fit")
                self.tap_text("1:1", (1180, 2580))
                self.wait_text("100%")
                full = self.media(f"raw-{name}-1to1")
                assert sum(ImageStat.Stat(ImageChops.difference(fit, full)).mean) > 15
                self.back_to_album()

            self.tap_text("Jewelry-4K", (260, 1150))
            self.wait_text("3840 x 2160")
            self.media("jpeg-portrait")
            self.shell("settings", "put", "system", "user_rotation", 1)
            time.sleep(2)  # Android's rotation animation must complete before OCR/pixel checks.
            self.wait_text("Jewelry-4K")
            assert self.screen().width > self.screen().height
            self.media("jpeg-landscape")
            self.shell("settings", "put", "system", "user_rotation", 0)
            time.sleep(2)
            self.back_to_album()

            self.tap_text("00:14", (720, 1150))
            self.wait_text("C0038")
            time.sleep(2)
            first = self.media("video-4k-first")
            time.sleep(2)
            second = self.media("video-4k-moving")
            assert sum(ImageStat.Stat(ImageChops.difference(first, second)).mean) > 2
            self.tap(435, 2714)  # Pause, verified SVG button.
            time.sleep(0.4)
            try:
                self.motion("DOWN", 390, 2520)
                self.motion("MOVE", 830, 2520)
                self.wait_text("1x")
                time.sleep(1.5)
                self.screen("scrub-loupe")
                self.motion("MOVE", 850, 2080)
                self.wait_text("20x")
                self.screen("scrub-loupe-fine")
                self.checks.append("scrub-loupe-1x-and-20x")
            finally:
                self.motion("UP", 850, 2080)
            self.tap(112, 2870)  # Trim mode, verified scissors icon.
            self.wait_text("In")
            self.screen("trim-controls")
            self.checks.append("trim-controls")
            self.shell("settings", "put", "system", "user_rotation", 1)
            time.sleep(2)
            self.wait_text("Export clip")
            self.screen("video-landscape-trim")
            self.checks.append("video-landscape-trim")
            self.shell("settings", "put", "system", "user_rotation", 0)
            time.sleep(2)
            self.back_to_album()
            self.tap_text("00:24", (1180, 1150))
            self.wait_text("C0010")
            time.sleep(2)
            self.media("video-1080p60")
            self.shell("input", "keyevent", 3)
            time.sleep(1)
            self.shell("am", "start", "-n", "app.luma.gallery/.GalleryActivity")
            self.wait_text("C0010")
            time.sleep(1)
            paused = self.media("video-resumed-paused")
            time.sleep(1)
            still = self.media("video-resumed-still")
            assert sum(ImageStat.Stat(ImageChops.difference(paused, still)).mean) < 1
            self.checks.append("background-pauses-video")
            print(json.dumps({"result": "passed", "checks": self.checks}, indent=2), flush=True)
            (self.evidence / "result.json").write_text(json.dumps(self.checks, indent=2))
        finally:
            self.shell("settings", "put", "system", "user_rotation", rotation)
            self.shell("settings", "put", "system", "accelerometer_rotation", auto)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--serial", required=True)
    parser.add_argument("--evidence", default="evidence/smoke")
    args = parser.parse_args()
    Device(args.serial, args.evidence).run()
