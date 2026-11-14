//! Atomic media placement from one bounded read and one probe.
use crate::{AssetInfo, ControlError, media_type_of, read_bounded, validate_existing_file};
use animathio_core::{
    Asset, AssetId, Clip, ClipId, ClipKind, Command, Project, TICKS_PER_SECOND, Time, TimeRange,
    Track, TrackId,
};
use animathio_media::MediaBackend;
use std::{path::Path, sync::Arc};

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
    let mut clip = Clip::new(
        clip_id,
        kind,
        TimeRange::new(
            Time(0),
            if project.duration().0 > 0 {
                project.duration()
            } else {
                Time(TICKS_PER_SECOND * 3)
            },
        )
        .map_err(|e| ControlError::Other(e.to_string()))?,
    );
    if matches!(clip.kind, ClipKind::Audio(_)) {
        clip.params.insert("gain".into(), 1.0.into());
        clip.params.insert("muted".into(), 0.0.into());
    }
    let mut commands = vec![
        Command::AddAsset {
            id: info.id.clone(),
            asset: Asset {
                name: info.name.clone(),
                media_type: info.media_type.clone(),
            },
        },
        Command::AddTrack {
            track: Track::new(track_id, info.name.clone()),
            index: project.tracks().len(),
        },
        Command::AddClip {
            track: track_id,
            clip: clip.clone(),
        },
    ];
    if matches!(clip.kind, ClipKind::Video(_)) {
        let info_media = animathio_media::FilmCraftBackend::new()
            .probe_bytes(&info.name, Arc::from(bytes.as_slice()))
            .map_err(|e| ControlError::Other(e.to_string()))?;
        if !info_media.audio.is_empty() {
            let audio_track = TrackId(base + 2);
            let mut audio = Clip::new(ClipId(base + 3), ClipKind::Audio(info.id), clip.range);
            audio.link_group = Some(clip_id);
            audio.params.insert("gain".into(), 1.0.into());
            audio.params.insert("muted".into(), 0.0.into());
            commands.push(Command::SetClipLink {
                id: clip_id,
                group: Some(clip_id),
            });
            commands.push(Command::AddTrack {
                track: Track::new(audio_track, "Audio"),
                index: project.tracks().len() + 1,
            });
            commands.push(Command::AddClip {
                track: audio_track,
                clip: audio,
            });
        }
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
    fn placement_failure_preserves_existing_assets_and_history() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("image.png");
        image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]))
            .save(&path)
            .unwrap();
        let mut handler = HeadlessHandler::new();
        handler
            .handle(Call::MediaAdd { path: path.clone() })
            .unwrap();
        let before = handler.project_get().unwrap();
        let assets = handler.assets().clone();
        assert!(handler.handle(Call::MediaAdd { path }).is_err());
        assert_eq!(before, handler.project_get().unwrap());
        assert_eq!(handler.assets(), &assets);
        assert!(handler.history_undo().unwrap());
        assert_eq!(handler.project().tracks(), []);
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
