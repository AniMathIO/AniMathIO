//! ABI 1.3 `text-layout` import: the renderer's text layout, split into glyph clusters.
//!
//! The layout is not a copy of the renderer's: both call `animathio-text-layout`
//! (embedded DejaVu Sans, wrapped to the canvas width, block centred on the canvas), so a
//! plugin that redraws the clusters as filled outlines puts every glyph where the
//! `text-run` would have. Clusters are grapheme clusters, so a ligature, a base letter
//! with its dot or combining accent, and each CJK character are single units. This module
//! only reads clusters and outlines out of that shared layout.

use std::cell::RefCell;

use animathio_text_layout::{
    TextLayouter, block_offset,
    parley::{self, PositionedLayoutItem},
};
use skrifa::MetadataProvider;
use skrifa::instance::{LocationRef, Size as FontSize};
use skrifa::outline::{DrawSettings, OutlinePen};

use crate::wit_types::{PathVerb, Point};

pub(crate) use animathio_text_layout::Align;
use animathio_text_layout::MAX_FONT_SIZE;

pub(crate) struct Cluster {
    pub outline: Vec<Vec<PathVerb>>,
    pub left: f32,
    pub right: f32,
    /// `(glyph id, x, y)` of the cluster's glyphs, canvas pixels as the renderer draws them.
    #[cfg(test)]
    pub glyphs: Vec<(u32, f32, f32)>,
}

pub(crate) struct Line {
    pub top: f32,
    pub bottom: f32,
    pub clusters: Vec<Cluster>,
}

thread_local! {
    /// Parsing the embedded font is the only costly setup; keep one layouter per thread.
    static LAYOUTER: RefCell<Option<TextLayouter>> = const { RefCell::new(None) };
}

/// Lay `text` out for a `width` x `height` canvas. `max_verbs` bounds the outline size.
pub(crate) fn layout_text(
    text: &str,
    font_size: f32,
    (width, height): (u32, u32),
    align: Align,
    max_verbs: usize,
) -> Result<Vec<Line>, &'static str> {
    if !font_size.is_finite() || font_size <= 0.0 || font_size > MAX_FONT_SIZE {
        return Err("font size must be finite and positive");
    }
    if width == 0 || height == 0 {
        return Err("canvas must not be empty");
    }
    #[allow(clippy::cast_precision_loss)] // Canvas sizes are far below 2^24.
    let (canvas_w, canvas_h) = (width as f32, height as f32);
    LAYOUTER.with(|cell| {
        let mut slot = cell.borrow_mut();
        let layouter = slot.get_or_insert_with(TextLayouter::default);
        let layout = layouter.layout(text, font_size, canvas_w, align);
        let (dx, dy) = block_offset(&layout, (canvas_w, canvas_h));
        read_lines(&layout, dx, dy, max_verbs)
    })
}

fn read_lines(
    layout: &parley::Layout<()>,
    dx: f32,
    dy: f32,
    max_verbs: usize,
) -> Result<Vec<Line>, &'static str> {
    let mut verbs = 0_usize;
    let mut lines = Vec::new();
    for line in layout.lines() {
        let metrics = line.metrics();
        let mut clusters = Vec::new();
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else {
                continue;
            };
            let font = run.run().font();
            let Ok(font_ref) = skrifa::FontRef::from_index(font.data.data(), font.index) else {
                continue;
            };
            let outlines = font_ref.outline_glyphs();
            let size = run.run().font_size();
            let mut glyphs = run.positioned_glyphs();
            for cluster in run.run().visual_clusters() {
                let mut contours: Vec<Vec<PathVerb>> = Vec::new();
                #[cfg(test)]
                let mut placed: Vec<(u32, f32, f32)> = Vec::new();
                let (mut left, mut right) = (f32::MAX, f32::MIN);
                for _ in 0..cluster.glyphs().count() {
                    let Some(glyph) = glyphs.next() else { break };
                    left = left.min(glyph.x);
                    right = right.max(glyph.x + glyph.advance);
                    #[cfg(test)]
                    placed.push((glyph.id, glyph.x + dx, glyph.y + dy));
                    let Some(outline) = outlines.get(skrifa::GlyphId::new(glyph.id)) else {
                        continue;
                    };
                    let mut pen = Pen {
                        contours: Vec::new(),
                        current: Vec::new(),
                        x: glyph.x + dx,
                        y: glyph.y + dy,
                    };
                    let _ = outline.draw(
                        DrawSettings::unhinted(FontSize::new(size), LocationRef::default()),
                        &mut pen,
                    );
                    verbs += pen.contours.iter().map(Vec::len).sum::<usize>();
                    if verbs > max_verbs {
                        return Err("text exceeds path verb limit");
                    }
                    contours.extend(pen.contours);
                }
                if right < left {
                    continue; // A ligature continuation: its glyphs belong to the start.
                }
                clusters.push(Cluster {
                    outline: contours,
                    left: left + dx,
                    right: right + dx,
                    #[cfg(test)]
                    glyphs: placed,
                });
            }
        }
        lines.push(Line {
            top: metrics.block_min_coord + dy,
            bottom: metrics.block_max_coord + dy,
            clusters,
        });
    }
    Ok(lines)
}

