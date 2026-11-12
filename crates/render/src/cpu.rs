use crate::{Backend, Error, Frame, FrameSource, ResourcePool, Size, check_size};
use animathio_core::{
    ClipKind, Color as CoreColor, PathVerb, Scene, SceneClip, SceneTransition, StrokeCap,
    StrokeJoin, StrokeStyle, TextElement, Transform, TransitionKind, TransitionRole, VectorElement,
    VectorScene, WipeDirection,
    builtin::{Builtin, Color, PathElement, Shape},
};
use parley::{
    Alignment, AlignmentOptions, FontContext, Layout, LayoutContext, PositionedLayoutItem,
    StyleProperty,
    fontique::{Collection, CollectionOptions, SourceCache},
};
use std::sync::Arc;
use vello_cpu::{
    Glyph, Image, ImageSource, Level, PixelMetadata, Pixmap, RenderContext, RenderSettings,
    Resources,
    color::{AlphaColor, Srgb},
    kurbo::{Affine, BezPath, Cap, Ellipse, Join, Rect, Shape as _, Stroke},
    peniko::{Blob, ImageAlphaType, ImageSampler},
};

const FONT: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSans.ttf");

/// Single-threaded scalar Vello CPU reference backend, with embedded fonts only.
pub struct CpuBackend {
    fonts: FontContext,
    layout: LayoutContext<()>,
}
impl Default for CpuBackend {
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

impl Backend for CpuBackend {
    fn render(
        &mut self,
        scene: &Scene,
        pool: &mut ResourcePool,
        video: &mut dyn FrameSource,
        size: Size,
    ) -> Result<Frame, Error> {
        let (width, height) = check_size(size)?;
        // Rebuild drawing state each frame. Only immutable media/font data may be cached.
        let mut ctx = RenderContext::new_with(
            width,
            height,
            RenderSettings {
                level: Level::fallback(),
                num_threads: 0,
            },
        );
        let mut resources = Resources::new();
        let mut paired = vec![false; scene.clips.len()];
        for (index, clip) in scene.clips.iter().enumerate() {
            if paired[index] {
                continue;
            }
            // Both sides of a transition are blended into one layer before compositing.
            if let Some(state) = clip.transition
                && let Some(partner) = scene.clips.iter().enumerate().position(|(j, other)| {
                    j != index
                        && other
                            .transition
                            .is_some_and(|o| o.id == state.id && o.role != state.role)
                })
            {
                paired[partner] = true;
                let (outgoing, incoming) = match state.role {
                    TransitionRole::Outgoing => (clip, &scene.clips[partner]),
                    TransitionRole::Incoming => (&scene.clips[partner], clip),
                };
                self.draw_transition(&mut ctx, state, outgoing, incoming, pool, video, size)?;
                continue;
            }
            if matches!(clip.kind, ClipKind::Audio(_)) || clip.transform.opacity == 0.0 {
                continue;
            }
            // Singular transforms have no visible area and cannot be inverted for image paints.
            if clip.transform.scale_x == 0.0 || clip.transform.scale_y == 0.0 {
                continue;
            }
            // Vector scenes are already in canvas pixel coordinates, so they use a
            // pixel-space transform instead of the unit-square transform used by media
            // and temporary builtin clips.
            let transform = match &clip.kind {
                ClipKind::Vector(_) => pixel_transform(clip.transform, size)?,
                _ => unit_transform(clip.transform, size)?,
            };
            ctx.set_transform(transform);
            if clip.effects.is_empty() {
                #[allow(clippy::cast_possible_truncation)] // Core validates opacity in [0, 1].
                ctx.push_opacity_layer(clip.transform.opacity as f32);
                self.draw_clip(&mut ctx, &mut resources, size, clip, pool, video)?;
                ctx.pop_layer();
            } else {
                // Rasterize the clip into its own layer, apply the effect chain
                // there, then composite. Effects must not see other clips.
                let mut layer = self.render_effect_layer(clip, pool, video, size, transform)?;
                crate::effects::apply_chain(&mut layer, &clip.effects)?;
                ctx.set_transform(Affine::IDENTITY);
                #[allow(clippy::cast_possible_truncation)] // Core validates opacity in [0, 1].
                ctx.push_opacity_layer(clip.transform.opacity as f32);
                composite_layer(&mut ctx, &layer)?;
                ctx.pop_layer();
            }
        }
        ctx.flush();
        let mut target = Pixmap::new(width, height);
        ctx.render(&mut target, &mut resources);
        Ok(Frame {
            size,
            pixels: target.take_rgba8(ImageAlphaType::Alpha),
        })
    }
}

fn color(value: Color) -> AlphaColor<Srgb> {
    AlphaColor::from_rgba8(value[0], value[1], value[2], value[3])
}

fn unit_transform(t: Transform, size: Size) -> Result<Affine, Error> {
    if !transform_valid(t) {
        return Err(Error::Invalid("invalid scene transform"));
    }
    let width = f64::from(size.0);
    let height = f64::from(size.1);
    let transform = Affine::translate((t.x * width, t.y * height))
        * Affine::rotate(t.rotation)
        * Affine::scale_non_uniform(t.scale_x * width, t.scale_y * height)
        * Affine::translate((-0.5, -0.5));
    finite_transform(transform)
}

/// Transform for resolved vector scenes, whose coordinates are already canvas pixels.
/// The default transform (x/y = 0.5, scale 1) is the identity: content is anchored at the
/// canvas center. Position, rotation and scale otherwise behave like other clip kinds.
fn pixel_transform(t: Transform, size: Size) -> Result<Affine, Error> {
    if !transform_valid(t) {
        return Err(Error::Invalid("invalid scene transform"));
    }
    let width = f64::from(size.0);
    let height = f64::from(size.1);
    let transform = Affine::translate((t.x * width, t.y * height))
        * Affine::rotate(t.rotation)
        * Affine::scale_non_uniform(t.scale_x, t.scale_y)
        * Affine::translate((-0.5 * width, -0.5 * height));
    finite_transform(transform)
}

fn transform_valid(t: Transform) -> bool {
    [t.x, t.y, t.scale_x, t.scale_y, t.rotation, t.opacity]
        .iter()
        .all(|v| v.is_finite())
        && (0.0..=1.0).contains(&t.opacity)
}

fn finite_transform(transform: Affine) -> Result<Affine, Error> {
    if !transform.as_coeffs().iter().all(|c| c.is_finite()) {
        return Err(Error::Invalid("scene transform overflow"));
    }
    Ok(transform)
}

fn color_core(value: CoreColor) -> AlphaColor<Srgb> {
    color([value.r, value.g, value.b, value.a])
}

fn vector_path(path: &[PathVerb]) -> BezPath {
    let mut bez = BezPath::new();
    for verb in path {
        match *verb {
            PathVerb::MoveTo(p) => bez.move_to((p.x, p.y)),
            PathVerb::LineTo(p) => bez.line_to((p.x, p.y)),
            PathVerb::QuadTo(c, to) => bez.quad_to((c.x, c.y), (to.x, to.y)),
            PathVerb::CubicTo(c1, c2, to) => {
                bez.curve_to((c1.x, c1.y), (c2.x, c2.y), (to.x, to.y));
            }
            PathVerb::Close => bez.close_path(),
        }
    }
    bez
}

fn vector_stroke(style: &StrokeStyle) -> Stroke {
    let cap = match style.cap {
        StrokeCap::Butt => Cap::Butt,
        StrokeCap::Round => Cap::Round,
        StrokeCap::Square => Cap::Square,
    };
    let join = match style.join {
        StrokeJoin::Miter => Join::Miter,
        StrokeJoin::Round => Join::Round,
        StrokeJoin::Bevel => Join::Bevel,
    };
    Stroke::new(style.width).with_join(join).with_caps(cap)
}

fn path(shape: &Shape) -> BezPath {
    match shape {
        Shape::Rect => Rect::new(0.0, 0.0, 1.0, 1.0).to_path(0.001),
        Shape::Ellipse => Ellipse::new((0.5, 0.5), (0.5, 0.5), 0.0).to_path(0.001),
        Shape::Path(elements) => {
            let mut path = BezPath::new();
            for element in elements {
                match *element {
                    PathElement::MoveTo([x, y]) => path.move_to((x, y)),
                    PathElement::LineTo([x, y]) => path.line_to((x, y)),
                    PathElement::QuadTo([x, y], [a, b]) => path.quad_to((x, y), (a, b)),
                    PathElement::CurveTo(first, second, end) => {
                        path.curve_to(
                            (first[0], first[1]),
                            (second[0], second[1]),
                            (end[0], end[1]),
                        );
                    }
                    PathElement::Close => path.close_path(),
                }
            }
            path
        }
    }
}

fn transparent(size: Size) -> Frame {
    Frame {
        size,
        pixels: vec![0; size.0 as usize * size.1 as usize * 4],
    }
}

fn composite_layer(ctx: &mut RenderContext, frame: &Frame) -> Result<(), Error> {
    frame.validate()?;
    let (width, height) = check_size(frame.size)?;
    let pixels = frame.pixels.as_slice().to_vec();
    let pixmap = Pixmap::from_parts(
        pixels,
        width,
        height,
        PixelMetadata::new(ImageAlphaType::Alpha, true),
    );
    ctx.set_paint(Image {
        image: ImageSource::Pixmap(Arc::new(pixmap)),
        sampler: ImageSampler::default(),
    });
    ctx.set_paint_transform(Affine::IDENTITY);
    ctx.fill_rect(&Rect::new(0.0, 0.0, f64::from(width), f64::from(height)));
    ctx.set_paint_transform(Affine::IDENTITY);
    Ok(())
}

fn draw_image(ctx: &mut RenderContext, frame: &Frame) -> Result<(), Error> {
    frame.validate()?;
    let (width, height) = check_size(frame.size)?;
    // Use a fresh exactly-sized vector: Vello's cast requires capacity divisible by four.
    let pixels = frame.pixels.as_slice().to_vec();
    let pixmap = Pixmap::from_parts(
        pixels,
        width,
        height,
        PixelMetadata::new(ImageAlphaType::Alpha, true),
    );
    ctx.set_paint(Image {
        image: ImageSource::Pixmap(Arc::new(pixmap)),
        sampler: ImageSampler::default(),
    });
    ctx.set_paint_transform(Affine::scale_non_uniform(
        1.0 / f64::from(width),
        1.0 / f64::from(height),
    ));
    ctx.fill_rect(&Rect::new(0.0, 0.0, 1.0, 1.0));
    ctx.set_paint_transform(Affine::IDENTITY);
    Ok(())
}

impl CpuBackend {
    fn draw_clip(
        &mut self,
        ctx: &mut RenderContext,
        resources: &mut Resources,
        size: Size,
        clip: &SceneClip,
        pool: &mut ResourcePool,
        video: &mut dyn FrameSource,
    ) -> Result<(), Error> {
        match &clip.kind {
            ClipKind::Image(id) => draw_image(ctx, pool.image(id)?.as_ref())?,
            ClipKind::Video(id) => {
                if let Some(frame) = video.frame_at(id, clip.source_time) {
                    draw_image(ctx, &frame)?;
                } else {
                    self.placeholder(
                        ctx,
                        resources,
                        size,
                        &format!("VIDEO {}\n{} ticks", &id.as_str()[..8], clip.source_time.0),
                    )?;
                }
            }
            ClipKind::Builtin(Builtin::Text {
                text,
                font_size,
                color,
            }) => {
                self.text(ctx, resources, size, text, *font_size, *color)?;
            }
            ClipKind::Builtin(Builtin::Shape {
                shape,
                fill,
                stroke,
            }) => {
                let path = path(shape);
                if let Some(fill) = fill {
                    ctx.set_paint(color(*fill));
                    ctx.fill_path(&path);
                }
                if let Some(stroke) = stroke {
                    ctx.set_paint(color(stroke.color));
                    ctx.set_stroke(Stroke::new(stroke.width));
                    ctx.stroke_path(&path);
                }
            }
            ClipKind::Plugin { plugin_id, .. } => {
                self.placeholder(ctx, resources, size, &format!("PLUGIN\n{plugin_id}"))?;
            }
            ClipKind::Vector(scene) => {
                self.vector_scene(ctx, resources, size, scene)?;
            }
            ClipKind::Error(message) => {
                self.placeholder(ctx, resources, size, &format!("ERROR\n{message}"))?;
            }
            ClipKind::Audio(_) => {}
        }
        Ok(())
    }

