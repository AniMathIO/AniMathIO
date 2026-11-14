//! Chapter sidecars from timeline markers.
use animathio_control::{
    ChapterFormat, ChaptersExportParams, ControlHandler, HeadlessHandler, chapters,
    export_chapters, format_chapters,
};
use animathio_core::{
    Clip, ClipId, ClipKind, Command, Marker, MarkerId, Project, TICKS_PER_SECOND, Time, TimeRange,
    Track, TrackId,
};

const SEC: i64 = TICKS_PER_SECOND;

fn project(markers: &[(i64, &str)]) -> Project {
    let mut p = Project::default();
    Command::AddTrack {
        track: Track::new(TrackId(1), "V"),
        index: 0,
    }
    .apply(&mut p)
    .unwrap();
    Command::AddClip {
        track: TrackId(1),
        clip: Clip::new(
            ClipId(1),
            ClipKind::Plugin {
                plugin_id: "x".into(),
                params: serde_json::json!({}),
            },
            TimeRange::new(Time(0), Time(3725 * SEC)).unwrap(),
        ),
    }
    .apply(&mut p)
    .unwrap();
    for (i, (t, name)) in markers.iter().enumerate() {
        let mut m = Marker::new(MarkerId(i as u64 + 1), Time(*t * SEC));
        m.name = (*name).into();
        Command::AddMarker { marker: m }.apply(&mut p).unwrap();
    }
    p
}

#[test]
fn youtube_list_prepends_an_intro_merges_close_markers_and_uses_hours() {
    let p = project(&[(30, "Setup"), (35, "Too close"), (95, ""), (3700, "Outro")]);
    let list = chapters(&p, &ChaptersExportParams::default());
    let titles: Vec<_> = list.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, ["Intro", "Setup", "Chapter 3", "Outro"]);
    assert_eq!(
        list[1].end,
        Time(95 * SEC),
        "chapters run to the next start"
    );
    assert_eq!(
        list[3].end,
        Time(3725 * SEC),
        "the last runs to the timeline end"
    );
    assert_eq!(
        format_chapters(&list, ChapterFormat::Youtube),
        "0:00 Intro\n0:30 Setup\n1:35 Chapter 3\n1:01:40 Outro\n"
    );
    // No intro when disabled, or when a marker already sits at zero.
    let no_intro = chapters(
        &p,
        &ChaptersExportParams {
            intro_title: String::new(),
            ..ChaptersExportParams::default()
        },
    );
    assert_eq!(no_intro[0].title, "Setup");
    let at_zero = chapters(
        &project(&[(0, "Start"), (20, "Next")]),
        &ChaptersExportParams::default(),
    );
    assert_eq!(at_zero.len(), 2);
    assert_eq!(at_zero[0].title, "Start");
}

#[test]
fn vtt_and_ffmetadata_forms() {
    let p = project(&[(0, "Intro"), (61, "Part = 2; #x")]);
    let list = chapters(&p, &ChaptersExportParams::default());
    let vtt = format_chapters(&list, ChapterFormat::Vtt);
    assert!(vtt.starts_with("WEBVTT\n\nchapter-1\n00:00:00.000 --> 00:01:01.000\nIntro\n"));
    assert!(
        vtt.contains("chapter-2\n00:01:01.000 --> 01:02:05.000\nPart = 2; #x\n"),
        "{vtt}"
    );
    let meta = format_chapters(&list, ChapterFormat::Ffmetadata);
    assert!(meta.starts_with(";FFMETADATA1\n\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=61000\n"));
    assert!(meta.contains(r"title=Part \= 2\; \#x"), "{meta}");
}

#[test]
fn export_range_shifts_chapters_and_drops_earlier_markers() {
    let p = project(&[(10, "Before"), (100, "Kept"), (200, "Later")]);
    let list = chapters(
        &p,
        &ChaptersExportParams {
            start: Time(60 * SEC),
            end: Some(Time(150 * SEC)),
            ..ChaptersExportParams::default()
        },
    );
    let seen: Vec<_> = list
        .iter()
        .map(|c| (c.start.0 / SEC, c.end.0 / SEC, c.title.as_str()))
        .collect();
    assert_eq!(seen, [(0, 40, "Intro"), (40, 90, "Kept")]);
}

#[test]
fn export_writes_files_picks_format_from_extension_and_refuses_overwrite() {
    let p = project(&[(0, "A"), (30, "B")]);
    let dir = std::env::temp_dir().join(format!("amio-chapters-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let vtt = dir.join("chapters.vtt");
    let result = export_chapters(
        &p,
        &ChaptersExportParams {
            output: Some(vtt.clone()),
            ..ChaptersExportParams::default()
        },
    )
    .unwrap();
    assert_eq!(result.format, ChapterFormat::Vtt);
    assert_eq!(result.chapters, 2);
    assert!(result.text.is_none() && result.path.is_some());
    assert!(std::fs::read_to_string(&vtt).unwrap().starts_with("WEBVTT"));
    let again = export_chapters(
        &p,
        &ChaptersExportParams {
            output: Some(vtt),
            ..ChaptersExportParams::default()
        },
    );
    assert!(again.is_err(), "existing files need overwrite");
    let _ = std::fs::remove_dir_all(dir);
    // No markers: an error, not an empty file.
    assert!(export_chapters(&project(&[]), &ChaptersExportParams::default()).is_err());
    assert!(ChapterFormat::parse("mp4").is_err());
}

#[test]
fn control_handler_exposes_chapters_export() {
    let mut handler = HeadlessHandler::new();
    let result = handler.chapters_export(ChaptersExportParams::default());
    assert!(result.is_err(), "an empty project has no chapters");
    let call: animathio_control::Call = serde_json::from_value(serde_json::json!({
        "method": "chapters.export",
        "params": {"params": {"format": "vtt", "min_length": 0}}
    }))
    .unwrap();
    assert!(
        matches!(call, animathio_control::Call::ChaptersExport { params } if params.format == ChapterFormat::Vtt)
    );
}
