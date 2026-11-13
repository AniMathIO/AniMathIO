//! Visual design tokens for the editor (see `docs/design.md`).
//!
//! UI code takes colours, radii and spacing from here (or from the gpui-component
//! `Theme`, which is derived from these tokens) instead of hard-coding values.
#![allow(dead_code)]

use gpui::{Hsla, rgb};

/// Colour tokens as `0xRRGGBB`.
pub mod color {
    /// Window background, visible as the gutter between panels.
    pub const BG: u32 = 0x0B0D12;
    /// Panel surface.
    pub const PANEL: u32 = 0x11141B;
    /// Raised surface: cards, inputs, popovers, track lanes.
    pub const RAISED: u32 = 0x171B24;
    /// Hovered surface.
    pub const HOVER: u32 = 0x1E2330;
    /// Hairline borders and dividers.
    pub const BORDER: u32 = 0x262C3A;
    pub const TEXT: u32 = 0xE6E9F0;
    pub const TEXT_SECONDARY: u32 = 0x9AA3B5;
    pub const TEXT_MUTED: u32 = 0x5D6578;

    /// Solid accent for selection outlines, active tabs and focus.
    pub const ACCENT: u32 = 0x8B7BFF;
    /// Primary-action gradient start and end.
    pub const ACCENT_FROM: u32 = 0x7C5CFF;
    pub const ACCENT_TO: u32 = 0x22D3EE;
    pub const PLAYHEAD: u32 = 0xFF4D6D;
    pub const SUCCESS: u32 = 0x22C55E;
    pub const WARNING: u32 = 0xF59E0B;
    pub const DANGER: u32 = 0xEF4444;

    /// Clip colours by kind.
    pub const CLIP_VIDEO: u32 = 0x3B82F6;
    pub const CLIP_IMAGE: u32 = 0x14B8A6;
    pub const CLIP_AUDIO: u32 = 0x22C55E;
    pub const CLIP_TEXT: u32 = 0xA855F7;
    pub const CLIP_SHAPE: u32 = 0xF59E0B;
    pub const CLIP_LATEX: u32 = 0xEC4899;
    pub const CLIP_GRAPH: u32 = 0x06B6D4;
    pub const CLIP_PLUGIN: u32 = 0x64748B;
    pub const CLIP_ERROR: u32 = 0xEF4444;
    pub const VIEWER_STAGE: u32 = 0x090B10;
    pub const VIEWER_SHADOW: u32 = 0x000000;
}

/// Corner radii in pixels.
pub mod radius {
    pub const PANEL: f32 = 10.0;
    pub const CARD: f32 = 6.0;
    pub const CLIP: f32 = 6.0;
    pub const CONTROL: f32 = 6.0;
    pub const PILL: f32 = 999.0;
}

/// Spacing in pixels (4px grid).
pub mod space {
    pub const GUTTER: f32 = 6.0;
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
}

/// Animation durations in milliseconds.
pub mod motion {
    pub const FAST_MS: u64 = 120;
    pub const BASE_MS: u64 = 180;
}

/// An opaque colour from a `0xRRGGBB` token.
pub fn hex(token: u32) -> Hsla {
    rgb(token).into()
}

/// A colour from a `0xRRGGBB` token with the given alpha (0–1).
pub fn hexa(token: u32, alpha: f32) -> Hsla {
    let mut color: Hsla = rgb(token).into();
    color.a = alpha.clamp(0.0, 1.0);
    color
}

/// Shared primary-action treatment. Keep dark lettering legible at the cyan end.
pub fn primary_gradient() -> gpui::Background {
    gpui::linear_gradient(
        110.,
        gpui::linear_color_stop(hex(color::ACCENT_FROM), 0.),
        gpui::linear_color_stop(hex(color::ACCENT_TO), 1.),
    )
}

/// Gradient primary button (Export, Create). Dark lettering stays legible on
/// both ends of the violet to cyan ramp.
pub fn primary_button(
    id: impl Into<gpui::ElementId>,
    cx: &gpui::App,
) -> gpui_component::button::Button {
    use gpui::Styled as _;
    use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(hex(color::ACCENT_FROM))
                .foreground(hex(color::BG))
                .hover(hex(color::ACCENT_FROM))
                .active(hex(color::ACCENT_FROM)),
        )
        .bg(primary_gradient())
        .font_weight(gpui::FontWeight::SEMIBOLD)
}

