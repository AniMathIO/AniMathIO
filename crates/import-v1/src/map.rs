//! Mapping from the lenient v1 model to a validated v2 [`Project`].
//!
//! The rebuild is done exclusively through [`Command::apply`], so every step is
//! transactional and validated. Media is content-addressed with BLAKE3 and
//! deduplicated; a video clip and its paired v1 audio element therefore share
//! one asset.

use crate::model::{V1Animation, V1Effect, V1Element, V1Placement, V1Project};
use crate::{Asset, ImportError, ImportLimits, Report};
use animathio_core::{
    Asset as CoreAsset, AssetId, Clip, ClipId, ClipKind, Command, EffectId, EffectInstance,
    FrameRate, Interpolation, Keyframe, Project, TICKS_PER_SECOND, Target, Time, TimeRange, Track,
    TrackId, Transform, TransformProperty,
};
use animathio_plugin_api::sources::Color;
use base64::Engine;
use std::collections::{BTreeMap, HashMap};
use std::io::Read;

/// The clip payload each element builder produces.
type BuiltClip = (
    ClipKind,
    Transform,
    BTreeMap<String, f64>,
    Vec<EffectInstance>,
    Option<f64>,
);

/// Resolved bookkeeping for one imported element, used by the animation pass.
struct ElementMeta {
    clip_id: ClipId,
    center_x: f64,
    center_y: f64,
    scale_x: f64,
    scale_y: f64,
    duration_ticks: i64,
    placement: V1Placement,
    canvas_width: f64,
    canvas_height: f64,
}

/// Slide timings gathered before keyframes are emitted (breathe needs both).
#[derive(Clone, Copy, Default)]
struct SlideInfo {
    in_ms: Option<f64>,
    out_ms: Option<f64>,
}

