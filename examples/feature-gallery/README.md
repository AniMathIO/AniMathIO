# AniMathIO feature gallery

The editable [projects](projects/) are the sources for
[AniMathIO's feature gallery](https://animathio.com/examples#all-features).
Each project is 3–6 seconds at 960×540. Open any `.amio` in the editor to
inspect the clips, effects and keyframes. Each audio demo embeds a synthetic
fixture tailored to the effect.

| Feature | Projects |
| --- | --- |
| LaTeX | `latex-inline-color` writes Euler's identity, then a Gaussian integral using named colors with `\color` and `\textcolor`. |
| Graph and shapes | `graph-tick-labels` draws a sine function with labelled axes. `shapes-stroke-fill` combines filled and stroked paths, an ellipse and rotation. |
| Manim importer | `manim-import` comes from [scene.py](manim/scene.py). The scene draws a circle, moves it and writes an equation. |
| Text and captions | `text-styling` shows DejaVu, weighted Inter and typewriter reveal. `captions` and `captions-playful` cover Classic, Word highlight, Karaoke, Pop, One word and Bold outline. |
| Character slide | `character-slide-in` and `character-slide-out` show the two per-glyph presets. |
| Video effects | `effect-*` shows grayscale, sepia, invert, saturation, brightness, contrast, blur, opacity and clipped slide next to the source. |
| Animation presets | `preset-*` covers the other 23 entrance, exit and emphasis presets, including both clipped slides. |
| Audio effects | `audio-*` repeats the same sound dry (blue, 0–3 s) and processed (green, 3–6 s). Listen for level, EQ, compression, gated pauses, low buzz, sibilance or steady static. Each pair of waveforms uses the same amplitude scale and comes from the exported mix. |
| Export | `webm-export` is available in both H.264/AAC MP4 and VP9/Opus WebM. `mp4-chapters` embeds chapter stops at 0, 2 and 4 seconds. |
| Code Snippets | `code-snippets-reveal` and `code-snippets-morph` use the released community plugin to reveal and morph highlighted Python code. |
| Molecule Studio | `molecule-caffeine` draws a skeletal structure, and `molecule-esterification` reveals the reactants and products of an esterification reaction. |
| Ray Optics | `optics-lens` traces three principal rays through a converging lens, and `optics-tir` sweeps the incidence angle through the threshold for total internal reflection. |
| Net Lab | `network-arp` shows ARP and a subsequent ping on a switched LAN, and `network-dns` follows a recursive DNS lookup. |
| Physics Play | `physics-projectile` compares launch angles, and `physics-dominoes` follows a ball triggering a chain reaction. |

## Render and test

Use the AniMathIO 2.3.0 CLI and its bundled plugins, Python 3.11 or newer and
FFmpeg. The smoke test also uses NumPy, Pillow and `ffprobe`. From the repository root:

```bash
export ANIMATHIO_CLI=/path/to/animathio-cli
export ANIMATHIO_PLUGIN_DIR=/path/to/bundled/plugins
python3 examples/feature-gallery/render.py --output target/feature-gallery
python3 examples/feature-gallery/test.py --output target/feature-gallery-smoke
```

The renderer writes H.264 MP4 and VP9 WebM files under `videos/`, WebP posters
under `img/` and a `manifest.json` with paths relative to the output directory.
Use `--only SLUG` to render a subset. Pass `--rebuild-projects` after changing
[clips.py](clips.py) or the Manim scene to regenerate the selected editable
sources through the CLI and its headless MCP server. Commit source projects
alongside authoring changes. The video and poster files go in
[AniMathIOWebsite](https://github.com/AniMathIO/AniMathIOWebsite).

The smoke test checks the preset and effect inventory against the loaded
first-party plugins, rebuilds projects into its output folder, renders
representative frames and checks MP4 chapters and WebM codecs. For every audio
card, it checks the visible waveform change and measures levels in the decoded
AAC and Opus. Use `test.py --audio-only` for the audio cards alone. The `export --chapters` text sidecar follows YouTube's 10-second
minimum, while the embedded MP4 carries all three short chapter stops.

The community projects use these released bundles:

- [Code Snippets 0.1.0](https://github.com/AniMathIO/plugins/releases/tag/io.github.animathio.code-snippets%400.1.0)
- [Molecule Studio 0.1.0](https://github.com/AniMathIO/plugins/releases/tag/io.github.animathio.molecule%400.1.0)
- [Ray Optics 0.1.0](https://github.com/AniMathIO/plugins/releases/tag/io.github.animathio.optics%400.1.0)
- [Net Lab 1.0.0](https://github.com/AniMathIO/plugins/releases/tag/io.github.animathio.network%401.0.0)
- [Physics Play 0.1.0](https://github.com/AniMathIO/plugins/releases/tag/io.github.animathio.physics%400.1.0)

Install each bundle from its release with its published SHA-256 using the
[community plugin instructions](../../docs/plugins.md#installing-community-plugins),
then enable community plugins and each installed plugin. Full render and smoke
runs skip the cards for each absent or disabled plugin.
