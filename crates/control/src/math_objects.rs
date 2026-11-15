//! Typed mathematical-object authoring shared by the editor and automation.
use crate::{ClipAdded, ControlError, place_visual_clip};
use animathio_core::{Clip, ClipId, ClipKind, Command, ParamValue, Project, Time, TimeRange};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Discoverable object kinds. Geometry uses the shapes plugin; plots use graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MathObjectKind {
    /// Typeset formula.
    Latex,
    /// Plain text.
    Text,
    /// Filled point.
    Point,
    /// Line segment.
    Line,
    /// Vector with arrow head.
    Vector,
    /// Arbitrary polygon vertices.
    Polygon,
    /// Function y = f(x).
    Plot,
    /// Circle.
    Circle,
    /// Ellipse.
    Ellipse,
    /// Cartesian axes and grid.
    Cartesian,
    /// Polar grid.
    Polar,
}
impl MathObjectKind {
    /// Library order.
    pub const ALL: [Self; 11] = [
        Self::Latex,
        Self::Text,
        Self::Point,
        Self::Line,
        Self::Vector,
        Self::Polygon,
        Self::Plot,
        Self::Circle,
        Self::Ellipse,
        Self::Cartesian,
        Self::Polar,
    ];
    /// Human-facing name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Latex => "LaTeX",
            Self::Text => "Text",
            Self::Point => "Point",
            Self::Line => "Line segment",
            Self::Vector => "Vector",
            Self::Polygon => "Polygon",
            Self::Plot => "Plot",
            Self::Circle => "Circle",
            Self::Ellipse => "Ellipse",
            Self::Cartesian => "Cartesian grid",
            Self::Polar => "Polar grid",
        }
    }
}

/// A typed field: numeric values are finite math units, source is ordinary text.
#[derive(Clone, Debug)]
pub struct MathObjectField {
    /// Stable field id.
    pub id: &'static str,
    /// Label with units.
    pub label: &'static str,
    /// Numeric field versus text field.
    pub numeric: bool,
    /// Initial value.
    pub default: &'static str,
}
fn number(id: &'static str, label: &'static str, default: &'static str) -> MathObjectField {
    MathObjectField {
        id,
        label,
        numeric: true,
        default,
    }
}
fn text(id: &'static str, label: &'static str, default: &'static str) -> MathObjectField {
    MathObjectField {
        id,
        label,
        numeric: false,
        default,
    }
}

