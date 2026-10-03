#!/usr/bin/env python3
"""Exercise real JPEG XL RAW decoding, not just Samsung's embedded JPEG.

Build with: cmake -S native/dng -B .build/dng-host -DCMAKE_BUILD_TYPE=Release
            cmake --build .build/dng-host --target dng_decode -j 8
Requires Pillow and ExifTool. Originals are read-only; variants use a temp dir.
"""
import argparse
import hashlib
import json
import subprocess
import tempfile
import time
from pathlib import Path

from PIL import Image, ImageStat


def run(args):
    decoder = str(Path(args.decoder).resolve())
    sample = Path(args.sample).resolve()
    metadata = json.loads(subprocess.check_output([
        "exiftool", "-j", "-n", "-IFD0:Compression", "-IFD0:PreviewJXLStart",
        "-IFD0:PreviewJXLLength", "-SubIFD:PreviewImageStart",
        "-SubIFD:PreviewImageLength", str(sample)], timeout=30))[0]
    assert metadata["Compression"] == 52546, "Expected the Galaxy S26 JPEG XL DNG fixture"
    source = sample.read_bytes()
    original_hash = hashlib.sha256(source).hexdigest()
    results = []
    with tempfile.TemporaryDirectory(prefix="luma-dng-") as temp:
        temp = Path(temp)

        def render(path, name, mode="full", stdin=None, succeeds=True, check_detail=True):
            output = temp / (name + ".ppm")
            start = time.monotonic()
            result = subprocess.run([decoder, str(path), str(output), mode], input=stdin,
                                    capture_output=True, timeout=180)
            elapsed = time.monotonic() - start
            if not succeeds:
                assert result.returncode != 0 and not output.exists(), name
                results.append({"check": name, "rejected": True})
                return None
            assert result.returncode == 0, result.stderr.decode(errors="replace")
            with Image.open(output) as image:
                assert image.mode == "RGB"
                dimensions = image.size
                assert min(dimensions) > 0
                stats = ImageStat.Stat(image.resize((128, 128)))
                if check_detail:
                    assert max(stats.stddev) > 5, name
                else:
                    # Adobe's gain-table fixtures deliberately contain uniform patches.
                    assert max(stats.mean) > 0, name
            results.append({"check": name, "dimensions": dimensions,
                            "seconds": round(elapsed, 3), "report": result.stderr.decode().strip()})
            return output

        full = render(sample, "s26-full")
        with Image.open(full) as image:
            assert image.size == (3060, 4080), "Resolution or EXIF orientation regressed"
        stdin = render("-", "s26-stdin", stdin=source)
        expected = hashlib.sha256(full.read_bytes()).digest()
        assert hashlib.sha256(stdin.read_bytes()).digest() == expected
        preview = render(sample, "s26-preview", mode="preview")
        with Image.open(preview) as image:
            assert image.size == (1200, 1600)

        # Damage only the embedded JPEG: full development must be unaffected.
        damaged = bytearray(source)
        offset = metadata["PreviewImageStart"]
        length = metadata["PreviewImageLength"]
        assert 0 < offset < offset + length <= len(damaged)
        damaged[offset:offset + length] = b"\0" * length
        variant = temp / "no-jpeg.dng"
        variant.write_bytes(damaged)
        developed = render(variant, "develops-without-embedded-jpeg")
        assert hashlib.sha256(developed.read_bytes()).digest() == expected

        # Conversely, a broken RAW stream must fail even with a valid JPEG preview.
        damaged = bytearray(source)
        offset = metadata["PreviewJXLStart"]
        length = metadata["PreviewJXLLength"]
        assert 0 < offset < offset + length <= len(damaged)
        damaged[offset:offset + 32] = b"\0" * 32
        variant = temp / "broken-jxl.dng"
        variant.write_bytes(damaged)
        render(variant, "rejects-broken-raw-with-valid-jpeg", succeeds=False)
        render("-", "rejects-non-dng", stdin=b"not a DNG", succeeds=False)
        render("-", "rejects-truncated-dng", stdin=source[:1024], succeeds=False)

        if args.sdk_samples:
            for fixture in sorted(Path(args.sdk_samples).glob("*.dng")):
                render(fixture.resolve(), fixture.stem, check_detail=False)

    assert hashlib.sha256(sample.read_bytes()).hexdigest() == original_hash
    report = {"result": "passed", "sample_sha256": original_hash, "checks": results}
    evidence = Path(args.evidence)
    evidence.mkdir(parents=True, exist_ok=True)
    (evidence / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--sample", required=True)
    parser.add_argument("--decoder", default=".build/dng-host/dng_decode")
    parser.add_argument("--sdk-samples")
    parser.add_argument("--evidence", default="evidence/dng-native")
    run(parser.parse_args())
