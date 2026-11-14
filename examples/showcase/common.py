"""Build showcases through CLI/MCP only; never serialize a Project ourselves."""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import json
import math
import os
import runpy
import shutil
import struct
import subprocess
import tempfile
import tomllib
import urllib.request
import wave
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
# Headless authoring/export must remain silent even when called outside Xvfb.
os.environ.update(
    ANIMATHIO_AUDIO="none",
    PIPEWIRE_REMOTE="animathio-silent-no-such-remote",
    PULSE_SERVER="unix:/nonexistent/animathio-silent",
    SDL_AUDIODRIVER="dummy",
)
TICKS = 705_600_000
WHITE = [235, 242, 255, 255]
BLUE = [87, 178, 255, 255]
GOLD = [255, 202, 102, 255]
MINT = [94, 228, 187, 255]
PINK = [240, 132, 183, 255]


def ticks(seconds):
    return round(seconds * TICKS)


def cli():
    return os.environ.get("ANIMATHIO_CLI", str(ROOT / "target/debug/animathio-cli"))


def run(*args):
    subprocess.run([cli(), *map(str, args)], check=True)


class Session:
    """Newline-delimited MCP stdio client; all protocol/tool errors are fatal."""

    def __init__(self, project):
        self.process = subprocess.Popen(
            [cli(), "mcp", "--project", str(project)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            text=True,
            encoding="utf-8",
        )
        self.next_id = 0
        self.request(
            "initialize",
            {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "showcase", "version": "1"},
            },
        )
        self.process.stdin.write(
            json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n"
        )
        self.process.stdin.flush()

    def request(self, method, params):
        self.next_id += 1
        self.process.stdin.write(
            json.dumps(
                {
                    "jsonrpc": "2.0",
                    "id": self.next_id,
                    "method": method,
                    "params": params,
                }
            )
            + "\n"
        )
        self.process.stdin.flush()
        while line := self.process.stdout.readline():
            response = json.loads(line)
            if response.get("id") != self.next_id:
                continue
            if "error" in response:
                raise RuntimeError(response["error"])
            return response["result"]
        raise RuntimeError("MCP closed unexpectedly")

    def tool(self, name, **arguments):
        result = self.request("tools/call", {"name": name, "arguments": arguments})
        if result.get("isError"):
            raise RuntimeError(f"{name}: {result}")
        content = result["content"][0]["text"]
        try:
            return json.loads(content)
        except json.JSONDecodeError:
            return content

    def batch(self, commands):
        if commands:
            return self.tool("batch", ops=commands)

    def close(self):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.process.terminate()
            self.process.wait(timeout=10)
        self.process.stdout.close()


def manifest(name):
    with (HERE / name / "music.toml").open("rb") as file:
        return tomllib.load(file)


def cache_dir():
    return Path(os.environ.get("SHOWCASE_CACHE", ROOT / "target/showcase-cache"))


def recording(name, fetch=False):
    data = manifest(name)
    path = cache_dir() / (name + Path(data["url"]).suffix)
    if not path.exists() and fetch:
        path.parent.mkdir(parents=True, exist_ok=True)
        request = urllib.request.Request(
            data["url"], headers={"User-Agent": "AniMathIO-showcase/2.0"}
        )
        # Download to a private temporary file and publish only verified bytes.
        with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as output:
            temp = Path(output.name)
            try:
                with urllib.request.urlopen(request, timeout=60) as response:
                    shutil.copyfileobj(response, output)
            except BaseException:
                temp.unlink(missing_ok=True)
                raise
        try:
            verify(temp, data["sha256"])
            temp.replace(path)
        finally:
            temp.unlink(missing_ok=True)
    if path.exists():
        verify(path, data["sha256"])
        return path
    return None


def verify(path, expected):
    if hashlib.sha256(path.read_bytes()).hexdigest() != expected:
        raise ValueError(f"SHA-256 mismatch: {path}")


def click_track(path, duration, beats):
    """Small deterministic PCM fixture with clicks at the committed beat positions."""
    rate = 8000
    samples = bytearray(duration * rate * 2)
    for ms in beats:
        start = round(ms * rate / 1000)
        for i in range(80):
            if start + i < duration * rate:
                value = round(
                    4000 * (1 - i / 80) * math.sin(2 * math.pi * 1000 * i / rate)
                )
                struct.pack_into("<h", samples, (start + i) * 2, value)
    with wave.open(str(path), "wb") as output:
        output.setparams((1, 2, rate, 0, "NONE", "not compressed"))
        output.writeframes(samples)


