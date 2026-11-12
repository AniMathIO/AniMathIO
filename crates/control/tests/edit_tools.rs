//! High-level edit helpers: silence trimming on a generated WAV fixture, timeline
//! description, title presets and export presets.
use animathio_control::{
    Call, ControlHandler, ExportPreset, HeadlessHandler, TitleStyle, TrimSilenceParams,
    describe_timeline, title_command,
};
use animathio_core::{
    Asset, AssetId, Clip, ClipId, ClipKind, Command, Project, TICKS_PER_SECOND, Time, TimeRange,
    Track, TrackId, builtin::Builtin,
};
use animathio_format::Bundle;
use std::collections::BTreeMap;

const MS: i64 = TICKS_PER_SECOND / 1000;

/// 48 kHz mono 16-bit WAV: one second of 440 Hz tone, one second of silence, one
/// second of tone.
fn tone_silence_tone() -> Vec<u8> {
    let rate = 48_000_u32;
    let mut samples = Vec::new();
    for second in 0..3 {
        for n in 0..rate {
            let value = if second == 1 {
                0.0
            } else {
                0.5 * (2.0 * std::f64::consts::PI * 440.0 * f64::from(n) / f64::from(rate)).sin()
            };
            #[allow(clippy::cast_possible_truncation)]
            samples.push((value * f64::from(i16::MAX)) as i16);
        }
    }
    let data_len = u32::try_from(samples.len() * 2).unwrap();
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1_u16.to_le_bytes()); // mono
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 2).to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

fn text(id: u64, start: i64, end: i64) -> Clip {
    Clip::new(
        ClipId(id),
        ClipKind::Builtin(Builtin::Text {
            text: "Hello".into(),
            font_size: 0.1,
            color: [255; 4],
        }),
        TimeRange::new(Time(start), Time(end)).unwrap(),
    )
}

/// Track 1: the WAV as an audio clip over [0, 3 s). Track 2: a text clip over [0, 3 s).
fn recording() -> HeadlessHandler {
    let wav = tone_silence_tone();
    let id = AssetId::of(&wav);
    let mut project = Project::default();
    Command::Batch {
        commands: vec![
            Command::AddAsset {
                id: id.clone(),
                asset: Asset {
                    name: "voice.wav".into(),
                    media_type: "audio/wav".into(),
                },
            },
            Command::AddTrack {
                track: Track::new(TrackId(1), "Voice"),
                index: 0,
            },
            Command::AddTrack {
                track: Track::new(TrackId(2), "Text"),
                index: 1,
            },
            Command::AddClip {
                track: TrackId(1),
                clip: Clip::new(
                    ClipId(3),
                    ClipKind::Audio(id.clone()),
                    TimeRange::new(Time(0), Time(3000 * MS)).unwrap(),
                ),
            },
            Command::AddClip {
                track: TrackId(2),
                clip: text(4, 0, 3000 * MS),
            },
        ],
    }
    .apply(&mut project)
    .unwrap();
    HeadlessHandler::from_bundle(
        Bundle {
            project,
            assets: BTreeMap::from([(id, wav)]),
        },
        None,
    )
}

fn spans(project: &Project, track: usize) -> Vec<(i64, i64, i64)> {
    let mut spans: Vec<_> = project.tracks()[track]
        .clips()
        .values()
        .map(|c| (c.range.start.0, c.range.end.0, c.source_offset.0))
        .collect();
    spans.sort_unstable();
    spans
}

fn near(actual: i64, expected_ms: i64) -> bool {
    (actual - expected_ms * MS).abs() <= 30 * MS
}

#[test]
fn trim_silence_removes_the_pause_with_ripple_in_one_undo_step() {
    let mut handler = recording();
    let original = handler.project().clone();
    let result = handler.trim_silence(TrimSilenceParams::default()).unwrap();

    // One second of silence minus 100 ms padding on each side.
    assert_eq!(result.removed.len(), 1, "{result:?}");
    let removed = result.removed[0];
    assert!(near(removed.start.0, 1100), "{removed:?}");
    assert!(near(removed.end.0, 1900), "{removed:?}");
    let (start, end) = (removed.start.0, removed.end.0);
    let length = end - start;

    // Both tracks are cut and rippled identically.
    for track in 0..2 {
        assert_eq!(
            spans(handler.project(), track),
            vec![(0, start, 0), (start, 3000 * MS - length, end)],
        );
    }

    // Detection on the result finds no more long pauses.
    let again = handler.trim_silence(TrimSilenceParams::default()).unwrap();
    assert!(again.removed.is_empty(), "{again:?}");

    assert!(handler.history_undo().unwrap());
    assert_eq!(*handler.project(), original);
}

#[test]
fn trim_silence_respects_min_silence_and_track_selection() {
    let mut handler = recording();
    let original = handler.project().clone();
    let result = handler
        .trim_silence(TrimSilenceParams {
            min_silence: Time(1500 * MS),
            ..TrimSilenceParams::default()
        })
        .unwrap();
    assert!(result.removed.is_empty());
    assert_eq!(*handler.project(), original);

    // The text track has no audio.
    let error = handler
        .handle(Call::TrimSilence {
            params: TrimSilenceParams {
                track: Some(TrackId(2)),
                ..TrimSilenceParams::default()
            },
        })
        .unwrap_err();
    assert!(error.to_string().contains("no audio"), "{error}");
    assert_eq!(*handler.project(), original);
}

#[test]
fn describe_lists_tracks_clips_and_gaps_in_timecode() {
    let mut project = recording().project().clone();
    Command::TrimClip {
        id: ClipId(4),
        range: TimeRange::new(Time(500 * MS), Time(2000 * MS)).unwrap(),
        source_offset: Time(0),
    }
    .apply(&mut project)
    .unwrap();
    let text = describe_timeline(&project);
    assert!(text.contains("duration 00:00:03.000"), "{text}");
    assert!(
        text.contains("clip 3 00:00:00.000 - 00:00:03.000 audio \"voice.wav\""),
        "{text}"
    );
    assert!(text.contains("gap   00:00:00.000 - 00:00:00.500"), "{text}");
    assert!(
        text.contains("clip 4 00:00:00.500 - 00:00:02.000 text \"Hello\""),
        "{text}"
    );
    assert!(
        text.contains("gap   00:00:02.000 - 00:00:03.000 (end)"),
        "{text}"
    );
}

#[test]
fn captions_share_a_track_and_presets_set_resolution() {
    let mut project = Project::default();
    for (start, end) in [(0, 1000), (1000, 2000)] {
        let range = TimeRange::new(Time(start * MS), Time(end * MS)).unwrap();
        let (command, _, _) =
            title_command(&project, "caption", range, TitleStyle::Caption).unwrap();
        command.apply(&mut project).unwrap();
    }
    assert_eq!(project.tracks().len(), 1);
    assert_eq!(project.tracks()[0].clips().len(), 2);
    let range = TimeRange::new(Time(0), Time(1000 * MS)).unwrap();
    let (command, _, _) = title_command(&project, "Intro", range, TitleStyle::Title).unwrap();
    command.apply(&mut project).unwrap();
    assert_eq!(project.tracks().len(), 2);

    ExportPreset::ShortsVertical
        .command(&project)
        .unwrap()
        .apply(&mut project)
        .unwrap();
    assert_eq!((project.width(), project.height()), (1080, 1920));
    assert_eq!(project.frame_rate().numerator(), 30);
}