#[allow(clippy::too_many_lines)] // The pass is a single, ordered migration sequence.
pub(crate) fn import_with_report(
    bytes: &[u8],
    limits: &ImportLimits,
) -> Result<(Project, Vec<Asset>, Report), ImportError> {
    let mut report = Report::default();
    let json = inflate_json(bytes, limits, &mut report)?;
    let v1: V1Project = serde_json::from_slice(&json)?;
    for key in v1.extra.keys() {
        report.warn(format!("unknown top-level field: {key}"));
    }

    let (width, height) = canvas_size(v1.canvas_width, v1.canvas_height, &mut report);
    let background = background_color(&v1.background_color, &mut report);
    let fps = frame_rate(v1.fps, &mut report)?;
    let duration = match ms_to_ticks(v1.max_time) {
        Ok(time) => time,
        Err(error) => {
            report.warn(format!("invalid maxTime ({error}); using 30 s"));
            Time(30 * TICKS_PER_SECOND)
        }
    };

    let mut project = Project::new(fps);
    Command::SetSettings {
        width,
        height,
        frame_rate: fps,
        background,
        duration,
    }
    .apply(&mut project)?;

    // Decode embedded media files once, capped and deduplicated by content hash.
    if v1.media_files.len() > limits.media_entries {
        return Err(ImportError::Invalid(format!(
            "{} media entries exceed the {} entry limit",
            v1.media_files.len(),
            limits.media_entries
        )));
    }
    let mut media_files: HashMap<String, (String, Vec<u8>)> = HashMap::new();
    for media in &v1.media_files {
        if media.id.is_empty() {
            report.warn("media file with an empty id ignored");
            continue;
        }
        for key in media.extra.keys() {
            report.warn(format!("media file {}: unknown field {key}", media.id));
        }
        match decode_data_url(&media.data, limits)? {
            Some((mime, bytes)) => {
                media_files.insert(media.id.clone(), (mime, bytes));
            }
            None => report.warn(format!(
                "media file {}: unparseable data URL ignored",
                media.id
            )),
        }
    }

    // First pass: resolve media, register content-addressed assets.
    let mut assets: BTreeMap<AssetId, (String, String, Vec<u8>)> = BTreeMap::new();
    let mut element_assets: HashMap<String, AssetId> = HashMap::new();
    for element in &v1.editor_elements {
        if !media_kind(&element.kind) {
            continue;
        }
        let Some((mime, bytes)) = element_media(element, &media_files, limits, &mut report)? else {
            continue;
        };
        let id = AssetId::of(&bytes);
        let name = asset_name(&element.id, &element.name, &mime);
        assets
            .entry(id.clone())
            .or_insert_with(|| (name, mime, bytes));
        element_assets.insert(element.id.clone(), id);
    }
    for (id, (name, mime, _)) in &assets {
        Command::AddAsset {
            id: id.clone(),
            asset: CoreAsset {
                name: name.clone(),
                media_type: mime.clone(),
            },
        }
        .apply(&mut project)?;
    }

    // Second pass: one track per element, in v1 z-order, with base transforms.
    let mut elements: HashMap<String, ElementMeta> = HashMap::new();
    let mut track_index = 0_u64;
    let mut clip_index = 0_u64;
    for element in &v1.editor_elements {
        if element.id.is_empty() {
            report.warn("element with an empty id skipped");
            continue;
        }
        for key in element.extra.keys() {
            report.warn(format!("element {}: unknown field {key}", element.id));
        }
        for key in element.properties.extra.keys() {
            report.warn(format!("element {}: unknown property {key}", element.id));
        }
        let Some(placement) = element.placement.clone() else {
            report.warn(format!(
                "element {}: missing placement; skipped",
                element.id
            ));
            continue;
        };
        let Some(time_frame) = element.time_frame.clone() else {
            report.warn(format!(
                "element {}: missing timeFrame; skipped",
                element.id
            ));
            continue;
        };
        let Some(start) = element_ticks(time_frame.start, &element.id, &mut report) else {
            continue;
        };
        let Some(end) = element_ticks(time_frame.end, &element.id, &mut report) else {
            continue;
        };
        if end <= start {
            report.warn(format!("element {}: empty timeFrame; skipped", element.id));
            continue;
        }
        let Ok(range) = TimeRange::new(start, end) else {
            report.warn(format!(
                "element {}: invalid timeFrame; skipped",
                element.id
            ));
            continue;
        };

        let built = match element.kind.as_str() {
            "video" => built_media(
                element,
                &element_assets,
                ClipKind::Video,
                f64::from(width),
                f64::from(height),
                &mut report,
            ),
            "image" => built_media(
                element,
                &element_assets,
                ClipKind::Image,
                f64::from(width),
                f64::from(height),
                &mut report,
            ),
            "audio" => built_audio(
                element,
                &element_assets,
                f64::from(width),
                f64::from(height),
                &mut report,
            ),
            "mafs" => built_mafs(
                element,
                &element_assets,
                f64::from(width),
                f64::from(height),
                &mut report,
            ),
            "text" => built_text(
                element,
                f64::from(width),
                f64::from(height),
                background,
                &mut report,
            ),
            other => {
                report.warn(format!(
                    "element {}: unknown type {other:?}; skipped",
                    element.id
                ));
                None
            }
        };
        let Some((kind, transform, params, effects, track_gain)) = built else {
            continue;
        };

        track_index += 1;
        clip_index += 1;
        let track_id = TrackId(track_index);
        let clip_id = ClipId(clip_index);
        Command::AddTrack {
            track: Track::new(track_id, element.name.clone()),
            index: usize::try_from(track_index - 1).unwrap_or(usize::MAX),
        }
        .apply(&mut project)?;
        if let Some(gain) = track_gain {
            Command::SetTrackGain { id: track_id, gain }.apply(&mut project)?;
        }
        let clip = Clip {
            link_group: None,
            id: clip_id,
            kind,
            range,
            source_offset: Time(0),
            transform,
            params: params.into_iter().map(|(k, v)| (k, v.into())).collect(),
            effects,
            audio_effects: Vec::new(),
            animations: Vec::new(),
        };
        Command::AddClip {
            track: track_id,
            clip,
        }
        .apply(&mut project)?;
        if elements
            .insert(
                element.id.clone(),
                ElementMeta {
                    clip_id,
                    center_x: transform.x,
                    center_y: transform.y,
                    scale_x: transform.scale_x,
                    scale_y: transform.scale_y,
                    duration_ticks: range.end.0 - range.start.0,
                    placement,
                    canvas_width: f64::from(width),
                    canvas_height: f64::from(height),
                },
            )
            .is_some()
        {
            report.warn(format!(
                "element {}: duplicate id; animations target the last",
                element.id
            ));
        }
    }

    // Gather slide timings first: breathe's window is bounded by them.
    let mut slides: HashMap<String, SlideInfo> = HashMap::new();
    for animation in &v1.animations {
        if animation.target_id.is_empty() || !elements.contains_key(&animation.target_id) {
            continue;
        }
        if animation.duration.is_finite() && animation.duration > 0.0 {
            let info = slides.entry(animation.target_id.clone()).or_default();
            match animation.kind.as_str() {
                "slideIn" => info.in_ms = Some(animation.duration),
                "slideOut" => info.out_ms = Some(animation.duration),
                _ => {}
            }
        }
    }

    // Emit keyframes.
    for animation in &v1.animations {
        import_animation(animation, &elements, &slides, &mut project, &mut report)?;
    }

    project.validate()?;
    let assets = assets
        .into_iter()
        .map(|(id, (name, media_type, bytes))| Asset {
            id,
            name,
            media_type,
            bytes,
        })
        .collect();
    Ok((project, assets, report))
}

