"""The feature gallery card registry.

Every card is one small deterministic 16:9 project built through the public
CLI/MCP surface (`gallery.Builder`). `render.py` exports each card as H.264
MP4 + VP9 WebM plus a WebP poster; `test.py` rebuilds every card and asserts
on rendered frames. Rendered media is never committed to the main repository.

Each card documents one first-party plugin, one effect, one animation preset,
an editor feature (WebM export, MP4 chapters) or a released community plugin.
"""

from __future__ import annotations

import array
import json
import math
import subprocess
import sys

from gallery import (
    BACKGROUND,
    BLUE,
    GOLD,
    MINT,
    MUTED,
    PINK,
    WHITE,
    run,
    curve,
    path_verbs,
    ticks,
    tone_fixture,
    transform,
)

DOCS = "https://docs.animathio.com/docs"
COMMUNITY_DOCS = (
    "https://github.com/AniMathIO/plugins/tree/io.github.animathio.code-snippets%400.1.0"
    "/community/code-snippets"
)
COMMUNITY_ID = "io.github.animathio.code-snippets"
COMMUNITY_PLUGINS = {
    "molecule": ("Molecule Studio", "0.1.0"),
    "optics": ("Ray Optics", "0.1.0"),
    "network": ("Net Lab", "1.0.0"),
}
RECTANGLE = [(0, 0), (1, 0), (1, 1), (0, 1)]


def star_points(cx, cy, outer, inner, points=5):
    """A star path in canvas-normalized coordinates, centred on (cx, cy)."""
    verts = []
    for i in range(2 * points):
        radius = outer if i % 2 == 0 else inner
        angle = -math.pi / 2 + i * math.pi / points
        verts.append((cx + radius * math.cos(angle), cy + radius * math.sin(angle)))
    return verts


def reveal_keys(b, identity, start, end):
    """Write-on `reveal` 0 -> 1; times are clip-local seconds."""
    b.commands.extend(
        [
            {
                "AddKeyframe": {
                    "id": identity,
                    "target": {"Param": "reveal"},
                    "time": ticks(start),
                    "keyframe": {"value": {"Number": 0.0}, "interpolation": "Linear"},
                }
            },
            {
                "AddKeyframe": {
                    "id": identity,
                    "target": {"Param": "reveal"},
                    "time": ticks(end),
                    "keyframe": {"value": {"Number": 1.0}, "interpolation": "Linear"},
                }
            },
        ]
    )


def build_latex(b, output):
    b.heading("LaTeX typesetting", "Write-on reveal and inline color")
    first, _ = b.math(r"e^{i\pi} + 1 = 0", 0.5, 0.5, end=2.2, size=0.102)
    reveal_keys(b, first, 0.15, 1.4)
    second, _ = b.math(
        r"\int_0^{\infty} {\color{orange} e^{-x^2}}\,dx = "
        r"\frac{\textcolor{cyan}{\sqrt{\pi}}}{2}",
        0.5, 0.56, start=2.2, size=0.093, color=WHITE,
    )
    reveal_keys(b, second, 0, 1.35)
    b.label("Euler’s identity", 0.5, 0.77, 0.034, start=0, end=2.2)
    b.label("Gaussian integral", 0.5, 0.79, 0.034, start=2.2)


def build_graph(b, output):
    b.heading("Function plots", "Grid, axes and tick labels")
    b.graph(
        "plot y=sin(x)",
        (0.5, 0.58, 0.86, 0.56),
        start=0.2,
        end=5.0,
        color=BLUE,
        draw=1.4,
        labels=True,
        x_range=(-3.5, 3.5),
        y_range=(-1.6, 1.6),
    )
    b.text("y = sin x", 0.5, 0.33, 0.045, GOLD)


