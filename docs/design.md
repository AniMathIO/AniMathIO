# AniMathIO 2 design system

AniMathIO is a dense, professional editor: a media Library, a viewer, an inspector and
a multi-track timeline, plus math objects, captions, audio tools and plugins. This
document is the design system for that editor. It names every colour, size and
component the UI is allowed to use, and the words it uses.

**Status.** The 2.3 tokens are in code (the UI-rework foundation PR); components
migrate per surface (UI rework PRs 2-6). Surface modules that still name a retired
2.2 constant (the gradient endpoints, the yellow snap guide) read it as a deprecated
alias for the new token until their PR removes it. Take every value from
`apps/animathio/src/style/` (or from the gpui-component `Theme`, which
`style::apply_palette` fills), never from a literal; the `style_guard` test fails a
file that gains new ad-hoc colour or size literals, and each surface PR ratchets its
files down to zero.

## Direction

1. **The picture is the loudest thing on screen.** Surfaces are near-neutral greys, so
   the colour of footage and of the math being animated reads true. The chrome recedes;
   nothing in it competes with the viewer.
2. **One accent, taken from the logo.** The logo's cyan is the only accent: selection,
   focus, the active tool, the primary button. The logo's red is the playhead. The
   violet-to-cyan gradient is retired; brown stays in the logo.
3. **One decorative element: graph paper.** The fine grid behind the logo is the single
   ornament, used once, behind the launcher's heading. Everything else is plain.
4. **Structure is information.** A border means "different surface". A colour on a clip
   means its kind. A number in a step label means the steps are in order. If a device
   carries no information, it goes.
5. **The same thing looks the same everywhere.** One panel header, one tab strip, one
   inspector row, one floating tool panel, one dialog frame, one empty state.
6. **Everything fits at 1280×720.** Sizes are in rem so UI scale and density scale all of
   it; the smallest supported window shows every primary control without scrolling the
   window itself.

## Tokens

All tokens live in `apps/animathio/src/style/tokens.rs` (re-exported by
`apps/animathio/src/style/mod.rs` together with the component builders in
`style/components.rs`). Colours are `0xRRGGBB` constants read through `style::hex`;
`style::hexa(token, alpha)` exists for the deprecated period, new tints use
`style::tint(token, Alpha::..)` (see the alpha steps below). The gpui-component
`Theme` palette is filled from them in `style::install_theme`, so kit widgets and
custom elements agree.

### Colour

AniMathIO has one theme, dark. There is no light theme, and none is planned for 2.3;
keep colours behind tokens so one could be added as a second palette later.

**Surfaces and lines**

| Token | hex | Role |
|---|---|---|
| `BG` | `#0E0F11` | Window background, the gutter between panels, wells behind tab strips |
| `PANEL` | `#17181B` | Docked panel surface (Library, viewer frame, inspector, timeline) |
| `RAISED` | `#1F2024` | Inputs, cards, popovers, dialogs, menus, track lanes |
| `HOVER` | `#27292E` | Hovered row, card or ghost button |
| `PRESSED` | `#2F3237` | Pressed state, and the fill of an "on" ghost toggle without accent |
| `STAGE` | `#141416` | Pasteboard around the canvas; lighter than black so a black frame stays visible (`VIEWER_STAGE` reads it as a deprecated alias until PR 4) |
| `BORDER` | `#2C2E33` | Hairlines: panel outlines, dividers, input outlines |
| `BORDER_STRONG` | `#3C3F46` | Outline of overlays (dialogs, popovers, tool panels) and of secondary buttons |

The 2.2 palette was tinted blue (hue about 220, saturation 20–30%). These neutrals keep
saturation under 6%, the way grading and editing suites do, so the UI does not shift how
the footage looks.

**Text**

| Token | hex | Role |
|---|---|---|
| `TEXT` | `#E8E9EB` | Primary text, values, titles |
| `TEXT_SECONDARY` | `#A9ACB3` | Labels, inactive tabs, secondary values |
| `TEXT_MUTED` | `#8F939B` | Hints, captions, placeholder text |
| `TEXT_DISABLED` | `#6B6F77` | Disabled labels only (exempt from contrast minimums) |

Every text token except `TEXT_DISABLED` reaches WCAG AA (4.5:1) on `BG`, `PANEL`,
`RAISED` and `HOVER`; the `muted_text_meets_aa_contrast` test in `style/mod.rs`
enforces it and must keep covering every text token (it also checks `ON_ACCENT` on
`ACCENT`, and `TEXT` at 7:1 on every clip body).

**Accent and semantic colours**

