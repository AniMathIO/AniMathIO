//! The one text layout `AniMathIO` draws with.
//!
//! The renderer (`animathio-render`) shapes every normalized `text-run` through this
//! crate, and the plugin host's ABI 1.3 `text-layout` import runs the very same calls, so
//! a plugin that redraws glyph clusters as filled outlines puts each glyph exactly where
//! the renderer would have. The layout is: the embedded `DejaVu Sans`, wrapped to the canvas
//! width, lines aligned left / centre / right, then the whole block (widest line by line
//! stack height) centred on the canvas.
//!
//! The crate owns only the parley setup and the centring rule. Reading glyph runs or
//! grapheme clusters out of the returned [`Layout`] is up to the caller.

use std::sync::Arc;

pub use parley;
use parley::{
    Alignment, AlignmentOptions, FontContext, Layout, LayoutContext, StyleProperty,
    fontique::{Blob, Collection, CollectionOptions, SourceCache},
};

/// The embedded font, the only one the layout ever uses.
pub const FONT: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSans.ttf");
/// Family name `FONT` is registered and requested under.
pub const FAMILY: &str = "DejaVu Sans";
/// Largest pixel font size the renderer accepts for text.
pub const MAX_FONT_SIZE: f32 = 1_048_560.0;

/// How lines sit relative to each other inside the block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    /// Lines start at the same left edge.
    Left,
    /// Lines are centred on each other.
    Center,
    /// Lines end at the same right edge.
    Right,
}

/// Owns the font collection and parley's scratch buffers. Parsing the embedded font is the
/// only costly setup, so keep one per thread or backend and reuse it.
pub struct TextLayouter {
    fonts: FontContext,
    layout: LayoutContext<()>,
}

impl Default for TextLayouter {
    fn default() -> Self {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        collection.register_fonts(Blob::new(Arc::new(FONT)), None);
        Self {
            fonts: FontContext {
                collection,
                source_cache: SourceCache::default(),
            },
            layout: LayoutContext::new(),
        }
    }
}

impl TextLayouter {
    /// Shape `text` at `font_size` pixels, break it into lines at `width` pixels and
    /// align the lines. The block is not centred yet; see [`block_offset`].
    pub fn layout(&mut self, text: &str, font_size: f32, width: f32, align: Align) -> Layout<()> {
        let mut builder = self
            .layout
            .ranged_builder(&mut self.fonts, text, 1.0, false);
        builder.push_default(StyleProperty::FontFamily(FAMILY.into()));
        builder.push_default(StyleProperty::FontSize(font_size));
        let mut layout: Layout<()> = builder.build(text);
        // parley 0.13: alignment is set before line breaking instead of after it.
        layout.set_alignment(
            match align {
                Align::Left => Alignment::Start,
                Align::Center => Alignment::Center,
                Align::Right => Alignment::End,
            },
            AlignmentOptions::default(),
        );
        layout.break_all_lines(Some(width));
        layout
    }
}

/// The shift `(dx, dy)` that moves the centre of the laid-out block (widest line by line
/// stack height) to the centre of a `canvas` pixel box. Alignment still decides how the
/// lines sit relative to each other; only the whole block moves. This makes a clip
/// transform position the middle of the text, like it does for media, shapes and graphs.
#[must_use]
pub fn block_offset(layout: &Layout<()>, (width, height): (f32, f32)) -> (f32, f32) {
    let (mut min_x, mut max_x) = (f32::MAX, f32::MIN);
    for line in layout.lines() {
        let metrics = line.metrics();
        min_x = min_x.min(metrics.offset);
        max_x = max_x.max(metrics.offset + metrics.advance - metrics.hanging_advance);
    }
    if max_x < min_x {
        (min_x, max_x) = (0.0, 0.0);
    }
    (
        width / 2.0 - f32::midpoint(min_x, max_x),
        height / 2.0 - layout.height() / 2.0,
    )
}
