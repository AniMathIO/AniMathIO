//! Deterministic automatic-cut analysis and transactional application.
use animathio_control::{
    AutocutParams, AutocutReason, SilenceMetric, apply_autocuts, detect_filler_words,
    detect_silent_samples, propose_autocuts,
};
use animathio_core::{
    Asset, AssetId, Clip, ClipId, ClipKind, Command, History, Project, TICKS_PER_SECOND, Time,
    TimeRange, Track, TrackId,
};
use animathio_transcribe::Word;
use std::collections::BTreeMap;
const MS: i64 = TICKS_PER_SECOND / 1000;
fn range(s: i64, e: i64) -> TimeRange {
    TimeRange::new(Time(s * MS), Time(e * MS)).unwrap()
}
fn options() -> AutocutParams {
    AutocutParams {
        padding_before: Time(0),
        padding_after: Time(0),
        ..AutocutParams::default()
    }
}
fn pcm(parts: &[(f32, usize)]) -> Vec<f32> {
    parts
        .iter()
        .flat_map(|(amplitude, ms)| {
            (0..ms * 16).map(move |i| if i % 2 == 0 { *amplitude } else { -*amplitude })
        })
        .collect()
}
#[test]
fn silence_hysteresis_padding_and_partial_windows_are_exact() {
    let samples = pcm(&[(0.5, 1000), (0., 1000), (0.5, 1000)]);
    let mut p = options();
    assert_eq!(
        detect_silent_samples(&samples, Time(0), &p).unwrap(),
        vec![range(1000, 2000)]
    );
    p.padding_after = Time(100 * MS);
    p.padding_before = Time(200 * MS);
    assert_eq!(
        detect_silent_samples(&samples, Time(100 * MS), &p).unwrap(),
        vec![range(1200, 1900)]
    );
    p.padding_before = Time(900 * MS);
    assert!(
        detect_silent_samples(&samples, Time(0), &p)
            .unwrap()
            .is_empty()
    );
    p = options();
    let near_threshold = pcm(&[(0.5, 100), (0., 200), (0.011, 100), (0., 200), (0.5, 100)]);
    assert_eq!(
        detect_silent_samples(&near_threshold, Time(0), &p).unwrap(),
        vec![range(100, 600)]
    );
    p.hysteresis_db = 0.;
    assert!(
        detect_silent_samples(&near_threshold, Time(0), &p)
            .unwrap()
            .is_empty()
    );
    p.keep_short_pauses = false;
    assert_eq!(
        detect_silent_samples(&near_threshold, Time(0), &p).unwrap(),
        vec![range(100, 300), range(400, 600)]
    );
    let partial = vec![0.; 8011];
    assert_eq!(
        detect_silent_samples(&partial, Time(0), &options()).unwrap()[0].end,
        Time(8011 * (TICKS_PER_SECOND / 16_000))
    );
}
#[test]
fn peak_preserves_transients_that_rms_treats_as_silence() {
    let mut samples = vec![0.; 16_000];
    for window in samples.chunks_mut(160) {
        window[0] = 0.1;
    }
    let mut p = options();
    assert_eq!(
        detect_silent_samples(&samples, Time(0), &p).unwrap(),
        vec![range(0, 1000)]
    );
    p.metric = SilenceMetric::Peak;
    assert!(
        detect_silent_samples(&samples, Time(0), &p)
            .unwrap()
            .is_empty()
    );
    p.silence_db = f64::NAN;
    assert!(detect_silent_samples(&samples, Time(0), &p).is_err());
    assert!(detect_silent_samples(&[f32::NAN], Time(0), &options()).is_err());
}
fn words(parts: &[&str]) -> Vec<Word> {
    parts
        .iter()
        .enumerate()
        .map(|(i, text)| {
            let i = i64::try_from(i).unwrap();
            Word {
                text: (*text).into(),
                start: Time(i * 300 * MS),
                end: Time((i * 300 + 200) * MS),
            }
        })
        .collect()
}
#[test]
fn fillers_support_phrases_hungarian_repeats_false_starts_and_overrides() {
    let cuts = detect_filler_words(
        &words(&["Um,", "hello", "you", "know", "a", "a", "prob-", "probably"]),
        "en",
        &[],
    );
    assert_eq!(cuts.len(), 4);
    assert_eq!(cuts[0].reason, AutocutReason::Filler);
    assert_eq!(cuts[1].range, range(600, 1100));
    assert_eq!(cuts[2].reason, AutocutReason::Repeat);
    assert_eq!(cuts[3].reason, AutocutReason::Repeat);
    assert!(cuts.iter().all(|c| c.confidence > 0. && c.confidence < 1.));
    assert_eq!(
        detect_filler_words(&words(&["ÖÖ,", "Hát", "jó"]), "hu", &[]).len(),
        2
    );
    assert!(detect_filler_words(&words(&["um"]), "en", &["well".into()]).is_empty());
    let cuts = detect_filler_words(&words(&["you", "know", "know"]), "en", &[]);
    assert_eq!(cuts.len(), 1); // overlapping filler/repeat intervals merged
    assert_eq!(cuts[0].range, range(0, 500));
    let mut distant = words(&["you", "know"]);
    distant[1].start = Time(2000 * MS);
    distant[1].end = Time(2200 * MS);
    assert!(detect_filler_words(&distant, "en", &[]).is_empty());
}

