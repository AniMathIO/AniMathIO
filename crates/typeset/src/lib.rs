//! Host-side math typesetting for `AniMathIO` 2.0.
//!
//! The editor's plugin sandbox cannot reasonably link a full typesetting
//! engine, so math is rendered by the host as a service. This crate wraps
//! [Typst](https://typst.app) as a library and [MiTeX](https://github.com/mitex-rs/mitex)
//! to convert LaTeX math to Typst math, then converts the laid-out frame into
//! pure vector path outlines plus a tight bounding box. A bundled OpenType math
//! font (STIX Two Math, OFL) supplies the glyphs.

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use kurbo::Affine;
use lru::LruCache;
use thiserror::Error;
use ttf_parser::OutlineBuilder;
use typst::compile;
use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration as TypstDuration};
use typst::layout::{Frame, FrameItem, Transform};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::visualize::{CurveItem, Geometry};
use typst::{LibraryExt, World};
use typst_layout::PagedDocument;

/// The bundled math font (STIX Two Math, SIL OFL 1.1).
const MATH_FONT: &[u8] = include_bytes!("../fonts/STIXTwoMath-Regular.otf");

/// Maximum UTF-8 bytes accepted for a single math source expression.
const MAX_SOURCE_BYTES: usize = 4 * 1024;
/// Maximum accepted font size in canvas pixels.
const MAX_FONT_SIZE: f32 = 1_000.0;
/// How long a single typesetting call may take before it is abandoned.
const TYPESET_TIMEOUT: Duration = Duration::from_secs(2);
/// Number of cached typesetting results kept in memory.
const CACHE_CAPACITY: usize = 512;

/// A 2D point in canvas pixel coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TypesetPoint {
    /// Horizontal coordinate.
    pub x: f64,
    /// Vertical coordinate.
    pub y: f64,
}

/// A path segment command in canvas pixel coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TypesetVerb {
    /// Move the cursor to a point without drawing.
    MoveTo(TypesetPoint),
    /// Draw a straight line to a point.
    LineTo(TypesetPoint),
    /// Draw a quadratic Bézier curve.
    QuadTo(TypesetPoint, TypesetPoint),
    /// Draw a cubic Bézier curve.
    CubicTo(TypesetPoint, TypesetPoint, TypesetPoint),
    /// Close the current contour.
    Close,
}

/// A single contour (glyph outline or rule/line) as a sequence of verbs.
pub type TypesetPath = Vec<TypesetVerb>;

/// A tight bounding box in canvas pixel coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TypesetBBox {
    /// Left edge.
    pub min_x: f64,
    /// Top edge.
    pub min_y: f64,
    /// Right edge.
    pub max_x: f64,
    /// Bottom edge.
    pub max_y: f64,
}

/// Result of one typesetting call.
#[derive(Clone, Debug, PartialEq)]
pub struct TypesetResult {
    /// Ordered glyph and rule contours.
    pub paths: Vec<TypesetPath>,
    /// Bounding box of all paths.
    pub bbox: TypesetBBox,
}

/// Input language of a math expression.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MathMode {
    /// LaTeX math, converted to Typst with `MiTeX` before layout.
    Latex,
    /// Typst math markup laid out directly.
    Typst,
}

impl MathMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Latex => "latex",
            Self::Typst => "typst",
        }
    }
}

