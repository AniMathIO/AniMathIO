# `animathio-cli` reference

Text and shape renders require the bundled WASM components. Run
`scripts/build-plugins.sh` first; for unpackaged release binaries set
`ANIMATHIO_PLUGIN_DIR=target/plugins`. PNG renders, sequences and MP4 exports all
resolve clips through the plugin host.

Headless project validation, PNG rendering, importers and the MCP sidecar.
Generated from the built binary's `--help` output.

```text
AniMathIO headless CPU renderer

Usage: animathio-cli <COMMAND>

Commands:
  render        Render one frame to PNG
  render-seq    Render [from, to) frame indices to `frame_000000.png`, etc
  export        Export an offline H.264 + AAC MP4
  validate      Check model invariants and all asset content hashes
  import-media  Import media into a new project, including linked companion audio
  import-v1     Convert a v1 `.animathio` project to a `.amio` archive
  import-manim  Convert a Manim Community scene script to a `.amio` archive
  new-sample    Write a three-second animated image/text/shape demo archive
  mcp           Run an MCP stdio server: connects to a running editor, or drives a project file headlessly when `--project` is given
  help          Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

Build it once (on shared Linux machines keep the resource limits):

```bash
CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=line-tables-only nice -n 19 ionice -c3 \
  cargo build -p animathio-cli --locked
```

Inputs may be `.amio` archives or `project.json` with content-addressed media in an
adjacent `assets/<blake3>` directory. `--frame`, `--from` and `--to` are zero-based
project frame indices at the project's rational frame rate; the sequence end is
exclusive. `--time` accepts exact decimal seconds (`1.25`) or integer ticks with a
`ticks` suffix (`705600000ticks`); 705,600,000 ticks = 1 second. Output size
defaults to 640×360 (or the project's canvas when set) unless overridden.

## `render`

Render one frame to PNG.

```text
Usage: animathio-cli render [OPTIONS] --output <OUTPUT> <PROJECT>

Arguments:
  <PROJECT>  project.json (assets/<hash> alongside it), or a self-contained .amio

Options:
      --frame <FRAME>    Zero-based frame index at the project's rational frame rate
      --time <TIME>      Exact nonnegative decimal seconds (e.g. 1.25), or integer ticks (123ticks)
  -o, --output <OUTPUT>
      --width <WIDTH>    Overrides the project's output width
      --height <HEIGHT>  Overrides the project's output height
  -h, --help             Print help
```

Exactly one of `--frame` or `--time` is required.

```bash
animathio-cli render sample.amio --frame 30 -o frame.png
animathio-cli render sample.amio --time 1.25 -o timed.png
animathio-cli render sample.amio --time 705600000ticks -o one-second.png
animathio-cli render sample.amio --frame 0 --width 1280 --height 720 -o wide.png
```

## `render-seq`

Render the half-open frame range `[from, to)` to `frame_000000.png`, etc.

```text
Usage: animathio-cli render-seq [OPTIONS] --to <TO> --output <OUTPUT> <PROJECT>

Options:
      --from <FROM>      [default: 0]
      --to <TO>          Exclusive final frame index
  -o, --output <OUTPUT>
      --width <WIDTH>    Overrides the project's output width
      --height <HEIGHT>  Overrides the project's output height
  -h, --help             Print help
```

```bash
animathio-cli render-seq sample.amio --from 0 --to 90 -o frames/
animathio-cli render-seq sample.amio --from 30 --to 31 --width 1920 --height 1080 -o frames/
```

## `export`

Export an offline H.264 + AAC MP4. Export steps the preview renderer frame by
frame and encodes with the FilmCraft backend; it is never a recording of
playback. `--from`/`--to` accept the same exact decimal-second or `Nticks` forms
as `render --time`; the end is exclusive and defaults to the project duration.
`--fps` overrides the project frame rate (e.g. `30` or `30000/1001`); `--crf` is
the H.264 constant-quality target (0–51, lower is better). `--preset` uses the same
`youtube_1080p`, `youtube_4k`, `shorts_vertical`, `square` and `portrait` table as the editor and
MCP. It sets the export snapshot to the preset resolution and 30 fps without writing
the project file. Quality defaults to CRF 18 with a preset or 23 otherwise;
`--fps` and `--crf` override those defaults.

Missing parent directories of `--output` are created. An existing file is never
replaced silently: without `--overwrite` the command fails with an
`already exists` error and leaves the file untouched. The MP4 is written to a
temporary file in the destination directory and renamed into place only after a
successful export, so a failed or interrupted run leaves no partial output.

```text
Export an offline H.264 + AAC MP4

