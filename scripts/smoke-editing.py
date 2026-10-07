#!/usr/bin/env python3
"""Editor regression explored with ARTEMIS observations and ADB on the s26ultra AVD.

Requires adb, ffmpeg/ffprobe, Pillow, Tesseract and media access in the installed app.
Creates seven synthetic files in DCIM/Luma-Editor and exports copies to Pictures/Luma
and Movies/Luma. Never deletes media or changes credentials. --qwen submits one
synthetic image to the server already configured in Luma (configure it in the UI).
OCR locates labels first; icon fallbacks use verified portrait coordinates scaled
from 1440x3120. Every export is awaited on disk and checked independently of the UI.
"""
import argparse
import array
import hashlib
import io
import json
import os
import runpy
import subprocess
import time
from pathlib import Path

from PIL import Image, ImageChops, ImageOps, ImageStat

Device = runpy.run_path(str(Path(__file__).with_name("smoke-android.py")))["Device"]


def run(*args):
    return subprocess.check_output(list(map(str, args)), stderr=subprocess.PIPE, timeout=120)


def fixtures(d, folder):
    folder.mkdir(parents=True, exist_ok=True)
    run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i",
        "testsrc=size=900x600:rate=1", "-frames:v", "1", folder / "Photo-Grid.png")
    source = Image.open(folder / "Photo-Grid.png").convert("RGB")
    source.save(folder / "Photo-WebP.webp", lossless=True)
    exif = Image.Exif(); exif[274] = 6
    source.save(folder / "Photo-Rotated.jpg", quality=95, exif=exif)
    source.save(folder / "Photo-Animation.gif", save_all=True,
                append_images=[ImageOps.mirror(source)], duration=500, loop=0)
    run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i",
        "testsrc2=size=640x360:rate=30:duration=12", "-f", "lavfi", "-i",
        r"aevalsrc=if(lt(mod(t\,2)\,0.1)\,0.7*sin(2*PI*440*t)\,0):s=48000:d=12",
        "-c:v", "libx264", "-crf", "20", "-pix_fmt", "yuv420p", "-c:a", "aac",
        "-shortest", folder / "Clip-Beats.mp4")
    run("ffmpeg", "-v", "error", "-y", "-display_rotation:v:0", "90", "-i", folder / "Clip-Beats.mp4",
        "-c", "copy", folder / "Clip-Rotated.mov")
    probe=json.loads(run("ffprobe","-v","error","-show_streams","-of","json",folder/"Clip-Rotated.mov"))
    assert any(abs(s.get("rotation",0))==90 for s in probe["streams"][0].get("side_data_list",[])), "Fixture lacks rotation"
    run("ffmpeg", "-v", "error", "-y", "-i", folder / "Clip-Beats.mp4", "-t", "3",
        "-an", "-c:v", "libvpx-vp9", "-deadline", "realtime", folder / "Clip-Silent.webm")
    d.shell("mkdir", "-p", "/sdcard/DCIM/Luma-Editor")
    for path in folder.iterdir():
        d.adb("push", "-q", str(path), "/sdcard/DCIM/Luma-Editor/" + path.name)
    d.shell("content", "call", "--uri", "content://media", "--method", "scan_volume", "--arg", "external_primary")


def open_fixture(d, name, photo=False):
    d.shell("am", "force-stop", "app.luma.gallery")
    d.shell("am", "start", "-n", "app.luma.gallery/.GalleryActivity")
    d.wait_text("Luma Gallery")
    d.tap_text("Luma-Editor", (337, 944)); d.wait_text("7 items")
    d.tap(1326, 389)  # Album search icon.
    d.tap_text("Search files or albums", (400, 535))
    d.shell("input", "text", name)
    d.wait_text("1 items"); d.tap(250, 810)
    d.wait_text(name)
    deadline=time.monotonic()+20
    while True:
        shot=d.screen();w,h=shot.size
        if max(ImageStat.Stat(shot.crop((w*.2,h*.4,w*.8,h*.62))).stddev)>12:break
        assert time.monotonic()<deadline,"Video/photo did not render: "+name
        time.sleep(.2)
    d.media(name + "-viewer")
    if name=="Clip-Rotated":
        d.wait_text("640p")
        deadline=time.monotonic()+10
        while True:
            im=d.screen().convert("RGB");w,h=im.size
            xs=[x for x in range(w) if max(im.getpixel((x,round(h*.39))))>100]
            if xs and max(xs)-min(xs) > w*.6:break
            assert time.monotonic()<deadline,"Portrait video is letterboxed twice"
            time.sleep(.2)
        d.screen("rotated-preview-aspect")
    if photo:
        d.tap(1340, 2580)  # Photo edit icon.
        d.wait_text("Edit photo"); d.wait_text("SDR copy")


