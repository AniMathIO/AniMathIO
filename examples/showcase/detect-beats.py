#!/usr/bin/env python3
"""Maintainer-only: re-detect the pinned recording excerpt with the public CLI.

Prints manifest-ready beats and selected story boundaries. Does not rewrite the
manifest: review the resulting musical/editorial timing before committing it.
"""

import argparse
import json
import subprocess
import tempfile
from pathlib import Path

from common import cli, manifest, recording, run

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("name", choices=["pythagorean", "unit-circle", "fourier"])
args = parser.parse_args()
data = manifest(args.name)
source = recording(args.name)
if source is None:
    parser.error("run fetch-music.py first")
with tempfile.TemporaryDirectory() as temporary:
    project = Path(temporary) / "music.amio"
    run("import-media", source, "--output", project)
    result = json.loads(
        subprocess.check_output(
            [
                cli(),
                "beats",
                str(project),
                "--from",
                str(data["offset_ms"] / 1000),
                "--to",
                str(data["offset_ms"] / 1000 + data["duration_seconds"]),
                "--min-bpm",
                str(data["detection_min_bpm"]),
                "--max-bpm",
                str(data["detection_max_bpm"]),
            ],
            text=True,
        )
    )
    beats = [
        round(beat["seconds"] * 1000) - data["offset_ms"] for beat in result["beats"]
    ]
    beats = [ms for ms in beats if 100 <= ms < data["duration_seconds"] * 1000 - 100]
    # Use recorded beats closest to the intended four story boundaries.
    steps = [
        min(beats, key=lambda ms: abs(ms - target)) for target in [1000, 7000, 15000]
    ]
    steps.append(data["duration_seconds"] * 1000)
    print("beats_ms =", json.dumps(beats))
    print("steps_ms =", json.dumps(steps))
    print("detection_bpm =", result["bpm"])
    print("detection_confidence =", result["confidence"])
