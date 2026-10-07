#!/usr/bin/env python3
"""Focus regression explored with ARTEMIS/ADB on the existing s26ultra AVD.

Requires the synthetic DCIM/Luma-Editor/Photo-Grid.png fixture, Pillow, Tesseract,
and adb. Uses OCR for labels and a verified footer-relative SVG fallback. Changes
emulator window size to exercise a tall keyboard; restores display/rotation in
finally. Never submits a Qwen job, edits saved media, or changes the selected IME.
"""
import argparse
import json
import re
import runpy
import time
from pathlib import Path

menu = runpy.run_path(str(Path(__file__).with_name("smoke-text-menu.py")))
Device = menu["Device"]


def shown(d):
    return "mInputShown=true" in d.shell("dumpsys", "input_method")


def stable_ime(d, visible):
    deadline = time.monotonic() + 6
    while shown(d) != visible:
        assert time.monotonic() < deadline, f"Keyboard never became visible={visible}"
        time.sleep(.1)
    # Check beyond the asynchronous inset and native-to-egui focus updates.
    deadline = time.monotonic() + 1.5
    while time.monotonic() < deadline:
        assert shown(d) == visible, f"Keyboard lost stable visible={visible} state"
        time.sleep(.15)


def tap_label(d, label):
    x, y = d.wait_text(label)
    d.foreground()
    d.shell("input", "tap", x, y)


def open_prompt(d, size):
    d.shell("am", "force-stop", "app.luma.gallery")
    d.shell("wm", "size", size)
    d.shell("am", "start", "-a", "android.intent.action.VIEW",
            "-d", "file:///sdcard/DCIM/Luma-Editor/Photo-Grid.png",
            "-t", "image/png", "-n", "app.luma.gallery/.GalleryActivity")
    _, y = d.wait_text("Full resolution")
    # Pencil is an SVG beside the photo footer, 14 dp below its status label.
    # Coordinates verified in both 1440x3120 and 1440x2200 windows at 560 dpi.
    w = d.screen().width
    d.foreground()
    d.shell("input", "tap", round(w * 1340 / 1440), y + 49)
    d.wait_text("Edit photo")
    tap_label(d, "Qwen edit")
    d.screen(f"{size}-before-tap")
    tap_label(d, "Paint an area")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serial", required=True)
    parser.add_argument("--evidence", default="evidence/keyboard-focus-0.1.10/regression")
    args = parser.parse_args()
    d = Device(args.serial, Path(args.evidence))
    assert "s26ultra" in d.adb("emu", "avd", "name").decode().lower(), "Use the selected s26ultra AVD"
    display = {}
    for name in ("size", "density"):
        match = re.search(r"Override \w+: (\S+)", d.shell("wm", name))
        display[name] = match.group(1) if match else "reset"
    settings = {name: d.shell("settings", "get", "system", name)
        for name in ("user_rotation", "accelerometer_rotation")}
    checks = []
    bounds = {}
    try:
        d.shell("settings", "put", "system", "accelerometer_rotation", 0)
        d.shell("settings", "put", "system", "user_rotation", 0)
        d.shell("wm", "density", 560)
        for size in ("1440x2200", "1440x3120"):
            open_prompt(d, size)
            stable_ime(d, True)
            d.shell("input", "text", "focus_marker")
            bounds[size] = menu["wait_prompt"](d, "focus_marker", f"{size}-typed")
            checks.append(f"{size}-opens-and-types-through-reflow")
            for n in range(3):
                d.shell("input", "keyevent", 4)
                stable_ime(d, False)
                tap_label(d, "focus_marker")
                stable_ime(d, True)
                d.shell("input", "keycombination", 113, 123)  # Ctrl+End
                d.shell("input", "text", str(n))
                menu["wait_prompt"](d, "focus_marker", f"{size}-reopen-{n}")
            checks.append(f"{size}-three-dismiss-reopen-cycles")
            if size == "1440x3120":
                for rotation, marker in ((1, "landscape"), (0, "portrait")):
                    menu["rotate"](d, rotation)
                    stable_ime(d, True)  # No retap: the same edit must survive rotation.
                    d.shell("input", "text", marker)
                    bounds[marker] = menu["wait_prompt"](d, marker, f"rotation-{marker}")
                    checks.append(f"rotate-{marker}-keeps-focus-and-typing")
            d.shell("input", "keyevent", 4)
            stable_ime(d, False)
            try:
                menu["toolbar"](d.screen())
            except AssertionError:
                pass
            else:
                raise AssertionError("Toolbar survived keyboard dismissal")
        checks.append("back-dismisses-keyboard-and-toolbar")
        result = {"success": True, "device": args.serial, "checks": checks, "bounds": bounds,
            "keyboard": d.shell("settings", "get", "secure", "default_input_method")}
        (d.evidence / "results.json").write_text(json.dumps(result, indent=2) + "\n")
        print(json.dumps(result, indent=2))
    except Exception:
        d.screen("failure")
        raise
    finally:
        d.shell("input", "keyevent", 4)
        for name, value in display.items():
            d.shell("wm", name, value)
        for name, value in settings.items():
            d.shell("settings", "put", "system", name, value)


if __name__ == "__main__":
    main()