def names(d, folder, prefix):
    output = d.shell("ls", "-1", folder, "2>/dev/null", "|| true")
    return {name for name in output.splitlines() if name.startswith(prefix)}


def collect(d, folder, prefix, before, local, timeout=120):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        added = names(d, folder, prefix) - before
        if added:
            assert len(added) == 1, "Ambiguous export: another task created the same fixture"
            name = added.pop()
            # MediaStore publishes the final filename after its pending write is complete.
            path = folder + "/" + name
            d.adb("pull", path, str(local))
            if local.stat().st_size > 100:
                return local
        time.sleep(.25)
    d.screen("export-timeout")
    raise AssertionError("No completed export: " + prefix)


def save_photo(d, prefix, local, ai=False):
    before = names(d, "/sdcard/Pictures/Luma", prefix)
    if not ai:
        # Full-screen OCR also sees '.png' in the source filename. Restrict it
        # to the observed format checkbox before falling back to its icon.
        shot=d.screen(); top=round(shot.height*.73)
        region=shot.crop((0,top,round(shot.width*.14),round(shot.height*.78)))
        match=next(((x,y+top) for text,x,y in d.lines(region) if text.strip()=="PNG"),None)
        if match: d.shell("input","tap",*match)
        else: d.tap(65,2345)
    d.tap_text("Save copy", (1206, 230))
    d.wait_text("Saved to Pictures/Luma", timeout=60)
    return collect(d, "/sdcard/Pictures/Luma", prefix, before, local)


def canvas(d):
    shot = d.screen()
    return shot.crop((shot.width*.104, shot.height*.2083, shot.width*.896, shot.height*.5192))


def pressure_check(d, work):
    assert d.screen().size==(1440,3120),"Pressure pixel probe is calibrated to the s26ultra AVD"
    sdk=Path(os.environ.get("ANDROID_HOME",str(Path.home()/"Android/Sdk")))
    build=work/"stylus-probe";build.mkdir(parents=True,exist_ok=True)
    platform=sorted((sdk/"platforms").glob("android-*/android.jar"))[-1]
    d8=sorted((sdk/"build-tools").glob("*/d8"))[-1]
    run("javac","-source","17","-target","17","-classpath",platform,"-d",build,Path(__file__).with_name("StylusStroke.java"))
    run(d8,"--min-api","29","--output",build,build/"StylusStroke.class")
    d.adb("push","-q",str(build/"classes.dex"),"/data/local/tmp/luma-stylus.dex")
    def stroke(y,force,tool=2,buttons=0):
        d.shell("CLASSPATH=/data/local/tmp/luma-stylus.dex","app_process","/system/bin","StylusStroke",635,y,force,tool,buttons,600)
    before=d.screen()
    stroke(900,.2);stroke(1250,1)
    painted=d.screen("pressure-held")
    diff=ImageChops.difference(before,painted)
    def area(y):
        band=diff.crop((570,y-65,700,y+65)).convert("L")
        return sum(value>8 for value in band.tobytes())
    light,firm=area(900),area(1250)
    assert light>100 and firm>light*2,(light,firm)
    stroke(1250,1,tool=4)
    assert d.screen().getpixel((635,1250))==before.getpixel((635,1250)),"Eraser tip did not erase"
    d.tap(430,2122)
    assert ImageChops.difference(painted,d.screen()).crop((570,1185,700,1315)).getbbox() is None,"Undo eraser did not restore"
    stroke(1250,1,buttons=32)
    assert d.screen().getpixel((635,1250))==before.getpixel((635,1250)),"Barrel button did not erase"
    d.tap(595,2122)  # Clear synthetic mask.
    (work/"pressure.json").write_text(json.dumps({"light_pixels":light,"firm_pixels":firm,"held_ms":600,"eraser_and_barrel":True},indent=2)+"\n")
    d.checks.append("stationary-pressure-eraser-tip-barrel-undo")


