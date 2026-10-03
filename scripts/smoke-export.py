#!/usr/bin/env python3
"""Export regression for C0038.MP4 in the documented six-item fixture album.

Creates two new clips in Movies/Luma and pulls them into the evidence directory.
Uses the ARTEMIS/ADB-explored portrait layout and the OCR-first Device helper.
Originals and existing exports are never deleted or modified.
"""
import argparse
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
        device.shell("am", "start", "-n", "app.luma.gallery/.GalleryActivity")
        device.wait_text("Luma-Tests")
        device.tap_text("Luma-Tests", (350, 1170))
        device.wait_text("6 items")
        device.tap_text("00:14", (720, 1150))
        device.wait_text("C0038")
        device.tap(1340, 230)  # File information.
        device.tap_text("Build playback proxy", (360, 1705))
        device.wait_text("Playback proxy", timeout=180, exact=True)
        device.tap(435, 2714)  # Pause the newly opened proxy.
        device.tap(112, 2870)  # Trim controls.
        device.wait_text("In")
        # Verified full-width rail at 1x: selects 4.007 to 7.402 seconds.
        for origin, target in [(83, 450), (1363, 760)]:
            device.motion("DOWN", origin, 2367)
            device.motion("MOVE", target, 2367)
            device.motion("UP", target, 2367)
        device.wait_text("00:04.007")
        device.wait_text("00:07.402")
        device.screen("selection")
        for mode in ("fast", "exact"):
            before = exported_names(device)
            device.tap_text("Export clip", (1160, 2870))
            if mode == "fast":
                device.tap_text("Fast / original quality", (325, 1325))
                device.wait_text("00:03.570")
            else:
                device.tap_text("Exact cut", (745, 1325))
                device.wait_text("00:04.007")
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
                       str(output), "--duration", "3.395"]
            if mode == "fast":
                command += ["--fast", "--start", "4.007"]
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
    parser.add_argument("--evidence", default="evidence/export")
    run(parser.parse_args())