/// Errors raised while validating, converting or laying out math.
#[derive(Debug, Error)]
pub enum TypesetError {
    /// The source or font size exceeded a host safety limit.
    #[error("typesetting input rejected: {0}")]
    InvalidInput(&'static str),
    /// The LaTeX source was malformed.
    #[error("invalid LaTeX: {0}")]
    InvalidLatex(String),
    /// The Typst compiler reported one or more errors.
    #[error("typesetting failed: {0}")]
    Compile(String),
    /// Typesetting exceeded the host time limit.
    #[error("typesetting timed out after {} ms", TYPESET_TIMEOUT.as_millis())]
    Timeout,
    /// The expression produced no visible outlines.
    #[error("typesetting produced no visible outlines")]
    Empty,
    /// An internal error that should never reach users.
    #[error("typesetting internal error: {0}")]
    Internal(String),
}

/// Bundled font bytes plus the shared Typst library and font book.
struct SharedData {
    library: LazyHash<typst::Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
}

/// A minimal [`World`] exposing a single source file and the bundled math font.
struct MathWorld {
    shared: Arc<SharedData>,
    main_id: FileId,
    source: Source,
    file_bytes: Bytes,
}

impl MathWorld {
    fn new(shared: Arc<SharedData>, source: String) -> Self {
        let main_id = FileId::new(RootedPath::new(
            VirtualRoot::Project,
            VirtualPath::new("main.typ").expect("static path is valid"),
        ));
        let file_bytes = Bytes::from_string(source.clone());
        let source = Source::new(main_id, source);
        Self {
            shared,
            main_id,
            source,
            file_bytes,
        }
    }
}

impl World for MathWorld {
    fn library(&self) -> &LazyHash<typst::Library> {
        &self.shared.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.shared.book
    }

    fn main(&self) -> FileId {
        self.main_id
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main_id {
            Ok(self.source.clone())
        } else {
            Err(FileError::NotFound(PathBuf::new()))
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if id == self.main_id {
            Ok(self.file_bytes.clone())
        } else {
            Err(FileError::NotFound(PathBuf::new()))
        }
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.shared.fonts.get(index).cloned()
    }

    fn today(&self, _offset: Option<TypstDuration>) -> Option<Datetime> {
        None
    }
}

/// Caching, thread-safe math typesetter intended to be shared across plugin stores.
pub struct TypesetEngine {
    shared: Arc<SharedData>,
    cache: Mutex<LruCache<[u8; 32], TypesetResult>>,
}

impl TypesetEngine {
    /// Load the bundled math font and construct the Typst library.
    pub fn new() -> Result<Self, TypesetError> {
        let font = Font::new(Bytes::new(MATH_FONT), 0).ok_or_else(|| {
            TypesetError::Internal("failed to parse bundled math font".to_string())
        })?;

        let mut book = FontBook::new();
        book.push(font.info().clone());

        let shared = Arc::new(SharedData {
            library: LazyHash::new(typst::Library::default()),
            book: LazyHash::new(book),
            fonts: vec![font],
        });

        let cache = Mutex::new(LruCache::new(
            NonZeroUsize::new(CACHE_CAPACITY).unwrap_or(NonZeroUsize::MIN),
        ));

        Ok(Self { shared, cache })
    }

    /// Number of cached results currently retained.
    pub fn cache_len(&self) -> usize {
        self.cache.lock().map_or(0, |cache| cache.len())
    }

    /// Typeset a math expression into vector paths, caching by source, mode and size.
    pub fn typeset(
        &self,
        source: &str,
        mode: MathMode,
        font_size: f32,
    ) -> Result<TypesetResult, TypesetError> {
        validate_input(source, font_size)?;

        let cache_key = cache_key(source, mode, font_size);
        if let Some(cached) = self
            .cache
            .lock()
            .ok()
            .and_then(|mut cache| cache.get(&cache_key).cloned())
        {
            return Ok(cached);
        }

        // Typesetting is heavy and runs untrusted input; run it on a worker
        // thread and abandon the call (without blocking the host) if it exceeds
        // the time budget. The input length cap keeps any abandoned worker
        // short-lived.
        let (tx, rx) = mpsc::channel();
        let shared = Arc::clone(&self.shared);
        let source = source.to_owned();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                typeset_inner(&shared, &source, mode, font_size)
            }));
            let _ = tx.send(result);
        });

        let result = match rx.recv_timeout(TYPESET_TIMEOUT) {
            Ok(Ok(Ok(result))) => result,
            Ok(Ok(Err(err))) => return Err(err),
            Ok(Err(_)) => {
                return Err(TypesetError::Internal(
                    "typesetting worker panicked".to_string(),
                ));
            }
            Err(RecvTimeoutError::Timeout) => return Err(TypesetError::Timeout),
            Err(RecvTimeoutError::Disconnected) => {
                return Err(TypesetError::Internal(
                    "typesetting worker disconnected".to_string(),
                ));
            }
        };

        if let Ok(mut cache) = self.cache.lock() {
            cache.put(cache_key, result.clone());
        }

        Ok(result)
    }
}

