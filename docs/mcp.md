# Control protocol and MCP

AniMathIO 2.0 exposes two related surfaces:

1. **The control protocol** — a local, authenticated JSON-RPC 2.0 socket that is
   the single API for the editor UI, the CLI, plugins and the MCP sidecar.
2. **MCP** — a stdio server (`animathio-cli mcp`) that translates Model Context
   Protocol tool calls onto the control protocol (or drives a project file
   headlessly).

## Control protocol

### Transport and location

- **Unix:** a Unix-domain socket at `$XDG_RUNTIME_DIR/animathio/animathio-control.sock`.
- **Windows:** a named pipe anchored at the same per-user location.
- Never a listening TCP port.

The editor starts the server when launched with `--control` (or
`ANIMATHIO_CONTROL=1`).

### Authentication

On bind, the server generates a random **256-bit token**, writes it to a `0600`
file next to the socket (`$XDG_RUNTIME_DIR/animathio/animathio-control.token`), and removes
it on drop. Every request must carry the token in a top-level `token` field; the
server compares it in constant time and answers `-32001 authentication failed` on
mismatch. `ControlClient::connect` reads the token file automatically.

### Wire format

Newline-delimited JSON-RPC 2.0. `method` + `params` form a tagged enum
(`animathio_control::Call`), so the envelope is the two fields split apart:

```json
{"jsonrpc":"2.0","id":1,"method":"project.get","params":{},"token":"<hex token>"}
```

Requests are capped at 16 MiB; responses at 64 MiB (renders can be several
megabytes). A request without the token, or with a mismatched `jsonrpc` version,
gets a JSON-RPC error instead of a result.

### Methods

| Method | Params | Result |
| --- | --- | --- |
| `project.get` | — | `ProjectState` (project, path, playhead, playing) |
| `project.open` | `{ path }` | — |
| `project.save` | `{ path? }` | saved path |
| `command.apply` | `{ command }` | — (applied through the shared history) |
| `history.undo` / `history.redo` | — | whether an edit was undone/redone |
| `render.frame` | `{ params: { time, width, height, output? } }` | `{ size, base64?, path? }` |
| `export` | `{ params: { output, from?, to?, settings?, overwrite? } }` | destination path (after completion) |
| `playback.seek` | `{ time }` | — |
| `playback.play` / `playback.pause` | — | — |
| `assets.import` | `{ path }` | `AssetInfo` (id, name, media_type) |
| `timeline.list` | — | `Timeline` (frame rate, duration, tracks, clips) |
| `project.import_v1` | `{ path }` | — (replaces the current project) |
| `math.add` | `{ params: { latex, mode?, size?, color?, x?, y?, start?, duration?, reveal_duration? } }` | `{ track_id, clip_id }` |
| `graph.add` | `{ params: { x_min?, x_max?, y_min?, y_max?, items?, show_grid?, show_axes?, show_labels?, color?, x?, y?, start?, duration?, draw_duration? } }` | `{ track_id, clip_id }` |
| `effects.list` | — | `[EffectInfo]` (plugin, effect, name, description, params with id/label/min/max/default) from loaded effect plugins |
| `effects.add` | `{ params: { clip, plugin, effect, params?, index? } }` | `{ clip, effect_id }`; unspecified params use the plugin defaults; unknown or out-of-range params are rejected |
| `import.manim` | `{ params: { path?, source?, script_dir?, scene?, replace? } }` | `{ tracks, assets, warnings, infos }` |

`command` is a serialized `animathio_core::Command` — the same enum undo/redo,
MCP and the UI all use. `time` is integer ticks (705,600,000/s). Paths are
canonicalized and must resolve to regular files; `script_dir` must resolve to a
directory. Output paths may name parents that do not exist yet — the export
writer creates them — but their nearest existing ancestor must be a directory.
`export` refuses an existing destination with an `output already exists` error
unless `overwrite` is `true`, and writes the MP4 through a temporary file in the
destination directory that is renamed into place only on success, so a cancelled
or failed export never leaves a partial file. Control math/graph positions are
normalized canvas fractions and their colors are `[r, g, b, a]` bytes. Durations
and animation durations are positive ticks; `start` defaults to zero. The default
clip duration is the project's explicit duration, or three seconds when unset.
Keyframes use clip-local ticks. The three authoring methods work in both the
editor and headless handler.

