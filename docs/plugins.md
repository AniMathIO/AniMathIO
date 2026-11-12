# Plugin authoring guide

AniMathIO 2.0 plugins are sandboxed **WebAssembly components** (wasmtime,
[Component Model](https://component-model.bytecodealliance.org/), WIT). Each plugin
is compiled to `wasm32-wasip2` and packaged as an `.amplugin` — a zip containing
`plugin.toml` (metadata) and `plugin.wasm` (the component).

A plugin is a pure function of `(params, time)` returning vector scenes; it never
touches files, sockets, the GPU or a font engine directly. Everything it needs is
provided by **host imports**.

## The WIT world

The contract is `crates/plugin-api/wit/plugin.wit`:

```wit
package animathio:plugin@0.1.0;

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
```

A component implements the four exports. The first three are selected by the
plugin's declared `kinds` (`clip-source` uses `render-clip`, `effect` uses
`apply-effect`, `animation` uses `evaluate-curve`), but the world requires all four
— the unused ones can be stubs (return the input scene / `0.0`).

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
- `text-element { text, font-size, x, y, color }`
- `draw-command`: `fill-path(tuple<path, color>)`,
  `stroke-path(tuple<path, stroke-style>)`, `text-run(text-element)`
- `scene { elements: list<draw-command> }`

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
api_version = "0.1.0"          # must start with "0.1"
id = "com.example.hello"       # reverse-domain; [a-zA-Z0-9._-]
name = "Hello"
version = "0.1.0"
description = "Minimal example plugin: a filled circle"
kinds = ["clip-source"]        # "clip-source" | "effect" | "animation"
capabilities = []              # requested host grants; empty = sandbox only
```

`api_version`, `id`, `name`, `version` and `kinds` are required; `description` and
`capabilities` are optional. The `id` must be unique within a host.

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
api_version = "0.1.0"
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
