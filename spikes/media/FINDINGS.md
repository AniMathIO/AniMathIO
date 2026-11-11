# Media spike: are the FilmCraft codec crates our media backend?

**Date:** 2026-11-11 · **Branch:** `v2-spike-media` · **Throwaway code in `spikes/media/`**

## Question

Can the FilmCraft codec crates (`filmcraft-codecs`, `-h264`, `-h264enc`, `-hevc`, `-vp9`,
`-av1`, `-aac`, `-opus`, `-isobmff`, `-matroska`) do frame-accurate decode + seek and encode a
playable MP4, or do we need an FFmpeg fallback backend?

## Short answer

**FilmCraft + FFmpeg fallback.** FilmCraft's decoders are bit-exact against FFmpeg and
frame-exact on random access (100 % seek match on every CFR file, 99 % on a synthetic VFR file),
and its H.264 + AAC encoders mux a valid, fully-decodable MP4 with 0 s A/V offset. Keep FilmCraft
as the **primary** backend behind `MediaBackend`. Keep FFmpeg as a **fallback** because FilmCraft
publishes **no VP9/AV1/Opus *encoders*** (WebM export) and because the crates are 0.9.0, six weeks
old, from one upstream — a fallback is cheap insurance for edge cases and codecs we don't exercise
here (AV1, HEVC 10-bit/HDR, MPEG-2, ProRes, unusual/corrupt files).

## Crate versions

All `0.9.0` (crates.io, published 2026-10-23 … 2026-11-04). No docs.rs build; APIs read from
source. `filmcraft-codecs` is a hub that pulls ~30 codec crates plus `symphonia` (audio bootstrap)
and `image` (JPEG); it has no feature flags to trim.

| Crate | Role |
|---|---|
| `filmcraft-codecs` 0.9.0 | container + codec hub: `Mp4Source`/`MkvSource`, GOP-aware seek, decoder registry |
| `filmcraft-h264` 0.9.0 | H.264/AVC decoder |
| `filmcraft-h264enc` 0.9.0 | H.264/AVC encoder (Baseline/Main/High, CRF/CBR/VBR, B-frames) |
| `filmcraft-hevc` 0.9.0 | HEVC decoder |
| `filmcraft-vp9` 0.9.0 | VP9 decoder (no encoder) |
| `filmcraft-av1` 0.9.0 | AV1 decoder (no encoder) |
| `filmcraft-aac` 0.9.0 | AAC-LC encoder **and** decoder |
| `filmcraft-opus` 0.9.0 | Opus decoder (no encoder) |
| `filmcraft-isobmff` 0.9.0 | MP4/MOV demux + mux (`Mp4Writer`, faststart) |
| `filmcraft-matroska` 0.9.0 | Matroska/WebM demux + mux |
| `filmcraft-time` 0.9.0 | integer ticks + rational frame rates (TICKS_PER_SECOND = 254 016 000 000) |
| `filmcraft-media` 0.9.0 | `MediaSource` trait, `VideoFrame`, `AudioBuffer` |
| `filmcraft-frame` 0.9.0 | frame/audio buffer types |

Rust toolchain required: `rust-version = 1.95`, edition 2024. Built with rustc 1.99.0.

## Corpus

`spikes/media/scripts/gen_corpus.sh` (uses system `ffmpeg`, frame counter burned in via
`drawtext`). 1280×720, 5 s. No v1 test media exists — `git ls-tree origin/staging` contains only
`.ts`/`.png` source and release assets, no `.mp4`/`.webm`/audio fixtures.

- `h264_30fps.mp4` — H.264 High + AAC, 30 fps
- `h264_2997.mp4` — H.264 High + AAC, 29.97 fps
- `hevc_30fps.mp4` — HEVC + AAC, 30 fps
- `vp9_30fps.webm` — VP9 + Opus, 30 fps
- `h264_vfr.mp4` — H.264, genuinely variable frame rate (60 fps first half, 30 fps second half)
- `aac_48k.m4a`, `opus_48k.opus` — audio-only

