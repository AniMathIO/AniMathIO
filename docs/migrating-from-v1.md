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

## Where 1.x features live

Audited against the frozen `legacy` sidebar, `renderer/types.ts`, the resource
panels and Mafs dialog. “Moved” means an equivalent workflow exists at the listed
location; “missing” identifies a concrete parity gap, not a hidden Library tab.
This describes authoring in 2.x; importing saved 1.x projects has the limitations below.

| 1.x feature | Where it lives in 2.x | Status |
| --- | --- | --- |
| Video / image / audio resources, upload | Library → Media; Import media; kind filters, grid/list and drag to timeline | Moved |
| Microphone audio recorder | Library → Media → Record audio (microphone button, or File → Record audio…): choose the input, watch the level meter, Stop to add a WAV asset (and a clip at the playhead) as one undo step, Cancel to discard | Moved |
| Plain text / Mafs text | Library → Elements → Text & formulas → Text; source and size in dialog/inspector | Moved |
| Title, Subtitle, Body, Caption, Heading 1–6 typography | Library → Titles → Typography has all ten presets with the 1.x sizes (normalized to the shorter canvas edge) and weights, all in Inter; Font weight (100–900) and Font are inspector fields on any text clip | Moved |
| LaTeX, matrix source, formula colour | Elements → Text & formulas → LaTeX; source, snippets, templates and colour in dialog/inspector | Moved |
| Natural-language → LaTeX (Gemini) | No equivalent; optional integration tracked in [#302](https://github.com/AniMathIO/AniMathIO/issues/302) | Missing |
| Mafs point, line segment, vector, polygon, circle, ellipse | Elements → Shapes & geometry; each has a configurable dialog and inspector | Moved |
| Mafs function plot | Elements → Graphs & grids → Plot; expression and ranges in dialog/inspector | Moved |
| Mafs Cartesian / polar coordinates | Elements → Graphs & grids → Cartesian grid / Polar grid; editable as separate layers | Moved |
| Fade in/out, directional slide in/out, breathe | Effects → Animations: In / Out / Emphasis; inspector → Animations | Moved |
| Mafs reveal in/out | Effects → zoom-in / zoom-out presets; LaTeX also has Write on | Moved |
| Clip-path slide and per-character slide | Clipped slide and Character slide presets (Effects → Animations, inspector → Animations); the importer maps the v1 options | Moved |
| None, black-and-white, sepia, invert, saturate effects | Effects → Video effects; inspector → Effects (remove/bypass gives None) | Moved |
| Audio mixer volume and mute | Track gain/mute/solo controls; clip inspector gain/mute; viewer monitor volume | Moved |
| Background Fill | File → Project settings → background colour | Moved |
| Element list, order, placement and timing | Multi-track timeline, track order and inspector → Transform | Moved |
| Manim source import | File → Import Manim scene | Moved |
| Canvas settings and MP4 video export | File → Project settings; Export dialog | Moved |
| WebM video export | No WebM output backend ([#303](https://github.com/AniMathIO/AniMathIO/issues/303)) | Missing |

Plugin clip sources appear only in Elements. Plugins is the installed inventory:
all loaded kinds, versions, enabled status and source directories. Titles contains
composition/caption presets; Effects contains video/audio effects, transitions
and animation presets. Media search and media-kind filters belong only to Media.

## What maps

| v1 | v2 |
| --- | --- |
| Canvas width/height/background/maxTime, fps | `Command::SetSettings` (width, height, background, duration, frame rate) |
| `editor_elements` z-order | One `Track` per element, in the same order |
| Embedded media (base64 data URLs) | Deduplicated content-addressed assets (`AssetId` = BLAKE3), registered via `AddAsset` |
| `image` element | `ClipKind::Image` |
| `video` element | `ClipKind::Video` (decoded by the `media` crate) |
| `audio` element | `ClipKind::Audio` with `gain` + `muted` clip params; `masterVolume` → `SetTrackGain` |
| `text` element | `ClipKind::Plugin` (`org.animathio.text`) with normalized font size and color |
| `mafs` graph element | `ClipKind::Image` (see gaps below) |
| `fadeIn` / `fadeOut` | Opacity keyframes |
| `slideIn` / `slideOut` | X/Y keyframes (directions `left`/`right`/`top`/`bottom`) |
| `breathe` | ScaleX/ScaleY keyframes (bounded by slide-in/out window) |
| `mafsReveal` | Opacity keyframes as a fallback |
| Effects (`blackAndWhite`, `sepia`, `invert`, `saturate`) | One `EffectInstance` per clip (`grayscale`/`sepia`/`invert`, and `saturate` with `amount = 2`) from the `org.animathio.effects-basic` plugin, run in the plugin sandbox |

v1 stores milliseconds and pixels as `f64`; v2 stores integer ticks and normalized
canvas fractions. The conversion is exact-but-lossy by design, with bounds checked
where they matter.

## What doesn't map (known gaps)

- **Mafs graphs become static images.** v1 rasterized graphs to a PNG and never
  stored the graph definition, so only a marker (`v1.mafs`) survives for a future
  graph plugin. A warning is emitted.
- **Unknown effects are ignored** with a warning. The four v1 presets become
  real, renderable plugin effects (`grayscale`/`sepia`/`invert`, `saturate` with
  `amount = 2`; the plugin must be installed to render them).
- **Rotation is approximated as center-based.** v1 rotated around the Konva
  top-left corner; the importer warns and uses the element center.
- **`fontWeight` is preserved** as the text plugin’s `font_weight` (100–900; values outside that range warn and use 400). An element that carries a `fontWeight` imports with the text plugin’s `font = inter`, so every weight (400 included) is the bundled Inter and the typeface does not change between weights; elements without one keep the default DejaVu Sans. Weighted shaping has no kerning.
- **Slide options become the `slide` effect or the text glyph slide.** A
  `slideIn`/`slideOut` with `useClipPath` ("Use Mask") is imported as a keyframed
  `org.animathio.effects-basic` `slide` effect on the clip, and `textType: "character"` on a
  text clip as keyframes of the text plugin's `char_slide*` parameters, instead of the
  whole-element position slide (that already moves the content, so keeping both would move
  it twice); every such animation adds a warning that the look differs. v1 never rendered the mask, and its per-character slide
  existed for slide-in on text only, travelled from beyond the canvas edge and faded the
  letters in; v2 slides within the element's own bounds, supports slide-out too and does
  not fade. A character slide on a non-text element is ignored with a warning (a mask, if set,
  still applies, otherwise the ordinary slide imports); a direction other than left/right/top/bottom imports as the plain slide.
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

- **WebM export.** Video clips import as ordinary video clips and export to H.264 + AAC
  MP4 like any other project; WebM output is not implemented.
- **Manim import** is a separate importer (`crates/import-manim`), run inside the
  `org.animathio.import-manim` plugin and used via `animathio-cli import-manim`, the
  editor's **Import Manim scene…** dialog or the MCP `import_manim_scene` tool.
- **v1's Animations panel** (fade, slide, breathe, Mafs reveal) maps to the
  `org.animathio.animations` presets in the inspector's Animations section: `fade-in`,
  `fade-out`, `slide-in`, `slide-out`, `breathe` and `zoom-in`/`zoom-out` (reveal). v1's
  clip-path slide and per-character slide are the `clipped-slide-in/out` and
  `character-slide-in/out` presets; `typewriter-in` is an alternative that reveals text
  one character at a time, not a replacement.
- **Round-tripping.** Import is one-way; there is no v2 → v1 export.

## Verifying an import

```bash
animathio-cli import-v1 project.animathio -o imported.amio
animathio-cli validate imported.amio
animathio-cli render imported.amio --frame 0 -o frame.png
```

`validate` checks model invariants and asset hashes; it does not decode video.

## Serializer fixture corpus

The checked-in corpus under `crates/import-v1/tests/fixtures` is produced by
running v1's own serializer on constructed store state. Public CLI integration
coverage imports and renders every compressed project at three times, including
all v1 element, animation and effect types, embedded media, missing media,
Unicode, non-unit scales, empty and very long projects, and a v1 Manim Text
translation. Placement reads v1's `scaleX`/`scaleY` names (older snake-case
inputs remain accepted).

This is not yet the complete M4 corpus: actual DOM-rasterized LaTeX/graph
captures, historical serializers and user-authored projects remain to be added.
The fixture README records provenance and regeneration instructions.

## Pitch preservation and project schema

Schema 8 stores a per-clip `maintain_pitch` boolean. Projects saved with schema
1–7 and imported v1 clips keep it off, preserving the previous varispeed sound.
New clips default to on; the option has no effect at normal playback speed.
Enable **Maintain pitch** in the Speed inspector or Speed/Duration dialog to
opt an older clip into time stretching. The change follows linked companions
and is undoable. Saving writes schema 8; older editors reject that schema.
