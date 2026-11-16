//! Small, command-built demo shared by the CLI and rendering fixtures.
use crate::{Error, Frame};
use animathio_core::{
    Asset, AssetId, Clip, ClipId, ClipKind, Command, Interpolation, Keyframe, Project,
    TICKS_PER_SECOND, Target, Time, TimeRange, Track, TrackId, Transform, TransformProperty,
};
use animathio_plugin_api::sources::{PathElement, Shape, Stroke};
use std::collections::BTreeMap;

/// Plain demo project and its content-addressed encoded image.
pub struct Sample {
    /// Project assembled exclusively through commands.
    pub project: Project,
    /// Encoded PNG bytes to write into an archive or load into a resource pool.
    pub assets: BTreeMap<AssetId, Vec<u8>>,
}

/// Three seconds of animated image, Unicode text, rectangle, ellipse and cubic path.
#[allow(clippy::too_many_lines)] // Keep the six demo layers and their animation data together.
pub fn build() -> Result<Sample, Error> {
    let mut project = Project::default();
    let mut image = Frame {
        size: (64, 64),
        pixels: Vec::with_capacity(64 * 64 * 4),
    };
    for y in 0_u8..64 {
        for x in 0_u8..64 {
            image.pixels.extend_from_slice(&[
                x * 4,
                y * 4,
                200,
                if (x / 8 + y / 8) % 2 == 0 { 255 } else { 150 },
            ]);
        }
    }
    let mut bytes = Vec::new();
    image.write_png(&mut bytes)?;
    let image_id = AssetId::of(&bytes);
    Command::AddAsset {
        id: image_id.clone(),
        asset: Asset {
            name: "gradient.png".into(),
            media_type: "image/png".into(),
        },
    }
    .apply(&mut project)?;
    let range = TimeRange::new(Time(0), Time(TICKS_PER_SECOND * 3))?;
    let shape = |shape, fill, stroke| animathio_plugin_api::sources::shape(shape, fill, stroke);
    let sources = [
        (
            shape(Shape::Rect, Some([17, 24, 39, 255]), None),
            Transform::default(),
            TransformProperty::Opacity,
            0.8,
            1.0,
        ),
        (
            ClipKind::Image(image_id.clone()),
            Transform {
                x: 0.24,
                y: 0.58,
                scale_x: 0.26,
                scale_y: 0.4,
                ..Transform::default()
            },
            TransformProperty::Rotation,
            -0.2,
            0.35,
        ),
        (
            shape(
                Shape::Rect,
                Some([42, 186, 170, 220]),
                Some(Stroke {
                    color: [220, 255, 255, 255],
                    width: 0.025,
                }),
            ),
            Transform {
                x: 0.5,
                y: 0.56,
                scale_x: 0.17,
                scale_y: 0.3,
                ..Transform::default()
            },
            TransformProperty::ScaleX,
            0.08,
            0.2,
        ),
        (
            shape(
                Shape::Ellipse,
                Some([251, 146, 60, 240]),
                Some(Stroke {
                    color: [255, 230, 180, 255],
                    width: 0.03,
                }),
            ),
            Transform {
                x: 0.76,
                y: 0.6,
                scale_x: 0.18,
                scale_y: 0.3,
                ..Transform::default()
            },
            TransformProperty::Y,
            0.64,
            0.5,
        ),
        (
            shape(
                Shape::Path(vec![
                    PathElement::MoveTo([0.0, 0.8]),
                    PathElement::CurveTo([0.25, -0.5], [0.6, 1.5], [1.0, 0.2]),
                    PathElement::LineTo([1.0, 1.0]),
                    PathElement::QuadTo([0.5, 0.5], [0.0, 1.0]),
                    PathElement::Close,
                ]),
                Some([129, 140, 248, 130]),
                Some(Stroke {
                    color: [192, 200, 255, 255],
                    width: 0.035,
                }),
            ),
            Transform {
                x: 0.5,
                y: 0.8,
                scale_x: 0.3,
                scale_y: 0.18,
                ..Transform::default()
            },
            TransformProperty::Rotation,
            -0.1,
            0.1,
        ),
        (
            animathio_plugin_api::sources::text(
                "AniMathIO 2.0\nAnimate α + β = γ".into(),
                0.08,
                [240, 245, 255, 255],
            ),
            Transform {
                x: 0.5,
                y: 0.14,
                ..Transform::default()
            },
            TransformProperty::Opacity,
            0.25,
            1.0,
        ),
    ];
    for (index, ((kind, transform, property, from, to), id)) in
        sources.into_iter().zip(1_u64..).enumerate()
    {
        Command::AddTrack {
            track: Track::new(TrackId(id), format!("Demo {id}")),
            index,
        }
        .apply(&mut project)?;
        let mut clip = Clip::new(ClipId(id), kind, range);
        clip.transform = transform;
        Command::AddClip {
            track: TrackId(id),
            clip,
        }
        .apply(&mut project)?;
        for (time, value) in [(Time(0), from), (Time(TICKS_PER_SECOND * 2), to)] {
            Command::AddKeyframe {
                id: ClipId(id),
                target: Target::Transform(property),
                time,
                keyframe: Keyframe {
                    value: value.into(),
                    interpolation: Interpolation::EaseInOut,
                },
            }
            .apply(&mut project)?;
        }
    }
    Ok(Sample {
        project,
        assets: BTreeMap::from([(image_id, bytes)]),
    })
}