def build_shapes(b, output):
    b.heading("Vector shapes", "Paths, ellipses, fills and strokes")
    b.shape(
        "Star",
        star_points(0.5, 0.5, 0.16, 0.068, points=5),
        MINT,
        fill=True,
        placement=transform(0.32, 0.52),
        animations=[curve({"Transform": "Rotation"}, [(0, 0), (4.0, math.pi / 3)])],
        stroke_width=0,
    )
    b.shape(
        "Ellipse",
        ellipse=True,
        color=GOLD,
        fill=True,
        placement=transform(0.70, 0.46, 0.24, 0.24),
        stroke_width=0,
    )
    outline = star_points(0.5, 0.5, 0.11, 0.047)
    b.shape(
        "Outline",
        [*outline, outline[0]],
        PINK,
        placement=transform(0.69, 0.75),
        stroke_width=0.008,
    )
    b.shape(
        "Segment",
        [(0.12, 0.86), (0.52, 0.86)],
        BLUE,
        stroke_width=0.01,
    )


def build_text_styling(b, output):
    b.heading("Text styling", "Typefaces, weights and write-on")
    b.text("Regular DejaVu Sans", 0.5, 0.42, 0.052, WHITE, start=0.2, end=5.0)
    # font=inter selects the bundled Inter face, outlined by the plugin; the
    # string param is authored with the clip (SetParam only takes numbers).
    b.clip(
        "Inter Variable, weight 700",
        "text",
        {
            "text": "Inter Variable, weight 700",
            "units": "normalized",
            "align": "center",
            "font_size": 0.052,
            "color": BLUE,
            "font": "inter",
            "font_weight": 700.0,
        },
        start=0.2,
        end=5.0,
        placement=transform(0.5, 0.54),
    )
    t3, _ = b.text(
        "and a typewriter reveal", 0.5, 0.66, 0.052, MINT, start=1.2, end=5.0
    )
    reveal_keys(b, t3, 0.0, 2.0)


def _srt(path, window, text):
    def stamp(t):
        ms = round(t * 1000)
        return f"00:00:{ms // 1000:02d},{ms % 1000:03d}"

    path.write_text(
        f"1\n{stamp(window[0])} --> {stamp(window[1])}\n{text}\n", encoding="utf-8"
    )
    return path


def build_captions(b, output, lines):
    b.heading("Caption styles", "Styled text imported from SRT")
    b.shape("Caption stage", RECTANGLE, [24, 34, 52, 255], True,
            placement=transform(0.5, 0.57, 0.88, 0.49), stroke_width=0)
    for i, (style, name, text, accent) in enumerate(lines):
        start, end = i * 2, (i + 1) * 2
        b.text(name, 0.5, 0.41, 0.048, BLUE, start=start, end=end)
    b.flush()
    for i, (style, _, text, accent) in enumerate(lines):
        window = (i * 2, (i + 1) * 2)
        cue = _srt(output / f"cue-{style}.srt", window, text)
        result = b.session.tool(
            "import_subtitles", path=str(cue), style=style,
            font_size=0.064, position="bottom", max_words=7, accent=accent,
        )
        tracks = b.session.tool("get_project_state")["project"]["tracks"]
        track = next(t for t in tracks if t["id"] == result["track"])
        b.session.batch([
            {"SetTransform": {"id": clip["id"], "transform": {**clip["transform"], "y": 0.64}}}
            for clip in track["clips"].values()
        ])


CAPTION_LINES = [
    ("classic", "Classic", "A steady line of text.", "#828cb5"),
    ("word-highlight", "Word highlight", "Follow the highlighted word.", "#5ee4bb"),
    ("karaoke", "Karaoke", "Fill each word in time.", "#ffca66"),
]

PLAYFUL_LINES = [
    ("pop", "Pop", "Each word pops in.", "#f084b7"),
    ("one-word", "One word", "One word at a time.", "#57b2ff"),
    ("bold-outline", "Bold outline", "A strong outline adds contrast.", "#5ee4bb"),
]


