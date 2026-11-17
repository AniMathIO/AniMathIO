# `animathio-cli` reference

Text, shape and effect renders require the bundled WASM components. Run
`scripts/build-plugins.sh` first; for unpackaged release binaries set
`ANIMATHIO_PLUGIN_DIR=target/plugins`. PNG renders, sequences and MP4 exports all
resolve clips through the plugin host.

Headless project validation, PNG rendering, MP4 export, importers and the MCP sidecar.
The `text` blocks below are the binary's own `--help` output: `scripts/check-docs.sh`
fails when they drift, and `scripts/check-docs.sh --fix-cli` regenerates them from a
built `animathio-cli`.

<!-- help:cli -->
```text
AniMathIO headless CPU renderer

Usage: animathio-cli <COMMAND>

Commands:
  set-clip-speed  Change a clip's speed/reverse/ramp, preserving its source interval and linked audio
  freeze-frame    Insert a frame hold at the playhead and ripple trailing clips
  math-objects    List or add typed objects from the editor Math panel
  templates       List the bundled project templates
  new             Create a project from a bundled template
  render          Render one frame to PNG
  render-seq      Render [from, to) frame indices to `frame_000000.png`, etc
  export          Export an offline H.264 + AAC MP4
  validate        Check model invariants and all asset content hashes
  import-media    Import media into a new project, including linked companion audio
  media-fit       Set one image/video clip's fit and save an edited project archive
  import-v1       Convert a v1 `.animathio` project to a `.amio` archive
  import-manim    Convert a Manim Community scene script to a `.amio` archive
  animations      List or apply animation presets from the loaded animation-preset plugins
  plugins         Install, enable, disable and remove community plugins, and switch them on or off
  transcribe      Transcribe speech locally (no cloud) and write SRT/WebVTT or add subtitle clips
  autocut         Propose or apply silence and optional filler cuts as one ripple edit
  learn-noise     Learn a denoise profile from a noise-only timeline interval
  duck            Lower music under speech with gain keyframes, as one undoable edit
  markers         List, add or remove timeline markers
  beats           Detect tempo and beats offline; optionally add markers at the beats
  scenes          Detect hard scene changes, split clips or add markers
  reframe         Reframe video to portrait or square with offline subject tracking
  models          List or download the local Whisper speech models
  new-sample      Write a three-second animated image/text/shape demo archive
  mcp             Run an MCP stdio server: connects to a running editor, or drives a project file headlessly when `--project` is given
  help            Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```
<!-- /help:cli -->

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
defaults to the project's canvas (640×360 when it is unset) unless overridden.

## `render`

Render one frame to PNG. Exactly one of `--frame` or `--time` is required.

<!-- help:render -->
```text
Render one frame to PNG

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
<!-- /help:render -->

```bash
animathio-cli render sample.amio --frame 30 -o frame.png
animathio-cli render sample.amio --time 1.25 -o timed.png
animathio-cli render sample.amio --time 705600000ticks -o one-second.png
animathio-cli render sample.amio --frame 0 --width 1280 --height 720 -o wide.png
```

## `render-seq`

Render the half-open frame range `[from, to)` to `frame_000000.png`, etc.

<!-- help:render-seq -->
```text
Render [from, to) frame indices to `frame_000000.png`, etc

Usage: animathio-cli render-seq [OPTIONS] --to <TO> --output <OUTPUT> <PROJECT>

Arguments:
  <PROJECT>  

Options:
      --from <FROM>      [default: 0]
      --to <TO>          Exclusive final frame index
  -o, --output <OUTPUT>  
      --width <WIDTH>    Overrides the project's output width
      --height <HEIGHT>  Overrides the project's output height
  -h, --help             Print help
```
<!-- /help:render-seq -->

```bash
animathio-cli render-seq sample.amio --from 0 --to 90 -o frames/
animathio-cli render-seq sample.amio --from 30 --to 31 --width 1920 --height 1080 -o frames/
```

## `export`

> Export fails (non-zero exit, no output file) when a plugin clip in the range cannot be rendered: `clip N (plugin 'id') cannot be exported: ...`. Build the plugins first (`scripts/build-plugins.sh`) or set `ANIMATHIO_PLUGIN_DIR`; `render` and `render-seq` still draw placeholders and print `warning:` lines for plugins that did not load. An export whose output is the input project file itself is refused with `the output path is the open project file; choose a different file name` — the same rule covers the `--chapters` sidecar, so a render can never destroy the project.

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

<!-- help:export -->
```text
Export an offline H.264 + AAC MP4

Usage: animathio-cli export [OPTIONS] --output <OUTPUT> <PROJECT>

Arguments:
  <PROJECT>  