| Token | hex | Role |
|---|---|---|
| `ACCENT` | `#0CC0DF` | Logo cyan. Primary button fill, focus ring, active tab and tool, selected list item border, links, slider fill, snapping guide |
| `ACCENT_HOVER` | `#3DD0EA` | Hovered primary button |
| `ACCENT_PRESSED` | `#09A3BE` | Pressed primary button |
| `ON_ACCENT` | `#0E0F11` | Text and icons on an accent fill |
| `SUCCESS` | `#3FB96F` | Completed jobs, success toasts |
| `WARNING` | `#F0B429` | Warnings, missing plugins, effect warning badges, mute |
| `DANGER` | `#F26B6F` | Errors, destructive actions, validation messages |
| `PLAYHEAD` | `#FF3131` | Logo red. Playhead, recording indicator; nothing else |
| `ACCENT_FROM`, `ACCENT_TO`, `SNAP` | retired | Deprecated aliases of `ACCENT` for files whose surface PR (4/5) has not merged yet: the gradient is gone (the primary action is solid `ACCENT`) and snapping draws `ACCENT` |

The logo colours `#0CC0DF` (cyan), `#8F4D30` (brown) and `#FF3131` (red) come from
`apps/animathio/assets/brand.svg`. Only cyan and red are UI tokens; brown appears only in
the logo.

**Alpha steps.** Tints use one of five named alphas, never an ad-hoc number:

| Step | Alpha | Use |
|---|---|---|
| wash | 0.08 | In/out range shading, drop-zone highlight, hover wash over thumbnails |
| subtle | 0.16 | Selected row, active tab pill, "on" toggle fill (`ACCENT`), selection fill |
| body | 0.32 | Clip body over `RAISED` |
| edge | 0.55 | Tinted borders of "on" toggles and pills |
| scrim | 0.78 | `BG` behind a modal dialog |

**Clip kinds.** Muted so a timeline full of clips stays calm, hue-separated from each
other, from the accent and from the playhead. Labels on the clip body reach 7:1 or more.

| Token | hex | Kind |
|---|---|---|
| `CLIP_VIDEO` | `#4C7BD9` | Video |
| `CLIP_IMAGE` | `#2E9E8F` | Image |
| `CLIP_AUDIO` | `#3FA866` | Audio |
| `CLIP_TEXT` | `#A15CC9` | Text, titles, captions |
| `CLIP_LATEX` | `#D35A8E` | LaTeX |
| `CLIP_SHAPE` | `#D69A2D` | Shapes |
| `CLIP_GRAPH` | `#8DB23A` | Graphs and grids (moved off cyan, which is now the accent) |
| `CLIP_PLUGIN` | `#6B7785` | Other plugin clips |
| `CLIP_ERROR` | `#F26B6F` | Clip whose source failed |

Marker colours stay the user's choice (`MarkerColor`); they are drawn as flags, never as
clip bodies.

### Type

Two families, both bundled under `apps/animathio/assets/fonts` with their OFL licences:

- **Inter Variable** for all UI text, including numbers in fields. Numeric fields and
  timecode-like values in proportional text use tabular figures (`.tabular()`,
  gpui `FontFeatures`) so digits do not jitter while scrubbing. gpui passes font
  features through on every platform (cosmic-text on Linux, DirectWrite on Windows,
  CoreText on macOS) and Inter Variable ships `tnum`, so `.tabular()` is a real
  setting, not a no-op.
- **JetBrains Mono** only where text is code or a clock: the transport timecode, the
  scrub pill, LaTeX and Manim source, JSON path fields. Never for labels.

The root size (`Theme::font_size`, the rem) is 15 px in Compact and 16 px in
Comfortable density, multiplied by UI scale. Every size below is in rem, so both settings
scale the whole UI. Pixel values are at Compact, 100%.

| Role | Size | px | Weight | Line height | Use |
|---|---|---|---|---|---|
| `CAPTION` | 0.733rem | 11 | 400 | 14 px | Ruler numbers, clip sub-labels, badges, keyboard hints, card meta |
| `LABEL` | 0.8rem | 12 | 400 (500 when active) | 16 px | Inspector and form labels, tab labels, chips, hints, status bar |
| `BODY` | 0.875rem | 13 | 400 | 18 px | Default text, list rows, menu items; the size kit inputs and buttons use (`text_sm`) |
| `HEADING` | 0.875rem | 13 | 600 | 18 px | Panel headers, inspector section headers, group titles in dialogs and the Library |
| `TITLE` | 1.067rem | 16 | 600 | 22 px | Dialog and tool panel titles |
| `DISPLAY` | 1.867rem | 28 | 600 | 34 px | Launcher heading only |
| `TIMECODE` | 0.933rem | 14 | 400 mono | 18 px | Transport timecode |

Rules: three weights only (400, 500, 600). No all-caps text and no letter-spaced labels.
Hierarchy comes from weight and colour (`TEXT` against `TEXT_SECONDARY`), not from more
sizes. In 2.2, `SM` (14 px) and the kit's 13 px control text sat side by side; 2.3 has
one `BODY` size.