fn inflate_json(
    bytes: &[u8],
    limits: &ImportLimits,
    report: &mut Report,
) -> Result<Vec<u8>, ImportError> {
    let mut inflated = Vec::new();
    let decoder = flate2::read::ZlibDecoder::new(bytes);
    match decoder
        .take(limits.inflated_bytes.saturating_add(1))
        .read_to_end(&mut inflated)
    {
        Ok(_) => {
            let len = u64::try_from(inflated.len()).unwrap_or(u64::MAX);
            if len > limits.inflated_bytes {
                return Err(ImportError::InflatedTooLarge {
                    inflated: len,
                    limit: limits.inflated_bytes,
                });
            }
            Ok(inflated)
        }
        Err(error) => {
            // Pre-compression v1 releases wrote plain JSON.
            let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
            if len > limits.inflated_bytes {
                return Err(ImportError::InflatedTooLarge {
                    inflated: len,
                    limit: limits.inflated_bytes,
                });
            }
            report.warn(format!(
                "payload is not deflate-compressed ({error}); parsed as plain JSON"
            ));
            Ok(bytes.to_vec())
        }
    }
}

fn canvas_size(width: f64, height: f64, report: &mut Report) -> (u32, u32) {
    let mut resolve = |value: f64, fallback: u32, axis: &str| {
        if !value.is_finite() || value <= 0.0 {
            report.warn(format!("invalid canvas_{axis} ({value}); using {fallback}"));
            return fallback;
        }
        let value = value.round();
        if value > 65_535.0 {
            report.warn(format!("canvas_{axis} ({value}) exceeds 65535; clamped"));
            return 65_535;
        }
        value as u32
    };
    (resolve(width, 800, "width"), resolve(height, 600, "height"))
}

fn background_color(value: &str, report: &mut Report) -> Color {
    if let Some(color) = parse_color(value) {
        return color;
    }
    report.warn(format!(
        "unparseable backgroundColor {value:?}; using #111111"
    ));
    [17, 17, 17, 255]
}

