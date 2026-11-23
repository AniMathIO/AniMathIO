"""Shared mechanics for the feature gallery (examples/feature-gallery).

Like `examples/showcase/common.py`, everything is authored through the shipped
`animathio-cli` and its headless MCP server: Python builds serialized
**Commands**, never project JSON. Each gallery card is one small 16:9 project
(see `clips.py` for the registry and `render.py` / `test.py` for the runners).

Headless authoring must stay silent even outside Xvfb; importing this module
pins the same audio-free environment the showcase uses.
"""

from __future__ import annotations

import array
import json
import math
import random
import sys
import wave
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "showcase"))
from common import ROOT, TICKS, Session, cli, run, ticks

# The editor's dark canvas and the accent palette shared by every card.
BACKGROUND = [12, 19, 35, 255]
WHITE = [235, 242, 255, 255]
BLUE = [87, 178, 255, 255]
GOLD = [255, 202, 102, 255]
MINT = [94, 228, 187, 255]
PINK = [240, 132, 183, 255]
MUTED = [130, 153, 181, 255]

# Gallery delivery: 960x540 at 30 fps keeps every card well under a megabyte.
WIDTH = 960
HEIGHT = 540
FPS = 30


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
    """A keyframe curve over a target such as `{"Param": "reveal"}`."""
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
    """Authors one gallery card through Commands at the gallery delivery size."""

    def __init__(self, session, duration, width=WIDTH, height=HEIGHT):
        self.session = session
        self.duration = duration
        self.width = width
        self.height = height
        self.commands = []
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
                        "frame_rate": {"numerator": FPS, "denominator": 1},
                        "background": BACKGROUND,
                        "duration": ticks(duration),
                    }
                }
            ]
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
        # Community plugin ids pass through; first-party names are prefixed.
        plugin_id = plugin if "." in plugin else "org.animathio." + plugin
        clip = {
            "id": identity,
            "kind": {"Plugin": {"plugin_id": plugin_id, "params": params}},
            "range": {"start": ticks(start), "end": ticks(self.duration if end is None else end)},
            "source_offset": 0,
            "transform": placement or transform(),
            "params": {},
            "animations": animations or [],
            "effects": [],
        }
        self.commands.append({"AddClip": {"track": track, "clip": clip}})
        return identity, track

    def text(self, text, x, y, size=0.045, color=WHITE, start=0, end=None, **kw):
        return self.clip(
            text if len(text) < 40 else "Text",
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

    def math(self, source, x, y, start=0, end=None, size=0.046, color=GOLD, **kw):
        # The LaTeX plugin typesets in canvas space; size is a short-edge fraction.
        return self.clip(
            "Equation",
            "latex",
            {
                "source": source,
                "mode": "latex",
                "units": "normalized",
                "size": size,
                "color": color,
            },
            start,
            end,
            transform(x, y),
            **kw,
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

    def graph(
        self,
        items,
        rect,
        start,
        end,
        color=BLUE,
        draw=1.0,
        labels=False,
        track=None,
        y_range=(-1.5, 1.5),
        x_range=None,
    ):
        if x_range is None:
            x_range = (0.0, 6.283185307179586)
        x, y, w, h = rect
        short = min(self.width, self.height)
        return self.clip(
            "Graph",
            "graph",
            {
                "items": items,
                "x_min": x_range[0],
                "x_max": x_range[1],
                "y_min": y_range[0],
                "y_max": y_range[1],
                "scale_mode": "stretch",
                "show_grid": True,
                "show_axes": True,
                "show_labels": labels,
                "color": color,
                "stroke_width": 3 * short / 360,
                "axis_color": WHITE,
                "grid_color": [46, 64, 88, 160],
            },
            start,
            end,
            transform(x, y, w, h),
            [curve({"Param": "draw"}, [(0, 0), (draw, 1)])],
            track,
        )

    def label(self, name, x, y, size=0.02, color=MUTED, start=0, end=None):
        """The small footer naming the plugin or effect under demonstration."""
        return self.text(name, x, y, size, color, start, end)

    def heading(self, title, subtitle=None):
        """Title and optional context above the demonstration."""
        self.text(title, 0.5, 0.14, 0.064)
        if subtitle:
            self.text(subtitle, 0.5, 0.23, 0.034, BLUE)

    def preset(self, identity, preset, duration_ms=None, **options):
        arguments = {"id": str(identity), "preset": preset}
        if duration_ms is not None:
            arguments["duration_ms"] = duration_ms
        if options:
            arguments["options"] = options
        applied = self.session.tool("apply_animation_preset", **arguments)
        assert applied["keyframes"] >= 2, applied
        return applied

    def markers(self, entries):
        """entries: list of (seconds, name); ids are assigned in order."""
        self.commands.extend(
            [
                {
                    "AddMarker": {
                        "marker": {
                            "id": i + 1,
                            "time": ticks(t),
                            "duration": None,
                            "name": name,
                            "color": "cyan",
                            "note": "Feature gallery chapter marker",
                        }
                    }
                }
                for i, (t, name) in enumerate(entries)
            ]
        )

    def flush(self):
        self.session.batch(self.commands)
        self.commands.clear()

    def sync_tracks(self):
        """Reserve the IDs and positions allocated by the high-level MCP tools."""
        tracks = self.session.tool("get_project_state")["project"]["tracks"]
        self.track_index = len(tracks)
        for track in tracks:
            self.next_id = max(self.next_id, track["id"], *map(int, track["clips"]))

    def save(self, project):
        # Snapshot state is only inspected, never written as project JSON.
        state = self.session.tool("get_project_state")["project"]
        assert state["tracks"], "empty gallery card"
        self.session.tool("save_project", path=str(project))


AUDIO_HALF = 3
AUDIO_RATE = 48_000

# (Start, length, frequency, peak level) for the voice-like or musical pulses.
# No noise floor is added to cards whose effect only changes the signal itself.
AUDIO_NOTES = {
    "gain": [(0.20, 0.44, 392, 0.11), (0.80, 0.49, 494, 0.105),
             (1.44, 0.45, 587, 0.12), (2.04, 0.68, 494, 0.115)],
    "eq3": [(0.20, 0.56, 260, 0.17), (0.93, 0.49, 312, 0.18),
            (1.58, 0.48, 390, 0.16), (2.23, 0.55, 260, 0.17)],
    "compressor": [(0.20, 0.46, 440, 0.042), (0.80, 0.46, 440, 0.86),
                   (1.40, 0.46, 494, 0.038), (2.00, 0.65, 494, 0.80)],
    "gate": [(0.32, 0.26, 350, 0.51), (1.02, 0.26, 430, 0.54),
             (1.72, 0.31, 320, 0.48), (2.43, 0.30, 390, 0.56)],
    "highpass": [(0.35, 0.39, 784, 0.36), (0.97, 0.39, 880, 0.35),
                 (1.60, 0.39, 988, 0.37), (2.26, 0.47, 880, 0.35)],
    "voice_cleanup": [(0.26, 0.50, 370, 0.41), (1.63, 0.51, 415, 0.41),
                      (2.40, 0.44, 330, 0.38)],
    "denoise": [(0.62, 0.50, 315, 0.45), (1.48, 0.50, 360, 0.46),
                (2.28, 0.48, 410, 0.43)],
}

# Leave room for processing and peaks introduced by the delivery codecs.
AUDIO_FIXTURE_GAIN_DB = -4.5


def audio_fixture(path, effect):
    """Build one three-second mono WAV, repeated unchanged in the two clips."""
    rng = random.Random(20261123)
    rate = AUDIO_RATE
    samples = [0.0] * (AUDIO_HALF * rate)

    for start, length, frequency, level in AUDIO_NOTES[effect]:
        first, count = round(start * rate), round(length * rate)
        for i in range(count):
            t = i / rate
            pulse = math.tau * frequency * t
            envelope = min(1, t / 0.045, (length - t) / 0.07)
            voiced = (math.sin(pulse) + 0.33 * math.sin(2 * pulse)
                      + 0.13 * math.sin(3 * pulse)) / 1.46
            sample = level * voiced
            if effect == "eq3":
                # The same 1.56 kHz overtone rings through each changing note.
                sample += 0.38 * math.sin(math.tau * 1560 * t)
            samples[first + i] = envelope * sample

    if effect == "voice_cleanup":
        for start in (0.31, 1.68, 2.45):
            first, count = round(start * rate), round(0.22 * rate)
            for i, noise in enumerate(band_noise(rng, count, 7300, 1450)):
                t = i / rate
                envelope = min(1, t / 0.022, (0.22 - t) / 0.045)
                samples[first + i] += 1.40 * envelope * noise

    if effect == "denoise":
        # Audio starts quietly while the spectral mask settles. The profile
        # is learned from the steady bed before the first note.
        for i, noise in enumerate(band_noise(rng, len(samples), 2750, 925)):
            t = i / rate
            fade = min(1, t / 0.15, (AUDIO_HALF - t) / 0.06)
            samples[i] += 0.83 * fade * noise

    if effect in {"gate", "highpass", "voice_cleanup"}:
        for i in range(len(samples)):
            t = i / rate
            fade = min(1, t / 0.035, (AUDIO_HALF - t) / 0.045)
            if effect == "gate":
                noise = rng.uniform(-0.074, 0.074)
            elif effect == "highpass":
                noise = (0.36 * math.sin(math.tau * 180 * t)
                         + 0.10 * math.sin(math.tau * 235 * t))
            else:
                noise = 0.060 * math.sin(math.tau * 175 * t)
            samples[i] += fade * noise

    assert max(map(abs, samples)) < 1, f"Clipped {effect} fixture"
    gain = 10 ** (AUDIO_FIXTURE_GAIN_DB / 20)
    pcm = array.array("h", (round(sample * gain * 32767) for sample in samples))
    if sys.byteorder != "little":
        pcm.byteswap()
    with wave.open(str(path), "wb") as output:
        output.setparams((1, 2, rate, 0, "NONE", "not compressed"))
        output.writeframes(pcm.tobytes())
    return path


def band_noise(rng, count, center, cutoff):
    """Two low-pass stages shape white noise before shifting it up to center."""
    rate = AUDIO_RATE
    decay = math.exp(-math.tau * cutoff / rate)
    low = smooth = 0.0
    for i in range(count):
        low = decay * low + (1 - decay) * rng.uniform(-1, 1)
        smooth = decay * smooth + (1 - decay) * low
        yield smooth * math.cos(math.tau * center * i / rate)
