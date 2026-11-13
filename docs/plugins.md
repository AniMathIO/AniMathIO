# Plugin authoring guide

AniMathIO 2.0 plugins are sandboxed **WebAssembly components** (wasmtime,
[Component Model](https://component-model.bytecodealliance.org/), WIT). Each plugin
is compiled to `wasm32-wasip2` and packaged as an `.amplugin` — a zip containing
`plugin.toml` (metadata) and `plugin.wasm` (the component).

A plugin is a pure function of `(params, time)` returning vector scenes — or, for an
*effect* plugin, a pure function of `(pixels, params, time)` returning pixels. It never
touches files, sockets, the GPU or a font engine directly. Everything it needs is
provided by **host imports**.

## The WIT world

The contract is `crates/plugin-api/wit/plugin.wit`:

```wit
package animathio:plugin@0.2.1;

interface types { /* shared vector + schema types, see below */ }

interface host {
    use types.{path, math-mode, typeset-result};
    shape-text: func(text: string, font-size: f32) -> list<path>;
    typeset-math: func(source: string, mode: math-mode, font-size: f32) -> typeset-result;
    svg-to-paths: func(svg-xml: string) -> list<path>;
    log: func(message: string);
}

world plugin {
    import host;
    use types.{manifest, scene, param-map, size};

    export describe: func() -> manifest;
    export render-clip: func(params: param-map, time-ticks: s64, size: size) -> scene;
    export apply-effect: func(input: scene, params: param-map, time-ticks: s64, size: size) -> scene;
    export evaluate-curve: func(time-ticks: s64, duration-ticks: s64, params: param-map) -> f64;
}

// Effect plugins implement this world instead (see "Effect plugins" below).
world effect-plugin {
    import host;
    use types.{manifest};
    export describe: func() -> manifest;
    export frame-effects;
}
```

A clip-source or animation component implements the four exports of `world plugin`. The first three are selected by the
plugin's declared `kinds` (`clip-source` uses `render-clip`, `effect` uses
`apply-effect`, `animation` uses `evaluate-curve`), but the world requires all four
— the unused ones can be stubs (return the input scene / `0.0`). `apply-effect`
(vector-scene effects) is reserved and not called by the host; pixel effects use the
separate `effect-plugin` world below.

### API versions

The WIT package is `animathio:plugin@0.2.1`. `0.2.1` is purely additive over `0.2.0`
(it adds the `frame-effects` interface and the `effect-plugin` world), and the component
model matches imports by semver-compatible version, so components built against
`0.2.0` still link and run unchanged — the repository keeps a prebuilt `0.2.0` hello
component as a test fixture. `plugin.toml` accepts any `0.2.x` `api_version`; effect
plugins must declare `0.2.1` or later.

### Host imports

| Import | Signature | Purpose |
| --- | --- | --- |
| `shape-text` | `(text, font_size) -> list<path>` | Text shaping via the host font engine; returns glyph outline paths. Keeps plugins small (no bundled fonts). |
| `typeset-math` | `(source, mode, font_size) -> typeset-result` | Typeset LaTeX or Typst math via the host (Typst + MiTeX); returns outline paths plus a tight bbox. |
| `svg-to-paths` | `(svg_xml) -> list<path>` | Parse an SVG XML string into vector paths. |
| `log` | `(message)` | Host logging for diagnostics. |

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

## Param schema types

`describe()` returns a `manifest` that declares the plugin's parameters. The host
uses it to build the inspector UI and interpret preserved legacy JSON values.
The core validates that each keyframe track has one consistent value type.

```wit
record manifest {
    name: string,
    version: string,
    description: string,
    kinds: list<plugin-kind>,      // clip-source, effect, animation
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
api_version = "0.2.0"          # any "0.2.x"; effect plugins need "0.2.1"
id = "com.example.hello"       # reverse-domain; [a-zA-Z0-9._-]
name = "Hello"
version = "0.1.0"
description = "Minimal example plugin: a filled circle"
kinds = ["clip-source"]        # "clip-source" | "effect" | "animation"; an effect plugin declares only ["effect"]
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
    use types.{size, param-map, param-spec};
    record effect-descriptor { id: string, name: string, description: string, params: list<param-spec> }
    list-effects: func() -> list<effect-descriptor>;
    apply-frame: func(effect: string, pixels: list<u8>, size: size,
                      params: param-map, time-ticks: s64) -> result<list<u8>, string>;
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
`wasm32-wasip2`, ~112 KB `.wasm`, packages as a valid `.amplugin`).

**`plugins/examples/hello/Cargo.toml`**

```toml
[package]
name = "hello"
version = "0.1.0"
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
api_version = "0.2.0"
id = "com.example.hello"
name = "Hello"
version = "0.1.0"
description = "Minimal example plugin: a filled circle"
kinds = ["clip-source"]
capabilities = []
```

**`plugins/examples/hello/src/lib.rs`**

```rust
//! Minimal `hello` example plugin: a filled circle with a keyframeable radius
//! and colour, plus a host log line.

wit_bindgen::generate!({
    path: "../../../crates/plugin-api/wit",
    world: "plugin",
});

use animathio::plugin::host;
use animathio::plugin::types::*;

struct HelloPlugin;

fn get_number(params: &[ParamEntry], id: &str, default: f64) -> f64 {
    params.iter().find(|e| e.id == id)
        .and_then(|e| match &e.value { ParamValue::Number(n) => Some(*n), _ => None })
        .unwrap_or(default)
}

fn get_color(params: &[ParamEntry], id: &str, default: Color) -> Color {
    params.iter().find(|e| e.id == id)
        .and_then(|e| match &e.value { ParamValue::Color(c) => Some(*c), _ => None })
        .unwrap_or(default)
}

/// A circle as four cubic Béziers (the standard kappa approximation).
fn circle(cx: f32, cy: f32, r: f32) -> Vec<PathVerb> {
    let k = 0.552_284_8 * r;
    vec![
        PathVerb::MoveTo(Point { x: cx + r, y: cy }),
        PathVerb::CubicTo((
            Point { x: cx + r, y: cy + k },
            Point { x: cx + k, y: cy + r },
            Point { x: cx, y: cy + r },
        )),
        PathVerb::CubicTo((
            Point { x: cx - k, y: cy + r },
            Point { x: cx - r, y: cy + k },
            Point { x: cx - r, y: cy },
        )),
        PathVerb::CubicTo((
            Point { x: cx - r, y: cy - k },
            Point { x: cx - k, y: cy - r },
            Point { x: cx, y: cy - r },
        )),
        PathVerb::CubicTo((
            Point { x: cx + k, y: cy - r },
            Point { x: cx + r, y: cy - k },
            Point { x: cx + r, y: cy },
        )),
        PathVerb::Close,
    ]
}

impl Guest for HelloPlugin {
    fn describe() -> Manifest {
        Manifest {
            name: "hello".to_string(),
            version: "0.1.0".to_string(),
            description: "Minimal example plugin: a filled circle".to_string(),
            kinds: vec![PluginKind::ClipSource],
            params: vec![
                ParamSpec {
                    id: "radius".to_string(),
                    label: "Radius".to_string(),
                    kind: ParamKind::Number(NumberRange {
                        min: 1.0, max: 512.0, default: 96.0, step: 1.0,
                    }),
                },
                ParamSpec {
                    id: "color".to_string(),
                    label: "Color".to_string(),
                    kind: ParamKind::Color(ColorParam {
                        default: Color { r: 90, g: 170, b: 255, a: 255 },
                    }),
                },
            ],
        }
    }

    fn render_clip(params: Vec<ParamEntry>, _time_ticks: i64, size: Size) -> Scene {
        let radius = get_number(&params, "radius", 96.0) as f32;
        let color = get_color(&params, "color",
            Color { r: 90, g: 170, b: 255, a: 255 });
        host::log(&format!("hello: circle radius={radius} at {}x{}",
            size.width, size.height));

        Scene {
            coordinate_space: CoordinateSpace::Canvas,
            elements: vec![DrawCommand::FillPath((
                circle(size.width as f32 / 2.0, size.height as f32 / 2.0, radius),
                color,
            ))],
        }
    }

    fn apply_effect(input: Scene, _params: Vec<ParamEntry>, _t: i64, _size: Size) -> Scene {
        input
    }

    fn evaluate_curve(_t: i64, _duration: i64, _params: Vec<ParamEntry>) -> f64 {
        0.0
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
`right`) and `units` (`pixels` or `normalized`). Pixel mode retains centered outline
layout. Normalized mode produces a generic text run with embedded-font shaping,
wrapping and line alignment; font size is a fraction of canvas height. Migrated
builtin text uses normalized units and left alignment.

`org.animathio.shapes` accepts `shape` (`rect`, `ellipse`, `polygon`, `star`, `arrow`,
`line`, `path`), `units`, position/dimensions, `fill`, `stroke` and `stroke_width`.
Alpha-zero colors disable fill or stroke; zero width disables the outline. In
normalized mode rectangle/ellipse defaults fill the unit square and stroke widths
scale with it. Normalized shapes remain static unless their parameters are keyframed.
`path` is a JSON string of local verbs, for example
`[{"MoveTo":[0,0]},{"QuadTo":[[0.5,1],[1,0]]},"Close"]`.
`CurveTo` carries three points (two controls and endpoint).

The component ABI is now `animathio:plugin@0.2.1`: `0.2.0` (explicit scene units
and text alignment) plus the additive `effect-plugin` world. Rebuild components against
the current WIT and set `api_version` to `0.2.0` or `0.2.1` in `plugin.toml`; old 0.1
packages are rejected.
`plugins/examples/hello` demonstrates a canvas-coordinate scene. Install the WASI
target and use `scripts/build-plugins.sh` to package all first-party components.