def transform(x=0.5, y=0.5, sx=1, sy=1, rotation=0, opacity=1):
    return {
        "x": x,
        "y": y,
        "scale_x": sx,
        "scale_y": sy,
        "rotation": rotation,
        "opacity": opacity,
    }


def curve(target, values, interpolation="Linear"):
    return {
        "target": target,
        "keys": {
            str(ticks(t)): {"value": {"Number": v}, "interpolation": interpolation}
            for t, v in values
        },
    }


def path_verbs(points, close=False):
    return (
        [{"MoveTo": list(points[0])}]
        + [{"LineTo": list(p)} for p in points[1:]]
        + (["Close"] if close else [])
    )


class Builder:
    def __init__(self, session, vertical, duration, small=False):
        self.session = session
        self.vertical = vertical
        self.duration = duration
        self.width, self.height = (
            ((360, 640) if vertical else (640, 360))
            if small
            else ((1080, 1920) if vertical else (1920, 1080))
        )
        short_edge = min(self.width, self.height)
        self.graph_stroke = 3 * short_edge / 360 / max(short_edge / 540, 1)
        self.commands = []
        self.presets = []
        self.next_id = 100
        self.track_index = 0
        state = session.tool("get_project_state")["project"]
        session.batch(
            [{"RemoveTrack": {"id": t["id"]}} for t in state["tracks"]]
            + [{"RemoveAsset": {"id": a}} for a in state["assets"]]
            + [
                {
                    "SetSettings": {
                        "width": self.width,
                        "height": self.height,
                        "frame_rate": {"numerator": 30, "denominator": 1},
                        "background": [12, 19, 35, 255],
                        "duration": ticks(duration),
                    }
                }
            ]
        )
        # The render API returns transparent frames. An explicit background layer makes
        # CLI PNGs, preview and the opaque MP4 delivery agree without postprocessing.
        self.shape(
            "Background",
            [(0, 0), (1, 0), (1, 1), (0, 1)],
            [12, 19, 35, 255],
            True,
            stroke_width=0,
        )

    def clip(
        self,
        name,
        plugin,
        params,
        start=0,
        end=None,
        placement=None,
        animations=None,
        track=None,
    ):
        self.next_id += 2
        identity = self.next_id
        if track is None:
            track = identity - 1
            self.commands.append(
                {
                    "AddTrack": {
                        "track": {"id": track, "name": name, "clips": {}},
                        "index": self.track_index,
                    }
                }
            )
            self.track_index += 1
        clip = {
            "id": identity,
            "kind": {
                "Plugin": {"plugin_id": "org.animathio." + plugin, "params": params}
            },
            "range": {"start": ticks(start), "end": ticks(end or self.duration)},
            "source_offset": 0,
            "transform": placement or transform(),
            "params": {},
            "animations": animations or [],
            "effects": [],
        }
        self.commands.append({"AddClip": {"track": track, "clip": clip}})
        return identity, track

    def text(self, text, x, y, size=0.04, color=WHITE, start=0, end=None, **kw):
        return self.clip(
            text,
            "text",
            {
                "text": text,
                "units": "normalized",
                "align": "center",
                "font_size": size,
                "color": color,
            },
            start,
            end,
            transform(x, y),
            **kw,
        )

    def math(self, source, x, y, start=0, end=None, size=50, track=None, reveal=True):
        # LaTeX uses canvas coordinates, so size is relative to the delivery's short edge.
        return self.clip(
            "Equation",
            "latex",
            {
                "source": source,
                "mode": "latex",
                "units": "normalized",
                "size": size / 1080,
                "color": GOLD,
            },
            start,
            end,
            transform(x, y),
            [curve({"Param": "reveal"}, [(0, 0), (0.8, 1)])] if reveal else [],
            track,
        )

    def shape(
        self,
        name,
        points=None,
        color=BLUE,
        fill=False,
        placement=None,
        animations=None,
        start=0,
        end=None,
        ellipse=False,
        track=None,
        stroke_width=0.006,
    ):
        params = {
            "units": "normalized",
            "shape": "ellipse" if ellipse else "path",
            "path": json.dumps(path_verbs(points, fill)) if points else "[]",
            "x": 0.5,
            "y": 0.5,
            "width": 1.0,
            "height": 1.0,
            "rotation": 0.0,
            "fill": color if fill else [0, 0, 0, 0],
            "stroke": color,
            "stroke_width": stroke_width,
        }
        return self.clip(
            name, "shapes", params, start, end, placement, animations, track
        )

    def flush(self):
        self.session.batch(self.commands)
        self.commands.clear()

    def graph(self, expression, rect, start, end, color=BLUE, draw=1, track=None):
        # Canvas-space graph output fills the canvas before this transform.
        x, y, w, h = rect
        return self.clip(
            "Function plot",
            "graph",
            {
                "items": "plot y=" + expression,
                "x_min": 0,
                "x_max": 2 * math.pi,
                "y_min": -1.5,
                "y_max": 1.5,
                "scale_mode": "stretch",
                "show_grid": True,
                "show_axes": True,
                "show_labels": False,
                "color": color,
                "stroke_width": self.graph_stroke,
                "axis_color": [115, 140, 170, 255],
                "grid_color": [46, 64, 88, 160],
            },
            start,
            end,
            transform(x, y, w, h),
            [curve({"Param": "draw"}, [(0, 0), (draw, 1)])],
            track,
        )

    def line(self, name, points, color=WHITE, **kw):
        return self.shape(name, points=points, color=color, **kw)


