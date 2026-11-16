//! Data-driven title compositions shared by the editor and control clients.
use crate::{ClipAdded, ControlError};
use animathio_core::{
    Clip, ClipId, ClipKind, Command, Interpolation, Keyframe, ParamValue, Project, Target, Time,
    TimeRange, Track, TrackId, Transform, TransformProperty,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::BTreeMap, sync::OnceLock};

/// A bundled title composition, in back-to-front layer order.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TitlePreset {
    /// Stable machine-readable name.
    pub id: String,
    /// Library card label.
    pub name: String,
    /// Entry/exit motion: fade, slide or scale.
    pub animation: String,
    /// Maximum entry/exit duration in integer timeline ticks (short titles use a quarter).
    pub ramp_ticks: i64,
    /// Clip-local entry, resting and exit curves, applied to every layer.
    pub curves: Vec<TitleCurve>,
    /// Plugin layer descriptions. Text is inside an eight-percent safe area.
    pub layers: Vec<TitleLayer>,
}

/// One data-defined transform curve shared by a composition's layers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TitleCurve {
    /// Animated transform component.
    pub property: TransformProperty,
    /// How values relate to the layer's base transform.
    pub space: TitleCurveSpace,
    /// Entry, resting and exit values.
    pub values: [f64; 3],
}

/// Coordinate interpretation for a title animation curve.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TitleCurveSpace {
    /// Use the values directly (e.g. opacity).
    Absolute,
    /// Add values to the layer's base coordinate (e.g. slide).
    Offset,
    /// Multiply the base scale by these values (e.g. word pop).
    Factor,
}

/// One plugin layer in a title preset.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TitleLayer {
    /// text, latex, bar or background.
    pub kind: String,
    /// Editable field identifier; empty for decoration.
    #[serde(default)]
    pub field: String,
    /// Initial field content.
    #[serde(default)]
    pub default: String,
    /// Horizontal centre on landscape canvases (portrait layouts are centred).
    pub x: f64,
    /// Landscape width as a canvas fraction (portrait layouts use 84%).
    pub width: f64,
    /// Top edge, or formula centre, as a canvas-height fraction.
    #[serde(default)]
    pub y: f64,
    /// Font height relative to the shorter canvas dimension.
    #[serde(default)]
    pub size: f64,
    /// Decorative bar or text-box height.
    #[serde(default)]
    pub height: f64,
    /// Decorative fill.
    #[serde(default)]
    pub color: [u8; 4],
}

/// Parameters for `titles.add`; times are integer ticks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TitleAddParams {
    /// Stable preset id from `titles.list`.
    pub preset: String,
    /// Field overrides (title, subtitle, name, role, caption, cta, word or latex).
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    /// Timeline interval occupied by all layers.
    pub range: TimeRange,
}

/// Bundled preset definitions, parsed once. No engine or GUI resources are retained.
///
/// # Panics
/// Panics if the embedded preset JSON is malformed (a build-time authoring error).
#[must_use]
pub fn title_presets() -> &'static [TitlePreset] {
    static PRESETS: OnceLock<Vec<TitlePreset>> = OnceLock::new();
    PRESETS.get_or_init(|| {
        serde_json::from_str(include_str!("../presets/titles.json")).expect("bundled title data")
    })
}

