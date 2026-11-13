use super::*;
use animathio_core::{Asset, Clip, ClipId, Command, Track, TrackId};
use std::process::Command as Process;

fn project() -> (Project, BTreeMap<AssetId, Vec<u8>>) {
    let bytes = include_bytes!("../../media/tests/fixtures/sample.mp4").to_vec();
    let id = AssetId::of(&bytes);
    let mut project = Project::default();
    let rate = FrameRate::new(30, 1).unwrap();
    Command::SetSettings {
        width: 160,
        height: 90,
        frame_rate: rate,
        background: [0, 0, 0, 255],
        duration: Time(TICKS_PER_SECOND * 2),
    }
    .apply(&mut project)
    .unwrap();
    Command::AddAsset {
        id: id.clone(),
        asset: Asset {
            name: "fixture.mp4".into(),
            media_type: "video/mp4".into(),
        },
    }
    .apply(&mut project)
    .unwrap();
    let range = TimeRange::new(Time(0), Time(TICKS_PER_SECOND * 2)).unwrap();
    for (index, kind) in [
        ClipKind::Video(id.clone()),
        animathio_plugin_api::sources::text(
            "Video + text + audio".into(),
            0.12,
            [255, 255, 255, 255],
        ),
        ClipKind::Audio(id.clone()),
    ]
    .into_iter()
    .enumerate()
    {
        let index = u64::try_from(index).unwrap();
        Command::AddTrack {
            track: Track::new(TrackId(index), "Layer"),
            index: usize::try_from(index).unwrap(),
        }
        .apply(&mut project)
        .unwrap();
        Command::AddClip {
            track: TrackId(index),
            clip: Clip::new(ClipId(index), kind, range),
        }
        .apply(&mut project)
        .unwrap();
    }
    (project, BTreeMap::from([(id, bytes)]))
}

#[test]
fn two_second_video_text_audio_export_has_aligned_streams() {
    let (project, assets) = project();
    let range = TimeRange::new(Time(0), duration(&project)).unwrap();
    let mut events = Vec::new();
    let bytes = Exporter::new(&project, &assets)
        .unwrap()
        .export(
            &project,
            range,
            Settings::default(),
            |p| {
                events.push(p);
                true
            },
            |scene| animathio_plugin_host::test_support::resolve(&scene, (160, 90)),
        )
        .unwrap();
    assert_eq!(
        events.first(),
        Some(&Progress {
            completed: 0,
            total: 60
        })
    );
    assert_eq!(
        events.last(),
        Some(&Progress {
            completed: 60,
            total: 60
        })
    );
    assert_eq!(events.len(), 61);
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("export.mp4");
    std::fs::write(&path, &bytes).unwrap();
    // Always check our own decoder, including explicitly tagged BT.709.
    let info = FilmCraftBackend::new().probe(&path).unwrap();
    assert_eq!(info.audio.len(), 1);
    assert_eq!(
        info.video.unwrap().color.matrix,
        animathio_media::Matrix::Bt709
    );
    let Ok(probe) = Process::new("ffprobe")
        .args(["-v", "error", "-show_streams", "-of", "json"])
        .arg(&path)
        .output()
    else {
        eprintln!("ffprobe absent; stream timing check skipped");
        return;
    };
    assert!(
        probe.status.success(),
        "{}",
        String::from_utf8_lossy(&probe.stderr)
    );
    let probe: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
    let streams = probe["streams"].as_array().unwrap();
    assert_eq!(streams.len(), 2);
    let video = streams.iter().find(|s| s["codec_type"] == "video").unwrap();
    let audio = streams.iter().find(|s| s["codec_type"] == "audio").unwrap();
    assert_eq!(video["codec_name"], "h264");
    assert_eq!(audio["codec_name"], "aac");
    assert_eq!(video["nb_frames"], "60");
    assert_eq!(video["color_space"], "bt709");
    let number =
        |s: &serde_json::Value, key: &str| s[key].as_str().unwrap().parse::<f64>().unwrap();
    let half_frame = 0.5 / 30.0;
    assert!((number(video, "duration") - 2.0).abs() <= half_frame);
    assert!((number(audio, "duration") - 2.0).abs() <= half_frame);
    assert!((number(video, "start_time") - number(audio, "start_time")).abs() <= half_frame);
}

