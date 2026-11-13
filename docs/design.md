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
  (decoded off-thread, cached by asset, time and row height); audio clips a filled,
  mirrored waveform in the clip colour; text, LaTeX and shape clips their snippet.
  Fades are smooth curves with handles, transitions a violet to cyan gradient over
  the overlap, keyframes rotated-square diamonds on the selected clip. Track headers
  carry a kind-colour stripe, a kind icon, compact mute/solo/lock pills (tinted when
  active) and a horizontal gain slider (drag to change, double-click for unity). The
  playhead is a `PLAYHEAD` pentagon head with a 1px glowing line and a timecode pill
  while scrubbing; the in/out range is shaded with the accent and an engaged snap
  draws an accent line with a magnet icon.
