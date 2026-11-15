"""Equivalent benchmark content for AniMathIO v1 and v2 (used by scripts/bench-v1-v2.sh).

Both versions get the same 1920x1080, 30 fps scene on the same dark background:

* a title text ("Fourier series"), fading in over one second
* a typeset LaTeX formula (v1: KaTeX raster; v2: the latex plugin), fading in over one second
* a graph with axes, a sin(x) curve, a circle and a triangle (v1: one static Mafs-style raster
  image, because v1 has no vector graph/shape element; v2: graph + shapes plugin clips)

The "medium" project is 50 text elements and 10 LaTeX formulas spread over 60 seconds.
"""

from __future__ import annotations

import copy
import json
import math
import subprocess
import zipfile
from pathlib import Path

TICKS_PER_SECOND = 705_600_000
WIDTH, HEIGHT, FPS = 1920, 1080, 30
BACKGROUND = (16, 19, 26, 255)
BACKGROUND_HEX = "#10131a"
TITLE = "Fourier series"
FORMULA = r"\sum_{n=1}^{\infty}\frac{1}{n^2}=\frac{\pi^2}{6}"

# Layout shared by both versions (canvas pixels, top-left origin for v1, normalized
# clip-centre fractions for v2; the v2 values below are the pixel positions converted).
GRAPH_SCALE = 0.45
GRAPH_W, GRAPH_H = int(WIDTH * GRAPH_SCALE), int(HEIGHT * GRAPH_SCALE)  # 864 x 486
GRAPH_CENTER = (0.7, 0.55)  # normalized, v2 transform
GRAPH_TOP_LEFT = (
    round(GRAPH_CENTER[0] * WIDTH - GRAPH_W / 2),
    round(GRAPH_CENTER[1] * HEIGHT - GRAPH_H / 2),
)
# Approximate clip-centre fractions for v2 matching the v1 top-left pixel positions
# (exact font metrics differ between KaTeX raster and the v2 plugins; same region).
TITLE_CENTER = (0.19, 0.115)   # v1 title drawn top-left at (120, 80), ~76 px line
FORMULA_CENTER = (0.17, 0.43)  # v1 formula drawn top-left at (120, 400)


def render_graph_png(path: Path) -> None:
    """A static raster of the v2 graph scene (what a v1 Mafs capture is: a PNG)."""
    from PIL import Image, ImageDraw, ImageFont

    ss = 2
    w, h = GRAPH_W * ss, GRAPH_H * ss
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    unit = 0.1 * HEIGHT * GRAPH_SCALE * ss  # v2 maps -5..5 onto the canvas height; units are square
    ox, oy = w / 2, h / 2
    grid = (118, 185, 255, 90)
    axis = (190, 205, 230, 220)
    curve = (118, 185, 255, 255)
    k = 0
    while ox + k * unit <= w:
        for sign in (1, -1):
            x = ox + sign * k * unit
            if 0 <= x <= w:
                d.line([(x, 0), (x, h)], fill=axis if k == 0 else grid, width=2 * ss if k == 0 else ss)
        k += 1
    k = 0
    while oy + k * unit <= h:
        for sign in (1, -1):
            y = oy + sign * k * unit
            if 0 <= y <= h:
                d.line([(0, y), (w, y)], fill=axis if k == 0 else grid, width=2 * ss if k == 0 else ss)
        k += 1
    try:
        font = ImageFont.load_default(size=11 * ss)
        for n in range(-8, 9, 2):
            if n:
                d.text((ox + n * unit + 3 * ss, oy + 3 * ss), str(n), fill=axis, font=font)
        for n in (-4, -2, 2, 4):
            d.text((ox + 3 * ss, oy - n * unit - 12 * ss), str(n), fill=axis, font=font)
    except Exception:  # fonts are cosmetic; the benchmark does not depend on them
        pass
    pts = []
    steps = 600
    for i in range(steps + 1):
        x = -5.0 - 4.0 + 18.0 * i / steps  # visible x range is about +-8.9 units
        pts.append((ox + x * unit, oy - math.sin(x) * unit))
    d.line(pts, fill=curve, width=3 * ss, joint="curve")
    r = 2 * unit
    d.ellipse([ox - r, oy - r, ox + r, oy + r], outline=curve, width=2 * ss)
    tri = [(ox - 2 * unit, oy + unit), (ox + 2 * unit, oy + unit), (ox, oy - 2 * unit)]
    d.line(tri + [tri[0]], fill=curve, width=2 * ss, joint="curve")
    img = img.resize((GRAPH_W, GRAPH_H), Image.LANCZOS)
    path.parent.mkdir(parents=True, exist_ok=True)
    img.save(path)