fn frame_rate(value: f64, report: &mut Report) -> Result<FrameRate, ImportError> {
    let fps = if value.is_finite() && value >= 1.0 {
        let rounded = value.round();
        if (value - rounded).abs() > 1e-9 {
            report.warn(format!("fractional fps {value} rounded to {rounded}"));
        }
        rounded as u32
    } else {
        report.warn(format!("invalid fps {value}; using 60"));
        60
    };
    Ok(FrameRate::new(fps.max(1), 1)?)
}

fn ms_to_ticks(ms: f64) -> Result<Time, ImportError> {
    if !ms.is_finite() || ms < 0.0 {
        return Err(ImportError::Invalid(format!(
            "non-finite or negative time {ms}ms"
        )));
    }
    let ticks = ms * TICKS_PER_SECOND as f64 / 1000.0;
    let ticks = ticks.round();
    if ticks > i64::MAX as f64 {
        return Err(ImportError::Invalid(format!(
            "time {ms}ms overflows the timeline"
        )));
    }
    Ok(Time(ticks as i64))
}

fn element_ticks(ms: f64, id: &str, report: &mut Report) -> Option<Time> {
    match ms_to_ticks(ms) {
        Ok(time) => Some(time),
        Err(error) => {
            report.warn(format!(
                "element {id}: invalid timeFrame ({error}); skipped"
            ));
            None
        }
    }
}

fn parse_color(value: &str) -> Option<Color> {
    let hex = value.trim();
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |range: std::ops::Range<usize>| u8::from_str_radix(&hex[range], 16).ok();
    match hex.len() {
        3 => Some([byte(0..1)? * 17, byte(1..2)? * 17, byte(2..3)? * 17, 255]),
        6 => Some([byte(0..2)?, byte(2..4)?, byte(4..6)?, 255]),
        8 => Some([byte(0..2)?, byte(2..4)?, byte(4..6)?, byte(6..8)?]),
        _ => None,
    }
}

fn contrast_color(background: Color) -> Color {
    let [r, g, b, _] = background;
    let luminance = (0.299 * f64::from(r) + 0.587 * f64::from(g) + 0.114 * f64::from(b)) / 255.0;
    if luminance > 0.5 {
        [0, 0, 0, 255]
    } else {
        [255, 255, 255, 255]
    }
}

fn media_kind(kind: &str) -> bool {
    matches!(kind, "video" | "image" | "audio" | "mafs")
}

fn element_media(
    element: &V1Element,
    media_files: &HashMap<String, (String, Vec<u8>)>,
    limits: &ImportLimits,
    report: &mut Report,
) -> Result<Option<(String, Vec<u8>)>, ImportError> {
    if let Some((mime, bytes)) = media_files.get(&element.id) {
        return Ok(Some((mime.clone(), bytes.clone())));
    }
    let Some(src) = element.properties.src.as_deref() else {
        report.warn(format!(
            "element {}: no embedded media source; skipped",
            element.id
        ));
        return Ok(None);
    };
    if let Some((mime, bytes)) = decode_data_url(src, limits)? {
        return Ok(Some((mime, bytes)));
    }
    report.warn(format!(
        "element {}: media source is not an embedded data URL; skipped",
        element.id
    ));
    Ok(None)
}

fn decode_data_url(
    url: &str,
    limits: &ImportLimits,
) -> Result<Option<(String, Vec<u8>)>, ImportError> {
    let Some(rest) = url.strip_prefix("data:") else {
        return Ok(None);
    };
    let Some((meta, payload)) = rest.split_once(',') else {
        return Ok(None);
    };
    let is_base64 = meta.ends_with(";base64");
    let mime = meta.trim_end_matches(";base64").to_owned();
    let bytes = if is_base64 {
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(payload.trim()) else {
            return Ok(None);
        };
        bytes
    } else {
        payload.as_bytes().to_vec()
    };
    let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if len > limits.asset_bytes {
        return Err(ImportError::AssetTooLarge {
            bytes: len,
            limit: limits.asset_bytes,
        });
    }
    Ok(Some((mime, bytes)))
}

