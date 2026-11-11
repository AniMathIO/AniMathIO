# AniMathIO 2.0 (in development)

This is the `v2` branch: a from-scratch **Rust** rewrite of AniMathIO. It aims to be a small, reliable video editor core where everything else is a plugin: text, LaTeX, graphs, Manim import, effects, MCP and more.

- **Plan and roadmap:** [#156: AniMathIO 2.0](https://github.com/AniMathIO/AniMathIO/issues/156)
- **Status:** headless engine prototype: project model, `.amio` format, CPU renderer and PNG CLI. The desktop app and media decoder are still under development.
- **Looking for the current app?** AniMathIO 1.x lives on [`main`](https://github.com/AniMathIO/AniMathIO/tree/main), and releases are on the [releases page](https://github.com/AniMathIO/AniMathIO/releases).

## Headless rendering

Build with stable Rust (edition 2024). On shared Linux machines, keep the resource
limits shown below; on other platforms run the same Cargo commands directly.

```bash
CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=line-tables-only nice -n 19 ionice -c3 cargo build -p animathio-cli --locked
./target/debug/animathio-cli new-sample -o sample.amio
./target/debug/animathio-cli validate sample.amio
./target/debug/animathio-cli render sample.amio --frame 30 -o frame.png
./target/debug/animathio-cli render sample.amio --time 1.25 --width 1280 --height 720 -o timed.png
./target/debug/animathio-cli render-seq sample.amio --from 0 --to 90 -o frames/
```

`--frame`, `--from` and `--to` use zero-based project frame indices; sequence end
is exclusive. `--time` accepts exact decimal seconds or integer ticks with a
`ticks` suffix (705,600,000 ticks/second). Output defaults to 640×360; dimensions
are command options until project canvas settings exist. Rendering uses a
transparent background unless a clip covers it. The sample lasts three seconds.

Inputs may be `.amio` archives or `project.json` with content-addressed media in
an adjacent `assets/<blake3>` directory. `validate` checks project invariants and
asset hashes; it does not attempt video decoding.

The CPU backend renders PNG/JPEG images, embedded-font Unicode text, filled and
stroked rectangles, ellipses and paths, with keyframed transforms, opacity and
track order. A random-access `FrameSource` adapter supplies video; its prototype
implementation renders a labelled checkerboard. Missing plugins also remain
visible as placeholders. Audio stays in the evaluated scene for a future mixer.
Text and shapes are temporary `builtin` sources pending the plugin API.

## Contributing

Pull requests for 2.0 target **`v2`**, not `main` or `staging`. When 2.0 ships, this branch is merged into `main` and 1.x moves to a read-only `legacy` branch.

## License

[MIT](LICENSE)
