//! End-to-end tests for the v1 importer: programmatic fixtures, value mapping,
//! container round-trips, malformed input and decompression-bomb guards.
#![allow(
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::too_many_lines
)]

use animathio_core::{
    AssetId, ClipKind, FrameRate, Project, TICKS_PER_SECOND, Time, builtin::Builtin,
};
use animathio_import_v1::{
    ImportError, ImportLimits, import_v1, import_v1_with_limits, import_v1_with_report,
};
use base64::Engine;
use serde_json::json;
use std::io::{Read, Write};

const TICKS_PER_MS: f64 = TICKS_PER_SECOND as f64 / 1000.0;

fn ticks(ms: f64) -> i64 {
    (ms * TICKS_PER_MS).round() as i64
}

fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

fn deflate(value: &serde_json::Value) -> Vec<u8> {
    let json = serde_json::to_vec(value).unwrap();
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&json).unwrap();
    encoder.finish().unwrap()
}

#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)] // JSON fixture builder.
fn element(
    id: &str,
    kind: &str,
    name: &str,
    _src: &str,
    placement: serde_json::Value,
    start: f64,
    end: f64,
    properties: serde_json::Value,
) -> serde_json::Value {
    json!({
        "id": id,
        "type": kind,
        "name": name,
        "placement": placement,
        "timeFrame": { "start": start, "end": end },
        "properties": properties,
    })
}

fn placement(x: f64, y: f64, width: f64, height: f64) -> serde_json::Value {
    json!({
        "x": x,
        "y": y,
        "width": width,
        "height": height,
        "rotation": 0,
        "scaleX": 1,
        "scaleY": 1,
    })
}

/// A v1 project exercising every element type, every animation, effects,
/// audio parameters and media deduplication (image + mafs share PNG bytes).
fn fixture() -> (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>) {
    let png = b"tiny-png-bytes".to_vec();
    let video = b"tiny-video-bytes".to_vec();
    let wav = b"tiny-wav-bytes".to_vec();

    let project = json!({
        "backgroundColor": "#111111",
        "selectedMenuOption": "Videos",
        "audios": [],
        "videos": [],
        "images": [],
        "editorElements": [
            element(
                "video-1",
                "video",
                "Media(video) 1",
                "blob:ignored",
                placement(0.0, 0.0, 200.0, 100.0),
                0.0,
                5000.0,
                json!({
                    "src": "blob:ignored",
                    "elementId": "video-video-1",
                    "effect": { "type": "sepia" },
                    "muted": true,
                }),
            ),
            element(
                "audio-1",
                "audio",
                "Media(audio) 1",
                "blob:ignored",
                placement(0.0, 0.0, 100.0, 100.0),
                0.0,
                5000.0,
                json!({
                    "src": "blob:ignored",
                    "elementId": "audio-audio-1",
                    "volume": 0.5,
                    "muted": false,
                    "masterVolume": 0.8,
                }),
            ),
            element(
                "image-1",
                "image",
                "Media(image) 1",
                "blob:ignored",
                placement(100.0, 50.0, 300.0, 200.0),
                1000.0,
                4000.0,
                json!({
                    "src": "blob:ignored",
                    "elementId": "image-image-1",
                    "effect": { "type": "blackAndWhite" },
                }),
            ),
            element(
                "text-1",
                "text",
                "Text 1",
                "",
                placement(50.0, 100.0, 200.0, 40.0),
                0.0,
                5000.0,
                json!({
                    "text": "hello",
                    "fontSize": 24.0,
                    "fontWeight": 400.0,
                    "color": "#ff0000",
                }),
            ),
            element(
                "mafs-1",
                "mafs",
                "Mafs(Line) 1",
                "",
                placement(0.0, 0.0, 160.0, 120.0),
                0.0,
                5000.0,
                json!({
                    "src": data_url("image/png", &png),
                    "elementId": "mafs-mafs-1",
                    "effect": { "type": "invert" },
                }),
            ),
        ],
        "maxTime": 30000.0,
        "animations": [
            { "id": "a1", "targetId": "video-1", "duration": 1000.0, "type": "fadeIn", "properties": {} },
            { "id": "a2", "targetId": "image-1", "duration": 500.0, "type": "slideIn", "properties": { "direction": "right", "useClipPath": false, "textType": "none" } },
            { "id": "a3", "targetId": "text-1", "duration": 800.0, "type": "fadeOut", "properties": {} },
            { "id": "a4", "targetId": "text-1", "duration": 4000.0, "type": "breathe", "properties": {} },
            { "id": "a5", "targetId": "mafs-1", "duration": 600.0, "type": "mafsReveal", "properties": { "direction": "in" } },
        ],
        "currentKeyFrame": 0,
        "fps": 30.0,
        "selectedVideoFormat": "mp4",
        "canvas_width": 800.0,
        "canvas_height": 600.0,
        "mediaFiles": [
            { "id": "video-1", "type": "video", "data": data_url("video/mp4", &video) },
            { "id": "audio-1", "type": "audio", "data": data_url("audio/wav", &wav) },
            { "id": "image-1", "type": "image", "data": data_url("image/png", &png) },
        ],
        "futureField": { "ignored": true },
    });

    (deflate(&project), video, wav, png)
}