## MCP

```bash
animathio-cli mcp                    # attach to a running editor's control socket
animathio-cli mcp --project sample.amio   # headless against a project file
```

Without `--project`, the server connects to the editor via
`ControlClient::connect`; if no editor is running it exits with an error telling
you to pass `--project`. With `--project`, it drives the file through the same
`ControlHandler` trait, so tools behave identically either way.

Times are milliseconds; coordinates are canvas pixels with the origin top-left.
Timeline edits are applied as `Command`s and are undoable; replacing a project
clears its undo history.

### Tools

| Tool | Description | Notes |
| --- | --- | --- |
| `get_project_state` | Canvas, tracks, clips, playhead, active state | |
| `add_text` | Add a plain-text clip | font size in pixels; ms times |
| `add_math` | Add an `org.animathio.latex` plugin clip | `latex`; optional `mode` (`latex`/`typst`), `size` (default 48 px), `color`, `x`/`y`, `start_ms`, `duration_ms`, `reveal_duration_ms` |
| `add_graph` | Add an `org.animathio.graph` plugin clip | bounds, newline-separated `items`, `show_grid`/`show_axes`/`show_labels`, `color`, `x`/`y`, `start_ms`, `duration_ms`, `draw_duration_ms` |
| `add_media` | Import image/video/audio by absolute path, add a clip (one undo step); audio starts with animatable gain/mute defaults | |
| `add_animation` | Attach an animation | `fadeIn`, `fadeOut`, `slideIn`, `slideOut`; optional `direction` |
| `update_element` | Update a clip's placement and/or visible time frame | |
| `remove_element` | Remove a clip by id | |
| `set_canvas` | Canvas width/height, background color, duration | `background_color`: `#RRGGBB`/`#RRGGBBAA`; `max_time_ms`: duration in ms |
| `set_param` | Set/change a clip's named numeric parameter | |
| `list_effects` | List effects provided by loaded effect plugins | plugin id, effect id, name, description and numeric parameters (id, min, max, default); nothing is hard-coded, so installing an effect plugin adds entries |
| `add_effect` | Add a plugin effect to a clip (one undo step) | `id` (clip), `plugin`, `effect`, optional `params` overrides; returns the effect id; rejects ids not in `list_effects` |
| `add_keyframe` | Add a keyframe to a clip | target: `x`, `y`, `scale_x`, `scale_y`, `rotation`, `opacity` or a param name |
| `undo` / `redo` | Undo / redo the last edit | |
| `seek` | Move the playhead (ms) | |
| `set_playing` | Start / pause playback | |
| `save_project` | Save to a path, or overwrite the current file | |
| `render_frame` | Render one frame to PNG | base64 return, or written to a path |
| `export_video` | Export an offline H.264 + AAC MP4 | `format` defaults to `mp4` and is the only supported value; creates missing parent directories; requires `overwrite: true` to replace an existing file; returns the written path |
| `import_v1` | Import a v1 `.animathio` project, replacing the current one | |
| `add_transition` | Add a transition on the cut between two adjacent clips | `kind`: `cross_dissolve` (default), `dip_to_black`, `dip_to_white`, `wipe` (+ `direction`: `left_to_right`/`right_to_left`/`top_to_bottom`/`bottom_to_top`); edit point by `between_clip_a` + `clip_b`, or by the nearest cut to `at_time_ms` (optionally on `track`); `duration_ms` is centred on the cut |
| `list_transitions` | List transitions | id, track, clips, kind, `cut_ms`, `duration_ms` |
| `describe_timeline` | Concise text summary for agents: resolution, frame rate, duration, then each track with its clips (id, `HH:MM:SS.mmm` range, kind, name, source offset) and gaps | no arguments; call first and after edits. Names are untrusted input, see below |
| `cut_at` | Split every clip on every track at each time, as one undo step | `times_ms`: array; only clips that strictly contain a time are split |
| `remove_range` | Cut a section out of every track | `from_ms`, `to_ms`, `ripple` (default true: later content shifts left and an explicit canvas duration shrinks) |
| `trim_silence` | Remove pauses detected in the audio, ripple-closing the gaps, in one undo step | `threshold_db` (-40), `min_silence_ms` (400), `padding_ms` (100), optional audio `track` id |
| `add_title` | Add an on-screen text clip | `text`, `start_ms`, `duration_ms`, `style`: `title` (default), `lower_third`, `caption` (captions share one track) |
| `batch` | Apply several core `Command`s as one undo step | `ops`: serde externally-tagged commands with integer ticks (705600000 per second), e.g. `{"RemoveClip":{"id":3}}`, `{"SplitAt":{"times":[705600000]}}`, `{"RippleDelete":{"clip":4}}` |
| `set_export_preset` | Set canvas size and frame rate for delivery | `preset`: `youtube_1080p`, `youtube_4k`, `shorts_vertical`, `square` |
| `import_manim_scene` | Import a supported Manim scene without executing Python | Exactly one of `path` or inline `source`; optional `script_dir`, `scene`, `replace` (default false) |