def build_character_slide(b, output, role):
    entrance = role == "in"
    b.heading(f"Character slide {role}", "Each glyph moves inside its own bounds")
    subject, _ = b.text(
        "Glyph by glyph", 0.5, 0.52, 0.115, WHITE if entrance else MINT,
        start=0.75 if entrance else 0, end=3.0 if not entrance else b.duration,
    )
    b.flush()
    b.preset(subject, f"character-slide-{role}", duration_ms=1900,
             direction="left" if entrance else "right")


# -- effects-basic -----------------------------------------------------------


def build_effect(b, output, effect, title, param, before, after):
    b.heading(title)
    for x, label in [(0.28, "Original"), (0.72, "With effect")]:
        b.text(label, x, 0.76, 0.043, MUTED)
        clip, _ = b.shape(
            label, star_points(0.5, 0.5, 0.18, 0.092), GOLD, fill=True,
            placement=transform(x, 0.45),
            animations=[curve({"Transform": "Rotation"}, [(0, 0), (3.5, 1.4)])],
            stroke_width=0,
        )
    b.flush()
    applied = b.session.tool(
        "add_effect", id=str(clip), plugin="org.animathio.effects-basic",
        effect=effect, params={param: before},
    )
    target = {"Effect": {"effect": applied["effect_id"], "param": param}}
    values = [(0, before), (0.5, before), (1.6, after), (2.7, after)]
    if effect == "slide":
        values = [(0, 0), (0.25, 0), (2.7, 1), (3.5, 1)]
    b.session.batch([
        {"AddKeyframe": {"id": clip, "target": target, "time": ticks(t),
                         "keyframe": {"value": {"Number": value}, "interpolation": "Linear"}}}
        for t, value in values
    ])


# -- animation presets --------------------------------------------------------


def build_preset_card(b, output, preset, role):
    b.heading(preset.replace("-", " ").capitalize())
    start = 0.75 if role == "in" else 0
    end = 2.7 if role == "out" else b.duration
    subject, _ = b.text("AniMathIO", 0.5, 0.5, 0.13, WHITE, start=start, end=end)
    b.text(
        {
            "in": "entrance preset",
            "out": "exit preset",
            "emphasis": "emphasis preset",
        }[role],
        0.5,
        0.80,
        0.042,
        BLUE,
    )
    b.flush()
    b.preset(subject, preset, duration_ms=1600 if role != "emphasis" else 2500)


PRESET_CARDS = [
    ("fade-in", "in"),
    ("slide-in", "in"),
    ("zoom-in", "in"),
    ("pop-in", "in"),
    ("spin-in", "in"),
    ("bounce-in", "in"),
    ("wipe-in", "in"),
    ("typewriter-in", "in"),
    ("fade-out", "out"),
    ("slide-out", "out"),
    ("zoom-out", "out"),
    ("pop-out", "out"),
    ("spin-out", "out"),
    ("bounce-out", "out"),
    ("wipe-out", "out"),
    ("typewriter-out", "out"),
    ("breathe", "emphasis"),
    ("shake", "emphasis"),
    ("spin", "emphasis"),
    ("hop", "emphasis"),
    ("flash", "emphasis"),
    ("clipped-slide-in", "in"),
    ("clipped-slide-out", "out"),
]


# -- audio --------------------------------------------------------------------


AUDIO_EFFECTS = [
    ("gain", "Gain / trim", "Raise the level by 6 dB.", {"gain_db": 6}),
    ("eq3", "3-band EQ", "Reduce the low shelf and boost the midrange.",
     {"low_freq": 300, "low_gain_db": -16, "mid_freq": 500, "mid_gain_db": 9}),
    ("compressor", "Compressor", "Tame peaks with a lower threshold and 8:1 ratio.",
     {"threshold_db": -24, "ratio": 8, "makeup_db": 5}),
    ("gate", "Noise gate", "Attenuate quiet intervals below the threshold.",
     {"threshold_db": -17, "hold_ms": 12, "release_ms": 35}),
    ("highpass", "High-pass filter", "Reduce the low-frequency rumble.",
     {"frequency": 160}),
    ("voice_cleanup", "Voice cleanup", "Apply high-pass filtering, gating and de-essing.",
     {"highpass_hz": 140, "gate_threshold_db": -37}),
    ("denoise", "Denoise", "Reduce the fixture’s steady noise floor.",
     {"strength": 1, "sensitivity_db": 12}),
]