/// Editable object definition. Kept as plugin metadata, never a format field.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MathObjectDraft {
    /// Object type.
    pub kind: MathObjectKind,
    /// Typed input buffers; validation happens before creating commands.
    pub fields: BTreeMap<String, String>,
    /// Straight-alpha sRGB colour in hex.
    pub color: String,
    /// Apply the animation plugin's Write preset on Add.
    #[serde(default)]
    pub write_on: bool,
}
impl MathObjectDraft {
    /// Default object at the origin of a ten-unit square.
    #[must_use]
    pub fn new(kind: MathObjectKind) -> Self {
        let mut draft = Self {
            kind,
            fields: BTreeMap::new(),
            color: "#76b9ff".into(),
            write_on: false,
        };
        draft.fields = draft
            .specs()
            .into_iter()
            .map(|f| (f.id.into(), f.default.into()))
            .collect();
        if matches!(kind, MathObjectKind::Latex | MathObjectKind::Text) {
            draft.color = "#ffffff".into();
        }
        draft
    }
    /// Fields displayed identically by dialog and inspector.
    #[must_use]
    pub fn specs(&self) -> Vec<MathObjectField> {
        use MathObjectKind as K;
        let center = || {
            vec![
                number("x", "Centre X (units)", "0"),
                number("y", "Centre Y (units)", "0"),
            ]
        };
        match self.kind {
            K::Latex | K::Text => vec![
                text(
                    "source",
                    if self.kind == K::Latex {
                        "LaTeX source"
                    } else {
                        "Text"
                    },
                    if self.kind == K::Latex {
                        r"\frac{a}{b}"
                    } else {
                        "Hello, AniMathIO!"
                    },
                ),
                number(
                    "size",
                    "Font size (% of short edge)",
                    if self.kind == K::Latex { "12" } else { "6" },
                ),
            ],
            K::Point => {
                let mut f = center();
                f.push(number("radius", "Radius (units)", "0.12"));
                f
            }
            K::Circle => {
                let mut f = center();
                f.push(number("radius", "Radius (units)", "2"));
                f
            }
            K::Ellipse => {
                let mut f = center();
                f.extend([
                    number("rx", "Horizontal radius (units)", "3"),
                    number("ry", "Vertical radius (units)", "1.5"),
                ]);
                f
            }
            K::Line | K::Vector => vec![
                number("x1", "Start X (units)", "-2"),
                number("y1", "Start Y (units)", "-1"),
                number("x2", "End X (units)", "2"),
                number("y2", "End Y (units)", "1"),
            ],
            K::Polygon => vec![text(
                "vertices",
                "Vertices (x,y; x,y; ... in units)",
                "-2,-1; 2,-1; 0,2",
            )],
            K::Plot => vec![
                text("expression", "y = f(x)", "sin(x)"),
                number("min", "Minimum X/Y (units)", "-5"),
                number("max", "Maximum X/Y (units)", "5"),
            ],
            K::Cartesian | K::Polar => vec![number("extent", "Grid radius (units, 1–10)", "5")],
        }
    }
    fn num(&self, id: &str) -> Result<f64, ControlError> {
        self.fields
            .get(id)
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|n| n.is_finite() && n.abs() <= 1000.)
            .ok_or_else(|| {
                invalid(format!(
                    "{id}: enter a finite number between -1000 and 1000"
                ))
            })
    }
    fn positive(&self, id: &str) -> Result<f64, ControlError> {
        let n = self.num(id)?;
        if n <= 0. {
            return Err(invalid(format!("{id} must be positive")));
        }
        Ok(n)
    }
    fn source(&self, id: &str) -> Result<&str, ControlError> {
        self.fields
            .get(id)
            .map(String::as_str)
            .filter(|s| !s.trim().is_empty() && s.len() <= 32_768)
            .ok_or_else(|| invalid(format!("{id}: enter text (up to 32768 bytes)")))
    }
    /// Plugin parameters used by preview, Add and Apply.
    // Keep the small object-specific mappings together so their coordinate convention is visible.
    #[allow(clippy::too_many_lines)]
    pub fn parameters(&self, size: (u32, u32)) -> Result<(String, Value), ControlError> {
        use MathObjectKind as K;
        if let Some(key) = self
            .fields
            .keys()
            .find(|key| !self.specs().iter().any(|s| s.id == key.as_str()))
        {
            return Err(invalid(format!("unknown field {key}")));
        }
        let color = crate::parse_caption_color(&self.color)?;
        let short = f64::from(size.0.min(size.1).max(1));
        let sx = short / f64::from(size.0.max(1)) * 0.1;
        let sy = short / f64::from(size.1.max(1)) * 0.1;
        let point = |x: f64, y: f64| [0.5 + x * sx, 0.5 - y * sy];
        let mut path = Vec::new();
        let mut params = json!({"units":"normalized", "shape":"path", "fill":[0,0,0,0], "stroke":color, "stroke_width":short / f64::from(size.0.max(1)) * 0.003});
        let mut plugin = "org.animathio.shapes";
        match self.kind {
            K::Latex | K::Text => {
                let source = self.source("source")?;
                let font_size = self.positive("size")?;
                if font_size > 40. {
                    return Err(invalid("font size must be at most 40%"));
                }
                plugin = if self.kind == K::Latex {
                    "org.animathio.latex"
                } else {
                    "org.animathio.text"
                };
                params = if self.kind == K::Latex {
                    json!({"source":source,"mode":"latex","size":font_size,"units":"percent","color":color,"reveal":1.})
                } else {
                    json!({"text":source,"font_size":font_size / 100. * short / f64::from(size.1.max(1)),"units":"normalized","fit_width":1.,"fit_height":1.,"color":color})
                };
            }
            K::Point | K::Circle | K::Ellipse => {
                let p = point(self.num("x")?, self.num("y")?);
                let (rx, ry) = if self.kind == K::Ellipse {
                    (self.positive("rx")?, self.positive("ry")?)
                } else {
                    let r = self.positive("radius")?;
                    (r, r)
                };
                params["shape"] = json!(if self.kind == K::Point {
                    "point"
                } else {
                    "ellipse"
                });
                params["x"] = json!(p[0]);
                params["y"] = json!(p[1]);
                params["width"] = json!(2. * rx * sx);
                params["height"] = json!(2. * ry * sy);
                if self.kind == K::Point {
                    params["fill"] = json!(color);
                }
            }
            K::Line | K::Vector => {
                let (x1, y1, x2, y2) = (
                    self.num("x1")?,
                    self.num("y1")?,
                    self.num("x2")?,
                    self.num("y2")?,
                );
                if (x2 - x1).hypot(y2 - y1) < 0.001 {
                    return Err(invalid("start and end must differ"));
                }
                path.extend([
                    json!({"MoveTo":point(x1,y1)}),
                    json!({"LineTo":point(x2,y2)}),
                ]);
                if self.kind == K::Vector {
                    let angle = (y2 - y1).atan2(x2 - x1);
                    for offset in [-0.45_f64, 0.45] {
                        path.extend([json!({"MoveTo":point(x2 - 0.4*(angle+offset).cos(),y2 - 0.4*(angle+offset).sin())}),json!({"LineTo":point(x2,y2)})]);
                    }
                }
            }
            K::Polygon => {
                let vertices = self
                    .source("vertices")?
                    .split(';')
                    .map(|pair| {
                        let (x, y) = pair.trim().split_once(',').ok_or_else(|| {
                            invalid("vertices need x,y pairs separated by semicolons")
                        })?;
                        let parse = |s: &str| {
                            s.trim()
                                .parse::<f64>()
                                .ok()
                                .filter(|v| v.is_finite() && v.abs() <= 1000.)
                                .ok_or_else(|| invalid("invalid vertex coordinate"))
                        };
                        Ok(point(parse(x)?, parse(y)?))
                    })
                    .collect::<Result<Vec<_>, ControlError>>()?;
                if !(3..=64).contains(&vertices.len()) {
                    return Err(invalid("use between 3 and 64 vertices"));
                }
                for (i, p) in vertices.into_iter().enumerate() {
                    path.push(if i == 0 {
                        json!({"MoveTo":p})
                    } else {
                        json!({"LineTo":p})
                    });
                }
                path.push(json!("Close"));
            }
            K::Plot => {
                let expression = self
                    .source("expression")?
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect::<String>();
                if expression.contains('=') {
                    return Err(invalid("use an expression such as sin(x), without y="));
                }
                let (min, max) = (self.num("min")?, self.num("max")?);
                if min >= max {
                    return Err(invalid("minimum must be less than maximum"));
                }
                plugin = "org.animathio.graph";
                params = json!({"items":format!("plot y={expression}"),"x_min":min,"x_max":max,"y_min":min,"y_max":max,"show_grid":true,"show_axes":true,"show_labels":true,"color":color,"draw":1.});
            }
            K::Cartesian | K::Polar => {
                let extent = self.positive("extent")?;
                if !(1.0..=10.).contains(&extent) {
                    return Err(invalid("grid radius must be between 1 and 10"));
                }
                if self.kind == K::Cartesian {
                    plugin = "org.animathio.graph";
                    params = json!({"items":"", "x_min":-extent,"x_max":extent,"y_min":-extent,"y_max":extent,"show_grid":true,"show_axes":true,"show_labels":true,"color":color,"draw":1.});
                } else {
                    for radius in 1..=10 {
                        let r = f64::from(radius);
                        if r > extent {
                            break;
                        }
                        for step in 0..=96 {
                            let angle = f64::from(step) * std::f64::consts::TAU / 96.;
                            let p = point(r * angle.cos(), r * angle.sin());
                            path.push(if step == 0 {
                                json!({"MoveTo":p})
                            } else {
                                json!({"LineTo":p})
                            });
                        }
                    }
                    for step in 0..12 {
                        let angle = f64::from(step) * std::f64::consts::TAU / 12.;
                        path.extend([
                            json!({"MoveTo":point(0.,0.)}),
                            json!({"LineTo":point(extent*angle.cos(),extent*angle.sin())}),
                        ]);
                    }
                }
                if self.kind == K::Cartesian {
                    params["axis_color"] = json!(color);
                    let mut grid_color = color;
                    grid_color[3] = color[3] / 3;
                    params["grid_color"] = json!(grid_color);
                }
                params["stroke_width"] = json!(short / f64::from(size.0.max(1)) * 0.0015);
            }
        }
        if !path.is_empty() {
            params["path"] = json!(serde_json::to_string(&path)?);
        }
        params["_math_object"] = json!(serde_json::to_string(self)?);
        Ok((plugin.into(), params))
    }
}
fn invalid(message: impl Into<String>) -> ControlError {
    ControlError::InvalidParams(message.into())
}