/// Reject inputs that could take disproportionate time or memory to typeset.
fn validate_input(source: &str, font_size: f32) -> Result<(), TypesetError> {
    if source.is_empty() || source.trim().is_empty() {
        return Err(TypesetError::InvalidInput("source is empty"));
    }
    if source.len() > MAX_SOURCE_BYTES {
        return Err(TypesetError::InvalidInput("source is too long"));
    }
    if !font_size.is_finite() || font_size <= 0.0 || font_size > MAX_FONT_SIZE {
        return Err(TypesetError::InvalidInput("font size out of range"));
    }
    Ok(())
}

fn cache_key(source: &str, mode: MathMode, font_size: f32) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(source.as_bytes());
    hasher.update(mode.as_str().as_bytes());
    hasher.update(&font_size.to_bits().to_le_bytes());
    *hasher.finalize().as_bytes()
}

fn typeset_inner(
    shared: &SharedData,
    source: &str,
    mode: MathMode,
    font_size: f32,
) -> Result<TypesetResult, TypesetError> {
    let typst_source = build_typst_source(source, mode, font_size)?;
    let world = MathWorld::new(
        Arc::new(SharedData {
            library: shared.library.clone(),
            book: shared.book.clone(),
            fonts: shared.fonts.clone(),
        }),
        typst_source,
    );

    let warned = compile::<PagedDocument>(&world);
    let document = warned
        .output
        .map_err(|errors| TypesetError::Compile(join_diagnostics(&errors)))?;

    let page = document.pages().first().ok_or(TypesetError::Empty)?;

    let mut paths = Vec::new();
    walk_frame(&page.frame, Affine::IDENTITY, &mut paths);
    if paths.is_empty() {
        return Err(TypesetError::Empty);
    }

    Ok(TypesetResult {
        bbox: path_bbox(&paths),
        paths,
    })
}

fn join_diagnostics(errors: &[typst::diag::SourceDiagnostic]) -> String {
    let mut parts = Vec::with_capacity(errors.len());
    for error in errors {
        parts.push(error.message.to_string());
    }
    parts.join("; ")
}

fn build_typst_source(
    source: &str,
    mode: MathMode,
    font_size: f32,
) -> Result<String, TypesetError> {
    let math = match mode {
        MathMode::Latex => {
            validate_latex(source)?;
            mitex::convert_math(source, None).map_err(TypesetError::InvalidLatex)?
        }
        MathMode::Typst => strip_math_delimiters(source.trim()).to_string(),
    };

    Ok(format!(
        "#set page(width: auto, height: auto, margin: 0pt)\n\
         #set text(size: {font_size}pt)\n\
         #show math.equation: set text(font: \"STIX Two Math\")\n\
         $ {math} $"
    ))
}

/// Strip one pair of surrounding `$…$` delimiters if present, for convenience.
fn strip_math_delimiters(source: &str) -> &str {
    if source.starts_with('$') && source.ends_with('$') && source.len() >= 2 {
        &source[1..source.len() - 1]
    } else {
        source
    }
}

