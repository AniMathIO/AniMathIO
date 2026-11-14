# v1 serializer corpus

These are real `ProjectStore.serialize()` outputs, generated headlessly from
v1 revision `147f522d4609db9616b6f5f9fe19a19ce61fcbdf`, rather than JSON
assembled by Rust tests. The adjacent JSON files are inflated inspection copies;
the CLI tests consume the compressed `.animathio` files. Inputs are constructed
store state, not projects collected from users. No private media is included.

## Reproduction

In a temporary directory extract (never check out main in the working tree):

```bash
git archive 147f522d4609db9616b6f5f9fe19a19ce61fcbdf renderer/states/stores/ProjectStore.ts renderer/types.ts renderer/utils/manim renderer/utils/color.ts | tar -x -C "$temp"
```

Install `tsx@4.23.15`, `mobx@7.1.7`, `animejs@4.5.0`,
`pako@3.0.3` there with npm (Node 22.23.5). Copy `generate.ts` there.
The serializer runs unmodified. The generator supplies a Blob/FileReader adapter
and minimal root stores. It checks that `konvaNode` was stripped. Node's fetch
reads the data URLs, so media embedding follows the original asynchronous code.
Audio defaults (volume 1, muted false) are applied by v1 itself.

For the translator only, replace the `@/utils` getUid import with a deterministic
incrementing ID function, point the color import to `../color`, and replace the
KaTeX renderer import with a function that throws (this fixture only uses Text).
All translation and serialization logic remains from v1.
Generate the local public-domain media with:

```bash
ffmpeg -f lavfi -i color=c=orange:s=16x16 -frames:v 1 -threads 1 tile.png
ffmpeg -f lavfi -i color=c=blue:s=16x16:r=2 -t 1 -an -threads 1 -pix_fmt yuv420p tile.mp4
ffmpeg -f lavfi -i sine=frequency=440:sample_rate=8000 -t 0.1 -threads 1 tone.wav
npx tsx generate.ts /absolute/path/to/fixtures
```

## Coverage and limits

- All five v1 element kinds, all six animation types, all five effect choices.
- Embedded PNG, H.264 MP4 and PCM WAV; missing external image reference.
- Unicode, bold weight/color/size, non-unit placement scales, audio defaults,
  empty project and a trillion-millisecond project.
- Actual v1 Manim translator output (Text + Write).
- Mafs raster snapshot storage is exercised using the generated tile. It is not
  a genuine graph/LaTeX rasterization: DOM/KaTeX/Mafs capture remains deferred.
- Current v1 declares no schema version. Historical Fabric/plain-JSON serializer
  snapshots, additional slide directions and real user-authored files remain
  deferred; do not interpret this corpus as complete M4 exit coverage.

The public CLI integration test imports every compressed file, validates the
archive and renders frames 0, 15 and 60 at 160×90. Representative PNG goldens
are compared with a maximum absolute RGBA channel difference of 2.