### Spacing and density

Spacing uses gpui's quarter-rem steps (step 1 = 0.25rem, 3.75 px at Compact 100%), and
only these:

| Step | Helper | px | Use |
|---|---|---|---|
| 0.5 | `gap_0p5` | 2 | Icon to badge, stacked label and caption |
| 1 | `gap_1`, `p_1` | 4 | Icon to label inside a control, chip padding |
| 2 | `gap_2`, `px_2` | 8 | Between controls in a row, row side padding, panel inset |
| 3 | `gap_3`, `p_3` | 11 | Padding of cards, popovers and tool panels; between inspector sections |
| 4 | `gap_4`, `p_4` | 15 | Dialog padding, between form groups |
| 6 | `gap_6`, `p_6` | 23 | Launcher margins only |

The gutter between docked panels is `style::gutter()` (6 px × UI scale; the 2.2
`space::GUTTER` constant reads as a deprecated alias until PR 2 switches the shell
over).

Lengths are rem (gpui helpers such as `rems(..)`, or the `style::size` tokens), so
they follow UI scale.
Literal `px(..)` is allowed only for hairlines (1 px), selection and focus strokes (2 px),
the clip kind band (3 px) and canvas painting, where every length is multiplied by
`style::ui_scale()`.

**Heights.** Rows and controls come in five heights (`style::size`):

| Name | Size | px | Use |
|---|---|---|---|
| `ROW_XS` | 1.25rem | 19 | Timeline track header pills, badges, chips (`Size::XSmall`) |
| `ROW_SM` | 1.5rem | 23 | Inspector and panel controls, toolbar icon buttons (`Size::Small`) |
| `ROW` | 1.867rem | 28 | Inspector rows, inspector section headers, tab strips, list rows |
| `ROW_MD` | 2rem | 30 | Dialog controls and footer buttons (`Size::Medium`) |
| `HEADER` | 2.133rem | 32 | Docked panel headers, tool panel headers |

Panels use `Size::Small` controls; dialogs and the launcher use `Size::Medium`. Never mix
sizes within one row.

### Radii, borders and elevation

Radii (style::radius) shrink with depth, so nested shapes read as nested:

| Token | px | Use |
|---|---|---|
| `radius::OVERLAY` | 10 | Dialogs, popovers, menus, floating tool panels, toasts |
| `radius::PANEL` | 8 | Docked panels |
| `radius::CARD` | 6 | Library cards, preset thumbnails, launcher cards |
| `radius::CONTROL` | 4 | Buttons, inputs, selects, segmented controls, chips |
| `radius::CLIP` | 4 | Timeline clips |
| `radius::PILL` | full | Status chips, switches, the scrub timecode pill |

Borders: docked panels have a 1 px `BORDER` outline. Inside a panel, regions are
separated by a 1 px `BORDER` divider, not by boxes: no cards inside cards, and the
inspector is one list with dividers. Inputs have a 1 px `BORDER` outline on `RAISED`.
Overlays have a 1 px `BORDER_STRONG` outline.

Elevation: only overlays and the drag ghost cast a shadow (`shadow_lg`). Docked panels,
cards and clips are flat.

### Motion

120 ms (`motion::FAST_MS`) for hover and press, 180 ms (`motion::BASE_MS`) for tabs,
panels, popovers and toasts, ease-out. Motion answers an action; nothing moves by itself
except the playhead, meters and progress. The launcher's light sweep and glow are retired
with the gradient. Preferences > Motion > Reduce motion sets every duration to zero
through gpui's motion policy.

## Components

Each component has one implementation in `apps/animathio/src/style/components.rs`
(re-exported by the `style` module) and is used everywhere that kind of thing appears.
The rework (PRs 2-6) moves each surface onto them; until a surface migrates it keeps
its previous look, except for what the theme already restyles (palette, radii, primary
buttons).

### Window frame and status bar

- The title bar shows the logo mark, the project name, an unsaved dot, the MCP status
  and the Export button (primary, `Size::Small`). On the launcher, Export is hidden, not
  disabled.
- The status bar is a row of chips (`LABEL`, `TEXT_SECONDARY`, `RAISED` fill, pill
  radius, no border): canvas, frame rate, duration, autosave, renderer, export queue. A
  chip that needs attention turns `WARNING` with an icon; a chip that opens something
  has a hover state and a tooltip.

### Docked panels and panel headers

```
+-----------------------------------------------------------+
| [icon] Title              context          [act] [act] [x] |  HEADER, PANEL, BORDER below
+-----------------------------------------------------------+
| toolbar (optional): search, filters, view toggles          |  ROW, px_2
|-----------------------------------------------------------|
| content                                                    |
```

