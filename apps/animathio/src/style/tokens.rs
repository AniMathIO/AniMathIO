//! The 2.3 design tokens (see `docs/design.md`): colours, radii, spacing, row
//! heights, type roles and dialog widths.
//!
//! Every length is rem-based (or multiplied by [`super::ui_scale`] for canvas
//! work), so UI scale and density scale the whole interface. Values are the
//! single source the theme palette (`super::apply_palette`) and every component
//! helper (see [`super::components`]) are built from.

use gpui::{FontFeatures, FontWeight, Hsla, Rems, Styled};

/// Colour tokens as `0xRRGGBB` (dark theme only; see `docs/design.md`).
pub mod color {
    // Surfaces and lines (neutral greys; saturation under 6%).
    /// Window background, the gutter between panels, wells behind tab strips.
    pub const BG: u32 = 0x0E0F11;
    /// Docked panel surface (Library, viewer frame, inspector, timeline).
    pub const PANEL: u32 = 0x17181B;
    /// Inputs, cards, popovers, dialogs, menus, track lanes.
    pub const RAISED: u32 = 0x1F2024;
    /// Hovered row, card or ghost button.
    pub const HOVER: u32 = 0x27292E;
    /// Pressed state, and the fill of an "on" ghost toggle without accent.
    pub const PRESSED: u32 = 0x2F3237;
    /// Pasteboard around the canvas; lighter than black so a black frame stays
    /// visible.
    pub const STAGE: u32 = 0x141416;
    /// Hairlines: panel outlines, dividers, input outlines.
    pub const BORDER: u32 = 0x2C2E33;
    /// Outline of overlays (dialogs, popovers, tool panels) and of secondary
    /// buttons.
    pub const BORDER_STRONG: u32 = 0x3C3F46;

    // Text.
    /// Primary text, values, titles.
    pub const TEXT: u32 = 0xE8E9EB;
    /// Labels, inactive tabs, secondary values.
    pub const TEXT_SECONDARY: u32 = 0xA9ACB3;
    /// Hints, captions, placeholder text (see `muted_text_meets_aa_contrast`).
    pub const TEXT_MUTED: u32 = 0x8F939B;
    /// Disabled labels only (exempt from the contrast minimums).
    pub const TEXT_DISABLED: u32 = 0x6B6F77;

    // Accent (the logo cyan) and semantic colours.
    /// Primary button fill, focus ring, active tab and tool, links, slider fill,
    /// snapping guide.
    pub const ACCENT: u32 = 0x0CC0DF;
    /// Hovered primary button.
    pub const ACCENT_HOVER: u32 = 0x3DD0EA;
    /// Pressed primary button.
    pub const ACCENT_PRESSED: u32 = 0x09A3BE;
    /// Text and icons on an accent fill.
    pub const ON_ACCENT: u32 = 0x0E0F11;
    /// Completed jobs, success toasts.
    pub const SUCCESS: u32 = 0x3FB96F;
    /// Warnings, missing plugins, effect warning badges, mute.
    pub const WARNING: u32 = 0xF0B429;
    /// Errors, destructive actions, validation messages.
    pub const DANGER: u32 = 0xF26B6F;
    /// Logo red: playhead and recording indicator, nothing else.
    pub const PLAYHEAD: u32 = 0xFF3131;

    /// Clip colours by kind (muted, hue-separated from each other, the accent
    /// and the playhead; labels on a clip body reach 7:1, see the contrast tests).
    pub const CLIP_VIDEO: u32 = 0x4C7BD9;
    pub const CLIP_IMAGE: u32 = 0x2E9E8F;
    pub const CLIP_AUDIO: u32 = 0x3FA866;
    pub const CLIP_TEXT: u32 = 0xA15CC9;
    pub const CLIP_LATEX: u32 = 0xD35A8E;
    pub const CLIP_SHAPE: u32 = 0xD69A2D;
    pub const CLIP_GRAPH: u32 = 0x8DB23A;
    pub const CLIP_PLUGIN: u32 = 0x6B7785;
    pub const CLIP_ERROR: u32 = 0xF26B6F;

    /// Shadow cast by the viewer canvas onto the stage (always black).
    pub const VIEWER_SHADOW: u32 = 0x000000;

