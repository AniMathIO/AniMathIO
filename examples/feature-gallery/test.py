#!/usr/bin/env python3
"""Rebuild and smoke-test the gallery through the public CLI and MCP tools."""

from __future__ import annotations

import argparse
import contextlib
import json
import subprocess
from dataclasses import dataclass
from pathlib import Path

import numpy as np
from PIL import Image, ImageChops

from clips import audio_samples, registry
from gallery import (
    AUDIO_HALF, AUDIO_RATE, BACKGROUND, BLUE, FPS, HEIGHT, MINT, ROOT, TICKS, WIDTH,
    Session, cli, run,
)
from render import (
    PROJECTS, build_project, enabled_community_plugins, export_videos, project_data,
)


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


def check_frame(project, at, output, *, physics=False):
    result = subprocess.run(
        [cli(), "render", str(project), "--time", f"{at:.3f}", "-o", str(output)],
        capture_output=True, text=True, check=True,
    )
    assert "warning:" not in result.stderr, (project, result.stderr)
    with Image.open(output) as image:
        assert image.size == (WIDTH, HEIGHT), (project, image.size)
        assert image.convert("RGBA").getchannel("A").getextrema() == (255, 255), project
        # Exclude the heading so a title alone cannot pass.
        body = image.convert("RGB").crop((0, HEIGHT // 3, WIDTH, HEIGHT))
        canvas = Image.new("RGB", body.size, tuple(BACKGROUND[:3]))
        assert ImageChops.difference(body, canvas).getbbox(), f"Blank demo: {project}"
        if physics:
            # The released plugin paints the highlighted projectile or launcher amber.
            # Ground, trails and readouts can remain visible when the body leaves frame.
            colors = {rgb: count for count, rgb in body.getcolors(body.width * body.height)}
            assert colors.get((240, 150, 40), 0) > 30, f"Physics body missing: {project} at {at}s"


def probe(media, codec, audio_codec):
    data = json.loads(subprocess.check_output([
        "ffprobe", "-v", "error", "-show_streams", "-show_chapters", "-of", "json", media,
    ]))
    (video,) = [s for s in data["streams"] if s["codec_type"] == "video"]
    (audio,) = [s for s in data["streams"] if s["codec_type"] == "audio"]
    assert video["codec_name"] == codec and audio["codec_name"] == audio_codec, media
    assert audio["channels"] == 2 and int(audio["sample_rate"]) == AUDIO_RATE, media
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


@dataclass(frozen=True)
class AudioCheck:
    label: str
    interval: tuple[float, float]
    band: tuple[int, int]
    min_change: float = -float("inf")
    max_change: float = float("inf")


# Windows are relative to each three-second half and omit note/fade edges.
# The secondary checks make sure the effect ranges from selective EQ and
# dynamics to broadband gating without simply turning the entire clip down.
AUDIO_CHECKS = {
    "gain": [
        AudioCheck("melody", (0.28, 0.56), (150, 4000), min_change=15),
    ],
    "eq3": [
        AudioCheck("nasal ring", (0.30, 0.65), (1480, 1640), max_change=-15),
        AudioCheck("note body", (0.30, 0.65), (220, 400), min_change=0, max_change=5),
    ],
    "compressor": [
        AudioCheck("loud note", (0.91, 1.15), (150, 3500), max_change=2),
        AudioCheck("quiet note", (0.31, 0.53), (150, 3500), min_change=15),
    ],
    "gate": [
        AudioCheck("hiss in gap", (0.78, 0.94), (1700, 6000), max_change=-25),
        AudioCheck("hiss under pulse", (0.38, 0.50), (1700, 6000), min_change=-3, max_change=3),
        AudioCheck("pulse", (0.38, 0.50), (300, 1400), min_change=-2, max_change=2),
    ],
    "highpass": [
        AudioCheck("buzz under melody", (0.44, 0.64), (150, 255), max_change=-15),
        AudioCheck("melody", (0.44, 0.64), (700, 3400), min_change=-3, max_change=2),
    ],
    "voice_cleanup": [
        AudioCheck("sibilance", (0.36, 0.46), (5900, 8100), max_change=-15),
        AudioCheck("buzz in gap", (1.22, 1.52), (150, 260), max_change=-18),
        AudioCheck("voiced syllable", (0.36, 0.46), (345, 1250), min_change=-5, max_change=2),
    ],
    "denoise": [
        AudioCheck("static under note", (0.73, 1.00), (1650, 4400), max_change=-20),
        AudioCheck("static before first note", (0.25, 0.52), (1650, 4400), max_change=-20),
        AudioCheck("static at wet onset", (0.035, 0.10), (1650, 4400), max_change=-20),
        AudioCheck("note body", (0.73, 1.00), (280, 970), min_change=-3, max_change=2),
    ],
}


def band_dbfs(samples, check):
    start, end = (round(seconds * AUDIO_RATE) for seconds in check.interval)
    window = np.hanning(end - start)
    hz = np.fft.rfftfreq(len(window), 1 / AUDIO_RATE)
    lo, hi = check.band
    spectrum = np.fft.rfft(window * samples[start:end])
    # One-sided power, normalized for the FFT length and Hann window energy.
    power = 2 * np.sum(np.abs(spectrum[(hz >= lo) & (hz <= hi)]) ** 2)
    mean_square = power / (len(window) * np.dot(window, window))
    return 10 * np.log10(max(mean_square, 1e-14))


def check_headroom(media):
    # Keep the channels separate and oversample to catch intersample peaks.
    for oversample in (1, 4):
        raw = subprocess.check_output([
            "ffmpeg", "-v", "error", "-i", str(media), "-vn",
            "-af", f"aresample={AUDIO_RATE * oversample}:filter_size=64", "-f", "f32le", "-",
        ])
        pcm = np.frombuffer(raw, dtype="<f4").reshape(-1, 2)
        half = AUDIO_HALF * AUDIO_RATE * oversample
        # Include a constant-power stereo-to-mono fold, as used by FFmpeg.
        mono = pcm.sum(axis=1) / np.sqrt(2)
        for layout, samples in (("stereo", pcm), ("mono fold", mono)):
            dry_peak = np.max(np.abs(samples[:half]))
            wet_peak = np.max(np.abs(samples[half:]))
            result = (f"{media.name} · {oversample}× {layout} peak: "
                      f"dry {dry_peak:.3f}, wet {wet_peak:.3f}")
            assert dry_peak <= 0.89 and wet_peak <= 0.89, result
            print(result, flush=True)


def check_audio(effect, mp4, webm):
    levels_by_codec = []
    for media, codecs in ((mp4, ("h264", "aac")), (webm, ("vp9", "opus"))):
        assert probe(media, *codecs) == [], media
        pcm = np.asarray(audio_samples(media))
        half = AUDIO_HALF * AUDIO_RATE
        # Container padding can extend the decoded stream by one packet.
        assert abs(len(pcm) - 2 * half) <= 1024, (media, len(pcm))
        check_headroom(media)
        if effect == "compressor":
            dry_rms, wet_rms = (
                np.sqrt(np.mean(samples ** 2)) for samples in (pcm[:half], pcm[half:2 * half])
            )
            change = 20 * np.log10(wet_rms / dry_rms)
            result = f"{media.name} · compressor whole-half RMS change: {change:+.1f} dB"
            assert abs(change) <= 3, result
            print(result, flush=True)
        levels = []
        for check in AUDIO_CHECKS[effect]:
            dry, wet = (band_dbfs(samples, check) for samples in (pcm[:half], pcm[half:]))
            change = wet - dry
            result = f"{media.name} · {check.label}: {dry:.1f} → {wet:.1f} dBFS ({change:+.1f} dB)"
            assert dry > -40, f"Dry signal too quiet: {result}"
            assert check.min_change <= change <= check.max_change, result
            print(result, flush=True)
            levels.append((dry, wet))
        levels_by_codec.append(levels)
    np.testing.assert_allclose(*levels_by_codec, atol=2, rtol=0, err_msg=effect)


def without_waveforms(project):
    """Compare the authored project, allowing roundoff in the encoded mix."""
    for track in project["tracks"]:
        if track["name"].endswith(" waveform"):
            for clip in track["clips"].values():
                del clip["kind"]["Plugin"]["params"]["path"]
    return project


def check_waveforms(poster):
    with Image.open(poster) as image:
        pixels = np.asarray(image.convert("RGB"))
    heights = []
    for y, color in ((0.47, BLUE), (0.74, MINT)):
        region = pixels[round(HEIGHT * (y - 0.114)):round(HEIGHT * (y + 0.114)),
                        round(WIDTH * 0.11):round(WIDTH * 0.90)]
        heights.append(np.all(region == color[:3], axis=2).sum(axis=0))
    dry, wet = heights
    assert min(dry.sum(), wet.sum()) > 500, f"Missing waveform: {poster}"
    assert np.abs(dry - wet).sum() > 0.12 * max(dry.sum(), wet.sum()), (
        f"Waveform change too small: {poster}"
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "target/feature-gallery-smoke")
    parser.add_argument("--audio-only", action="store_true", help="check just the audio cards")
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    if output == PROJECTS:
        parser.error("choose an output directory outside the checked-in projects directory")

    cards = registry()
    assert len({c["slug"] for c in cards}) == len(cards), "duplicate card slug"
    assert {p.stem for p in PROJECTS.glob("*.amio")} == {c["slug"] for c in cards}
    check_coverage(cards)
    if args.audio_only:
        cards = [card for card in cards if "audio_effect" in card]
    enabled = enabled_community_plugins() if not args.audio_only else set()

    checked = skipped = 0
    for card in cards:
        slug = card["slug"]
        plugin = card["plugin"]
        if card.get("community") and plugin not in enabled:
            print(f"skip {slug}: {plugin} is not enabled", flush=True)
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
        effect = card.get("audio_effect")
        original = project_data(source)
        if effect:
            assert without_waveforms(original) == without_waveforms(rebuilt), (
                f"Regenerate {source.name}"
            )
        else:
            assert original == rebuilt, f"Regenerate {source.name}"
        poster_time = card.get("poster_time", seconds / 2)
        poster = output / f"{slug}.png"
        physics = plugin == "io.github.animathio.physics"
        check_frame(source if effect else project, poster_time, poster, physics=physics)
        if effect:
            check_waveforms(poster)
            mp4, webm = (output / f"{slug}.{ext}" for ext in ("mp4", "webm"))
            export_videos(source, mp4, webm)
            check_audio(effect, mp4, webm)
        if card.get("community") and poster_time != seconds / 2:
            midpoint = output / f"{slug}-midpoint.png"
            check_frame(project, seconds / 2, midpoint, physics=physics)
            with Image.open(poster) as a, Image.open(midpoint) as b:
                assert ImageChops.difference(a.convert("RGB"), b.convert("RGB")).getbbox(), slug
        if physics:
            check_frame(project, seconds - 1 / FPS, output / f"{slug}-end.png", physics=True)
        checked += 1

    if not args.audio_only:
        check_exports(output)
    print(f"Gallery smoke passed: {checked} cards, {skipped} skipped")


if __name__ == "__main__":
    main()