/// Very small LaTeX sanity check: reject unbalanced braces before `MiTeX`'s
/// permissive parser turns them into surprising output.
fn validate_latex(source: &str) -> Result<(), TypesetError> {
    let mut depth: i64 = 0;
    let mut escaped = false;

    for ch in source.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth < 0 {
                    return Err(TypesetError::InvalidLatex(
                        "unbalanced closing brace".to_string(),
                    ));
                }
            }
            _ => {}
        }
    }

    if depth != 0 {
        return Err(TypesetError::InvalidLatex(
            "unbalanced opening brace".to_string(),
        ));
    }
    Ok(())
}

/// Collector used to pull glyph outlines out of `ttf-parser`.
#[derive(Default)]
struct GlyphCollector {
    paths: Vec<TypesetPath>,
    current: TypesetPath,
}

impl GlyphCollector {
    fn finish(mut self) -> Vec<TypesetPath> {
        if !self.current.is_empty() {
            self.paths.push(std::mem::take(&mut self.current));
        }
        self.paths
    }
}

impl OutlineBuilder for GlyphCollector {
    fn move_to(&mut self, x: f32, y: f32) {
        if !self.current.is_empty() {
            self.paths.push(std::mem::take(&mut self.current));
        }
        self.current.push(TypesetVerb::MoveTo(TypesetPoint {
            x: f64::from(x),
            y: f64::from(y),
        }));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.current.push(TypesetVerb::LineTo(TypesetPoint {
            x: f64::from(x),
            y: f64::from(y),
        }));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.current.push(TypesetVerb::QuadTo(
            TypesetPoint {
                x: f64::from(x1),
                y: f64::from(y1),
            },
            TypesetPoint {
                x: f64::from(x),
                y: f64::from(y),
            },
        ));
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.current.push(TypesetVerb::CubicTo(
            TypesetPoint {
                x: f64::from(x1),
                y: f64::from(y1),
            },
            TypesetPoint {
                x: f64::from(x2),
                y: f64::from(y2),
            },
            TypesetPoint {
                x: f64::from(x),
                y: f64::from(y),
            },
        ));
    }

    fn close(&mut self) {
        self.current.push(TypesetVerb::Close);
        if !self.current.is_empty() {
            self.paths.push(std::mem::take(&mut self.current));
        }
    }
}

/// Convert a Typst [`Transform`] into the same affine matrix in kurbo form.
fn affine_of(transform: Transform) -> Affine {
    Affine::new([
        transform.sx.get(),
        transform.ky.get(),
        transform.kx.get(),
        transform.sy.get(),
        transform.tx.to_pt(),
        transform.ty.to_pt(),
    ])
}

/// Recursively convert a laid-out Typst frame into path contours.
fn walk_frame(frame: &Frame, state: Affine, out: &mut Vec<TypesetPath>) {
    for (pos, item) in frame.items() {
        let state = state * Affine::translate((pos.x.to_pt(), pos.y.to_pt()));
        match item {
            FrameItem::Group(group) => {
                let state = state * affine_of(group.transform);
                walk_frame(&group.frame, state, out);
            }
            FrameItem::Text(text) => walk_text(text, state, out),
            FrameItem::Shape(shape, _) => walk_shape(&shape.geometry, state, out),
            FrameItem::Image(_, _, _) | FrameItem::Link(_, _) | FrameItem::Tag(_) => {}
        }
    }
}

fn walk_text(text: &typst::text::TextItem, state: Affine, out: &mut Vec<TypesetPath>) {
    let units_per_em = f64::from(text.font.ttf().units_per_em());
    if units_per_em <= 0.0 {
        return;
    }
    let scale = text.size.to_pt() / units_per_em;
    // Font outlines are y-up while frames are y-down; flip once for the run.
    let text_state = state * Affine::FLIP_Y;

    let mut cursor_x = 0.0_f64;
    let mut cursor_y = 0.0_f64;
    for glyph in &text.glyphs {
        let offset_x = cursor_x + glyph.x_offset.at(text.size).to_pt();
        let offset_y = cursor_y + glyph.y_offset.at(text.size).to_pt();
        let glyph_state = text_state * Affine::translate((offset_x, offset_y));

        let mut collector = GlyphCollector::default();
        let outlined = text
            .font
            .ttf()
            .outline_glyph(ttf_parser::GlyphId(glyph.id), &mut collector)
            .is_some();
        if outlined {
            for path in collector.finish() {
                out.push(transform_path(&path, glyph_state, scale));
            }
        }

        cursor_x += glyph.x_advance.at(text.size).to_pt();
        cursor_y += glyph.y_advance.at(text.size).to_pt();
    }
}

