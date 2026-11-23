#!/usr/bin/env python3
"""Export gallery projects as H.264 MP4, VP9 WebM and WebP posters."""

from __future__ import annotations

import argparse
import contextlib
import json
import subprocess
import tempfile
import zipfile
from pathlib import Path

from clips import registry
from gallery import BACKGROUND, FPS, HEIGHT, HERE, ROOT, TICKS, WIDTH, Builder, Session, cli, run

PROJECTS = HERE / "projects"


def enabled_community_plugins():
    result = subprocess.check_output([cli(), "plugins", "list"], text=True)
    header, _, rows = result.partition("\n")
    if not header.startswith("community plugins: on"):
        return set()
    return {row[0] for line in rows.splitlines()
            if len(row := line.split("\t")) >= 3 and row[2] == "enabled"}


def build_project(card, project):
    """Author one project through MCP or the Manim importer."""
    project.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="author-", dir=project.parent) as folder:
        work = Path(folder)
        draft = work / project.name
        if card.get("manim"):
            run("import-manim", HERE / "manim" / "scene.py", "-o", draft)
            with contextlib.closing(Session(draft)) as session:
                data = session.tool("get_project_state")["project"]
                session.batch([{"SetSettings": {
                    "width": WIDTH, "height": HEIGHT, "frame_rate": data["frame_rate"],
                    "duration": data["duration"], "background": BACKGROUND,
                }}])
                session.tool("save_project", path=str(draft))
        else:
            run("new", "--template", "math_explainer", "-o", draft)
            with contextlib.closing(Session(draft)) as session:
                builder = Builder(session, card["duration"])
                card["build"](builder, work)
                builder.flush()
                builder.save(draft)
        run("validate", draft)
        draft.replace(project)


def project_data(project):
    with zipfile.ZipFile(project) as archive:
        return json.loads(archive.read("project.json"))


def render_card(card, project, output):
    slug = card["slug"]
    videos = output / "videos"
    images = output / "img"
    videos.mkdir(exist_ok=True)
    images.mkdir(exist_ok=True)
    mp4 = videos / f"{slug}.mp4"
    webm = videos / f"{slug}.webm"
    run("export", project, "-o", mp4, "--crf", 27, "--fps", FPS, "--overwrite")
    run("export", project, "-o", webm, "--crf", 31, "--fps", FPS, "--overwrite")

    seconds = project_data(project)["duration"] / TICKS
    at = card.get("poster_time", seconds / 2)
    poster = images / f"{slug}.webp"
    with tempfile.TemporaryDirectory(prefix="poster-", dir=output) as folder:
        still = Path(folder) / "poster.png"
        run("render", project, "--time", f"{at:.3f}", "-o", still)
        subprocess.run(
            ["ffmpeg", "-v", "error", "-y", "-i", str(still),
             "-vf", "scale=800:450", "-q:v", "78", str(poster)],
            check=True,
        )

    entry = {key: card[key] for key in ("slug", "title", "kinds", "plugin", "docs", "summary")}
    entry.update({key: card[key] for key in ("plugin_name", "release") if key in card})
    entry["seconds"] = seconds
    entry.update({key: path.relative_to(output).as_posix() for key, path in (
        ("mp4", mp4), ("webm", webm), ("poster", poster),
    )})
    return entry


def main():
    cards = registry()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "target/feature-gallery")
    parser.add_argument("--only", action="append", choices=[c["slug"] for c in cards],
                        help="render just this card (repeat to select more)")
    parser.add_argument("--rebuild-projects", action="store_true",
                        help="refresh the selected editable .amio sources before rendering")
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)

    selected = [c for c in cards if not args.only or c["slug"] in args.only]
    enabled = enabled_community_plugins() if any(c.get("community") for c in selected) else set()
    entries = []
    for card in selected:
        slug = card["slug"]
        plugin = card["plugin"]
        if card.get("community") and plugin not in enabled:
            if args.only:
                parser.error(f"install and enable {plugin} to render {slug}")
            print(f"skip {slug}: {plugin} is not enabled", flush=True)
            continue
        print(f"== {slug}", flush=True)
        project = PROJECTS / f"{slug}.amio"
        if args.rebuild_projects:
            build_project(card, project)
        entries.append(render_card(card, project, output))

    manifest = output / "manifest.json"
    manifest.write_text(json.dumps(entries, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    total = sum(
        (output / e[key]).stat().st_size
        for e in entries for key in ("mp4", "webm", "poster")
    )
    print(f"{len(entries)} cards, {total / 1e6:.1f} MB → {manifest}")


if __name__ == "__main__":
    main()