/// Collects one glyph's outline, mapped into canvas pixels (y down).
struct Pen {
    contours: Vec<Vec<PathVerb>>,
    current: Vec<PathVerb>,
    x: f32,
    y: f32,
}

impl Pen {
    fn point(&self, x: f32, y: f32) -> Point {
        Point {
            x: self.x + x,
            y: self.y - y,
        }
    }
}

impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        if !self.current.is_empty() {
            self.contours.push(std::mem::take(&mut self.current));
        }
        let p = self.point(x, y);
        self.current.push(PathVerb::MoveTo(p));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.point(x, y);
        self.current.push(PathVerb::LineTo(p));
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let (c, p) = (self.point(cx, cy), self.point(x, y));
        self.current.push(PathVerb::QuadTo((c, p)));
    }

    fn curve_to(&mut self, c0x: f32, c0y: f32, c1x: f32, c1y: f32, x: f32, y: f32) {
        let (a, b, p) = (self.point(c0x, c0y), self.point(c1x, c1y), self.point(x, y));
        self.current.push(PathVerb::CubicTo((a, b, p)));
    }

    fn close(&mut self) {
        self.current.push(PathVerb::Close);
        self.contours.push(std::mem::take(&mut self.current));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use animathio_core::TextAlignment;
    use animathio_render::CpuBackend;

    /// The host import and the renderer run the same layout: every glyph id and position
    /// the clusters carry equals what `CpuBackend` draws for a normalized text run, bit
    /// for bit, across texts, alignments and canvas sizes.
    #[test]
    fn clusters_carry_exactly_the_glyphs_the_renderer_draws() {
        let texts = [
            "Hello",
            "Hello, wrapped text that is long enough to need several lines on a small canvas",
            "line one\nline two is a bit longer\nthree",
            "fi ffi office fl", // ligature candidates
            "Zoë café \u{1e9e}",
            "\u{5e9}\u{5dc}\u{5d5}\u{5dd} abc", // RTL run mixed with latin
            "",
            "   ",
        ];
        let canvases = [(320_u32, 180_u32), (1280, 720), (1080, 1920), (97, 61)];
        let aligns = [
            (Align::Left, TextAlignment::Left),
            (Align::Center, TextAlignment::Center),
            (Align::Right, TextAlignment::Right),
        ];
        let sizes = [0.04_f32, 0.12, 0.3];
        let mut backend = CpuBackend::default();
        let mut compared = 0_usize;
        for text in texts {
            for canvas in canvases {
                for (host_align, core_align) in aligns {
                    for rel in sizes {
                        #[allow(clippy::cast_precision_loss)]
                        let font_size = rel * canvas.1 as f32;
                        let lines =
                            layout_text(text, font_size, canvas, host_align, usize::MAX).unwrap();
                        let host: Vec<(u32, f32, f32)> = lines
                            .iter()
                            .flat_map(|line| &line.clusters)
                            .flat_map(|cluster| cluster.glyphs.iter().copied())
                            .collect();
                        let drawn = backend
                            .text_glyphs(text, font_size, canvas, core_align)
                            .unwrap();
                        assert_eq!(
                            host, drawn,
                            "text {text:?} canvas {canvas:?} align {core_align:?} size {font_size}"
                        );
                        compared += host.len();
                    }
                }
            }
        }
        assert!(compared > 1000, "the comparison must cover real glyphs");
    }
}
