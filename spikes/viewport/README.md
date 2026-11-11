# GPUI viewport risk spike

Throwaway, isolated Cargo project. Read [FINDINGS.md](FINDINGS.md) before treating this as an architecture recommendation.

```sh
cd spikes/viewport
export CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=line-tables-only
nice -n 19 ionice -c3 cargo build --locked
./target/debug/animathio-viewport-spike
```

Click Play/Pause; click or drag the timeline to seek. The source frame is always 1920×1080; the initial window reserves a 1920×1080 logical-pixel preview. Timeline state is integer ticks (60,000/s). Scene evaluation converts ticks to floating point only for drawing. This is a synthetic vector workload, not a video decoder or a project model.

For the final optimized measurement:

```sh
nice -n 19 ionice -c3 cargo build --locked --release
./target/release/animathio-viewport-spike --bench
# X11 (XWayland when running within KDE Wayland):
env -u WAYLAND_DISPLAY ./target/release/animathio-viewport-spike --bench
```

`--bench` opens a real GPUI window, warms up for 2 seconds, scrubs pseudo-randomly for 10 seconds, prints p50/p95 and throughput, then closes the app. Keep the window visible. Rendering is synchronous and has one frame in flight. This deliberately exposes the cost of the simplest supported integration. Every generated frame replaces and explicitly evicts its predecessor from GPUI's image atlas.

Set `VIEWPORT_FONT` to a local TrueType/OpenType font file if `/usr/share/fonts/noto/NotoSans-Regular.ttf` is absent. Text uses a simple character-to-glyph mapping plus font advance widths; full shaping belongs in the engine, not this spike. Windows would need this override and an actual build/run.

Validation and cleanup (do not clean a shared/root target directory):

```sh
nice -n 19 ionice -c3 cargo fmt --check
nice -n 19 ionice -c3 cargo clippy --locked -- -D warnings
# After pushing the spike:
nice -n 19 ionice -c3 cargo clean
```

Use the same resource environment variables for **every** Cargo command. The committed lockfile and local `xattr` compatibility patch are needed on the current toolchain. Build artifacts are intentionally removed after delivery.