fn transform_path(path: &[TypesetVerb], state: Affine, scale: f64) -> TypesetPath {
    path.iter()
        .map(|verb| match *verb {
            TypesetVerb::MoveTo(point) => TypesetVerb::MoveTo(apply(state, point, scale)),
            TypesetVerb::LineTo(point) => TypesetVerb::LineTo(apply(state, point, scale)),
            TypesetVerb::QuadTo(control, point) => {
                TypesetVerb::QuadTo(apply(state, control, scale), apply(state, point, scale))
            }
            TypesetVerb::CubicTo(control1, control2, point) => TypesetVerb::CubicTo(
                apply(state, control1, scale),
                apply(state, control2, scale),
                apply(state, point, scale),
            ),
            TypesetVerb::Close => TypesetVerb::Close,
        })
        .collect()
}

fn apply(state: Affine, point: TypesetPoint, scale: f64) -> TypesetPoint {
    let mapped = state * kurbo::Point::new(point.x * scale, point.y * scale);
    TypesetPoint {
        x: mapped.x,
        y: mapped.y,
    }
}

fn walk_shape(geometry: &Geometry, state: Affine, out: &mut Vec<TypesetPath>) {
    let mut path = Vec::new();
    match geometry {
        Geometry::Line(end) => {
            path.push(TypesetVerb::MoveTo(TypesetPoint::default()));
            path.push(TypesetVerb::LineTo(TypesetPoint {
                x: end.x.to_pt(),
                y: end.y.to_pt(),
            }));
        }
        Geometry::Rect(size) => {
            path.push(TypesetVerb::MoveTo(TypesetPoint::default()));
            path.push(TypesetVerb::LineTo(TypesetPoint {
                x: size.x.to_pt(),
                y: 0.0,
            }));
            path.push(TypesetVerb::LineTo(TypesetPoint {
                x: size.x.to_pt(),
                y: size.y.to_pt(),
            }));
            path.push(TypesetVerb::LineTo(TypesetPoint {
                x: 0.0,
                y: size.y.to_pt(),
            }));
            path.push(TypesetVerb::Close);
        }
        Geometry::Curve(curve) => {
            for item in &curve.0 {
                match item {
                    CurveItem::Move(point) => {
                        path.push(TypesetVerb::MoveTo(TypesetPoint {
                            x: point.x.to_pt(),
                            y: point.y.to_pt(),
                        }));
                    }
                    CurveItem::Line(point) => {
                        path.push(TypesetVerb::LineTo(TypesetPoint {
                            x: point.x.to_pt(),
                            y: point.y.to_pt(),
                        }));
                    }
                    CurveItem::Cubic(control1, control2, point) => {
                        path.push(TypesetVerb::CubicTo(
                            TypesetPoint {
                                x: control1.x.to_pt(),
                                y: control1.y.to_pt(),
                            },
                            TypesetPoint {
                                x: control2.x.to_pt(),
                                y: control2.y.to_pt(),
                            },
                            TypesetPoint {
                                x: point.x.to_pt(),
                                y: point.y.to_pt(),
                            },
                        ));
                    }
                    CurveItem::Close => path.push(TypesetVerb::Close),
                }
            }
        }
    }

    if !path.is_empty() {
        out.push(transform_path(&path, state, 1.0));
    }
}

