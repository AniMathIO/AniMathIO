# AniMathIO 2.0 release notes

> The human-written summary for the `v2.0.0` GitHub release; the full list of changes is
> in [`CHANGELOG.md`](../CHANGELOG.md).

AniMathIO 2.0 is a from-scratch rewrite of the editor in Rust. The core is a small,
reliable video editor (media clips, a multi-track timeline, transforms and keyframes,
audio, preview and export); the math and creative features are sandboxed WebAssembly
plugins, so the core stays small and every capability has a versioned API. Projects use
the new `.amio` format. AniMathIO 1.x moves to a read-only `legacy` branch.

## Faster and lighter than 1.x

Measured on one Linux machine under Xvfb, medians of three runs (details and caveats in
[`benchmarks.md`](benchmarks.md)):

| | 1.8.3 (Electron) | 2.0 (Rust) |
| --- | ---: | ---: |
| Export, 60 s 1080p30 MP4 | 116.8 s | 37.4 s |
| Export, 10 s 1080p30 MP4 | 20.4 s | 6.5 s |
| Open a project | 1.08 s | 0.21 s |
| Start to a usable window | 0.81 s | 0.20 s |
| Memory, idle with a project open | 923 MiB | 487 MiB |
| Size on disk (unpacked 1.x build vs. 2.0 install) | 658 MiB | 293 MiB |

## What's new since v1

**A real editing application.** The editor is a dark, dockable NLE built on
gpui-component: resizable panels, a vertically scrolling timeline, S/M/L track heights, a
project launcher with bundled and user templates, context menus, toasts, an export queue
and a Premiere Pro default keyboard map. The preview renders on the GPU (Vello on wgpu)
when an adapter is available and falls back to the CPU reference otherwise.

**Timeline editing.** Multi-select with rubber-band, copy/cut/paste and duplicate that
never overwrite; group move, nudge, delete and split as one undo step; ripple edits
across all unlocked tracks; transitions (cross dissolve, dip to black and white, wipe)
with audio crossfades; Rate Stretch, Slip, Slide and Rolling Edit tools; markers and beat
sync; clip speed, reverse, keyframed speed ramps and frame holds; and linked companion
audio for imported video.

**Text, titles and captions.** Text and shapes render through plugins. Eight animated
title presets (centered title, title with subtitle, lower third, caption bar, chapter
card, end card, kinetic word pop and math title) are built from plugin clips and laid out
inside the safe area. Captions come from local Whisper transcription with caption styles
(Classic bar, Word highlight, Karaoke fill, Pop, one word at a time, Bold outline) and
SRT/WebVTT import and export.

**Math and import.** LaTeX (Typst with MiTeX), a graph plugin and a math object workflow
(LaTeX, text, points, lines, vectors, polygons, plots, circles, ellipses and Cartesian or
polar grids) with live previews. Manim scenes import through a sandboxed plugin, and v1
`.animathio` projects import through a native importer.

**Audio.** Streaming decode of WAV/PCM, MP3, FLAC, AAC, Vorbis and Opus; surround downmix
to stereo and mono; and a shared mixer with clip- and track-level effect chains (gain,
3-band EQ, compressor, noise gate, high-pass, voice cleanup and denoise), automatic
ducking and waveform display.

**Effects and automation.** The eight video effects and the audio effects are plugins.
Automatic cuts review silence, filler words and scene changes before one undoable edit;
auto-reframe tracks subjects to 9:16, 1:1 or 4:5; a local control protocol and MCP
sidecar expose the same operations to automation, and the reproducible math showcases
are authored only through the public CLI and MCP surface.

**Engine and quality.** Rendering is one pure function shared by preview, export and
tests. Golden-frame tests cover the CPU reference renderer, CI runs on Linux and Windows,
and the installers are built, smoke-tested and provenance-attested on every release PR.

## Install

There is no published 2.0 release yet at the time of writing. When `v2.0.0` is tagged,
download the installer for your platform from the GitHub release. Each installer ships
both `animathio` and `animathio-cli` plus the first-party plugins:

| Platform | File | Notes |
| --- | --- | --- |
| Linux | `animathio_<version>_<arch>.AppImage` | the host provides the runtime libraries |
| Linux (Debian/Ubuntu) | `animathio_<version>_amd64.deb` | declares its runtime libraries as dependencies |
| Windows | `animathio_<version>_x64-setup.exe` | NSIS installer, unsigned |
| macOS | `AniMathIO_<version>_<arch>.dmg` | unsigned and not notarized |

Verify an install with `animathio --version` and `animathio-cli --version`.

### Checksums and provenance

The release ships a `SHA256SUMS` file covering every installer. Download it next to the
installers and check them with:

```bash
sha256sum -c SHA256SUMS
```

CI-built installers also carry a build-provenance attestation, verifiable with
`gh attestation verify <file> --repo AniMathIO/AniMathIO`.

## Known limitations

- **The Windows installer is unsigned.** Windows SmartScreen shows a warning. To run it,
  click **More info** and then **Run anyway**.
- **The macOS disk image is unsigned and not notarized.** Gatekeeper blocks the app on
  first launch. Right-click the app and choose **Open**, or allow it under **System
  Settings → Privacy & Security → Open Anyway**.
- **Export is MP4 only** (H.264 video with AAC audio). WebM, AV1 and other containers are
  not implemented yet.
- **The editor has not been hand-tested on Windows.** CI builds it and runs the test
  suite there, but no maintainer has walked through the UI on a Windows machine.
- **Flatpak, AUR and Flathub are not shipped yet.**
- **Mafs graphs from v1 import as static images**, because v1 never stored the graph
  definition. See the migration guide for the full list of import gaps.
- **Update notices only.** Since 2.0.1 the editor checks GitHub at most once a day and
  shows a notice when a newer 2.x release exists (opt out in Preferences or with
  `ANIMATHIO_NO_UPDATE_CHECK=1`). It never downloads or installs anything itself.

## Migrating from v1

Import a v1 `.animathio` project with `animathio-cli import-v1 project.animathio -o imported.amio`.
The import is one-way and prints non-fatal warnings to stderr. What maps and what does
not is documented in [`migrating-from-v1.md`](migrating-from-v1.md).
