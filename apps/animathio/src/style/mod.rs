//! Visual design tokens for the editor (see `docs/design.md`).
//!
//! UI code takes colours, radii and spacing from [`tokens`] (or from the
//! gpui-component `Theme`, which [`apply_palette`] derives from them) and its
//! components from [`components`] instead of hard-coding values.
//!
//! - [`tokens`]: the 2.3 colour, radius, spacing, row-height, type and dialog
//!   tokens (including the [`tokens::Role`] text roles).
//! - [`components`]: one implementation of each shared component (buttons,
//!   panel headers, the segmented control, the tool panel, form rows, the
//!   empty state, chips, icon toggles, toasts).
#![allow(dead_code)]

pub mod components;
pub mod tokens;

// The full token/component surface is re-exported: surface PRs (UI rework
// 2-6) take their helpers from `style::`, so some re-exports (and the
// components themselves) stay unused until their PR adopts them.
#[allow(unused_imports)]
pub use components::{
    SegmentedOption, Toast, ToolPanel, chip, destructive_button, empty_state, form_group, form_row,
    icon_toggle, panel, panel_header, panel_header_full, primary_button, secondary_button,
    segmented, switch_row,
};
#[allow(unused_imports)]
pub use tokens::{
    Alpha, Role, TextRole, color, dialog_width, motion, radius, size, space, tint, type_size,
};

/// An opaque colour from a `0xRRGGBB` token.
pub fn hex(token: u32) -> gpui::Hsla {
    gpui::rgb(token).into()
}

/// A colour from a `0xRRGGBB` token with the given alpha (0-1).
pub fn hexa(token: u32, alpha: f32) -> gpui::Hsla {
    let mut color: gpui::Hsla = gpui::rgb(token).into();
    color.a = alpha.clamp(0.0, 1.0);
    color
}

/// The gutter between docked panels: 6px scaled by UI scale.
pub fn gutter() -> gpui::Pixels {
    gpui::px(6. * ui_scale())
}

/// Scale shared by canvas geometry and the component theme.
static UI_SCALE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1.0f32.to_bits());
pub fn ui_scale() -> f32 {
    f32::from_bits(UI_SCALE.load(std::sync::atomic::Ordering::Relaxed))
}
pub fn set_ui_scale(scale: f32) {
    UI_SCALE.store(scale.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// Compose the bundled brand with the component kit's complete Lucide catalog.
pub struct Assets;
impl gpui::AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        if path == "brand.svg" {
            return Ok(Some(std::borrow::Cow::Borrowed(include_bytes!(
                "../../assets/brand.svg"
            ))));
        }
        gpui::AssetSource::load(&gpui_kit_assets::AllAssets, path)
    }
    fn list(&self, path: &str) -> gpui::Result<Vec<gpui::SharedString>> {
        let mut paths = gpui::AssetSource::list(&gpui_kit_assets::AllAssets, path)?;
        if "brand.svg".starts_with(path) {
            paths.push("brand.svg".into());
        }
        Ok(paths)
    }
}

/// Install the palette, bundled fonts and scaled base size before creating widgets.
pub fn install_theme(cx: &mut gpui::App, compact: bool, reduce_motion: bool) {
    use gpui::px;
    use gpui_component::{Theme, ThemeMode};
    use std::borrow::Cow;
    set_ui_scale(crate::preferences::Preferences::load().effective_scale());
    let fonts = vec![
        Cow::Borrowed(include_bytes!("../../assets/fonts/InterVariable.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf").as_slice()),
    ];
    let fonts_loaded = cx.text_system().add_fonts(fonts).is_ok();
    cx.set_reduce_motion(reduce_motion);
    Theme::change(ThemeMode::Dark, None, cx);
    Theme::update(cx, |theme| {
        apply_palette(&mut theme.colors);
        if fonts_loaded {
            theme.font_family = "Inter Variable".into();
            theme.mono_font_family = "JetBrains Mono".into();
        }
        theme.font_size = px((if compact { 15. } else { 16. }) * ui_scale());
        theme.radius = px(radius::CONTROL);
        theme.radius_lg = px(radius::PANEL);
        theme.shadow = true;
        // Thin scrollbars that stay visible, so a panel that overflows says so.
        theme.scrollbar_mode = gpui_component::scroll::ScrollbarMode::Always;
        theme.motion.duration_fast =
            std::time::Duration::from_millis(if reduce_motion { 0 } else { motion::FAST_MS });
        theme.motion.duration_normal =
            std::time::Duration::from_millis(if reduce_motion { 0 } else { motion::BASE_MS });
        theme.motion.duration_slow = theme.motion.duration_normal;
    });
}

