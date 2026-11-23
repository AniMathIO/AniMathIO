#!/usr/bin/env python3
"""Rebuild and smoke-test the gallery through the public CLI and MCP tools."""

from __future__ import annotations

import argparse
import contextlib
import json
import subprocess
from pathlib import Path

from PIL import Image, ImageChops

from clips import audio_samples, registry
from gallery import BACKGROUND, HEIGHT, ROOT, TICKS, WIDTH, Session, cli, run
from render import PROJECTS, build_project, community_available, project_data


def check_coverage(cards):
    with contextlib.closing(Session(PROJECTS / "latex-inline-color.amio")) as session:
        for tool, plugin, field, card_key in (
            ("list_effects", "effects-basic", "effect", "video_effect"),
            ("list_audio_effects", "audio-basic", "effect", "audio_effect"),
        ):
            loaded = {e[field] for e in session.tool(tool)["effects"]
                      if e["plugin"] == f"org.animathio.{plugin}"}
            shown = {c[card_key] for c in cards if card_key in c}
            assert loaded == shown, (plugin, loaded - shown, shown - loaded)
        loaded = {p["preset"] for p in session.tool("list_animation_presets")["presets"]
                  if p["plugin"] == "org.animathio.animations"}
        shown = {preset for c in cards for preset in c.get("presets", ())}
        assert loaded == shown, (loaded - shown, shown - loaded)


def check_frame(project, at, output):
    result = subprocess.run(
        [cli(), "render", str(project), "--time", f"{at:.3f}", "-o", str(output)],
        capture_output=True, text=True, check=True,
    )
    assert "warning:" not in result.stderr, (project, result.stderr)
    with Image.open(output) as image:
        assert image.size == (WIDTH, HEIGHT), (project, image.size)
        assert image.convert("RGBA").getchannel("A").getextrema() == (255, 255), project
        # Exclude the heading so an empty demonstration cannot pass on its title.
        body = image.convert("RGB").crop((0, HEIGHT // 3, WIDTH, HEIGHT))
        canvas = Image.new("RGB", body.size, tuple(BACKGROUND[:3]))
        assert ImageChops.difference(body, canvas).getbbox(), f"Blank demo: {project}"


def probe(media, codec, audio_codec):
    data = json.loads(subprocess.check_output([
        "ffprobe", "-v", "error", "-show_streams", "-show_chapters", "-of", "json", media,
    ]))
    (video,) = [s for s in data["streams"] if s["codec_type"] == "video"]
    (audio,) = [s for s in data["streams"] if s["codec_type"] == "audio"]
    assert video["codec_name"] == codec and audio["codec_name"] == audio_codec, media
    assert (video["width"], video["height"]) == (WIDTH, HEIGHT), data
    subprocess.run(["ffmpeg", "-v", "error", "-xerror", "-i", media, "-f", "null", "-"],
                   check=True)
    return data["chapters"]


def check_exports(output):
    mp4 = output / "mp4-chapters.mp4"
    text = output / "chapters.txt"
    run("export", output / "mp4-chapters.amio", "-o", mp4, "--chapters", text,
        "--fps", 8, "--overwrite")
    chapters = probe(mp4, "h264", "aac")
    assert [(float(c["start_time"]), c["tags"]["title"]) for c in chapters] == [
        (0, "Intro"), (2, "The theorem"), (4, "The proof"),
    ], chapters
    # The text sidecar follows YouTube's 10-second minimum.
    assert text.read_text(encoding="utf-8").strip() == "0:00 Intro"

    webm = output / "webm-export.webm"
    run("export", output / "webm-export.amio", "--format", "webm", "-o", webm,
        "--fps", 8, "--overwrite")
    assert probe(webm, "vp9", "opus") == [], webm

    sample = output / "audio-gain.mp4"
    run("export", output / "audio-gain.amio", "-o", sample, "--fps", 8, "--overwrite")
    probe(sample, "h264", "aac")
    pcm = audio_samples(sample)
    dry = max(abs(value) for value in pcm[:24000])
    wet = max(abs(value) for value in pcm[24000:])
    assert 1000 < dry < wet / 1.5, (dry, wet)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "target/feature-gallery-smoke")
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    if output == PROJECTS:
        parser.error("choose an output directory outside the checked-in projects directory")

    cards = registry()
    assert len({c["slug"] for c in cards}) == len(cards), "duplicate card slug"
    assert {p.stem for p in PROJECTS.glob("*.amio")} == {c["slug"] for c in cards}
    check_coverage(cards)
    have_community = community_available()

    checked = skipped = 0
    for card in cards:
        slug = card["slug"]
        if card.get("community") and not have_community:
            print(f"skip {slug}: Code Snippets is not enabled", flush=True)
            skipped += 1
            continue
        print(f"== {slug}", flush=True)
        source = PROJECTS / f"{slug}.amio"
        project = output / source.name
        build_project(card, project)
        rebuilt = project_data(project)
        seconds = rebuilt["duration"] / TICKS
        assert 3 <= seconds <= 6 and seconds == card["duration"], (slug, seconds)
        assert rebuilt["tracks"], slug
        if not card.get("audio_effect"):
            # Audio waveform values derive from a lossy export, which may vary slightly
            # between platforms. The audio project is verified by the export smoke test.
            assert project_data(source) == rebuilt, f"Regenerate {source.name}"
        check_frame(project, card.get("poster_time", seconds / 2), output / f"{slug}.png")
        checked += 1

    check_exports(output)
    print(f"Gallery smoke passed: {checked} cards, {skipped} skipped")


if __name__ == "__main__":
    main()
