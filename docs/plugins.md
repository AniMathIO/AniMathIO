# Plugin authoring guide

AniMathIO 2.0 plugins are sandboxed **WebAssembly components** (wasmtime,
[Component Model](https://component-model.bytecodealliance.org/), WIT). Each plugin
is compiled to `wasm32-wasip2` and packaged as an `.amplugin` — a zip containing
`plugin.toml` (metadata) and `plugin.wasm` (the component).

A plugin is a pure function of `(params, time)` returning vector scenes — or, for an
*effect* plugin, a pure function of `(pixels, params, time)` returning pixels, or for an
*audio-effect* plugin, a pure function of `(PCM block, params, state)` returning PCM. An
*animation-preset* plugin turns a preset choice into keyframe curves once, when the preset
is applied, and an *importer* plugin turns source bytes into a command document the host
validates and replays. A plugin never
touches files, sockets, the GPU or a font engine directly. Everything it needs is
provided by **host imports**.

## The WIT world

The contract is `crates/plugin-api/wit/plugin.wit`:

```wit
package animathio:plugin@1.1.0;

interface types { /* shared vector + schema types, see below */ }

interface host {
    use types.{path, math-mode, typeset-result, diagnostic};
    shape-text: func(text: string, font-size: f32) -> result<list<path>, diagnostic>;
    typeset-math: func(source: string, mode: math-mode, font-size: f32) -> result<typeset-result, diagnostic>;
    svg-to-paths: func(svg-xml: string) -> result<list<path>, diagnostic>;
    limits: func() -> list<tuple<string, u64>>;
    log: func(message: string);
}

world source-plugin {
    import host;
    use types.{manifest, scene, param-map, size, diagnostic};

    export describe: func() -> manifest;
    export render-clip: func(params: param-map, time-ticks: s64, size: size) -> result<scene, diagnostic>;
}

// Effect plugins implement this world instead (see "Effect plugins" below).
world effect-plugin {
    import host;
    use types.{manifest};
    export describe: func() -> manifest;
    export frame-effects;
}

// Audio-effect plugins implement this world (see "Audio-effect plugins" below).
world audio-effect-plugin {
    import host;
    use types.{manifest};
    export describe: func() -> manifest;
    export audio-effects;
}

// API 1.1: animation presets (see "Animation-preset plugins" below).
world animation-preset-plugin {
    import host;
    use types.{manifest};
    export describe: func() -> manifest;
    export animation-presets;
}

// API 1.1: importers, which also import bounded sibling-file reads
// (see "Importer plugins" below).
world importer-plugin {
    import host;
    import import-files;
    use types.{manifest};
    export describe: func() -> manifest;
    export importer;
}
```

A clip-source component implements the two exports of `world source-plugin`.
Effects implement their separate worlds; no unused stubs are required in 1.0.
The legacy adapter retains the old source world's reserved vector-effect and
curve hooks, without making them part of the frozen 1.0 contract.

### API versions and compatibility policy

The current contract is `animathio:plugin@1.1.0`: the frozen 1.0 contract plus two
new worlds (`animation-preset-plugin`, `importer-plugin`) and one new interface that only
the importer world imports (`import-files`). 1.1 changes no existing record, enum,
variant, function or world, so every 1.0 component loads and runs unchanged (the
checked-in frozen `hello-1.0.0` fixture and the 0.2.x fixtures are tested against this
host). ABI semver is independent of the
application's 2.0 version and each plugin's own version. The Rust SDK crate keeps
its workspace version (2.0.0); `API_VERSION` identifies the component ABI. Its
host-side kind and error enums are non-exhaustive so SDK callers handle future
host classifications/errors with a fallback. Package `api_version` is
an exact stable semantic version naming the **minimum host ABI required**, not a
Cargo-style range. This host supports `1.0.0` and `1.1.0` and explicitly adapts legacy
`0.2.0`, `0.2.1`, `0.2.2`. It rejects malformed versions, prereleases, build
metadata, unknown majors and requirements newer than its implemented ABI before
compiling the component. Merely changing `plugin.toml` cannot upgrade a component.
A 1.x component imports and exports interfaces at the minor its WIT came from; the
host's linker satisfies `@1.0.0` imports with its `@1.1.0` definitions (Wasmtime
semver matching) and looks up exports at every implemented 1.x minor, newest first.
The `animation-preset` and `importer` kinds require `api_version = "1.1.0"` and, like
`effect`/`audio-effect`, must be a package's only kind.
The host checks the selected world's canonical types and versioned exports;
for 1.x packages, known execution kinds, the plugin version and the parameter
schema returned by `describe` must also agree with the package and be well formed;
a mismatch fails loading before registry insertion. Legacy 0.2.x packages are not
retroactively held to these `describe` checks, since they were accepted without
them, only to the typed export checks.

- A **patch** may fix implementation bugs or clarify documentation without changing
  WIT layouts, required imports/exports, units or valid-input semantics.
- A **minor** may add a separate interface/world or an optional advisory metadata
  key. New hosts retain every old world and import namespace. New mandatory host
  imports must live in a new world; a plugin using them raises its minimum ABI.
  A minor cannot add fields (even optional fields) to an existing record, add cases
  to an existing enum/variant/flags, change function signatures, or require old
  plugins to export new functions. Optional WIT fields still change record layout.
- A **major** is required for those structural or semantic breaks. It gets a new
  WIT package version and an explicit adapter; no binary version rewriting.
  A host introducing a new major must retain the previous major for at least one
  full subsequent application minor release and announce removal one minor ahead.
- Deprecated items retain their types and behavior for the entire 1.x line.
  The 1.0 source world deliberately omits the old unused vector-effect and curve
  hooks. Their signatures are checked only for legacy components. A future curve
  execution interface must be a separate world with defined semantics.
- Legacy 0.2.x support is guaranteed through **AniMathIO 2.1 inclusive**. Removal
  can occur no earlier than 2.2, with release-note notice in 2.1 and a migration
  guide. The immutable component fixtures are tested without rebuilding them. A frozen
  1.0 hello fixture also guards this contract against future accidental breaks.

WIT `plugin-kind` is now a **string**, with execution IDs `clip-source`,
`effect`, `audio-effect` and, since 1.1, `animation-preset` and `importer`. The old
`animation` package kind is accepted only for 0.2.x; it never had an execution world
(1.1 adds `animation-preset`, which generates ordinary keyframes instead). Adding a string value
does not change canonical types.
Unknown strings returned by `describe` are advisory classifications and grant no
execution capability. Unknown execution kinds in `plugin.toml` are rejected with
a diagnostic; the host cannot run worlds it does not implement. This avoids
repeating the closed-enum failure found in #211. The 0.2 adapter maps that old
world's `effect` report to `audio-effect` only for an audio-effect package.

The importer world's exchange format is a **versioned command document**
(`{"version": 1, "commands": [...]}`, core `Command`s in their control-protocol JSON).
Version 1 is tied to this host's command serialization; a host that changes it must
accept or reject versions explicitly. The v1 importer is still a native crate behind
the control API (its embedded media can inflate to 2 GiB, beyond a wasm32 importer
sandbox); the Manim importer runs as the `org.animathio.import-manim` plugin. Raster
generators need a separate future world; the source world returns vector scenes only.

### Upgrading a 0.2 component

Existing binaries can keep their original manifests and run through the adapter.
To build a new 1.0 component:

1. Generate against the current WIT. Source plugins select `world: "source-plugin"`
   and remove the unused vector-effect/curve trait methods. Effect/audio world
   names stay the same.
2. Return `Result<Scene, Diagnostic>` from source rendering. Propagate fallible
   host services with `?`. Frame/audio errors now use `Diagnostic` too; choose a
   string code, a fallback message and empty metadata when no details are needed.
3. Return string kind IDs from `describe`, including the real `audio-effect` kind,
   and initialize its `metadata` field (an empty list is fine).
4. Set package `api_version = "1.1.0"` (or `"1.0.0"` for binaries built against the
   frozen 1.0 WIT); keep the plugin's own version consistent
   between package and `describe`. Preserve existing parameter IDs, units and
   enum values when retaining compatibility with saved projects.
5. Rebuild for `wasm32-wasip2` and package the new binary with its matching manifest.

### Host imports

| Import | Signature | Purpose |
| --- | --- | --- |
| `shape-text` | `(text, font_size) -> result<list<path>, diagnostic>` | Text shaping with the host's embedded DejaVu Sans (never a system font, so layout is identical on every machine); returns glyph outline paths. Keeps plugins small (no bundled fonts). |
| `typeset-math` | `(source, mode, font_size) -> result<typeset-result, diagnostic>` | Typeset LaTeX or Typst math via the host (Typst + MiTeX); returns outline paths plus a tight bbox. |
| `svg-to-paths` | `(svg_xml) -> result<list<path>, diagnostic>` | Parse an SVG XML string into vector paths. |
| `log` | `(message)` | Host logging for diagnostics. |
| `limits` | `() -> list<tuple<string, u64>>` | Configured sandbox policy and tick frequency; unique keys, unknown keys ignored. |
| `read-sibling` | `(relative-path) -> result<list<u8>, diagnostic>` | Importer world only (`import-files` interface): bounded read of a file below the directory the caller granted for this import. |

`typeset-result` is `{ paths: list<path>, bbox: bbox }`; `math-mode` is `latex` or
`typst`; `bbox` is `{ min-x, min-y, max-x, max-y }` (f32).

### Vector types (`types`)

The drawing model is a small vector set:

- `point { x: f32, y: f32 }`, `size { width: u32, height: u32 }`
- `color { r, g, b, a: u8 }` (straight-alpha RGBA)
- `path-verb`: `move-to(point)`, `line-to(point)`, `quad-to(tuple<point, point>)`,
  `cubic-to(tuple<point, point, point>)`, `close`
- `path = list<path-verb>` — one contour; the host imports return `list<path>`
  (one entry per contour)
- `stroke-style { width: f32, color, cap, join }` with `stroke-cap` (`butt`,
  `round`, `square`) and `stroke-join` (`miter`, `round`, `bevel`)
- `text-element { align: text-alignment, text, font-size, x, y, color }`
  with `text-alignment` (`left`, `center`, `right`)
- `draw-command`: `fill-path(tuple<path, color>)`,
  `stroke-path(tuple<path, stroke-style>)`, `text-run(text-element)`
- `scene { coordinate-space: coordinate-space, elements: list<draw-command> }`
  with `coordinate-space` (`canvas`, `normalized`)

In generated Rust, `draw-command` variants are spelled
`DrawCommand::FillPath((path, color))`, `DrawCommand::StrokePath((path, style))`
and `DrawCommand::TextRun(element)`.

### Units, purity and errors

Time is signed integer flicks: **705,600,000 ticks per second**. Clip rendering and
frame effects receive clip-local source time, including trims/rate changes; audio
block time belongs to its chain, as specified below. `size` is the requested
render canvas in integer pixels. Coordinates are finite, x right/y down.
`canvas` scenes use pixel units; `normalized` scenes use the clip-local unit square
scaled by canvas width/height and the clip transform. Stroke outlines use local
units and follow that same possibly nonuniform transform. Coordinates outside
0…1 are allowed. Normalized text size is a fraction of canvas height and its x/y
are offsets from the centered text block. Host text/math services always use
pixel font sizes and return pixel paths/bounds; normalize them explicitly if needed.

All rendering, effects and audio processing must be pure in their arguments;
opaque audio state is explicit. WASI clocks/randomness and retained guest memory
must not influence output. Logs are diagnostics only. The error record is
`diagnostic { code: string, message: string, metadata: list<tuple<string, string>> }`.
Codes are open strings: `invalid-input`, `limit-exceeded`, `service-failed`,
`plugin-error`. Unknown codes display the fallback message; message text is not a
machine protocol. Metadata may include translation keys or extra context, and
unknown keys are ignored. Traps remain host failures and discard the instance.

ABI 1.0 clip rendering, frame/audio effects and host text/math/SVG services return
an explicit `result` with that diagnostic. An empty successful scene/path list is
valid. The 0.2 adapter preserves its old empty-result host services and direct
scene return, translating string effect errors to `plugin-error`. The SVG service
extracts double-quoted path `d` attributes (or raw SVG path data); it does not
interpret transforms, styles, other primitives or external resources. Unsupported
nonempty input fails explicitly in 1.0. Host math has a 4 KiB source limit, a
positive font size up to 1,000 px and a 2 s service timeout.

## Param schema types

`describe()` returns a `manifest` that declares the plugin's parameters. The host
uses it to build the inspector UI and interpret preserved legacy JSON values.
The core validates that each keyframe track has one consistent value type.

```wit
record manifest {
    name: string,
    version: string,
    description: string,
    kinds: list<plugin-kind>,      // open string ids
    metadata: list<tuple<string, string>>, // optional advisory entries
    params: list<param-spec>,
}

record param-spec {
    id: string,
    label: string,
    kind: param-kind,
}

variant param-kind {
    %number(number-range),         // { min, max, default, step } (f64)
    %color(color-param),           // { default: color }
    %string(string-param),         // { default: string }
    enum-choice(enum-param),       // { options: list<string>, default: string }
    %bool(bool-param),             // { default: bool }
    %point(point-param),           // { default: point }
    asset-ref(asset-ref-param),    // { media-type: string, default-asset: option<string> }
}
```

`%number`, `%color`, `%string`, `%bool` and `%point` are escaped with `%` because
those names are WIT keywords.

At evaluation the host sends a `param-map` (`list<param-entry>` of
`{ id, value: param-value }`), where `param-value` mirrors `param-kind` with a
concrete value. A plugin reads the entry for each `id`, falling back to its default:

```rust
fn get_number(params: &[ParamEntry], id: &str, default: f64) -> f64 {
    params.iter().find(|e| e.id == id)
        .and_then(|e| match &e.value { ParamValue::Number(n) => Some(*n), _ => None })
        .unwrap_or(default)
}
```

### Schema validation and optional metadata

Parameter IDs are stable, nonempty and unique within a schema. Numeric ranges
are finite and inclusive with `min <= default <= max` and `step > 0`; step/range
are UI hints, not a quantization of animation values. Enum options are unique,
nonempty stable **values**, and the default must name one. Point defaults must be
finite. Colors are straight-alpha sRGB RGBA8; asset strings identify project
assets, not filesystem paths, and `media-type` is a MIME hint. Missing parameter
entries use the plugin's declared default; input entries use unique IDs. Frame and
audio effects currently support numeric parameters only. Numeric effect values
outside the declared range are clamped; generators may choose their own validation.
Changing an existing parameter ID, type, units or enum value is a plugin data
migration concern, independent of the host ABI version.

`manifest.metadata` is an optional-by-content list: supply `[]` when unused. Its
record field is fixed in 1.0; new advisory keys can be added without positional
record growth. Unique key conventions are `label-key`, `description-key`,
`param.<id>.label-key`, `param.<id>.unit`, and
`param.<id>.option.<value>.label-key`; effect metadata uses an
`effect.<id>.` prefix before the parameter key. Units are descriptive (for example
px, normalized, Hz, dB, ms), never implicit conversion instructions. Third-party
keys should use a reverse-domain prefix. The host preserves metadata in the loaded
description; the current inspector displays fallback labels and option values.
A future localization UI can consume translation keys without an ABI change.
In metadata keys, escape ID/option segments outside ASCII letters, digits,
underscores and hyphens as uppercase percent-encoded UTF-8 bytes, including dots,
so arbitrary enum values cannot create ambiguous key paths.

The host bounds what `describe` returns after validating it: at most 8 kinds, 64
metadata entries (keys up to 128 bytes, values up to 256), 32 parameters, 64 enum
options per parameter and 256 bytes for names, labels, options and defaults;
longer text is truncated at a UTF-8 boundary and extra entries are dropped. A
plugin that needs more should split into several plugins.

### Keyframes

Keyframes are **not** part of the WIT world — they live in the core model
(`Clip::animations` with `Target::Param(name)`). By the time the host calls a
plugin, it has already sampled the clip's keyframes: each animated value is passed
in the `param-map` with its WIT parameter type, overriding the clip's static base
parameter. Every parameter kind supports keyframes. Numbers and points support
linear, smoothstep ease-in/out, and hold interpolation. Colours interpolate in
linear-light RGBA and convert to sRGB bytes at the WIT boundary. Booleans, enum
choices, strings, and asset references hold until the next key. Plugins read the
evaluated parameters on every call; they do not need to sample curves themselves.

Every inspector parameter has a keyframe diamond. Editing an animated parameter
creates or updates a key at the playhead. Typed values override preserved opaque
plugin JSON when the host prepares the `param-map`.

Time arrives as `time-ticks: s64` (Rust `i64`) in the same integer tick unit as the
core: **705,600,000 ticks/second**. To convert to seconds in a plugin:

```rust
const TICKS_PER_SECOND: f64 = 705_600_000.0;
let t = time_ticks as f64 / TICKS_PER_SECOND;
```

## `plugin.toml`

The manifest stored at the archive root of an `.amplugin`:

```toml
api_version = "1.1.0"          # minimum host ABI, not a version range
id = "com.example.hello"       # reverse-domain; [a-zA-Z0-9._-]
name = "Hello"
version = "0.1.0"
description = "Minimal example plugin: a filled circle"
kinds = ["clip-source"]        # "clip-source" | "effect" | "audio-effect" | "animation-preset" | "importer"; all but clip-source must be the only kind
capabilities = []              # requested host grants; empty = sandbox only
```

`api_version`, `id`, `name`, `version` and `kinds` are required; `description` and
`capabilities` are optional. The `id` must be unique within a host.

## Effect plugins

An effect plugin is a pixel-filter library. It declares `kinds = ["effect"]`, implements
the `effect-plugin` world and is built like any other plugin
(`wit_bindgen::generate!({ path: ..., world: "effect-plugin" })`). Its exports:

```wit
interface frame-effects {
    use types.{size, param-map, param-spec, diagnostic};
    record effect-descriptor { id: string, name: string, description: string, params: list<param-spec> }
    list-effects: func() -> list<effect-descriptor>;
    apply-frame: func(effect: string, pixels: list<u8>, size: size,
                      params: param-map, time-ticks: s64) -> result<list<u8>, diagnostic>;
}
```

- `list-effects` advertises each filter: a stable `id` (`[a-z0-9_-]`, stored in projects),
  a display name, and its parameters. Only `number` parameters are supported (effect
  parameters are keyframeable numbers); `min`/`max`/`step` are UI hints and `default`
  seeds new instances. The editor's Effects tab and context menu and the MCP
  `list_effects` tool are generated from these descriptors — nothing is hard-coded.
- `apply-frame` receives `size.width * size.height * 4` bytes of row-major,
  straight-alpha sRGB RGBA8, the evaluated numeric parameters (keyframes already
  sampled) and the clip-local source time in ticks. It must return a buffer of exactly
  the same length, or an `Err` diagnostic for invalid parameters. It must be a pure
  function of its arguments: preview and export both rely on identical output.
- Missing parameters should fall back to the declared defaults.

Projects reference an effect by plugin id and effect id (`EffectInstance { plugin,
effect, params }`), so a project saved with `org.animathio.effects-basic/grayscale`
renders anywhere that plugin is installed, and fails with a clear
`effect plugin ... is not loaded` error where it is not.

The host runs every call in a fresh store: the component is compiled once (disk-cached)
and its imports are resolved once, then each call copies the frame in, runs the filter
and copies the result out. The frame is never shared with the guest.

First-party `org.animathio.effects-basic` provides `grayscale`, `sepia`, `invert`,
`saturate`, `brightness`, `contrast` (all `amount`), `blur` (`radius`, a fraction of the
canvas height) and `opacity` (`amount`). Color filters work in linear light with an exact
`powf`-free sRGB table lookup, `invert` on encoded channels, `blur` on premultiplied
linear light with edge replication. Its output is checked bit-for-bit against the former
builtin implementation by `crates/plugin-host/tests/effects.rs` and by the render goldens.

## Audio-effect plugins

An audio-effect plugin is a filter library for sound. It declares
`kinds = ["audio-effect"]` with `api_version = "1.0.0"` or later, implements the
`audio-effect-plugin` world and is built like any other plugin
(`wit_bindgen::generate!({ path: ..., world: "audio-effect-plugin" })`). Its exports:

```wit
interface audio-effects {
    use types.{param-map, param-spec, diagnostic};
    record audio-effect-descriptor { id: string, name: string, description: string, params: list<param-spec> }
    record audio-block { sample-rate: u32, channels: u16, start-ticks: s64, samples: list<f32> }
    record block-output { samples: list<f32>, state: option<list<u8>> }
    list-audio-effects: func() -> list<audio-effect-descriptor>;
    process-block: func(effect: string, block: audio-block, params: param-map,
                        state: option<list<u8>>) -> result<block-output, diagnostic>;
}
```

- `list-audio-effects` advertises each filter exactly like `list-effects` does for pixels:
  stable `id` (`[a-z0-9_-]`), name, description and `number` parameters (`min`/`max`/`step`
  are UI hints, `default` seeds new instances). The editor's Effects tab and the MCP
  `list_audio_effects` tool are generated from these descriptors.
- `process-block` receives one block of interleaved `f32` PCM (finite, unclipped, `1..=32` channels, 1…768,000 Hz, up to
  `max_audio_block_frames` frames) and must return exactly as many samples. `start-ticks`
  is the time of the first frame in the chain's own timeline: clip-local ticks for a clip
  chain, project ticks for a track chain. Parameters arrive already evaluated
  (keyframes sampled at the block start); missing ones should fall back to the declared
  defaults and out-of-range ones should be clamped rather than rejected, so keyframe
  overshoot never stops playback. Return an `Err` diagnostic only for genuinely invalid
  input.
- **State.** The plugin keeps no memory between calls. Filters, envelope followers and
  gates return their memory as the opaque `state` blob, and the host hands it back with
  the next contiguous block (`none` for the first block of a signal). A well-behaved
  plugin is block-size independent: processing a signal in blocks of any size with the
  state carried over is bit-identical to one big block (first-party tests assert this).
  The host caps the blob at `max_audio_state_bytes`. State formats are private to
  the plugin and should carry their own version tag; the host never interprets
  their contents. Empty state is equivalent to no state and is sent back as `none`. Channel order must be preserved (stereo is left/right); this
  interface does not negotiate surround layouts or planar sample buffers.
- The call must be a pure function of its arguments: preview and export both rely on
  identical output.

The host compiles each component once, resolves its imports once and then **pools warm
instances**: a finished call returns its store to the plugin's pool (up to eight idle) and
the next call reuses it after refilling fuel, epoch deadline and the memory limit. A
trapped call discards its instance. Audio effects run in the mix worker or the export
thread — never in the device callback.

Projects reference an audio effect like a pixel effect (`EffectInstance { plugin, effect,
params, bypass }`) on a clip's or a track's `audio_effects` chain; see
[architecture.md](architecture.md#audio-effects) for the signal flow.

First-party `org.animathio.audio-basic` provides (arithmetic in `f64`, block-size
independent, deterministic):

| Effect id | Parameters (default) | Notes |
| --- | --- | --- |
| `gain` | `gain_db` (0, −60…24) | Gain / trim. Stateless. |
| `eq3` | `low_gain_db`/`mid_gain_db`/`high_gain_db` (0, ±18), `low_freq` (120 Hz), `mid_freq` (1 kHz), `mid_q` (1), `high_freq` (6 kHz) | RBJ low shelf, peaking EQ and high shelf (slope 1). All gains 0 is an exact identity. |
| `compressor` | `threshold_db` (−18), `ratio` (4), `attack_ms` (10), `release_ms` (100), `makeup_db` (0) | Feed-forward, channel-linked peak detector, gain reduction smoothed in dB. |
| `gate` | `threshold_db` (−50), `attack_ms` (2), `hold_ms` (50), `release_ms` (150), `range_db` (60) | Starts open; closes to `−range_db` below the threshold after the hold. |
| `highpass` | `frequency` (80 Hz, 10…2000) | 2nd-order Butterworth, 12 dB/octave. |
| `voice_cleanup` | `highpass_hz` (80), `gate_threshold_db` (−55), `deess_freq` (6.5 kHz), `deess_threshold_db` (−30), `deess_amount` (0.5) | Preset: high-pass, gentle 12 dB gate, then a split-band de-esser (Linkwitz-Riley crossover; only the band above `deess_freq` is attenuated). No spectral processing. |
| `denoise` | `strength` (0.7, 0…1), `sensitivity_db` (6, 0…18), `smoothing` (0.6, 0…1) | Streaming spectral gating: 512-point STFT (hop 128, sqrt-Hann), noise profile learned continuously from the quietest frames (per-bin minimum of the smoothed power over the last ~1 s, no noise sample needed). A bin is attenuated by up to `40 dB × strength` while its power stays within `sensitivity_db` of that profile; the gain mask is smoothed across bins and in time (`smoothing` sets how slowly it closes) to avoid musical noise, and is shared by all channels. Latency is 512 frames: the signal is delayed by that much and the mixer's clip window cuts the last 512 frames of a clip. Up to 8 channels (state blob stays under 64 KiB). Silence stays exactly silent; `strength` 0 only delays. Needs about 125 ms of audio before it acts. |

## Animation-preset plugins

An animation-preset plugin (API 1.1) declares `kinds = ["animation-preset"]` with
`api_version = "1.1.0"` and implements the `animation-preset-plugin` world
(`wit_bindgen::generate!({ path: ..., world: "animation-preset-plugin" })`):

```wit
interface animation-presets {
    use types.{param-map, param-spec, diagnostic};
    enum preset-role { entrance, exit, emphasis }
    record preset-descriptor { id: string, name: string, description: string, role: preset-role,
                               applies-to: list<string>, default-duration-ticks: s64,
                               params: list<param-spec> }
    record clip-info { kind: string, duration-ticks: s64, frame-ticks: s64,
                       rest: list<tuple<string, f64>> }
    enum curve-interpolation { hold, linear, ease-in-out }
    record curve-key { time-ticks: s64, value: f64, interpolation: curve-interpolation }
    record curve { target: string, keys: list<curve-key> }
    list-presets: func() -> list<preset-descriptor>;
    generate: func(preset: string, clip: clip-info, window-ticks: s64, params: param-map)
        -> result<list<curve>, diagnostic>;
}
```

- `list-presets` advertises each preset: a stable `id` (`[a-z0-9_-]`), a role, the clip
  kinds it fits (`video`, `image`, `vector`, `plugin:<plugin-id>`; empty means every
  visual clip, never audio), a positive default window and typed options (`number`,
  `enum-choice` or `bool` only). The host validates the whole list strictly at load.
- The host places the window: `entrance` at the clip start, `exit` at its end,
  `emphasis` centred; the window is clamped to the clip, so a very long default (the
  first-party emphasis presets use one hour) means "the whole clip".
- `generate` receives the clip's kind, length, frame length and **rest values**: the
  current value of every animatable target where the animation must end up (the end of
  an entrance window, the start of an exit or emphasis window), already sampled from
  existing keyframes. Targets are `opacity`, `x`, `y`, `scale-x`, `scale-y`,
  `rotation` (radians) and `param:<name>` for numeric clip parameters (for plugin clips
  also the numeric defaults of the clip plugin's schema).
- It returns curves with window-relative key times (`0..=window-ticks`, strictly
  increasing per curve), finite values and core interpolations. Easings that core
  keyframes cannot express are the plugin's job to sample into linear segments. The host
  caps the total key count (`max_preset_keys`) and the curve count (16), clamps opacity to
  `[0, 1]`, rejects unknown targets and duplicate curves, and turns the result into one
  `Command::Batch`: it removes each animated target's existing keys inside the window,
  then adds the generated ones. Nothing about the preset is stored in the project.
- `generate` runs once per application under the default per-call budget; it must be a
  pure function of its arguments.

First-party `org.animathio.animations` provides these presets (ids in parentheses,
options after the colon):

| Role | Presets |
| --- | --- |
| In / Out | fade (fade-in, fade-out: easing); slide (slide-in, slide-out: direction, distance, fade, easing); zoom (zoom-in, zoom-out: scale, fade, easing); pop (pop-in, pop-out: intensity overshoot); spin (spin-in, spin-out: turns, rotation, easing); bounce (bounce-in, bounce-out: from); wipe (wipe-in, wipe-out: direction, easing; an edge-anchored stretch reveal, not a mask); typewriter (typewriter-in, typewriter-out: text clips only, keyframes the text plugin's `reveal`) |
| Emphasis | breathe (v1's breathe: intensity, period), shake, spin, hop and flash (intensity, period; spin: turns, rotation, easing) |

Exits mirror their entrance in time. Easing choices are linear, ease-in, ease-out,
ease-in-out, back, bounce and elastic.

## Importer plugins

An importer plugin (API 1.1) declares `kinds = ["importer"]` with `api_version = "1.1.0"`
and implements the `importer-plugin` world, which additionally imports `import-files`:

```wit
interface importer {
    use types.{param-map, param-spec, diagnostic};
    record importer-descriptor { id: string, name: string, description: string,
                                 extensions: list<string>, params: list<param-spec> }
    record import-source { file-name: string, bytes: list<u8> }
    record imported-asset { id: string, name: string, media-type: string, bytes: list<u8> }
    enum message-severity { info, warning }
    record import-message { severity: message-severity, line: u32, column: u32,
                            message: string, snippet: string }
    record import-output { commands: string, assets: list<imported-asset>,
                           messages: list<import-message> }
    list-importers: func() -> list<importer-descriptor>;
    run-import: func(importer: string, source: import-source, params: param-map)
        -> result<import-output, diagnostic>;
}
```

- `list-importers` names each format with lower-case extensions and options (`string`,
  `number`, `enum-choice` or `bool`). The host fills missing options with their
  defaults and rejects unknown or out-of-range ones before the plugin runs.
- `run-import` gets the source bytes (at most `max_import_source_bytes`) and a file-name
  hint. Files next to the source are reachable only through `read-sibling`, and only
  when the caller granted a directory (the editor, CLI and control API grant the
  script's folder); paths are relative, `/`-separated, without `..`, and symlinks
  resolving outside the directory are refused. Each call has file-size, total-byte and
  file-count budgets.
- `commands` is a UTF-8 JSON document `{"version": 1, "commands": [...]}` of core
  `Command`s (`Project::rebuild_commands` produces one from a project). The host replays
  it on an empty project through the validating `Command::apply`; any failing command,
  an unknown version or field, more than `max_import_commands` commands or more than
  `max_import_output_bytes` bytes rejects the whole import. Every returned asset's `id`
  must be the lower-case hex BLAKE3 of its bytes and be registered by an `AddAsset`, and
  every registered asset needs bytes.
- `messages` are non-fatal notes shown to the user (`line:column: message (snippet)`).

First-party `org.animathio.import-manim` (format id `manim`, extension `py`, option
`scene`) runs the `import-manim` crate, the same code the native tests exercise, so
the plugin path produces the same project as the crate.

## Building an `.amplugin`

Each first-party plugin is its own standalone Cargo workspace (it has an empty
`[workspace]` table, so it is not part of the root workspace). The release script
builds the first-party set for `wasm32-wasip2` and zips each `plugin.toml` +
`plugin.wasm` into `target/plugins/<name>.amplugin`:

```bash
./scripts/build-plugins.sh
```

To build a plugin by hand (this is what the script does per plugin):

```bash
rustup target add wasm32-wasip2
cargo build \
    --manifest-path plugins/<name>/Cargo.toml \
    --target wasm32-wasip2 \
    --target-dir "$PWD/target" \
    --release --locked
# then zip plugins/<name>/plugin.toml and target/wasm32-wasip2/release/<name>.wasm
# as plugin.toml + plugin.wasm into an .amplugin
```

The plugin host loads `.amplugin` files via `PluginHost::load_plugin_file` or a
whole directory via `PluginHost::load_plugins_dir`.

Installed packages bundle these files under the packager's `resources` target
`plugins`, and `animathio_plugin_host::plugin_search_dirs()` finds them relative
to the running executable: `plugins/` beside the binary (Windows and the in-tree
`target/release` layout), `../lib/<binary>/plugins` (deb and AppImage, where
cargo-packager puts resources under `usr/lib/animathio/`) and
`../Resources/plugins` (the macOS app bundle). `ANIMATHIO_PLUGIN_DIR` overrides
all of them.

## Sandbox limits

Plugins run with no ambient authority. Defaults (in `plugin-host`,
`SandboxLimits::default`):

| Limit | Default |
| --- | --- |
| Memory | 64 MiB per store (`max_memory_bytes`) |
| Table elements | 50,000 (`max_table_elements`) |
| Fuel | 50,000,000 units per call (`fuel_per_call`) |
| Epoch deadline | 100 ticks at a 10 ms ticker → ~1 s backstop (`epoch_deadline_ticks`) |

Fuel metering traps runaway loops deterministically and long before the epoch
backstop fires; after a trap the host discards the store and continues. The
`trap` plugin (`plugins/trap`, an infinite-loop fixture) exercises exactly this.

Pixel-effect calls use a frame-scaled budget (`SandboxLimits`):

| Limit | Default |
| --- | --- |
| Frame size | 16 Mpx (`max_effect_pixels`); larger frames are rejected before any plugin code runs |
| Memory | 64 MiB + 48 bytes per pixel, capped at 1 GiB (`effect_memory_bytes`) |
| Fuel | `fuel_per_call` + 4,096 per pixel (`effect_fuel_per_pixel`) |
| Epoch deadline | 1,000 ticks → ~10 s backstop (`effect_epoch_deadline_ticks`) |

A trap, limit violation, wrong-sized output or `Err` result fails the frame with
`render::Error::Effect`, and the caller's pixels are left untouched.

Audio-effect calls use a block-scaled budget (`SandboxLimits`):

| Limit | Default |
| --- | --- |
| Block size | 65,536 frames (`max_audio_block_frames`, ~1.4 s at 48 kHz) and 1–32 channels; larger blocks are rejected before any plugin code runs |
| State blob | 64 KiB (`max_audio_state_bytes`), in and out |
| Memory | 16 MiB + 64 bytes per interleaved sample, capped at 256 MiB (`audio_memory_bytes`) |
| Fuel | `fuel_per_call` + 4,096 per interleaved sample (`audio_fuel_per_sample`) |
| Epoch deadline | 200 ticks → ~2 s backstop (`audio_epoch_deadline_ticks`) |

A trap, limit violation, wrong-sized or non-finite output or `Err` result fails the block;
the effect is skipped: preview passes the audio through that effect unprocessed and shows
a warning toast, while export fails with an error naming the effect
(`audio effect <plugin>/<effect>: ...`) rather than writing unprocessed audio.

Importer calls (API 1.1) use their own budget (`SandboxLimits`):

| Limit | Default |
| --- | --- |
| Source size | 64 MiB (`max_import_source_bytes`); larger sources are rejected before any plugin code runs |
| Memory | 1 GiB (`import_memory_bytes`), committed only as the guest grows |
| Fuel | 100 × `fuel_per_call` + 20,000 per source byte (`import_fuel_per_byte`) |
| Epoch deadline | 3,000 ticks → ~30 s backstop (`import_epoch_deadline_ticks`) |
| Command document | 64 MiB (`max_import_output_bytes`) and 100,000 commands (`max_import_commands`) |
| `read-sibling` | 64 MiB per file (`max_sibling_file_bytes`), 256 MiB in total (`max_sibling_total_bytes`), 256 files (`max_sibling_files`) per import |

Animation-preset `generate` calls use the default per-call budget and may return at most
1,000 keys in total (`max_preset_keys`).

Guests can call `host::limits()` to read configured policy. Keys match the
`SandboxLimits` fields below with underscores replaced by hyphens; memory values
are bytes, fuel is Wasmtime instruction fuel, and epoch deadlines are ticker
counts, with `epoch-interval-ms = 10`. These are policy ceilings and budget
formulas, not remaining resources. The query includes `ticks-per-second`,
`max-audio-channels`, `max-audio-sample-rate`, and the import caps below as
`max-shape-text-chars`, `max-shape-text-verbs`, `max-svg-input-bytes`,
`max-svg-verbs`, `max-typeset-verbs`, `max-typeset-source-bytes`,
`max-typeset-font-size-px`, `typeset-timeout-ms`, `max-log-message-bytes`,
`max-log-entries`, `max-effects-per-plugin` (64: the most frame effects, and
separately the most audio effects, one plugin may list; a longer listing is rejected at load),
and since 1.1 `max-import-source-bytes`, `import-memory-bytes`,
`import-fuel-per-byte`, `import-epoch-deadline-ticks`, `max-import-output-bytes`,
`max-import-commands`, `max-sibling-file-bytes`, `max-sibling-total-bytes`,
`max-sibling-files` and `max-preset-keys`.
`max-effects-per-plugin` was added after the 1.0.0 freeze as a minor-compatible key, so a
host older than that omits it: treat any missing key as "not reported", never as unlimited.
Limits may be lower in a custom host. Unknown keys must be ignored; a missing
optional key does not authorize unbounded work. Fuel, memory and time limits
remain enforced even if the guest never queries them.

Host imports are separately capped:

| Import | Cap |
| --- | --- |
| `shape-text` | 10,000 chars / 200,000 path verbs |
| `svg-to-paths` | 1 MiB input / 100,000 path verbs |
| `typeset-math` | 200,000 path verbs; single call timeout 2 s |
| `log` | 4 KiB per message, 1,000 retained |

Package limits (in `plugin-api`): `plugin.toml` ≤ 1 MiB, `plugin.wasm` ≤ 64 MiB,
≤ 16 archive entries. Plugin output scenes are validated after each call; an
invalid scene becomes `ClipKind::Error` instead of crashing the host. Signing is
not implemented yet — only load packages from trusted sources.

WASI Preview 2 is linked into every store with **no** filesystem preopens and **no**
socket permissions, and `CapabilityGrants` (filesystem directories, network, env
vars) default to none.

## Minimal example: `plugins/examples/hello`

A complete clip-source plugin — a filled circle with a keyframeable radius and a
colour, plus one host log line. This example is built and verified (compiles for
`wasm32-wasip2`, ~108 KiB `.wasm`, packages as a valid `.amplugin`).

**`plugins/examples/hello/Cargo.toml`**

```toml
[package]
name = "hello"
version = "1.0.0"
edition = "2021"
publish = false

[lib]
crate-type = ["cdylib"]

[dependencies]
wit-bindgen = "0.40.0"

[workspace]
```

**`plugins/examples/hello/plugin.toml`**

```toml
api_version = "1.1.0"
id = "com.example.hello"
name = "Hello"
version = "1.0.0"
description = "Minimal example plugin: a filled circle"
kinds = ["clip-source"]
capabilities = []
```

**`plugins/examples/hello/src/lib.rs`**

```rust
//! Minimal `hello` example plugin: a filled circle with a keyframeable radius
//! and colour, plus a host log line. Use it as a starting point for new plugins.

wit_bindgen::generate!({
    path: "../../../crates/plugin-api/wit",
    world: "source-plugin",
});

use animathio::plugin::host;
use animathio::plugin::types::*;

struct HelloPlugin;

fn get_number(params: &[ParamEntry], id: &str, default: f64) -> f64 {
    params
        .iter()
        .find(|e| e.id == id)
        .and_then(|e| match &e.value {
            ParamValue::Number(n) => Some(*n),
            _ => None,
        })
        .unwrap_or(default)
}

fn get_color(params: &[ParamEntry], id: &str, default: Color) -> Color {
    params
        .iter()
        .find(|e| e.id == id)
        .and_then(|e| match &e.value {
            ParamValue::Color(c) => Some(*c),
            _ => None,
        })
        .unwrap_or(default)
}

/// A circle as four cubic Béziers (the standard kappa approximation).
fn circle(cx: f32, cy: f32, r: f32) -> Vec<PathVerb> {
    let k = 0.552_284_8 * r;
    vec![
        PathVerb::MoveTo(Point { x: cx + r, y: cy }),
        PathVerb::CubicTo((
            Point {
                x: cx + r,
                y: cy + k,
            },
            Point {
                x: cx + k,
                y: cy + r,
            },
            Point { x: cx, y: cy + r },
        )),
        PathVerb::CubicTo((
            Point {
                x: cx - k,
                y: cy + r,
            },
            Point {
                x: cx - r,
                y: cy + k,
            },
            Point { x: cx - r, y: cy },
        )),
        PathVerb::CubicTo((
            Point {
                x: cx - r,
                y: cy - k,
            },
            Point {
                x: cx - k,
                y: cy - r,
            },
            Point { x: cx, y: cy - r },
        )),
        PathVerb::CubicTo((
            Point {
                x: cx + k,
                y: cy - r,
            },
            Point {
                x: cx + r,
                y: cy - k,
            },
            Point { x: cx + r, y: cy },
        )),
        PathVerb::Close,
    ]
}

impl Guest for HelloPlugin {
    fn describe() -> Manifest {
        Manifest {
            name: "hello".to_string(),
            version: "1.0.0".to_string(),
            description: "Minimal example plugin: a filled circle".to_string(),
            kinds: vec!["clip-source".into(), "com.example.circle".into()],
            metadata: vec![
                ("param.radius.unit".into(), "px".into()),
                ("param.radius.label-key".into(), "hello.radius".into()),
            ],
            params: vec![
                ParamSpec {
                    id: "radius".to_string(),
                    label: "Radius".to_string(),
                    kind: ParamKind::Number(NumberRange {
                        min: 1.0,
                        max: 512.0,
                        default: 96.0,
                        step: 1.0,
                    }),
                },
                ParamSpec {
                    id: "color".to_string(),
                    label: "Color".to_string(),
                    kind: ParamKind::Color(ColorParam {
                        default: Color {
                            r: 90,
                            g: 170,
                            b: 255,
                            a: 255,
                        },
                    }),
                },
            ],
        }
    }

    fn render_clip(
        params: Vec<ParamEntry>,
        _time_ticks: i64,
        size: Size,
    ) -> Result<Scene, Diagnostic> {
        let radius = get_number(&params, "radius", 96.0) as f32;
        let color = get_color(
            &params,
            "color",
            Color {
                r: 90,
                g: 170,
                b: 255,
                a: 255,
            },
        );
        let ticks_per_second = host::limits()
            .into_iter()
            .find(|(key, _)| key == "ticks-per-second")
            .map(|(_, value)| value)
            .unwrap_or(705_600_000);
        host::log(&format!(
            "hello: circle radius={radius} at {}x{} ({ticks_per_second} ticks/s)",
            size.width, size.height
        ));

        Ok(Scene {
            coordinate_space: CoordinateSpace::Canvas,
            elements: vec![DrawCommand::FillPath((
                circle(size.width as f32 / 2.0, size.height as f32 / 2.0, radius),
                color,
            ))],
        })
    }
}

export!(HelloPlugin);
```

Build and package it:

```bash
rustup target add wasm32-wasip2
cargo build \
    --manifest-path plugins/examples/hello/Cargo.toml \
    --target wasm32-wasip2 --release
# output: plugins/examples/hello/target/wasm32-wasip2/release/hello.wasm
```

Notes for real plugins:

- `wit_bindgen::generate!` resolves the WIT path **relative to the plugin's
  `Cargo.toml`**. `plugins/examples/hello` is one level deeper than the first-party
  plugins, hence `"../../../crates/plugin-api/wit"` instead of `"../../..."`.
- The `trait Guest` and the `Manifest`/`ParamSpec`/`Scene`/`DrawCommand` types come
  from the generated `animathio::plugin` module; `export!(HelloPlugin)` emits the
  component entry points.
- The host-side `animathio-plugin-api` crate also ships a small geometry helper SDK
  (`animathio_plugin_api::guest::{Point, Color, PathVerb, Shapes}`) with `rect`,
  `ellipse`, `circle`, `polygon`, `star`, `line` and `arrow` builders. The
  standalone plugins use the generated WIT types directly and write their own
  path helpers, as above.

## First-party text and shapes

`org.animathio.text` accepts `text`, `font_size`, `color`, `align` (`left`, `center`,
`right`), `units` (`pixels` or `normalized`) and `reveal` (0–1, default 1: the fraction
of characters shown, keyframed by the typewriter presets; the visible prefix is laid
out on its own, so left alignment keeps characters in place while they appear). Pixel mode retains centered outline
layout. Normalized mode produces a generic text run with embedded-font shaping,
wrapping and line alignment; font size is a fraction of canvas height. The laid-out
text block (widest line by line stack) is centred on the clip's transform position like
other clips, `align` only decides how lines sit relative to each other inside the block
(a normalized `text-run`'s `x`/`y` are an extra offset from that centre). Migrated
builtin text uses normalized units and left alignment. Optional `fit_width` and `fit_height` (0 by default) select a fitted title box,
expressed as fractions of the full canvas. In that mode the plugin emits normalized
glyph outlines sized for a clip with the matching width/height scale, centres explicit
lines in the box, and shrinks long text to fit. Title presets use this mode to keep
edited fields and transform handles inside safe areas.
Caption styles (text plugin 1.1.0, ABI unchanged) add optional parameters that are ignored
when `caption_style` is absent or `off`: `caption_style` (`off`, `classic`, `"word-highlight"`,
`karaoke`, `pop`, `"one-word"`, `"bold-outline"`), `words` (space-separated `start:end` pairs in
ticks, one per whitespace-separated word of `text`, relative to the clip's source time),
`accent` and `outline` colours, `outline_width` (a fraction of the font size) and `max_words`
(words per line, 0 fits the box). They only apply in the fitted-title mode (`fit_width` and
`fit_height` set). The active word is the last one whose start time has passed; the scene is a
pure function of the clip's source time, so preview and export match. `classic` renders exactly
like the plain fitted title; when the edited text no longer has as many words as `words`, the
timings are spread over the stored span. The editor and `subtitles.generate` write these
parameters; see `docs/mcp.md` for `subtitles.style`.
The LaTeX plugin also accepts `units`: `pixels` preserves existing projects;
`percent` reads `size` as a percentage of the shorter render-canvas edge (so a formula
looks the same on 16:9 and 9:16) and only shrinks a formula that would leave the frame
(92% of width/height); `normalized` interprets `size` as a fraction of the shorter edge
and fits formula outlines within 84% width and 22% height (title presets). Formulas added
from the editor library start as `percent`, size 14, `e^{i\pi} + 1 = 0`.

`org.animathio.shapes` accepts `shape` (`rect`, `ellipse`, `polygon`, `star`, `arrow`,
`line`, `path`), `units`, position/dimensions, `fill`, `stroke` and `stroke_width`.
Alpha-zero colors disable fill or stroke; zero width disables the outline. In
normalized mode rectangle/ellipse defaults fill the unit square and stroke widths
scale with it. Normalized shapes remain static unless their parameters are keyframed.
`path` is a JSON string of local verbs, for example
`[{"MoveTo":[0,0]},{"QuadTo":[[0.5,1],[1,0]]},"Close"]`.
`CurveTo` carries three points (two controls and endpoint).

All first-party components and the example are built against ABI 1.1.0 and declare
`api_version = "1.1.0"`; their plugin versions are 1.0.0 except `org.animathio.text`
1.1.0 (the `reveal` parameter). The frozen 1.0 fixture and the explicit 0.2.x adapter
retain older binaries; 0.1 packages are rejected.
`plugins/examples/hello` demonstrates a canvas-coordinate scene. Install the WASI
target and use `scripts/build-plugins.sh` to package all first-party components.