/// Build one atomic undo step. Each layer gets its own front-most track and all
/// clips share a fresh link group. Text size adapts to the shorter canvas edge.
#[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
pub fn title_preset_command(
    project: &Project,
    params: &TitleAddParams,
) -> Result<(Command, ClipAdded), ControlError> {
    let invalid = |message: &str| ControlError::InvalidParams(message.to_owned());
    TimeRange::new(params.range.start, params.range.end)?;
    let preset = title_presets()
        .iter()
        .find(|p| p.id == params.preset)
        .ok_or_else(|| invalid("unknown title preset"))?;
    if params
        .fields
        .keys()
        .any(|field| !preset.layers.iter().any(|layer| layer.field == *field))
    {
        return Err(invalid("unknown field for title preset"));
    }
    let next = project
        .tracks()
        .iter()
        .map(|t| t.id().0)
        .chain(project.tracks().iter().flat_map(|t| {
            t.clips()
                .values()
                .flat_map(|clip| [clip.id.0, clip.link_group.map_or(0, |group| group.0)])
        }))
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| invalid("identity overflow"))?;
    next.checked_add(
        u64::try_from(preset.layers.len())
            .unwrap_or(u64::MAX)
            .saturating_mul(2),
    )
    .ok_or_else(|| invalid("identity overflow"))?;
    let duration = params.range.end.0 - params.range.start.0;
    let ramp = (duration / 4).min(preset.ramp_ticks);
    let (width, height) = animathio_export::resolution(project);
    let aspect = f64::from(width) / f64::from(height);
    let mut commands = Vec::new();
    let mut added = None;
    for (index, layer) in preset.layers.iter().enumerate() {
        let track_id = TrackId(next + index as u64 * 2);
        let id = ClipId(track_id.0 + 1);
        let content = params.fields.get(&layer.field).unwrap_or(&layer.default);
        if !layer.field.is_empty() && (content.trim().is_empty() || content.len() > 4096) {
            return Err(invalid("title fields must contain 1 to 4096 bytes"));
        }
        let width = if aspect < 1.0 { 0.84 } else { layer.width };
        let mut transform = Transform {
            x: if aspect < 1.0 { 0.5 } else { layer.x },
            ..Transform::default()
        };
        let kind = match layer.kind.as_str() {
            "text" => {
                // The text plugin fits edited content to this safe-area box at render time.
                let font = layer.size * aspect.min(1.0);
                transform.scale_x = width;
                transform.scale_y = layer.height;
                transform.y = layer.y + layer.height / 2.0;
                let mut kind = animathio_plugin_api::sources::text(content.clone(), font, [255; 4]);
                if let ClipKind::Plugin { params, .. } = &mut kind {
                    params["align"] = json!("center");
                    params["fit_width"] = json!(width);
                    params["fit_height"] = json!(layer.height);
                }
                kind
            }
            "latex" => {
                transform.y = layer.y;
                // Formula size follows the shorter render-canvas edge.
                ClipKind::Plugin {
                    plugin_id: "org.animathio.latex".into(),
                    params: json!({"source":content,"mode":"latex","units":"normalized","size":layer.size,"color":[255,255,255,255]}),
                }
            }
            "bar" | "background" => {
                let background = layer.kind == "background";
                transform.y = if background {
                    0.5
                } else {
                    layer.y + layer.height / 2.0
                };
                transform.scale_x = if background { 1.0 } else { width };
                transform.scale_y = if background { 1.0 } else { layer.height };
                rounded_bar(layer.color, if background { 0.0 } else { 0.035 })
            }
            _ => return Err(invalid("unknown title layer")),
        };
        let mut kind = kind;
        if let ClipKind::Plugin { params, .. } = &mut kind {
            params["_title_preset"] = json!(preset.id);
            params["_title_field"] = json!(layer.field);
        }
        let mut clip = Clip::new(id, kind, params.range);
        clip.transform = transform;
        clip.link_group = Some(ClipId(next + 1));
        commands.push(Command::AddTrack {
            track: Track::new(
                track_id,
                format!(
                    "{} / {}",
                    preset.name,
                    if layer.field.is_empty() {
                        "Backdrop"
                    } else {
                        &layer.field
                    }
                ),
            ),
            index: project.tracks().len() + index,
        });
        commands.push(Command::AddClip {
            track: track_id,
            clip,
        });
        if !layer.field.is_empty() {
            added.get_or_insert(ClipAdded {
                track_id,
                clip_id: id,
            });
        }
        if ramp > 0 {
            for curve in &preset.curves {
                let base = match curve.property {
                    TransformProperty::X => transform.x,
                    TransformProperty::Y => transform.y,
                    TransformProperty::ScaleX => transform.scale_x,
                    TransformProperty::ScaleY => transform.scale_y,
                    TransformProperty::Rotation => transform.rotation,
                    TransformProperty::Opacity => transform.opacity,
                };
                let [from, value, to] = curve.values.map(|value| match curve.space {
                    TitleCurveSpace::Absolute => value,
                    TitleCurveSpace::Offset => base + value,
                    TitleCurveSpace::Factor => base * value,
                });
                for (time, number) in [
                    (0, from),
                    (ramp, value),
                    (duration - ramp, value),
                    (duration, to),
                ] {
                    commands.push(Command::AddKeyframe {
                        id,
                        target: Target::Transform(curve.property),
                        time: Time(time),
                        keyframe: Keyframe {
                            value: ParamValue::Number(number),
                            interpolation: Interpolation::EaseInOut,
                        },
                    });
                }
            }
        }
    }
    Ok((
        Command::Batch { commands },
        added.ok_or_else(|| invalid("title preset has no editable field"))?,
    ))
}

fn rounded_bar(color: [u8; 4], r: f64) -> ClipKind {
    use animathio_plugin_api::sources::{
        PathElement::{Close, LineTo, MoveTo, QuadTo},
        Shape, shape,
    };
    shape(
        Shape::Path(vec![
            MoveTo([r, 0.0]),
            LineTo([1.0 - r, 0.0]),
            QuadTo([1.0, 0.0], [1.0, r]),
            LineTo([1.0, 1.0 - r]),
            QuadTo([1.0, 1.0], [1.0 - r, 1.0]),
            LineTo([r, 1.0]),
            QuadTo([0.0, 1.0], [0.0, 1.0 - r]),
            LineTo([0.0, r]),
            QuadTo([0.0, 0.0], [r, 0.0]),
            Close,
        ]),
        Some(color),
        None,
    )
}
