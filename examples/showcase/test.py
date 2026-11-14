#!/usr/bin/env python3
"""Offline public-surface E2E suite. UPDATE_GOLDEN=1 deliberately updates references."""

import argparse
import array
import contextlib
import json
import os
import subprocess
import tempfile
from pathlib import Path

from common import HERE, Session, build, manifest, run, ticks
from PIL import Image, ImageChops


def compare(actual, golden, update):
    if update:
        golden.parent.mkdir(parents=True, exist_ok=True)
        golden.write_bytes(actual.read_bytes())
    with Image.open(actual) as a, Image.open(golden) as g:
        a, g = a.convert("RGBA"), g.convert("RGBA")
        assert a.size == g.size, (actual, a.size, g.size)
        assert a.getchannel("A").getextrema() == (255, 255), actual
        # Match crates/render/tests/golden.rs: every RGBA channel must be within 2.
        extremes = ImageChops.difference(a, g).getextrema()
        assert (
            max(high for _, high in extremes) <= 2
        ), f"Golden mismatch: {actual}; {extremes}"
        background = Image.new("RGBA", a.size, (12, 19, 35, 255))
        assert (
            ImageChops.difference(a, background).convert("RGB").getbbox()
        ), f"Blank frame: {actual}"


def probe(path, vertical):
    data = json.loads(
        subprocess.check_output(
            [
                "ffprobe",
                "-v",
                "error",
                "-count_frames",
                "-show_streams",
                "-show_format",
                "-of",
                "json",
                str(path),
            ],
            text=True,
        )
    )
    (video,) = [s for s in data["streams"] if s["codec_type"] == "video"]
    (audio,) = [s for s in data["streams"] if s["codec_type"] == "audio"]
    assert video["codec_name"] == "h264", data
    assert audio["codec_name"] == "aac", data
    decoded = subprocess.check_output(
        [
            "ffmpeg",
            "-v",
            "error",
            "-threads",
            "1",
            "-i",
            str(path),
            "-vn",
            "-ac",
            "1",
            "-ar",
            "8000",
            "-f",
            "f32le",
            "-",
        ]
    )
    samples = array.array("f")
    samples.frombytes(decoded)
    # Every smoke window contains pinned clicks; verify the mixer was actually attached.
    assert samples and max(abs(value) for value in samples) > 0.005, path
    assert (video["width"], video["height"]) == (
        (360, 640) if vertical else (640, 360)
    ), data
    assert video["avg_frame_rate"] == "12/1", data
    assert int(video["nb_read_frames"]) == 24, data
    assert abs(float(video["duration"]) - 2) < 0.05, data
    assert abs(float(audio["duration"]) - 2) < 0.15, data
    assert (
        abs(float(video.get("start_time", 0)) - float(audio.get("start_time", 0)))
        < 0.05
    ), data
    # Decode the entire MP4, catching corrupt packets as well as plausible metadata.
    subprocess.run(
        [
            "ffmpeg",
            "-v",
            "error",
            "-xerror",
            "-threads",
            "1",
            "-i",
            str(path),
            "-f",
            "null",
            "-",
        ],
        check=True,
    )


def check_structure(session, data, vertical):
    project = session.tool("get_project_state")["project"]
    assert (project["width"], project["height"]) == (
        (360, 640) if vertical else (640, 360)
    )
    assert project["duration"] == ticks(data["duration_seconds"])
    assert [m["time"] for m in project["markers"]] == [
        ticks(ms / 1000) for ms in data["beats_ms"]
    ]
    clips = [c for t in project["tracks"] for c in t["clips"].values()]
    plugins = {c["kind"]["Plugin"]["plugin_id"] for c in clips if "Plugin" in c["kind"]}
    assert {
        "org.animathio.text",
        "org.animathio.shapes",
        "org.animathio.latex",
        "org.animathio.graph",
    } <= plugins
    assert any(c["animations"] for c in clips)
    # The heading is animated through apply_animation_preset, rather than handwritten keys.
    title_track = next(t for t in project["tracks"] if t["name"] == "Explainer title")
    title = next(iter(title_track["clips"].values()))
    opacity = next(
        c for c in title["animations"] if c["target"] == {"Transform": "Opacity"}
    )
    assert opacity["keys"]["0"]["value"] == {"Number": 0.0}
    assert opacity["keys"][str(ticks(0.8))]["value"] == {"Number": 1.0}
    assert any(t.get("transitions") for t in project["tracks"])
    assert sum("Audio" in c["kind"] for c in clips) == 1
    assert len(project["assets"]) == 1
    assert any(
        "caption_style" in c["kind"].get("Plugin", {}).get("params", {}) for c in clips
    )
    # Beat sync must actually split a visible clip at every interior marker.
    rail = next(t for t in project["tracks"] if t["name"] == "Beat rail")
    boundaries = {c["range"]["start"] for c in rail["clips"].values()}
    assert {ticks(ms / 1000) for ms in data["beats_ms"]} <= boundaries


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output",
        type=Path,
        help="Retain generated projects, frames and smoke MP4s here",
    )
    parser.add_argument("--update-golden", action="store_true")
    parser.add_argument("--only", choices=["pythagorean", "unit-circle", "fourier"])
    args = parser.parse_args()
    update = args.update_golden or os.environ.get("UPDATE_GOLDEN") == "1"
    with contextlib.ExitStack() as stack:
        output = (
            args.output.resolve()
            if args.output
            else Path(stack.enter_context(tempfile.TemporaryDirectory()))
        )
        for name in (
            [args.only] if args.only else ["pythagorean", "unit-circle", "fourier"]
        ):
            data = manifest(name)
            assert 0 < data["duration_seconds"] <= 30
            assert data["beats_ms"] == sorted(set(data["beats_ms"]))
            assert set(data["steps_ms"][:-1]) <= set(data["beats_ms"])
            assert data["steps_ms"][-1] == data["duration_seconds"] * 1000
            assert 0 < data["detection_confidence"] <= 1
            for vertical in [False, True]:
                aspect = "9x16" if vertical else "16x9"
                folder = output / name / aspect
                project = build(
                    name,
                    argparse.Namespace(
                        output=folder,
                        vertical=vertical,
                        small=True,
                        offline=True,
                        require_music=False,
                    ),
                )
                with contextlib.closing(Session(project)) as session:
                    check_structure(session, data, vertical)
                    for index, ms in enumerate(data["golden_ms"]):
                        actual = folder / f"frame-{index}.png"
                        session.tool(
                            "render_frame",
                            time_ms=ms,
                            width=360 if vertical else 640,
                            height=640 if vertical else 360,
                            output=str(actual),
                        )
                        compare(
                            actual,
                            HERE / name / "golden" / f"{aspect}-{index}.png",
                            update,
                        )
                smoke = folder / "smoke.mp4"
                # Exercise the dynamic middle of each composition, not just a title card.
                run(
                    "export",
                    project,
                    "--from",
                    8,
                    "--to",
                    10,
                    "--fps",
                    12,
                    "--output",
                    smoke,
                    "--overwrite",
                )
                probe(smoke, vertical)
                print(f"PASS {name} {aspect}: 4 goldens, H.264/AAC smoke", flush=True)


if __name__ == "__main__":
    main()
