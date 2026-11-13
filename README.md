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

Editor and headless engine prototype. What works today:

| Area | Status |
| --- | --- |
| Project model, commands, undo/redo | ✅ `core` |
| `.amio` / `project.json` format | ✅ `format` (schema version 2) |
| Deterministic CPU rendering (Vello CPU backend) | ✅ `render` |
| PNG frame / sequence rendering | ✅ `animathio-cli` |
| Audio decoding, mixing, playback (cpal) | ✅ `audio` (prototype limits) |
| Plugin system (wasmtime Component Model) | ✅ `plugin-api` + `plugin-host` + first-party plugins |
| Host math typesetting (Typst + MiTeX) | ✅ `typeset` |
| Local control protocol + MCP sidecar | ✅ `control` + `animathio-cli mcp` |
| v1 and Manim importers | ✅ `import-v1`, `import-manim` |
| GPUI desktop editor | 🚧 dockable NLE: shuttle, in/out, blade/ripple tools, OS drops, project launcher and workflow dialogs |
| Media decoding / video export | ✅ native decoding and offline H.264/AAC MP4 export |

Text and shapes use the bundled WASM components end to end, including CLI renders
and exports. Build them with `scripts/build-plugins.sh`; schema-1 projects migrate
automatically to plugin clips when loaded. Effects remain in the renderer.

## Screenshot

The DaVinci-style editor shell on the `v2` branch (dark theme, media pool, viewer
with transport controls, resizable inspector and vertically scrolling timeline):

![AniMathIO 2.0 editor](docs/screenshots/editor-main.png)

| Start screen | Export |
| --- | --- |
| ![Start screen](docs/screenshots/editor-start.png) | ![Export dialog](docs/screenshots/editor-export.png) |

| Viewer and inspector | Timeline |
| --- | --- |
| ![Viewer and inspector](docs/screenshots/editor-viewer.png) | ![Timeline](docs/screenshots/editor-timeline.png) |

| Multi-selection (rubber band, inspector summary) |
| --- |
| ![Multi-selection](docs/screenshots/editor-multiselect.png) |

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

Build the bundled WASM components before rendering text or shapes:

```bash
scripts/build-plugins.sh
```

Debug binaries discover `target/plugins` relative to the executable. For an
unpackaged release binary, set `ANIMATHIO_PLUGIN_DIR=target/plugins` explicitly.
Installed packages discover the components in their bundled resource directories.

### The desktop editor

```bash
cargo run -p animathio                 # start screen with presets and recent projects
cargo run -p animathio -- --sample     # open the shared sample project
cargo run -p animathio -- file.amio    # open a project
cargo run -p animathio -- --control    # also start the control socket (see docs/mcp.md)
```

The preview renders on the GPU (Vello on wgpu) when an adapter is available and
falls back to the CPU reference otherwise. Toggle it with **View → GPU preview**
(persisted), or force it for one run with `ANIMATHIO_RENDER=cpu` or `ANIMATHIO_RENDER=gpu`.
Export always uses the CPU renderer.

The editor is a thin GPUI shell over the command-only core. Playback, timeline
editing, generated plugin controls and the control socket share the same model.

The title bar provides File/Edit/View/Playback/Tools/Clip/Help menus, project status, the control
socket badge and Export. Drag panel dividers to resize; View toggles panel visibility.
The layout and S/M/L timeline track heights persist between sessions.

Use Ctrl (Cmd on macOS) with N/O/S/I/M for New/Open/Save/Import/Export and Z or
Shift+Z for Undo/Redo. In the timeline, Ctrl/Cmd+B splits the selected clips at the
playhead and Delete removes the selection; Shift+Delete ripple-deletes it (or the in/out
range across all tracks when no clip is selected). Space, Left/Right, Up/Down
and Home/End control playback, frame stepping, edit-point stepping and project bounds.
J/K/L shuttle backward/stop/forward; repeated J/L doubles the speed up to 16x. Holding K
while pressing J or L jogs at half speed. Audio follows the shuttle and stays the master
clock: forward 1x/2x/4x/8x and reverse 1x/2x/4x are audible (varispeed, so pitch follows
speed); faster rates are muted while video keeps pace.