def mask_checks(d):
    d.tap_text("Qwen edit", (570, 1986))
    d.tap_text("Pen only", (710, 2118))
    before = canvas(d)
    d.shell("input", "swipe", 400, 1000, 920, 1350, 700)
    assert ImageChops.difference(before, canvas(d)).getbbox() is None, "Finger was accepted in pen-only mode"
    d.shell("input", "stylus", "swipe", 400, 1000, 920, 1350, 700)
    painted=canvas(d)
    assert ImageChops.difference(before, painted).getbbox(), "Stylus did not paint"
    d.screen("stylus-mask")
    d.tap_text("Crop",(110,1986));d.tap_text("Qwen edit",(570,1986))
    assert ImageChops.difference(painted,canvas(d)).getbbox() is None, "Tab switch lost the mask"
    d.tap(430, 2122)  # Undo SVG.
    assert ImageChops.difference(before, canvas(d)).getbbox() is None, "Undo changed the source"
    d.tap(710, 2118)  # Return to finger painting for the optional Qwen test.
    d.checks.append("pen-only-finger-rejection-stylus-paint-undo")


def ime_state(d, shown, timeout=8):
    deadline = time.monotonic() + timeout
    while True:
        state = d.shell("dumpsys", "input_method")
        # The hidden EguiNativeActivity EditText is served only when the IME bridge is active.
        if ("mInputShown=true" in state and "EguiNativeActivity" in state) == shown:
            return
        assert time.monotonic() < deadline, "Soft keyboard " + ("did not open" if shown else "stayed open")
        time.sleep(.25)


def keyboard_check(d):
    old = d.shell("settings", "get", "secure", "show_ime_with_hard_keyboard")
    d.shell("settings", "put", "secure", "show_ime_with_hard_keyboard", 1)
    try:
        time.sleep(1)  # The IME reloads its configuration after the setting changes.
        d.tap_text("Paint an area", (489, 2405))
        ime_state(d, True)
        d.screen("qwen-soft-keyboard")
        d.shell("input", "text", "keyboard")
        d.shell("input", "keyevent", "KEYCODE_BACK")
        ime_state(d, False)
        # The suggestion strip is gone, so the only match is the prompt itself.
        x, y = d.wait_text("keyboard", exact=True)
        d.shell("input", "tap", x + 600, y)
        ime_state(d, True)
        d.shell("input", "keyevent", *["KEYCODE_DEL"] * 8)
        d.shell("input", "keyevent", "KEYCODE_BACK")
        ime_state(d, False)
        d.wait_text("Paint an area")
    finally:
        d.shell("settings", "put", "secure", "show_ime_with_hard_keyboard", old if old in ("0", "1") else 0)
    d.checks.append("qwen-prompt-raises-soft-keyboard")


def qwen_check(d, source, work):
    d.tap(297, 2238)  # Larger brush, as explored on the Qwen canvas.
    d.shell("input", "swipe", 1140, 1050, 1200, 1230, 600)
    d.tap_text("Paint an area", (489, 2405))
    d.shell("input", "text", "Replace%sthe%sblack%snumber%son%sthe%sright%swith%sa%ssolid%sred%scircle.")
    d.tap_text("Run edit", (715, 2545))
    d.wait_text("Qwen queued",timeout=180)
    d.tap_text("Stop waiting",(210,2844))
    d.wait_text("Stopped waiting",timeout=30)
    d.tap_text("Resume",(966,2542))
    d.wait_text("Review the result", timeout=600)
    d.screen("qwen-result")
    output = Image.open(save_photo(d, "Photo-Grid_qwen_", work / "qwen.png", ai=True)).convert("RGB")
    original = Image.open(source).convert("RGB")
    box = ImageChops.difference(original, output).getbbox()
    assert output.size == original.size and box and box[0] > 580 and box[1] > 50, box
    d.checks.append("live-qwen-upload-stop-resume-save-preserves-unpainted-area")
    open_fixture(d, "Photo-Grid", photo=True)
    d.tap_text("Qwen edit", (570, 1986)); d.tap_text("Resume", (966, 2542))
    d.wait_text("Qwen result"); d.screen("qwen-resume")
    d.checks.append("qwen-result-resumes-after-process-restart")


