//! Atomic media placement from one bounded read and one probe.
use crate::{AssetInfo, ControlError, media_type_of, read_bounded, validate_existing_file};
use animathio_core::{
    Asset, AssetId, Clip, ClipId, ClipKind, Command, Project, TICKS_PER_SECOND, Time, TimeRange,
    Track, TrackId,
};
use animathio_media::MediaBackend;
use std::{path::Path, sync::Arc};

/// Probe video and audio once. A video must probe (its audio companion depends on
/// it); an audio file the prober cannot read keeps the default length.
fn probe(
    kind: &ClipKind,
    name: &str,
    bytes: &[u8],
) -> Result<Option<animathio_media::MediaInfo>, ControlError> {
    let probe = || {
        animathio_media::FilmCraftBackend::new()
            .probe_bytes(name, Arc::from(bytes))
            .map_err(|e| ControlError::Other(e.to_string()))
    };
    Ok(match kind {
        ClipKind::Video(_) => Some(probe()?),
        ClipKind::Audio(_) => probe().ok(),
        _ => None,
    })
}

#[allow(clippy::type_complexity)]
/// Prepare an import without mutating the project or retaining asset bytes.
pub fn prepare_media_import(
    project: &Project,
    path: &Path,
) -> Result<(AssetId, Vec<u8>, Command, serde_json::Value), ControlError> {
    let canonical = validate_existing_file(path)?;
    let bytes = read_bounded(&canonical, animathio_format::Limits::default().asset_bytes)?;
    let name = canonical
        .file_name()
        .ok_or_else(|| ControlError::Other("media path has no filename".into()))?
        .to_string_lossy()
        .into_owned();
    let info = AssetInfo {
        id: AssetId::of(&bytes),
        media_type: media_type_of(&name).to_owned(),
        name,
    };
    let asset_id = info.id.clone();
    let kind = match info.media_type.as_str() {
        t if t.starts_with("image/") => ClipKind::Image(info.id.clone()),
        t if t.starts_with("video/") => ClipKind::Video(info.id.clone()),
        t if t.starts_with("audio/") => ClipKind::Audio(info.id.clone()),
        _ => return Err(ControlError::Other("unsupported media type".into())),
    };
    let base = project
        .tracks()
        .iter()
        .flat_map(|t| std::iter::once(t.id().0).chain(t.clips().keys().map(|id| id.0)))
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .filter(|id| *id <= u64::MAX - 3)
        .ok_or_else(|| ControlError::Other("id space exhausted".into()))?;
    let track_id = TrackId(base);
    let clip_id = ClipId(base + 1);
    // Video and audio are probed once: the clip takes the media's own length.
    let probed = probe(&kind, &info.name, &bytes)?;
    let length = match &probed {
        Some(media) if media.duration.0 > 0 => media.duration,
        _ if project.duration().0 > 0 => project.duration(),
        _ => Time(TICKS_PER_SECOND * 3),
    };
    let mut clip = Clip::new(
        clip_id,
        kind,
        TimeRange::new(Time(0), length).map_err(|e| ControlError::Other(e.to_string()))?,
    );
    if matches!(clip.kind, ClipKind::Audio(_)) {
        clip.params.insert("gain".into(), 1.0.into());
        clip.params.insert("muted".into(), 0.0.into());
    }
    let mut commands = Vec::new();
    // Placing a file that is already in the project reuses its asset.
    if !project.assets().contains_key(&info.id) {
        commands.push(Command::AddAsset {
            id: info.id.clone(),
            asset: Asset {
                name: info.name.clone(),
                media_type: info.media_type.clone(),
            },
        });
    }
    commands.extend([
        Command::AddTrack {
            track: Track::new(track_id, info.name.clone()),
            index: project.tracks().len(),
        },
        Command::AddClip {
            track: track_id,
            clip: clip.clone(),
        },
    ]);
    if matches!(clip.kind, ClipKind::Video(_))
        && probed.as_ref().is_some_and(|media| !media.audio.is_empty())
    {
        let audio_track = TrackId(base + 2);
        let mut audio = Clip::new(ClipId(base + 3), ClipKind::Audio(info.id), clip.range);
        audio.link_group = Some(clip_id);
        audio.params.insert("gain".into(), 1.0.into());
        audio.params.insert("muted".into(), 0.0.into());
        commands.extend([
            Command::SetClipLink {
                id: clip_id,
                group: Some(clip_id),
            },
            Command::AddTrack {
                track: Track::new(audio_track, "Audio"),
                index: project.tracks().len() + 1,
            },
            Command::AddClip {
                track: audio_track,
                clip: audio,
            },
        ]);
    }
    Ok((
        asset_id,
        bytes,
        Command::Batch { commands },
        serde_json::json!({"track_id": track_id.0, "clip_id": clip_id.0}),
    ))
}

#[cfg(test)]
mod tests {
    use crate::{Call, ControlHandler, HeadlessHandler};
    #[test]
    fn video_import_uses_verified_bytes_and_is_one_undo_step() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../media/tests/fixtures/sample.mp4");
        let bytes = std::fs::read(&path).unwrap();
        let mut handler = HeadlessHandler::new();
        handler.handle(Call::MediaAdd { path }).unwrap();
        assert_eq!(
            handler.assets().get(&animathio_core::AssetId::of(&bytes)),
            Some(&bytes)
        );
        assert_ne!(handler.project().tracks(), []);
        assert!(handler.history_undo().unwrap());
        assert_eq!(handler.project().tracks(), []);
        assert!(!handler.history_undo().unwrap());
    }

    #[test]
    fn placing_the_same_file_twice_reuses_its_asset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("image.png");
        image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]))
            .save(&path)
            .unwrap();
        let mut handler = HeadlessHandler::new();
        handler
            .handle(Call::MediaAdd { path: path.clone() })
            .unwrap();
        let once = handler.project().clone();
        let assets = handler.assets().clone();
        handler.handle(Call::MediaAdd { path }).unwrap();
        assert_eq!(handler.project().assets().len(), 1);
        assert_eq!(handler.project().tracks().len(), 2);
        assert_eq!(handler.assets(), &assets);
        // Undoing the second placement keeps the first one and its asset.
        assert!(handler.history_undo().unwrap());
        assert_eq!(handler.project(), &once);
    }

    #[test]
    fn video_clip_takes_the_probed_media_length() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../media/tests/fixtures/sample.mp4");
        let bytes = std::fs::read(&path).unwrap();
        let expected = {
            use animathio_media::MediaBackend;
            animathio_media::FilmCraftBackend::new()
                .probe_bytes("sample.mp4", bytes.into())
                .unwrap()
                .duration
        };
        let mut handler = HeadlessHandler::new();
        handler.handle(Call::MediaAdd { path }).unwrap();
        let clip = handler.project().tracks()[0]
            .clips()
            .values()
            .next()
            .unwrap()
            .clone();
        assert_eq!(clip.range.end, expected);
    }

    #[test]
    fn failed_video_probe_retains_no_asset_or_edit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.mp4");
        std::fs::write(&path, b"invalid video").unwrap();
        let mut handler = HeadlessHandler::new();
        let before = handler.project_get().unwrap();
        assert!(handler.handle(Call::MediaAdd { path }).is_err());
        assert!(handler.assets().is_empty());
        assert_eq!(before, handler.project_get().unwrap());
        assert!(!handler.history_undo().unwrap());
    }
}