- Header: `HEADER` high, `PANEL` fill, 1 px `BORDER` below. Icon (Lucide, small,
  `TEXT_SECONDARY`), title in `HEADING`, optional context in `LABEL` `TEXT_SECONDARY`
  (sentence case: "Program", not "PROGRAM"), then at most three ghost icon buttons. More
  actions go in an overflow menu (`Ellipsis`).
- Every docked panel uses `style::panel` and `style::panel_header`. A panel that can be
  hidden has its hide button as the last header action.

### Tab strips and segmented controls

One segmented control serves the Library tabs, Installed/Browse in Plugins, Fit/Fill/
Stretch/1:1 in the inspector and any other choice of two to five short options.

- A `BG` well, `ROW` high, `radius::CONTROL`, padding 0.5 step. Options share the width.
- Active option: `ACCENT` subtle fill, `ACCENT` edge border, `TEXT`, weight 500. Inactive:
  `TEXT_SECONDARY`, `HOVER` fill on hover. The sliding pill animates in `BASE_MS`.
- Labels are one word where possible. When labels do not fit, show icons only, with a
  tooltip on each. Arrow keys, Home and End move between options.
- A heading under a tab strip never repeats the active tab's name.

### Inspector

The inspector is a single scrolling list of sections divided by hairlines, modelled on an
effect-controls list.

```
v [icon] Transform                                    [reset]   ROW, HEADING
  <> o  Position     [ X  960     px ] [ Y  540     px ]         ROW
  <> o  Scale        [ 100.0    % ] [link]
  <> o  Rotation     [ 0.0        ° ]
--------------------------------------------------------------  BORDER divider
> [icon] Compositing                                  [reset]
```

- **Section header:** `ROW` high, chevron (xsmall, `TEXT_MUTED`), icon (small,
  `TEXT_SECONDARY`), title in `HEADING`, reset as a ghost xsmall icon button at the right
  with the tooltip "Reset {section}". No box, no fill; a `BORDER` divider above every
  section but the first. The section icon and title name what the section edits: a shape
  clip's parameters are "Shape", a text clip's are "Text", a LaTeX clip's are "Formula".
- **Row:** `ROW` minimum height, `px_2`. Three columns: the keyframe column (fixed
  width, holds the diamond toggle and previous/next arrows; empty but present on rows that
  cannot be keyframed, so labels align), the label (`LABEL`, `TEXT_SECONDARY`, fixed 40%
  of the width, truncates with a tooltip) and the control (fills the rest).
- Pairs that belong together (X/Y, width/height, start/end) share one row with two
  half-width fields, each with a one-letter prefix inside the field.
- Units live inside the field as a suffix in `TEXT_MUTED`: `px`, `%`, `°`, `dB`, `s`,
  `fps`. A value is never shown without its unit.
- Dragging a numeric label scrubs the value (Shift coarse, Ctrl fine), as today. That is
  the only drag affordance: inspector number fields have no separate grip icon and no
  "−"/"+" stepper buttons (arrow keys step the focused field). A slider, when a parameter
  has one, sits in the same row as its field, never on a second line.
- Every clip kind uses this layout, including LaTeX and graph clips: their source,
  ranges and colours are a section like any other ("Formula", "Plot"), they commit on
  Enter or focus loss like every other field (no separate Apply button), and they do not
  draw a second preview, because the viewer already shows the clip.
- Parameters that only matter in one mode stay hidden until the mode is on (caption
  styling appears when a caption style is chosen).
- Help text for a section sits under its header in `LABEL` `TEXT_MUTED`, at most two
  lines.

### Fields, sliders, toggles and selects

- **Text and number inputs:** kit `Input`/`NumberInput`, `Size::Small` in panels,
  `Size::Medium` in dialogs. `RAISED` fill, `BORDER` outline, `ACCENT` focus ring.
  Placeholder in `TEXT_MUTED` says what to type ("Search media", "Paste LaTeX"), never
  "Enter value".
- **Sliders:** a slider is always paired with a numeric field (width 4rem) holding the
  exact value. Track `BORDER`, fill `ACCENT`, thumb `TEXT`.
- **Toggles:** a kit `Switch` for settings that apply immediately, a `Checkbox` inside
  forms that are submitted. Label to the left on the same row, control at the right edge;
  a one-line description may follow below in `LABEL` `TEXT_MUTED`. A label never sits
  above its toggle.
- **Selects:** kit `Select` with the current value as its label. Four or fewer short
  options use the segmented control instead. A setting with two states ("Reduce motion")
  is a switch, not a two-item select.
- **Colour:** a 1.5rem swatch (opens the picker) followed by the hex value field.
- **File paths:** the path input followed by a secondary "Browse…" button on the same
  row; an empty path shows the default it falls back to as placeholder text.

### Buttons