Usage: animathio-cli export [OPTIONS] --output <OUTPUT> <PROJECT>

Arguments:
  <PROJECT>

Options:
  -o, --output <OUTPUT>
      --from <FROM>      Inclusive start in decimal seconds or Nticks [default: 0]
      --to <TO>          Exclusive end in decimal seconds or Nticks; defaults to project duration
      --fps <FPS>        Output rate, e.g. 30 or 30000/1001
      --preset <PRESET>  Delivery preset: `youtube_1080p`, `youtube_4k`, `shorts_vertical`, `square` or `portrait`
      --crf <CRF>        Constant quality, 0..=51; lower is better (defaults to preset quality or 23)
      --overwrite        Replace an existing output file instead of failing
  -h, --help             Print help
```

```bash
animathio-cli export sample.amio -o out.mp4
animathio-cli export sample.amio -o shorts.mp4 --preset shorts_vertical
animathio-cli export sample.amio -o clip.mp4 --from 1.0 --to 3.0 --fps 30000/1001 --crf 18
animathio-cli export sample.amio -o renders/out.mp4 --overwrite
```

## `validate`

Check model invariants and all asset content hashes. It does not attempt video
decoding.

```text
Usage: animathio-cli validate <PROJECT>
```

```bash
animathio-cli validate sample.amio
```

## `import-v1`

Convert a v1 (Electron/TypeScript) `.animathio` project to a 2.0 `.amio` archive.
Non-fatal warnings are printed to stderr.

```text
Usage: animathio-cli import-v1 --output <OUTPUT> <INPUT>

Arguments:
  <INPUT>  v1 `.animathio` project file (deflated JSON)

Options:
  -o, --output <OUTPUT>  Output `.amio` archive
  -h, --help             Print help
```

```bash
animathio-cli import-v1 project.animathio -o imported.amio
```

## `import-manim`

Convert a Manim Community scene script (`.py`) to a 2.0 `.amio` archive.

```text
Usage: animathio-cli import-manim [OPTIONS] --output <OUTPUT> <INPUT>

Arguments:
  <INPUT>  Manim scene script (`.py`)

Options:
      --scene <SCENE>    Import this scene class; defaults to the first `Scene` subclass
  -o, --output <OUTPUT>  Output `.amio` archive
  -h, --help             Print help
```

```bash
animathio-cli import-manim scene.py -o imported.amio
animathio-cli import-manim scene.py --scene MyScene -o imported.amio
```

## `new-sample`

Write a three-second animated image/text/shape demo archive (the same sample the
editor opens by default).

```text
Usage: animathio-cli new-sample --output <OUTPUT>
```

```bash
animathio-cli new-sample -o sample.amio
```

## `mcp`

Run an MCP stdio server: connects to a running editor's control socket, or drives a
project file headlessly when `--project` is given. See [`docs/mcp.md`](mcp.md).

```text
Usage: animathio-cli mcp [OPTIONS]

Options:
      --project <PROJECT>  Project file to operate on headlessly
  -h, --help               Print help
```

```bash
animathio-cli mcp                    # attach to a running editor
animathio-cli mcp --project sample.amio
```

## `import-media`

```text
Import media into a new project, including linked companion audio

Usage: animathio-cli import-media --output <OUTPUT> <INPUT>

Arguments:
  <INPUT>

Options:
  -o, --output <OUTPUT>
  -h, --help             Print help
```

Creates a new `.amio` project from video or audio. Video with audio creates linked clips on separate tracks.