Options:
  -o, --output <OUTPUT>      
      --from <FROM>          Inclusive start in decimal seconds or Nticks [default: 0]
      --to <TO>              Exclusive end in decimal seconds or Nticks; defaults to project duration
      --fps <FPS>            Output rate, e.g. 30 or 30000/1001
      --preset <PRESET>      Delivery preset: `youtube_1080p`, `youtube_4k`, `shorts_vertical`, `square` or `portrait`
      --crf <CRF>            Constant quality, 0..=51; lower is better (defaults to preset quality or 23)
      --overwrite            Replace an existing output file instead of failing
      --chapters <CHAPTERS>  Also write the project's markers as chapters to this sidecar file (`.txt` `YouTube` list, `.vtt` `WebVTT`, `.ffmeta` `FFmpeg` metadata). The MP4 muxer has no chapter atom, so chapters ship beside the video
      --bench                Report wall time, fps and opt-in stage timings
  -h, --help                 Print help
```
<!-- /help:export -->

```bash
animathio-cli export sample.amio -o out.mp4
animathio-cli export sample.amio -o shorts.mp4 --preset shorts_vertical
animathio-cli export sample.amio -o clip.mp4 --from 1.0 --to 3.0 --fps 30000/1001 --crf 18
animathio-cli export sample.amio -o renders/out.mp4 --overwrite
```

## `validate`

Check model invariants and all asset content hashes. It does not attempt video
decoding.

<!-- help:validate -->
```text
Check model invariants and all asset content hashes

Usage: animathio-cli validate <PROJECT>

Arguments:
  <PROJECT>  

Options:
  -h, --help  Print help
```
<!-- /help:validate -->

```bash
animathio-cli validate sample.amio
```

## `import-media`

Create a new `.amio` project from one video or audio file (the file is embedded as a
content-addressed asset and must be at most 1 GiB). A video with an audio stream
becomes linked video and audio clips on separate tracks (they move, trim, split and
delete together); a video without audio stays video-only. Use MCP `add_media` to add
media, including images, to an existing project.

<!-- help:import-media -->
```text
Import media into a new project, including linked companion audio

Usage: animathio-cli import-media --output <OUTPUT> <INPUT>

Arguments:
  <INPUT>  

Options:
  -o, --output <OUTPUT>  
  -h, --help             Print help
```
<!-- /help:import-media -->

```bash
animathio-cli import-media interview.mp4 -o interview.amio
```

## `import-v1`

Convert a v1 (Electron/TypeScript) `.animathio` project to an AniMathIO 2 `.amio` archive.
Non-fatal warnings are printed to stderr. See
[`docs/migrating-from-v1.md`](migrating-from-v1.md) for what maps.

<!-- help:import-v1 -->
```text
Convert a v1 `.animathio` project to a `.amio` archive

Usage: animathio-cli import-v1 --output <OUTPUT> <INPUT>

Arguments:
  <INPUT>  v1 `.animathio` project file (deflated JSON)

Options:
  -o, --output <OUTPUT>  Output `.amio` archive
  -h, --help             Print help
```
<!-- /help:import-v1 -->

```bash
animathio-cli import-v1 project.animathio -o imported.amio
```

## `import-manim`

Convert a Manim Community scene script (`.py`) to an AniMathIO 2 `.amio` archive.

<!-- help:import-manim -->
```text
Convert a Manim Community scene script to a `.amio` archive

Usage: animathio-cli import-manim [OPTIONS] --output <OUTPUT> <INPUT>

Arguments:
  <INPUT>  Manim scene script (`.py`)

Options:
      --scene <SCENE>    Import this scene class; defaults to the first `Scene` subclass
  -o, --output <OUTPUT>  Output `.amio` archive
  -h, --help             Print help