fn tracks(project: &Project) -> Vec<(animathio_core::TrackId, animathio_core::Clip)> {
    let mut out = Vec::new();
    for track in project.tracks() {
        for clip in track.clips().values() {
            out.push((track.id(), clip.clone()));
        }
    }
    out
}

#[test]
fn imports_every_element_type_and_maps_values() {
    let (bytes, video, wav, png) = fixture();
    let (project, assets, report) = import_v1_with_report(&bytes).unwrap();

    // Canvas, duration and frame rate.
    assert_eq!(project.frame_rate(), FrameRate::new(30, 1).unwrap());
    assert_eq!(project.width(), 800);
    assert_eq!(project.height(), 600);
    assert_eq!(project.background(), [17, 17, 17, 255]);
    assert_eq!(project.duration(), Time(ticks(30_000.0)));

    // One track per element, in v1 z-order.
    assert_eq!(project.tracks().len(), 5);
    let clips = tracks(&project);
    assert_eq!(clips.len(), 5);

    let video_asset = AssetId::of(&video);
    let wav_asset = AssetId::of(&wav);
    let png_asset = AssetId::of(&png);
    // Image and mafs share the PNG bytes, so exactly three distinct assets.
    assert_eq!(assets.len(), 3);
    assert_eq!(project.assets().len(), 3);
    assert_eq!(assets.iter().map(|a| &a.id).collect::<Vec<_>>().len(), 3);

    // Video: media type from the data URL, mute + effect params, fade-in keys.
    let video_clip = &clips[0].1;
    assert_eq!(video_clip.kind, ClipKind::Video(video_asset.clone()));
    assert_eq!(video_clip.params["muted"], 1.0);
    assert_eq!(video_clip.params["effect.sepia"], 1.0);
    assert_eq!(video_clip.transform.x, 0.125);
    assert_eq!(video_clip.transform.y, 100.0 / 600.0 / 2.0);
    assert_eq!(video_clip.transform.scale_x, 0.25);
    assert_eq!(video_clip.transform.scale_y, 100.0 / 600.0);
    assert_eq!(video_clip.range.start, Time(0));
    assert_eq!(video_clip.range.end, Time(ticks(5000.0)));
    let fade = video_clip
        .animations
        .iter()
        .find(|a| {
            a.target
                == animathio_core::Target::Transform(animathio_core::TransformProperty::Opacity)
        })
        .unwrap();
    assert_eq!(fade.keys.keys().len(), 2);

    // Audio: gain/mute params and track gain from masterVolume.
    let audio_clip = &clips[1].1;
    assert_eq!(audio_clip.kind, ClipKind::Audio(wav_asset.clone()));
    assert_eq!(audio_clip.params["gain"], 0.5);
    assert_eq!(audio_clip.params["muted"], 0.0);
    let audio_track = project
        .tracks()
        .iter()
        .find(|t| t.id() == clips[1].0)
        .unwrap();
    assert_eq!(audio_track.gain(), 0.8);

    // Image: effect param and a slide-in position animation.
    let image_clip = &clips[2].1;
    assert_eq!(image_clip.kind, ClipKind::Image(png_asset.clone()));
    assert_eq!(image_clip.params["effect.blackAndWhite"], 1.0);
    assert!(image_clip.animations.iter().any(|a| matches!(
        a.target,
        animathio_core::Target::Transform(animathio_core::TransformProperty::X)
    )));

    // Text: builtin text with normalized font size and explicit color.
    let text_clip = &clips[3].1;
    let ClipKind::Builtin(Builtin::Text {
        text,
        font_size,
        color,
    }) = &text_clip.kind
    else {
        panic!("expected builtin text clip");
    };
    assert_eq!(text, "hello");
    assert_eq!(*font_size, 24.0 / 600.0);
    assert_eq!(*color, [255, 0, 0, 255]);

    // Mafs: static PNG image with the mafs marker and effect params.
    let mafs_clip = &clips[4].1;
    assert_eq!(mafs_clip.kind, ClipKind::Image(png_asset.clone()));
    assert_eq!(mafs_clip.params["v1.mafs"], 1.0);
    assert_eq!(mafs_clip.params["effect.invert"], 1.0);

    // Media types come from the data URLs.
    let media_types: Vec<_> = assets.iter().map(|a| a.media_type.as_str()).collect();
    assert!(media_types.contains(&"video/mp4"));
    assert!(media_types.contains(&"audio/wav"));
    assert!(media_types.contains(&"image/png"));

    // The known-but-unused v1 fields do not produce "unknown field" warnings.
    assert!(
        !report
            .warnings()
            .iter()
            .any(|w| w.contains("selectedMenuOption"))
    );
    // The genuinely unknown field is reported.
    assert!(report.warnings().iter().any(|w| w.contains("futureField")));
    // Effects and mafs are reported as pending/static.
    assert!(
        report
            .warnings()
            .iter()
            .any(|w| w.contains("rendering is pending"))
    );
    assert!(
        report
            .warnings()
            .iter()
            .any(|w| w.contains("mafs graph imported as a static image"))
    );
}