def heading(b, number, title, subtitle):
    b.text(f"ANIMATHIO / {number}", 0.5, 0.06, 0.024, MINT)
    title_id, title_track = b.text(title, 0.5, 0.13, 0.038 if b.vertical else 0.063)
    b.commands.append({"RenameTrack": {"id": title_track, "name": "Explainer title"}})
    b.presets.append((title_id, "fade-in", 800))
    b.text(subtitle, 0.5, 0.21, 0.023 if b.vertical else 0.035, BLUE)


def motion(b, name, center, radius, start, end, terms=1):
    """Sample analytic parent positions; rotations themselves use unwrapped linear keys."""
    cx, cy = center
    period = end - start
    samples = (
        240  # all key times are exact integer ticks, 120 intervals per revolution.
    )

    def point(t, count):
        theta = (4 if name == "Unit" else 2) * math.pi * t / period
        return (
            sum(
                4 / (math.pi * n) * math.cos(n * theta) for n in range(1, count * 2, 2)
            ),
            sum(
                4 / (math.pi * n) * math.sin(n * theta) for n in range(1, count * 2, 2)
            ),
        )

    for k in range(terms):
        n = 2 * k + 1
        r = radius * (1 if name == "Unit" else 4 / (math.pi * n))
        values = []
        for j in range(samples + 1):
            t = period * j / samples
            px, py = point(t, k) if k else (0, 0)
            values.append((t, cx + radius * px / b.width, cy - radius * py / b.height))
        xkeys = curve({"Transform": "X"}, [(t, x) for t, x, _ in values])
        ykeys = curve({"Transform": "Y"}, [(t, y) for t, _, y in values])
        place = transform(values[0][1], values[0][2], 2 * r / b.width, 2 * r / b.height)
        if name == "Unit":
            # Exercise a real parametric curve; matching normalized transforms keep it round.
            b.clip(
                "Unit circle",
                "graph",
                {
                    "items": "parametric x=cos(t) y=sin(t)",
                    "x_min": -1,
                    "x_max": 1,
                    "y_min": -1,
                    "y_max": 1,
                    "scale_mode": "stretch",
                    "show_grid": False,
                    "show_axes": False,
                    "show_labels": False,
                    "color": [65, 100, 135, 255],
                    "stroke_width": b.graph_stroke,
                },
                start,
                end,
                place,
                [curve({"Param": "draw"}, [(0, 0), (0.8, 1)])],
            )
        else:
            b.shape(
                f"{name} circle {n}",
                color=[65, 100, 135, 255],
                placement=place,
                animations=[xkeys, ykeys],
                start=start,
                end=end,
                ellipse=True,
            )
        b.line(
            f"Rotating vector {n}",
            [(0.5, 0.5), (1, 0.5)],
            [MINT, GOLD, PINK, BLUE][k % 4],
            placement=place,
            animations=[
                xkeys,
                ykeys,
                curve(
                    {"Transform": "Rotation"},
                    [(0, 0), (period, -(4 if name == "Unit" else 2) * math.pi * n)],
                ),
            ],
            start=start,
            end=end,
        )
    values = []
    for j in range(samples + 1):
        t = period * j / samples
        if name == "Unit":
            px, py = (
                math.cos(4 * math.pi * t / period),
                math.sin(4 * math.pi * t / period),
            )
        else:
            px, py = point(t, terms)
        values.append((t, cx + radius * px / b.width, cy - radius * py / b.height))
    b.shape(
        "Tracing point",
        color=GOLD,
        fill=True,
        ellipse=True,
        start=start,
        end=end,
        placement=transform(
            sx=min(b.width, b.height) / (30 * b.width),
            sy=min(b.width, b.height) / (30 * b.height),
        ),
        animations=[
            curve({"Transform": "X"}, [(t, x) for t, x, _ in values]),
            curve({"Transform": "Y"}, [(t, y) for t, _, y in values]),
        ],
    )
    return values