    // Retired (2.2) tokens, kept as aliases so surface PRs still compile.
    #[deprecated = "the viewer stage is color::STAGE (2.3)"]
    pub const VIEWER_STAGE: u32 = STAGE;
    #[deprecated = "the violet-to-cyan gradient is retired; use color::ACCENT"]
    pub const ACCENT_FROM: u32 = ACCENT;
    #[deprecated = "the violet-to-cyan gradient is retired; use color::ACCENT"]
    pub const ACCENT_TO: u32 = ACCENT;
    #[deprecated = "snapping uses color::ACCENT"]
    pub const SNAP: u32 = ACCENT;
}

/// Corner radii in pixels; they shrink with depth (overlay > panel > card >
/// control) so nested shapes read as nested.
pub mod radius {
    /// Dialogs, popovers, menus, floating tool panels, toasts.
    pub const OVERLAY: f32 = 10.0;
    /// Docked panels.
    pub const PANEL: f32 = 8.0;
    /// Library cards, preset thumbnails, launcher cards.
    pub const CARD: f32 = 6.0;
    /// Buttons, inputs, selects, segmented controls, chips.
    pub const CONTROL: f32 = 4.0;
    /// Timeline clips.
    pub const CLIP: f32 = 4.0;
    /// Status chips, switches, the scrub timecode pill.
    pub const PILL: f32 = 999.0;
}

/// Spacing (4px grid, quarter-rem steps at Compact).
pub mod space {
    /// Panel inset between docked panels (2.2 value; prefer [`super::gutter`],
    /// which scales with UI scale).
    #[deprecated = "use style::gutter(), which scales with UI scale"]
    pub const GUTTER: f32 = 6.0;
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
}

/// Row and control heights in rems (see `docs/design.md` "Heights").
pub mod size {
    use gpui::Rems;
    /// Timeline track header pills, badges, chips (`Size::XSmall`).
    pub const ROW_XS: Rems = Rems(1.25);
    /// Inspector and panel controls, toolbar icon buttons (`Size::Small`).
    pub const ROW_SM: Rems = Rems(1.5);
    /// Inspector rows, inspector section headers, tab strips, list rows.
    pub const ROW: Rems = Rems(1.867);
    /// Dialog controls and footer buttons (`Size::Medium`).
    pub const ROW_MD: Rems = Rems(2.0);
    /// Docked panel headers, tool panel headers.
    pub const HEADER: Rems = Rems(2.133);
}

/// Animation durations in milliseconds.
pub mod motion {
    pub const FAST_MS: u64 = 120;
    pub const BASE_MS: u64 = 180;
}

/// Dialog widths in pixels (see `docs/design.md` "Dialogs").
pub mod dialog_width {
    /// Confirmations (replace project, overwrite file).
    pub const SMALL: f32 = 440.0;
    /// Forms: Export, Project settings, Preferences, Speed/Duration.
    pub const MEDIUM: f32 = 560.0;
    /// Workspaces: the Math dialog, Generate subtitles.
    pub const LARGE: f32 = 760.0;

    /// A dialog width scaled by UI scale.
    pub fn of(width: f32) -> gpui::Pixels {
        gpui::px(width * crate::style::ui_scale())
    }
}

/// The named alpha steps (see `docs/design.md` "Alpha steps"). Tints use one of
/// these, never an ad-hoc number.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Alpha {
    /// In/out range shading, drop-zone highlight, hover wash over thumbnails.
    Wash,
    /// Selected row, active tab pill, "on" toggle fill, selection fill.
    Subtle,
    /// Clip body over `RAISED`.
    Body,
    /// Tinted borders of "on" toggles and pills.
    Edge,
    /// `BG` behind a modal dialog.
    Scrim,
}

impl Alpha {
    pub fn value(self) -> f32 {
        match self {
            Alpha::Wash => 0.08,
            Alpha::Subtle => 0.16,
            Alpha::Body => 0.32,
            Alpha::Edge => 0.55,
            Alpha::Scrim => 0.78,
        }
    }
}

/// A `0xRRGGBB` token at one of the named alpha steps.
pub fn tint(token: u32, alpha: Alpha) -> Hsla {
    super::hexa(token, alpha.value())
}

