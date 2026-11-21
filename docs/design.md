# AniMathIO 2 visual design

The editor should feel like a modern pro NLE (DaVinci Resolve 19, Final Cut Pro,
Premiere 2025): calm dark surfaces, floating rounded panels, one vivid accent,
colour-coded clips, crisp monospace timecode and subtle motion. All values live in
`apps/animathio/src/style.rs`; don't hard-code colours elsewhere.

## Rules

- **Icons:** Lucide via gpui-component `Icon`/`IconName` (or bundled SVGs). Never
  emoji or Unicode symbol glyphs (the `glyph_guard` test enforces this).
- **Surfaces:** the window background (`BG`) shows as a 6px gutter between
  floating panels (`PANEL`, radius 10, 1px `BORDER`). Raised elements (cards, inputs,
  popovers) use `RAISED`; hover uses `HOVER`. Dialogs and popovers get a soft shadow.
- **Accent:** a violet to cyan gradient (`ACCENT_FROM` to `ACCENT_TO`) is reserved for the
  primary action (Export, Create project), the active tool and focus rings. Solid
  `ACCENT` for selection outlines and active tabs. The playhead uses `PLAYHEAD`.
- **Clips:** colour by kind (`CLIP_*`). Body is the kind colour at ~30% alpha over
  `RAISED`, a 3px solid band on the left edge, a 1px lighter top highlight, rounded
  radius 6, label in `TEXT`. Selected clips get a 2px `ACCENT` outline plus glow.
- **Type:** UI text in Inter (fall back to the system sans), timecode and numbers in
  JetBrains Mono (bundled OFL fonts). Named rem sizes: xs 12 px, sm 14 px,
  base 15 px, lg 20 px at compact 100%; comfortable uses a 16 px base.
  Preferences → Interface: UI scale offers desktop default or 80–200% in 10% steps,
  saved separately from the project and applied when Save is pressed.
  `ANIMATHIO_UI_SCALE=150` overrides the saved percentage. Desktop default reads
  KDE forceFontDPI, Xresources Xft.dpi or GNOME text-scaling-factor, otherwise 100%.
  GPUI output scaling still applies independently. Windows reads the accessibility
  text-size slider (`TextScaleFactor`); Linux reads KDE forceFontDPI, Xresources
  Xft.dpi or GNOME text-scaling-factor. Component rem spacing, panel rows and
  timeline track heights follow UI scale; timeline time coordinates remain independent.
  The reference screenshot at [100%](../docs/screenshots/editor-scale-100.png) uses a private
  1920×1080 Xvfb display; 150% needs a proportionally larger display to leave room for the preview.
- **Spacing:** 4px grid. Panel padding 8–12. Control and panel row spacing follows the theme rem size.
- **Motion:** 120–180ms ease-out for hover, selection, tab and panel changes, popovers
  and toasts (gpui `with_animation`). Nothing animates during playback except the
  playhead and meters. Respect a "Reduce motion" preference if one exists.
- **States:** every interactive element has hover, active, focus and disabled styles.
  Empty states get an icon, one line of text and a primary action.
- **Timeline:** video/image clips show a filmstrip of low-resolution thumbnails
  (one tile per visible slot, decoded off-thread, cached by asset, time and row height); audio clips a filled,
  mirrored waveform in the clip colour; text, LaTeX and shape clips their snippet.
  Fades are smooth curves with handles, transitions a violet to cyan gradient over
  the overlap, keyframes rotated-square diamonds on the selected clip. Track headers
  carry a kind-colour stripe, a kind icon, compact mute/solo/lock pills (tinted when
  active) and a horizontal gain slider (drag to change, double-click for unity); tracks that
  hold only text, formulas, shapes or images show an eye (hide) toggle and the lock instead. The
  playhead is a `PLAYHEAD` pentagon head with a 1px glowing line and a timecode pill
  while scrubbing; the in/out range is shaded with the accent and an engaged snap
  draws an accent line with a magnet icon.


## Application frame and launcher

