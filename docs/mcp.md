# Control protocol and MCP

AniMathIO 2.0 exposes two related surfaces:

1. **The control protocol** — a local, authenticated JSON-RPC 2.0 socket that is
   the single API for the editor UI, the CLI, plugins and the MCP sidecar.
2. **MCP** — a stdio server (`animathio-cli mcp`) that translates Model Context
   Protocol tool calls onto the control protocol (or drives a project file
   headlessly).

## Control protocol

### Transport and location

- **Unix:** a Unix-domain socket at `$XDG_RUNTIME_DIR/animathio-control.sock`.
- **Windows:** a named pipe anchored at the same per-user location.
- Never a listening TCP port.

The editor starts the server when launched with `--control` (or
`ANIMATHIO_CONTROL=1`).

### Authentication

On bind, the server generates a random **256-bit token**, writes it to a `0600`
file next to the socket (`$XDG_RUNTIME_DIR/animathio-control.token`), and removes
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
| `render.frame` | `{ time, width, height, output? }` | `{ size, base64?, path? }` |
| `export` | `{ output, from?, to?, settings? }` | destination path (after completion) |
| `playback.seek` | `{ time }` | — |
| `playback.play` / `playback.pause` | — | — |
| `assets.import` | `{ path }` | `AssetInfo` (id, name, media_type) |
| `timeline.list` | — | `Timeline` (frame rate, duration, tracks, clips) |
| `project.import_v1` | `{ path }` | — (replaces the current project) |

`command` is a serialized `animathio_core::Command` — the same enum undo/redo,
MCP and the UI all use. `time` is integer ticks (705,600,000/s). Paths are
canonicalized and must resolve to regular files.

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
All mutations are applied as `Command`s and are undoable.

### Tools

| Tool | Description | Notes |
| --- | --- | --- |
| `get_project_state` | Canvas, tracks, clips, playhead, active state | |
| `add_text` | Add a plain-text clip | font size in pixels; ms times |
| `add_math` | Add a LaTeX fragment | **Not implemented** — the tool returns `not yet supported` (the latex plugin is not wired into the MCP path) |
| `add_media` | Import image/video/audio by absolute path, add a clip | |
| `add_animation` | Attach an animation | `fadeIn`, `fadeOut`, `slideIn`, `slideOut`; optional `direction` |
| `update_element` | Update a clip's placement and/or visible time frame | |
| `remove_element` | Remove a clip by id | |
| `set_canvas` | Canvas width/height, background color, duration | `#RRGGBB`/`#RRGGBBAA`; ms |
| `set_param` | Set/change a clip's named numeric parameter | |
| `add_keyframe` | Add a keyframe to a clip | target: `x`, `y`, `scale_x`, `scale_y`, `rotation`, `opacity` or a param name |
| `undo` / `redo` | Undo / redo the last edit | |
| `seek` | Move the playhead (ms) | |
| `set_playing` | Start / pause playback | |
| `save_project` | Save to a path, or overwrite the current file | |
| `render_frame` | Render one frame to PNG | base64 return, or written to a path |
| `export_video` | Export an offline H.264 + AAC MP4 | `format` defaults to `mp4` and is the only supported value; returns the written path |
| `import_v1` | Import a v1 `.animathio` project, replacing the current one | |
| `import_manim_scene` | Build a scene from a Manim script | **Not implemented** — returns `not yet supported` (use `animathio-cli import-manim`) |

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