/// Recover a library object definition, or the effective fields of a legacy LaTeX clip.
#[must_use]
pub fn math_object_draft(clip: &Clip, canvas: (u32, u32)) -> Option<MathObjectDraft> {
    let ClipKind::Plugin { plugin_id, params } = &clip.kind else {
        return None;
    };
    let metadata = match clip.params.get("_math_object") {
        Some(ParamValue::String(v)) => Some(v.as_str()),
        _ => params.get("_math_object").and_then(Value::as_str),
    };
    if let Some(metadata) = metadata {
        let mut draft: MathObjectDraft = serde_json::from_str(metadata).ok()?;
        let source_key = match draft.kind {
            MathObjectKind::Latex => Some("source"),
            MathObjectKind::Text => Some("text"),
            _ => None,
        };
        if let Some(key) = source_key
            && let Some(ParamValue::String(source)) = clip.params.get(key)
        {
            draft.fields.insert("source".into(), source.clone());
        }
        if draft.kind == MathObjectKind::Latex
            && let Some(ParamValue::Number(size)) = clip.params.get("size")
            && draft
                .fields
                .get("size")
                .and_then(|s| s.parse::<f64>().ok())
                .is_none_or(|old| (old - size).abs() > f64::EPSILON)
        {
            draft.fields.insert("size".into(), size.to_string());
        }
        let color_key = if matches!(
            draft.kind,
            MathObjectKind::Latex
                | MathObjectKind::Text
                | MathObjectKind::Plot
                | MathObjectKind::Cartesian
        ) {
            "color"
        } else {
            "stroke"
        };
        if let Some(value @ ParamValue::Color(_)) = clip.params.get(color_key)
            && let Ok(color) = serde_json::from_value::<[u8; 4]>(value.to_plugin_json())
            && crate::parse_caption_color(&draft.color).ok() != Some(color)
        {
            let [r, g, b, a] = color;
            draft.color = format!("#{r:02x}{g:02x}{b:02x}{a:02x}");
        }
        return Some(draft);
    }
    if plugin_id != "org.animathio.latex" {
        return None;
    }
    let string_param = |name: &str| match clip.params.get(name) {
        Some(ParamValue::String(value) | ParamValue::Enum(value)) => Some(value.as_str()),
        _ => params.get(name).and_then(Value::as_str),
    };
    if string_param("mode").is_some_and(|mode| mode != "latex") {
        return None;
    }
    let mut draft = MathObjectDraft::new(MathObjectKind::Latex);
    let source = match clip.params.get("source") {
        Some(ParamValue::String(v)) => v.clone(),
        _ => params.get("source")?.as_str()?.into(),
    };
    draft.fields.insert("source".into(), source);
    let size = match clip.params.get("size") {
        Some(ParamValue::Number(n)) => *n,
        _ => params.get("size").and_then(Value::as_f64).unwrap_or(48.),
    };
    // Translate legacy sizes into the dialog's percentage of the actual short edge.
    let units = string_param("units").unwrap_or("pixels");
    draft.fields.insert(
        "size".into(),
        (match units {
            "percent" => size,
            "normalized" => size * 100.,
            _ => size / f64::from(canvas.0.min(canvas.1).max(1)) * 100.,
        })
        .to_string(),
    );
    let color = match clip.params.get("color") {
        Some(value @ ParamValue::Color(_)) => {
            serde_json::from_value::<[u8; 4]>(value.to_plugin_json()).ok()
        }
        _ => params
            .get("color")
            .and_then(|v| serde_json::from_value(v.clone()).ok()),
    };
    if let Some([r, g, b, a]) = color {
        draft.color = format!("#{r:02x}{g:02x}{b:02x}{a:02x}");
    }
    Some(draft)
}

