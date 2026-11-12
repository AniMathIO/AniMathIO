# AniMathIO 2.0

> **Pre-release, rewrite in progress.** This is the `v2` branch: a from-scratch **Rust** rewrite of AniMathIO, an editor for Manim-like math animation videos. Nothing here is stable yet — APIs, the file format and the CLI may change until 2.0 ships.

The full plan, architecture and roadmap live in **[issue #156](https://github.com/AniMathIO/AniMathIO/issues/156)**. Read it before proposing structure.

**Looking for the current app?** AniMathIO 1.x lives on [`main`](https://github.com/AniMathIO/AniMathIO/tree/main), and releases are on the [releases page](https://github.com/AniMathIO/AniMathIO/releases).

## What 2.0 is

The core is a small, reliable video editor: media clips, a multi-track timeline, transforms and keyframes, audio, preview and export. Nothing math-specific lives in the core — **everything else is a plugin**: text, LaTeX, graphs, Manim import, effects and MCP, each a sandboxed WebAssembly component (wasmtime, Component Model, WIT).

The architectural rules from #156:

- **Rendering is one pure function,** `render(&Project, Time) -> Frame`, shared by preview, export and tests.
- **Every mutation is a serializable, invertible `Command`.** Undo/redo, the control protocol, MCP and plugins all go through it.
- **Time is integer ticks + rational frame rates.** No `f64` seconds and no absolute pixels in the model.
- **The core owns its domain types.** Third-party crates (FilmCraft codecs, vello, GPUI) sit behind adapters.

## Status

Headless engine prototype. What works today:

| Area | Status |
| --- | --- |
| Project model, commands, undo/redo | ✅ `core` |
| `.amio` / `project.json` format | ✅ `format` (schema version 1) |
| Deterministic CPU rendering (Vello CPU backend) | ✅ `render` |
| PNG frame / sequence rendering | ✅ `animathio-cli` |
| Audio decoding, mixing, playback (cpal) | ✅ `audio` (prototype limits) |
| Plugin system (wasmtime Component Model) | ✅ `plugin-api` + `plugin-host` + first-party plugins |
| Host math typesetting (Typst + MiTeX) | ✅ `typeset` |
| Local control protocol + MCP sidecar | ✅ `control` + `animathio-cli mcp` |
| v1 and Manim importers | ✅ `import-v1`, `import-manim` |
| GPUI desktop editor | 🚧 dockable NLE shell (media pool, viewer, inspector, timeline) |
| Media decoding / video export | 🚧 deferred (labelled checkerboard placeholder) |

Text and shapes under `core::builtin` are temporary stand-ins for the plugin equivalents.

## Screenshot

The DaVinci-style editor shell on the `v2` branch (dark theme, media pool, viewer
with transport controls, resizable inspector and vertically scrolling timeline):

![AniMathIO 2.0 editor](docs/screenshots/editor-main.png)

| Viewer and inspector | Timeline |
| --- | --- |
| ![Viewer and inspector](docs/screenshots/editor-viewer.png) | ![Timeline](docs/screenshots/editor-timeline.png) |

## Documentation

- [`docs/architecture.md`](docs/architecture.md) — crate map, time model, engine rules and data flow
- [`docs/plugins.md`](docs/plugins.md) — plugin authoring guide
- [`docs/cli.md`](docs/cli.md) — every `animathio-cli` subcommand
- [`docs/mcp.md`](docs/mcp.md) — control protocol and MCP tool surface
- [`docs/migrating-from-v1.md`](docs/migrating-from-v1.md) — what maps from 1.x and what doesn't

## Building

Stable Rust (edition 2024). On shared Linux machines keep the resource limits shown
below; on other platforms run the same Cargo commands directly. Linux builds also
need `libasound2-dev` (CI installs it) for the audio crate.

### The desktop editor

```bash
cargo run -p animathio                 # open the shared sample project
cargo run -p animathio -- file.amio    # open a project
cargo run -p animathio -- --control    # also start the control socket (see docs/mcp.md)
```

The preview renders on the GPU (Vello on wgpu) when an adapter is available and
falls back to the CPU reference otherwise. Toggle it with **View → GPU preview**
(persisted), or force it for one run with `ANIMATHIO_RENDER=cpu` or `ANIMATHIO_RENDER=gpu`.
Export always uses the CPU renderer.

The editor is a thin GPUI shell over the command-only core. Playback, the timeline,
selection and the control socket are wired; a media decoder and full plugin UI are
still in progress.

The title bar provides File/Edit/View/Clip/Help menus, project status, the control
socket badge and Export. Drag panel dividers to resize; View toggles panel visibility.
The layout and S/M/L timeline track heights persist between sessions.

Use Ctrl (Cmd on macOS) with N/O/S/I/M for New/Open/Save/Import/Export and Z or
Shift+Z for Undo/Redo. In the timeline, Ctrl/Cmd+B splits at the playhead and Delete
removes the selection. Space, Left/Right and Home/End control playback/navigation
in the timeline or viewer. N toggles snapping; Ctrl/Cmd+plus/minus zooms and
Shift+Z fits the timeline. Help → Keyboard shortcuts shows the registered bindings.
Timecode and settings fields support normal text selection, paste and IME.

### The CLI

```bash
cargo build -p animathio-cli --locked
./target/debug/animathio-cli new-sample -o sample.amio
./target/debug/animathio-cli validate sample.amio
./target/debug/animathio-cli render sample.amio --frame 30 -o frame.png
./target/debug/animathio-cli render sample.amio --time 1.25 --width 1280 --height 720 -o timed.png
./target/debug/animathio-cli render-seq sample.amio --from 0 --to 90 -o frames/
```

`--frame`, `--from` and `--to` use zero-based project frame indices; sequence end is
exclusive. `--time` accepts exact decimal seconds or integer ticks with a `ticks`
suffix (705,600,000 ticks/second). Output defaults to 640×360; dimensions are
command options until project canvas settings exist. See [`docs/cli.md`](docs/cli.md)
for the full reference.

## Importing projects

Convert a v1 (Electron/TypeScript) `.animathio` project to a 2.0 `.amio` archive:

```bash
./target/debug/animathio-cli import-v1 project.animathio -o imported.amio
```

Convert a Manim Community scene script to a `.amio` archive:

```bash
./target/debug/animathio-cli import-manim scene.py -o imported.amio
./target/debug/animathio-cli import-manim scene.py --scene MyScene -o imported.amio
```

Both importers rebuild the project exclusively through `Command`s and print
non-fatal warnings to stderr. What maps and what doesn't is documented in
[`docs/migrating-from-v1.md`](docs/migrating-from-v1.md).

## Contributing

Pull requests for 2.0 target **`v2`**, not `main` or `staging`. When 2.0 ships, this branch is merged into `main` and 1.x moves to a read-only `legacy` branch. See `AGENTS.md` for branch rules, automation and commands.

## License

[MIT](LICENSE)
