#!/usr/bin/env python3
"""Cross-app handoffs explored with ARTEMIS/ADB on the existing s26ultra AVD.

Builds a test-only sender with an unexported content provider and synthetic media.
Checks Android's real chooser, cold/warm URI grants without library access, mixed
shares, editing, failed shares and preserving an open editor. Requires the Android
SDK, Java 17, ffmpeg, Pillow and Tesseract. Temporarily revokes Luma's media reads;
restores previously granted reads in finally. Does not clear Luma's app data.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import runpy
import subprocess
import time
import xml.etree.ElementTree as ET
import zipfile

from PIL import Image, ImageChops, ImageOps, ImageStat

ROOT = Path(__file__).resolve().parent.parent
editing = runpy.run_path(str(ROOT / "scripts/smoke-editing.py"))
BaseDevice = editing["Device"]
PACKAGE = "app.luma.gallery"
PROBE = "app.luma.shareprobe"


def run(*args):
    return subprocess.check_output(list(map(str, args)), stderr=subprocess.PIPE, timeout=120)


def build_probe():
    build = ROOT / ".build/share-probe"
    for name in ("assets", "classes", "dex"):
        (build / name).mkdir(parents=True, exist_ok=True)
    assets = build / "assets"
    run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=900x600:rate=1",
        "-frames:v", "1", assets / "Shared-Photo.png")
    ImageOps.mirror(Image.open(assets / "Shared-Photo.png")).save(assets / "Shared-Second.webp", lossless=True)
    run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=640x360:rate=30:duration=12",
        "-f", "lavfi", "-i", r"aevalsrc=if(lt(mod(t\,2)\,0.1)\,0.7*sin(2*PI*440*t)\,0):s=48000:d=12",
        "-c:v", "libx264", "-crf", "20", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", assets / "Shared-Clip.mp4")
    (assets / "Note.txt").write_text("This is not media.\n")
    sdk = Path(os.environ.get("ANDROID_HOME", Path.home() / "Android/Sdk"))
    bt, jar = sdk / "build-tools/35.0.0", sdk / "platforms/android-35/android.jar"
    run("/usr/lib/jvm/java-17-openjdk/bin/javac", "-source", "17", "-target", "17", "-classpath", jar,
        "-d", build / "classes", *sorted((ROOT / "scripts/share-probe").glob("*.java")))
    run(bt / "d8", "--lib", jar, "--min-api", "29", "--output", build / "dex", *sorted((build / "classes").rglob("*.class")))
    apk = build / "probe.apk"
    run(bt / "aapt2", "link", "-I", jar, "--manifest", ROOT / "scripts/share-probe/AndroidManifest.xml", "-A", assets, "-o", apk)
    with zipfile.ZipFile(apk, "a", zipfile.ZIP_DEFLATED) as archive:
        archive.write(build / "dex/classes.dex", "classes.dex")
    run(bt / "apksigner", "sign", "--ks", Path.home() / ".android/debug.keystore", "--ks-pass", "pass:android", apk)
    return apk, assets


class Device(BaseDevice):
    def screen(self, name=None):
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            shot = super().screen()
            if 100 < shot.width < shot.height:
                if name:
                    shot.save(self.evidence / f"{name}.png")
                return shot
            time.sleep(.15)
        raise AssertionError("Portrait rendering did not settle")

    def system_elements(self):
        self.shell("uiautomator", "dump", "/sdcard/luma-share-probe.xml")
        tree = ET.fromstring(self.shell("cat", "/sdcard/luma-share-probe.xml"))
        return list(tree.iter("node"))

    def select_luma(self, view=False):
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            nodes = self.system_elements()
            match = next((n for n in nodes if n.get("text") == "Luma Gallery" and n.get("package") != PACKAGE), None)
            if match is not None:
                self.screen("open-with-chooser" if view else "android-share-sheet")
                x1, y1, x2, y2 = map(int, re.findall(r"\d+", match.get("bounds")))
                self.shell("input", "tap", (x1+x2)//2, (y1+y2)//2)
                break
            time.sleep(.2)
        else:
            # Verified positions for this AVD; only used in the system chooser.
            assert "ResolverActivity" in self.shell("dumpsys", "activity", "activities") or "ChooserActivity" in self.shell("dumpsys", "activity", "activities")
            w, h = self.screen().size
            x, y = (331, 2409) if view else (432, 2836)
            self.shell("input", "tap", round(x*w/1440), round(y*h/3120))
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if re.search(r"topResumedActivity=.*app\.luma\.gallery/", self.shell("dumpsys", "activity", "activities")):
                return
            match = next((n for n in self.system_elements() if n.get("resource-id") == "android:id/button_once"), None)
            if match is not None:
                x1, y1, x2, y2 = map(int, re.findall(r"\d+", match.get("bounds")))
                self.shell("input", "tap", (x1+x2)//2, (y1+y2)//2)
            time.sleep(.2)
        raise AssertionError("Chooser did not hand off to Luma")

    def send(self, mode, direct=True):
        self.shell("am", "start", "-f", "0x10008000", "-n", PROBE + "/.ProbeActivity",
                   "--es", "mode", mode, "--ez", "direct", str(direct).lower())
        if not direct:
            self.select_luma(view=mode == "view")

    def main_pid(self):
        # Native helper forks can briefly share the app's process name. Ask Android
        # for the managed Activity process instead of comparing raw pidof output.
        processes = set(re.findall(r"app=ProcessRecord\{\S+ (\d+):app\.luma\.gallery/",
                                   self.shell("dumpsys", "activity", "activities")))
        assert len(processes) == 1, processes
        return processes.pop()

    def rendered(self, name, evidence):
        self.wait_text(name)
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            shot = self.screen()
            w, h = shot.size
            if max(ImageStat.Stat(shot.crop((w*.2,h*.4,w*.8,h*.6))).stddev) > 12:
                self.foreground()
                self.screen(evidence)
                return
            time.sleep(.2)
        raise AssertionError("Shared media stayed blank: " + name)

    def notice(self, wanted):
        deadline = time.monotonic() + 9
        while time.monotonic() < deadline:
            shot = self.screen()
            # The photo texture overwhelms whole-frame OCR; notices occupy the footer.
            region = shot.crop((0, shot.height*.83, shot.width, shot.height*.948))
            if any(wanted.lower() in text.lower() for text, _, _ in self.lines(region)):
                self.screen("notice-" + wanted.split()[0])
                return
            time.sleep(.2)
        raise AssertionError("Missing notice: " + wanted)


def main(args):
    d = Device(args.serial, args.evidence)
    apk, assets = build_probe()
    d.shell("am", "force-stop", PACKAGE)
    d.adb("install", "--no-incremental", "-r", str(apk))
    before = d.shell("dumpsys", "package", PACKAGE)
    permissions = re.findall(r"(android.permission.READ_MEDIA_\w+): granted=true", before)
    (d.evidence / "permissions-before.txt").write_text("\n".join(permissions))
    rotation = d.shell("settings", "get", "system", "user_rotation")
    auto = d.shell("settings", "get", "system", "accelerometer_rotation")
    try:
        d.shell("settings", "put", "system", "accelerometer_rotation", 0)
        d.shell("settings", "put", "system", "user_rotation", 0)
        for permission in permissions:
            d.shell("pm", "revoke", PACKAGE, permission)
        assert not re.search(r"READ_MEDIA_\w+: granted=true", d.shell("dumpsys", "package", PACKAGE))
        d.shell("am", "force-stop", PACKAGE)
        d.send("view", direct=False)
        d.rendered("Shared-Photo.png", "cold-open-with")
        d.wait_text("900 x 600")
        d.checks.append("cold-open-with-private-uri-without-library-permission")
        pid = d.main_pid()
        d.send("video", direct=False)
        d.rendered("Shared-Clip.mp4", "warm-video-share")
        warm_pid = d.main_pid()
        assert warm_pid == pid, f"Warm share recreated the process: {pid} -> {warm_pid}"
        d.wait_text("00:12")
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            shot = d.screen()
            lane = shot.crop((shot.width*.05, shot.height*.785, shot.width*.94, shot.height*.825))
            if sum(r < 100 and g > 140 and b > 120 for r, g, b in lane.getdata()) > 100:
                break
            time.sleep(.2)
        else:
            raise AssertionError("Shared video audio waveform stayed empty")
        d.screen("warm-video-share-with-waveform")
        d.checks.append("warm-video-share-and-waveform")

        d.send("multiple", direct=False)
        d.rendered("Shared-Photo.png", "mixed-share-first")
        d.shell("input", "swipe", 1200, 1600, 400, 1600, 350)
        d.rendered("Shared-Clip.mp4", "mixed-share-video")
        d.tap(750, 2714)  # Next item icon, beside frame-step controls.
        d.rendered("Shared-Second.webp", "mixed-share-last")
        d.shell("input", "keyevent", 4)
        d.wait_text("Luma Gallery")
        d.checks.append("mixed-multiple-order-and-return-to-library")

        for mode, title in [("clipdata", "Shared-Photo.png"), ("opaque", "Shared without extension")]:
            d.send(mode)
            d.rendered(title, mode)
            d.checks.append(mode)
        d.send("partial")
        d.rendered("Shared-Photo.png", "partial-share")
        d.notice("1 unavailable")
        d.shell("input", "swipe", 1200, 1600, 400, 1600, 350)
        d.rendered("Shared-Second.webp", "partial-share-second")
        d.checks.append("unsupported-attachment-skipped")
        d.send("missing")
        d.notice("Couldn't open this share")
        d.wait_text("Shared-Second.webp")
        d.checks.append("unreadable-share-preserves-current-view")

        d.send("photo")
        d.rendered("Shared-Photo.png", "before-edit")
        d.tap(1342, 2583)  # Photo pencil, explored on this AVD.
        d.wait_text("Save copy")
        shot = d.screen()
        top = round(shot.height*.63)
        region = shot.crop((0, top, shot.width*.6, shot.height*.71))
        match = next(((x, y+top) for text, x, y in d.lines(region) if text.strip() == "1:1"), None)
        if match:
            d.shell("input", "tap", *match)
        else:
            d.tap(257, 2100)  # Verified square preset; viewer zoom OCR uses another region.
        d.wait_text("600 x 600")
        d.send("video")
        d.wait_text("Open shared media?")
        d.screen("preserve-edit-confirmation")
        d.tap_text("Keep editing", (482, 1600))
        d.wait_text("Save copy")
        d.wait_text("Shared-Photo")
        exported = editing["save_photo"](d, "Shared-Photo", d.evidence / "shared-crop.png")
        actual = Image.open(exported).convert("RGB")
        expected = Image.open(assets / "Shared-Photo.png").convert("RGB").crop((150, 0, 750, 600))
        assert actual.size == (600, 600), actual.size
        assert ImageChops.difference(actual, expected).getbbox() is None, "Shared photo crop differs from original pixels"
        d.checks.append("shared-photo-edit-export-and-preserved-unsaved-crop")
        d.send("video")
        d.wait_text("Open shared media?")
        d.tap_text("Open media", (859, 1600))
        d.rendered("Shared-Clip.mp4", "accepted-next-share")
        d.checks.append("accept-new-share-after-editor")
        d.send("slow")
        # The old provider sleeps 5 s. Deliver a newer share while its query is pending.
        time.sleep(.3)
        d.send("opaque")
        d.rendered("Shared without extension", "latest-share-wins")
        time.sleep(.5)
        d.wait_text("Shared without extension")
        d.checks.append("newest-share-wins-over-slow-provider")

        hashes = {}
        for path in assets.iterdir():
            uri = "content://app.luma.shareprobe.media/" + path.name
            denied = subprocess.run(["adb", "-s", args.serial, "shell", "content", "read", "--uri", uri],
                                    capture_output=True, text=True, timeout=15)
            remote = denied.stdout + denied.stderr
            # Shell has no grant to this unexported provider, unlike the receiving Activity.
            assert "Permission Denial" in remote or "not exported" in remote, path.name
            hashes[path.name] = hashlib.sha256(path.read_bytes()).hexdigest()
        d.checks.append("sender-private-provider-inaccessible-without-grant")
        (d.evidence / "results.json").write_text(json.dumps({"success": True, "checks": d.checks,
            "serial": args.serial, "private_fixture_sha256": hashes, "library_permission_during_checks": False}, indent=2) + "\n")
        print(json.dumps({"success": True, "checks": d.checks}, indent=2))
    finally:
        for permission in permissions:
            d.shell("pm", "grant", PACKAGE, permission)
        d.shell("settings", "put", "system", "user_rotation", rotation)
        d.shell("settings", "put", "system", "accelerometer_rotation", auto)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serial", required=True)
    parser.add_argument("--evidence", default="evidence/share-0.1.8")
    main(parser.parse_args())
