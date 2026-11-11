use crate::{Backend, Error, Frame, FrameSource, ResourcePool, Size, check_size};
use animathio_core::{
    ClipKind, Scene, Transform,
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
    kurbo::{Affine, BezPath, Ellipse, Rect, Shape as _, Stroke},
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
        for clip in &scene.clips {
            if matches!(clip.kind, ClipKind::Audio(_)) || clip.transform.opacity == 0.0 {
                continue;
            }
            let transform = unit_transform(clip.transform, size)?;
            // Singular transforms have no visible area and cannot be inverted for image paints.
            if clip.transform.scale_x == 0.0 || clip.transform.scale_y == 0.0 {
                continue;
            }
            ctx.set_transform(transform);
            #[allow(clippy::cast_possible_truncation)] // Core validates opacity in [0, 1].
            ctx.push_opacity_layer(clip.transform.opacity as f32);
            match &clip.kind {
                ClipKind::Image(id) => draw_image(&mut ctx, pool.image(id)?.as_ref())?,
                ClipKind::Video(id) => {
                    if let Some(frame) = video.frame_at(id, clip.source_time) {
                        draw_image(&mut ctx, &frame)?;
                    } else {
                        self.placeholder(
                            &mut ctx,
                            &mut resources,
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
                    self.text(&mut ctx, &mut resources, size, text, *font_size, *color)?;
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
                ClipKind::Plugin { plugin_id, .. } => self.placeholder(
                    &mut ctx,
                    &mut resources,
                    size,
                    &format!("PLUGIN\n{plugin_id}"),
                )?,
                ClipKind::Audio(_) => {}
            }
            ctx.pop_layer();
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
    if ![t.x, t.y, t.scale_x, t.scale_y, t.rotation, t.opacity]
        .iter()
        .all(|v| v.is_finite())
        || !(0.0..=1.0).contains(&t.opacity)
    {
        return Err(Error::Invalid("invalid scene transform"));
    }
    let width = f64::from(size.0);
    let height = f64::from(size.1);
    let transform = Affine::translate((t.x * width, t.y * height))
        * Affine::rotate(t.rotation)
        * Affine::scale_non_uniform(t.scale_x * width, t.scale_y * height)
        * Affine::translate((-0.5, -0.5));
    if !transform.as_coeffs().iter().all(|c| c.is_finite()) {
        return Err(Error::Invalid("scene transform overflow"));
    }
    Ok(transform)
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