## Decode + seek (release build, 720p, this machine)

Method: FilmCraft `video_frame(FrameRequest::full(tick))` vs a full FFmpeg `rawvideo yuv420p`
reference dump (presentation order). Compared **codec-native YUV planes**, so the comparison is a
pure decoder check, not a colour-conversion check. "Match" = PSNR ≥ 45 dB; in practice every match
was bit-exact (999 dB). 200 random-access seeks per file, at frame boundaries.

| File | Codec | Decode fps | Seq vs FFmpeg | Seek p50 / p95 | Cold seek¹ | Seek match |
|---|---|---|---|---|---|---|
| h264_30fps.mp4 | H.264 High | 827 | bit-exact (999 dB) | 0.05 / 0.06 ms | 21.9 ms | 100 % |
| h264_2997.mp4 | H.264 High | 1305 | bit-exact (999 dB) | 0.05 / 0.06 ms | 25.8 ms | 100 % |
| hevc_30fps.mp4 | HEVC | 709 | bit-exact (999 dB) | 0.05 / 0.06 ms | 101 ms | 100 % |
| vp9_30fps.webm | VP9 | 462 | bit-exact (999 dB) | 0.05 / 0.07 ms | 159 ms | 100 % |
| h264_vfr.mp4 | H.264 (VFR) | 1382 | bit-exact (999 dB) | 0.11 / 0.16 ms | 9.8 ms | 99 %² |

¹ Cold seek = a fresh backend's first decode at mid-clip: GOP-aware seek decodes forward from the
preceding keyframe (no warm GOP cache). The tiny p50/p95 are the cached-GOP path; the cold number is
the one that matters for scrubbing to an arbitrary point.
² The single miss is the concat boundary frame of the synthetic VFR file (segment join), not a
general VFR failure — sequential VFR decode is bit-exact across the whole file.

**Audio:** `decode_audio(range)` compared to FFmpeg `f32le` mono.

| File | Codec | RMS vs FFmpeg | Best-lag alignment |
|---|---|---|---|
| aac_48k.m4a | AAC-LC 48 kHz | 0.073 (raw) | −19 samples (0.4 ms) → 0.0004 RMS |
| opus_48k.opus | Opus 48 kHz | 0.0048 | 0 samples → 0.0048 (≈1 % gain diff) |

Both decoders are sample-correct. The AAC −19-sample (0.4 ms) offset is within half an AAC frame
(1024 samples) and comes from priming/edit-list handling; it's why the *raw* RMS looks big. Opus is
sample-aligned with a ~1 % gain difference.

## Encode

Generated 10 s of synthetic RGBA (1280×720, moving gradient + box) + 440 Hz sine (48 kHz stereo),
encoded H.264 High CRF 23 (`filmcraft-h264enc`, `LengthPrefixed`) + AAC-LC 128 kbps
(`filmcraft-aac`), muxed to MP4 with `filmcraft-isobmff::Mp4Writer` (faststart, edit lists for
B-frame delay and AAC priming).

| Check | Result |
|---|---|
| Encode speed | 300 frames in ~10 s (~30 fps) with 2 encoder threads (default is all cores) |
| `ffprobe` video | h264, 30/1, start 0.000000 s, duration 10.000000 s, 300 frames ✓ |
| `ffprobe` audio | aac, start 0.000000 s, duration 10.005000 s, 470 frames ✓ |
| **A/V start offset** | **0.000 s (within ½ frame)** ✓ |
| `ffmpeg -v error -i out.mp4 -f null -` | decodes fully, zero errors ✓ |
| Round-trip (FilmCraft decodes its own MP4) | YUV PSNR 48.8–50.7 dB vs synthetic (lossy, as expected) |

## Licensing