| Variant | Look | Use |
|---|---|---|
| Primary | `ACCENT` fill, `ON_ACCENT` text, weight 600; hover `ACCENT_HOVER`, pressed `ACCENT_PRESSED` | The one action a surface exists for: Export, Create, Apply, Install. At most one per dialog, tool panel or view |
| Secondary | `RAISED` fill, 1 px `BORDER_STRONG`, `TEXT`; hover `HOVER` | Cancel, Browse…, Add to queue, every other button with a label |
| Ghost | transparent, `TEXT_SECONDARY`; hover `HOVER` fill and `TEXT` | Icon buttons in headers, toolbars, rows and the transport |
| Destructive | Secondary with `DANGER` text; in a confirm dialog whose purpose is destruction, the primary button is `DANGER` filled with `ON_ACCENT` text | Remove, Delete, Discard changes |
| Link | `ACCENT` text, no fill | "Source", "Open Plugins", inline navigation |

- `style::primary_button` builds the primary variant (solid `ACCENT`, no gradient);
  `style::secondary_button` and `style::destructive_button(filled)` build the secondary
  and destructive looks; other variants come from the kit (`.ghost()`, `.outline()`)
  with the theme set from tokens. No other custom button styles. The kit draws keyboard
  focus as a 3 px ring in `ACCENT` at 50% **outside** the button's edge, so it stays
  visible around a solid `ACCENT` fill against the panel; the 2.3 foundation keeps that
  ring for every variant instead of a separate `TEXT` ring.
- Labels are a verb, or a verb and an object: "Export", "Add to queue", "Install
  plugin". A button that needs more input before it acts ends in "…" (U+2026).
- Order in a footer, right-aligned: secondary actions, then Cancel, then the primary
  action last (rightmost). A destructive confirm keeps the same order.
- **Icon buttons:** Lucide icon (`icons::icon`, small in `Size::Small` buttons), ghost,
  and always a tooltip naming the action, with the shortcut when one exists
  (`support::icon_button` adds it). A toggled-on icon button gets the `ACCENT` subtle fill
  and an `ACCENT` icon.

### Dialogs

```
+------------------------------------------------+
| [icon] Title                                    |  TITLE, p_4
| One line saying what this dialog does.          |  BODY, TEXT_SECONDARY (optional)
|------------------------------------------------|
| Label         [ control                      ]  |  label column 8.5rem
| Label         [ control           ] [Browse…]   |
| Label         [x] Option             (toggle)   |
|                                                 |  body scrolls; footer stays
|------------------------------------------------|
| ! Error text                [Cancel] [Primary]  |  footer, ROW_MD buttons
+------------------------------------------------+
```

- Built with the kit `Dialog` through `dialogs::open_dialog`, so validation errors show
  in the dialog. Three widths: small 440 px (confirmations), medium 560 px (forms:
  Export, Project settings, Preferences, Speed/Duration), large 760 px (workspaces such as
  the Math dialog and Generate subtitles). Height is at most the window height minus
  4rem; the body scrolls and the footer stays visible.
- Title: Lucide icon in `TEXT_SECONDARY` and the title in `TITLE`. The title names the
  thing or the action ("Export video", "Project settings"), never a slogan.
- Forms put labels in a left column (8.5rem, `LABEL`, `TEXT_SECONDARY`) and controls on
  the right, one setting per row, grouped under `HEADING` group titles with a `gap_4`
  between groups. Preferences has groups: General, Interface, Preview, Plugins, Updates.
- Errors appear next to the field they concern; an error that concerns the whole form
  appears at the left of the footer with an alert icon, in `DANGER`.
- Enter confirms and Escape cancels (as `dialogs::submit_footer` does today).

### Floating tool panels

Markers and beats, Automatic cuts, Auto duck, Scenes, Auto reframe and the export queue
are non-modal panels over the workspace. They share one frame:

- Anchored at the top left of the viewer area (`gap_3` inset), width 24rem, height at
  most the space above the timeline, so it never covers the clips it is working on.
  All of them open on the same side. `RAISED` fill, `BORDER_STRONG` outline,
  `radius::OVERLAY`, `shadow_lg`.
- Header as a panel header (`HEADER`), title in `TITLE`, close button (`X`) at the
  right. There is no separate "Close" button in the footer.
- The body scrolls with a visible scrollbar; the footer with the primary action
  (Apply, Add markers, Cut) is pinned and always visible.
- Steps are numbered only when they are done in order (Auto duck: 1 Voice, 2 Music,
  3 Preview).
- Toggle chips for "include this track" use a `Check` icon when on and no icon when off,
  never a minus sign.

### Toasts

Kit notifications through `style::Toast` (info, success, warning, error): `RAISED` fill,
`radius::OVERLAY`, a 3 px left stripe in the semantic colour, the message in `BODY`, at
most two lines, and at most one action button. The message names what happened using the
same verb as the action: the button "Export" ends in "Exported title.mp4". Info and
success toasts dismiss themselves; warnings and errors stay until closed.

### Empty states