fn apply_palette(c: &mut gpui_component::ThemeColor) {
    use color::*;
    c.background = hex(BG);
    c.title_bar = hex(BG);
    c.status_bar = hex(BG);
    c.scrollbar = hex(BG);
    c.accordion = hex(PANEL);
    c.list = hex(PANEL);
    c.list_even = hex(PANEL);
    c.table = hex(PANEL);
    c.table_foot = hex(PANEL);
    c.sidebar = hex(PANEL);
    c.tab_bar = hex(PANEL);
    c.tab = hex(PANEL);
    c.button = hex(RAISED);
    c.secondary = hex(RAISED);
    c.muted = hex(RAISED);
    c.popover = hex(RAISED);
    c.group_box = hex(RAISED);
    c.description_list_label = hex(RAISED);
    c.list_head = hex(RAISED);
    c.table_head = hex(RAISED);
    c.tab_bar_segmented = hex(RAISED);
    c.skeleton = hex(RAISED);
    // Unchecked track: BORDER, so a switch stays visible on RAISED cards as well as panels.
    c.switch = hex(BORDER);
    c.button_hover = hex(HOVER);
    c.button_active = hex(PRESSED);
    c.secondary_hover = hex(HOVER);
    c.secondary_active = hex(PRESSED);
    c.list_hover = hex(HOVER);
    c.table_hover = hex(HOVER);
    c.table_even = hex(HOVER);
    c.tab_active = hex(HOVER);
    c.sidebar_accent = hex(HOVER);
    c.border = hex(BORDER);
    c.input = hex(BORDER);
    c.sidebar_border = hex(BORDER);
    c.table_row_border = hex(BORDER);
    c.window_border = hex(BORDER);
    c.scrollbar_thumb = hex(BORDER);
    c.title_bar_border = hex(BORDER);
    c.status_bar_border = hex(BORDER);
    c.chart_grid = hex(BORDER);
    c.foreground = hex(TEXT);
    c.accent_foreground = hex(TEXT);
    c.button_foreground = hex(TEXT);
    c.popover_foreground = hex(TEXT);
    c.group_box_foreground = hex(TEXT);
    c.tab_active_foreground = hex(TEXT);
    c.sidebar_foreground = hex(TEXT);
    c.muted_foreground = hex(TEXT_SECONDARY);
    c.secondary_foreground = hex(TEXT_SECONDARY);
    c.tab_foreground = hex(TEXT_SECONDARY);
    c.table_head_foreground = hex(TEXT_SECONDARY);
    c.table_foot_foreground = hex(TEXT_SECONDARY);
    c.description_list_label_foreground = hex(TEXT_SECONDARY);
    c.primary = hex(ACCENT);
    c.ring = hex(ACCENT);
    c.caret = hex(ACCENT);
    c.drag_border = hex(ACCENT);
    c.list_active_border = hex(ACCENT);
    c.table_active_border = hex(ACCENT);
    c.sidebar_primary = hex(ACCENT);
    c.link = hex(ACCENT);
    c.link_hover = hex(ACCENT);
    c.link_active = hex(ACCENT);
    c.slider_bar = hex(ACCENT);
    c.scrollbar_thumb_hover = hex(ACCENT);
    c.primary_active = hex(ACCENT_PRESSED);
    c.primary_hover = hex(ACCENT_HOVER);
    c.progress_bar = hex(ACCENT);
    c.info = hex(ACCENT);
    c.info_hover = hex(ACCENT);
    c.info_active = hex(ACCENT);
    c.primary_foreground = hex(ON_ACCENT);
    c.sidebar_primary_foreground = hex(ON_ACCENT);
    c.info_foreground = hex(ON_ACCENT);
    c.danger_foreground = hex(ON_ACCENT);
    c.warning_foreground = hex(ON_ACCENT);
    c.success_foreground = hex(ON_ACCENT);
    c.switch_thumb = hex(ON_ACCENT);
    c.slider_thumb = hex(TEXT);
    c.danger = hex(DANGER);
    c.danger_hover = hex(DANGER);
    c.danger_active = hex(DANGER);
    c.chart_bearish = hex(DANGER);
    c.red = hex(DANGER);
    c.red_light = hex(DANGER);
    c.warning = hex(WARNING);
    c.warning_hover = hex(WARNING);
    c.warning_active = hex(WARNING);
    c.yellow = hex(WARNING);
    c.yellow_light = hex(WARNING);
    c.success = hex(SUCCESS);
    c.success_hover = hex(SUCCESS);
    c.success_active = hex(SUCCESS);
    c.chart_bullish = hex(SUCCESS);
    c.green = hex(SUCCESS);
    c.green_light = hex(SUCCESS);
    c.chart_1 = hex(CLIP_VIDEO);
    c.blue = hex(CLIP_VIDEO);
    c.blue_light = hex(CLIP_VIDEO);
    c.chart_2 = hex(CLIP_IMAGE);
    c.cyan = hex(ACCENT);
    c.cyan_light = hex(ACCENT);
    c.chart_3 = hex(CLIP_TEXT);
    c.magenta = hex(CLIP_TEXT);
    c.magenta_light = hex(CLIP_TEXT);
    c.chart_4 = hex(CLIP_SHAPE);
    c.chart_5 = hex(CLIP_LATEX);
    c.accent = tint(ACCENT, Alpha::Subtle);
    c.selection = tint(ACCENT, Alpha::Subtle);
    c.list_active = tint(ACCENT, Alpha::Subtle);
    c.table_active = tint(ACCENT, Alpha::Subtle);
    c.drop_target = tint(ACCENT, Alpha::Subtle);
    c.overlay = tint(BG, Alpha::Scrim);
    c.button_primary = c.primary;
    c.button_primary_hover = c.primary_hover;
    c.button_primary_active = c.primary_active;
    c.button_primary_foreground = c.primary_foreground;
    c.button_secondary = c.secondary;
    c.button_secondary_hover = c.secondary_hover;
    c.button_secondary_active = c.secondary_active;
    // Secondary buttons read TEXT; ghost buttons keep `secondary_foreground` (TEXT_SECONDARY).
    c.button_secondary_foreground = hex(TEXT);
    c.button_danger = c.danger;
    c.button_danger_hover = c.danger_hover;
    c.button_danger_active = c.danger_active;
    c.button_danger_foreground = c.danger_foreground;
    c.button_warning = c.warning;
    c.button_warning_hover = c.warning_hover;
    c.button_warning_active = c.warning_active;
    c.button_warning_foreground = c.warning_foreground;
    c.button_success = c.success;
    c.button_success_hover = c.success_hover;
    c.button_success_active = c.success_active;
    c.button_success_foreground = c.success_foreground;
    c.button_info = c.info;
    c.button_info_hover = c.info_hover;
    c.button_info_active = c.info_active;
    c.button_info_foreground = c.info_foreground;
}

