//! How the inspector presents numbers and colors, kept apart from the widgets so the rules are
//! testable: one hex format, rotation in degrees with one decimal, and one font size unit
//! (percent of the short canvas edge) across text and LaTeX. Stored values never change; only
//! what the inspector shows and what a typed value is converted back to.

use gpui::{App, Div, IntoElement, ParentElement, Styled, div, px};
use gpui_component::ActiveTheme;

/// `#rrggbb`, with the alpha byte appended only when the color is not fully opaque.
pub(super) fn format_hex(color: [u8; 4]) -> String {
    if color[3] == 255 {
        format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2])
    } else {
        format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            color[0], color[1], color[2], color[3]
        )
    }
}

/// Rotation shown with at most one decimal, so "-11.459" never clips the field.
pub(super) fn format_degrees(value: f64) -> String {
    let text = format!("{value:.1}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    match text {
        "" | "-" | "-0" => "0".to_owned(),
        other => other.to_owned(),
    }
}

/// The unit a plugin font size parameter is stored in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SizeUnits {
    /// Absolute pixels of the project canvas.
    Pixels,
    /// Already a percentage of the short edge.
    Percent,
    /// A fraction of the canvas height (the text plugin's normalized space).
    Normalized,
    /// A fraction of the short canvas edge (the LaTeX plugin's normalized space).
    NormalizedShort,
}

impl SizeUnits {
    /// The unit named by a plugin's `units` parameter; the plugin decides what `normalized`
    /// is relative to.
    pub(super) fn parse(plugin_id: &str, name: &str) -> Self {
        match name {
            "percent" => Self::Percent,
            "normalized" if plugin_id == "org.animathio.latex" => Self::NormalizedShort,
            "normalized" => Self::Normalized,
            _ => Self::Pixels,
        }
    }
}

/// The plugin number parameter that is a font size: `font_size` of the text plugin and `size`
/// of the LaTeX plugin.
pub(super) fn is_font_size_param(plugin_id: &str, name: &str) -> bool {
    matches!(
        (plugin_id, name),
        ("org.animathio.text", "font_size") | ("org.animathio.latex", "size")
    )
}

/// Font size as a percentage of the short canvas edge, from the stored value.
pub(super) fn size_to_display(model: f64, units: SizeUnits, canvas: (u32, u32)) -> f64 {
    let short = f64::from(canvas.0.min(canvas.1).max(1));
    let height = f64::from(canvas.1.max(1));
    match units {
        SizeUnits::Percent => model,
        SizeUnits::Pixels => model / short * 100.0,
        SizeUnits::Normalized => model * 100.0 * height / short,
        SizeUnits::NormalizedShort => model * 100.0,
    }
}

/// The stored value for a font size typed as a percentage of the short canvas edge.
pub(super) fn size_to_model(display: f64, units: SizeUnits, canvas: (u32, u32)) -> f64 {
    let short = f64::from(canvas.0.min(canvas.1).max(1));
    let height = f64::from(canvas.1.max(1));
    match units {
        SizeUnits::Percent => display,
        SizeUnits::Pixels => display / 100.0 * short,
        SizeUnits::Normalized => display / 100.0 * short / height,
        SizeUnits::NormalizedShort => display / 100.0,
    }
}

/// Whether a plugin parameter can carry keyframes: values that interpolate. Text, choices,
/// switches and asset references step or are not values at all, so they get no keyframe controls.
pub(super) fn is_animatable(model: &crate::inspector::ParamModel) -> bool {
    use crate::inspector::ParamModel;
    matches!(
        model,
        ParamModel::Number { .. } | ParamModel::Color { .. } | ParamModel::Point { .. }
    )
}

const CHECKER_CELL: f32 = 8.0;
const SWATCH_CELLS: usize = 4;

