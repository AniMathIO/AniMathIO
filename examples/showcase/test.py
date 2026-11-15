#!/usr/bin/env python3
"""Offline public-surface E2E suite. UPDATE_GOLDEN=1 deliberately updates references."""

import argparse
import array
import contextlib
import json
import math
import os
import subprocess
import tempfile
from pathlib import Path

from common import HERE, Session, build, manifest, run, ticks
from PIL import Image, ImageChops


def check_frame(actual):
    with Image.open(actual) as source:
        a = source.convert("RGBA")
        assert a.getchannel("A").getextrema() == (255, 255), actual
        background = Image.new("RGBA", a.size, (12, 19, 35, 255))
        assert (
            ImageChops.difference(a, background).convert("RGB").getbbox()
        ), f"Blank frame: {actual}"


def compare(actual, golden, update):
    check_frame(actual)
    if update:
        golden.parent.mkdir(parents=True, exist_ok=True)
        golden.write_bytes(actual.read_bytes())
    with Image.open(actual) as a, Image.open(golden) as g:
        a, g = a.convert("RGBA"), g.convert("RGBA")
        assert a.size == g.size, (actual, a.size, g.size)
        # Match crates/render/tests/golden.rs: every RGBA channel must be within 2.
        extremes = ImageChops.difference(a, g).getextrema()
        assert (
            max(high for _, high in extremes) <= 2
        ), f"Golden mismatch: {actual}; {extremes}"


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
    check_motion(project)
    check_dissection(project)


def check_motion(project):
    """Check the authored rig's geometric constraints independently of raster goldens."""
    width, height = project["width"], project["height"]
    tracks = project["tracks"]
    names = {c["id"]: t["name"] for t in tracks for c in t["clips"].values()}

    def clips_named(name):
        return [
            c for t in tracks if t["name"].startswith(name) for c in t["clips"].values()
        ]

    def keys(clip, property):
        return next(
            curve["keys"]
            for curve in clip["animations"]
            if curve["target"] == {"Transform": property}
        )

    def number(values, time):
        return values[time]["value"]["Number"]

    for point in clips_named("Tracing point"):
        vectors = sorted(
            [c for c in clips_named("Rotating vector") if c["range"] == point["range"]],
            key=lambda c: int(names[c["id"]].split()[-1]),
        )
        assert vectors
        projection = next(
            c for c in clips_named("Height projection") if c["range"] == point["range"]
        )
        plot = next(
            c
            for c in clips_named("Function plot")
            if c["range"] == point["range"]
            and c["kind"]["Plugin"]["params"]["color"] == [94, 228, 187, 255]
        )
        placement = plot["transform"]
        unit_circle = any(t["name"] == "Unit circle" for t in tracks)
        px, py = keys(point, "X"), keys(point, "Y")
        for time in px:
            end = None
            for vector in vectors:
                center = (
                    number(keys(vector, "X"), time) * width,
                    number(keys(vector, "Y"), time) * height,
                )
                if end is not None:
                    assert math.dist(center, end) < 0.01, (
                        "disconnected epicycle",
                        time,
                    )
                rotation = keys(vector, "Rotation")
                finish = max(rotation, key=int)
                angle = number(rotation, finish) * int(time) / int(finish)
                radius = vector["transform"]["scale_x"] * width / 2
                end = (
                    center[0] + radius * math.cos(angle),
                    center[1] + radius * math.sin(angle),
                )
            tracer = (number(px, time) * width, number(py, time) * height)
            assert math.dist(tracer, end) < 0.01, ("tracer detached from vector", time)
            source = (
                number(keys(projection, "X"), time) * width,
                number(keys(projection, "Y"), time) * height,
            )
            assert math.dist(source, tracer) < 0.01, (
                "projection detached from tracer",
                time,
            )
            # The other endpoint must reach the drawn graph, including its different
            # physical height in the vertical layout.
            progress = int(time) / int(max(px, key=int))
            theta = 2 * math.pi * progress
            amplitude = (
                math.sin(2 * theta)
                if unit_circle
                else sum(
                    4 / (math.pi * n) * math.sin(n * theta)
                    for n in range(1, len(vectors) * 2, 2)
                )
            )
            expected = (
                (placement["x"] + placement["scale_x"] * (progress - 0.5)) * width,
                (placement["y"] - placement["scale_y"] * amplitude / 3) * height,
            )
            length = number(keys(projection, "ScaleX"), time) * width
            angle = number(keys(projection, "Rotation"), time)
            endpoint = (
                source[0] + length * math.cos(angle),
                source[1] + length * math.sin(angle),
            )
            assert math.dist(endpoint, expected) < 0.01, (
                "projection misses graph",
                time,
            )