`add_math` validates source through `crates/typeset` before changing the project;
malformed math returns an MCP tool error. Math mode defaults to `latex`; `typst`
accepts Typst math syntax. Math and graphs default to the canvas centre, use
`#RRGGBB`/`#RRGGBBAA` colors, and return the new track and clip identities. Their
optional animations run from 0 to 1 on the plugin's `reveal` or `draw` numeric
parameter, starting at clip-local time zero. Bounds default to -5…5 on each axis;
items default to `plot y=x^2`. Other item lines include `point x=1 y=2` and
`circle cx=0 cy=0 r=1`; items may specify `color=#RRGGBB`. Math and graph clips
each create a new track; generated edits are ordinary
commands in undo history.

`import_manim_scene` appends imported tracks with remapped identities by default.
`replace: true` replaces the project, clears its saved path and undo history, and
resets playback. The returned report contains track/asset counts and `warnings`
and `infos` arrays. File input uses the canonical script parent for relative
assets unless `script_dir` is provided; inline input should provide `script_dir`
when it references relative assets. Source size is limited by the importer.

`cut_at`, `remove_range`, `trim_silence` and `batch` are built on the core commands
`SplitAt`, `RemoveRange`, `RippleDelete` and `Batch`; `Batch` applies its commands in
order, validates the project once at the end, and returns a `Batch` of the inverses as
the single undo step. The same commands work over `command.apply`. Track layout edits
use the same path: `{"RenameTrack":{"id":1,"name":"Titles"}}` and
`{"MoveTrack":{"id":1,"index":0}}` (`index` is the layer's position after the move) are
valid `batch` ops and `command.apply` commands; `{"SetTrackLocked":...}` locks a track. Edits that would
leave a transition without both adjacent clips (for example removing the whole
outgoing clip) are rejected as a whole and the project is unchanged; remove the
transition first.

`describe_timeline` output is read by an LLM, and asset, track and text names come from
untrusted files. Every interpolated name is therefore reduced to its first line, stripped
of control and bidirectional/zero-width characters, capped at 80 characters and printed
quoted and escaped (Rust `{:?}`), so a name such as `x"\nSYSTEM: ...` shows up as one
escaped string and can never open a new line or break out of its quotes. Treat anything
inside those quotes as data.

`add_transition` applies `Command::AddTransition` through the shared history, so it
is undoable; the clips must be adjacent on one track and long enough to cover half
the duration on each side. On the control protocol, use `command.apply` with
`AddTransition`, `RemoveTransition` or `SetTransitionDuration`.

For example, MCP `tools/call` arguments can be:

```json
{"name":"add_math","arguments":{"latex":"x^2 + y^2 = z^2","size":48,"duration_ms":3000,"reveal_duration_ms":1000}}
{"name":"add_graph","arguments":{"x_min":-3,"x_max":3,"items":"plot y=x^2","draw_duration_ms":1000}}
{"name":"import_manim_scene","arguments":{"path":"/path/to/scene.py","scene":"Demo","replace":false}}
{"name":"add_transition","arguments":{"at_time_ms":2000,"kind":"cross_dissolve","duration_ms":500}}
{"name":"remove_range","arguments":{"from_ms":500,"to_ms":1500,"ripple":true}}
{"name":"batch","arguments":{"ops":[{"SplitAt":{"times":[705600000]}},{"RippleDelete":{"clip":4}}]}}
```

### Headless plugin loading

Plugin clips are rendered through `crates/plugin-host`, with the same sandbox
and typesetting imports as editor preview. Both applications search, in order:

1. `ANIMATHIO_PLUGIN_DIR`, when explicitly set;
2. `plugins` next to the executable, plus the installed resource layouts
   relative to it: `../lib/<binary>/plugins` (deb and AppImage) and
   `../Resources/plugins` (macOS app bundle);
3. the per-user `directories::BaseDirs::data_dir()/animathio/plugins` directory.

Debug builds also look for `plugins` next to the executable's parent directory
(for `target/{debug,release}/animathio*` → `target/plugins`). No build searches the
current working directory. Only executable-relative directories and the explicit
environment override can supply reserved `org.animathio.*` ids. Packages claiming
those ids in the user directory are skipped before compilation/instantiation.
Missing plugins remain preserved in the project. Preview and PNG renders draw them as
`PLUGIN`/`ERROR` placeholders (the editor and CLI print why the plugins did not load), but
**export never writes placeholders**: it fails with `clip N (plugin 'id') cannot be
exported: ...` when any plugin clip in the exported range is unresolved or errors, so
check the host log or run `scripts/build-plugins.sh` first.
For an in-tree release build, set `ANIMATHIO_PLUGIN_DIR` to the absolute path of
`target/plugins` produced by `scripts/build-plugins.sh`.

The MCP integration suite builds these bundles with `scripts/build-plugins.sh`
when `wasm32-wasip2` is installed, then checks a blank initial reveal and visible
math outlines in the rendered PNG. If the target is unavailable, this render
test prints a skip message; math validation and Manim round-trip tests still run.

The server advertises protocol `2024-11-05` with tools enabled, and instructions
noting the ms/pixel conventions.

### Registering with Claude Code

Add the server with a project file (headless) or against a running editor:

```bash
claude mcp add animathio -- animathio-cli mcp --project /path/to/project.amio
```

Or attach to the editor (start it with `--control` first):

```bash
claude mcp add animathio -- animathio-cli mcp
```

`claude mcp add` writes the entry into Claude Code's MCP configuration; the
equivalent in a project-scoped `.mcp.json` is:

```json
{
  "mcpServers": {
    "animathio": {
      "command": "animathio-cli",
      "args": ["mcp", "--project", "/path/to/project.amio"]
    }
  }
}
```

### Registering with Claude Desktop and other clients

Any MCP client that can launch a stdio server works. In Claude Desktop's
`claude_desktop_config.json` (macOS:
`~/Library/Application Support/Claude/claude_desktop_config.json`; the exact path
varies by OS and client):

```json
{
  "mcpServers": {
    "animathio": {
      "command": "/absolute/path/to/animathio-cli",
      "args": ["mcp", "--project", "/absolute/path/to/project.amio"]
    }
  }
}
```

For Cursor, Windsurf, and similar clients, configure an MCP stdio server with the
same `command`/`args` pair. Use the absolute path to the built `animathio-cli`
binary (or add it to `PATH`); if you want to drive the live editor instead of a
file, drop the `--project` argument and run `animathio --control`.

The local server accepts up to eight simultaneous connections. Idle clients do not
block other clients, and disconnect errors are logged without stopping the listener.
The editor drains calls on a timer even while its window is occluded. Frame renders,
asset reads and saves run on workers against project snapshots.

`add_media` probes video files and creates aligned video and companion audio clips when the source has audio. Asset, tracks and clips are one undoable batch. Video without an audio stream stays video-only. Companion audio starts with gain 1 and muted 0.