#[test]
fn encoder_input_frame_n_is_exactly_preview_frame_n() {
    let (project, assets) = project();
    let mut exporter = Exporter::new(&project, &assets).unwrap();
    let mut preview = RenderCtx::from_assets(assets.clone()).unwrap();
    let rate = FrameRate::new(30, 1).unwrap();
    // A nonzero subrange exercises source offsets and absolute timeline evaluation.
    let range = TimeRange::new(Time(TICKS_PER_SECOND / 2), Time(TICKS_PER_SECOND)).unwrap();
    let mut index = 0;
    exporter
        .encode_frames(
            &project,
            range,
            rate,
            &mut |_| true,
            &mut |scene| animathio_plugin_host::test_support::resolve(&scene, (160, 90)),
            |frame, pcm| {
                let offset = rate.frame_to_time(index, Rounding::Nearest).unwrap();
                let expected = render(
                    &animathio_plugin_host::test_support::resolve(
                        &evaluate(&project, Time(range.start.0 + offset.0)),
                        (160, 90),
                    ),
                    &mut preview,
                    (160, 90),
                )
                .unwrap();
                assert_eq!(*frame, expected, "encoder input at frame {index}");
                assert_ne!(pcm.len(), 0);
                index += 1;
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(index, 15);
}

#[test]
fn cancellation_and_invalid_ranges_fail_without_finishing() {
    let (project, assets) = project();
    let range = TimeRange::new(Time(0), duration(&project)).unwrap();
    assert!(matches!(
        export_with_plugins(&project, &assets, range, Settings::default(), |_| false),
        Err(Error::Cancelled)
    ));
    let mut last = 0;
    assert!(matches!(
        export_with_plugins(&project, &assets, range, Settings::default(), |p| {
            last = p.completed;
            p.completed < 2
        }),
        Err(Error::Cancelled)
    ));
    assert_eq!(last, 2);
    for range in [
        TimeRange {
            start: Time(1),
            end: Time(1),
        },
        TimeRange {
            start: Time(-1),
            end: Time(10),
        },
        TimeRange {
            start: Time(0),
            end: Time(i64::MAX),
        },
    ] {
        assert!(
            export_with_plugins(&project, &assets, range, Settings::default(), |_| true).is_err()
        );
    }
}

#[test]
fn unset_resolution_falls_back_to_640x360() {
    let mut project = Project::default();
    assert_eq!(project.width(), 0);
    assert_eq!(project.height(), 0);
    let range = TimeRange::new(Time(0), Time(TICKS_PER_SECOND)).unwrap();
    Command::AddTrack {
        track: Track::new(TrackId(0), "Text"),
        index: 0,
    }
    .apply(&mut project)
    .unwrap();
    Command::AddClip {
        track: TrackId(0),
        clip: Clip::new(
            ClipId(0),
            animathio_plugin_api::sources::text("default canvas".into(), 0.1, [255, 255, 255, 255]),
            range,
        ),
    }
    .apply(&mut project)
    .unwrap();
    let bytes = export_with_plugins(
        &project,
        &BTreeMap::new(),
        TimeRange::new(Time(0), Time(TICKS_PER_SECOND / 2)).unwrap(),
        Settings::default(),
        |_| true,
    )
    .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("fallback.mp4");
    std::fs::write(&path, &bytes).unwrap();
    let video = FilmCraftBackend::new().probe(&path).unwrap().video.unwrap();
    assert_eq!((video.width, video.height), (640, 360));
}

#[test]
fn rational_frame_steps_and_audio_blocks_are_deterministic() {
    let (project, assets) = project();
    let mut exporter = Exporter::new(&project, &assets).unwrap();
    let rate = FrameRate::new(30000, 1001).unwrap();
    let range = TimeRange::new(Time(1001), Time(TICKS_PER_SECOND + 1001)).unwrap();
    let expected = exporter.mixer.mix(&project, range).unwrap();
    let mut actual = Vec::new();
    exporter
        .encode_frames(
            &project,
            range,
            rate,
            &mut |_| true,
            &mut |scene| animathio_plugin_host::test_support::resolve(&scene, (160, 90)),
            |_, pcm| {
                actual.extend_from_slice(pcm);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(
        actual, expected,
        "audio uses one global sample lattice across video boundaries"
    );
}

/// Entries in `dir`, sorted; used to prove no temp file survives an export.
fn entries(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    paths.sort();
    paths
}

#[test]
fn export_to_file_creates_missing_parents_and_never_overwrites_silently() {
    let (project, assets) = project();
    let range = TimeRange::new(Time(0), Time(TICKS_PER_SECOND / 3)).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("nested/deeper/clip.mp4");
    let path = export_file_with_plugins(
        &project,
        &assets,
        range,
        Settings::default(),
        &Destination::new(&output),
        |_| true,
    )
    .unwrap();
    assert_eq!(path, output);
    assert!(output.is_file());
    // Only the finished MP4 is left behind; no temporary file survives.
    assert_eq!(entries(output.parent().unwrap()), vec![output.clone()]);

    let original = std::fs::read(&output).unwrap();
    let rejected = export_file_with_plugins(
        &project,
        &assets,
        range,
        Settings::default(),
        &Destination::new(&output),
        |_| true,
    );
    assert!(
        matches!(rejected, Err(Error::AlreadyExists(ref path)) if *path == output),
        "{rejected:?}"
    );
    assert_eq!(
        std::fs::read(&output).unwrap(),
        original,
        "a rejected export must not touch the existing file"
    );
    assert_eq!(entries(output.parent().unwrap()), vec![output.clone()]);

    std::fs::write(&output, b"not an mp4").unwrap();
    export_file_with_plugins(
        &project,
        &assets,
        range,
        Settings::default(),
        &Destination::new(&output).overwriting(),
        |_| true,
    )
    .unwrap();
    let video = FilmCraftBackend::new()
        .probe(&output)
        .unwrap()
        .video
        .unwrap();
    assert_eq!((video.width, video.height), (160, 90));
    assert_eq!(entries(output.parent().unwrap()), vec![output.clone()]);
}

#[test]
fn failed_export_leaves_no_partial_file_and_no_temp_file() {
    let (project, assets) = project();
    let range = TimeRange::new(Time(0), Time(TICKS_PER_SECOND / 3)).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("fresh/clip.mp4");
    let error = export_file_with_plugins(
        &project,
        &assets,
        range,
        Settings::default(),
        &Destination::new(&output),
        |progress| progress.completed < 2,
    )
    .unwrap_err();
    assert!(matches!(error, Error::Cancelled), "{error:?}");
    assert!(!output.exists());
    assert_eq!(entries(&dir.path().join("fresh")).len(), 0);

    // A cancelled overwrite keeps the previous file and drops its temp file.
    std::fs::write(&output, b"previous").unwrap();
    let error = export_file_with_plugins(
        &project,
        &assets,
        range,
        Settings::default(),
        &Destination::new(&output).overwriting(),
        |_| false,
    )
    .unwrap_err();
    assert!(matches!(error, Error::Cancelled), "{error:?}");
    assert_eq!(std::fs::read(&output).unwrap(), b"previous");
    assert_eq!(entries(&dir.path().join("fresh")), vec![output.clone()]);
}

fn export_with_plugins(
    project: &Project,
    assets: &BTreeMap<AssetId, Vec<u8>>,
    range: TimeRange,
    settings: Settings,
    progress: impl FnMut(Progress) -> bool,
) -> Result<Vec<u8>, Error> {
    Exporter::new(project, assets)?.export(project, range, settings, progress, |scene| {
        animathio_plugin_host::test_support::resolve(&scene, resolution(project))
    })
}

fn export_file_with_plugins(
    project: &Project,
    assets: &BTreeMap<AssetId, Vec<u8>>,
    range: TimeRange,
    settings: Settings,
    destination: &Destination,
    progress: impl FnMut(Progress) -> bool,
) -> Result<std::path::PathBuf, Error> {
    Exporter::new(project, assets)?.export_to_file(
        project,
        range,
        settings,
        destination,
        progress,
        |scene| animathio_plugin_host::test_support::resolve(&scene, resolution(project)),
    )
}

#[test]
fn export_fails_instead_of_baking_placeholders_for_unresolved_plugins() {
    let (project, assets) = project();
    let range = TimeRange::new(Time(0), Time(TICKS_PER_SECOND / 10)).unwrap();
    // No resolver at all: the text clip stays a plugin clip.
    let error = Exporter::new(&project, &assets)
        .unwrap()
        .export(
            &project,
            range,
            Settings::default(),
            |_| true,
            |scene| scene,
        )
        .unwrap_err();
    let message = error.to_string();
    assert!(matches!(error, Error::Plugin { clip: 1, .. }), "{message}");
    assert!(message.contains("clip 1"), "{message}");
    assert!(message.contains("org.animathio.text"), "{message}");

    // A resolver whose plugin trapped turns the clip into an error clip.
    let error = Exporter::new(&project, &assets)
        .unwrap()
        .export(
            &project,
            range,
            Settings::default(),
            |_| true,
            |mut scene| {
                for clip in &mut scene.clips {
                    if matches!(clip.kind, ClipKind::Plugin { .. }) {
                        clip.kind = ClipKind::Error("Plugin trapped: out of fuel".into());
                    }
                }
                scene
            },
        )
        .unwrap_err();
    assert!(error.to_string().contains("out of fuel"), "{error}");
    assert!(error.to_string().contains("org.animathio.text"), "{error}");
}

#[test]
fn project_error_clips_still_export() {
    // An error clip authored in the project (not produced by a failed plugin) is
    // content, not a plugin failure.
    let (mut project, assets) = project();
    Command::AddTrack {
        track: Track::new(TrackId(9), "Err"),
        index: 3,
    }
    .apply(&mut project)
    .unwrap();
    Command::AddClip {
        track: TrackId(9),
        clip: Clip::new(
            ClipId(9),
            ClipKind::Error("authored".into()),
            TimeRange::new(Time(0), Time(TICKS_PER_SECOND)).unwrap(),
        ),
    }
    .apply(&mut project)
    .unwrap();
    let range = TimeRange::new(Time(0), Time(TICKS_PER_SECOND / 10)).unwrap();
    Exporter::new(&project, &assets)
        .unwrap()
        .export(
            &project,
            range,
            Settings::default(),
            |_| true,
            |scene| animathio_plugin_host::test_support::resolve(&scene, (160, 90)),
        )
        .unwrap();
}