def check_dissection(project):
    """The proof must preserve four 3-4-5 triangles and leave two final squares."""
    triangles = [
        c
        for t in project["tracks"]
        if t["name"].startswith("Congruent triangle")
        for c in t["clips"].values()
    ]
    if not triangles:
        return
    assert len(triangles) == 4
    outer = next(
        c
        for t in project["tracks"]
        if t["name"] == "Outer square"
        for c in t["clips"].values()
    )["transform"]
    width, height = project["width"], project["height"]
    side = outer["scale_x"] * width
    assert abs(side - outer["scale_y"] * height) < 1e-6

    def vertices(clip, final):
        placement = clip["transform"].copy()
        for curve in clip["animations"]:
            field = {"X": "x", "Y": "y", "Rotation": "rotation"}[
                curve["target"]["Transform"]
            ]
            time = (max if final else min)(curve["keys"], key=int)
            placement[field] = curve["keys"][time]["value"]["Number"]
        angle = placement["rotation"]
        result = []
        for verb in json.loads(clip["kind"]["Plugin"]["params"]["path"]):
            if verb == "Close":
                continue
            x, y = next(iter(verb.values()))
            dx, dy = (x - 0.5) * placement["scale_x"] * width, (y - 0.5) * placement[
                "scale_y"
            ] * height
            result.append(
                (
                    (
                        placement["x"] * width
                        + dx * math.cos(angle)
                        - dy * math.sin(angle)
                        - outer["x"] * width
                    )
                    / side
                    + 0.5,
                    (
                        placement["y"] * height
                        + dx * math.sin(angle)
                        + dy * math.cos(angle)
                        - outer["y"] * height
                    )
                    / side
                    + 0.5,
                )
            )
        assert len(result) == 3
        lengths = sorted(
            math.dist(result[i], result[(i + 1) % 3]) * 7 for i in range(3)
        )
        assert all(abs(a - b) < 1e-6 for a, b in zip(lengths, [3, 4, 5]))
        assert all(-1e-6 <= value <= 1 + 1e-6 for point in result for value in point)
        return result

    for final in [False, True]:
        polygons = [vertices(c, final) for c in triangles]
        # Sample away from all edges. No overlap is allowed; after rearranging,
        # precisely the 3² and 4² corner squares must remain uncovered.
        for ix in range(70):
            for iy in range(70):
                p = ((ix + 0.37) / 70, (iy + 0.61) / 70)
                inside = []
                for polygon in polygons:
                    cross = [
                        (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
                        for a, b in zip(polygon, polygon[1:] + polygon[:1])
                    ]
                    inside.append(
                        all(c > 1e-9 for c in cross) or all(c < -1e-9 for c in cross)
                    )
                assert sum(inside) <= 1, ("overlapping proof triangles", final, p)
                if final:
                    square = (p[0] < 3 / 7 and p[1] < 3 / 7) or (
                        p[0] > 3 / 7 and p[1] > 3 / 7
                    )
                    assert (not any(inside)) == square, (
                        "incorrect uncovered squares",
                        p,
                    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output",
        type=Path,
        help="Retain generated projects, frames and smoke MP4s here",
    )
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--update-golden", action="store_true")
    mode.add_argument(
        "--render-only",
        action="store_true",
        help="Render review frames without comparing/replacing goldens or exporting MP4s; requires --output",
    )
    parser.add_argument("--only", choices=["pythagorean", "unit-circle", "fourier"])
    args = parser.parse_args()
    update = args.update_golden or os.environ.get("UPDATE_GOLDEN") == "1"
    if args.render_only and (not args.output or update):
        parser.error(
            "--render-only requires --output and conflicts with golden updates"
        )
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
                        if args.render_only:
                            check_frame(actual)
                        else:
                            compare(
                                actual,
                                HERE / name / "golden" / f"{aspect}-{index}.png",
                                update,
                            )
                if args.render_only:
                    print(f"REVIEW {name} {aspect}: {folder}", flush=True)
                    continue
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
