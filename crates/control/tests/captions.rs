//! Caption styles: word timings on generated clips, styling at creation and restyling.
use animathio_control::{
    CaptionPosition, CaptionStyleParams, ControlHandler, HeadlessHandler, SubtitleStyleParams,
    apply_cues, caption_style_command, caption_styles, clip_caption_style, subtitle_command,
};
use animathio_core::{
    ClipKind, Command, History, ParamValue, Project, TICKS_PER_SECOND, Time, TrackId,
};
use animathio_format::Bundle;
use animathio_transcribe::{Cue, Word};
use std::collections::BTreeMap;

const MS: i64 = TICKS_PER_SECOND / 1000;

fn t(ms: i64) -> Time {
    Time(ms * MS)
}

fn word(text: &str, start: i64, end: i64) -> Word {
    Word {
        text: text.into(),
        start: t(start),
        end: t(end),
    }
}

/// A cue with recogniser word timings and one without (as an SRT import yields).
fn cues() -> Vec<Cue> {
    let mut timed = Cue::new(t(1000), t(3000), "Hello brave world");
    timed.words = vec![
        word("Hello", 1000, 1500),
        word("brave", 1700, 2200),
        word("world", 2400, 3000),
    ];
    vec![timed, Cue::new(t(4000), t(5000), "Plain cue")]
}

fn plugin_param(clip: &animathio_core::Clip, name: &str) -> serde_json::Value {
    match &clip.kind {
        ClipKind::Plugin { params, .. } => params[name].clone(),
        _ => panic!("not a plugin clip"),
    }
}

fn text_clips(project: &Project) -> Vec<&animathio_core::Clip> {
    let track = project
        .tracks()
        .iter()
        .find(|t| t.name() == "Subtitles")
        .unwrap();
    track.clips().values().collect()
}

#[test]
fn word_timings_are_stored_clip_relative() {
    let mut project = Project::default();
    let mut history = History::default();
    apply_cues(
        &mut project,
        &mut history,
        "en".into(),
        &cues(),
        &CaptionStyleParams::named("karaoke"),
    )
    .unwrap();
    let clips = text_clips(&project);
    let first = clips.iter().find(|c| c.range.start == t(1000)).unwrap();
    assert_eq!(
        plugin_param(first, "words"),
        format!(
            "0:{} {}:{} {}:{}",
            500 * MS,
            700 * MS,
            1200 * MS,
            1400 * MS,
            2000 * MS
        )
    );
    assert_eq!(plugin_param(first, "caption_style"), "karaoke");
    // Without recogniser timings the words are spread evenly over the cue.
    let plain = clips.iter().find(|c| c.range.start == t(4000)).unwrap();
    let words = plugin_param(plain, "words");
    let pairs: Vec<(i64, i64)> = words
        .as_str()
        .unwrap()
        .split(' ')
        .map(|p| {
            let (a, b) = p.split_once(':').unwrap();
            (a.parse().unwrap(), b.parse().unwrap())
        })
        .collect();
    assert_eq!(pairs.len(), 2);
    assert_eq!(pairs[0].0, 0);
    assert_eq!(pairs[0].1, pairs[1].0);
    assert_eq!(pairs[1].1, 1000 * MS);
    assert!(pairs[0].1 > 0 && pairs[0].1 < 1000 * MS);
}

#[test]
fn classic_keeps_the_bar_and_other_styles_have_none() {
    let project = Project::default();
    let (command, added) =
        subtitle_command(&project, &cues(), &CaptionStyleParams::default()).unwrap();
    let mut classic = project.clone();
    command.apply(&mut classic).unwrap();
    assert_eq!(classic.tracks().len(), 2);
    assert!(added.background_track_id.is_some());

    let (command, added) =
        subtitle_command(&project, &cues(), &CaptionStyleParams::named("pop")).unwrap();
    let mut pop = project.clone();
    command.apply(&mut pop).unwrap();
    assert_eq!(pop.tracks().len(), 1);
    assert_eq!(pop.tracks()[0].name(), "Subtitles");
    assert!(added.background_track_id.is_none());
    assert_eq!(pop.tracks()[0].clips().len(), 2);
    // Every text clip keeps its fade keyframes and a safe-area box.
    for clip in pop.tracks()[0].clips().values() {
        assert_ne!(clip.animations.len(), 0);
        assert!(clip.transform.y + clip.transform.scale_y / 2.0 <= 0.921);
    }
}

#[test]
fn every_style_builds_and_unknown_styles_fail_early() {
    for style in caption_styles() {
        let (command, _) = subtitle_command(
            &Project::default(),
            &cues(),
            &CaptionStyleParams::named(style.id),
        )
        .unwrap();
        let mut project = Project::default();
        command.apply(&mut project).unwrap();
    }
    let err = subtitle_command(
        &Project::default(),
        &cues(),
        &CaptionStyleParams::named("neon"),
    )
    .unwrap_err();
    assert!(err.to_string().contains("unknown caption style"), "{err}");
    let mut big = CaptionStyleParams::named("pop");
    big.font_size = Some(5.0);
    assert!(subtitle_command(&Project::default(), &cues(), &big).is_err());
}