# ----------------------------------------------------------------------------------- v2 projects


def _write_amio(path: Path, project: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("project.json", json.dumps(project))


def _read_amio(path: Path) -> dict:
    with zipfile.ZipFile(path) as z:
        return json.loads(z.read("project.json"))


def _empty_v2(seconds: int) -> dict:
    return {
        "format": "animathio",
        "schemaVersion": 7,
        "frame_rate": {"numerator": FPS, "denominator": 1},
        "tracks": [],
        "assets": {},
        "width": WIDTH,
        "height": HEIGHT,
        "background": list(BACKGROUND),
        "duration": seconds * TICKS_PER_SECOND,
    }


def _fade_in(seconds: float = 1.0) -> list:
    keys = {
        "0": {"value": {"Number": 0.0}, "interpolation": "EaseInOut"},
        str(int(seconds * TICKS_PER_SECOND)): {"value": {"Number": 1.0}, "interpolation": "EaseInOut"},
    }
    return [{"target": {"Transform": "Opacity"}, "keys": keys}]


def _cli(cli: Path, env: dict, *args: str) -> None:
    subprocess.run([str(cli), *args], env=env, check=True, stdout=subprocess.DEVNULL)


def build_v2_scene(cli: Path, env: dict, out: Path, seconds: int, work: Path) -> Path:
    """The equivalent scene as an `.amio`, authored through `animathio-cli math-objects add`."""
    work.mkdir(parents=True, exist_ok=True)
    cur = work / f"scene-{seconds}-0.amio"
    _write_amio(cur, _empty_v2(seconds))
    steps = [
        ("text", {"source": TITLE, "size": "7"}),
        ("latex", {"source": FORMULA, "size": "10"}),
        ("plot", {"expression": "sin(x)"}),
        ("circle", {"radius": "2", "x": "0", "y": "0"}),
        ("polygon", {"vertices": "-2,-1; 2,-1; 0,2"}),
    ]
    for i, (kind, fields) in enumerate(steps, 1):
        nxt = work / f"scene-{seconds}-{i}.amio"
        args = ["math-objects", "add", str(cur), "--kind", kind, "-o", str(nxt)]
        for key, value in fields.items():
            args += ["--field", f"{key}={value}"]
        _cli(cli, env, *args)
        cur = nxt
    project = _read_amio(cur)
    project["duration"] = seconds * TICKS_PER_SECOND
    project["width"], project["height"] = WIDTH, HEIGHT
    project["background"] = list(BACKGROUND)
    placement = [
        (*TITLE_CENTER, 1.0),  # title
        (*FORMULA_CENTER, 1.0),  # formula
        (*GRAPH_CENTER, GRAPH_SCALE),  # plot
        (*GRAPH_CENTER, GRAPH_SCALE),  # circle
        (*GRAPH_CENTER, GRAPH_SCALE),  # triangle
    ]
    for track, (x, y, scale) in zip(project["tracks"], placement, strict=True):
        for clip in track["clips"].values():
            clip["range"] = {"start": 0, "end": seconds * TICKS_PER_SECOND}
            clip["transform"].update(x=x, y=y, scale_x=scale, scale_y=scale, opacity=1.0)
            clip["animations"] = _fade_in()
    _write_amio(out, project)
    return out


def build_v2_medium(cli: Path, env: dict, out: Path, work: Path) -> Path:
    """50 text + 10 LaTeX clips over 60 s, 6 tracks (an editor-sized project, no media)."""
    work.mkdir(parents=True, exist_ok=True)
    base = work / "medium-0.amio"
    _write_amio(base, _empty_v2(60))
    a = work / "medium-1.amio"
    b = work / "medium-2.amio"
    _cli(cli, env, "math-objects", "add", str(base), "--kind", "text", "-o", str(a))
    _cli(cli, env, "math-objects", "add", str(a), "--kind", "latex", "-o", str(b))
    template = _read_amio(b)
    text_track, latex_track = template["tracks"]
    text_clip = next(iter(text_track["clips"].values()))
    latex_clip = next(iter(latex_track["clips"].values()))
    project = _empty_v2(60)
    clip_id = 0
    tracks = []
    slot = 5 * TICKS_PER_SECOND
    for t in range(6):
        track = {k: copy.deepcopy(v) for k, v in text_track.items() if k != "clips"}
        track.update(id=t + 1, name=f"Track {t + 1}", clips={})
        for s in range(10):
            clip_id += 1
            latex = (t * 10 + s) % 6 == 0
            clip = copy.deepcopy(latex_clip if latex else text_clip)
            clip["id"] = clip_id
            clip["link_group"] = None
            clip["range"] = {"start": s * slot + (t % 2) * slot // 2, "end": s * slot + (t % 2) * slot // 2 + slot}
            clip["range"]["end"] = min(clip["range"]["end"], 60 * TICKS_PER_SECOND)
            clip["transform"].update(x=0.15 + 0.14 * t, y=0.15 + 0.07 * s)
            clip["animations"] = _fade_in(0.5)
            params = clip["kind"]["Plugin"]["params"]
            if latex:
                params["source"] = rf"\frac{{{clip_id}}}{{{clip_id + 1}}}+\sqrt{{{clip_id}}}"
                params.pop("_math_object", None)
            else:
                params["text"] = f"Element {clip_id}"
                params.pop("_math_object", None)
            track["clips"][str(clip_id)] = clip
        # clips on one track must not overlap: drop any that would
        spans = sorted(track["clips"].values(), key=lambda c: c["range"]["start"])
        keep, last_end = {}, 0
        for c in spans:
            if c["range"]["start"] >= last_end and c["range"]["end"] > c["range"]["start"]:
                keep[str(c["id"])] = c
                last_end = c["range"]["end"]
        track["clips"] = keep
        tracks.append(track)
    project["tracks"] = tracks
    project["background"] = list(BACKGROUND)
    _write_amio(out, project)
    return out


# ----------------------------------------------------------------------------------- v1 projects


def write_v1_seed(path: Path, seconds: int = 10) -> Path:
    """A minimal zlib-deflated v1 `.animathio` (the ProjectStore serialize shape) so the
    benchmark can open *something* before driving set_canvas/add_* through MCP."""
    import zlib

    state = {
        "backgroundColor": BACKGROUND_HEX,
        "selectedMenuOption": "Videos",
        "audios": [],
        "videos": [],
        "images": [],
        "editorElements": [],
        "maxTime": seconds * 1000,
        "animations": [],
        "currentKeyFrame": 0,
        "fps": FPS,
        "selectedVideoFormat": "mp4",
        "canvas_width": WIDTH,
        "canvas_height": HEIGHT,
        "mediaFiles": [],
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(zlib.compress(json.dumps(state).encode("utf-8"), 9))
    return path


def v1_scene_steps(graph_png: Path, seconds: int) -> list[dict]:
    """MCP tool calls that build the equivalent scene in v1 (run once, then saved)."""
    ms = seconds * 1000
    steps = [
        {"tool": "set_canvas", "args": {"width": WIDTH, "height": HEIGHT, "backgroundColor": BACKGROUND_HEX, "maxTimeMs": ms}},
        {"tool": "add_text", "args": {"text": TITLE, "fontSize": 76, "fontWeight": 700, "x": 120, "y": 80, "color": "#ffffff"}},
        {"tool": "add_math", "args": {"latex": FORMULA, "x": 120, "y": 400, "color": "#ffffff"}},
        {"tool": "add_media", "args": {"path": str(graph_png)}},
    ]
    for index in (1, 2):  # ids come back from steps 1..2 (0 is set_canvas)
        steps.append(
            {"tool": "update_element", "args": {"id": f"${index}.id", "timeFrame": {"start": 0, "end": ms}}}
        )
    # v1 has no vector graph or shape elements: the graph, circle and triangle ship as
    # one static raster image (what a Mafs capture is). Place it where v2 puts its
    # graph+shapes clips. `$3.ids.0` is the image timeline element created by add_media.
    steps.append(
        {
            "tool": "update_element",
            "args": {
                "id": "$3.ids.0",
                "placement": {"x": GRAPH_TOP_LEFT[0], "y": GRAPH_TOP_LEFT[1], "width": GRAPH_W, "height": GRAPH_H},
                "timeFrame": {"start": 0, "end": ms},
            },
        }
    )
    return steps


def v1_medium_steps(seconds: int = 60) -> list[dict]:
    ms = seconds * 1000
    steps = [{"tool": "set_canvas", "args": {"width": WIDTH, "height": HEIGHT, "backgroundColor": BACKGROUND_HEX, "maxTimeMs": ms}}]
    n = 0
    for t in range(6):
        for s in range(10):
            n += 1
            start = (s * 5 + (t % 2) * 2.5) * 1000
            frame = {"start": start, "end": min(ms, start + 5000)}
            x = 0.05 * WIDTH + 0.14 * t * WIDTH
            y = 0.1 * HEIGHT + 0.07 * s * HEIGHT
            if (t * 10 + s) % 6 == 0:
                steps.append({"tool": "add_math", "args": {"latex": rf"\frac{{{n}}}{{{n + 1}}}+\sqrt{{{n}}}", "x": x, "y": y, "color": "#ffffff"}})
            else:
                steps.append({"tool": "add_text", "args": {"text": f"Element {n}", "fontSize": 32, "x": x, "y": y, "color": "#ffffff"}})
            idx = len(steps) - 1
            steps.append({"tool": "update_element", "args": {"id": f"${idx}.id", "timeFrame": frame}})
            steps.append({"tool": "add_animation", "args": {"targetId": f"${idx}.id", "type": "fadeIn", "durationMs": 500}})
    return steps


# ----------------------------------------------------------------------------------- builder


def build(out: Path, cli: Path, plugins: Path | None, save_dir: Path) -> dict:
    """Generate everything scripts/bench-v1-v2.sh needs:
    the shared graph raster, the v1 seed project, the v1 MCP step scripts and the v2
    `.amio` projects (authored through `animathio-cli math-objects add`)."""
    import os

    graph_png = out / "graph.png"
    render_graph_png(graph_png)

    seed = write_v1_seed(out / "v1-seed.animathio")

    steps = {
        "steps-v1-scene-10.json": v1_scene_steps(graph_png, 10) + [{"tool": "save_project", "args": {"path": str(save_dir / "scene-10.animathio")}}],
        "steps-v1-scene-60.json": v1_scene_steps(graph_png, 60) + [{"tool": "save_project", "args": {"path": str(save_dir / "scene-60.animathio")}}],
        "steps-v1-medium.json": v1_medium_steps(60) + [{"tool": "save_project", "args": {"path": str(save_dir / "medium.animathio")}}],
    }
    for name, body in steps.items():
        (out / name).write_text(json.dumps(body, indent=1), encoding="utf-8")

    env = dict(os.environ)
    if plugins is not None:
        env["ANIMATHIO_PLUGIN_DIR"] = str(plugins)
    work = out / "v2-work"
    v2 = {
        "scene-10.amio": build_v2_scene(cli, env, out / "scene-10.amio", 10, work),
        "scene-60.amio": build_v2_scene(cli, env, out / "scene-60.amio", 60, work),
        "medium.amio": build_v2_medium(cli, env, out / "medium.amio", work),
    }
    return {"graph": str(graph_png), "seed": str(seed), "v1_steps": {k: str(out / k) for k in steps}, "v2": {k: str(v) for k, v in v2.items()}}


def main() -> None:
    import argparse

    ap = argparse.ArgumentParser(description=__doc__)
    sub = ap.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build", help="Generate graph raster, v1 seed/steps and v2 projects.")
    b.add_argument("--out", required=True, type=Path)
    b.add_argument("--cli", required=True, type=Path, help="animathio-cli binary")
    b.add_argument("--plugins", type=Path, default=None, help="ANIMATHIO_PLUGIN_DIR for the CLI")
    b.add_argument("--save-dir", required=True, type=Path, help="Where v1 save_project steps write")
    args = ap.parse_args()
    print(json.dumps(build(args.out, args.cli, args.plugins, args.save_dir)))


if __name__ == "__main__":
    main()