    /// Rasterize one clip with its effects and opacity into a canvas-sized layer.
    fn clip_layer(
        &mut self,
        clip: &SceneClip,
        pool: &mut ResourcePool,
        video: &mut dyn FrameSource,
        size: Size,
    ) -> Result<Frame, Error> {
        if matches!(clip.kind, ClipKind::Audio(_))
            || clip.transform.opacity == 0.0
            || clip.transform.scale_x == 0.0
            || clip.transform.scale_y == 0.0
        {
            return Ok(transparent(size));
        }
        let transform = match &clip.kind {
            ClipKind::Vector(_) => pixel_transform(clip.transform, size)?,
            _ => unit_transform(clip.transform, size)?,
        };
        let mut layer = self.render_effect_layer(clip, pool, video, size, transform)?;
        crate::effects::apply_chain(&mut layer, &clip.effects)?;
        crate::transition::fade(&mut layer, clip.transform.opacity);
        Ok(layer)
    }

    /// Composite a transition: opacity blend for dissolves, a solid black or white
    /// intermediate for dips, and a rectangular clip mask for wipes.
    #[allow(clippy::too_many_arguments)]
    fn draw_transition(
        &mut self,
        ctx: &mut RenderContext,
        state: SceneTransition,
        outgoing: &SceneClip,
        incoming: &SceneClip,
        pool: &mut ResourcePool,
        video: &mut dyn FrameSource,
        size: Size,
    ) -> Result<(), Error> {
        use crate::transition::{mix, solid};
        let from = self.clip_layer(outgoing, pool, video, size)?;
        let to = self.clip_layer(incoming, pool, video, size)?;
        let p = state.progress;
        ctx.set_transform(Affine::IDENTITY);
        match state.kind {
            TransitionKind::CrossDissolve => composite_layer(ctx, &mix(&from, &to, p))?,
            TransitionKind::DipToBlack | TransitionKind::DipToWhite => {
                let level = if state.kind == TransitionKind::DipToBlack {
                    0
                } else {
                    255
                };
                let middle = solid(size, level);
                let blended = if p < 0.5 {
                    mix(&from, &middle, p * 2.0)
                } else {
                    mix(&middle, &to, p * 2.0 - 1.0)
                };
                composite_layer(ctx, &blended)?;
            }
            TransitionKind::Wipe { direction } => {
                let (w, h) = (f64::from(size.0), f64::from(size.1));
                let mask = match direction {
                    WipeDirection::LeftToRight => Rect::new(0.0, 0.0, p * w, h),
                    WipeDirection::RightToLeft => Rect::new((1.0 - p) * w, 0.0, w, h),
                    WipeDirection::TopToBottom => Rect::new(0.0, 0.0, w, p * h),
                    WipeDirection::BottomToTop => Rect::new(0.0, (1.0 - p) * h, w, h),
                };
                let inverse = match direction {
                    WipeDirection::LeftToRight => Rect::new(p * w, 0.0, w, h),
                    WipeDirection::RightToLeft => Rect::new(0.0, 0.0, (1.0 - p) * w, h),
                    WipeDirection::TopToBottom => Rect::new(0.0, p * h, w, h),
                    WipeDirection::BottomToTop => Rect::new(0.0, 0.0, w, (1.0 - p) * h),
                };
                ctx.push_clip_layer(&inverse.to_path(0.1));
                composite_layer(ctx, &from)?;
                ctx.pop_layer();
                ctx.push_clip_layer(&mask.to_path(0.1));
                composite_layer(ctx, &to)?;
                ctx.pop_layer();
            }
        }
        Ok(())
    }

