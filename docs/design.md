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


## Application frame and launcher

The dark gpui-component theme is installed from `style.rs` before the first
window opens. Its solid palette, semantic widget backgrounds, scrollbar and
resize-handle appearance are updated together using `Theme::update`. Inter
Variable and JetBrains Mono are embedded; their OFL licenses ship beside the font
sources under `apps/animathio/assets/fonts`. The bundled gradient brand is
`apps/animathio/assets/brand.svg`.

The workspace wraps each resizable pane in a rounded panel with a consistent
icon/title/action header and 6px shared gutters. The library uses a sliding active
pill for Assets, Plugins and Effects. The title bar shows the project name, an
unsaved dot, MCP listener status and Export. The bottom chips report the canvas,
frame rate, duration, autosave policy, requested preview renderer and export queue.
“GPU requested” describes the preview preference: the worker can fall back to CPU.

The launcher offers four aspect-ratio canvas cards, recent-project cards with
cached modification times, media drops and Open / Import v1 / Import Manim.
Its quiet gradient glow runs only while the launcher is visible. Card lifts and
tab changes settle in 180ms. Preferences > Motion > Reduce motion makes the glow
static and disables decorative widget transitions through GPUI's motion policy;
the preference is saved separately from projects. Toasts retain the component
kit's slide-in/dismiss behavior and add a semantic coloured left border.
