# AniMathIO 2.0 visual design

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
  JetBrains Mono (both OFL; embed them if practical). Sizes: 11 (meta), 12 (body),
  13 (panel titles, semibold), 20+ (start-screen hero).
- **Spacing:** 4px grid. Panel padding 8–12. Controls 26px tall (compact 22).
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
  active) and a horizontal gain slider (drag to change, double-click for unity). The
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
pill for Assets, Plugins, Effects and Titles. The title bar shows the project name, an
unsaved dot, MCP listener status and Export. The bottom chips report the canvas,
frame rate, duration, autosave policy, requested preview renderer and export queue.
“GPU requested” describes the preview preference: the worker can fall back to CPU.

The launcher offers five canvas preset cards, a Templates section with cached CPU
thumbnails, aspect labels and user-template deletion, recent-project cards with
cached modification times, media drops and Open / Import v1 / Import Manim.
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
  numeric scrubbing, multi-selection, the gizmo pill). Transforms and keyframes still
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
  toast when the preview first fails ([screenshot](screenshots/editor-effect-warning.png)).
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