- All `filmcraft-*` crates: **MIT OR Apache-2.0** (dual). The repo ships `LICENSE-APACHE` and
  `LICENSE-MIT`. Permissive, and the issue's "vendor it if upstream disappears" fallback is viable.
- `filmcraft-codecs` pulls `symphonia` (MPL-2.0) for audio bootstrap (MP3/FLAC/ALAC/HE-AAC and the
  MP4/MKV demux fallback); MPL-2.0 is file-level copyleft — add it to the `cargo-deny` allowlist and
  note it in attribution. `image` (MIT/Apache) is used for JPEG.
- Patent exposure is unchanged from v1's FFmpeg export: H.264/HEVC/AAC sit in patent pools; VP9/AV1/
  Opus are royalty-free.

## API friction

1. **No docs.rs** for any crate — you read the source. The lib.rs doc comments and FilmCraft's own
   `crates/export` are the real documentation.
2. **Hub crate is heavy and untrimmable** — `filmcraft-codecs` has no features; depending on it
   builds ~30 codec crates + symphonia + image (release `target/` ≈ 500 MB).
3. **`open_bytes` loads the whole file into memory.** `reader_openers` + `ByteReader` exist for
   indexed/streaming demux (large-file path); the spike used `open_bytes` for simplicity.
4. **`to_rgba8()` colour default differs from FFmpeg.** The YUV planes are bit-identical, but for
   streams whose colour matrix is *unspecified* FilmCraft converts to RGB with BT.709 (HD
   resolution heuristic) while FFmpeg defaults to BT.601 — the spike's RGB frames differed by
   ~24.5 dB for that reason. Our own encodes signal BT.709 explicitly, so this mostly affects
   third-party files with unspecified colour; it's a `ColorInfo`-level fix and a golden-test
   concern, not a decode-correctness issue.
5. **Encoder takes planar 4:2:0 YUV, not RGBA.** There is no published RGBA→YUV helper (FilmCraft's
   `convert.rs` lives in its export crate); the spike wrote a ~20-line BT.709 limited-range
   conversion.
6. **Muxing is manual** — track timescale = fps numerator, sample duration = fps denominator,
   `composition_offset = pts − dts`, and edit-list `media_start` for B-frame delay (video) and AAC
   priming (audio). Following FilmCraft's own export crate (`crates/export/{encoders,job}.rs`)
   reproduces it exactly and yields a correct file.
7. **Time is integer ticks** (`filmcraft_time::Tick`, 254 016 000 000/s) with rational `FrameRate` —
   matches the 2.0 "no `f64` seconds" rule directly.
8. Crates are **0.9.0 / six weeks old / one fast-moving upstream** — everything above could shift
   under a Renovate bump, so `MediaBackend` + golden/fuzz tests are the right guardrail (and
   `cargo-deny` gates each bump).

## Reproduce

```sh
cd spikes/media
./scripts/gen_corpus.sh                                   # regenerates corpus/ (gitignored)
nice -n 19 ionice -c3 env CARGO_BUILD_JOBS=3 \
  CARGO_PROFILE_DEV_DEBUG=line-tables-only cargo run --release
```

## Recommendation

**FilmCraft + FFmpeg fallback.**

- **Decode/seek: GO.** Bit-exact vs FFmpeg, 100 % frame-exact random access, and far faster than
  real time (H.264 ≈ 800–1300 fps, HEVC ≈ 700 fps, VP9 ≈ 460 fps at 720p). Cold GOP seeks are
  10–160 ms — fine for scrubbing.
- **H.264+AAC MP4 export: GO.** Valid, fully-decodable, 0 s A/V offset, ~real-time at 720p on 2
  threads.
- **FFmpeg fallback stays for:** WebM export (VP9/AV1/Opus *encode* — FilmCraft ships no encoders
  for those), AV1/HEVC-10-bit/HDR/ProRes/MPEG-2 and other long-tail codecs, and robustness against
  corrupt or unusual files while the pure-Rust parsers are young. It also gives us an escape hatch
  if the single upstream stalls.
