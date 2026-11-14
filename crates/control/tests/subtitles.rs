//! Subtitle clips, SRT/WebVTT export and import, without a speech model.
use animathio_control::{
    Call, ControlError, ControlHandler, HeadlessHandler, SubtitleExportParams,
    SubtitleGenerateParams, SubtitleImportParams, apply_cues, subtitle_command, track_cues,
};
use animathio_core::{
    Clip, ClipId, Command, History, Project, TICKS_PER_SECOND, Time, TimeRange, Track, TrackId,
};
use animathio_format::Bundle;
use animathio_transcribe::Cue;
use std::collections::BTreeMap;

const MS: i64 = TICKS_PER_SECOND / 1000;

fn cue(start: i64, end: i64, text: &str) -> Cue {
    Cue {
        start: Time(start * MS),
        end: Time(end * MS),
        text: text.to_owned(),
    }
}

fn cues() -> Vec<Cue> {
    vec![
        cue(500, 2000, "Hello world"),
        cue(2500, 4000, "Second cue\nwith two lines"),
        cue(4500, 6000, "Third"),
    ]
}

fn project_with_track() -> Project {
    let mut project = Project::default();
    Command::AddTrack {
        track: Track::new(TrackId(1), "Video"),
        index: 0,
    }
    .apply(&mut project)
    .unwrap();
    Command::AddClip {
        track: TrackId(1),
        clip: Clip::new(
            ClipId(2),
            animathio_plugin_api::sources::text("Title".into(), 0.1, [255; 4]),
            TimeRange::new(Time(0), Time(10_000 * MS)).unwrap(),
        ),
    }
    .apply(&mut project)
    .unwrap();
    project
}

#[test]
fn subtitle_command_adds_two_tracks_with_linked_pairs() {
    let project = project_with_track();
    let (command, added) = subtitle_command(&project, &cues()).unwrap();
    let mut applied = project.clone();
    command.apply(&mut applied).unwrap();
    assert_eq!(applied.tracks().len(), 3);
    let names: Vec<_> = applied.tracks().iter().map(Track::name).collect();
    assert_eq!(names, ["Video", "Subtitles background", "Subtitles"]);
    let text = &applied.tracks()[2];
    let bars = &applied.tracks()[1];
    assert_eq!(text.id(), added.track_id);
    assert_eq!(bars.id(), added.background_track_id);
    assert_eq!(
        (text.clips().len(), bars.clips().len(), added.cues),
        (3, 3, 3)
    );
    assert!(text.clips().contains_key(&added.first_clip));
    // Each text clip is linked with exactly one bar over the same interval, with fades.
    for clip in text.clips().values() {
        let partner: Vec<_> = bars
            .clips()
            .values()
            .filter(|b| b.link_group == clip.link_group)
            .collect();
        assert_eq!(partner.len(), 1);
        assert_eq!(partner[0].range, clip.range);
        assert!(clip.link_group.is_some());
        assert!(!clip.animations.is_empty(), "fade keyframes expected");
    }
    // Clip ids are unique and cues are carried verbatim.
    let (_, read) = track_cues(&applied, None).unwrap();
    assert_eq!(read, cues());
}

#[test]
fn generating_twice_names_the_second_track_uniquely() {
    let mut project = project_with_track();
    let mut history = History::default();
    apply_cues(&mut project, &mut history, "en".into(), &cues()).unwrap();
    apply_cues(&mut project, &mut history, "en".into(), &cues()).unwrap();
    let names: Vec<_> = project.tracks().iter().map(Track::name).collect();
    assert_eq!(
        names,
        [
            "Video",
            "Subtitles background",
            "Subtitles",
            "Subtitles 2 background",
            "Subtitles 2"
        ]
    );
}

#[test]
fn adding_subtitles_is_one_undo_step() {
    let mut project = project_with_track();
    let before = project.clone();
    let mut history = History::default();
    let result = apply_cues(&mut project, &mut history, "en".into(), &cues()).unwrap();
    assert_eq!(result.added.unwrap().cues, 3);
    assert_ne!(project, before);
    assert!(history.undo(&mut project).unwrap());
    assert_eq!(project, before);
    assert!(history.redo(&mut project).unwrap());
    assert_eq!(project.tracks().len(), 3);
}

#[test]
fn empty_cue_list_changes_nothing() {
    let mut project = project_with_track();
    let before = project.clone();
    let mut history = History::default();
    let result = apply_cues(&mut project, &mut history, "en".into(), &[]).unwrap();
    assert!(result.added.is_none());
    assert_eq!(project, before);
    assert!(!history.undo(&mut project).unwrap());
}

fn handler() -> HeadlessHandler {
    let mut handler = HeadlessHandler::from_bundle(
        Bundle {
            project: project_with_track(),
            assets: BTreeMap::new(),
        },
        None,
    );
    let (command, _) = subtitle_command(&handler.project_get().unwrap().project, &cues()).unwrap();
    handler.command_apply(command).unwrap();
    handler
}