def projection(b, values, rect, center, radius, start, end):
    """Join a vector sum to the matching graph-domain endpoint."""
    x, y, w, h = rect
    points = [(0.5, 0.5), (1.5, 0.5)]
    # Two endpoints in local normalized space can be animated with shape width plus its position.
    xs, ys, widths, rotations = [], [], [], []
    for t, px, py in values:
        qx = x - w / 2 + w * t / (end - start)
        qy = y - h / 3 * ((center[1] - py) * b.height / radius)
        dx, dy = (qx - px) * b.width, (qy - py) * b.height
        xs.append((t, px))
        ys.append((t, py))
        widths.append((t, math.hypot(dx, dy) / b.width))
        angle = math.atan2(dy, dx)
        if rotations:
            angle += 2 * math.pi * round((rotations[-1][1] - angle) / (2 * math.pi))
        rotations.append((t, angle))
    b.line(
        "Height projection",
        points,
        [110, 145, 180, 160],
        stroke_width=0.0015,
        start=start,
        end=end,
        placement=transform(),
        animations=[
            curve({"Transform": "X"}, xs),
            curve({"Transform": "Y"}, ys),
            curve({"Transform": "ScaleX"}, widths),
            curve({"Transform": "Rotation"}, rotations),
        ],
    )


def captions(b, name, steps, output):
    texts = {
        "pythagorean": [
            "Four equal right triangles surround c squared.",
            "Move the triangles. Keep every area unchanged.",
            "The same space is a squared plus b squared.",
        ],
        "unit-circle": [
            "A point travels around the unit circle.",
            "Project its height onto the graph.",
            "One full turn traces one sine wave.",
        ],
        "fourier": [
            "Start with one rotating vector: the fundamental.",
            "Add the third harmonic at one third the radius.",
            "Add fifth and seventh. The sum approaches a square wave.",
        ],
    }[name]

    # The public subtitle importer gives us wrapping, safe areas and caption styling.
    def stamp(t):
        ms = round(t * 1000)
        return f"00:00:{ms // 1000:02d},{ms % 1000:03d}"

    bounds = [0, steps[1], steps[2], b.duration]
    path = output / "captions.srt"
    path.write_text(
        "\n\n".join(
            f"{i + 1}\n{stamp(a)} --> {stamp(z)}\n{text}"
            for i, (a, z, text) in enumerate(zip(bounds, bounds[1:], texts))
        )
        + "\n",
        encoding="utf-8",
    )
    b.session.tool(
        "import_subtitles",
        path=str(path),
        style="word-highlight",
        font_size=0.035,
        position="bottom",
        max_words=7,
        accent="#5ee4bb",
    )
    # Keep math labels and caption pills in separate bands in these authored layouts.
    state = b.session.tool("get_project_state")["project"]
    edits = []
    for track in state["tracks"]:
        if track["name"].startswith("Subtitles"):
            for clip in track["clips"].values():
                placement = {**clip["transform"], "y": 0.85 if b.vertical else 0.90}
                edits.append(
                    {"SetTransform": {"id": clip["id"], "transform": placement}}
                )
    b.session.batch(edits)