An empty panel or list is centred in its space: a Lucide icon (1.5rem, `TEXT_MUTED`),
one line in `BODY` `TEXT_SECONDARY` that says what will be here, and one button that
adds it.

| Where | Text | Action |
|---|---|---|
| Media, no assets | "Imported media appears here." | Import media… |
| Inspector, nothing selected | "Select a clip to edit it." | none (the hint is the action) |
| Search with no results | "No effects match “blur”." | Clear search (ghost) |
| Effects, no plugin loaded | "No effect plugins are loaded." | Open Plugins (link) |
| Timeline, no clips | "Drag media or an element here." | none |

`style::empty_state` builds it (icon, line, optional action); `support::empty_state`
delegates to it for the text-only calls that have not migrated yet (PR 2 switches
them).

### Library cards and lists

- Grids compute their column count from the panel width (minimum card width 7rem) and
  never clip a card at the panel edge.
- Grid cards: thumbnail at the asset's aspect inside a 16:9 frame (`radius::CARD`), name
  in `LABEL` `TEXT` on one line with middle truncation, meta (duration, size, kind) in
  `CAPTION` `TEXT_MUTED`. Hover: `HOVER` fill. Selected: `ACCENT` subtle fill and a 1 px
  `ACCENT` border.
- List rows: `ROW` high, kind icon, name, meta at the right.
- Media cards show the kind as a small kind icon over the thumbnail, not as a coloured
  word; audio thumbnails draw the waveform in `CLIP_AUDIO`.
- Plugin cards, first-party and community alike: icon, name, version as a caption, origin
  ("Built in" or "Community"), and one state control, the `Switch`. State is never shown
  twice.
- Group headings inside a tab use `HEADING` and a `gap_3` above.

### Timeline

- **Track headers:** one row, vertically centred at every track height (name and
  controls side by side, never name at the top and controls at the bottom): a 3 px kind
  stripe at the left, the kind icon, the name in `BODY`,
  then `ROW_XS` pills for mute (tint `WARNING`), solo (tint `ACCENT`) and lock (tint
  `TEXT_SECONDARY`), using the edge and subtle alpha steps when on. Visual-only tracks
  show hide and lock. The gain slider follows the same slider rule as the inspector.
- **Ruler:** numbers in `CAPTION` with tabular figures. The overview strip above it
  has no text label. Marker flags and their names sit
  in the marker lane under the ruler and never overprint tick labels; a name that does not
  fit before the next marker is truncated.
- **Clips:** body is the kind colour at the body alpha over `RAISED`, a 3 px band in the
  kind colour at the left, `radius::CLIP`, the name in `CAPTION` weight 500 `TEXT`, and
  the content preview (filmstrip, waveform, snippet) below it. The duration follows the
  name in `TEXT_SECONDARY` only when the clip is wide enough for both; it is never
  allowed to push the name out. Math clips use their kind's colour: graphs are
  `CLIP_GRAPH`, not the LaTeX colour. Selected: a 2 px `TEXT`
  outline, which reads on every kind colour (the accent would vanish on some). Disabled
  or hidden: 50% opacity.
- **Keyframes:** 8 px diamonds on the selected clip only, `TEXT` fill with a 1 px `BG`
  outline; a selected keyframe is `ACCENT`.
- **Playhead:** `PLAYHEAD` head and 1 px line; while scrubbing a pill shows the
  timecode in JetBrains Mono.
- **In/out range** shades with `ACCENT` at the wash step and draws `ACCENT` edges.
  **Snapping** draws a 1 px `ACCENT` line and the magnet icon.
- Fades are curves with handles, transitions a hatched overlap in `TEXT` at the wash step,
  effect warnings an amber badge next to the kind icon.

### Viewer

The canvas sits centred on `STAGE`. Safe areas, the gizmo and the zoom control use
`TEXT` at reduced alpha and `ACCENT` for the active handle; they never use clip colours.
The transport row under the viewer: ghost icon buttons, the timecode in `TIMECODE`, and
the zoom select at the right.

### Launcher

The launcher is a start page, not a landing page.

- Heading in `DISPLAY` ("Start a project"), one line of `BODY` `TEXT_SECONDARY` under it,
  and the three entry actions (New project as primary, Open project…, Import…) visible
  without scrolling at 1280×720. The graph-paper grid from the logo is drawn faintly
  (`BORDER` at the wash step) behind this heading block and nowhere else.
- Canvas presets, templates and recent projects follow as card rows with `HEADING`
  titles. The page scrolls with a visible scrollbar.

## Interaction states

