"""Turn a demo recording into one PNG per example, to check layouts by eye.

Usage:
    python3 scripts/record_demo.py /tmp/examples.cast
    python3 scripts/demo_frames.py /tmp/examples.cast /tmp/frames

record_demo.py writes an asciicast marker ("m" event) naming each example at
the start of its segment. This script splits the cast at those markers,
renders each segment with agg, and writes NN-<example>.gif plus
NN-<example>.png (the segment's last frame, i.e. the settled screen).

Needs agg on PATH and Pillow (`brew install pillow`).
"""
import argparse
import json
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image

# Same font as record_demo.py's agg line: agg's default font renders U+2800
# blank Braille as dotted tofu, which looks like a framework bug and is not one.
AGG_FONT = "DejaVu Sans Mono"


def split_segments(cast_path):
    """Return (header line, [(name, [event, ...]), ...]) with event times
    rebased to each segment's marker."""
    lines = cast_path.read_text(encoding="utf-8").splitlines()
    header, segments = lines[0], []
    for line in lines[1:]:
        if not line.strip():
            continue
        t, kind, data = json.loads(line)
        if kind == "m":
            segments.append((data, t, []))
        elif segments:
            _, start, events = segments[-1]
            events.append([round(t - start, 6), kind, data])
    return header, [(name, events) for name, _, events in segments]


def safe_name(name):
    # Marker labels come from the cast file; keep them out of path syntax.
    return re.sub(r"[^A-Za-z0-9_-]", "_", name) or "segment"


def main():
    parser = argparse.ArgumentParser(
        description="Write one PNG per example from a record_demo.py cast.")
    parser.add_argument("cast", type=Path, help="asciicast written by record_demo.py")
    parser.add_argument("out_dir", type=Path, help="directory for the GIFs and PNGs")
    args = parser.parse_args()

    if shutil.which("agg") is None:
        sys.exit("demo_frames: agg not found on PATH (cargo install --git https://github.com/asciinema/agg)")
    header, segments = split_segments(args.cast)
    if not segments:
        sys.exit(f"demo_frames: no marker events in {args.cast}; record it with scripts/record_demo.py")

    args.out_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        for i, (name, events) in enumerate(segments, start=1):
            stem = f"{i:02d}-{safe_name(name)}"
            if not events:
                print(f"{stem}: no output recorded, skipped", flush=True)
                continue
            seg_cast = Path(tmp) / f"{stem}.cast"
            seg_cast.write_text(
                header + "\n" + "".join(json.dumps(e) + "\n" for e in events),
                encoding="utf-8")
            gif = args.out_dir / f"{stem}.gif"
            png = args.out_dir / f"{stem}.png"
            subprocess.run(
                ["agg", "-q", "--font-family", AGG_FONT, str(seg_cast), str(gif)],
                check=True)
            with Image.open(gif) as im:
                im.seek(getattr(im, "n_frames", 1) - 1)
                im.convert("RGB").save(png)
            print(f"{stem}: {png}", flush=True)


if __name__ == "__main__":
    main()
