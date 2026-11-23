# AniMathIO feature gallery

The editable [projects](projects/) are the sources for
[AniMathIO's feature gallery](https://animathio.com/examples#all-features).
Each project is 3–6 seconds at 960×540. Open any `.amio` in the editor to
inspect the clips, effects and keyframes. The audio demos embed a synthetic
fixture.

| Feature | Projects |
| --- | --- |
| LaTeX | `latex-inline-color` writes Euler's identity, then a Gaussian integral using named colors with `\color` and `\textcolor`. |
| Graph and shapes | `graph-tick-labels` draws a sine function with labelled axes. `shapes-stroke-fill` combines filled and stroked paths, an ellipse and rotation. |
| Manim importer | `manim-import` comes from [scene.py](manim/scene.py). The scene draws a circle, moves it and writes an equation. |
| Text and captions | `text-styling` shows DejaVu, weighted Inter and typewriter reveal. `captions` and `captions-playful` cover Classic, Word highlight, Karaoke, Pop, One word and Bold outline. |
| Character slide | `character-slide-in` and `character-slide-out` show the two per-glyph presets. |
| Video effects | `effect-*` shows grayscale, sepia, invert, saturation, brightness, contrast, blur, opacity and clipped slide next to the source. |
| Animation presets | `preset-*` covers the other 23 entrance, exit and emphasis presets, including both clipped slides. |
| Audio effects | `audio-*` plays the same fixture dry and with gain, 3-band EQ, compression, gating, high-pass filtering, voice cleanup or denoise. The waveforms come from the exported mix. |
| Export | `webm-export` is available in both H.264/AAC MP4 and VP9/Opus WebM. `mp4-chapters` embeds chapter stops at 0, 2 and 4 seconds. |
| Code Snippets | `code-snippets-reveal` and `code-snippets-morph` use the released community plugin to reveal and morph highlighted Python code. |

## Render and test

Use the AniMathIO 2.3.0 CLI and its bundled plugins, Python 3.11 or newer and
FFmpeg. The smoke test also uses Pillow and `ffprobe`. From the repository root:

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
representative frames and checks MP4 chapters, the WebM codecs and the gain
demo's audio. The `export --chapters` text sidecar follows YouTube's 10-second
minimum, while the embedded MP4 carries all three short chapter stops.

The Code Snippets projects require the
[0.1.0 bundle](https://github.com/AniMathIO/plugins/releases/tag/io.github.animathio.code-snippets%400.1.0).
Install it from the release with its published SHA-256 using the
[community plugin instructions](../../docs/plugins.md#installing-community-plugins), then
enable community plugins and Code Snippets. Full gallery runs skip these two
cards when the bundle is disabled or absent.