| Element | Hover | Pressed | Selected / on | Keyboard focus | Disabled |
|---|---|---|---|---|---|
| Ghost button | `HOVER` fill, `TEXT` | `PRESSED` fill | `ACCENT` subtle fill, `ACCENT` icon | 2 px `ACCENT` ring | `TEXT_DISABLED`, no hover |
| Secondary button | `HOVER` fill | `PRESSED` fill | n/a | 2 px `ACCENT` ring | `TEXT_DISABLED`, `BORDER` outline |
| Primary button | `ACCENT_HOVER` | `ACCENT_PRESSED` | n/a | the kit's outer 3 px `ACCENT` ring at 50%, visible around the fill | `RAISED` fill, `TEXT_DISABLED` text |
| List row, card | `HOVER` fill | `PRESSED` fill | `ACCENT` subtle fill, 1 px `ACCENT` border | 2 px `ACCENT` ring | 50% opacity |
| Tab option | `HOVER` fill | `PRESSED` fill | sliding `ACCENT` pill | 2 px `ACCENT` ring | `TEXT_DISABLED` |
| Input | `BORDER_STRONG` outline | n/a | n/a | `ACCENT` outline and ring | `BG` fill, `TEXT_DISABLED` |
| Clip | lighter body (+0.08 alpha) | n/a | 2 px `TEXT` outline | n/a | 50% opacity |