    fn render_effect_layer(
        &mut self,
        clip: &SceneClip,
        pool: &mut ResourcePool,
        video: &mut dyn FrameSource,
        size: Size,
        transform: Affine,
    ) -> Result<Frame, Error> {
        let (width, height) = check_size(size)?;
        let mut ctx = RenderContext::new_with(
            width,
            height,
            RenderSettings {
                level: Level::fallback(),
                num_threads: 0,
            },
        );
        let mut resources = Resources::new();
        ctx.set_transform(transform);
        self.draw_clip(&mut ctx, &mut resources, size, clip, pool, video)?;
        ctx.flush();
        let mut target = Pixmap::new(width, height);
        ctx.render(&mut target, &mut resources);
        Ok(Frame {
            size,
            pixels: target.take_rgba8(ImageAlphaType::Alpha),
        })
    }

    pub(crate) fn placeholder_frame(
        &mut self,
        asset: &animathio_core::AssetId,
        time: animathio_core::Time,
    ) -> Result<Frame, Error> {
        let size = (320, 180);
        let mut ctx = RenderContext::new_with(
            320,
            180,
            RenderSettings {
                level: Level::fallback(),
                num_threads: 0,
            },
        );
        let mut resources = Resources::new();
        ctx.set_transform(unit_transform(Transform::default(), size)?);
        self.placeholder(
            &mut ctx,
            &mut resources,
            size,
            &format!("VIDEO {}\n{} ticks", &asset.as_str()[..8], time.0),
        )?;
        ctx.flush();
        let mut target = Pixmap::new(320, 180);
        ctx.render(&mut target, &mut resources);
        Ok(Frame {
            size,
            pixels: target.take_rgba8(ImageAlphaType::Alpha),
        })
    }
    #[allow(clippy::cast_possible_truncation)] // Pixel-space font dimensions are bounded below.
    fn text(
        &mut self,
        ctx: &mut RenderContext,
        resources: &mut Resources,
        size: Size,
        text: &str,
        font_size: f64,
        fill: Color,
    ) -> Result<(), Error> {
        let font_size = font_size * f64::from(size.1);
        if !font_size.is_finite() || font_size <= 0.0 || font_size > 1_048_560.0 {
            return Err(Error::Invalid("invalid scene font size"));
        }
        let mut builder = self
            .layout
            .ranged_builder(&mut self.fonts, text, 1.0, false);
        builder.push_default(StyleProperty::FontFamily("DejaVu Sans".into()));
        builder.push_default(StyleProperty::FontSize(font_size as f32));
        let mut layout: Layout<()> = builder.build(text);
        layout.break_all_lines(Some(f32::from(check_size(size)?.0)));
        layout.align(Alignment::Start, AlignmentOptions::default());
        let unit = *ctx.transform();
        ctx.set_transform(
            unit * Affine::scale_non_uniform(1.0 / f64::from(size.0), 1.0 / f64::from(size.1)),
        );
        ctx.set_paint(color(fill));
        for line in layout.lines() {
            for item in line.items() {
                if let PositionedLayoutItem::GlyphRun(run) = item {
                    ctx.glyph_run(resources, run.run().font())
                        .font_size(run.run().font_size())
                        .hint(false)
                        .fill_glyphs(run.positioned_glyphs().map(|g| Glyph {
                            id: g.id,
                            x: g.x,
                            y: g.y,
                        }))
                        .map_err(|e| Error::Glyph(e.to_string()))?;
                }
            }
        }
        ctx.set_transform(unit);
        Ok(())
    }