/// Create or update an object with one invertible batch. Animation presets can be
/// appended by the caller after applying this batch to a temporary project.
pub fn math_object_command(
    project: &Project,
    draft: &MathObjectDraft,
    range: TimeRange,
    edit: Option<ClipId>,
) -> Result<(Command, ClipAdded), ControlError> {
    let size = animathio_export::resolution(project);
    let (plugin_id, params) = draft.parameters(size)?;
    if let Some(id) = edit {
        let (track, clip) = project
            .tracks()
            .iter()
            .find_map(|t| t.clips().get(&id).map(|c| (t.id(), c)))
            .ok_or_else(|| invalid("clip no longer exists"))?;
        if !matches!(&clip.kind, ClipKind::Plugin { plugin_id: p, .. } if p==&plugin_id) {
            return Err(invalid("object type cannot change while editing"));
        }
        let commands = params
            .as_object()
            .ok_or_else(|| invalid("invalid object parameters"))?
            .iter()
            .map(|(key, value)| {
                let value = if let Some(s) = value.as_str() {
                    ParamValue::String(s.into())
                } else if let Some(n) = value.as_f64() {
                    ParamValue::Number(n)
                } else if let Some(b) = value.as_bool() {
                    ParamValue::Bool(b)
                } else {
                    ParamValue::from_srgba(serde_json::from_value::<[u8; 4]>(value.clone())?)
                };
                Ok(Command::SetPluginParam {
                    id,
                    name: key.clone(),
                    value: Some(value),
                })
            })
            .collect::<Result<Vec<_>, ControlError>>()?;
        return Ok((
            Command::Batch { commands },
            ClipAdded {
                track_id: track,
                clip_id: id,
            },
        ));
    }
    let id = ClipId(
        project
            .tracks()
            .iter()
            .flat_map(|t| t.clips().keys().map(|id| id.0))
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| invalid("clip id exhausted"))?,
    );
    let clip = Clip::new(id, ClipKind::Plugin { plugin_id, params }, range);
    let (commands, track_id) = place_visual_clip(project, clip, draft.kind.label(), None);
    Ok((
        Command::Batch { commands },
        ClipAdded {
            track_id,
            clip_id: id,
        },
    ))
}