fn asset_name(id: &str, element_name: &str, mime: &str) -> String {
    let extension = match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "audio/mpeg" => "mp3",
        "audio/wav" | "audio/x-wav" | "audio/wave" => "wav",
        "audio/ogg" => "ogg",
        "audio/mp4" | "audio/aac" => "m4a",
        _ => "bin",
    };
    let stem = if id.is_empty() {
        element_name.to_owned()
    } else {
        id.to_owned()
    };
    format!("{stem}.{extension}")
}

fn element_effects(
    effect: Option<&V1Effect>,
    report: &mut Report,
    element_id: &str,
) -> Vec<EffectInstance> {
    let Some(effect) = effect else {
        return Vec::new();
    };
    let name = effect.kind.as_str();
    let kind = match name {
        "blackAndWhite" => "grayscale",
        "sepia" => "sepia",
        "invert" => "invert",
        // v1's preset is `saturate(200%)`, so the multiplier is 2, not 1.
        "saturate" => "saturate",
        "none" => return Vec::new(),
        other => {
            report.warn(format!(
                "element {element_id}: unknown effect {other:?} ignored"
            ));
            return Vec::new();
        }
    };
    let mut instance = EffectInstance::basic(EffectId(1), kind);
    if kind == "saturate" {
        instance.params.insert("amount".to_owned(), 2.0);
    }
    vec![instance]
}

fn base_geometry(placement: &V1Placement) -> (f64, f64, f64, f64) {
    let width = placement.width * placement.scale_x;
    let height = placement.height * placement.scale_y;
    (
        placement.x + width / 2.0,
        placement.y + height / 2.0,
        width,
        height,
    )
}

fn built_media(
    element: &V1Element,
    element_assets: &HashMap<String, AssetId>,
    kind: fn(AssetId) -> ClipKind,
    canvas_width: f64,
    canvas_height: f64,
    report: &mut Report,
) -> Option<BuiltClip> {
    let asset = element_assets.get(&element.id)?;
    let placement = element.placement.as_ref()?;
    let effects = element_effects(element.properties.effect.as_ref(), report, &element.id);
    let mut params = BTreeMap::new();
    let muted = element.properties.muted.unwrap_or(false);
    params.insert("muted".to_owned(), f64::from(u8::from(muted)));
    if placement.rotation != 0.0 {
        report.warn(format!(
            "element {}: rotation is approximated as center-based",
            element.id
        ));
    }
    Some((
        kind(asset.clone()),
        media_transform(placement, canvas_width, canvas_height),
        params,
        effects,
        None,
    ))
}

fn built_audio(
    element: &V1Element,
    element_assets: &HashMap<String, AssetId>,
    canvas_width: f64,
    canvas_height: f64,
    report: &mut Report,
) -> Option<BuiltClip> {
    let asset = element_assets.get(&element.id)?;
    let placement = element.placement.as_ref()?;
    let mut params = BTreeMap::new();
    let volume = element.properties.volume.unwrap_or(1.0);
    let gain = if volume.is_finite() && volume >= 0.0 {
        volume
    } else {
        report.warn(format!(
            "element {}: invalid volume ({volume}); using 1",
            element.id
        ));
        1.0
    };
    params.insert("gain".to_owned(), gain);
    params.insert(
        "muted".to_owned(),
        f64::from(u8::from(element.properties.muted.unwrap_or(false))),
    );
    let master = element.properties.master_volume;
    let track_gain = master.map(|value| {
        if value.is_finite() && value >= 0.0 {
            value
        } else {
            report.warn(format!(
                "element {}: invalid masterVolume ({value}); using 1",
                element.id
            ));
            1.0
        }
    });
    Some((
        ClipKind::Audio(asset.clone()),
        media_transform(placement, canvas_width, canvas_height),
        params,
        Vec::new(),
        track_gain,
    ))
}

