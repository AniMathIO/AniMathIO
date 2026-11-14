# Math showcases as end-to-end tests

Three 24-second explainers built entirely through the shipped `animathio-cli`
and its headless MCP server. Python constructs serialized **Commands**, never
project JSON; `new --template math_explainer` supplies the starting archive and
`batch` removes the template contents. Compound scene construction is one atomic
undo step. There is no schema change. Headings use `apply_animation_preset` with the bundled
`org.animathio.animations` fade-in preset.

| Directory | Explainer | Recording |
|---|---|---|
| `pythagorean` | Four congruent 3–4–5 triangles rearrange, leaving equal square areas | Bach, Prelude in C major BWV 846 — Kimiko Ishizaka, CC0 |
| `unit-circle` | A rotating point's vertical projection traces two sine periods | Satie, Gymnopédie No. 1 — Robin Alciatore / Musopen, public-domain dedication |
| `fourier` | One, two, then four odd harmonics sum through rotating epicycles | Debussy, Clair de lune — USAF Wright Brass, public domain in the US |

Each directory contains its entry-point `build.py`, recording manifest and golden
frames. Shared mechanics live in `common.py`. Both 16:9 and 9:16 have deliberate
vector layouts: a media crop/reframe would cut off equations and diagrams. An
explicit shape background keeps transparent PNG rendering and opaque MP4 deliveries
consistent. Output is 1920×1080 / 1080×1920 at 30 fps, with a smaller CI mode. The
examples use LaTeX write-on, parametric curves, graph draw-on, normalized shapes,
keyframes, animation presets, caption styles, markers and beat-synced cuts. Pythagoras also uses a real track-level equation dissolve.

## Build and export

Requires Python 3.11+, the built CLI and first-party plugins. Follow the resource
limits in the root AGENTS.md when building Rust. No display or editor is needed.

```sh
export ANIMATHIO_PLUGIN_DIR="$PWD/target/plugins"
python3 examples/showcase/pythagorean/build.py --offline
python3 examples/showcase/unit-circle/build.py --offline --vertical
python3 examples/showcase/fourier/build.py --offline

# Download the pinned recordings, then export all six full website deliveries.
# This is opt-in and is never run in CI.
scripts/showcase-export.sh target/showcase-website
```

`ANIMATHIO_CLI` selects a different built CLI. `SHOWCASE_CACHE` selects the music
cache (default `target/showcase-cache`). Without `--offline`, builders use a
verified recording if present and otherwise generate a deterministic PCM click
track of exactly the same duration. `--require-music` rejects a missing recording.
The export script requires real music. It never silently publishes a click track.
Audio and generated archives/MP4s stay outside Git under `target/` by default.

## Music provenance and beat timing

`music.toml` distinguishes the **recording's** license from the old composition.
It pins the original URL, description/license page, performer, SHA-256, excerpt
offset, duration, detected beats and editorial story boundaries. Source pages:

- [Bach recording and CC0 dedication](https://commons.wikimedia.org/wiki/File:Kimiko_Ishizaka_-_Bach_-_Well-Tempered_Clavier,_Book_1_-_01_Prelude_No._1_in_C_major,_BWV_846.ogg)
- [Satie recording and performer's public-domain dedication](https://commons.wikimedia.org/wiki/File:Erik_Satie_-_gymnopedies_-_la_1_ere._lent_et_douloureux.ogg)
- [Debussy recording and USAF public-domain statement](https://commons.wikimedia.org/wiki/File:Clair_de_Lune_-_Wright_Brass_-_United_States_Air_Force_Band_of_Flight.mp3)

The Debussy choice is a brass arrangement, not the commonly shared CC BY piano
recording. Its recording is a US government work; the manifest preserves that
jurisdictional qualification. End credits are embedded in each composition.

```sh
python3 examples/showcase/fetch-music.py
python3 examples/showcase/detect-beats.py pythagorean
```

The fetcher verifies existing files too, uses a temporary file for downloads and
publishes only a matching checksum. Beat detection is a maintainer-only operation
on the real excerpt through `animathio-cli beats`. Its settings are documented in
`detect-beats.py`; recorded outputs are in each manifest. Review detection and
story boundaries before replacing them: expressive classical timing is not a
perfect metronome. Ordinary builds always use these pinned markers. A visible
progress rail is split with `sync_cuts_to_beats` at those markers and advances on
each beat. Story boundaries, captions and harmonic changes use selected markers, with the
last interval ending at the 24-second project boundary.
CI neither downloads audio nor re-detects the synthetic track.

## Offline regression suite

Requires Pillow and FFmpeg/ffprobe, only as test tooling; AniMathIO encodes the MP4s.

```sh
python3 examples/showcase/test.py
# Retain artifacts when inspecting a regression:
python3 examples/showcase/test.py --output target/showcase-e2e
# Deliberate reference refresh; review the PNGs before committing:
python3 examples/showcase/test.py --update-golden --output target/showcase-review
```

CI builds all six projects with click audio, checks model structure and beat
boundaries, renders four PNGs per layout (including an equation/graph transition),
and compares every RGBA channel with maximum absolute error 2 (the renderer's existing golden policy). It exports
seconds 8–10 at 12 fps per project, then checks H.264/AAC, dimensions, duration,
24 decoded frames, non-silent decoded audio and A/V start alignment, and decodes
the complete file with FFmpeg's fatal-error mode. The full 1080p deliveries are deliberately outside CI.

## Capability gaps

- **Semantic equation morphing:** equations dissolve rather than matching symbols.
  A plugin animation could match stable glyph/group identities and return commands
  or vector interpolation data. Preserve editable source and avoid core math types.
- **Shape morphing:** triangle translation/rotation works, but unrelated outlines
  cannot interpolate point correspondence. A shape-plugin morph parameter with
  topology validation and explicit resampling would support geometric proofs.
- **Parented/analytic motion:** epicycle centers and the sine projection use 241
  sampled keys per position curve; radius vectors rotate with unwrapped linear
  rotation keys. Parametric graph curves now exist, but an animatable phase and
  expression-bound endpoints would compute exact sums without thousands of keys.
- **Graph draw-on and moving points share no binding:** the graph's `draw` key and
  separately sampled tracing point are synchronized by this script. A graph item
  exposing an evaluated endpoint for attachment, or a combined math-plugin rig
  driven by one phase parameter, would keep them exact together.
- **Graph axes styling:** this change exposes plot stroke width and grid/axis colors.
  Tick placement/formatting, axis widths and arrowheads remain fixed. These examples
  suppress numeric labels and add text where needed. Expose normalized axis styles
  and π-aware tick formatting in the graph plugin.
- **Write-on is contour reveal:** LaTeX reveals glyph contours, not pen-ordered
  semantic strokes. It is usable here but a true handwriting effect needs ordered
  centerlines/path trim and a documented reveal contract.
- **Save after removing assets:** headless `RemoveAsset` removes the manifest entry,
  but retained undo-cache bytes make archive saving fail with “asset manifest does
  not match container”. These builders start from an asset-free bundled template.
  Saving should serialize only active assets while preserving cached bytes for undo.

Deferred: fixing that general asset-save issue, semantic morphs, declarative
constraints/parenting, a new format version, narrated speech, website publishing
and CI full-length delivery encoding.