    fn vector_scene(
        &mut self,
        ctx: &mut RenderContext,
        resources: &mut Resources,
        size: Size,
        scene: &VectorScene,
    ) -> Result<(), Error> {
        for element in &scene.elements {
            match element {
                VectorElement::FillPath { path, color } => {
                    ctx.set_paint(color_core(*color));
                    ctx.fill_path(&vector_path(path));
                }
                VectorElement::StrokePath { path, style } => {
                    ctx.set_paint(color_core(style.color));
                    ctx.set_stroke(vector_stroke(style));
                    ctx.stroke_path(&vector_path(path));
                }
                VectorElement::TextRun(text) => {
                    self.vector_text(ctx, resources, size, text)?;
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::cast_possible_truncation)] // Pixel-space font dimensions are bounded below.
    fn vector_text(
        &mut self,
        ctx: &mut RenderContext,
        resources: &mut Resources,
        size: Size,
        text: &TextElement,
    ) -> Result<(), Error> {
        if !text.font_size.is_finite() || text.font_size <= 0.0 || text.font_size > 1_048_560.0 {
            return Err(Error::Invalid("invalid scene font size"));
        }
        let mut builder = self
            .layout
            .ranged_builder(&mut self.fonts, &text.text, 1.0, false);
        builder.push_default(StyleProperty::FontFamily("DejaVu Sans".into()));
        builder.push_default(StyleProperty::FontSize(text.font_size as f32));
        let mut layout: Layout<()> = builder.build(&text.text);
        layout.break_all_lines(Some(f32::from(check_size(size)?.0)));
        layout.align(Alignment::Start, AlignmentOptions::default());
        let unit = *ctx.transform();
        ctx.set_transform(unit * Affine::translate((text.x, text.y)));
        ctx.set_paint(color_core(text.color));
        for line in layout.lines() {
            for item in line.items() {
                if let PositionedLayoutItem::GlyphRun(run) = item {
                    ctx.glyph_run(resources, run.run().font())
                        .font_size(run.run().font_size())
                        .hint(false)
                        .fill_glyphs(run.positioned_glyphs().map(|g| Glyph {
                            id: g.id,
                            x: g.x,
                            y: g.y,
                        }))
                        .map_err(|e| Error::Glyph(e.to_string()))?;
                }
            }
        }
        ctx.set_transform(unit);
        Ok(())
    }

    fn placeholder(
        &mut self,
        ctx: &mut RenderContext,
        resources: &mut Resources,
        size: Size,
        label: &str,
    ) -> Result<(), Error> {
        for y in 0..6 {
            for x in 0..10 {
                let shade = if (x + y) % 2 == 0 { 70 } else { 110 };
                ctx.set_paint(color([shade, shade, shade, 255]));
                ctx.fill_rect(&Rect::new(
                    f64::from(x) / 10.0,
                    f64::from(y) / 6.0,
                    f64::from(x + 1) / 10.0,
                    f64::from(y + 1) / 6.0,
                ));
            }
        }
        ctx.set_paint(color([0, 0, 0, 200]));
        ctx.fill_rect(&Rect::new(0.0, 0.25, 1.0, 0.75));
        let unit = *ctx.transform();
        ctx.set_transform(unit * Affine::translate((0.04, 0.3)));
        self.text(ctx, resources, size, label, 0.12, [255; 4])?;
        ctx.set_transform(unit);
        Ok(())
    }
}