fn built_mafs(
    element: &V1Element,
    element_assets: &HashMap<String, AssetId>,
    canvas_width: f64,
    canvas_height: f64,
    report: &mut Report,
) -> Option<BuiltClip> {
    let asset = element_assets.get(&element.id)?;
    let placement = element.placement.as_ref()?;
    let effects = element_effects(element.properties.effect.as_ref(), report, &element.id);
    // v1 rasterized mafs graphs to a PNG and never stored the graph
    // definition, so only a marker survives for a future graph plugin.
    let mut params = BTreeMap::new();
    params.insert("v1.mafs".to_owned(), 1.0);
    report.warn(format!(
        "element {}: mafs graph imported as a static image; graph definition not stored in v1",
        element.id
    ));
    Some((
        ClipKind::Image(asset.clone()),
        media_transform(placement, canvas_width, canvas_height),
        params,
        effects,
        None,
    ))
}

fn built_text(
    element: &V1Element,
    canvas_width: f64,
    canvas_height: f64,
    background: Color,
    report: &mut Report,
) -> Option<BuiltClip> {
    let placement = element.placement.as_ref()?;
    let Some(text) = element.properties.text.clone() else {
        report.warn(format!(
            "element {}: text element with no text; skipped",
            element.id
        ));
        return None;
    };
    let font_px = match element.properties.font_size {
        Some(size) if size.is_finite() && size > 0.0 => size,
        other => {
            report.warn(format!(
                "element {}: invalid fontSize ({other:?}); using 16",
                element.id
            ));
            16.0
        }
    };
    let fraction = (font_px / canvas_height).clamp(f64::EPSILON, 16.0);
    let color = match element.properties.color.as_deref().map(parse_color) {
        Some(Some(color)) => color,
        Some(None) => {
            report.warn(format!(
                "element {}: unparseable color; using contrast color",
                element.id
            ));
            contrast_color(background)
        }
        None => contrast_color(background),
    };
    if let Some(weight) = element.properties.font_weight
        && (weight - 400.0).abs() > 1e-9
    {
        report.warn(format!(
            "element {}: fontWeight {weight} has no v2 text plugin equivalent; dropped",
            element.id
        ));
    }
    if placement.rotation != 0.0 {
        report.warn(format!(
            "element {}: rotation is approximated as center-based",
            element.id
        ));
    }
    Some((
        animathio_plugin_api::sources::text(text, fraction, color),
        text_transform(placement, canvas_width, canvas_height),
        BTreeMap::new(),
        Vec::new(),
        None,
    ))
}

fn media_transform(placement: &V1Placement, canvas_width: f64, canvas_height: f64) -> Transform {
    let (center_x, center_y, width, height) = base_geometry(placement);
    Transform {
        x: center_x / canvas_width,
        y: center_y / canvas_height,
        scale_x: width / canvas_width,
        scale_y: height / canvas_height,
        rotation: placement.rotation.to_radians(),
        opacity: 1.0,
    }
}

fn text_transform(placement: &V1Placement, canvas_width: f64, canvas_height: f64) -> Transform {
    let (center_x, center_y, _, _) = base_geometry(placement);
    Transform {
        x: center_x / canvas_width,
        y: center_y / canvas_height,
        scale_x: placement.scale_x,
        scale_y: placement.scale_y,
        rotation: placement.rotation.to_radians(),
        opacity: 1.0,
    }
}