#[test]
fn export_returns_text_or_writes_a_file_for_both_formats() {
    let mut handler = handler();
    let srt = handler
        .subtitles_export(SubtitleExportParams {
            format: "srt".into(),
            output: None,
            track: None,
            overwrite: false,
        })
        .unwrap();
    assert_eq!(srt.cues, 3);
    assert!(
        srt.text
            .as_deref()
            .unwrap()
            .starts_with("1\n00:00:00,500 --> 00:00:02,000\nHello world")
    );

    let dir = tempfile::tempdir().unwrap();
    let vtt_path = dir.path().join("out.vtt");
    let vtt = handler
        .subtitles_export(SubtitleExportParams {
            format: "srt".into(), // the .vtt extension wins over the default format
            output: Some(vtt_path.clone()),
            track: None,
            overwrite: false,
        })
        .unwrap();
    assert_eq!((vtt.format.as_str(), vtt.text), ("vtt", None));
    let written = std::fs::read_to_string(&vtt_path).unwrap();
    assert!(written.starts_with("WEBVTT\n\n00:00:00.500 --> 00:00:02.000"));
    // An existing file is protected unless overwrite is set.
    let again = SubtitleExportParams {
        format: "vtt".into(),
        output: Some(vtt_path),
        track: None,
        overwrite: false,
    };
    assert!(matches!(
        handler.subtitles_export(again.clone()),
        Err(ControlError::AlreadyExists(_))
    ));
    handler
        .subtitles_export(SubtitleExportParams {
            overwrite: true,
            ..again
        })
        .unwrap();
}

#[test]
fn export_then_import_round_trips_the_cues() {
    let mut source = handler();
    let dir = tempfile::tempdir().unwrap();
    for name in ["round.srt", "round.vtt"] {
        let path = dir.path().join(name);
        source
            .subtitles_export(SubtitleExportParams {
                format: "srt".into(),
                output: Some(path.clone()),
                track: None,
                overwrite: false,
            })
            .unwrap();
        let mut target = HeadlessHandler::from_bundle(
            Bundle {
                project: project_with_track(),
                assets: BTreeMap::new(),
            },
            None,
        );
        let result = target
            .subtitles_import(SubtitleImportParams {
                path,
                offset: Time(0),
            })
            .unwrap();
        assert_eq!(result.cues.len(), 3);
        let (_, read) = track_cues(&target.project_get().unwrap().project, None).unwrap();
        assert_eq!(read, cues());
        assert!(target.history_undo().unwrap());
        assert_eq!(target.project_get().unwrap().project.tracks().len(), 1);
    }
}

#[test]
fn import_offset_shifts_and_clamps() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.srt");
    std::fs::write(
        &path,
        "1\n00:00:01,000 --> 00:00:02,000\nA\n\n2\n00:00:03,000 --> 00:00:04,000\nB\n",
    )
    .unwrap();
    let mut handler = HeadlessHandler::from_bundle(
        Bundle {
            project: project_with_track(),
            assets: BTreeMap::new(),
        },
        None,
    );
    let result = handler
        .subtitles_import(SubtitleImportParams {
            path,
            offset: Time(-1500 * MS),
        })
        .unwrap();
    // A ends at 0.5 s (kept, clamped to start at 0), B at 1.5-2.5 s.
    assert_eq!(result.cues.len(), 2);
    assert_eq!(result.cues[0].start, Time(0));
    assert_eq!(result.cues[1].start, Time(1500 * MS));
}

#[test]
fn export_without_subtitles_and_bad_import_are_errors() {
    let mut empty = HeadlessHandler::from_bundle(
        Bundle {
            project: project_with_track(),
            assets: BTreeMap::new(),
        },
        None,
    );
    let err = empty
        .subtitles_export(SubtitleExportParams {
            format: "srt".into(),
            output: None,
            track: None,
            overwrite: false,
        })
        .unwrap_err();
    assert!(err.to_string().contains("Subtitles"), "{err}");
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.srt");
    std::fs::write(&bad, "1\nnot a time --> nope\nX\n").unwrap();
    assert!(
        empty
            .subtitles_import(SubtitleImportParams {
                path: bad,
                offset: Time(0)
            })
            .is_err()
    );
}

#[test]
fn generating_without_audio_fails_before_any_download() {
    let mut handler = HeadlessHandler::from_bundle(
        Bundle {
            project: project_with_track(),
            assets: BTreeMap::new(),
        },
        None,
    );
    let err = handler
        .handle(Call::SubtitlesGenerate {
            params: SubtitleGenerateParams {
                download_model: true,
                ..SubtitleGenerateParams::default()
            },
        })
        .unwrap_err();
    assert!(err.to_string().contains("no audio"), "{err}");
    let err = handler
        .subtitles_generate(SubtitleGenerateParams {
            model: "gigantic".into(),
            ..SubtitleGenerateParams::default()
        })
        .unwrap_err();
    assert!(matches!(err, ControlError::InvalidParams(_)), "{err}");
}

#[test]
fn unknown_clip_kind_is_not_exported() {
    let mut project = project_with_track();
    Command::AddTrack {
        track: Track::new(TrackId(9), "Subtitles"),
        index: 1,
    }
    .apply(&mut project)
    .unwrap();
    // A track called Subtitles without text clips does not count.
    assert!(track_cues(&project, None).is_err());
}

#[test]
fn import_rejects_absurd_offsets_and_far_future_cues_do_not_overflow() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.srt");
    std::fs::write(&path, "1\n99999:00:00,000 --> 99999:00:01,000\nA\n").unwrap();
    let mut handler = HeadlessHandler::from_bundle(
        Bundle {
            project: project_with_track(),
            assets: BTreeMap::new(),
        },
        None,
    );
    for offset in [i64::MAX, i64::MIN] {
        assert!(
            handler
                .subtitles_import(SubtitleImportParams {
                    path: path.clone(),
                    offset: Time(offset)
                })
                .is_err()
        );
    }
    // The largest accepted timestamp plus a big offset must not panic.
    let _ = handler.subtitles_import(SubtitleImportParams {
        path,
        offset: Time(50_000 * 3600 * TICKS_PER_SECOND),
    });
}