The dark gpui-component theme is installed from `style.rs` before the first
window opens. Its solid palette, semantic widget backgrounds, scrollbar and
resize-handle appearance are updated together using `Theme::update`. Inter
Variable and JetBrains Mono are embedded; their OFL licenses ship beside the font
sources under `apps/animathio/assets/fonts`. The restored original AniMathIO 1.x logo is
`apps/animathio/assets/brand.svg`: the white wordmark over cyan and brown panels
and a red semicircle. Its SVG wrapper embeds a 2000px PNG resampled from the
original 500px artwork, preserving its shapes, colours and proportions (the
resampling adds no source detail). Launcher, title bar and packaged icons share
this source; `apps/animathio/assets/gen-icons.py` regenerates the icon formats.
The violet-to-cyan UI action accent remains defined in `style.rs`.

The workspace wraps each resizable pane in a rounded panel with a consistent
icon/title/action header and 6px shared gutters. The library uses a sliding active
pill for Media, Elements, Titles, Effects and Plugins; below a 400 px dock the tabs are
icon-only with tooltips, which covers the default width (`LIBRARY_TABS_LABEL_WIDTH`). Each tab keeps its own
search; only Media has asset-kind filters and grid/list controls. Elements groups
text/formulas, geometry and graphs/grids, followed by additional shape sources and
third-party plugins. Plugins is an inventory of all loaded plugin kinds, versions,
enabled status and bundle origins, with a link to the plugin catalog. Arrow keys,
Home and End move selection and focus between tabs; Tab reaches search and cards. The title bar shows the project name, an
unsaved dot, MCP listener status and Export. The bottom chips report the canvas,
frame rate, duration, autosave policy, preview renderer actually in use and export queue.
The renderer chip names the backend the worker really uses: “GPU preview”, “CPU preview”, or “CPU preview (fallback)” when the GPU was requested but is unavailable. On the launcher the autosave and export-queue chips are hidden, as are the Playback, Tools and Clip menus and the title-bar Export button.

The launcher offers five canvas preset cards, a Templates section with cached CPU
thumbnails, aspect labels and user-template deletion, recent-project cards with
cached modification times and media drops. Open project, Import v1 and Import Manim are
pinned in the hero so they stay above the fold at 1280×720; the page scrolls (with a scrollbar)
for the rest.
Its quiet gradient glow runs only while the launcher is visible. Card lifts and
tab changes settle in 180ms. Preferences > Motion > Reduce motion makes the glow
static and disables decorative widget transitions through GPUI's motion policy;
the preference is saved separately from projects. Toasts retain the component
kit's slide-in/dismiss behavior and add a semantic coloured left border.

The Export button, dialog primaries and "Create" actions share `style::primary_button`
(violet-to-cyan gradient, dark lettering). The launcher hero adds a slow light sweep
over a static wash; Reduce motion keeps only the wash. Preset cards carry an aspect
chip, and the drop zone is dashed and lights up while files are dragged over it.

## Viewer, inspector and timeline follow-ups

- **Rotation** is shown and typed in degrees everywhere in the UI (inspector rows,
  numeric scrubbing, multi-selection, the gizmo pill), with one decimal and a degree suffix. Transforms and keyframes still
  store radians; conversion lives in `field_display_value` / `field_model_value`.
- **Click-to-pick:** clicking the preview selects the topmost visible clip under the
  pointer, Alt-click cycles through the stack, and a click on empty canvas clears the
  selection. Media clips hit anywhere inside their transformed bounds. Text, LaTeX,
  graph and shape clips have a canvas-sized box but draw little, so they are rendered
  alone at thumbnail size and only count when they have ink within a few pixels of the
  pointer. Handles of the selected clip keep priority for scale and rotate drags.
- **Compositing:** the model has no blend modes, so the inspector shows only the
  opacity row (section "Compositing"); a blend dropdown waits for core support.
- **Effect warnings:** a clip whose effect plugin is not loaded, or whose last preview
  frame failed inside an effect, gets an amber warning badge next to its kind icon in
  the timeline, a warning row (icon plus reason) in the inspector Effects section and a
  toast when the preview first fails.
- **Filmstrip:** one thumbnail per visible tile, requested per source time, decoded on
  a worker and kept in a 256-tile least-recently-used cache; the worker also keeps
  LRU caches of open decoders and stills and one shared copy of each encoded video. Fades are drawn by
  sampling the clip's actual fade keyframes, so linear, ease and hold fades differ.
- **Gizmo:** scale handles work on mirrored (negative-scale) clips and keep their sign;
  rotate drags are unwrapped so crossing the -x axis never jumps by a full turn.
