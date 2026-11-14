//! Beat analysis through the project model, markers at beats and beat/marker cut sync.
#![allow(
    clippy::assert_is_empty,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss
)]
use animathio_control::{
    BeatDetectParams, BeatMarkerOptions, BeatSelection, ControlHandler, HeadlessHandler,
    SourceLimits, SyncCutsParams, SyncMode, SyncSource, analyze_beats, apply_beat_markers,
    apply_sync_cuts,
};
use animathio_core::{
    Asset, AssetId, Clip, ClipId, ClipKind, Command, History, Project, TICKS_PER_SECOND, Time,
    TimeRange, Track, TrackId,
};
use std::collections::BTreeMap;

const SEC: i64 = TICKS_PER_SECOND;
const MS: i64 = TICKS_PER_SECOND / 1000;

/// 16 kHz mono 16-bit WAV: decaying 1 kHz bursts at 120 BPM, first click at 0.1 s.
fn click_wav(seconds: usize) -> Vec<u8> {
    let rate = 16_000_usize;
    let mut samples = vec![0_i16; seconds * rate];
    let mut at = rate / 10;
    while at + 400 < samples.len() {
        for i in 0..200 {
            let t = i as f64 / rate as f64;
            let v = 0.9 * (-t * 400.0).exp() * (2.0 * std::f64::consts::PI * 1000.0 * t).sin();
            samples[at + i] = (v * 32_000.0) as i16;
        }
        at += rate / 2;
    }
    let len = u32::try_from(samples.len() * 2).unwrap();
    let mut wav = b"RIFF".to_vec();
    wav.extend((36 + len).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16_u32.to_le_bytes());
    wav.extend(1_u16.to_le_bytes());
    wav.extend(1_u16.to_le_bytes());
    wav.extend(16_000_u32.to_le_bytes());
    wav.extend(32_000_u32.to_le_bytes());
    wav.extend(2_u16.to_le_bytes());
    wav.extend(16_u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(len.to_le_bytes());
    for s in samples {
        wav.extend(s.to_le_bytes());
    }
    wav
}

fn range(s: i64, e: i64) -> TimeRange {
    TimeRange::new(Time(s), Time(e)).unwrap()
}
fn plugin_clip(id: u64, s: i64, e: i64) -> Clip {
    Clip::new(
        ClipId(id),
        ClipKind::Plugin {
            plugin_id: "org.animathio.test@1".into(),
            params: serde_json::json!({}),
        },
        range(s, e),
    )
}

/// Track 1: music (audio clip 1, starts at 1 s, 12 s long, so beats fall on k/2 + 1.1 s).
/// Track 2: video cuts 10 `[0, 3.1)`, 11 `[3.1, 6.4)`, 12 `[6.4, 10)` seconds.
/// Track 3: linked audio companions of 10..12 as plugin clips 20..22.
fn session() -> (Project, BTreeMap<AssetId, Vec<u8>>) {
    let wav = click_wav(12);
    let id = AssetId::of(&wav);
    let mut project = Project::default();
    Command::AddAsset {
        id: id.clone(),
        asset: Asset {
            name: "clicks.wav".into(),
            media_type: "audio/wav".into(),
        },
    }
    .apply(&mut project)
    .unwrap();
    for t in 1..=3 {
        Command::AddTrack {
            track: Track::new(TrackId(t), format!("t{t}")),
            index: usize::try_from(t - 1).unwrap(),
        }
        .apply(&mut project)
        .unwrap();
    }
    Command::AddClip {
        track: TrackId(1),
        clip: Clip::new(ClipId(1), ClipKind::Audio(id.clone()), range(SEC, 13 * SEC)),
    }
    .apply(&mut project)
    .unwrap();
    let cuts = [(10, 0, 31), (11, 31, 64), (12, 64, 100)];
    for (i, (id, s, e)) in cuts.into_iter().enumerate() {
        let mut video = plugin_clip(id, s * SEC / 10, e * SEC / 10);
        let mut audio = plugin_clip(id + 10, s * SEC / 10, e * SEC / 10);
        let group = ClipId(100 + i as u64);
        video.link_group = Some(group);
        audio.link_group = Some(group);
        video.source_offset = Time(s * SEC / 10);
        audio.source_offset = Time(s * SEC / 10);
        for (track, clip) in [(2, video), (3, audio)] {
            Command::AddClip {
                track: TrackId(track),
                clip,
            }
            .apply(&mut project)
            .unwrap();
        }
    }
    (project, BTreeMap::from([(id, wav)]))
}
fn music() -> BeatDetectParams {
    BeatDetectParams {
        clip: Some(ClipId(1)),
        ..BeatDetectParams::default()
    }
}
fn clip_range(project: &Project, id: u64) -> TimeRange {
    project
        .tracks()
        .iter()
        .find_map(|t| t.clips().get(&ClipId(id)))
        .unwrap()
        .range
}

#[test]
fn analysis_reports_tempo_and_timeline_beats() {
    let (project, assets) = session();
    let before = project.clone();
    let detection = analyze_beats(&project, &assets, &music()).unwrap();
    assert_eq!(project, before);
    assert!((detection.bpm - 120.0).abs() < 0.5, "{}", detection.bpm);
    assert!(detection.beats.len() >= 20, "{}", detection.beats.len());
    for beat in &detection.beats {
        // Clicks sit at 1.1 s + k/2 s on the timeline (the clip starts at 1 s).
        let offset = (beat.time.0 - (SEC + SEC / 10)).rem_euclid(SEC / 2);
        let error = offset.min(SEC / 2 - offset);
        assert!(
            error < 4 * MS,
            "beat {} off by {} ms",
            beat.time.0,
            error / MS
        );
    }
    assert!(detection.downbeats.windows(2).all(|w| w[1] - w[0] == 4));
    // Whole-project mix gives the same tempo.
    let all = analyze_beats(&project, &assets, &BeatDetectParams::default()).unwrap();
    assert!((all.bpm - 120.0).abs() < 0.5);
}

#[test]
fn markers_at_beats_every_n_downbeats_idempotent_and_undoable() {
    let (mut project, assets) = session();
    let detection = analyze_beats(&project, &assets, &music()).unwrap();
    let mut history = History::default();
    let original = project.clone();

    let every = BeatMarkerOptions::default();
    let result = apply_beat_markers(&mut project, &mut history, &detection, &every).unwrap();
    assert_eq!(result.added, detection.beats.len());
    assert_eq!(project.markers().len(), detection.beats.len());
    assert_eq!(project.markers()[0].name, "Beat 1");
    assert!(
        apply_beat_markers(&mut project, &mut history, &detection, &every).is_err(),
        "repeating adds nothing"
    );
    assert!(history.undo(&mut project).unwrap());
    assert_eq!(project, original, "one undo step");

    let every_second = BeatMarkerOptions {
        selection: BeatSelection {
            every: 2,
            downbeats_only: false,
        },
        ..BeatMarkerOptions::default()
    };
    let added = apply_beat_markers(&mut project, &mut history, &detection, &every_second)
        .unwrap()
        .added;
    assert_eq!(added, detection.beats.len().div_ceil(2));

    let (mut project, _) = session();
    let bars = BeatMarkerOptions {
        selection: BeatSelection {
            every: 1,
            downbeats_only: true,
        },
        ..BeatMarkerOptions::default()
    };
    let added = apply_beat_markers(&mut project, &mut History::default(), &detection, &bars)
        .unwrap()
        .added;
    assert_eq!(added, detection.downbeats.len());
    assert_eq!(project.markers()[0].name, "Bar 1");
    assert!(
        apply_beat_markers(
            &mut project,
            &mut History::default(),
            &detection,
            &BeatMarkerOptions {
                selection: BeatSelection {
                    every: 0,
                    downbeats_only: false
                },
                ..BeatMarkerOptions::default()
            }
        )
        .is_err()
    );
}

#[test]
fn snapping_moves_cuts_to_beats_keeps_linked_audio_and_is_one_undo_step() {
    let (mut project, assets) = session();
    let detection = analyze_beats(&project, &assets, &music()).unwrap();
    let original = project.clone();
    let mut history = History::default();
    // Cuts at 3.1 s and 6.4 s; the nearest beats are 3.1 s (already there) and 6.6 s
    // (1.1 + k/2), so only the second cut moves; the clip heads at 0 s are 1.1 s from a beat.
    let params = SyncCutsParams {
        clips: vec![ClipId(10), ClipId(11)],
        max_shift: Time(250 * MS),
        ..SyncCutsParams::default()
    };
    let result = apply_sync_cuts(
        &mut project,
        &mut history,
        Some(&detection),
        &params,
        &SourceLimits::new(),
    )
    .unwrap();
    assert_eq!(result.shifted.len(), 1, "{result:?}");
    let tolerance = SEC / 30; // frame alignment
    let moved = clip_range(&project, 11);
    assert!(
        (moved.end.0 - (6 * SEC + 6 * SEC / 10)).abs() <= tolerance,
        "{moved:?}"
    );
    // Adjacent clips stay adjacent; linked audio moved identically; source stays in place.
    assert_eq!(clip_range(&project, 12).start, moved.end);
    assert_eq!(clip_range(&project, 22), clip_range(&project, 12));
    assert_eq!(clip_range(&project, 21), clip_range(&project, 11));
    let after_right = project
        .tracks()
        .iter()
        .find_map(|t| t.clips().get(&ClipId(12)))
        .unwrap();
    assert_eq!(
        after_right.source_offset.0,
        64 * SEC / 10 + (after_right.range.start.0 - 64 * SEC / 10),
        "right clip's content stays put"
    );
    // Rolling a cut is one undo step.
    assert!(history.undo(&mut project).unwrap());
    assert_eq!(project, original);

    // A tolerance too small for any cut is an error, not an empty edit.
    let none = SyncCutsParams {
        clips: vec![ClipId(11)],
        max_shift: Time(MS),
        source: SyncSource::Beats,
        ..SyncCutsParams::default()
    };
    // Clip 11 starts exactly on a beat already; its end is 200 ms off.
    assert!(
        apply_sync_cuts(
            &mut project,
            &mut history,
            Some(&detection),
            &none,
            &SourceLimits::new()
        )
        .is_err()
    );
    assert_eq!(project, original);
}

#[test]
fn snapping_to_markers_needs_no_audio_and_skips_invalid_edits() {
    let (mut project, _) = session();
    let mut history = History::default();
    for (id, t) in [(1, 3 * SEC), (2, 6 * SEC + 6 * SEC / 10)] {
        history
            .execute(
                &mut project,
                &Command::AddMarker {
                    marker: animathio_core::Marker::new(animathio_core::MarkerId(id), Time(t)),
                },
            )
            .unwrap();
    }
    let params = SyncCutsParams {
        clips: vec![ClipId(11)],
        source: SyncSource::Markers,
        ..SyncCutsParams::default()
    };
    let result = apply_sync_cuts(
        &mut project,
        &mut history,
        None,
        &params,
        &SourceLimits::new(),
    )
    .unwrap();
    assert_eq!(result.shifted.len(), 2);
    assert_eq!(
        clip_range(&project, 11),
        range(3 * SEC, 6 * SEC + 6 * SEC / 10)
    );
    // Neighbours rolled with it, so nothing overlaps or gaps.
    assert_eq!(clip_range(&project, 10).end, Time(3 * SEC));
    assert_eq!(clip_range(&project, 12).start, Time(6 * SEC + 6 * SEC / 10));
    project.validate().unwrap();

    // Without beats a beat-source sync cannot run.
    assert!(
        apply_sync_cuts(
            &mut project,
            &mut history,
            None,
            &SyncCutsParams {
                clips: vec![ClipId(11)],
                ..SyncCutsParams::default()
            },
            &SourceLimits::new()
        )
        .is_err()
    );
    // Locked tracks are reported as skipped, not applied.
    Command::AddMarker {
        marker: animathio_core::Marker::new(animathio_core::MarkerId(9), Time(9 * SEC)),
    }
    .apply(&mut project)
    .unwrap();
    Command::SetTrackLocked {
        id: TrackId(2),
        locked: true,
    }
    .apply(&mut project)
    .unwrap();
    let err = apply_sync_cuts(
        &mut project,
        &mut history,
        None,
        &SyncCutsParams {
            clips: vec![ClipId(12)],
            source: SyncSource::Markers,
            max_shift: Time(5 * SEC),
            ..SyncCutsParams::default()
        },
        &SourceLimits::new(),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("locked"), "{err}");
}

#[test]
fn splitting_long_clips_at_every_other_beat_keeps_linked_audio_in_sync() {
    let (mut project, assets) = session();
    let detection = analyze_beats(&project, &assets, &music()).unwrap();
    let original = project.clone();
    let mut history = History::default();
    let params = SyncCutsParams {
        clips: vec![ClipId(12)], // video [6.4, 10)
        mode: SyncMode::Split,
        selection: BeatSelection {
            every: 2,
            downbeats_only: false,
        },
        ..SyncCutsParams::default()
    };
    let result = apply_sync_cuts(
        &mut project,
        &mut history,
        Some(&detection),
        &params,
        &SourceLimits::new(),
    )
    .unwrap();
    assert!(result.splits.len() >= 2, "{result:?}");
    for track in [2, 3] {
        let clips = project.tracks()[track - 1].clips();
        let mut edits: Vec<_> = clips
            .values()
            .filter(|c| c.range.start >= Time(64 * SEC / 10))
            .map(|c| c.range)
            .collect();
        edits.sort_by_key(|r| r.start);
        assert_eq!(edits.len(), result.splits.len() + 1, "track {track}");
        assert_eq!(edits.first().unwrap().start, Time(64 * SEC / 10));
        assert_eq!(edits.last().unwrap().end, Time(10 * SEC));
    }
    let mut video: Vec<_> = project.tracks()[1]
        .clips()
        .values()
        .map(|c| c.range)
        .collect();
    let mut audio: Vec<_> = project.tracks()[2]
        .clips()
        .values()
        .map(|c| c.range)
        .collect();
    video.sort_by_key(|r| r.start);
    audio.sort_by_key(|r| r.start);
    assert_eq!(video, audio);
    // Split pieces are spaced two beats (1 s) apart, give or take a frame of alignment.
    let starts: Vec<i64> = result.splits.iter().map(|t| t.0).collect();
    for pair in starts.windows(2) {
        assert!((pair[1] - pair[0] - SEC).abs() <= SEC / 15, "{pair:?}");
    }
    assert!(history.undo(&mut project).unwrap());
    assert_eq!(project, original);
}

#[test]
fn control_calls_cover_markers_beats_and_sync() {
    let (project, assets) = session();
    let mut handler =
        HeadlessHandler::from_bundle(animathio_format::Bundle { project, assets }, None);
    assert!(handler.markers_list().unwrap().is_empty());
    let added = handler
        .markers_add(animathio_control::MarkerAddParams {
            time: Time(2 * SEC),
            name: "Verse".into(),
            color: animathio_core::MarkerColor::Green,
            ..animathio_control::MarkerAddParams::default()
        })
        .unwrap();
    assert_eq!(handler.markers_list().unwrap()[0].name, "Verse");
    handler.markers_remove(added.id).unwrap();
    assert!(handler.markers_list().unwrap().is_empty());
    assert!(handler.markers_remove(added.id).is_err());
    assert!(handler.history_undo().unwrap());
    assert_eq!(handler.markers_list().unwrap().len(), 1);

    let detection = handler.beats_detect(music()).unwrap();
    assert!((detection.bpm - 120.0).abs() < 0.5);
    let markers = handler
        .beats_add_markers(animathio_control::BeatMarkersParams {
            detect: music(),
            ..animathio_control::BeatMarkersParams::default()
        })
        .unwrap();
    assert_eq!(markers.added, detection.beats.len());
    assert!(handler.history_undo().unwrap());
    let synced = handler
        .sync_cuts_to_beats(SyncCutsParams {
            clips: vec![ClipId(11)],
            detect: music(),
            max_shift: Time(250 * MS),
            ..SyncCutsParams::default()
        })
        .unwrap();
    assert!(!synced.shifted.is_empty());
    // The wire shape round-trips with defaults filled in.
    let params: SyncCutsParams =
        serde_json::from_value(serde_json::json!({"clips": [11], "every": 2, "mode": "split"}))
            .unwrap();
    assert_eq!(params.selection.every, 2);
    assert_eq!(params.mode, SyncMode::Split);
    assert!(params.frame_align);
}

#[test]
fn call_wire_shapes_round_trip_with_flattened_params() {
    use animathio_control::{BeatMarkersParams, Call};
    let call: Call = serde_json::from_value(serde_json::json!({
        "method": "beats.add_markers",
        "params": {"params": {"clip": 3, "every": 2, "downbeats_only": true, "color": "red"}}
    }))
    .unwrap();
    let Call::BeatsAddMarkers { params } = &call else {
        panic!("{call:?}")
    };
    assert_eq!(params.detect.clip, Some(ClipId(3)));
    assert_eq!(params.options.selection.every, 2);
    assert!(params.options.selection.downbeats_only);
    assert_eq!(params.options.color, animathio_core::MarkerColor::Red);
    let again: Call = serde_json::from_value(serde_json::to_value(&call).unwrap()).unwrap();
    assert_eq!(again, call);
    let minimal: Call = serde_json::from_value(serde_json::json!({
        "method": "beats.add_markers", "params": {"params": {}}
    }))
    .unwrap();
    assert_eq!(
        minimal,
        Call::BeatsAddMarkers {
            params: BeatMarkersParams::default()
        }
    );
    let sync: Call = serde_json::from_value(serde_json::json!({
        "method": "timeline.sync_cuts_to_beats",
        "params": {"params": {"clips": [1, 2], "source": "markers", "mode": "split", "every": 4}}
    }))
    .unwrap();
    assert!(matches!(sync, Call::SyncCutsToBeats { params } if params.selection.every == 4));
}

// --- source-media limits ---------------------------------------------------------------

/// Track 1: video clip 1 `[0, 4)` s (source 0..4) and clip 2 `[4, 6)` s reading source 3..5 s
/// of a 5 s asset. Clip 2 therefore sits exactly at the end of its media.
fn limited_session() -> (Project, SourceLimits) {
    let asset = AssetId::of(b"five second video");
    let mut project = Project::default();
    Command::AddAsset {
        id: asset.clone(),
        asset: Asset {
            name: "v.mp4".into(),
            media_type: "video/mp4".into(),
        },
    }
    .apply(&mut project)
    .unwrap();
    Command::AddTrack {
        track: Track::new(TrackId(1), "V"),
        index: 0,
    }
    .apply(&mut project)
    .unwrap();
    for (id, s, e, offset) in [(1, 0, 4, 0), (2, 4, 6, 3)] {
        let mut clip = Clip::new(
            ClipId(id),
            ClipKind::Video(asset.clone()),
            range(s * SEC, e * SEC),
        );
        clip.source_offset = Time(offset * SEC);
        Command::AddClip {
            track: TrackId(1),
            clip,
        }
        .apply(&mut project)
        .unwrap();
    }
    let mut limits = SourceLimits::new();
    limits.set(asset, Time(5 * SEC));
    (project, limits)
}
fn add_markers(project: &mut Project, times: &[i64]) {
    for (i, t) in times.iter().enumerate() {
        Command::AddMarker {
            marker: animathio_core::Marker::new(animathio_core::MarkerId(i as u64 + 1), Time(*t)),
        }
        .apply(project)
        .unwrap();
    }
}

#[test]
fn snapping_never_extends_a_clip_past_its_source_media() {
    let (mut project, limits) = limited_session();
    // 4.2 s: moving the cut later trims clip 2's head (source 3.2..5.0, fits) and lengthens
    // clip 1 (source 0..4.2, fits). 6.2 s: lengthening clip 2 would read to 5.2 s > 5 s.
    add_markers(&mut project, &[4 * SEC + SEC / 5, 6 * SEC + SEC / 5]);
    let mut history = History::default();
    let result = apply_sync_cuts(
        &mut project,
        &mut history,
        None,
        &SyncCutsParams {
            clips: vec![ClipId(1), ClipId(2)],
            source: SyncSource::Markers,
            frame_align: false,
            max_shift: Time(SEC / 4),
            ..SyncCutsParams::default()
        },
        &limits,
    )
    .unwrap();
    assert_eq!(result.shifted.len(), 1, "{result:?}");
    assert_eq!(result.skipped.len(), 1, "{result:?}");
    assert!(result.skipped[0].contains("source media"), "{result:?}");
    assert_eq!(clip_range(&project, 1).end, Time(4 * SEC + SEC / 5));
    assert_eq!(clip_range(&project, 2).start, Time(4 * SEC + SEC / 5));
    assert_eq!(
        clip_range(&project, 2).end,
        Time(6 * SEC),
        "the cut that cannot fit stays"
    );
    assert!(history.undo(&mut project).unwrap());

    // Only the impossible edit requested: an error, nothing changes.
    let before = project.clone();
    let only_bad = SyncCutsParams {
        clips: vec![ClipId(2)],
        source: SyncSource::Markers,
        frame_align: false,
        max_shift: Time(SEC / 4),
        ..SyncCutsParams::default()
    };
    // Clip 2's head is a candidate too (4.0 -> 4.2 is valid), so restrict markers to 6.2 s.
    Command::RemoveMarker {
        id: animathio_core::MarkerId(1),
    }
    .apply(&mut project)
    .unwrap();
    let before_bad = project.clone();
    let err = apply_sync_cuts(&mut project, &mut history, None, &only_bad, &limits)
        .unwrap_err()
        .to_string();
    assert!(err.contains("source media"), "{err}");
    assert_eq!(project, before_bad);
    // Without limits (unknown duration) the same edit is applied: no regression for
    // generated clips and unprobed media.
    apply_sync_cuts(
        &mut project,
        &mut history,
        None,
        &only_bad,
        &SourceLimits::new(),
    )
    .unwrap();
    assert_ne!(project, before);
}

#[test]
fn splitting_never_leaves_a_clip_reading_past_its_media() {
    let (mut project, limits) = limited_session();
    add_markers(&mut project, &[SEC, 2 * SEC, 5 * SEC]);
    let mut history = History::default();
    let result = apply_sync_cuts(
        &mut project,
        &mut history,
        None,
        &SyncCutsParams {
            clips: vec![ClipId(1), ClipId(2)],
            source: SyncSource::Markers,
            mode: SyncMode::Split,
            ..SyncCutsParams::default()
        },
        &limits,
    )
    .unwrap();
    assert_eq!(result.splits.len(), 3, "{result:?}");
    // Every piece still ends at or before its media's 5 s.
    for track in project.tracks() {
        for clip in track.clips().values() {
            let end = clip.source_offset.0 + clip.range.end.0 - clip.range.start.0;
            assert!(end <= 5 * SEC, "{clip:?}");
        }
    }
    project.validate().unwrap();
}

#[test]
fn the_control_handler_rejects_trims_past_the_end_of_the_media() {
    use animathio_control::Call;
    let (project, assets) = session();
    let mut handler =
        HeadlessHandler::from_bundle(animathio_format::Bundle { project, assets }, None);
    // Clip 1: a 12 s WAV placed at [1 s, 13 s) reading source 0..12 s.
    let trim = |end: i64, offset: i64| Call::CommandApply {
        command: Command::TrimClip {
            id: ClipId(1),
            range: range(SEC, end),
            source_offset: Time(offset),
        },
    };
    let err = handler.handle(trim(15 * SEC, 0)).unwrap_err().to_string();
    assert!(err.contains("source media"), "{err}");
    // Even a single tick beyond the source end is rejected.
    assert!(handler.handle(trim(13 * SEC + 1, 0)).is_err());
    // Shifting the source window later would also read past the end.
    assert!(handler.handle(trim(13 * SEC, SEC)).is_err());
    // Shortening, or trimming within the media, is fine.
    handler.handle(trim(10 * SEC, 0)).unwrap();
    handler.handle(trim(12 * SEC, 0)).unwrap();
}