def numeric(d, x, y, value):
    # Use the observed mobile Select All toolbar; ADB Ctrl+A is text on this host.
    d.tap(x, y)
    ime_state(d, True)  # The layout shrinks above the soft keyboard before the toolbar settles.
    # Tap Select All only once two polls agree on its position (the bar moves while the keyboard slides in).
    deadline=time.monotonic()+4;match=previous=None
    while time.monotonic()<deadline:
        shot=d.screen();w,h=shot.size;left,top=round(w*.62),round(h*.526)
        band=shot.crop((left,top,round(w*.71),round(h*.61)))
        match=next(((tx+left,ty+top) for text,tx,ty in d.lines(band) if text.strip().lower()=="aa"),None)
        if match and previous and abs(match[1]-previous[1])<=4:break
        previous=match
    if match:d.shell("input","tap",*match)
    else:d.tap(963,1825)
    time.sleep(.15)  # Select All lands on the next frame.
    d.shell("input", "text", str(value)); d.shell("input", "keyevent", "KEYCODE_ENTER")
    ime_state(d, False)
    d.tap(855, 2550)


def clip_check(d, work, name, mute=False):
    open_fixture(d, name)
    d.tap(111, 2865); d.tap(267, 2865)  # Trim and Crop SVGs.
    d.wait_text("Keep audio in clip")
    numeric(d, 300, 2440, 2)
    numeric(d, 700, 2440, 6)
    d.tap_text("1:1", (257, 2610))
    d.screen(name + "-trim-waveform-crop")
    # Color peaks must form a separate lane beneath the thumbnails.
    im=d.screen().convert("RGB")
    if not mute:
        counts=[]
        for i in range(6):
            x=round(im.width*(.06+.9*(i*2/12)))
            band=im.crop((max(0,x-10),round(im.height*.658),min(im.width,x+22),round(im.height*.695)))
            pixels=band.tobytes()
            counts.append(sum(g>110 and b>90 and r<90 for r,g,b in zip(pixels[0::3],pixels[1::3],pixels[2::3])))
        # The trimmed-out peaks are deliberately dimmed; two retained transients remain bright.
        assert sum(c>20 for c in counts)>=2, counts
    before=names(d,"/sdcard/Movies/Luma",name+"_clip_")
    d.tap_text("Export clip",(1200,2820));d.wait_text("Save clip")
    if mute:
        d.tap_text("Keep audio in clip",(310,1425))
    d.tap_text("Save clip",(522,1941))
    path=collect(d,"/sdcard/Movies/Luma",name+"_clip_",before,work/(name+"-square.mp4"))
    probe=json.loads(run("ffprobe","-v","error","-show_streams","-show_format","-of","json",path))
    video=next(s for s in probe["streams"] if s["codec_type"]=="video")
    assert (video["width"],video["height"])==(360,360),video
    assert not any(s.get("rotation",0) for s in video.get("side_data_list",[])),"Export would rotate a second time"
    assert abs(float(probe["format"]["duration"])-4)<.16,probe["format"]["duration"]
    extension=".mov" if name=="Clip-Rotated" else ".mp4"
    expected=Image.open(io.BytesIO(run("ffmpeg","-v","error","-ss","2","-i",work/"fixtures"/(name+extension),
        "-vf","crop=360:360","-frames:v","1","-f","image2pipe","-c:v","png","-"))).convert("RGB")
    actual=Image.open(io.BytesIO(run("ffmpeg","-v","error","-i",path,"-frames:v","1","-f","image2pipe","-c:v","png","-"))).convert("RGB")
    assert max(ImageStat.Stat(ImageChops.difference(expected,actual)).rms)<12,"Wrong first frame or crop orientation"
    audio=[s for s in probe["streams"] if s["codec_type"]=="audio"]
    assert bool(audio)!=mute
    if audio:
        pcm=array.array("h",run("ffmpeg","-v","error","-i",path,"-map","0:a:0","-ac","1","-ar","8000","-f","s16le","-"))
        peaks=[max(abs(v) for v in pcm[i:i+160]) for i in range(0,len(pcm),160)]
        active=[i*.02 for i,x in enumerate(peaks) if x>8000]
        assert active and all(0<=t<=.22 or 1.98<=t<=2.22 for t in active),active
        assert any(t<.12 for t in active) and any(2<=t<2.12 for t in active)
    d.checks.append(name+"-exact-crop-"+("muted" if mute else "audio-pulses-aligned"))


