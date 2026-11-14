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
| `media.add` | `{ path }` | track_id, clip_id; bounded import and placement in one undo step, with a single video/audio probe before retaining bytes. The clip takes the probed media length (images: the project duration, else three seconds); placing a file that is already in the project reuses its asset |
| `assets.import` | `{ path }` | `AssetInfo` (id, name, media_type) |
| `timeline.list` | — | `Timeline` (frame rate, duration, tracks, clips) |
| `project.import_v1` | `{ path }` | — (replaces the current project) |
| `math.add` | `{ params: { latex, mode?, size?, color?, x?, y?, start?, duration?, reveal_duration? } }` | `{ track_id, clip_id }` |
| `graph.add` | `{ params: { x_min?, x_max?, y_min?, y_max?, items?, show_grid?, show_axes?, show_labels?, color?, x?, y?, start?, duration?, draw_duration? } }` | `{ track_id, clip_id }` |
| `titles.list` | — | Bundled title presets and editable fields |
| `titles.add` | `{params: {preset, fields?, range: {start, end}}}` | `{track_id, clip_id}`; linked plugin composition, one undo step |
| `animations.list` | — | `[AnimationPresetInfo]` (plugin, preset, name, description, role `in`/`out`/`emphasis`, applies_to, default_duration in ticks, options with id/label/kind/default/min/max/options) from loaded animation-preset plugins |
| `animations.apply` | `{ params: { clip, preset, plugin?, duration?, options? } }` | `{ clip, keyframes }`; applies the preset as one undoable `Batch` of keyframes (`plugin` defaults to `org.animathio.animations`, `duration` in ticks to the preset's, clamped to the clip); unknown presets, unfitting clips and invalid options are rejected |
| `effects.list` | — | `[EffectInfo]` (plugin, effect, name, description, params with id/label/min/max/default) from loaded effect plugins |
| `effects.add` | `{ params: { clip, plugin, effect, params?, index? } }` | `{ clip, effect_id }`; unspecified params use the plugin defaults; unknown or out-of-range params are rejected |
| `audio_effects.list` | — | `[EffectInfo]` (same shape as `effects.list`) from loaded audio-effect plugins |
| `audio_effects.add` | `{ params: { clip? \| track?, plugin, effect, params?, index? } }` | `{ chain, effect_id }` where `chain` is `{ "Clip": id }` or `{ "Track": id }`; give exactly one of `clip` and `track`; unspecified params use the plugin defaults; unknown or out-of-range params are rejected |
| `timeline.propose_autocuts` | `{ params: { silence_db?, hysteresis_db?, metric?, min_silence?, keep_short_pauses?, padding_before?, padding_after?, clip?, track?, range?, all_unlocked_tracks?, fillers?, language?, filler_words?, model?, download_model? } }` | snapshot-bound proposal `{ revision, tracks, cuts: [{ range: { start, end }, reason, confidence, enabled }], total_saved }`; all times are ticks; no mutation |
| `timeline.apply_autocuts` | `{ proposal }` | removed ranges and total removed ticks; one undoable ripple batch; rejects stale project revisions, invalid/overlapping ranges, locked tracks and missing companion tracks |
| `timeline.trim_silence` | `{ params: { threshold_db?, min_silence?, padding?, track? } }` | removed ranges; detects silent audio (RMS in 10 ms windows) and ripple-removes it from every track as one undo step; `min_silence` and `padding` are ticks |
| `markers.list` | — | `[Marker]` sorted by time: `{ id, time, duration?, name, color, note }` (ticks) |
| `markers.add` | `{ params: { time, duration?, name?, color?, note? } }` | `{ id }`; one undo step; `color` is `red`, `orange`, `yellow`, `green`, `cyan`, `blue` (default), `purple` or `pink` |
| `markers.remove` | `{ id }` | — (one undo step) |
| `beats.detect` | `{ params: { clip?, track?, range?, min_bpm?, max_bpm?, bpm?, beats_per_bar? } }` | `{ bpm, confidence, beats: [{ time, strength }], downbeats }` in timeline ticks; no mutation; silent or very short audio is an error |
| `beats.add_markers` | `{ params: { clip?, track?, range?, min_bpm?, max_bpm?, bpm?, beats_per_bar?, every?, downbeats_only?, color?, name_prefix? } }` | `{ added, bpm, confidence }`; one batch adding a marker at each selected beat; beats that already carry a marker are skipped |
| `timeline.sync_cuts_to_beats` | `{ params: { clips, source?, mode?, max_shift?, min_segment?, frame_align?, every?, downbeats_only?, detect? } }` | `{ shifted, splits, unchanged, skipped, bpm? }`; one batch, see [Markers and beat sync](#markers-and-beat-sync) |
| `import.manim` | `{ params: { path?, source?, script_dir?, scene?, replace? } }` | `{ tracks, assets, warnings, infos }`; runs in the sandboxed `org.animathio.import-manim` plugin, which must be loaded |
| `video.reframe` | `{ params: { aspect?, smoothness?, clips? } }` | video clip count; one undo batch; editor replies after its cancellable worker completes |
| `subtitles.generate` | `{ params: { model?, language?, clip?, track?, range?, download_model?, split?, style? } }` | `{ language, added?: { track_id, background_track_id?, first_clip, cues }, cues: [{ start, end, text }] }`; transcribes the project's audio (or one clip, track or `range`) locally with Whisper and adds caption clips on a new `Subtitles` track as one undo step. `model` is `tiny`, `base` (default) or `small`; a missing model is an error unless `download_model` is true. `style` is a caption style choice (see `subtitles.style`; default `classic`) and `background_track_id` is absent for styles without a bar. The editor answers when the job finishes (it can take minutes) |
| `subtitles.export` | `{ params: { format?, output?, track?, overwrite? } }` | `{ format, track, cues, text?, path? }`; renders a subtitle track (default: the first track named `Subtitles` with text clips) as `srt` or `vtt`, returning the text or writing `output` (a `.vtt` extension selects WebVTT) |
| `subtitles.style` | `{ params: { track?, style, font_size?, color?, accent?, outline?, position?, max_words? } }` | `{ style, captions }`; restyles the existing caption clips of `track` (default: every `Subtitles*` track with captions) as one undo step. `style` is `classic`, `word-highlight`, `karaoke`, `pop`, `one-word` or `bold-outline`; `font_size` is relative to the shorter canvas edge (0.01-0.2), `color`/`accent`/`outline` are RGBA byte arrays, `position` is `top`, `middle` or `bottom` (safe-area aware) and `max_words` is the words per line (0 fits the box). Classic keeps (or re-adds) the translucent bar clips, the other styles remove them |
| `subtitles.import` | `{ params: { path, offset?, style? } }` | same shape as `subtitles.generate`; adds the cues of an SRT or WebVTT file as caption clips (one undo step); `offset` is signed ticks |

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
Clip and track ids may be passed as strings (`"3"`) or numbers (`3`), matching
how `describe_timeline` prints them.
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
| `list_animation_presets` | List animation presets provided by loaded animation-preset plugins | plugin id, preset id, name, role (`in`, `out`, `emphasis`), clip kinds, default duration (ticks) and options; first-party `org.animathio.animations` offers fade, slide, zoom, pop, spin, bounce, wipe and typewriter in/out plus `breathe`, `shake`, `spin`, `hop` and `flash` |
| `apply_animation_preset` | Apply a preset to a clip as ordinary keyframes (one undo step) | `id` (clip), `preset`, optional `plugin`, `duration_ms`, `options` (by option id); returns the keyframe count |
| `list_effects` | List effects provided by loaded effect plugins | plugin id, effect id, name, description and numeric parameters (id, min, max, default); nothing is hard-coded, so installing an effect plugin adds entries |
| `add_effect` | Add a plugin effect to a clip (one undo step) | `id` (clip), `plugin`, `effect`, optional `params` overrides; returns the effect id; rejects ids not in `list_effects` |
| `list_audio_effects` | List audio effects provided by loaded audio-effect plugins | same shape as `list_effects` (plugin id, effect id, name, description, numeric parameters in dB/Hz/ms with min/max/default); first-party `org.animathio.audio-basic` offers `gain`, `eq3`, `compressor`, `gate`, `highpass` and `voice_cleanup` |
| `add_audio_effect` | Add an audio effect to a clip's own chain (`id`) or a track's chain (`track`) (one undo step) | exactly one of `id` (clip) and `track`; `plugin`, `effect`, optional `params` overrides and `index`; returns `{chain, effect_id}`. Clip chains run on the clip's signal right after its `gain`; track chains run on the summed track before its gain. Rejects ids not in `list_audio_effects` |
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
| `ripple_delete` | Delete one clip and close the gap on its own track | `id`; later clips on that track move left, other tracks are not shifted (use `remove_range` to keep tracks in sync); linked companions are deleted with it |
| `remove_range` | Cut a section out of every track | `from_ms`, `to_ms`, `ripple` (default true: later content shifts left and an explicit canvas duration shrinks) |
| `propose_autocuts` | Review automatic cuts without editing; returns a proposal with tick ranges, reason, heuristic confidence and enabled toggles | `silence_db` (-40), `min_silence_ms` (500), `hysteresis_db` (3), `peak` (false), `keep_short_pauses` (true), `padding_before_ms` / `padding_after_ms` (100), optional `clip`, `track`, paired `from_ms` / `to_ms`, `all_unlocked_tracks` (false), `fillers` (false), `language`, `filler_words`, `model` (base), `download_model` (false) |
| `apply_autocuts` | Apply the reviewed proposal as one undo step; selected tracks and linked companions ripple together | `proposal`: the exact object from `propose_autocuts`, with unwanted `cuts[].enabled` set to false. Coordinates are ticks in the original project; stale proposals are rejected |
| `list_markers` | List timeline markers | no arguments; returns `{markers: [{id, time_ms, ticks, duration_ms, name, color, note}]}` |
| `add_marker` | Add a marker (one undo step) | `time_ms`, optional `name`, `color`, `note`, `duration_ms`; returns `{id}` |
| `remove_marker` | Remove a marker (one undo step) | `id` |
| `detect_beats` | Detect tempo and beats offline from the audio, without editing | optional `clip`, `track`, paired `from_ms` / `to_ms`, `min_bpm` (60), `max_bpm` (200), `bpm`, `beats_per_bar` (4); `add_markers` (false) also adds markers at the selected beats (`every`, `downbeats_only`, `color`) as one undo step. Returns `bpm`, `confidence`, `beats_ms`, `beat_ticks`, `downbeats` |
| `sync_cuts_to_beats` | Snap the selected clips' cuts to beats/markers, or split them there, as one undo step | `clips` (ids), `source` (`beats`, `markers`, `both`), `mode` (`snap`, `split`), `max_shift_ms` (250), `min_segment_ms` (100, split only), `frame_align` (true), `every`, `downbeats_only`, `analyze_clip` / `analyze_track` / `bpm` / `beats_per_bar` for the beat analysis |
| `trim_silence` | Remove pauses detected in the audio, ripple-closing the gaps, in one undo step | `threshold_db` (-40), `min_silence_ms` (400), `padding_ms` (100), optional audio `track` id |
| `auto_reframe` | `aspect?`, `smoothness?`, `clips?` | Subject-following portrait/square video; changes canvas and position/scale animation as one undo step; returns `{clips}` |
| `auto_subtitles` | Fully local automatic subtitles: transcribe the project's audio (or one `clip`/`track`, or `from_ms`..`to_ms`) with Whisper and add caption-bar clips on a new `Subtitles` track, one undo step | `model` (`tiny`/`base`/`small`, default `base`), `language` (ISO 639-1, omit to detect), `download_model` (false; set only with the user's consent: one-time 78-488 MB download, SHA-256 verified), `max_chars_per_line` (42), `max_lines` (2), `style` (`classic` default, `word-highlight`, `karaoke`, `pop`, `one-word`, `bold-outline`: animated CapCut-style captions driven by Whisper's per-word timings) with optional `font_size`, `color`, `accent`, `outline` (`#RRGGBB[AA]`), `position` (`top`/`middle`/`bottom`) and `max_words`. Returns the language, track ids and the cues in ms. Needs a build with the `whisper` feature; blocks until done |
| `export_subtitles` | Return or write the subtitle track as SRT or WebVTT | `format` (`srt`/`vtt`), `track`, `path`, `overwrite` |
| `import_subtitles` | Add an SRT or WebVTT file as caption clips on a new `Subtitles` track; words are spread evenly over each cue for the animated styles | `path`, `offset_ms`, `style` and the look options of `auto_subtitles` |
| `set_caption_style` | Restyle existing captions (from `auto_subtitles`/`import_subtitles`) as one undo step; adds or removes the background bars as the style needs | `style` (required), `track`, `font_size`, `color`, `accent`, `outline`, `position`, `max_words` |
| `list_templates` | List bundled and user project manifests and placeholders | — |
| `create_from_template` | Create, save and open a project from a template, optionally filling text/media | `template`, `output`, optional `text`, `media`, `cover` |
| `list_title_presets` | List title compositions and their editable fields | — |
| `add_title` | Add animated linked title compositions, or legacy single text clips | `start_ms`, `duration_ms`; `preset` + `fields` (see below), or `text` + legacy `style`: `title` (default, centred), `lower_third` (lower left), `caption` (captions share one track); text is centred on its position |
| `batch` | Apply several core `Command`s as one undo step | `ops`: serde externally-tagged commands with integer ticks (705600000 per second), e.g. `{"RemoveClip":{"id":3}}`, `{"SplitAt":{"times":[705600000]}}`, `{"RippleDelete":{"clip":4}}` |
| `set_export_preset` | Set canvas size and frame rate for delivery | `preset`: `youtube_1080p`, `youtube_4k`, `shorts_vertical`, `square`, `portrait` |
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
when it references relative assets. The import runs inside the
`org.animathio.import-manim` importer plugin (API 1.1), which reads image files only
below that directory through the bounded `read-sibling` host import; the call fails
with a hint to run `scripts/build-plugins.sh` when the plugin is not loaded. Source
size is limited by the importer and the host (`max_import_source_bytes`).

`apply_animation_preset` (control method `animations.apply`) asks the preset plugin
for keyframe curves once and applies them as a single `Batch`; the project stores only
ordinary keyframes, so rendering never needs the plugin and `undo` restores the
previous keys exactly. In presets occupy the start of the clip, out presets its end
and emphasis presets the middle (the whole clip by default); existing keys of the
animated properties inside that window are replaced. `add_animation` is the v1-parity
tool (fadeIn/fadeOut/slideIn/slideOut) and does not use plugins.

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

### Title preset library

`list_title_presets` returns the bundled composition definitions, including stable
ids, editable `field` names/defaults, plugin layers and entry/exit motion.
`add_title` accepts `preset` and a `fields` object; omitted fields use the preset's
defaults. `text`, when supplied, fills the first text field unless that field is
already in `fields`. The returned `clip_id` identifies an editable text layer.
Every layer shares a fresh `link_group`; the complete composition, including its
tracks and keyframes, is one undo step. Existing calls using only `style` retain
their single-clip behavior.

```json
{"name":"add_title","arguments":{"preset":"lower_third","fields":{"name":"Jane Doe","role":"Researcher"},"start_ms":5000,"duration_ms":4000}}
```

| Preset id | Editable fields | Motion |
|---|---|---|
| `centered_title` | `title` | Fade |
| `title_subtitle` | `title`, `subtitle` | Fade |
| `lower_third` | `name`, `role` | Fade + slide; rounded backdrop |
| `caption` | `caption` | Fade; translucent backdrop |
| `chapter_card` | `title`, `subtitle` | Fade; full-frame backdrop |
| `end_card` | `title`, `cta` | Fade |
| `kinetic_word_pop` | `word` | Fade + scale |
| `math_title` | `latex`, `caption` | Fade; requires the LaTeX plugin |

The same surface is available directly over control as `titles.list` (no params)
and `titles.add` with `{"params":{"preset":"lower_third","fields":{"name":"Jane Doe","role":"Researcher"},"range":{"start":3528000000,"end":6350400000}}}`.
Control ranges and animation keys use integer ticks. Unknown presets/fields,
blank fields and fields longer than 4096 bytes are rejected without changing the
project. New title layers use the text/shapes/LaTeX plugins and adapt to the canvas
aspect ratio, with an eight-percent resting safe area. Backdrops for chapter cards
intentionally cover the whole canvas.

The shared project/export choices now include `portrait` (1080x1350, 4:5) alongside
`youtube_1080p`, `youtube_4k`, `shorts_vertical` and `square`, all at 30 fps when applied.
The editor labels vertical delivery “Shorts/Reels/TikTok 1080x1920 30fps”.

### Markers and beat sync

Markers are project-level, named, coloured points or ranges on the ruler (schema 5). They
never affect rendering. `list_markers`, `add_marker` and `remove_marker` (or the
`AddMarker`, `RemoveMarker`, `MoveMarker` and `UpdateMarker` commands through `batch` and
`command.apply`) edit them; every edit is one undo step.

`detect_beats` decodes the project mix (or one clip, track or range) to 16 kHz mono, so
clip and track gain apply and effect chains are ignored, then estimates the tempo and
tracks the beats. It is deterministic: the same project always gives the same beats. Beat
times are timeline positions (clip offsets included). Downbeats are the beats on the
strongest phase of `beats_per_bar`.

`sync_cuts_to_beats` edits the listed `clips` (linked companions follow, so list a video
clip or its audio, not both):

- `mode: snap` moves each clip edge to the nearest target within `max_shift_ms`. An edge
  shared with an adjacent clip is a roll edit: both clips move to the target and their
  source content stays in place. A lone edge is trimmed. Moving a later edge past the end
  of the media extends the clip past its footage, so keep `max_shift_ms` small for video.
- `mode: split` cuts each clip at every target strictly inside it that leaves segments of at
  least `min_segment_ms`.
- Targets are the analysed beats (every Nth or downbeats only), the project markers (a range
  marker contributes its start and end) or both. With `source: markers` no audio is needed.
- Edits that would overlap another clip, collapse a clip or touch a locked track are skipped
  and listed in `skipped`; the call fails if nothing could change. Edits are rounded to the
  frame grid unless `frame_align` is false.

### Automatic-cut review

Silence analysis reuses the dry 16 kHz mono project mix (clip gain, keyframes, mute,
solo and track gain; audio effects are ignored). RMS or peak is measured in 10 ms
windows. Silence starts below the entry threshold and ends at the entry threshold
plus hysteresis. The minimum applies to the raw pause; padding keeps the end of the
previous speech and the start of the next speech. Disabling keep-short-pauses cuts
all detected pauses that survive padding. Detection is limited to the selected
clip, track or range. Video selection resolves linked companion audio.

Application cuts the selected tracks and their complete companion-track graph;
without a selection it cuts unlocked tracks. Selecting all unlocked tracks expands
this scope. A locked companion causes an error so audio/video cannot drift. Scoped
ripple leaves other tracks and explicit project duration unchanged; when every track
is selected the explicit duration also shrinks. Review up to 1000 ranges at once.
Application removes enabled intervals from last to first in one batch.

Filler detection requires a build with the whisper feature and a local model. It uses
word timestamps, never estimated segment-level word positions. English defaults are
um, uh, erm, like, you know; Hungarian defaults are öö, ö, ő, hát, izé, szóval.
Other languages need an explicit filler list. Immediate repeated words and
hyphen-marked false starts are proposed with lower heuristic confidence. These
scores are review hints, not calibrated speech-engine probabilities. Missing engines
report a capability error; model downloads require explicit consent.

In the editor, Clip > Automatic cuts opens `apps/animathio/src/editor/autocut.rs`: analysis runs on a
worker, enabled ranges shade the affected timeline rows, and the panel offers sliders,
per-range toggles, seek buttons, saved duration and Apply/Cancel. Edits invalidate the
draft; Apply makes one undo entry. Cancel discards the draft and signals the worker.

`list_templates` returns bundled and user template manifests (`id`, `name`, `description`,
`aspect`, `category`, `placeholders`); each placeholder has its `clip` id, `label` and
`accepts_text` (true for text and LaTeX clips). `create_from_template` takes `template`
(catalog id), `output` (new `.amio` path), optional `text` and `media` maps (decimal
placeholder clip id to replacement text or local image/video path) and `cover` (default
false: contain). It refuses existing destinations, unknown placeholders, audio files and a
placeholder present in both maps, writes the project and opens it through the current
control backend. Replacement swaps the clip's source in place, retaining its timing,
transform, effects, animation and linked companions. The media is fitted to the
placeholder's box through the clip's `media_fit` parameter (`cover` crops, `contain`
letterboxes), honoured by both render backends. User templates live in the platform user
data directory under `templates`, separate from the project schema; the editor's
File > Save as template… writes them and the launcher lists them with a delete action.

## Auto-reframe

`auto_reframe` / `video.reframe` share offline deterministic motion, contrast and
edge-energy tracking. `aspect` is `9:16` (default, 1080×1920), `1:1` (1080×1080) or
`4:5` (1080×1350); `smoothness` is 0..=1 (default 0.5, higher follows more slowly).
`clips` is a list of video ids, empty or omitted for all videos. Selected tracks
must be unlocked and videos unrotated (including rotation animation).

Analysis samples four frames per second and computes saliency at 96×54. A dead
zone and rate-limited follow reduce jitter; frame-difference spikes reset tracking
at scene cuts. Time-aware path simplification preserves both sides of detected
cuts and limits each clip to 256 centre points (512 position keys plus two constant
fill-scale keys). Analysis is
bounded to 20000 decoded samples per call and rejects clips with more scene cuts
than the key budget permits. Decode currently produces full-size frames before
thumbnailing. Cancellation is checked between decodes. A project edit during the
editor worker discards the draft. No project mutation occurs until completion.

Application changes the current canvas, preserves source aspect while filling it,
and replaces the targeted videos' position/scale curves; opacity, effects, timing,
audio and other layers' normalized transforms remain. Undo restores the complete
prior project in one step. Save explicitly after MCP use to retain changes.

This is a visual-saliency heuristic, without face identity or semantic subject
recognition. Multiple subjects, busy backgrounds, camera motion and short shots
can need manual correction; scene cuts are located only at sampled times. Skin-tone
priors, subject selection, rotated-video support and layout adaptation for titles
are deferred. The editor menu provides aspect buttons, a smoothness slider,
progress and cancellation; the CLI writes a separate archive by default.
