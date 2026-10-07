#!/usr/bin/env python3
"""Export regression for C0038.MP4 in the documented six-item fixture album.

Creates two new clips in Movies/Luma and pulls them into the evidence directory.
Uses the ARTEMIS/ADB-explored portrait layout and the OCR-first Device helper.
Originals and existing exports are never deleted or modified.
"""
import argparse
import re
import runpy
import subprocess
import sys
import time
from pathlib import Path


HERE = Path(__file__).resolve().parent
Device = runpy.run_path(str(HERE / "smoke-android.py"))["Device"]


def exported_names(device):
    try:
        return {name for name in device.shell("ls", "/sdcard/Movies/Luma").splitlines()
                if name.startswith("C0038_clip_") and name.endswith(".mp4")}
    except subprocess.CalledProcessError:
        return set()


def run(args):
    device = Device(args.serial, args.evidence)
    auto = device.shell("settings", "get", "system", "accelerometer_rotation")
    rotation = device.shell("settings", "get", "system", "user_rotation")
    try:
        device.shell("settings", "put", "system", "accelerometer_rotation", 0)
        device.shell("settings", "put", "system", "user_rotation", 0)
        device.shell("am", "force-stop", "app.luma.gallery")
        if args.device_uri:
            device.shell("am", "start", "-a", "android.intent.action.VIEW", "-d", args.device_uri,
                         "-t", "video/mp4", "-n", "app.luma.gallery/.GalleryActivity")
        else:
            device.shell("am", "start", "-n", "app.luma.gallery/.GalleryActivity")
            device.wait_text("Luma-Tests")
            device.tap_text("Luma-Tests", (350, 1170))
            device.wait_text("6 items")
            device.tap_text("00:14", (720, 1150))
        device.wait_text("C0038")
        device.tap(1340, 230)  # File information.
        device.tap_text("Build playback proxy", (360, 1705))
        device.wait_text("Playback proxy", timeout=180)
        device.tap(435, 2714)  # Pause the newly opened proxy.
        device.tap(112, 2870)  # Trim controls.
        device.wait_text("In")
        # The frame lane is above the separate audio lane. A held swipe lets
        # egui observe intermediate motion even when decoding delays a frame.
        for origin, target in [(83, 450), (1363, 760)]:
            device.foreground()
            device.shell("input", "swipe", origin, 2170, target, 2170, 450)
        # Read the selected numeric bounds instead of assuming a gesture ends
        # at the same millisecond on every render cadence.
        deadline = time.monotonic() + 10
        while True:
            shot = device.screen(); w, h = shot.size
            fields = sorted((x, float(match[1])) for text, x, _ in
                            device.lines(shot.crop((0, h * .84, w * .6, h * .9)))
                            if (match := re.search(r"(\d+\.\d{3})", text)))
            if len(fields) == 2:
                start, end = (field[1] for field in fields)
                if 3 < start < 5 and 6 < end < 9:
                    break
            assert time.monotonic() < deadline, "Could not read the selected trim bounds"
            time.sleep(.2)
        device.screen("selection")
        for mode in ("fast", "exact"):
            before = exported_names(device)
            device.tap_text("Export clip", (1160, 2870))
            if mode == "fast":
                device.tap_text("Fast / original quality", (325, 1325))
                device.wait_text("00:03.570")
            else:
                device.tap_text("Exact cut", (745, 1325))
                device.wait_text(f"00:{start:06.3f}")
            device.screen(f"{mode}-dialog")
            device.tap_text("Save clip", (520, 1895))
            deadline = time.monotonic() + 180
            while time.monotonic() < deadline:
                new = exported_names(device) - before
                if new:
                    break
                time.sleep(0.5)
            else:
                device.screen(f"{mode}-failure")
                raise AssertionError(f"No completed {mode} export within 180 seconds")
            assert len(new) == 1, "Another export appeared during the test"
            output = device.evidence / f"{mode}.mp4"
            device.adb("pull", f"/sdcard/Movies/Luma/{new.pop()}", str(output))
            command = [sys.executable, str(HERE / "verify-export.py"), args.original,
                       str(output), "--duration", str(end - start)]
            if mode == "fast":
                command += ["--fast", "--start", str(start)]
            subprocess.run(command, check=True)
            device.screen(f"{mode}-saved")
        print("PASS: nonzero fast/exact cuts from proxy playback retain original 4K resolution", flush=True)
    finally:
        device.shell("settings", "put", "system", "user_rotation", rotation)
        device.shell("settings", "put", "system", "accelerometer_rotation", auto)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--serial", required=True)
    parser.add_argument("--original", required=True, help="Host path to the original C0038.MP4")
    parser.add_argument("--device-uri", help="Open the same recording directly when the six-item album is not installed")
    parser.add_argument("--evidence", default="evidence/export")
    run(parser.parse_args())
