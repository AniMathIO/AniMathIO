//! ABI 1.3 `text-layout` import: the renderer's text layout, split into glyph clusters.
//!
//! The layout is the same one the renderer runs for a normalized `text-run`
//! (embedded DejaVu Sans, wrapped to the canvas width, block centred on the canvas), so
//! a plugin that redraws the clusters as filled outlines puts every glyph where the
//! `text-run` would have. Clusters are grapheme clusters, so a ligature, a base letter
//! with its dot or combining accent, and each CJK character are single units.

use std::cell::RefCell;
use std::sync::Arc;

use parley::{
    Alignment, AlignmentOptions, FontContext, Layout, LayoutContext, PositionedLayoutItem,
    StyleProperty,
    fontique::{Blob, Collection, CollectionOptions, SourceCache},
};
use skrifa::MetadataProvider;
use skrifa::instance::{LocationRef, Size as FontSize};
use skrifa::outline::{DrawSettings, OutlinePen};

use crate::wit_types::{PathVerb, Point};

const FONT: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSans.ttf");
/// Largest pixel font size the renderer accepts for text.
const MAX_FONT_SIZE: f32 = 1_048_560.0;

pub(crate) struct Cluster {
    pub outline: Vec<Vec<PathVerb>>,
    pub left: f32,
    pub right: f32,
}

pub(crate) struct Line {
    pub top: f32,
    pub bottom: f32,
    pub clusters: Vec<Cluster>,
}

pub(crate) enum Align {
    Left,
    Center,
    Right,
}

struct Layouter {
    fonts: FontContext,
    layout: LayoutContext<()>,
}

thread_local! {
    /// Parsing the embedded font is the only costly setup; keep one context per thread.
    static LAYOUTER: RefCell<Option<Layouter>> = const { RefCell::new(None) };
}

fn layouter() -> Layouter {
    let mut collection = Collection::new(CollectionOptions {
        shared: false,
        system_fonts: false,
    });
    collection.register_fonts(Blob::new(Arc::new(FONT)), None);
    Layouter {
        fonts: FontContext {
            collection,
            source_cache: SourceCache::default(),
        },
        layout: LayoutContext::new(),
    }
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
    LAYOUTER.with(|cell| {
        let mut slot = cell.borrow_mut();
        let this = slot.get_or_insert_with(layouter);
        let mut builder = this
            .layout
            .ranged_builder(&mut this.fonts, text, 1.0, false);
        builder.push_default(StyleProperty::FontFamily("DejaVu Sans".into()));
        builder.push_default(StyleProperty::FontSize(font_size));
        let mut layout: Layout<()> = builder.build(text);
        #[allow(clippy::cast_precision_loss)] // Canvas sizes are far below 2^24.
        let (canvas_w, canvas_h) = (width as f32, height as f32);
        layout.break_all_lines(Some(canvas_w));
        layout.align(
            match align {
                Align::Left => Alignment::Start,
                Align::Center => Alignment::Center,
                Align::Right => Alignment::End,
            },
            AlignmentOptions::default(),
        );
        // Block centring, exactly as the renderer's `shape_centered`.
        let (mut min_x, mut max_x) = (f32::MAX, f32::MIN);
        for line in layout.lines() {
            let metrics = line.metrics();
            min_x = min_x.min(metrics.offset);
            max_x = max_x.max(metrics.offset + metrics.advance - metrics.hanging_advance);
        }
        if max_x < min_x {
            (min_x, max_x) = (0.0, 0.0);
        }
        let dx = canvas_w / 2.0 - f32::midpoint(min_x, max_x);
        let dy = canvas_h / 2.0 - layout.height() / 2.0;

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
                    let (mut left, mut right) = (f32::MAX, f32::MIN);
                    for _ in 0..cluster.glyphs().count() {
                        let Some(glyph) = glyphs.next() else { break };
                        left = left.min(glyph.x);
                        right = right.max(glyph.x + glyph.advance);
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
    })
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