def audio_samples(path):
    """Decode the exported mix in mono for the waveform and smoke test."""
    pcm = subprocess.check_output([
        "ffmpeg", "-v", "error", "-i", str(path), "-vn", "-ac", "1",
        "-ar", "8000", "-f", "s16le", "-",
    ])
    samples = array.array("h")
    samples.frombytes(pcm)
    if sys.byteorder != "little":
        samples.byteswap()
    return samples


def build_audio(b, output, effect, title, summary, params):
    wav = tone_fixture(output / "gallery-tone.wav", 3)
    dry = b.session.tool("add_media", path=str(wav))["clip_id"]
    wet = b.session.tool("add_media", path=str(wav))["clip_id"]
    b.session.batch([
        {"TrimClip": {"id": dry, "range": {"start": 0, "end": ticks(3)}, "source_offset": 0}},
        {"TrimClip": {"id": wet, "range": {"start": ticks(3), "end": ticks(6)}, "source_offset": 0}},
    ])
    b.session.tool("add_audio_effect", id=str(wet), plugin="org.animathio.audio-basic",
                   effect=effect, params=params)

    # The waveform comes from the exported mix, so it includes the actual effect.
    scratch = output / f"waveform-{effect}.amio"
    mix = scratch.with_suffix(".mp4")
    try:
        b.save(scratch)
        run("export", scratch, "-o", mix, "--fps", 4, "--crf", 35, "--overwrite")
        samples = audio_samples(mix)
    finally:
        scratch.unlink(missing_ok=True)
        mix.unlink(missing_ok=True)

    b.sync_tracks()
    b.heading(title, summary)
    for i, (label, color) in enumerate([("Dry", BLUE), ("With effect", MINT)]):
        y = 0.47 + i * 0.27
        b.text(label, 0.5, y - 0.139, 0.044, color)
        path = []
        for bar in range(64):
            start = i * 24000 + bar * 375
            peak = max((abs(s) for s in samples[start:start + 375]), default=0)
            h = max(0.002, min(0.115, peak / 130000))
            x = 0.11 + bar * 0.0124
            path.extend(path_verbs([(x, y - h), (x + 0.008, y - h),
                                    (x + 0.008, y + h), (x, y + h)], close=True))
        b.clip(f"{label} waveform", "shapes", {
            "units": "normalized", "shape": "path", "path": json.dumps(path),
            "fill": color, "stroke_width": 0,
        })
        b.shape("Playhead", RECTANGLE, WHITE, True, stroke_width=0,
                start=i * 3, end=(i + 1) * 3, placement=transform(0.11, y, 0.003, 0.18),
                animations=[curve({"Transform": "X"}, [(0, 0.11), (3, 0.89)])])


# -- community ----------------------------------------------------------------


def build_code_snippets(b, output, mode):
    b.heading("Token morph" if mode == "morph" else "Code reveal",
              "Syntax-highlighted Python")
    code = "def area(radius):\n    return pi * radius ** 2"
    destination = "def area(radius):\n    diameter = 2 * radius\n    return pi * diameter ** 2 / 4"
    b.clip("Code Snippets", COMMUNITY_ID, {
        "code": code, "code_to": destination, "language": "python",
        "theme": "ocean", "font_size": 6, "line_numbers": True, "mode": mode,
    }, placement=transform(0.5, 0.61), animations=[
        curve({"Param": "progress"}, [(0, 0), (0.55, 0), (3.7, 1), (5, 1)])
    ])