/// Type sizes in rems (see `docs/design.md` "Type"). Hierarchy comes from
/// weight and colour, not from more sizes.
pub mod type_size {
    use gpui::Rems;
    /// Ruler numbers, clip sub-labels, badges, keyboard hints, card meta (11px).
    pub const CAPTION: Rems = Rems(0.733);
    /// Inspector and form labels, tab labels, chips, hints, status bar (12px).
    pub const LABEL: Rems = Rems(0.8);
    /// Default text, list rows, menu items; the size kit controls use (13px).
    pub const BODY: Rems = Rems(0.875);
    /// Dialog and tool panel titles (16px).
    pub const TITLE: Rems = Rems(1.067);
    /// Launcher heading only (28px).
    pub const DISPLAY: Rems = Rems(1.867);
    /// Transport timecode (14px, mono).
    pub const TIMECODE: Rems = Rems(0.933);

    // 2.2 names, kept as aliases so surface PRs still compile.
    #[deprecated = "use type_size::LABEL (same value)"]
    pub const XS: Rems = LABEL;
    #[deprecated = "use type_size::BODY (was 0.933)"]
    pub const SM: Rems = BODY;
    #[deprecated = "use type_size::TITLE (was 1.333)"]
    pub const LG: Rems = TITLE;
    #[deprecated = "use a Role (see docs/design.md Type); 1rem has no role"]
    pub const BASE: Rems = Rems(1.0);
}

/// A text role: size, weight, line height and default colour in one value (see
/// `docs/design.md` "Type").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// 0.733rem, 400, `TEXT_MUTED`. Ruler numbers, badges, card meta.
    Caption,
    /// 0.8rem, 400 (500 when active), `TEXT_SECONDARY`. Labels, chips, hints.
    Label,
    /// 0.875rem, 400, `TEXT`. Default text and list rows.
    Body,
    /// 0.875rem at 600, `TEXT`. Panel headers, section and group titles.
    Heading,
    /// 1.067rem, 600, `TEXT`. Dialog and tool panel titles.
    Title,
    /// 1.867rem, 600, `TEXT`. Launcher heading only.
    Display,
}

impl Role {
    pub fn size(self) -> Rems {
        match self {
            Role::Caption => type_size::CAPTION,
            Role::Label => type_size::LABEL,
            Role::Body | Role::Heading => type_size::BODY,
            Role::Title => type_size::TITLE,
            Role::Display => type_size::DISPLAY,
        }
    }

    pub fn weight(self) -> FontWeight {
        match self {
            Role::Caption | Role::Label | Role::Body => FontWeight(400.),
            Role::Heading | Role::Title | Role::Display => FontWeight(600.),
        }
    }

    pub fn line_height(self) -> Rems {
        match self {
            Role::Caption => Rems(0.933),
            Role::Label => Rems(1.067),
            Role::Body | Role::Heading => Rems(1.2),
            Role::Title => Rems(1.467),
            Role::Display => Rems(2.267),
        }
    }

    pub fn color(self) -> Hsla {
        super::hex(match self {
            Role::Caption => color::TEXT_MUTED,
            Role::Label => color::TEXT_SECONDARY,
            Role::Body | Role::Heading | Role::Title | Role::Display => color::TEXT,
        })
    }
}

/// Extensions on [`Styled`] for the type roles.
///
/// `.tabular()` switches digits to fixed-width (OpenType `tnum`) so values do
/// not jitter while scrubbing. gpui passes font features to cosmic-text on
/// Linux (and to DirectWrite/CoreText elsewhere), and the bundled Inter
/// Variable ships a `tnum` feature, so it works with the default family; it
/// leaves family and weight alone, so it composes with `.role()` and with the
/// mono timecode font in any order.
pub trait TextRole: Styled + Sized {
    /// Size, weight (400/600), line height and default colour for a [`Role`].
    ///
    /// Named `text_role` (not `role`): gpui's own `Div` carries an inherent
    /// accessibility `.role()` (a different, unrelated `gpui::Role`). Import
    /// this enum qualified (as `style::Role`) where a file also globs `gpui::*`.
    fn text_role(self, role: Role) -> Self {
        self.text_size(role.size())
            .line_height(role.line_height())
            .font_weight(role.weight())
            .text_color(role.color())
    }

    /// Tabular figures (`tnum`) for numbers and timecode-like values.
    fn tabular(self) -> Self {
        self.font_features(FontFeatures(std::sync::Arc::new(vec![("tnum".into(), 1)])))
    }
}
impl<T: Styled> TextRole for T {}
