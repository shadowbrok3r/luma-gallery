#!/usr/bin/env python3
"""Validate a pulled export; --fast proves each video packet belongs to the original."""
import argparse
import json
import subprocess


def probe(path, *options):
    return json.loads(subprocess.check_output(
        ["ffprobe", "-v", "error", *options, "-of", "json", path], timeout=120))


parser = argparse.ArgumentParser()
parser.add_argument("original")
parser.add_argument("export")
parser.add_argument("--duration", type=float, required=True)
parser.add_argument("--fast", action="store_true")
parser.add_argument("--start", type=float, help="For fast trims, require the nearest preceding keyframe")
args = parser.parse_args()
source = probe(args.original, "-show_streams")
output = probe(args.export, "-show_streams", "-show_format")
video = next(s for s in output["streams"] if s["codec_type"] == "video")
original = next(s for s in source["streams"] if s["codec_type"] == "video")
audio = next(s for s in output["streams"] if s["codec_type"] == "audio")
assert (video["width"], video["height"]) == (original["width"], original["height"])
assert audio["codec_name"] == "aac"
assert abs(float(output["format"]["duration"]) - args.duration) < (1.2 if args.fast else 0.12)
if args.fast:
    options = ("-select_streams", "v:0", "-show_packets", "-show_entries", "packet=data_hash,pts_time,flags", "-show_data_hash", "sha256")
    packets = probe(args.original, *options)["packets"]
    originals = [p["data_hash"] for p in packets]
    exported = [p["data_hash"] for p in probe(args.export, *options)["packets"]]
    assert exported
    start = originals.index(exported[0])
    assert originals[start:start + len(exported)] == exported, "Video packets changed or reordered"
    if args.start is not None:
        keyframes = [p for p in packets if "K" in p.get("flags", "") and float(p["pts_time"]) <= args.start]
        expected = max(keyframes, key=lambda p: float(p["pts_time"])) if keyframes else packets[0]
        assert exported[0] == expected["data_hash"], "Export did not start at the displayed preceding keyframe"
        print(f"PASS: first video packet is the original keyframe at {packets[start]['pts_time']}s")
    print(f"PASS: {len(exported)} video packets are byte-identical and contiguous in the original")
print(f"PASS: {video['width']}x{video['height']}, AAC, {output['format']['duration']}s")