def build_community_scene(b, output, plugin, title, subtitle, params, placement, reveal_end):
    if subtitle:
        b.heading(title, subtitle)
    name, _ = COMMUNITY_PLUGINS[plugin]
    b.clip(name, f"io.github.animathio.{plugin}", params,
           placement=placement, animations=[
               curve({"Param": "progress"}, [(0, 0), (0.35, 0), (reveal_end, 1)])
           ])


# -- editor delivery features --------------------------------------------------


def build_webm(b, output):
    b.heading("Two containers, one render", "The same project exports to both formats")
    for x, label, codec, color in [
        (0.28, "MP4", "H.264 + AAC", BLUE),
        (0.72, "WebM", "VP9 + Opus", MINT),
    ]:
        start = 1 if label == "WebM" else 0
        fade = [curve({"Transform": "Opacity"}, [(0, 0.15), (0.7, 1)])]
        b.shape(label, RECTANGLE, color, True, start=start, stroke_width=0,
                placement=transform(x, 0.51, 0.34, 0.28), animations=fade)
        b.text(label, x, 0.51, 0.088, BACKGROUND, start=start, animations=fade)
        b.text(codec, x, 0.74, 0.042, color, start=start, animations=fade)


def build_chapters(b, output):
    b.heading("MP4 chapters", "Timeline markers become chapter stops")
    segments = [
        ("Intro", 0, BLUE),
        ("The theorem", 2, GOLD),
        ("The proof", 4, MINT),
    ]
    b.markers([(start, name) for name, start, _ in segments])
    for index, (name, start, color) in enumerate(segments):
        x = 0.224 + 0.276 * index
        b.shape(f"Chapter {name}", RECTANGLE, color, True, stroke_width=0,
                placement=transform(x, 0.52, 0.265, 0.12),
                animations=[curve({"Transform": "Opacity"}, [(0, 0.34), (2, 0.34)])])
        b.shape(f"Current chapter {name}", RECTANGLE, color, True, stroke_width=0,
                start=start, end=start + 2, placement=transform(x, 0.52, 0.265, 0.12))
        b.text(name, x, 0.39, 0.045, WHITE)
        b.text(f"0:0{start}", x, 0.68, 0.039, color)
    b.shape("Playback cursor", RECTANGLE, WHITE, True, stroke_width=0,
            placement=transform(0.09, 0.75, 0.004, 0.08),
            animations=[curve({"Transform": "X"}, [(0, 0.09), (6, 0.91)])])


# ---------------------------------------------------------------------------
# Registry
# ---------------------------------------------------------------------------


def _card(slug, title, kinds, plugin, docs, summary, duration, build, **extra):
    return {
        "slug": slug,
        "title": title,
        "kinds": kinds,
        "plugin": plugin,
        "docs": docs,
        "summary": summary,
        "duration": duration,
        "build": build,
        **extra,
    }