fn recording() -> (Project, BTreeMap<AssetId, Vec<u8>>) {
    let samples: Vec<i16> = (0..48_000)
        .map(|i| {
            if (16_000..32_000).contains(&i) {
                0
            } else if i % 2 == 0 {
                16_000
            } else {
                -16_000
            }
        })
        .collect();
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
    let id = AssetId::of(&wav);
    let mut project = Project::default();
    Command::AddAsset {
        id: id.clone(),
        asset: Asset {
            name: "recording.wav".into(),
            media_type: "audio/wav".into(),
        },
    }
    .apply(&mut project)
    .unwrap();
    for i in 1..=3 {
        Command::AddTrack {
            track: Track::new(TrackId(i), "recording"),
            index: usize::try_from(i - 1).unwrap(),
        }
        .apply(&mut project)
        .unwrap();
        let mut clip = Clip::new(
            ClipId(i),
            if i == 2 {
                ClipKind::Video(id.clone())
            } else {
                ClipKind::Audio(id.clone())
            },
            range(0, 3000),
        );
        if i < 3 {
            clip.link_group = Some(ClipId(1));
        }
        Command::AddClip {
            track: TrackId(i),
            clip,
        }
        .apply(&mut project)
        .unwrap();
    }
    (project, BTreeMap::from([(id, wav)]))
}
#[test]
fn linked_video_selection_review_apply_undo_redo_and_stale_guard() {
    let (mut project, assets) = recording();
    let original = project.clone();
    let p = AutocutParams {
        clip: Some(ClipId(2)),
        ..options()
    };
    let mut proposal = propose_autocuts(&project, &assets, &p).unwrap();
    assert_eq!(project, original);
    assert_eq!(proposal.tracks, vec![TrackId(1), TrackId(2)]);
    assert_eq!(proposal.cuts.len(), 1);
    assert_eq!(proposal.cuts[0].range, range(1000, 2000));
    let mut history = History::default();
    proposal.cuts[0].enabled = false;
    assert!(
        apply_autocuts(&mut project, &mut history, &proposal)
            .unwrap()
            .removed
            .is_empty()
    );
    assert!(!history.undo(&mut project).unwrap());
    proposal.cuts[0].enabled = true;
    apply_autocuts(&mut project, &mut history, &proposal).unwrap();
    let edited = project.clone();
    for t in &project.tracks()[..2] {
        assert_eq!(t.clips().len(), 2);
    }
    let a: Vec<_> = project.tracks()[0]
        .clips()
        .values()
        .map(|c| (c.range, c.source_offset, c.link_group))
        .collect();
    let b: Vec<_> = project.tracks()[1]
        .clips()
        .values()
        .map(|c| (c.range, c.source_offset, c.link_group))
        .collect();
    assert_eq!(a, b);
    assert_eq!(project.tracks()[2], original.tracks()[2]);
    assert!(
        apply_autocuts(&mut project, &mut history, &proposal)
            .unwrap_err()
            .to_string()
            .contains("project changed")
    );
    assert!(history.undo(&mut project).unwrap());
    assert_eq!(project, original);
    assert!(history.redo(&mut project).unwrap());
    assert_eq!(project, edited);
}
#[test]
fn all_unlocked_respects_locks_but_rejects_locked_companions() {
    let (mut project, assets) = recording();
    Command::SetTrackLocked {
        id: TrackId(3),
        locked: true,
    }
    .apply(&mut project)
    .unwrap();
    let p = AutocutParams {
        clip: Some(ClipId(2)),
        all_unlocked_tracks: true,
        ..options()
    };
    let proposal = propose_autocuts(&project, &assets, &p).unwrap();
    let locked = project.tracks()[2].clone();
    apply_autocuts(&mut project, &mut History::default(), &proposal).unwrap();
    assert_eq!(project.tracks()[2], locked);
    let (mut project, assets) = recording();
    Command::SetTrackLocked {
        id: TrackId(1),
        locked: true,
    }
    .apply(&mut project)
    .unwrap();
    let original = project.clone();
    assert!(propose_autocuts(&project, &assets, &p).is_err());
    assert_eq!(project, original);
}
#[test]
fn range_selection_clamps_analysis_and_bad_apply_is_atomic() {
    let (mut project, assets) = recording();
    let p = AutocutParams {
        track: Some(TrackId(1)),
        range: Some(range(1200, 1800)),
        ..options()
    };
    let mut proposal = propose_autocuts(&project, &assets, &p).unwrap();
    assert_eq!(proposal.cuts[0].range, range(1200, 1800));
    proposal.cuts.push(proposal.cuts[0].clone());
    let original = project.clone();
    assert!(apply_autocuts(&mut project, &mut History::default(), &proposal).is_err());
    assert_eq!(project, original);
    proposal.cuts.pop();
    proposal.tracks = vec![TrackId(1)];
    assert!(apply_autocuts(&mut project, &mut History::default(), &proposal).is_err());
    assert_eq!(project, original);
}
#[test]
fn no_engine_reports_capability_before_loading_audio() {
    if animathio_transcribe::engine_available() {
        return;
    }
    let error = propose_autocuts(
        &Project::default(),
        &BTreeMap::new(),
        &AutocutParams {
            fillers: true,
            ..AutocutParams::default()
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("no speech recognition engine"));
}

#[test]
fn fractional_sample_end_is_clamped_and_cancelled_workers_do_not_edit() {
    let (project, assets) = recording();
    let end = Time(1500 * MS + 1);
    let params = AutocutParams {
        track: Some(TrackId(1)),
        range: Some(TimeRange::new(Time(1200 * MS), end).unwrap()),
        min_silence: Time(1),
        ..options()
    };
    let proposal = propose_autocuts(&project, &assets, &params).unwrap();
    assert_eq!(proposal.cuts[0].range.end, end);
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let error =
        animathio_control::propose_autocuts_cancellable(&project, &assets, &params, &cancel)
            .unwrap_err();
    assert_eq!(error.to_string(), "cancelled");
}

#[test]
fn reverse_order_multi_cut_batch_shrinks_duration_and_undo_restores_it() {
    let (mut project, assets) = recording();
    Command::SetSettings {
        width: project.width(),
        height: project.height(),
        frame_rate: project.frame_rate(),
        background: project.background(),
        duration: Time(3000 * MS),
    }
    .apply(&mut project)
    .unwrap();
    let original = project.clone();
    let mut proposal = propose_autocuts(
        &project,
        &assets,
        &AutocutParams {
            all_unlocked_tracks: true,
            ..options()
        },
    )
    .unwrap();
    let mut second = proposal.cuts[0].clone();
    second.range = range(1400, 1600);
    proposal.cuts[0].range = range(1000, 1200);
    proposal.cuts.push(second);
    proposal.cuts.reverse(); // Caller order cannot change the pre-edit coordinates.
    let mut history = History::default();
    apply_autocuts(&mut project, &mut history, &proposal).unwrap();
    assert_eq!(project.duration(), Time(2600 * MS));
    for track in project.tracks() {
        assert_eq!(
            track.clips().values().map(|c| c.range.end).max(),
            Some(Time(2600 * MS))
        );
    }
    assert!(history.undo(&mut project).unwrap());
    assert_eq!(project, original);
}