/// Compose the bundled brand with the component kit's complete Lucide catalog.
pub struct Assets;
impl gpui::AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        if path == "brand.svg" {
            return Ok(Some(std::borrow::Cow::Borrowed(include_bytes!(
                "../assets/brand.svg"
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

/// Install once before creating widgets. `Theme::update` also synchronizes the
/// semantic backgrounds and the base kit (resize handles and scrollbars).
pub fn install_theme(cx: &mut gpui::App, compact: bool, reduce_motion: bool) {
    use gpui::px;
    use gpui_component::{Theme, ThemeMode};
    use std::borrow::Cow;
    let fonts = vec![
        Cow::Borrowed(include_bytes!("../assets/fonts/InterVariable.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf").as_slice()),
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
        theme.font_size = px(if compact { 12. } else { 14. });
        theme.radius = px(radius::CONTROL);
        theme.radius_lg = px(radius::PANEL);
        theme.shadow = true;
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
    c.switch = hex(RAISED);
    c.button_hover = hex(HOVER);
    c.button_active = hex(HOVER);
    c.secondary_hover = hex(HOVER);
    c.secondary_active = hex(HOVER);
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
    c.primary_active = hex(ACCENT_FROM);
    c.primary_hover = hex(ACCENT_TO);
    c.progress_bar = hex(ACCENT_TO);
    c.info = hex(ACCENT_TO);
    c.info_hover = hex(ACCENT_TO);
    c.info_active = hex(ACCENT_TO);
    c.primary_foreground = hex(BG);
    c.sidebar_primary_foreground = hex(BG);
    c.info_foreground = hex(BG);
    c.danger_foreground = hex(BG);
    c.warning_foreground = hex(BG);
    c.success_foreground = hex(BG);
    c.switch_thumb = hex(BG);
    c.slider_thumb = hex(BG);
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
    c.chart_2 = hex(ACCENT_TO);
    c.cyan = hex(ACCENT_TO);
    c.cyan_light = hex(ACCENT_TO);
    c.chart_3 = hex(ACCENT);
    c.magenta = hex(ACCENT);
    c.magenta_light = hex(ACCENT);
    c.chart_4 = hex(CLIP_SHAPE);
    c.chart_5 = hex(CLIP_LATEX);
    c.accent = hexa(ACCENT, 0.20);
    c.selection = hexa(ACCENT, 0.20);
    c.list_active = hexa(ACCENT, 0.20);
    c.table_active = hexa(ACCENT, 0.20);
    c.drop_target = hexa(ACCENT, 0.20);
    c.overlay = hexa(BG, 0.78);
    c.button_primary = c.primary;
    c.button_primary_hover = c.primary_hover;
    c.button_primary_active = c.primary_active;
    c.button_primary_foreground = c.primary_foreground;
    c.button_secondary = c.secondary;
    c.button_secondary_hover = c.secondary_hover;
    c.button_secondary_active = c.secondary_active;
    c.button_secondary_foreground = c.secondary_foreground;
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

/// A floating panel, leaving half a gutter on each shared edge.
pub fn panel(content: impl gpui::IntoElement) -> gpui::Div {
    use gpui::{ParentElement as _, Styled as _, div, px};
    div().size_full().p(px(space::GUTTER / 2.)).child(
        div()
            .size_full()
            .overflow_hidden()
            .rounded(px(radius::PANEL))
            .border_1()
            .border_color(hex(color::BORDER))
            .bg(hex(color::PANEL))
            .child(content),
    )
}

pub fn panel_header(
    icon: crate::icons::IconName,
    title: &'static str,
    action: impl gpui::IntoElement,
) -> gpui::Div {
    use gpui::{ParentElement as _, Styled as _, div, px};
    use gpui_component::Sizable as _;
    div()
        .flex()
        .items_center()
        .h(px(38.))
        .flex_shrink_0()
        .px_3()
        .gap_2()
        .bg(hex(color::PANEL))
        .border_b_1()
        .border_color(hex(color::BORDER))
        .text_size(px(13.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child(
            crate::icons::icon(icon)
                .small()
                .text_color(hex(color::TEXT_SECONDARY)),
        )
        .child(div().flex_1().child(title))
        .child(action)
}

/// The kit owns toast placement, dismissal and its slide animation; the app
/// adds the semantic left stripe before handing it to the kit.
pub struct Toast;
impl Toast {
    fn styled(
        message: impl Into<gpui::SharedString>,
        kind: gpui_component::notification::NotificationType,
        color: u32,
    ) -> gpui_component::notification::Notification {
        use gpui::Styled as _;
        gpui_component::notification::Notification::new()
            .message(message)
            .with_type(kind)
            .border_0()
            .border_l(gpui::px(3.))
            .border_color(hex(color))
            .rounded(gpui::px(radius::PANEL))
    }
    pub fn info(
        message: impl Into<gpui::SharedString>,
    ) -> gpui_component::notification::Notification {
        Self::styled(
            message,
            gpui_component::notification::NotificationType::Info,
            color::ACCENT_TO,
        )
    }
    pub fn warning(
        message: impl Into<gpui::SharedString>,
    ) -> gpui_component::notification::Notification {
        Self::styled(
            message,
            gpui_component::notification::NotificationType::Warning,
            color::WARNING,
        )
    }
    pub fn success(
        message: impl Into<gpui::SharedString>,
    ) -> gpui_component::notification::Notification {
        Self::styled(
            message,
            gpui_component::notification::NotificationType::Success,
            color::SUCCESS,
        )
    }
    pub fn error(
        message: impl Into<gpui::SharedString>,
    ) -> gpui_component::notification::Notification {
        Self::styled(
            message,
            gpui_component::notification::NotificationType::Error,
            color::DANGER,
        )
    }
}

#[cfg(test)]
mod theme_tests {
    use super::*;
    #[test]
    fn palette_covers_component_surfaces_and_semantic_variants() {
        let mut colors = gpui_component::ThemeColor::default();
        apply_palette(&mut colors);
        assert_eq!(colors.background, hex(color::BG));
        assert_eq!(colors.popover, hex(color::RAISED));
        assert_eq!(colors.button_primary, colors.primary);
        assert_eq!(colors.button_danger, colors.danger);
        assert_eq!(colors.table_hover, hex(color::HOVER));
        assert_eq!(colors.selection, hexa(color::ACCENT, 0.2));
    }
}
