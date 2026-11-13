use crate::{Backend, Error, Frame, FrameSource, ResourcePool, Size, check_size};
use animathio_core::{
    ClipKind, Color as CoreColor, PathVerb, Scene, SceneClip, SceneTransition, StrokeCap,
    StrokeJoin, StrokeStyle, TextElement, Transform, TransitionKind, TransitionRole, VectorElement,
    VectorScene, WipeDirection,
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
    kurbo::{Affine, BezPath, Cap, Join, Rect, Shape as _, Stroke},
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
            // and normalized vector scenes.
            let transform = match &clip.kind {
                ClipKind::Vector(scene)
                    if scene.coordinate_space == animathio_core::CoordinateSpace::Canvas =>
                {
                    pixel_transform(clip.transform, size)?
                }
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
                crate::effects::apply_chain(&mut layer, clip, pool)?;
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

/// One shaped glyph run: font, pixel size and `(glyph id, x, y)` positions.
pub(crate) struct ShapedRun {
    pub(crate) font: parley::FontData,
    pub(crate) font_size: f32,
    pub(crate) glyphs: Vec<(u32, f32, f32)>,
}

/// Convert a normalized drawing font size into pixels.
pub(crate) fn relative_text_pixel_size(font_size: f64, size: Size) -> Result<f64, Error> {
    let font_size = font_size * f64::from(size.1);
    if !font_size.is_finite() || font_size <= 0.0 || font_size > 1_048_560.0 {
        return Err(Error::Invalid("invalid scene font size"));
    }
    Ok(font_size)
}

pub(crate) fn check_vector_font_size(font_size: f64) -> Result<(), Error> {
    if !font_size.is_finite() || font_size <= 0.0 || font_size > 1_048_560.0 {
        return Err(Error::Invalid("invalid scene font size"));
    }
    Ok(())
}

fn color(value: [u8; 4]) -> AlphaColor<Srgb> {
    AlphaColor::from_rgba8(value[0], value[1], value[2], value[3])
}

pub(crate) fn unit_transform(t: Transform, size: Size) -> Result<Affine, Error> {
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
pub(crate) fn pixel_transform(t: Transform, size: Size) -> Result<Affine, Error> {
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

pub(crate) fn vector_path(path: &[PathVerb]) -> BezPath {
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

pub(crate) fn vector_stroke(style: &StrokeStyle) -> Stroke {
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
            ClipKind::Vector(scene)
                if scene.coordinate_space == animathio_core::CoordinateSpace::Canvas =>
            {
                pixel_transform(clip.transform, size)?
            }
            _ => unit_transform(clip.transform, size)?,
        };
        let mut layer = self.render_effect_layer(clip, pool, video, size, transform)?;
        crate::effects::apply_chain(&mut layer, clip, pool)?;
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

    pub(crate) fn render_effect_layer(
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
    /// Shape `text` with the embedded font. Shared by the CPU and GPU backends so both
    /// position identical glyphs.
    pub(crate) fn shape(
        &mut self,
        text: &str,
        font_size: f32,
        size: Size,
    ) -> Result<Vec<ShapedRun>, Error> {
        self.shape_aligned(text, font_size, size, animathio_core::TextAlignment::Left)
    }

    pub(crate) fn shape_aligned(
        &mut self,
        text: &str,
        font_size: f32,
        size: Size,
        align: animathio_core::TextAlignment,
    ) -> Result<Vec<ShapedRun>, Error> {
        let mut builder = self
            .layout
            .ranged_builder(&mut self.fonts, text, 1.0, false);
        builder.push_default(StyleProperty::FontFamily("DejaVu Sans".into()));
        builder.push_default(StyleProperty::FontSize(font_size));
        let mut layout: Layout<()> = builder.build(text);
        layout.break_all_lines(Some(f32::from(check_size(size)?.0)));
        layout.align(
            match align {
                animathio_core::TextAlignment::Left => Alignment::Start,
                animathio_core::TextAlignment::Center => Alignment::Center,
                animathio_core::TextAlignment::Right => Alignment::End,
            },
            AlignmentOptions::default(),
        );
        let mut runs = Vec::new();
        for line in layout.lines() {
            for item in line.items() {
                if let PositionedLayoutItem::GlyphRun(run) = item {
                    runs.push(ShapedRun {
                        font: run.run().font().clone(),
                        font_size: run.run().font_size(),
                        glyphs: run.positioned_glyphs().map(|g| (g.id, g.x, g.y)).collect(),
                    });
                }
            }
        }
        Ok(runs)
    }

    fn fill_runs(
        ctx: &mut RenderContext,
        resources: &mut Resources,
        runs: &[ShapedRun],
    ) -> Result<(), Error> {
        for run in runs {
            ctx.glyph_run(resources, &run.font)
                .font_size(run.font_size)
                .hint(false)
                .fill_glyphs(run.glyphs.iter().map(|&(id, x, y)| Glyph { id, x, y }))
                .map_err(|e| Error::Glyph(e.to_string()))?;
        }
        Ok(())
    }

    #[allow(clippy::cast_possible_truncation)] // Pixel-space font dimensions are bounded below.
    fn text(
        &mut self,
        ctx: &mut RenderContext,
        resources: &mut Resources,
        size: Size,
        text: &str,
        font_size: f64,
        fill: [u8; 4],
    ) -> Result<(), Error> {
        self.normalized_text(
            ctx,
            resources,
            size,
            &TextElement {
                text: text.into(),
                font_size,
                x: 0.0,
                y: 0.0,
                color: CoreColor::rgba(fill[0], fill[1], fill[2], fill[3]),
                align: animathio_core::TextAlignment::Left,
            },
        )
    }

    #[allow(clippy::cast_possible_truncation)]
    fn normalized_text(
        &mut self,
        ctx: &mut RenderContext,
        resources: &mut Resources,
        size: Size,
        text: &TextElement,
    ) -> Result<(), Error> {
        let font_size = relative_text_pixel_size(text.font_size, size)?;
        let runs = self.shape_aligned(&text.text, font_size as f32, size, text.align)?;
        let unit = *ctx.transform();
        ctx.set_transform(
            unit * Affine::scale_non_uniform(1.0 / f64::from(size.0), 1.0 / f64::from(size.1)),
        );
        ctx.set_paint(color_core(text.color));
        Self::fill_runs(ctx, resources, &runs)?;
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
                    if scene.coordinate_space == animathio_core::CoordinateSpace::Normalized {
                        let t = *ctx.transform();
                        ctx.set_transform(t * Affine::translate((text.x, text.y)));
                        self.normalized_text(ctx, resources, size, text)?;
                        ctx.set_transform(t);
                    } else {
                        self.vector_text(ctx, resources, size, text)?;
                    }
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
        check_vector_font_size(text.font_size)?;
        let runs = self.shape_aligned(&text.text, text.font_size as f32, size, text.align)?;
        let unit = *ctx.transform();
        ctx.set_transform(unit * Affine::translate((text.x, text.y)));
        ctx.set_paint(color_core(text.color));
        Self::fill_runs(ctx, resources, &runs)?;
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