A disabled control keeps its place and has a tooltip that says why ("Select a clip to
add an effect"). Never hide a control only because it is disabled, except on the
launcher, where editor-only controls are not shown at all.

## Icons

Lucide icons from gpui-component (`Icon`, `IconName`, wrapped by
`apps/animathio/src/icons.rs`) and the bundled logo. No emoji and no Unicode symbols as
icons; `apps/animathio/src/glyph_guard.rs` fails the build on them. Icon sizes: xsmall
in `ROW_XS` controls, small in panels, medium in dialog titles and empty states. An icon
next to text uses the text's colour, or `TEXT_SECONDARY` when the text is `TEXT`.

## Writing

Words are part of the interface. They name things by what the user does with them.

- **Sentence case** everywhere: menus, buttons, tooltips, headers, tabs, dialog titles
  ("Open recent", "Rate stretch", "Rolling edit"). Proper nouns keep their case (LaTeX,
  Manim, AniMathIO, MP4).
- **No all caps,** no slogans and no marketing lines. The launcher says what the page
  is for; it does not say "Make ideas move".
- **One name per action through a flow.** The menu item, the dialog title, the primary
  button and the toast use the same verb: Export… opens "Export video", whose button is
  "Export" and whose toast is "Exported clip.mp4". Install → "Installed Code Snippets
  0.1.0". Add to queue → "Added to export queue".
- **Ellipsis** is the single character "…": on commands that open a dialog or picker
  before acting ("Import media…"), and on progress text ("Analysing audio…"). Never three
  dots.
- **Spelling** follows the docs: British English ("colour", "analysing", "cancelling").
  Identifiers and file formats are not translated.
- **Errors** say what happened and what to do, in one or two sentences, without
  apologising and without "Error:" in front: "Could not read song.mp3: the file is not
  an audio format AniMathIO can open. Convert it to WAV, MP3, FLAC or Ogg and import it
  again." Name the file, the clip or the field involved.
- **Empty states** say what appears here and how to add it (see Empty states).
- **Numbers** carry their unit, with a space except for percentages and degrees:
  "12 px", "-6.0 dB", "1.5 s", "30 fps", "100%", "45.0°". Use the fewest decimals that
  the control can change (rotation one, gain one, scale one).
- **No raw identifiers as text.** Show "Rectangle", "Normalised", "DejaVu Sans", "Off",
  not `rect`, `normalized`, `dejavu`, `off`; show "Graph", not `org.animathio.graph`. Plugin
  ids, file names and ABI versions go in a details disclosure or a tooltip.
- **State is not part of a button label.** "Keep short pauses: on" and "Sync to: beats"
  become a label with a switch or a segmented control next to it. A button's label is
  what it does.
- **No dot-joined status strings** such as "Third-party · sandboxed · not audited" or
  "v1.2.0 · Enabled". Write a sentence ("Community plugins run sandboxed and are not
  reviewed."), or give each value its own place (version as a caption, state as the
  switch).
- **Tooltips** name the action and, when it has one, the shortcut is added by the kit.
  They do not end with a full stop.

## Implemented behaviour notes

These notes record how existing UI behaves; the rework keeps the behaviour and changes
only its look.

- **Rotation** is shown and typed in degrees everywhere in the UI (inspector rows,
  numeric scrubbing, multi-selection, the gizmo pill), with one decimal and a degree
  suffix. Transforms and keyframes still store radians; conversion lives in
  `field_display_value` / `field_model_value`.
- **UI scale:** Preferences > Interface offers desktop default or 80–200% in 10% steps,
  saved separately from the project and applied when Save is pressed.
  `ANIMATHIO_UI_SCALE=150` overrides the saved percentage. Desktop default reads KDE
  forceFontDPI, Xresources Xft.dpi or GNOME text-scaling-factor on Linux and the
  accessibility text-size slider (`TextScaleFactor`) on Windows, otherwise 100%. GPUI
  output scaling still applies independently. Component rem spacing, panel rows and
  timeline track heights follow UI scale; timeline time coordinates do not. The
  reference screenshot at [100%](../docs/screenshots/editor-scale-100.png) uses a private
  1920×1080 Xvfb display.
- **Library:** tabs Media, Elements, Titles, Effects and Plugins; below a 400 px dock the
  tabs are icon-only with tooltips (`LIBRARY_TABS_LABEL_WIDTH`). Each tab keeps its own
  search; only Media has kind filters and grid/list controls. Arrow keys, Home and End
  move between tabs; Tab reaches search and cards.
- **Click-to-pick:** clicking the preview selects the topmost visible clip under the
  pointer, Alt-click cycles through the stack, and a click on empty canvas clears the
  selection. Text, LaTeX, graph and shape clips only count where they have ink near the
  pointer. Handles of the selected clip keep priority for scale and rotate drags.
- **Compositing:** the model has no blend modes, so the inspector shows only the
  opacity row; a blend control waits for core support.
- **Effect warnings:** a clip whose effect plugin is not loaded, or whose last preview
  frame failed inside an effect, gets a warning badge in the timeline, a warning row in
  the inspector Effects section and a toast when the preview first fails.
- **Filmstrip:** one thumbnail per visible tile, decoded on a worker and kept in a
  256-tile least-recently-used cache. Fades are drawn by sampling the clip's actual fade
  keyframes, so linear, ease and hold fades differ.
- **Gizmo:** scale handles work on mirrored clips and keep their sign; rotate drags are
  unwrapped so crossing the -x axis never jumps by a full turn.
- **Window controls:** the window is client-decorated and the gpui-component title bar
  supplies minimise, maximise/restore and close (close hovers in `DANGER`). On X11, client
  decorations need a compositing window manager, so a bare Xvfb shows none; to test them
  there, run `scripts/virtual-display.sh` with a helper that sets `_NET_WM_CM_OWNER`,
  `_NET_SUPPORTING_WM_CHECK` and `_GTK_FRAME_EXTENTS` on the root window.
- **Renderer chip:** names the backend the worker really uses: "GPU preview", "CPU
  preview", or "CPU preview (fallback)" when the GPU was requested but is unavailable.
- **Titles:** CPU-rendered preset cards cached by canvas size; double-click adds at the
  playhead. Preset data lives in `crates/control/presets/titles.json`; applying a
  composition is one command batch.
- **Logo:** the restored original AniMathIO 1.x logo is
  `apps/animathio/assets/brand.svg` (an SVG wrapper around a 2000 px PNG resampled from
  the 500 px original). Launcher, title bar and packaged icons share it;
  `apps/animathio/assets/gen-icons.py` regenerates the icon formats.

See [Keyboard shortcuts](keyboard-shortcuts.md) for the Premiere default keymap.

## Checking UI changes

- Every UI change is checked at 1280×720 and 1920×1080 on a private Xvfb display with
  `scripts/virtual-display.sh` (`--size`, `--crop`), never on a real desktop and never with
  audio. Look at every capture before committing.
- Behaviour is covered by the headless UI harness in `apps/animathio/src/editor/ui_tests/`
  (plain `cargo test --workspace`); a visual change must keep it green and should add a
  scenario when it moves or renames a control.


## Reproducing panel screenshots

The foundation PR (UI rework PR 1) also added selection and dialog hooks for
captures: `--screenshot-panel=select-<kind>` (kinds `image`, `video`, `audio`,
`text`, `shapes`, `latex`, `graph`) selects the first clip of that kind (audio
also scrolls the timeline to the bottom), so inspector and timeline shots show a
real selection; `--screenshot-panel=export`, `preferences` and `settings` open
the Export, Preferences and Project settings dialogs on the next turn (add a
LaTeX and a graph clip first for representative inspector captures, as in the
recipe below).

The developer launch option `--screenshot-panel=markers` opens the same panel as
Clip > Markers and beat sync. `--screenshot-panel=duck` selects the first audio clip as the voice and opens Auto duck (the other
audio tracks are suggested as music, then the normal analysis runs), and scrolls the timeline to the bottom so the shaded music rows are visible. `--screenshot-panel=autocut` selects the first audio
clip in track order and runs the normal asynchronous analysis before showing its
review. `--screenshot-panel=record` opens the Media tab's recorder mid-take on a scripted tone (never a real input). `--screenshot-panel=media|titles|effects|elements` open those Library tabs and
`--screenshot-panel=math` opens the Add math object dialog (LaTeX). Unknown
panel names are listed (with the selection and dialog hooks) on stderr. `--screenshot-panel=plugins` opens the Library's Plugins tab, and
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