/// Bounding box over every verb's control and end points.
fn path_bbox(paths: &[TypesetPath]) -> TypesetBBox {
    let mut bbox = TypesetBBox {
        min_x: f64::INFINITY,
        min_y: f64::INFINITY,
        max_x: f64::NEG_INFINITY,
        max_y: f64::NEG_INFINITY,
    };

    for path in paths {
        for verb in path {
            match *verb {
                TypesetVerb::MoveTo(point) | TypesetVerb::LineTo(point) => {
                    bbox.min_x = bbox.min_x.min(point.x);
                    bbox.min_y = bbox.min_y.min(point.y);
                    bbox.max_x = bbox.max_x.max(point.x);
                    bbox.max_y = bbox.max_y.max(point.y);
                }
                TypesetVerb::QuadTo(control, point) => {
                    for point in [control, point] {
                        bbox.min_x = bbox.min_x.min(point.x);
                        bbox.min_y = bbox.min_y.min(point.y);
                        bbox.max_x = bbox.max_x.max(point.x);
                        bbox.max_y = bbox.max_y.max(point.y);
                    }
                }
                TypesetVerb::CubicTo(control1, control2, point) => {
                    for point in [control1, control2, point] {
                        bbox.min_x = bbox.min_x.min(point.x);
                        bbox.min_y = bbox.min_y.min(point.y);
                        bbox.max_x = bbox.max_x.max(point.x);
                        bbox.max_y = bbox.max_y.max(point.y);
                    }
                }
                TypesetVerb::Close => {}
            }
        }
    }

    bbox
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typeset_latex(source: &str) -> Result<TypesetResult, TypesetError> {
        let engine = TypesetEngine::new()?;
        engine.typeset(source, MathMode::Latex, 48.0)
    }

    #[test]
    fn latex_fractions_integrals_and_identities_produce_paths() {
        for source in [r"\frac{a}{b}", r"\int_0^1 x^2\,dx", r"e^{i\pi}+1=0"] {
            let result = typeset_latex(source).unwrap();
            assert!(!result.paths.is_empty(), "{source} produced no paths");
            assert!(
                result.bbox.min_x < result.bbox.max_x,
                "{source} has empty width"
            );
            assert!(
                result.bbox.min_y < result.bbox.max_y,
                "{source} has empty height"
            );
            assert!(result.bbox.min_x.is_finite() && result.bbox.max_x.is_finite());
            assert!(result.bbox.min_y.is_finite() && result.bbox.max_y.is_finite());
        }
    }

    #[test]
    fn typst_mode_matches_latex_style_output() {
        let engine = TypesetEngine::new().unwrap();
        let result = engine.typeset("frac(a,b)", MathMode::Typst, 48.0).unwrap();
        assert!(!result.paths.is_empty(), "typst output is empty");
        assert!(result.bbox.min_x < result.bbox.max_x);
    }

    #[test]
    fn invalid_latex_is_a_clear_error() {
        let err = typeset_latex(r"\frac{a}{b").unwrap_err();
        assert!(matches!(err, TypesetError::InvalidLatex(_)), "got {err}");
    }

    #[test]
    fn oversized_input_is_rejected() {
        let engine = TypesetEngine::new().unwrap();
        let source = "x".repeat(MAX_SOURCE_BYTES + 1);
        let err = engine.typeset(&source, MathMode::Typst, 48.0).unwrap_err();
        assert!(matches!(err, TypesetError::InvalidInput(_)));
    }

    #[test]
    fn results_are_cached_by_source_mode_and_size() {
        let engine = TypesetEngine::new().unwrap();
        let first = engine
            .typeset(r"\frac{a}{b}", MathMode::Latex, 48.0)
            .unwrap();
        let second = engine
            .typeset(r"\frac{a}{b}", MathMode::Latex, 48.0)
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(engine.cache_len(), 1);
    }
}