```
<!-- /help:import-manim -->

```bash
animathio-cli import-manim scene.py -o imported.amio
animathio-cli import-manim scene.py --scene MyScene -o imported.amio
```

The import runs inside the sandboxed `org.animathio.import-manim` importer plugin (build
it with `scripts/build-plugins.sh`); `ImageMobject` files are read only from the script's
folder.

## `plugins`

Manage community plugins: third-party `.amplugin` bundles in the per-user plugin folder.
They are off by default: the global switch (`plugins community on`) and a per-plugin flag
(`plugins enable <id>`) must both be on for a plugin to load, and an installed plugin starts
disabled. `install` takes a local file or an `https://` URL; a URL needs `--sha256`, which is
verified before anything is written, and the package must pass manifest and ABI validation.
Reserved `org.animathio.*` ids are refused and an id that is already installed needs
`--force` (an update). `list --json` prints the folder, the global switch and every installed
plugin. See [Installing community plugins](plugins.md#installing-community-plugins) for the
trust model and folders.

<!-- help:plugins -->
```text
Install, enable, disable and remove community plugins, and switch them on or off

Usage: animathio-cli plugins <COMMAND>

Commands:
  list       List the installed community plugins and whether each is enabled
  install    Install a community plugin from a local `.amplugin` file or an https URL. Installed plugins start disabled; run `plugins enable <id>` afterwards
  enable     Enable an installed community plugin
  disable    Disable a community plugin without removing it
  remove     Uninstall a community plugin: delete its file and its enabled flag
  community  Turn the global community plugins switch on or off; without an argument, show it
  help       Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```
<!-- /help:plugins -->

```bash
animathio-cli plugins install ./my-plugin.amplugin
animathio-cli plugins community on
animathio-cli plugins enable com.example.my-plugin
animathio-cli plugins list --json
```

## `animations`

List or apply animation presets from the loaded animation-preset plugins
(`org.animathio.animations`). `apply` writes the preset as ordinary keyframes on one clip
and saves the project as a `.amio` archive; in presets sit at the clip start, out presets
at its end and emphasis presets in its middle (the whole clip by default).

<!-- help:animations -->
```text
List or apply animation presets from the loaded animation-preset plugins

Usage: animathio-cli animations <COMMAND>

Commands:
  list   Print every preset with its role, default duration and options as JSON
  apply  Apply a preset to one clip as keyframes and write the project to a `.amio` archive
  help   Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```
<!-- /help:animations -->

```bash
animathio-cli animations list
animathio-cli animations apply project.amio --clip 3 --preset fade-in -o animated.amio
animathio-cli animations apply project.amio --clip 3 --preset slide-out --duration 0.75 \
    --option direction=left --option fade=true -o animated.amio
```

## `transcribe`

Local automatic subtitles. The input is a project (`.amio` or `project.json`; its audio
clips are mixed) or an audio/video file (first audio track). The speech engine is
whisper.cpp, built with `--features whisper` (needs cmake, a C++ compiler and libclang);
without it the command reports that this build has no engine. The Whisper model must be
installed: pass `--download` to fetch a missing one (SHA-256 verified), or fetch it first
with the `models` subcommand. With no `--srt`, `--vtt` or `--output`, the subtitles are
printed as SRT. `--style` picks the look of the caption clips added with `--output`:
`classic` (default), `word-highlight`, `karaoke`, `pop`, `one-word` or `bold-outline`; the animated
styles follow Whisper's per-word timings. `--position`, `--accent` and `--max-words` tune them.

<!-- help:transcribe -->
```text
Transcribe speech locally (no cloud) and write SRT/WebVTT or add subtitle clips

Usage: animathio-cli transcribe [OPTIONS] <INPUT>

Arguments:
  <INPUT>  A project (`.amio` or `project.json`; its audio tracks are mixed) or an audio/video file (first audio track)

Options:
      --model <MODEL>          Whisper model: `tiny`, `base` or `small` [default: base]
      --language <LANGUAGE>    ISO 639-1 language code (`en`, `de`, ...); detected automatically when omitted
      --download               Download the model first if it is not installed (verified, one-time)
      --srt <SRT>              Write `SubRip` subtitles here
      --vtt <VTT>              Write `WebVTT` subtitles here
  -o, --output <OUTPUT>        For project inputs: save a copy with the subtitle clips added to this `.amio`
      --track <TRACK>          Project input only: transcribe this track's audio (numeric id)
      --clip <CLIP>            Project input only: transcribe this clip's audio (numeric id)
      --from <FROM>            Project input only: inclusive start in decimal seconds or Nticks
      --to <TO>                Project input only: exclusive end in decimal seconds or Nticks
      --max-chars <MAX_CHARS>  Longest subtitle line in characters
      --max-lines <MAX_LINES>  Most lines per subtitle
      --style <STYLE>          Caption style for the clips added with `--output`: `classic` (default), `word-highlight`, `karaoke`, `pop`, `one-word` or `bold-outline` [default: classic]
      --position <POSITION>    Caption placement: `top`, `middle` or `bottom` (style default when omitted)
      --accent <ACCENT>        Accent colour of the active word, `#RRGGBB[AA]`
      --max-words <MAX_WORDS>  Words per line for the caption style (0 fits the box width)
      --overwrite              Replace existing output files
  -h, --help                   Print help
```
<!-- /help:transcribe -->

```bash
cargo build -p animathio-cli --features whisper
animathio-cli transcribe talk.mp4 --download --srt talk.srt --vtt talk.vtt
animathio-cli transcribe lesson.amio --language de --output lesson-subtitled.amio
animathio-cli transcribe lesson.amio --style karaoke --position middle -o lesson-captions.amio
```

## `markers`

List, add or remove timeline markers (named, coloured points or ranges on the ruler;
see [architecture](architecture.md#markers-and-beat-sync)). `markers PROJECT list` prints
the markers as JSON with times in Nticks and seconds. `markers PROJECT add` takes
`--time` (required), `--duration`, `--name`, `--color` and `--note`, and
`markers PROJECT remove ID` removes one; both save atomically to the input unless
`--output` is given (existing separate outputs require `--overwrite`).
`markers PROJECT chapters` writes the markers as a chapter list (`--format youtube`, `vtt` or
`ffmetadata`; stdout unless `--output`), and `export --chapters FILE` writes the same list as
a sidecar for the exported range (the MP4 muxer has no chapter atom; embed an `ffmetadata`
sidecar with `ffmpeg -i out.mp4 -i out.ffmeta -map_metadata 1 -codec copy final.mp4`).
Markers move with ripple edits and autocut; see
[architecture](architecture.md#markers-and-beat-sync).

```bash
animathio-cli markers song.amio list
animathio-cli markers song.amio chapters -o chapters.vtt
animathio-cli export song.amio -o song.mp4 --chapters song.txt
```

<!-- help:markers -->
```text
List, add or remove timeline markers

Usage: animathio-cli markers <PROJECT> <COMMAND>

Commands:
  list      Print every marker as JSON (times are Nticks and decimal seconds)
  add       Add a marker, saving atomically to the input unless --output is given
  chapters  Write the markers as chapters: a `YouTube` description list, `WebVTT` chapters or `FFmpeg` chapter metadata. Prints to stdout unless --output is given
  remove    Remove a marker by id, saving atomically to the input unless --output is given
  help      Print this message or the help of the given subcommand(s)

Arguments:
  <PROJECT>  Input project archive or project.json

Options:
  -h, --help  Print help
```
<!-- /help:markers -->

## `beats`

Detect tempo and beats offline from the decoded audio (the whole mix, or one `--clip`,
`--track` or `--from`/`--to` range) and print them as JSON: `bpm`, `confidence`, beat
`time` in Nticks and `seconds`, and `downbeats` indices. Analysis is deterministic and
never changes the project. With `--add-markers` it also adds a marker at each selected beat
(`--every N` or `--downbeats-only`, `--color`) as one batch and saves atomically to the
input, or to `--output`. Needs audio clips with a clear pulse; silence and audio shorter
than two seconds are errors.

```bash
animathio-cli beats song.amio --clip 1 --min-bpm 80 --max-bpm 180
animathio-cli beats song.amio --add-markers --downbeats-only -o marked.amio
```

<!-- help:beats -->
```text
Detect tempo and beats offline; optionally add markers at the beats

Usage: animathio-cli beats [OPTIONS] <PROJECT>

Arguments:
  <PROJECT>  Input project archive or project.json

Options:
      --clip <CLIP>                    Analyse a clip, including linked audio for a video clip
      --track <TRACK>                  Analyse this track's audio mix
      --from <FROM>                    Inclusive analysis start in seconds or Nticks (requires --to)
      --to <TO>                        Exclusive analysis end in seconds or Nticks (requires --from)
      --min-bpm <MIN_BPM>              Slowest tempo considered, in beats per minute [default: 60]
      --max-bpm <MAX_BPM>              Fastest tempo considered, in beats per minute [default: 200]
      --bpm <BPM>                      Force this tempo instead of estimating it
      --beats-per-bar <BEATS_PER_BAR>  Beats per bar, used to pick downbeats [default: 4]
      --add-markers                    Add a marker at each selected beat, saving atomically to the input unless --output is given
      --every <EVERY>                  With --add-markers: use every Nth beat [default: 1]
      --downbeats-only                 With --add-markers: only mark downbeats
      --color <COLOR>                  With --add-markers: marker colour [default: cyan]
  -o, --output <OUTPUT>                Save an edited archive here instead of changing the input
      --overwrite                      Replace an existing output archive
  -h, --help                           Print help
```
<!-- /help:beats -->

## `models`

List the Whisper models (`tiny`, `base`, `small`) and whether they are installed, or
download one into the user data directory (`ANIMATHIO_MODEL_DIR` overrides it).

<!-- help:models -->
```text
List or download the local Whisper speech models

Usage: animathio-cli models [OPTIONS]

Options:
      --download <DOWNLOAD>  Download this model (`tiny`, `base` or `small`) into the user data directory, verifying its SHA-256; resumes an interrupted download
  -h, --help                 Print help
```
<!-- /help:models -->

```bash
animathio-cli models
animathio-cli models --download base
```

## `new-sample`

Write a three-second animated image/text/shape demo archive (the same sample the
editor opens with `--sample`).

<!-- help:new-sample -->
```text
Write a three-second animated image/text/shape demo archive

Usage: animathio-cli new-sample --output <OUTPUT>

Options:
  -o, --output <OUTPUT>  
  -h, --help             Print help
```
<!-- /help:new-sample -->

```bash
animathio-cli new-sample -o sample.amio
```

## `mcp`

Run an MCP stdio server: connects to a running editor's control socket, or drives a
project file headlessly when `--project` is given. See [`docs/mcp.md`](mcp.md).

<!-- help:mcp -->
```text
Run an MCP stdio server: connects to a running editor, or drives a project file headlessly when `--project` is given

Usage: animathio-cli mcp [OPTIONS]

Options:
      --project <PROJECT>  Project file to operate on headlessly (a `.amio` archive or `project.json`)
  -h, --help               Print help
```
<!-- /help:mcp -->

```bash
animathio-cli mcp                    # attach to a running editor
animathio-cli mcp --project sample.amio
```

Export profiling: `export PROJECT -o OUTPUT --bench` reports total wall time and
frames per second (including media preparation, excluding plugin discovery).
Set `ANIMATHIO_EXPORT_TIMING=1` to also print process-wide stage timers:
`decode` (`decode-codec` + `decode-yuv-to-rgba`, on the media pool workers),
`render-inclusive` (the render call, including effects and decode waits),
`effects` / `effects-setup` (plugin calls and instantiation), `resolve-plugins`
(text/vector plugin resolution), `audio-mix`, and the encoder worker's
`encode-video` (`encode-rgba-to-yuv` + `encode-h264`), `encode-audio` and
`encode-flush-mux`. Stages run on different threads and nest, so the numbers
overlap and must not be added together; compare them with the wall time.

For the reproducible 10-second 1080p30 MP4/audio, text title, one-second dissolve
and grayscale effect workload, build the release CLI and first-party plugins,
then run `python3 scripts/bench-export.py [OUTPUT_DIRECTORY]` (requires ffmpeg).
The script retains the source, project and exported MP4 for inspection.

## `autocut`

Analyse a recording before applying silence/filler cuts. Exactly one of `--dry-run`
or `--apply` is required. Dry runs print a JSON proposal with integer-tick ranges,
reasons and heuristic confidence. Apply atomically saves the input project, or writes
a separate archive through `--output`; existing separate outputs require
`--overwrite`. Dry runs preserve the input. For per-cut review, use the editor or
MCP proposal and apply tools. See [automatic-cut review](mcp.md#automatic-cut-review) for scope,
padding, hysteresis and language rules.

```bash
animathio-cli autocut recording.amio --silence-db -40 --min-silence 0.5 --dry-run
animathio-cli autocut recording.amio --clip 1 --apply -o edited.amio
animathio-cli autocut recording.amio --fillers --language hu --dry-run
```

Filler/repeat analysis needs a build with the whisper feature. Models are local and
never bundled; `--download-model` grants consent to download a missing model.

<!-- help:autocut -->
```text
Propose or apply silence and optional filler cuts as one ripple edit

Usage: animathio-cli autocut [OPTIONS] <PROJECT>

Arguments:
  <PROJECT>  Input project archive or project.json

Options:
      --silence-db <SILENCE_DB>
          Silence entry threshold in dBFS [default: -40]
      --min-silence <MIN_SILENCE>
          Minimum pause in seconds or Nticks [default: 0.5]
      --hysteresis-db <HYSTERESIS_DB>
          Silence exit threshold above the entry level, in dB [default: 3]
      --peak
          Use peak amplitude instead of RMS
      --remove-short-pauses
          Also remove pauses shorter than --min-silence
      --padding-before <PADDING_BEFORE>
          Silence retained before speech, in seconds or Nticks [default: 0.1]
      --padding-after <PADDING_AFTER>
          Silence retained after speech, in seconds or Nticks [default: 0.1]
      --clip <CLIP>
          Analyse a clip, including linked audio for a video clip
      --track <TRACK>
          Analyse this track's audio mix
      --from <FROM>
          Inclusive analysis start in seconds or Nticks (requires --to)
      --to <TO>
          Exclusive analysis end in seconds or Nticks (requires --from)
      --all-unlocked-tracks
          Ripple all unlocked tracks, rather than selected tracks and companions
      --fillers
          Include filler phrases and repeats; requires a build with Whisper
      --language <LANGUAGE>
          ISO language code for speech recognition and filler defaults
      --filler-word <FILLER_WORDS>
          Filler phrase override; may be repeated
      --model <MODEL>
          Whisper model: tiny, base or small [default: base]
      --download-model
          Consent to download a missing model
      --apply
          Apply the proposed cuts, saving atomically to the input unless --output is given
      --dry-run
          Print the review proposal as JSON without saving changes
  -o, --output <OUTPUT>
          Save an edited archive here instead of changing the input
      --overwrite
          Replace an existing output archive
  -h, --help
          Print help
```
<!-- /help:autocut -->

## `templates`

`animathio-cli templates list` prints the five bundled template manifests as JSON,
including editable clip ids. These are six-second title compositions with no media.

<!-- help:templates -->
```text
List the bundled project templates

Usage: animathio-cli templates <COMMAND>

Commands:
  list  Print template metadata and placeholder clip ids as JSON
  help  Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```
<!-- /help:templates -->

## `new`

Create an independent project with `animathio-cli new --template quote -o quote.amio`.
The ids are `math_explainer`, `vertical_short`, `interview`, `quote` and `equation`.

<!-- help:new -->
```text
Create a project from a bundled template

Usage: animathio-cli new --template <TEMPLATE> --output <OUTPUT>

Options:
      --template <TEMPLATE>  Bundled template id from templates list
  -o, --output <OUTPUT>      Destination .amio archive
  -h, --help                 Print help
```
<!-- /help:new -->

### Portable template directories

A `.amiotemplate` directory contains a normal project.amio file and a `template.json`
manifest: id, name, description, aspect, category, optional thumbnail filename and
placeholders (clip id plus optional label). The only accepted thumbnail filename is
`thumbnail.png`. This keeps project schema versioning independent of templates.
The shared control library loads and saves these directories (save refuses to overwrite).
Saving currently supports templates without thumbnails. All placeholder ids must exist
and be unique. Text fills return one undoable batch, preserving timing, transforms,
effects and animation. Media replacement, user library discovery, rendered thumbnails,
launcher cards, File menu and MCP tools are deferred.

## `reframe`

Offline subject-following crop for video clips, with no model downloads. Changes
the canvas to 9:16 (1080×1920), 1:1 (1080×1080) or 4:5 (1080×1350), replaces video
position/scale animation, and saves a new `.amio`. The input is preserved unless
explicitly chosen as the output with `--overwrite`. Repeat `--clip` to scope to
video ids; omitted selects all videos. Higher `--smoothness` follows more slowly.
Other layers keep their normalized transforms. Rotated videos and locked tracks
are rejected. See [tracking limitations](mcp.md#auto-reframe).

```sh
animathio-cli reframe landscape.amio -o vertical.amio --aspect 9:16 --smoothness 0.6
animathio-cli reframe landscape.amio -o square.amio --aspect 1:1 --clip 2
```

<!-- help:reframe -->
```text
Reframe video to portrait or square with offline subject tracking

Usage: animathio-cli reframe [OPTIONS] --output <OUTPUT> <PROJECT>

Arguments:
  <PROJECT>  Input .amio or project.json

Options:
  -o, --output <OUTPUT>          Output .amio archive (input is preserved)
      --aspect <ASPECT>          Target aspect: 9:16, 1:1 or 4:5 [default: 9:16] [possible values: 9:16, 1:1, 4:5]
      --smoothness <SMOOTHNESS>  Tracking smoothness, 0..=1; higher follows more slowly [default: 0.5]
      --clip <CLIPS>             Reframe only these video clip ids; repeat for multiple clips
      --overwrite                Replace an existing output archive
  -h, --help                     Print help
```
<!-- /help:reframe -->

## `duck`

Lower music under speech. Exactly one of `--dry-run` or `--apply` is required. The voice
is given with `--voice-clip` and/or `--voice-track` (repeatable) and the music with one or
more `--music-track` or `--music-clip` targets. Dry runs print the JSON proposal (voice regions, ducked spans and the
per-clip gain keyframes in ticks) and leave the input untouched; `--apply` saves it
atomically, or to `--output` (`--overwrite` replaces an existing archive). Clips that
already carry gain keyframes are an error unless `--replace` is given. See
[auto-ducking](mcp.md#auto-ducking) for how regions and ramps are built.

```bash
animathio-cli duck talk.amio --voice-track 1 --music-track 2 --dry-run
animathio-cli duck talk.amio --voice-track 1 --music-track 2 --depth-db 15 --apply -o ducked.amio
```

<!-- help:duck -->
```text
Lower music under speech with gain keyframes, as one undoable edit

Usage: animathio-cli duck [OPTIONS] <PROJECT>

Arguments:
  <PROJECT>  Input project archive or project.json

Options:
      --voice-clip <VOICE_CLIPS>       Voice clip id; may be repeated (a video clip resolves its linked audio)
      --voice-track <VOICE_TRACKS>     Voice track id; may be repeated
      --music-track <MUSIC_TRACKS>     Music track id to duck; may be repeated
      --music-clip <MUSIC_CLIPS>       Individual music audio clip id to duck; may be repeated
      --depth-db <DEPTH_DB>            How far the music drops under speech, in dB [default: 12]
      --attack <ATTACK>                Ramp down, finishing when the voice starts (seconds or Nticks) [default: 0.3]
      --release <RELEASE>              Ramp up, starting when the voice ends (seconds or Nticks) [default: 0.5]
      --min-gap <MIN_GAP>              Bridge voice gaps shorter than this (seconds or Nticks) [default: 0.4]
      --threshold-db <THRESHOLD_DB>    Voice is absent below this dBFS level [default: -40]
      --hysteresis-db <HYSTERESIS_DB>  Voice returns this many dB above the threshold [default: 3]
      --from <FROM>                    Inclusive voice analysis start in seconds or Nticks (requires --to)
      --to <TO>                        Exclusive voice analysis end in seconds or Nticks (requires --from)
      --replace                        Overwrite existing gain keyframes on the music clips
      --apply                          Apply the duck, saving atomically to the input unless --output is given
      --dry-run                        Print the review proposal as JSON without saving changes
  -o, --output <OUTPUT>                Save an edited archive here instead of changing the input
      --overwrite                      Replace an existing output archive
  -h, --help                           Print help
```
<!-- /help:duck -->



## `media-fit`

Set one image/video clip's fit and save an edited archive. New clips default to
contain; existing files retain their migrated fitting. `none` means 1:1 source
pixels at project resolution, independent of preview size.

```sh
animathio-cli media-fit project.amio --clip 3 --mode fill -o filled.amio
```

<!-- help:media-fit -->
```text
Set one image/video clip's fit and save an edited project archive

Usage: animathio-cli media-fit [OPTIONS] --clip <CLIP> --mode <MODE> --output <OUTPUT> <PROJECT>

Arguments:
  <PROJECT>  

Options:
      --clip <CLIP>      Clip id from the timeline
      --mode <MODE>      fit/contain, fill/cover, stretch, or none/original (1:1 project pixels)
  -o, --output <OUTPUT>  
      --overwrite        Replace an existing output file
  -h, --help             Print help
```
<!-- /help:media-fit -->


## `set-clip-speed`

Retimes a clip and linked companions while keeping their source ranges. Speed is
an exact decimal or rational multiplier, 1/16–16x at millionth precision. Optional
linear ramp keys use local timeline ticks. Audio follows the clip pitch setting
(on for new clips, off for migrated projects). Pass `--maintain-pitch true` or
`--maintain-pitch false` to change it with the speed edit in one undo step.
Without ripple, overlapping results fail; ripple shifts unlocked trailing clips.

```sh
animathio-cli set-clip-speed input.amio --clip 1 --speed 2 --reverse --ripple -o retimed.amio
animathio-cli set-clip-speed input.amio --clip 1 --speed 1 --ramps '[{"time":0,"speed":{"numerator":1,"denominator":2}},{"time":705600000,"speed":{"numerator":2,"denominator":1}}]' -o ramp.amio
```

<!-- help:set-clip-speed -->
```text
Change a clip's speed/reverse/ramp, preserving its source interval and linked audio

Usage: animathio-cli set-clip-speed [OPTIONS] --clip <CLIP> --speed <SPEED> --output <OUTPUT> <PROJECT>

Arguments:
  <PROJECT>  Input .amio project archive

Options:
      --clip <CLIP>      Clip identity
      --speed <SPEED>    Exact multiplier: 0.5, 2 or 3/2 (1/16..16, millionth precision)
      --reverse          Play the source interval backwards
      --ramps <RAMPS>    JSON array of {"time": ticks, "speed": {"numerator": n, "denominator": d}}
      --ripple           Shift trailing clips on all unlocked tracks. Otherwise reject overlaps
  -o, --output <OUTPUT>  Destination .amio archive
      --overwrite        Replace an existing output archive
  -h, --help             Print help
```
<!-- /help:set-clip-speed -->

## `freeze-frame`

Inserts a frame hold at an absolute timeline position inside a clip; linked audio
is silent during the hold. Splits and ripples unlocked trailing clips as one edit.

```sh
animathio-cli freeze-frame input.amio --clip 1 --at 0.5 --frames 30 -o held.amio
```

<!-- help:freeze-frame -->
```text
Insert a frame hold at the playhead and ripple trailing clips

Usage: animathio-cli freeze-frame [OPTIONS] --clip <CLIP> --at <AT> --frames <FRAMES> --output <OUTPUT> <PROJECT>

Arguments:
  <PROJECT>  Input .amio project archive

Options:
      --clip <CLIP>      Clip identity
      --at <AT>          Timeline playhead, decimal seconds or Nticks
      --frames <FRAMES>  Positive hold duration in project frames; inserts and ripples the tail
  -o, --output <OUTPUT>  Destination .amio archive
      --overwrite        Replace an existing output archive
  -h, --help             Print help
```
<!-- /help:freeze-frame -->

## `math-objects`

The Math panel's typed object catalogue is also available headlessly. List prints
all default definitions; add accepts field overrides in math units and puts the
object on a free visual layer. The size field for text and LaTeX is a percentage
of the short canvas edge. The same plugins validate and render the result.

<!-- help:math-objects -->
```text
List or add typed objects from the editor Math panel

Usage: animathio-cli math-objects <COMMAND>

Commands:
  list  Print default definitions for every supported object kind
  add   Add an object to a free layer and save a new archive
  help  Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```
<!-- /help:math-objects -->

```sh
animathio-cli math-objects list
animathio-cli math-objects add project.amio --kind vector --field x1=-2 --field y1=-1 --field x2=2 --field y2=1 -o vector.amio
animathio-cli math-objects add project.amio --kind latex --field 'source=e^{i\pi}+1=0' --write-on -o formula.amio
animathio-cli math-objects add project.amio --kind polygon --field 'vertices=-2,-1; 2,-1; 0,2' -o polygon.amio
```

Kinds: `latex`, `text`, `point`, `line`, `vector`, `polygon`, `plot`, `circle`,
`ellipse`, `cartesian`, `polar`. Omitted fields retain the defaults printed by list.
Write on uses the animation plugin's typewriter entrance preset and is available
for LaTeX. Parametric curves are deferred; plots currently use the graph plugin's
function-of-X expression language.

## `learn-noise`

`animathio-cli learn-noise input.amio --clip 2 --effect 88 --from 0 --to 0.5 -o learned.amio`
learns a persistent profile from noise only. The audio clip must already have that denoise
instance. The output must be a new path. The interval is in timeline time, lies inside the
clip, and is limited to 30 seconds.

<!-- help:learn-noise -->
```text
Learn a denoise profile from a noise-only timeline interval

Usage: animathio-cli learn-noise --clip <CLIP> --effect <EFFECT> --from <FROM> --to <TO> --output <OUTPUT> <PROJECT>

Arguments:
  <PROJECT>  Input archive or project JSON

Options:
      --clip <CLIP>      Audio clip id
      --effect <EFFECT>  Existing denoise instance id
      --from <FROM>      Inclusive timeline start (seconds or Nticks)
      --to <TO>          Exclusive timeline end (seconds or Nticks)
  -o, --output <OUTPUT>  Output archive (must not exist)
  -h, --help             Print help
```
<!-- /help:learn-noise -->

## `scenes`

Detect hard cuts, split linked video/audio, or add markers. Select `--clip` or `--track`. `--sensitivity` defaults to 0.5 and `--min-shot-frames` to 6. `--delete-shorter-than-frames N` removes resulting short shots and linked companions, leaving gaps. Detection prints JSON; mutations require `--output` and protect existing files unless `--overwrite` is passed.

```sh
animathio-cli scenes project.amio detect --clip 1
animathio-cli scenes project.amio split --track 1 -o split.amio
animathio-cli scenes project.amio markers --clip 1 -o marked.amio
```

<!-- help:scenes -->
```text
Detect hard scene changes, split clips or add markers

Usage: animathio-cli scenes [OPTIONS] <PROJECT> <MODE>

Arguments:
  <PROJECT>  Input project archive or JSON
  <MODE>     Detect only, split linked clips, or add markers [possible values: detect, split, markers]

Options:
      --clip <CLIP>
          Analyse this video clip
      --track <TRACK>
          Analyse all video on this track
      --sensitivity <SENSITIVITY>
          Sensitivity in 0..=1; higher detects smaller changes [default: 0.5]
      --min-shot-frames <MIN_SHOT_FRAMES>
          Minimum shot length in project frames [default: 6]
      --delete-shorter-than-frames <DELETE_SHORTER_THAN_FRAMES>
          Remove resulting short shots and linked companions, leaving gaps [default: 0]
  -o, --output <OUTPUT>
          Output archive, required for split and markers
      --overwrite
          Replace an existing output
  -h, --help
          Print help
```
<!-- /help:scenes -->

PNG outputs from render and render-seq also refuse to overwrite the input project,
including aliases through symlinks and hard links.