#[allow(clippy::too_many_lines)] // One arm per v1 animation preset keeps the mapping together.
fn import_animation(
    animation: &V1Animation,
    elements: &HashMap<String, ElementMeta>,
    slides: &HashMap<String, SlideInfo>,
    project: &mut Project,
    report: &mut Report,
) -> Result<(), ImportError> {
    for key in animation.extra.keys() {
        report.warn(format!("animation {}: unknown field {key}", animation.id));
    }
    for key in animation.properties.extra.keys() {
        report.warn(format!(
            "animation {}: unknown property {key}",
            animation.id
        ));
    }
    if animation.target_id.is_empty() {
        report.warn("animation with no targetId skipped");
        return Ok(());
    }
    if animation.properties.text_type.as_deref() == Some("character") {
        report.warn(format!(
            "animation {}: per-character text slide imported as a whole-element slide",
            animation.id
        ));
    }
    let Some(meta) = elements.get(&animation.target_id) else {
        report.warn(format!(
            "animation {}: target element {} missing; skipped",
            animation.id, animation.target_id
        ));
        return Ok(());
    };
    if !animation.duration.is_finite() || animation.duration <= 0.0 {
        report.warn(format!(
            "animation {}: invalid duration ({}); skipped",
            animation.id, animation.duration
        ));
        return Ok(());
    }
    let Some(duration_ticks) = element_ticks(animation.duration, &animation.id, report) else {
        return Ok(());
    };
    let duration_ticks = duration_ticks.0;
    let slide = slides
        .get(&animation.target_id)
        .copied()
        .unwrap_or_default();
    let clip_id = meta.clip_id;

    match animation.kind.as_str() {
        "fadeIn" => {
            add_key(project, clip_id, TransformProperty::Opacity, 0, 0.0)?;
            add_key(
                project,
                clip_id,
                TransformProperty::Opacity,
                duration_ticks,
                1.0,
            )?;
        }
        "fadeOut" => {
            let start = late_start(meta.duration_ticks, duration_ticks, &animation.id, report);
            add_key(project, clip_id, TransformProperty::Opacity, start, 1.0)?;
            add_key(
                project,
                clip_id,
                TransformProperty::Opacity,
                meta.duration_ticks,
                0.0,
            )?;
        }
        "mafsReveal" => {
            report.warn(format!(
                "animation {}: mafsReveal imported as an opacity fallback",
                animation.id
            ));
            if animation.properties.direction.as_deref() == Some("out") {
                let start = late_start(meta.duration_ticks, duration_ticks, &animation.id, report);
                add_key(project, clip_id, TransformProperty::Opacity, start, 1.0)?;
                add_key(
                    project,
                    clip_id,
                    TransformProperty::Opacity,
                    meta.duration_ticks,
                    0.0,
                )?;
            } else {
                add_key(project, clip_id, TransformProperty::Opacity, 0, 0.0)?;
                add_key(
                    project,
                    clip_id,
                    TransformProperty::Opacity,
                    duration_ticks,
                    1.0,
                )?;
            }
        }
        "slideIn" => {
            let (start_x, start_y) = slide_position(animation, true, meta);
            add_key(project, clip_id, TransformProperty::X, 0, start_x)?;
            add_key(
                project,
                clip_id,
                TransformProperty::X,
                duration_ticks,
                meta.center_x,
            )?;
            add_key(project, clip_id, TransformProperty::Y, 0, start_y)?;
            add_key(
                project,
                clip_id,
                TransformProperty::Y,
                duration_ticks,
                meta.center_y,
            )?;
        }
        "slideOut" => {
            let start = late_start(meta.duration_ticks, duration_ticks, &animation.id, report);
            let (end_x, end_y) = slide_position(animation, false, meta);
            add_key(project, clip_id, TransformProperty::X, start, meta.center_x)?;
            add_key(
                project,
                clip_id,
                TransformProperty::X,
                meta.duration_ticks,
                end_x,
            )?;
            add_key(project, clip_id, TransformProperty::Y, start, meta.center_y)?;
            add_key(
                project,
                clip_id,
                TransformProperty::Y,
                meta.duration_ticks,
                end_y,
            )?;
        }
        "breathe" => {
            import_breathe(animation, meta, slide, project, report)?;
        }
        other => {
            report.warn(format!(
                "animation {}: unknown type {other:?}; skipped",
                animation.id
            ));
        }
    }
    Ok(())
}

