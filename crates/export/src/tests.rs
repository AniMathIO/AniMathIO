#![allow(
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::chunks_exact_to_as_chunks,
    clippy::items_after_statements,
    clippy::similar_names,
    clippy::too_many_lines
)]

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
                assert_eq!(frame, expected, "encoder input at frame {index}");
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
                actual.extend_from_slice(&pcm);
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

#[test]
fn pipelined_export_is_byte_identical_to_serial_encoding() {
    let (project, assets) = project();
    let range = TimeRange::new(Time(0), Time(TICKS_PER_SECOND / 5)).unwrap();
    let rate = project.frame_rate();
    let mut serial = Exporter::new(&project, &assets).unwrap();
    let mut encoder = Mp4Encoder::new(EncoderConfig::with_audio(
        VideoConfig::new(160, 90, rate),
        AudioConfig::new(48_000, 2, 128_000),
    ))
    .unwrap();
    serial
        .encode_frames(
            &project,
            range,
            rate,
            &mut |_| true,
            &mut |scene| animathio_plugin_host::test_support::resolve(&scene, (160, 90)),
            |frame, pcm| {
                encoder.encode_video_frame(&frame.pixels)?;
                encoder.encode_audio(&pcm)?;
                Ok(())
            },
        )
        .unwrap();
    let expected = encoder.finish().unwrap();
    let actual =
        export_with_plugins(&project, &assets, range, Settings::default(), |_| true).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn light_project_background_survives_mp4_export() {
    let mut project = Project::default();
    Command::SetSettings {
        width: 160,
        height: 90,
        frame_rate: FrameRate::new(30, 1).unwrap(),
        background: [240, 240, 240, 255],
        duration: Time(TICKS_PER_SECOND / 30),
    }
    .apply(&mut project)
    .unwrap();
    let assets = BTreeMap::new();
    let bytes = Exporter::new(&project, &assets)
        .unwrap()
        .export(
            &project,
            TimeRange::new(Time(0), Time(TICKS_PER_SECOND / 30)).unwrap(),
            Settings::default(),
            |_| true,
            |scene| animathio_plugin_host::test_support::resolve(&scene, (160, 90)),
        )
        .unwrap();
    let decoder = FilmCraftBackend::new()
        .open_video_bytes("background.mp4", bytes.into())
        .unwrap();
    let frame = decoder.frame_at(Time(0)).unwrap();
    assert!(
        frame.pixels[..3]
            .iter()
            .all(|channel| channel.abs_diff(240) <= 5),
        "{:?}",
        &frame.pixels[..4]
    );
}

// Chapter embedding through the real exporter (faststart layout).

fn add_marker(project: &mut Project, id: u64, millis: i64, name: &str) {
    let ticks = millis * TICKS_PER_SECOND / 1000;
    let mut marker = animathio_core::Marker::new(animathio_core::MarkerId(id), Time(ticks));
    marker.name = name.into();
    Command::AddMarker { marker }.apply(project).unwrap();
}

fn chapter_settings(embed_chapters: bool) -> Settings {
    Settings {
        embed_chapters,
        ..Settings::default()
    }
}

fn decode_all(mp4: &[u8]) -> (Vec<Vec<u8>>, Vec<f32>) {
    let backend = FilmCraftBackend::new();
    let bytes: Arc<[u8]> = Arc::from(mp4);
    let video = backend
        .open_video_bytes("export.mp4", bytes.clone())
        .unwrap();
    let frames = (0..60)
        .map(|i| {
            let time = Time(i * TICKS_PER_SECOND / 30);
            video.frame_at(time).unwrap().pixels.to_vec()
        })
        .collect();
    let audio = backend.open_audio_bytes("export.mp4", bytes).unwrap();
    let samples = audio.samples(Time(0), 48_000).unwrap().to_vec();
    (frames, samples)
}

#[test]
fn chapters_embed_into_a_faststart_export_without_changing_decoded_media() {
    let (mut project, assets) = project();
    add_marker(&mut project, 1, 1500, "Outro");
    add_marker(&mut project, 2, 200, "Intro");
    add_marker(&mut project, 3, 800, "");
    let range = TimeRange::new(Time(0), duration(&project)).unwrap();
    let with =
        export_with_plugins(&project, &assets, range, chapter_settings(true), |_| true).unwrap();
    let without =
        export_with_plugins(&project, &assets, range, chapter_settings(false), |_| true).unwrap();

    // The file stays faststart (moov in front of mdat) and its tables point at the shifted mdat.
    assert!(with.len() > without.len());
    for (name, bytes) in [("with chapters", &with), ("without", &without)] {
        let kinds = chapters::top_level_kinds(bytes);
        let position = |kind: &[u8; 4]| kinds.iter().position(|k| k == kind);
        let (moov, mdat) = (position(b"moov"), position(b"mdat"));
        assert!(
            moov.is_some() && mdat.is_some() && moov < mdat,
            "{name}: moov must precede mdat, got {kinds:?}"
        );
    }
    assert_eq!(
        chapters::read_chpl(&with).unwrap(),
        vec![
            (2_000_000, "Intro".to_string()),
            (8_000_000, "Chapter 2".to_string()),
            (15_000_000, "Outro".to_string()),
        ]
    );
    assert!(chapters::read_chpl(&without).is_none());

    // Decoding both files yields identical frames and samples: every chunk offset still
    // lands on its sample.
    let (frames_with, audio_with) = decode_all(&with);
    let (frames_without, audio_without) = decode_all(&without);
    assert_eq!(frames_with.len(), 60);
    assert!(frames_with.iter().any(|f| f.iter().any(|&b| b != 0)));
    assert!(frames_with == frames_without, "decoded video frames differ");
    assert_eq!(audio_with.len(), 96_000);
    assert!(audio_with.iter().any(|s| s.abs() > 1e-3), "audio is silent");
    assert!(audio_with == audio_without, "decoded audio samples differ");
}

#[test]
fn chapter_times_are_relative_to_the_export_range() {
    let (mut project, assets) = project();
    add_marker(&mut project, 1, 100, "before the range");
    add_marker(&mut project, 2, 500, "First");
    add_marker(&mut project, 3, 1000, "Second");
    add_marker(&mut project, 4, 1900, "after the range");
    let range = TimeRange::new(Time(TICKS_PER_SECOND / 2), Time(TICKS_PER_SECOND * 3 / 2)).unwrap();
    let bytes =
        export_with_plugins(&project, &assets, range, Settings::default(), |_| true).unwrap();
    assert_eq!(
        chapters::read_chpl(&bytes).unwrap(),
        vec![(0, "First".to_string()), (5_000_000, "Second".to_string())]
    );
}

#[test]
fn no_chapters_without_markers_in_range_and_none_when_disabled() {
    let (mut project, assets) = project();
    let range = TimeRange::new(Time(0), Time(TICKS_PER_SECOND / 2)).unwrap();
    let none =
        export_with_plugins(&project, &assets, range, Settings::default(), |_| true).unwrap();
    assert!(chapters::read_chpl(&none).is_none());
    add_marker(&mut project, 1, 1000, "outside");
    let outside =
        export_with_plugins(&project, &assets, range, Settings::default(), |_| true).unwrap();
    assert_eq!(outside, none);
    add_marker(&mut project, 2, 100, "inside");
    let off =
        export_with_plugins(&project, &assets, range, chapter_settings(false), |_| true).unwrap();
    assert_eq!(off, none);
}

#[test]
fn settings_without_the_chapter_field_default_to_embedding() {
    let settings: Settings = serde_json::from_str(r#"{"frame_rate":null,"crf":20.0}"#).unwrap();
    assert!(settings.embed_chapters);
    let off: Settings =
        serde_json::from_str(r#"{"frame_rate":null,"crf":20.0,"embed_chapters":false}"#).unwrap();
    assert!(!off.embed_chapters);
}

// WebM (VP9 + Opus) export through the same pipeline.

fn webm_settings() -> Settings {
    Settings {
        format: Format::Webm,
        ..Settings::default()
    }
}

fn psnr_rgba(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    let mse = a
        .chunks_exact(4)
        .zip(b.chunks_exact(4))
        .flat_map(|(x, y)| (0..3).map(move |c| f64::from(x[c]) - f64::from(y[c])))
        .map(|d| d * d)
        .sum::<f64>()
        / (a.len() / 4 * 3) as f64;
    if mse == 0.0 {
        999.0
    } else {
        10.0 * (255.0_f64 * 255.0 / mse).log10()
    }
}

#[test]
fn webm_export_decodes_to_the_preview_frames_and_the_mixed_audio() {
    use animathio_media::{Matrix, MediaKind};
    let (project, assets) = project();
    let range = TimeRange::new(Time(0), duration(&project)).unwrap();
    let mut events = Vec::new();
    let bytes = export_with_plugins(&project, &assets, range, webm_settings(), |p| {
        events.push(p);
        true
    })
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
    assert!(bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]), "EBML header");

    let backend = FilmCraftBackend::new();
    let shared: Arc<[u8]> = Arc::from(bytes.clone());
    let info = backend.probe_bytes("export.webm", shared.clone()).unwrap();
    assert_eq!(info.kind, MediaKind::Movie);
    assert_eq!(info.container, "WebM");
    let video_info = info.video.clone().unwrap();
    assert_eq!((video_info.width, video_info.height), (160, 90));
    assert_eq!(video_info.codec, "VP9");
    assert_eq!(video_info.color.matrix, Matrix::Bt709);
    assert_eq!(info.audio.len(), 1);
    assert_eq!(info.audio[0].sample_rate, 48_000);
    assert_eq!(info.audio[0].channels, 2);
    assert!(info.audio[0].codec.to_lowercase().contains("opus"));
    // The video ends at 2 s; the audio track may run up to one 20 ms Opus packet longer (the
    // flushed encoder lag plus padding of the last packet; WebM has no DiscardPadding here).
    let half_frame = TICKS_PER_SECOND / 60;
    let opus_packet = TICKS_PER_SECOND / 50;
    assert!(
        info.duration.0 >= 2 * TICKS_PER_SECOND - half_frame
            && info.duration.0 <= 2 * TICKS_PER_SECOND + opus_packet,
        "duration {}",
        info.duration.0
    );

    // Frame n of the file is frame n of the preview renderer. Both codecs are lossy, and the
    // decode path has its own colour rounding, so the bar is the MP4 export of the same project.
    let mp4 = export_with_plugins(&project, &assets, range, Settings::default(), |_| true).unwrap();
    let mp4_decoder = backend
        .open_video_bytes("export.mp4", Arc::from(mp4))
        .unwrap();
    let decoder = backend
        .open_video_bytes("export.webm", shared.clone())
        .unwrap();
    let mut preview = RenderCtx::from_assets(assets.clone()).unwrap();
    let rate = project.frame_rate();
    for index in [0_i64, 1, 29, 30, 58, 59] {
        let time = rate.frame_to_time(index, Rounding::Nearest).unwrap();
        let expected = render(
            &animathio_plugin_host::test_support::resolve(&evaluate(&project, time), (160, 90)),
            &mut preview,
            (160, 90),
        )
        .unwrap();
        let decoded = decoder.frame_at(time).unwrap();
        assert_eq!((decoded.width, decoded.height), (160, 90));
        let psnr = psnr_rgba(&decoded.pixels, &expected.pixels);
        let mp4_psnr = psnr_rgba(
            &mp4_decoder.frame_at(time).unwrap().pixels,
            &expected.pixels,
        );
        assert!(
            psnr > 20.0 && psnr + 1.5 >= mp4_psnr,
            "frame {index}: WebM PSNR {psnr:.1} dB vs MP4 {mp4_psnr:.1} dB"
        );
    }

    // The container carries 60 video frames and one Opus packet per 20 ms (plus one that flushes
    // the encoder lag), ascending in time.
    let mut demuxer = filmcraft_matroska::Demuxer::from_slice(&bytes).unwrap();
    let delay = demuxer
        .tracks()
        .iter()
        .find(|t| t.kind == filmcraft_matroska::TrackKind::Audio)
        .unwrap()
        .codec_delay_ns as i64;
    let (mut video, mut audio, mut last) = (0, 0, i64::MIN / 2);
    while let Some(packet) = demuxer.next_packet().unwrap() {
        let is_audio = demuxer.tracks()[packet.track].kind == filmcraft_matroska::TrackKind::Audio;
        let stored = packet.pts_ns + if is_audio { delay } else { 0 };
        assert!(stored >= last - 500_000, "packets out of order");
        last = last.max(stored);
        if is_audio {
            audio += 1;
        } else {
            video += 1;
        }
    }
    assert_eq!((video, audio), (60, 101));

    // Audio: the decoded stream is the mixer's output (same lattice, pre-skip honoured), and the
    // timestamps line up: no lag between the decoded and the reference samples.
    let mut reference = Vec::new();
    Exporter::new(&project, &assets)
        .unwrap()
        .encode_frames(
            &project,
            range,
            rate,
            &mut |_| true,
            &mut |scene| animathio_plugin_host::test_support::resolve(&scene, (160, 90)),
            |_, pcm| {
                reference.extend_from_slice(&pcm);
                Ok(())
            },
        )
        .unwrap();
    let decoded = backend
        .open_audio_bytes("export.webm", shared)
        .unwrap()
        .samples(Time(0), 96_000)
        .unwrap();
    assert_eq!(decoded.len(), reference.len());
    let energy = |lag: i64| {
        let (mut signal, mut error) = (0.0_f64, 0.0_f64);
        for n in 2_000..94_000_i64 {
            let want = f64::from(reference[n as usize]);
            let got = f64::from(decoded[(n + lag) as usize]);
            signal += want * want;
            error += (want - got) * (want - got);
        }
        (signal, error)
    };
    let (signal, error) = energy(0);
    assert!(signal > 1.0, "the fixture audio is not silent");
    let snr = 10.0 * (signal / error).log10();
    assert!(snr > 10.0, "decoded audio SNR {snr:.1} dB");
    for lag in [-96_i64, -48, -8, 8, 48, 96] {
        assert!(energy(lag).1 > error, "lag {lag} fits better than zero lag");
    }
}

#[test]
fn webm_export_is_deterministic_and_pipelined_equals_serial() {
    let (project, assets) = project();
    let range = TimeRange::new(Time(0), Time(TICKS_PER_SECOND / 5)).unwrap();
    let rate = project.frame_rate();
    let mut serial = Exporter::new(&project, &assets).unwrap();
    let mut encoder = WebmEncoder::new(EncoderConfig::with_audio(
        VideoConfig::new(160, 90, rate),
        AudioConfig::new(48_000, 2, 128_000),
    ))
    .unwrap();
    serial
        .encode_frames(
            &project,
            range,
            rate,
            &mut |_| true,
            &mut |scene| animathio_plugin_host::test_support::resolve(&scene, (160, 90)),
            |frame, pcm| {
                encoder.encode_video_frame(&frame.pixels)?;
                encoder.encode_audio(&pcm)?;
                Ok(())
            },
        )
        .unwrap();
    let expected = encoder.finish().unwrap();
    let first = export_with_plugins(&project, &assets, range, webm_settings(), |_| true).unwrap();
    let second = export_with_plugins(&project, &assets, range, webm_settings(), |_| true).unwrap();
    assert_eq!(first, expected);
    assert_eq!(second, expected);
}

#[test]
fn webm_export_to_file_is_atomic_and_cancellable() {
    let (project, assets) = project();
    let range = TimeRange::new(Time(0), Time(TICKS_PER_SECOND / 3)).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("nested/clip.webm");
    let error = export_file_with_plugins(
        &project,
        &assets,
        range,
        webm_settings(),
        &Destination::new(&output),
        |progress| progress.completed < 2,
    )
    .unwrap_err();
    assert!(matches!(error, Error::Cancelled), "{error:?}");
    assert!(!output.exists());
    assert_eq!(
        entries(output.parent().unwrap()).len(),
        0,
        "no temp file survives"
    );

    let path = export_file_with_plugins(
        &project,
        &assets,
        range,
        webm_settings(),
        &Destination::new(&output),
        |_| true,
    )
    .unwrap();
    assert_eq!(path, output);
    assert_eq!(entries(output.parent().unwrap()), vec![output.clone()]);
    let info = FilmCraftBackend::new().probe(&output).unwrap();
    assert_eq!(info.container, "WebM");
    assert_eq!(info.video.unwrap().codec, "VP9");
    let rejected = export_file_with_plugins(
        &project,
        &assets,
        range,
        webm_settings(),
        &Destination::new(&output),
        |_| true,
    );
    assert!(
        matches!(rejected, Err(Error::AlreadyExists(_))),
        "{rejected:?}"
    );
}

#[test]
fn format_and_file_extension_must_agree() {
    let (project, assets) = project();
    let range = TimeRange::new(Time(0), Time(TICKS_PER_SECOND / 10)).unwrap();
    let dir = tempfile::tempdir().unwrap();
    for (name, settings, wanted) in [
        ("clip.webm", Settings::default(), "ends in .webm"),
        ("clip.MP4", webm_settings(), "ends in .mp4"),
    ] {
        let output = dir.path().join(name);
        let error = export_file_with_plugins(
            &project,
            &assets,
            range,
            settings,
            &Destination::new(&output),
            |_| true,
        )
        .unwrap_err();
        assert!(
            matches!(&error, Error::Invalid(message) if message.contains(wanted)),
            "{error:?}"
        );
        assert!(!output.exists());
    }
    assert_eq!(entries(dir.path()).len(), 0);

    use std::path::Path;
    assert_eq!(Format::for_path(Path::new("a/b.WebM")), Some(Format::Webm));
    assert_eq!(Format::for_path(Path::new("b.mp4")), Some(Format::Mp4));
    assert_eq!(Format::for_path(Path::new("b.mov")), None);
    assert_eq!(
        Format::resolve(None, Path::new("b.webm")).unwrap(),
        Format::Webm
    );
    assert_eq!(Format::resolve(None, Path::new("b")).unwrap(), Format::Mp4);
    assert_eq!(
        Format::resolve(Some(Format::Webm), Path::new("b.mkv")).unwrap(),
        Format::Webm
    );
    assert!(Format::resolve(Some(Format::Mp4), Path::new("b.webm")).is_err());
    assert_eq!("WEBM".parse::<Format>().unwrap(), Format::Webm);
    assert!(
        matches!("avi".parse::<Format>(), Err(Error::Invalid(m)) if m.contains("mp4 and webm"))
    );
}

#[test]
fn settings_format_defaults_to_mp4_and_round_trips() {
    let old: Settings = serde_json::from_str(r#"{"frame_rate":null,"crf":20.0}"#).unwrap();
    assert_eq!(old.format, Format::Mp4);
    let webm: Settings =
        serde_json::from_str(r#"{"frame_rate":null,"crf":20.0,"format":"webm"}"#).unwrap();
    assert_eq!(webm.format, Format::Webm);
    assert_eq!(
        serde_json::to_value(webm).unwrap()["format"],
        serde_json::json!("webm")
    );
    assert!(
        serde_json::from_str::<Settings>(r#"{"frame_rate":null,"crf":20.0,"format":"avi"}"#)
            .is_err()
    );
}

#[test]
fn webm_carries_no_chapters_and_ignores_the_setting() {
    let (mut project, assets) = project();
    add_marker(&mut project, 1, 500, "Intro");
    let range = TimeRange::new(Time(0), Time(TICKS_PER_SECOND / 2)).unwrap();
    let with = export_with_plugins(&project, &assets, range, webm_settings(), |_| true).unwrap();
    let without = export_with_plugins(
        &project,
        &assets,
        range,
        Settings {
            embed_chapters: false,
            ..webm_settings()
        },
        |_| true,
    )
    .unwrap();
    assert_eq!(with, without);
    assert!(chapters::read_chpl(&with).is_none());
}

#[test]
fn webm_rejects_odd_dimensions_with_a_clear_error() {
    let (mut project, assets) = project();
    Command::SetSettings {
        width: 161,
        height: 91,
        frame_rate: project.frame_rate(),
        background: [0, 0, 0, 255],
        duration: project.duration(),
    }
    .apply(&mut project)
    .unwrap();
    let range = TimeRange::new(Time(0), Time(TICKS_PER_SECOND / 10)).unwrap();
    let error =
        export_with_plugins(&project, &assets, range, webm_settings(), |_| true).unwrap_err();
    assert!(error.to_string().contains("even"), "{error}");
}

#[test]
fn webm_export_decodes_cleanly_with_libvpx_and_libopus_when_ffmpeg_is_installed() {
    // ffmpeg is only a decoder oracle here (the export never runs it); skipped when absent.
    let Ok(version) = Process::new("ffprobe").arg("-version").output() else {
        eprintln!("ffprobe absent; independent WebM decode check skipped");
        return;
    };
    assert!(version.status.success());
    let (project, assets) = project();
    let range = TimeRange::new(Time(0), duration(&project)).unwrap();
    let bytes = export_with_plugins(&project, &assets, range, webm_settings(), |_| true).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("export.webm");
    std::fs::write(&path, &bytes).unwrap();
    let probe = Process::new("ffprobe")
        .args([
            "-v",
            "error",
            "-count_frames",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
        ])
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        probe.status.success(),
        "{}",
        String::from_utf8_lossy(&probe.stderr)
    );
    let probe: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
    let streams = probe["streams"].as_array().unwrap();
    let video = streams.iter().find(|s| s["codec_type"] == "video").unwrap();
    let audio = streams.iter().find(|s| s["codec_type"] == "audio").unwrap();
    assert_eq!(video["codec_name"], "vp9");
    assert_eq!(audio["codec_name"], "opus");
    assert_eq!(video["nb_read_frames"], "60");
    assert_eq!(video["color_space"], "bt709");
    let seconds = |v: &serde_json::Value| v.as_str().unwrap().parse::<f64>().unwrap();
    assert!((seconds(&probe["format"]["duration"]) - 2.0).abs() <= 0.02);
    assert!(seconds(&video["start_time"]).abs() <= 0.001);
    // Newer ffmpeg applies the Opus CodecDelay to the start time (0); older releases report the
    // first packet's timestamp, one pre-skip (2.5 ms, printed as -0.003) before zero. Either way it is
    // not later.
    let audio_start = seconds(&audio["start_time"]);
    assert!(
        (-0.0035..=0.001).contains(&audio_start),
        "audio start_time {audio_start}"
    );
    // Both streams decode end to end without a single warning.
    let decode = Process::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(&path)
        .args(["-f", "null", "-"])
        .output()
        .unwrap();
    assert!(decode.status.success());
    let stderr = String::from_utf8_lossy(&decode.stderr);
    // FFmpeg 8.0 (only that series; fixed upstream in 618fc15e65f5, first released in 9.1)
    // prints exactly one spurious "[opus @ ..] Error parsing Opus packet header." when its
    // Matroska demuxer flushes the Opus parser with a zero-length buffer at EOF
    // (PARSER_FLAG_COMPLETE_FRAMES, FFmpeg ticket #20954). It prints the line for its own
    // libopus-encoded WebM too, and the stream stats still show every packet read and
    // decoded, so on 8.0.x that single line says nothing about our file. Anything else — or
    // the same line on any other ffmpeg — still fails: a genuinely broken packet makes the
    // parser log the message again ("Last message repeated N times") or the decoder log
    // "Error parsing the packet header.", so the tolerance cannot hide real corruption.
    let lines: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).collect();
    let ff_8_0 = String::from_utf8_lossy(&version.stdout)
        .split_whitespace()
        .skip_while(|w| *w != "version")
        .nth(1)
        .is_some_and(|v| v.starts_with("8.0"));
    let known_spurious_flush = ff_8_0
        && lines.len() == 1
        && lines[0].contains("[opus @ ")
        && lines[0].contains("Error parsing Opus packet header.");
    if !stderr.is_empty() && !known_spurious_flush {
        // Name the offending packets: index, timestamp, size and the first bytes (the Opus TOC).
        let mut demuxer = filmcraft_matroska::Demuxer::from_slice(&bytes).unwrap();
        let mut packets = Vec::new();
        let mut index = 0;
        while let Some(packet) = demuxer.next_packet().unwrap() {
            if demuxer.tracks()[packet.track].kind == filmcraft_matroska::TrackKind::Audio {
                let head: Vec<String> = packet
                    .data
                    .iter()
                    .take(4)
                    .map(|b| format!("{b:02x}"))
                    .collect();
                packets.push(format!(
                    "#{index} pts {} ns len {} [{}]",
                    packet.pts_ns,
                    packet.data.len(),
                    head.join(" ")
                ));
                index += 1;
            }
        }
        panic!("{}\naudio packets:\n{}", stderr, packets.join("\n"));
    }
}