#[test]
fn breathe_animates_scale_and_mafs_reveal_falls_back_to_opacity() {
    let (bytes, _, _, _) = fixture();
    let (project, _) = import_v1(&bytes).unwrap();
    let text_clip = tracks(&project)[3].1.clone();
    let targets: Vec<_> = text_clip
        .animations
        .iter()
        .map(|a| a.target.clone())
        .collect();
    assert!(targets.contains(&animathio_core::Target::Transform(
        animathio_core::TransformProperty::ScaleX
    )));
    assert!(targets.contains(&animathio_core::Target::Transform(
        animathio_core::TransformProperty::ScaleY
    )));
    let breathe = text_clip
        .animations
        .iter()
        .find(|a| {
            a.target == animathio_core::Target::Transform(animathio_core::TransformProperty::ScaleX)
        })
        .unwrap();
    // A 4000ms breathe window over a 5000ms clip (no slide) yields one beat.
    assert_eq!(breathe.keys.keys().len(), 2);

    let mafs_clip = tracks(&project)[4].1.clone();
    let opacity = mafs_clip
        .animations
        .iter()
        .find(|a| {
            a.target
                == animathio_core::Target::Transform(animathio_core::TransformProperty::Opacity)
        })
        .unwrap();
    assert_eq!(opacity.keys.keys().len(), 2);
}

#[test]
fn imports_plain_json_without_deflate_compression() {
    let (deflated, _, _, _) = fixture();
    // Re-inflate to get the raw JSON, then import it uncompressed.
    let mut json = Vec::new();
    flate2::read::ZlibDecoder::new(deflated.as_slice())
        .read_to_end(&mut json)
        .unwrap();
    let (project, assets, report) = import_v1_with_report(&json).unwrap();
    assert_eq!(project.tracks().len(), 5);
    assert_eq!(assets.len(), 3);
    assert!(report.warnings().iter().any(|w| w.contains("plain JSON")));
}

#[test]
fn round_trips_through_the_amio_container() {
    let (bytes, _, _, _) = fixture();
    let (project, assets) = import_v1(&bytes).unwrap();
    let asset_bytes: std::collections::BTreeMap<AssetId, Vec<u8>> =
        assets.into_iter().map(|a| (a.id, a.bytes)).collect();
    let mut cursor =
        animathio_format::write_amio(std::io::Cursor::new(Vec::new()), &project, &asset_bytes)
            .unwrap();
    cursor.set_position(0);
    let bundle = animathio_format::read_amio(cursor, animathio_format::Limits::default()).unwrap();
    assert_eq!(bundle.project, project);
    assert_eq!(bundle.assets, asset_bytes);
    bundle.project.validate().unwrap();
}

#[test]
fn malformed_input_errors_instead_of_panicking() {
    for input in [
        &b""[..],
        b"not json at all",
        b"{\"editorElements\": [",
        &deflate(&json!({"editorElements": "not-an-array"})),
        &deflate(&json!({"canvas_width": "wide"})),
    ] {
        assert!(import_v1(input).is_err(), "accepted malformed input");
    }
}

#[test]
fn inflated_size_is_capped() {
    // A highly compressible payload inflates far beyond the configured cap.
    let big = json!({ "editorElements": [], "pad": "a".repeat(100_000) });
    let bytes = deflate(&big);
    let limits = ImportLimits {
        inflated_bytes: 1_024,
        ..ImportLimits::default()
    };
    match import_v1_with_limits(&bytes, &limits) {
        Err(ImportError::InflatedTooLarge { limit, .. }) => assert_eq!(limit, 1_024),
        other => panic!("expected InflatedTooLarge, got {other:?}"),
    }
}

#[test]
fn per_asset_size_is_capped() {
    let big_asset = vec![b'x'; 10_000];
    let project = json!({
        "editorElements": [
            element(
                "img",
                "image",
                "big",
                "blob:ignored",
                placement(0.0, 0.0, 10.0, 10.0),
                0.0,
                1000.0,
                json!({ "src": "blob:ignored", "effect": { "type": "none" } }),
            )
        ],
        "canvas_width": 800.0,
        "canvas_height": 600.0,
        "fps": 30.0,
        "maxTime": 1000.0,
        "mediaFiles": [
            { "id": "img", "type": "image", "data": data_url("image/png", &big_asset) }
        ],
    });
    let bytes = deflate(&project);
    let limits = ImportLimits {
        asset_bytes: 1_024,
        ..ImportLimits::default()
    };
    match import_v1_with_limits(&bytes, &limits) {
        Err(ImportError::AssetTooLarge { limit, .. }) => assert_eq!(limit, 1_024),
        other => panic!("expected AssetTooLarge, got {other:?}"),
    }
}