/// A short preview interval also used by the library's default Add action.
#[must_use]
pub fn math_object_range(start: Time) -> Option<TimeRange> {
    TimeRange::new(
        start,
        Time(start.0.checked_add(5 * animathio_core::TICKS_PER_SECOND)?),
    )
    .ok()
}

/// Snippets inserted at the source editor's caret.
pub const LATEX_SNIPPETS: &[(&str, &str)] = &[
    ("Fraction", r"\frac{a}{b}"),
    ("Root", r"\sqrt{x}"),
    ("Sum", r"\sum_{n=1}^{\infty} a_n"),
    ("Integral", r"\int_a^b f(x)\,dx"),
    ("Limit", r"\lim_{x\to 0} f(x)"),
    ("Matrix", r"\begin{matrix}a & b \\ c & d\end{matrix}"),
    ("Bmatrix", r"\begin{bmatrix}a & b \\ c & d\end{bmatrix}"),
    ("Cases", r"\begin{cases}x & x>0 \\ -x & x\le 0\end{cases}"),
    ("alpha", r"\alpha"),
    ("beta", r"\beta"),
    ("gamma", r"\gamma"),
    ("theta", r"\theta"),
    ("pi", r"\pi"),
    ("sigma", r"\sigma"),
    ("phi", r"\phi"),
    ("omega", r"\omega"),
    ("sin", r"\sin{x}"),
    ("cos", r"\cos{x}"),
    ("log", r"\log{x}"),
    ("exp", r"\exp{x}"),
    ("times", r"\times"),
    ("le", r"\le"),
    ("ge", r"\ge"),
    ("infty", r"\infty"),
];
/// Ready-to-use formula templates.
pub const LATEX_TEMPLATES: &[(&str, &str)] = &[
    ("Quadratic", r"x=\frac{-b\pm\sqrt{b^2-4ac}}{2a}"),
    ("Euler", r"e^{i\pi}+1=0"),
    ("Pythagoras", r"a^2+b^2=c^2"),
    ("Derivative", r"f'(x)=\lim_{h\to0}\frac{f(x+h)-f(x)}{h}"),
    (
        "Gaussian",
        r"\int_{-\infty}^{\infty}e^{-x^2}\,dx=\sqrt{\pi}",
    ),
    ("2x2 matrix", r"\begin{bmatrix}1 & 0 \\ 0 & 1\end{bmatrix}"),
];