#[test]
fn position_is_safe_area_aware() {
    let y = |position, portrait: bool| {
        let mut project = Project::default();
        if portrait {
            Command::SetSettings {
                width: 1080,
                height: 1920,
                frame_rate: project.frame_rate(),
                background: project.background(),
                duration: Time(0),
            }
            .apply(&mut project)
            .unwrap();
        }
        let mut style = CaptionStyleParams::named("pop");
        style.position = Some(position);
        let (command, _) = subtitle_command(&project, &cues(), &style).unwrap();
        command.apply(&mut project).unwrap();
        let clip = project.tracks()[0].clips().values().next().unwrap().clone();
        (clip.transform.y, clip.transform.scale_y)
    };
    let (top, h) = y(CaptionPosition::Top, false);
    assert!((top - h / 2.0 - 0.08).abs() < 1e-9);
    let (bottom, h) = y(CaptionPosition::Bottom, false);
    assert!((bottom + h / 2.0 - 0.92).abs() < 1e-9);
    let (middle, _) = y(CaptionPosition::Middle, false);
    assert!((middle - 0.5).abs() < 1e-9);
    // Portrait keeps clear of the platform UI at the bottom.
    let (bottom, h) = y(CaptionPosition::Bottom, true);
    assert!((bottom + h / 2.0 - 0.84).abs() < 1e-9);
    // Bold outline centres itself on 9:16 by default.
    let mut portrait = Project::default();
    Command::SetSettings {
        width: 1080,
        height: 1920,
        frame_rate: portrait.frame_rate(),
        background: portrait.background(),
        duration: Time(0),
    }
    .apply(&mut portrait)
    .unwrap();
    let (command, _) = subtitle_command(
        &portrait,
        &cues(),
        &CaptionStyleParams::named("bold-outline"),
    )
    .unwrap();
    command.apply(&mut portrait).unwrap();
    let clip = portrait.tracks()[0].clips().values().next().unwrap();
    assert!((clip.transform.y - 0.5).abs() < 1e-9);
}

#[test]
fn restyle_is_one_undo_step_and_manages_bars() {
    let mut project = Project::default();
    let mut history = History::default();
    apply_cues(
        &mut project,
        &mut history,
        "en".into(),
        &cues(),
        &CaptionStyleParams::default(),
    )
    .unwrap();
    assert_eq!(project.tracks().len(), 2);
    let classic = project.clone();

    let mut handler = HeadlessHandler::from_bundle(
        Bundle {
            project: project.clone(),
            assets: BTreeMap::new(),
        },
        None,
    );
    let mut style = CaptionStyleParams::named("word-highlight");
    style.accent = Some([0, 229, 255, 255]);
    style.max_words = Some(2);
    let result = handler
        .subtitles_style(SubtitleStyleParams { track: None, style })
        .unwrap();
    assert_eq!(
        (result.style.as_str(), result.captions),
        ("word-highlight", 2)
    );
    let restyled = handler.project_get().unwrap().project;
    // The bar track is gone, the captions carry the new look.
    assert_eq!(restyled.tracks().len(), 1);
    assert_eq!(restyled.tracks()[0].clips().len(), 2);
    for clip in restyled.tracks()[0].clips().values() {
        assert_eq!(clip_caption_style(clip).as_deref(), Some("word-highlight"));
        assert_eq!(clip.params.get("max_words"), Some(&ParamValue::Number(2.0)));
        assert_eq!(
            clip.params.get("accent"),
            Some(&ParamValue::from_srgba([0, 229, 255, 255]))
        );
    }
    // One undo restores the original captions and their bars.
    assert!(handler.history_undo().unwrap());
    assert_eq!(handler.project_get().unwrap().project, classic);
    // Switching a bar-less set back to classic re-adds the bars.
    handler
        .subtitles_style(SubtitleStyleParams {
            track: None,
            style: CaptionStyleParams::named("pop"),
        })
        .unwrap();
    handler
        .subtitles_style(SubtitleStyleParams {
            track: None,
            style: CaptionStyleParams::named("classic"),
        })
        .unwrap();
    let back = handler.project_get().unwrap().project;
    assert_eq!(back.tracks().len(), 2);
    let bars = back
        .tracks()
        .iter()
        .find(|t| t.name().contains("background"))
        .unwrap();
    assert_eq!(bars.clips().len(), 2);
    for clip in bars.clips().values() {
        assert_ne!(clip.animations.len(), 0);
        assert!(clip.link_group.is_some());
    }
}

#[test]
fn restyle_fills_missing_word_timings_and_rejects_empty_projects() {
    // Captions made by the plain title preset have no `words`; they get an even spread.
    let mut project = Project::default();
    let (command, _) = animathio_control::title_preset_command(
        &project,
        &animathio_control::TitleAddParams {
            preset: "caption".into(),
            fields: BTreeMap::from([("caption".to_owned(), "one two three".to_owned())]),
            range: animathio_core::TimeRange::new(t(0), t(3000)).unwrap(),
        },
    )
    .unwrap();
    command.apply(&mut project).unwrap();
    // Not on a `Subtitles` track, so it needs an explicit track.
    let err = caption_style_command(
        &project,
        &SubtitleStyleParams {
            track: None,
            style: CaptionStyleParams::named("pop"),
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("no caption clips"), "{err}");
    let text_track = project
        .tracks()
        .iter()
        .find(|t| t.clips().values().any(|c| clip_caption_style(c).is_some()))
        .map(animathio_core::Track::id)
        .unwrap();
    let (command, count) = caption_style_command(
        &project,
        &SubtitleStyleParams {
            track: Some(text_track),
            style: CaptionStyleParams::named("pop"),
        },
    )
    .unwrap();
    assert_eq!(count, 1);
    command.apply(&mut project).unwrap();
    let clip = project
        .tracks()
        .iter()
        .flat_map(|t| t.clips().values())
        .find(|c| clip_caption_style(c).is_some())
        .unwrap();
    let ParamValue::String(words) = clip.params.get("words").unwrap() else {
        panic!("words should be a string");
    };
    assert_eq!(words.split(' ').count(), 3);
    let _ = TrackId(0);
}
