# Migrating from v1

`crates/import-v1` is a **one-way** importer for v1 (Electron/TypeScript)
`.animathio` projects. v1 serialized a project as pako-deflated JSON (the earliest
releases used plain JSON) with media embedded as base64 data URLs. The importer
inflates, parses leniently, decodes media into content-addressed BLAKE3 assets and
rebuilds a v2 `Project` exclusively through `animathio_core::Command`s.

```bash
animathio-cli import-v1 project.animathio -o imported.amio
```

Warnings go to stderr; the import succeeds as long as the project is structurally
usable. Resource limits guard untrusted input: ≤ 2 GiB inflated JSON, ≤ 1 GiB per
embedded asset, ≤ 10,000 media entries.

## What maps

| v1 | v2 |
| --- | --- |
| Canvas width/height/background/maxTime, fps | `Command::SetSettings` (width, height, background, duration, frame rate) |
| `editor_elements` z-order | One `Track` per element, in the same order |
| Embedded media (base64 data URLs) | Deduplicated content-addressed assets (`AssetId` = BLAKE3), registered via `AddAsset` |
| `image` element | `ClipKind::Image` |
| `video` element | `ClipKind::Video` (renders a checkerboard until the media decoder lands) |
| `audio` element | `ClipKind::Audio` with `gain` + `muted` clip params; `masterVolume` → `SetTrackGain` |
| `text` element | `ClipKind::Builtin(Builtin::Text)` with normalized font size and color |
| `mafs` graph element | `ClipKind::Image` (see gaps below) |
| `fadeIn` / `fadeOut` | Opacity keyframes |
| `slideIn` / `slideOut` | X/Y keyframes (directions `left`/`right`/`top`/`bottom`) |
| `breathe` | ScaleX/ScaleY keyframes (bounded by slide-in/out window) |
| `mafsReveal` | Opacity keyframes as a fallback |
| Effects (`blackAndWhite`, `sepia`, `invert`, `saturate`) | Stored as clip params `effect.<name> = 1`; **rendering is pending** |

v1 stores milliseconds and pixels as `f64`; v2 stores integer ticks and normalized
canvas fractions. The conversion is exact-but-lossy by design, with bounds checked
where they matter.

## What doesn't map (known gaps)

- **Mafs graphs become static images.** v1 rasterized graphs to a PNG and never
  stored the graph definition, so only a marker (`v1.mafs`) survives for a future
  graph plugin. A warning is emitted.
- **Effects aren't rendered yet.** `blackAndWhite`/`sepia`/`invert`/`saturate` are
  preserved as clip parameters but the renderer does not apply them; unknown
  effects are ignored with a warning.
- **Rotation is approximated as center-based.** v1 rotated around the Konva
  top-left corner; the importer warns and uses the element center.
- **`fontWeight` is dropped** — the v2 builtin text source has no weight
  equivalent.
- **Per-character text animation is flattened.** A character-level slide is
  imported as a whole-element slide (warning).
- **`mafsReveal` is a fallback.** It becomes an opacity fade, not a path reveal.
- **Animations with no `targetId`, unknown types, or invalid durations are
  skipped**, as are animations targeting missing elements. A slide/fade whose
  duration exceeds the clip is clamped to the clip length.
- **Fractional or invalid fps is rounded** (fractional to the nearest integer,
  invalid to 60). Invalid `maxTime` falls back to 30 s; invalid canvas values
  fall back or clamp to 65535.
- **Unknown top-level fields, element/property/media/animation fields and
  element types are ignored** with warnings, never fatal.
- **Duplicate element ids**: animations target the last element with that id
  (warning).

## Not covered by this importer

- **Video playback/export.** Video clips import as data, but decoding and MP4/WebM
  export await the `media` crate; the renderer draws a labelled checkerboard.
- **Manim import** is a separate importer (`crates/import-manim`), used via
  `animathio-cli import-manim`.
- **Round-tripping.** Import is one-way; there is no v2 → v1 export.

## Verifying an import

```bash
animathio-cli import-v1 project.animathio -o imported.amio
animathio-cli validate imported.amio
animathio-cli render imported.amio --frame 0 -o frame.png
```

`validate` checks model invariants and asset hashes; it does not decode video.