def registry():
    """All gallery cards in site order."""
    cards = [
        _card(
            "latex-inline-color",
            "LaTeX typesetting",
            ["math"],
            "org.animathio.latex",
            f"{DOCS}/plugins",
            "Write on Euler’s identity and the Gaussian integral. Inline colors "
            "highlight parts of the integral.",
            5.0,
            build_latex,
            poster_time=4.1,
        ),
        _card(
            "graph-tick-labels",
            "Function plots",
            ["math"],
            "org.animathio.graph",
            f"{DOCS}/plugins",
            "A plotted sine curve with grid, axes and numeric tick labels, "
            "drawn on over time.",
            5.0,
            build_graph,
        ),
        _card(
            "manim-import",
            "Manim scene import",
            ["math"],
            "org.animathio.import-manim",
            f"{DOCS}/cli",
            "A Manim Community scene script converted to a project by the "
            "sandboxed importer plugin.",
            5.0,
            None,
            manim=True,
            poster_time=4.45,
        ),
        _card(
            "shapes-stroke-fill",
            "Vector shapes",
            ["math"],
            "org.animathio.shapes",
            f"{DOCS}/plugins",
            "Filled paths, ellipses, strokes and keyframed rotation in "
            "normalized coordinates.",
            4.0,
            build_shapes,
        ),
        _card(
            "text-styling",
            "Text styling",
            ["text"],
            "org.animathio.text",
            f"{DOCS}/plugins",
            "DejaVu or bundled Inter with weights, plus the typewriter reveal "
            "parameter.",
            5.0,
            build_text_styling,
            poster_time=3.5,
        ),
        _card(
            "captions",
            "Caption styles",
            ["text"],
            "org.animathio.text",
            f"{DOCS}/captions-and-automation",
            "SRT import restyled per track: classic, word highlight and "
            "karaoke fill.",
            6.0,
            lambda b, o: build_captions(b, o, CAPTION_LINES),
        ),
        _card(
            "captions-playful",
            "Caption styles II",
            ["text"],
            "org.animathio.text",
            f"{DOCS}/captions-and-automation",
            "Pop, one-word-at-a-time and bold-outline caption styles on "
            "imported cues.",
            6.0,
            lambda b, o: build_captions(b, o, PLAYFUL_LINES),
            poster_time=1.1,
        ),
    ]
    for role in ("in", "out"):
        cards.append(_card(
            f"character-slide-{role}", f"Character slide {role}", ["text", "animations"],
            f"org.animathio.animations / character-slide-{role}", f"{DOCS}/plugins",
            f"The text plugin staggers individual glyphs as they slide {role}.", 3.8,
            lambda b, o, r=role: build_character_slide(b, o, r),
            presets=(f"character-slide-{role}",),
        ))
    effects = [
        ("grayscale", "Grayscale", "Desaturate toward luminance.", "amount", 0, 1),
        ("sepia", "Sepia", "Add a warm monochrome tint.", "amount", 0, 1),
        ("invert", "Invert", "Invert the star’s colors.", "amount", 0, 1),
        ("saturate", "Saturate", "Boost color saturation.", "amount", 1, 2.5),
        ("brightness", "Brightness", "Increase brightness in linear light.", "amount", 1, 2.5),
        ("contrast", "Contrast", "Increase contrast around mid-gray.", "amount", 1, 1.8),
        ("blur", "Blur", "Soften the star with a box blur.", "radius", 0, 0.036),
        ("opacity", "Opacity", "Reduce the clip’s alpha.", "amount", 1, 0.36),
        ("slide", "Clipped slide", "Slide the star into its own bounds.", "progress", 0, 1),
    ]
    for effect, title, summary, param, before, after in effects:
        cards.append(_card(
            f"effect-{effect}", title, ["effects"],
            f"org.animathio.effects-basic / {effect}", f"{DOCS}/plugins", summary, 3.5,
            lambda b, o, args=(effect, title, param, before, after): build_effect(b, o, *args),
            video_effect=effect,
        ))
    for preset, role in PRESET_CARDS:
        title = preset.replace("-", " ").capitalize()
        summary = {
            "in": f"A title animates in with {title.lower()}.",
            "out": f"A title animates out with {title.lower()}.",
            "emphasis": f"Apply the {title.lower()} emphasis preset to a title.",
        }[role]
        cards.append(_card(
            f"preset-{preset}", title, ["animations"],
            f"org.animathio.animations / {preset}", f"{DOCS}/plugins", summary, 3.4,
            lambda b, o, p=preset, r=role: build_preset_card(b, o, p, r),
            presets=(preset,),
        ))
    for effect, title, summary, params in AUDIO_EFFECTS:
        cards.append(_card(
            f"audio-{effect.replace('_', '-')}", title, ["audio"],
            f"org.animathio.audio-basic / {effect}", f"{DOCS}/plugins",
            f"{summary} Hear the same fixture dry, then processed.", 6.0,
            lambda b, o, args=(effect, title, summary, params): build_audio(b, o, *args),
            audio_effect=effect,
        ))
    for mode, title, summary in [
        ("reveal", "Code reveal", "Reveal syntax-highlighted Python one character at a time."),
        ("morph", "Code morph", "Move retained tokens between two Python snippets."),
    ]:
        cards.append(_card(
            f"code-snippets-{mode}", title, ["community"], COMMUNITY_ID,
            COMMUNITY_DOCS, summary, 5.0,
            lambda b, o, m=mode: build_code_snippets(b, o, m),
            community=True,
        ))
    for plugin, scene, title, subtitle, summary, params, placement, reveal_end, poster in [
        ("molecule", "caffeine", "Caffeine draws itself", "Molecule Studio · SMILES to structure",
         "Watch Molecule Studio draw caffeine’s fused rings and atom labels bond by bond.",
         {"preset": "caffeine", "ink": WHITE, "cpk": False},
         transform(0.5, 0.61, 0.79, 0.79), 3.8, 4.6),
        ("molecule", "esterification", "Esterification", "Molecule Studio · Reaction diagram",
         "Molecule Studio draws acetic acid and ethanol, then reveals ethyl acetate and water.",
         {"preset": "esterification", "ink": WHITE, "cpk": False, "font_size": 60},
         transform(0.5, 0.61, 0.94, 0.94), 4.4, 4.8),
        ("optics", "lens", "Converging lens", "Ray Optics · Principal rays",
         "Follow three rays through a converging lens to an inverted real image in Ray Optics.",
         {"setup": "lens-diagram", "object_distance": 45},
         transform(0.5, 0.61, 0.72, 0.72), 3.8, 4.6),
        ("optics", "tir", "Total internal reflection", "Ray Optics · Glass to air",
         "Ray Optics sweeps the incidence angle through the critical angle until only the reflected ray remains.",
         {"setup": "tir", "index": 1.5, "angle_deg": 68},
         transform(0.5, 0.61, 0.76, 0.76), 4.5, 4.7),
        ("network", "arp", "ARP request and reply", None,
         "Net Lab floods an ARP request across a switch, then uses the learned address to ping a peer.",
         {"scenario": "arp", "topology": "single-switch", "table": "arp"},
         transform(), 5.5, 1.2),
        ("network", "dns", "Recursive DNS lookup", None,
         "Net Lab follows a query for example.test from the client to its resolver and the authoritative server.",
         {"scenario": "dns", "topology": "single-switch"},
         transform(), 5.5, 3.7),
    ]:
        name, version = COMMUNITY_PLUGINS[plugin]
        plugin_id = f"io.github.animathio.{plugin}"
        tag = f"{plugin_id}%40{version}"
        upstream = "https://github.com/AniMathIO/plugins"
        cards.append(_card(
            f"{plugin}-{scene}", title, ["community"], plugin_id,
            f"{upstream}/blob/{tag}/community/{plugin}/README.md", summary,
            6.0 if plugin == "network" else 5.0,
            lambda b, o, args=(plugin, title, subtitle, params, placement, reveal_end):
                build_community_scene(b, o, *args),
            community=True, poster_time=poster, plugin_name=name,
            release=f"{upstream}/releases/tag/{tag}",
        ))
    cards.append(
        _card(
            "webm-export",
            "WebM export",
            ["editing"],
            "animathio-cli export",
            f"{DOCS}/cli",
            "Every gallery card renders twice from one project: H.264 MP4 and "
            "VP9 + Opus WebM. This clip plays the WebM version in supported browsers.",
            4.0,
            build_webm,
        )
    )
    cards.append(
        _card(
            "mp4-chapters",
            "MP4 chapters",
            ["editing"],
            "animathio-cli export",
            f"{DOCS}/cli",
            "Timeline markers export as a chapter list and embed as the MP4 "
            "chapter atom.",
            6.0,
            build_chapters,
        )
    )
    return cards