def main(args):
    d = Device(args.serial, args.evidence)
    work = Path(args.evidence)
    originals = work / "fixtures"
    old_rotation = d.shell("settings", "get", "system", "user_rotation")
    old_auto = d.shell("settings", "get", "system", "accelerometer_rotation")
    complete=False
    try:
        d.shell("settings", "put", "system", "accelerometer_rotation", 0)
        d.shell("settings", "put", "system", "user_rotation", 0)
        fixtures(d, originals)
        expected_hashes = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in originals.iterdir()}
        source = Image.open(originals / "Photo-Grid.png").convert("RGB")
        open_fixture(d, "Photo-Grid", photo=True)
        d.tap_text("1:1", (257, 2100))
        output = Image.open(save_photo(d, "Photo-Grid_edit_", work / "square.png")).convert("RGB")
        assert output.size == (600, 600)
        assert ImageChops.difference(output, source.crop((150, 0, 750, 600))).getbbox() is None
        d.checks.append("square-crop-pixel-exact")
        d.tap_text("Reset", (685, 2365)); d.tap(111, 2232); d.tap(270, 2232)
        before = names(d, "/sdcard/Pictures/Luma", "Photo-Grid_edit_")
        d.tap_text("Save copy", (1206, 230)); d.wait_text("Saved to Pictures/Luma")
        path = collect(d, "/sdcard/Pictures/Luma", "Photo-Grid_edit_", before, work / "rotated-flipped.png")
        expected = ImageOps.mirror(source.transpose(Image.Transpose.ROTATE_270))
        assert ImageChops.difference(Image.open(path).convert("RGB"), expected).getbbox() is None
        d.checks.append("rotate-flip-pixel-exact")
        d.tap_text("Reset", (685, 2365));d.tap_text("Adjust",(315,1986));d.tap(45,2305)
        d.tap_text("Crop",(110,1986))
        before=names(d,"/sdcard/Pictures/Luma","Photo-Grid_edit_")
        d.tap_text("Save copy",(1206,230));d.wait_text("Saved to Pictures/Luma")
        path=collect(d,"/sdcard/Pictures/Luma","Photo-Grid_edit_",before,work/"grayscale.png")
        red,green,blue=Image.open(path).convert("RGB").split()
        assert ImageChops.difference(red,green).getbbox() is None and ImageChops.difference(green,blue).getbbox() is None
        d.checks.append("color-adjustment-grayscale-export")
        d.tap_text("Reset",(685,2365)); mask_checks(d); keyboard_check(d)
        if args.stylus:
            pressure_check(d,work)
        if args.qwen:
            qwen_check(d, originals / "Photo-Grid.png", work)
        for filename in ["Photo-WebP.webp", "Photo-Rotated.jpg", "Photo-Animation.gif"]:
            stem = Path(filename).stem
            open_fixture(d, stem, photo=True)
            output = Image.open(save_photo(d, stem + "_edit_", work / (stem + ".png"))).convert("RGB")
            expected = ImageOps.exif_transpose(Image.open(originals / filename)).convert("RGB")
            assert output.size == expected.size, (filename, output.size, expected.size)
            difference = ImageStat.Stat(ImageChops.difference(output, expected)).mean
            assert max(difference) < 1.5, (filename, difference)
            d.checks.append(stem + "-oriented-still-export")
        open_fixture(d, "Clip-Silent")
        d.wait_text("No audio track"); d.screen("silent-webm")
        d.checks.append("webm-playback-no-audio")
        clip_check(d, work, "Clip-Beats")
        clip_check(d, work, "Clip-Rotated", mute=True)
        for filename, digest in expected_hashes.items():
            actual = d.shell("sha256sum", "/sdcard/DCIM/Luma-Editor/" + filename).split()[0]
            assert actual == digest, "Original changed: " + filename
        d.checks.append("all-seven-originals-unchanged")
        complete=True
    finally:
        d.shell("settings", "put", "system", "user_rotation", old_rotation)
        d.shell("settings", "put", "system", "accelerometer_rotation", old_auto)
        (work / "results.json").write_text(json.dumps({"success":complete,"checks": d.checks}, indent=2) + "\n")
    print(json.dumps({"passed": len(d.checks), "checks": d.checks}, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serial", required=True)
    parser.add_argument("--evidence", default="evidence/editing-smoke")
    parser.add_argument("--qwen", action="store_true", help="Use the server credentials saved in Luma for one synthetic edit")
    parser.add_argument("--stylus", action="store_true", help="Compile an SDK/app_process probe for held pressure, eraser and barrel input")
    main(parser.parse_args())