fn late_start(clip_duration: i64, animation_duration: i64, id: &str, report: &mut Report) -> i64 {
    let start = clip_duration - animation_duration;
    if start >= 0 {
        return start;
    }
    report.warn(format!(
        "animation {id}: duration exceeds clip; clamped to the clip length"
    ));
    0
}

fn slide_position(animation: &V1Animation, slide_in: bool, meta: &ElementMeta) -> (f64, f64) {
    // v1 animated the Konva top-left corner; the element center follows that
    // path. Off-canvas offsets reuse the unscaled placement width/height.
    let direction = animation.properties.direction.as_deref().unwrap_or("left");
    let placement = &meta.placement;
    let (left, top) = if slide_in {
        match direction {
            "right" => (meta.canvas_width, placement.y),
            "left" => (-placement.width, placement.y),
            "bottom" => (placement.x, meta.canvas_height),
            "top" => (placement.x, -placement.height),
            _ => (placement.x, placement.y),
        }
    } else {
        match direction {
            "right" => (meta.canvas_width, placement.y),
            "left" => (-placement.width, placement.y),
            "bottom" => (placement.x, meta.canvas_height),
            // v1 slide-out to the top overshoots by 100px.
            "top" => (placement.x, -100.0 - placement.height),
            _ => (placement.x, placement.y),
        }
    };
    let displayed_width = placement.width * placement.scale_x;
    let displayed_height = placement.height * placement.scale_y;
    (
        (left + displayed_width / 2.0) / meta.canvas_width,
        (top + displayed_height / 2.0) / meta.canvas_height,
    )
}

fn import_breathe(
    animation: &V1Animation,
    meta: &ElementMeta,
    slide: SlideInfo,
    project: &mut Project,
    report: &mut Report,
) -> Result<(), ImportError> {
    let start_ms = slide.in_ms.unwrap_or(0.0);
    let clip_ms = meta.duration_ticks as f64 * 1000.0 / TICKS_PER_SECOND as f64;
    let end_ms = clip_ms - slide.out_ms.unwrap_or(0.0);
    let duration_ms = end_ms - start_ms;
    if duration_ms <= 0.0 {
        report.warn(format!(
            "animation {}: breathe window is empty; skipped",
            animation.id
        ));
        return Ok(());
    }
    let period_ms = (1000.0 * 60.0 / 72.0) * 4.0;
    let beats = (duration_ms / period_ms).floor() as u32;
    if beats == 0 {
        report.warn(format!(
            "animation {}: breathe window too short; skipped",
            animation.id
        ));
        return Ok(());
    }
    let up = 1.05;
    let steps = beats * 2;
    for index in 0..steps {
        let fraction = if steps > 1 {
            f64::from(index) / f64::from(steps - 1)
        } else {
            0.0
        };
        let time_ms = start_ms + duration_ms * fraction;
        let time = ms_to_ticks(time_ms)?.0;
        let scale = if index % 2 == 0 { up } else { 1.0 };
        add_key(
            project,
            meta.clip_id,
            TransformProperty::ScaleX,
            time,
            meta.scale_x * scale,
        )?;
        add_key(
            project,
            meta.clip_id,
            TransformProperty::ScaleY,
            time,
            meta.scale_y * scale,
        )?;
    }
    Ok(())
}

fn add_key(
    project: &mut Project,
    clip_id: ClipId,
    property: TransformProperty,
    time: i64,
    value: f64,
) -> Result<(), ImportError> {
    Command::AddKeyframe {
        id: clip_id,
        target: Target::Transform(property),
        time: Time(time),
        keyframe: Keyframe {
            value: value.into(),
            interpolation: Interpolation::Linear,
        },
    }
    .apply(project)?;
    Ok(())
}