def build(name, args):
    data = manifest(name)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    project = output / (name + ("-9x16" if args.vertical else "-16x9") + ".amio")
    # Start with a real asset-free template; clear its contents through Commands.
    run("new", "--template", "math_explainer", "-o", project)
    with contextlib.closing(Session(project)) as session:
        duration = data["duration_seconds"]
        b = Builder(session, args.vertical, duration, args.small)
        steps = [ms / 1000 for ms in data["steps_ms"]]
        runpy.run_path(str(HERE / name / "build.py"))["scene"](b, steps)
        b.text(
            data["credit"],
            0.5,
            0.975,
            0.012 if b.vertical else 0.021,
            [130, 153, 181, 255],
        )
        b.flush()
        for identity, preset, duration_ms in b.presets:
            applied = session.tool(
                "apply_animation_preset",
                id=str(identity),
                preset=preset,
                duration_ms=duration_ms,
            )
            assert applied["keyframes"] >= 2, applied
        # Pin markers from the real recording; CI never re-detects a substitute soundtrack.
        session.batch(
            [
                {
                    "AddMarker": {
                        "marker": {
                            "id": i + 1,
                            "time": ticks(ms / 1000),
                            "duration": None,
                            "name": f"Music beat {i + 1}",
                            "color": "cyan",
                            "note": "Pinned detection: see music.toml",
                        }
                    }
                }
                for i, ms in enumerate(data["beats_ms"])
            ]
        )
        # A visible progress rail is split into beat-aligned segments through the public tool.
        rail, _ = b.shape(
            "Beat rail",
            [(0, 0), (1, 0), (1, 1), (0, 1)],
            MINT,
            True,
            placement=transform(0.5, 0.245, 0.8, 0.003),
        )
        b.flush()
        synced = session.tool(
            "sync_cuts_to_beats",
            clips=[rail],
            source="markers",
            mode="split",
            frame_align=False,
            min_segment_ms=100,
        )
        if not synced["splits_ms"] or synced["skipped"]:
            raise AssertionError(synced)
        rail_track = next(
            t
            for t in session.tool("get_project_state")["project"]["tracks"]
            if t["name"] == "Beat rail"
        )
        segments = sorted(
            rail_track["clips"].values(), key=lambda c: c["range"]["start"]
        )
        session.batch(
            [
                {
                    "SetTransform": {
                        "id": c["id"],
                        "transform": transform(
                            0.1 + 0.4 * (i + 1) / len(segments),
                            0.245,
                            0.8 * (i + 1) / len(segments),
                            0.003,
                        ),
                    }
                }
                for i, c in enumerate(segments)
            ]
        )
        captions(b, name, steps, output)
        music = None if args.offline else recording(name)
        if args.require_music and music is None:
            raise FileNotFoundError(
                f"Fetch the soundtrack first: python3 examples/showcase/fetch-music.py {name}"
            )
        real_music = music is not None
        if music is None:
            music = output / "click.wav"
            click_track(music, duration, data["beats_ms"])
        audio = session.tool("add_media", path=str(music.resolve()))
        # Trim the original recording and fade out within the explainer, using one undo step.
        identity = audio["clip_id"]
        session.batch(
            [
                {
                    "TrimClip": {
                        "id": identity,
                        "range": {"start": 0, "end": ticks(duration)},
                        "source_offset": (
                            ticks(data["offset_ms"] / 1000) if real_music else 0
                        ),
                    }
                },
                {
                    "AddKeyframe": {
                        "id": identity,
                        "target": {"Param": "gain"},
                        "time": 0,
                        "keyframe": {
                            "value": {"Number": 0.65},
                            "interpolation": "Linear",
                        },
                    }
                },
                {
                    "AddKeyframe": {
                        "id": identity,
                        "target": {"Param": "gain"},
                        "time": ticks(duration - 1),
                        "keyframe": {
                            "value": {"Number": 0.65},
                            "interpolation": "Linear",
                        },
                    }
                },
                {
                    "AddKeyframe": {
                        "id": identity,
                        "target": {"Param": "gain"},
                        "time": ticks(duration),
                        "keyframe": {"value": {"Number": 0}, "interpolation": "Linear"},
                    }
                },
            ]
        )
        # Snapshot state is only inspected, never written as project JSON.
        state = session.tool("get_project_state")["project"]
        assert len(state["markers"]) == len(data["beats_ms"])
        session.tool("save_project", path=str(project))
    run("validate", project)
    print(project)
    return project


def main(name):
    parser = argparse.ArgumentParser(
        description=f"Build the {name} showcase through AniMathIO MCP"
    )
    parser.add_argument("--output", type=Path, default=ROOT / "target/showcase" / name)
    parser.add_argument(
        "--vertical", action="store_true", help="Separate 9:16 vector layout"
    )
    parser.add_argument(
        "--small", action="store_true", help="640x360 / 360x640 CI delivery"
    )
    parser.add_argument(
        "--offline", action="store_true", help="Always use a deterministic click track"
    )
    parser.add_argument(
        "--require-music", action="store_true", help="Fail instead of falling back"
    )
    args = parser.parse_args()
    if args.offline and args.require_music:
        parser.error("--offline conflicts with --require-music")
    build(name, args)