#[cfg(test)]
mod theme_tests {
    use super::color;
    use super::tokens::Alpha;
    use super::{apply_palette, hex};

    fn luminance(rgb: u32) -> f64 {
        let channel = |shift: u32| {
            let value = f64::from((rgb >> shift) & 0xFF) / 255.0;
            if value <= 0.03928 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }

    fn contrast(foreground: u32, background: u32) -> f64 {
        let (a, b) = (luminance(foreground), luminance(background));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// `fg` at `alpha` over `bg`, composited in gamma (sRGB-encoded) space like
    /// the framebuffer blend, as `0xRRGGBB`.
    fn composite(fg: u32, alpha: f64, bg: u32) -> u32 {
        (0..3)
            .map(|step| {
                let shift = (2 - step) * 8;
                let front = f64::from((fg >> shift) & 0xFF);
                let back = f64::from((bg >> shift) & 0xFF);
                (front * alpha + back * (1. - alpha)).round() as u32
            })
            .fold(0u32, |acc, channel| (acc << 8) | channel & 0xFF)
    }

    /// Every text token except `TEXT_DISABLED` (hints, labels, values) stays
    /// readable (WCAG AA, 4.5:1) on every surface it sits on.
    #[test]
    fn muted_text_meets_aa_contrast() {
        for text in [color::TEXT, color::TEXT_SECONDARY, color::TEXT_MUTED] {
            for surface in [color::BG, color::PANEL, color::RAISED, color::HOVER] {
                let ratio = contrast(text, surface);
                assert!(
                    ratio >= 4.5,
                    "{text:#08X} on {surface:#08X} is only {ratio:.2}:1"
                );
            }
        }
    }

    /// Lettering on a primary (accent fill) button stays readable (4.5:1).
    #[test]
    fn on_accent_meets_aa_contrast() {
        let ratio = contrast(color::ON_ACCENT, color::ACCENT);
        assert!(ratio >= 4.5, "ON_ACCENT on ACCENT is only {ratio:.2}:1");
    }

    /// A clip body (kind colour at the Body step over `RAISED`) carries `TEXT`
    /// labels at 7:1 (see `docs/design.md` "Clip kinds").
    #[test]
    fn text_on_clip_bodies_meets_7_to_1_contrast() {
        for (kind, token) in [
            ("video", color::CLIP_VIDEO),
            ("image", color::CLIP_IMAGE),
            ("audio", color::CLIP_AUDIO),
            ("text", color::CLIP_TEXT),
            ("latex", color::CLIP_LATEX),
            ("shape", color::CLIP_SHAPE),
            ("graph", color::CLIP_GRAPH),
            ("plugin", color::CLIP_PLUGIN),
            ("error", color::CLIP_ERROR),
        ] {
            let body = composite(token, f64::from(Alpha::Body.value()), color::RAISED);
            let ratio = contrast(color::TEXT, body);
            assert!(
                ratio >= 7.,
                "{kind} body (#{body:06X}) under TEXT is only {ratio:.2}:1"
            );
        }
    }

    #[test]
    fn alpha_steps_match_design() {
        assert_eq!(Alpha::Wash.value(), 0.08);
        assert_eq!(Alpha::Subtle.value(), 0.16);
        assert_eq!(Alpha::Body.value(), 0.32);
        assert_eq!(Alpha::Edge.value(), 0.55);
        assert_eq!(Alpha::Scrim.value(), 0.78);
    }

    #[test]
    fn palette_covers_component_surfaces_and_semantic_variants() {
        let mut colors = gpui_component::ThemeColor::default();
        apply_palette(&mut colors);
        assert_eq!(colors.background, hex(color::BG));
        assert_eq!(colors.popover, hex(color::RAISED));
        assert_eq!(colors.button_primary, colors.primary);
        assert_eq!(colors.button_danger, colors.danger);
        assert_eq!(colors.table_hover, hex(color::HOVER));
        assert_eq!(colors.selection, super::tint(color::ACCENT, Alpha::Subtle));
        assert_eq!(colors.overlay, super::tint(color::BG, Alpha::Scrim));
        assert_eq!(colors.primary_hover, hex(color::ACCENT_HOVER));
        assert_eq!(colors.primary_active, hex(color::ACCENT_PRESSED));
        assert_eq!(colors.progress_bar, hex(color::ACCENT));
        assert_eq!(colors.info, hex(color::ACCENT));
    }
}