/// Frame a color picker's swatch: a checkerboard behind it so transparent colors read as
/// transparent, and a 1px outline so dark colors stay visible on the dark panel.
pub(super) fn framed_swatch(picker: impl IntoElement, transparent: bool, cx: &App) -> Div {
    let mut frame = div()
        .relative()
        .flex_shrink_0()
        .w(gpui::rems(2.0))
        .h(gpui::rems(2.0));
    if transparent {
        let light = gpui::rgb(0xcfcfcf);
        let dark = gpui::rgb(0x8c8c8c);
        let mut board = div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .rounded(cx.theme().radius)
            .overflow_hidden()
            .flex()
            .flex_col();
        for row in 0..SWATCH_CELLS {
            let mut line = div().flex().flex_row();
            for column in 0..SWATCH_CELLS {
                let color = if (row + column) % 2 == 0 { light } else { dark };
                line = line.child(
                    div()
                        .w(px(CHECKER_CELL))
                        .h(px(CHECKER_CELL))
                        .flex_shrink_0()
                        .bg(color),
                );
            }
            board = board.child(line);
        }
        frame = frame.child(board);
    }
    frame.child(picker).child(
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().foreground.opacity(0.35)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[core::prelude::v1::test]
    fn hex_drops_opaque_alpha() {
        assert_eq!(format_hex([255, 255, 255, 255]), "#ffffff");
        assert_eq!(format_hex([1, 2, 3, 0x80]), "#01020380");
        assert_eq!(format_hex([0, 0, 0, 0]), "#00000000");
    }

    #[core::prelude::v1::test]
    fn degrees_have_one_decimal() {
        assert_eq!(format_degrees(-11.459_155), "-11.5");
        assert_eq!(format_degrees(90.0), "90");
        assert_eq!(format_degrees(0.02), "0");
        assert_eq!(format_degrees(-0.04), "0");
        assert_eq!(format_degrees(33.333), "33.3");
    }

    #[core::prelude::v1::test]
    fn font_size_converts_to_percent_of_short_edge() {
        let canvas = (1920, 1080);
        // 48 px on a 1080 short edge.
        let pixels = size_to_display(48.0, SizeUnits::Pixels, canvas);
        assert!((pixels - 4.444_444).abs() < 1e-4);
        assert!((size_to_model(pixels, SizeUnits::Pixels, canvas) - 48.0).abs() < 1e-9);
        // Normalized text fractions are of the height; short edge equals height here.
        assert!((size_to_display(0.08, SizeUnits::Normalized, canvas) - 8.0).abs() < 1e-9);
        // Portrait: the width is the short edge.
        let portrait = (1080, 1920);
        let shown = size_to_display(0.08, SizeUnits::Normalized, portrait);
        assert!((shown - 14.222_222).abs() < 1e-4);
        assert!((size_to_model(shown, SizeUnits::Normalized, portrait) - 0.08).abs() < 1e-9);
        assert_eq!(size_to_display(12.0, SizeUnits::Percent, canvas), 12.0);
    }

    /// Each plugin's own pixel formula for a normalized size: text is a fraction of the height
    /// (`font_size * size.height`), LaTeX of the short edge (`size * min(w, h)`). Converting the
    /// shown percentage back through the display rule must give the same pixels, both ways.
    #[core::prelude::v1::test]
    fn portrait_normalized_sizes_match_each_plugin() {
        let canvas = (1080_u32, 1920_u32);
        let short = f64::from(canvas.0.min(canvas.1));
        let height = f64::from(canvas.1);
        // Each plugin multiplies the normalized size by its own edge to get pixels.
        let units = [
            ("org.animathio.text", SizeUnits::Normalized, height),
            ("org.animathio.latex", SizeUnits::NormalizedShort, short),
        ];
        for (plugin, expected_units, edge) in units {
            let pixels = |model: f64| model * edge;
            assert_eq!(SizeUnits::parse(plugin, "normalized"), expected_units);
            // Model -> display: percent of the short edge equals the rendered pixels / short.
            let model = 0.05;
            let shown = size_to_display(model, expected_units, canvas);
            assert!(
                (shown - pixels(model) / short * 100.0).abs() < 1e-9,
                "{plugin}"
            );
            // Display -> model: typing a percentage stores the value that renders those pixels.
            let typed = 10.0;
            let stored = size_to_model(typed, expected_units, canvas);
            assert!(
                (pixels(stored) - typed / 100.0 * short).abs() < 1e-9,
                "{plugin}"
            );
        }
    }

    #[core::prelude::v1::test]
    fn only_font_size_params_are_converted() {
        assert!(is_font_size_param("org.animathio.text", "font_size"));
        assert!(is_font_size_param("org.animathio.latex", "size"));
        assert!(!is_font_size_param("org.animathio.text", "font_weight"));
        assert!(!is_font_size_param("org.animathio.shapes", "size"));
    }
}