**Selection and clipboard.** Click selects a clip; Ctrl/Cmd+click toggles it, Shift+click
adds the clips between it and the last-selected clip on the same track, and dragging a
rubber band over empty track space (Shift/Ctrl extends the selection) selects every
clip it touches. Ctrl/Cmd+A selects all, Esc clears (and first cancels a drag, menu or
rename). The inspector shows the clip count, kinds, tracks and time span, and its
Transform fields (plus Gain for all-audio selections) edit every selected clip at once.
Dragging, nudging (Left/Right = 1 frame, Shift+Left/Right = 10 frames while the timeline
has a selection; without one they move the playhead), deleting and splitting act on the
whole selection as one undo step. Ctrl/Cmd+C/X copy/cut, Ctrl/Cmd+V pastes at the playhead
on the original tracks (a missing, locked or occupied track gets a new one, so a paste
never overwrites), and Ctrl/Cmd+D duplicates the selection right after itself. The clip
JSON is also written to the system clipboard, so it can be pasted into another project
window.

**Tracks.** Double-click a track name to rename it inline (Enter commits, Esc cancels).
Drag a track header onto another to reorder, or use the header's right-click menu
(Rename, Move up, Move down).

**Ripple.** The toolbar link button (Tools → Ripple all tracks) switches ripple edits
between "this track only" (default) and "all tracks". With all tracks, ripple delete and
Ripple trim (T, or Ctrl+edge-drag) slide later clips on every unlocked track, never into
a neighbouring clip; locked tracks stay put.

I and O set in/out points (out is exclusive), Shift+I/O jumps to them, and Alt+X clears them. The ruler highlights
the range; looping uses it and export defaults to it. Select (A), Blade (B), Ripple
trim (T) and Hand/pan (H) are available in the timeline toolbar. Shift+blade-click
splits across tracks; Ctrl+edge-drag also ripple-trims. Ripple trim shifts later clips
on the same track as one undo step. N toggles snapping and an engaged snap draws an
accent line. Ctrl/Cmd+plus/minus zooms and Shift+Z fits the timeline.
Ctrl/Cmd+wheel smoothly zooms around the pointer; drag the overview above the
tracks to scroll. Clips show kind colours, video/image filmstrips, filled audio
waveforms, content snippets, smooth fade curves and gradient transition windows. Select a
clip to show its keyframe diamonds. Track headers have mute/solo/lock buttons and
a horizontal gain slider (drag to adjust, double-click to reset to unity); a drag
is one undo step. Use large tracks for the most detailed clip previews.

Export opens a form with an MP4 path chooser, whole/in-out/custom range and shared
`youtube_1080p`, `youtube_4k`, `shorts_vertical` and `square` presets (30 fps, CRF 18).
Presets apply to an export snapshot, keeping project settings unchanged. Existing
files require confirmation. Exports never block the editor: "Add to queue" or "Export now"
snapshots the project into a job, and the Export queue popover (status bar, View menu) runs
jobs one at a time with progress, ETA, cancel, show-in-folder/open and error text; toasts
announce completion or failure.
File → Project settings edits resolution, rational fps and background. Preferences
(Ctrl/Cmd+,) saves the preview renderer, autosave interval (0 disables), default export
folder and density in the application config directory, separately from projects.

Drop OS media files on the pool to import assets, or on a timeline track to import
and place them at that position. Multiple files are placed consecutively. The Plugins
tab's **Write on…** action replaces a selected LaTeX clip's reveal animation with
0-to-1 keyframes over a chosen duration, in one undo step. Help → Keyboard shortcuts
shows the registered bindings. Text fields support selection, paste and IME.

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

Video imports include a linked companion audio clip when the file contains audio. Linked clips move, trim, split and delete together; use **Unlink** in the clip context menu to edit them independently. Headless import is available through MCP `add_media` or `animathio-cli import-media INPUT -o OUTPUT.amio`.

## Contributing

Pull requests for 2.0 target **`v2`**, not `main` or `staging`. When 2.0 ships, this branch is merged into `main` and 1.x moves to a read-only `legacy` branch. See `AGENTS.md` for branch rules, automation and commands.


## License

[MIT](LICENSE)