- **Window controls:** the window is client-decorated and the gpui-component title bar
  supplies minimise, maximise/restore and close (theme hover colours; close hovers in
  `DANGER`). Minimise and maximise appear when the window manager advertises them;
  close is always shown. On X11 client decorations need a compositing window manager,
  so a bare Xvfb shows none. To test them there, run
  `scripts/virtual-display.sh` with a tiny helper that sets `_NET_WM_CM_OWNER`,
  `_NET_SUPPORTING_WM_CHECK` and `_GTK_FRAME_EXTENTS` on the root window.

The Titles tab uses CPU-rendered preset cards cached by canvas size. Double-click
adds at the playhead; dragging uses timeline snapping. The inspector presents all
linked composition text fields before the usual clip properties. Preset data lives
in `crates/control/presets/titles.json`; applying a composition is one command batch.

See [Keyboard shortcuts](keyboard-shortcuts.md) for the Premiere default keymap.

## Reproducing panel screenshots

The developer launch option `--screenshot-panel=markers` opens the same panel as
Clip > Markers and beat sync. `--screenshot-panel=duck` selects the first audio clip as the voice and opens Auto duck (the other
audio tracks are suggested as music, then the normal analysis runs), and scrolls the timeline to the bottom so the shaded music rows are visible. `--screenshot-panel=autocut` selects the first audio
clip in track order and runs the normal asynchronous analysis before showing its
review. `--screenshot-panel=record` opens the Media tab's recorder mid-take on a scripted tone (never a real input). `--screenshot-panel=media|titles|effects|elements` open those Library tabs and
`--screenshot-panel=math` opens the Add math object dialog (LaTeX). `--screenshot-panel=plugins` opens the Library's Plugins tab, and
`--screenshot-panel=plugins-browse` also opens Browse, which fetches the community index
only when the Community plugins switch is already on. Pass a project path as usual; none
of these options applies edits. The control protocol can create markers and propose cuts
but does not open these UI panels.

The capture fixture combines the shared visual sample with a synthetic voice
(tone, one-second pause, tone), a separate steady music bed and three named
markers. The voice scope keeps the music from masking the pause. These are
synthetic signals, not a speech-recognition demonstration.

Run from the repository root (the release build and plugin compilation can take
several minutes):

```bash
export CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0
nice -n 19 ionice -c3 cargo build -p animathio --release --locked --bin animathio --example screenshot_project
scripts/build-plugins.sh
./target/release/examples/screenshot_project target/screenshot-project.amio
mkdir --parents target/screenshot-config
export XDG_CONFIG_HOME="$PWD/target/screenshot-config"
export ANIMATHIO_PLUGIN_DIR="$PWD/target/plugins" ANIMATHIO_UI_SCALE=100 ANIMATHIO_AUDIO=none
scripts/virtual-display.sh --screenshot docs/screenshots/editor-markers.png 30 \
  ./target/release/animathio --screenshot-panel=markers target/screenshot-project.amio
scripts/virtual-display.sh --screenshot docs/screenshots/editor-autocut.png 30 \
  ./target/release/animathio --screenshot-panel=autocut target/screenshot-project.amio
scripts/virtual-display.sh --screenshot docs/screenshots/editor-auto-duck.png 30 \
  ./target/release/animathio --screenshot-panel=duck target/screenshot-project.amio
```

`scripts/virtual-display.sh --size WxH` sets the virtual screen (for example `--size 1280x720`
for the smallest supported window) and `--crop WxH+X+Y` keeps a rectangle of the capture:
at 1920×1080 a first-run window is the 90% restore size, so `--crop 1728x972+96+54` is the
window without the black margin (the `editor-*.png` shots). The window only appears after the
renderer and plugins have started and the root window is solid black until then, so a capture is
retried until the screen is no longer black; a run that never paints fails with a message
instead of writing a black PNG (this was the black launcher capture in #286: the delay was
shorter than the start-up). The script also parks the pointer over the title bar so no card
shows a hover state.

The private config directory avoids changing personal preferences. Captures use
1920×1080 Xvfb, silent audio and release binaries with first-party plugins. Inspect
all PNGs: markers must have named rows; autocut must have completed analysis and
show a proposed cut, and duck must show its preview summary, before capturing (increase the delay on slower machines).
