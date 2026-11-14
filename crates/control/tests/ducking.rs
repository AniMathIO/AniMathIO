//! Auto-ducking: voice detection, envelope keyframes, mixer-level effect and undo.
#![allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
use animathio_audio::{AudioPool, SymphoniaSource};
use animathio_control::{
    DuckParams, apply_duck, detect_voice_samples, duck_command, duck_envelope, merge_voice,
    propose_duck,
};
use animathio_core::{
    Asset, AssetId, Clip, ClipId, ClipKind, Command, History, Interpolation, Keyframe, ParamValue,
    Project, TICKS_PER_SECOND, Target, Time, TimeRange, Track, TrackId,
};
use std::collections::BTreeMap;

const MS: i64 = TICKS_PER_SECOND / 1000;

fn range(s: i64, e: i64) -> TimeRange {
    TimeRange::new(Time(s * MS), Time(e * MS)).unwrap()
}

/// Alternating-sign 16 kHz samples: (amplitude, milliseconds) parts.
fn pcm(parts: &[(f32, usize)]) -> Vec<f32> {
    parts
        .iter()
        .flat_map(|(amplitude, ms)| {
            (0..ms * 16).map(move |i| if i % 2 == 0 { *amplitude } else { -*amplitude })
        })
        .collect()
}

fn wav(samples: &[f32]) -> Vec<u8> {
    let data: Vec<u8> = samples
        .iter()
        .flat_map(|s| ((s * 32_000.0) as i16).to_le_bytes())
        .collect();
    let len = u32::try_from(data.len()).unwrap();
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
    wav.extend(data);
    wav
}

fn options() -> DuckParams {
    DuckParams {
        voice_tracks: vec![TrackId(1)],
        music_tracks: vec![TrackId(2)],
        attack: Time(100 * MS),
        release: Time(200 * MS),
        min_gap: Time(400 * MS),
        ..DuckParams::default()
    }
}

/// Voice track 1: speech 0-1 s and 2-3 s over a 4 s clip. Music track 2: a 4 s clip of
/// constant 0.25 starting at 0 and a second clip 4-6 s.
fn session() -> (Project, BTreeMap<AssetId, Vec<u8>>) {
    let voice = wav(&pcm(&[(0.5, 1000), (0.0, 1000), (0.5, 1000), (0.0, 1000)]));
    let music = wav(&pcm(&[(0.25, 6000)]));
    let (voice_id, music_id) = (AssetId::of(&voice), AssetId::of(&music));
    let mut project = Project::default();
    let mut commands = vec![];
    for (id, name) in [(&voice_id, "voice.wav"), (&music_id, "music.wav")] {
        commands.push(Command::AddAsset {
            id: id.clone(),
            asset: Asset {
                name: name.into(),
                media_type: "audio/wav".into(),
            },
        });
    }
    commands.push(Command::AddTrack {
        track: Track::new(TrackId(1), "Voice"),
        index: 0,
    });
    commands.push(Command::AddTrack {
        track: Track::new(TrackId(2), "Music"),
        index: 1,
    });
    commands.push(Command::AddClip {
        track: TrackId(1),
        clip: Clip::new(ClipId(1), ClipKind::Audio(voice_id.clone()), range(0, 4000)),
    });
    commands.push(Command::AddClip {
        track: TrackId(2),
        clip: Clip::new(ClipId(2), ClipKind::Audio(music_id.clone()), range(0, 4000)),
    });
    commands.push(Command::AddClip {
        track: TrackId(2),
        clip: Clip::new(
            ClipId(3),
            ClipKind::Audio(music_id.clone()),
            range(4000, 6000),
        ),
    });
    Command::Batch { commands }.apply(&mut project).unwrap();
    (
        project,
        BTreeMap::from([(voice_id, voice), (music_id, music)]),
    )
}

fn gain_keys(project: &Project, clip: ClipId) -> Vec<(i64, f64)> {
    let clip = project
        .tracks()
        .iter()
        .flat_map(|t| t.clips().values())
        .find(|c| c.id == clip)
        .unwrap();
    clip.animations
        .iter()
        .find(|a| matches!(&a.target, Target::Param(n) if n == "gain"))
        .map_or_default(|a| {
            a.keys
                .keys()
                .iter()
                .map(|(t, k)| (t.0 / MS, k.value.as_number().unwrap()))
                .collect()
        })
}

/// RMS of the music alone over `[from, to)` milliseconds, through the real mixer.
fn music_rms(project: &Project, assets: &BTreeMap<AssetId, Vec<u8>>, from: i64, to: i64) -> f64 {
    let mut solo = project.clone();
    Command::RemoveTrack { id: TrackId(1) }
        .apply(&mut solo)
        .unwrap();
    let mut pool = AudioPool::default();
    for (id, bytes) in assets {
        pool.insert(
            id.clone(),
            &SymphoniaSource::from_bytes(bytes.clone()).unwrap(),
        )
        .unwrap();
    }
    let samples = pool
        .prepare(16_000, 1)
        .unwrap()
        .mix(&solo, range(from, to))
        .unwrap();
    (samples.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / samples.len() as f64).sqrt()
}

fn db(x: f64) -> f64 {
    20.0 * x.log10()
}

#[test]
fn voice_activity_uses_the_silence_detector_with_hysteresis() {
    let params = DuckParams::default();
    let samples = pcm(&[(0.0, 300), (0.5, 1000), (0.0, 700), (0.5, 500)]);
    assert_eq!(
        detect_voice_samples(&samples, Time(0), &params).unwrap(),
        vec![range(300, 1300), range(2000, 2500)]
    );
    // Level between the entry threshold (-40 dB) and entry + hysteresis (-37 dB) neither
    // starts nor ends voice: 0.0125 is -38 dB.
    let hovering = pcm(&[(0.5, 500), (0.0125, 400), (0.5, 500)]);
    assert_eq!(
        detect_voice_samples(&hovering, Time(0), &params).unwrap(),
        vec![range(0, 1400)]
    );
    assert!(detect_voice_samples(&[f32::NAN], Time(0), &params).is_err());
}

#[test]
fn short_gaps_are_bridged_and_ramps_never_overlap() {
    let params = options(); // min_gap 400 ms, attack + release 300 ms
    let voice = vec![range(0, 1000), range(1300, 2000), range(2500, 3000)];
    assert_eq!(
        merge_voice(voice, &params),
        vec![range(0, 2000), range(2500, 3000)]
    );
    // A gap above min_gap but below attack + release still merges.
    let mut wide = options();
    wide.min_gap = Time(0);
    wide.attack = Time(300 * MS);
    wide.release = Time(300 * MS);
    assert_eq!(
        merge_voice(vec![range(0, 1000), range(1500, 2000)], &wide),
        vec![range(0, 2000)]
    );
    let envelope = duck_envelope(&[range(1000, 2000)], &params);
    let floor = 10_f64.powf(-12.0 / 20.0);
    assert_eq!(
        envelope,
        vec![
            (900 * MS, 1.0),
            (1000 * MS, floor),
            (2000 * MS, floor),
            (2200 * MS, 1.0)
        ]
    );
}

#[test]
fn proposal_writes_attack_hold_release_keyframes_through_the_mixer() {
    let (mut project, assets) = session();
    let original = project.clone();
    let proposal = propose_duck(&project, &assets, &options()).unwrap();
    assert_eq!(project, original, "analysis must not edit the project");
    assert_eq!(proposal.voice, vec![range(0, 1000), range(2000, 3000)]);
    assert_eq!(proposal.ducked, vec![range(0, 1200), range(1900, 3200)]);
    // Clip 2 (0-4 s) is touched; clip 3 (4-6 s) is not.
    assert_eq!(proposal.clips.len(), 1);
    assert_eq!(proposal.clips[0].clip, ClipId(2));
    let mut history = History::default();
    let result = apply_duck(&mut project, &mut history, &proposal).unwrap();
    assert_eq!(result.clips, 1);
    let floor = 10_f64.powf(-12.0 / 20.0);
    let keys = gain_keys(&project, ClipId(2));
    let times: Vec<i64> = keys.iter().map(|k| k.0).collect();
    assert_eq!(times, vec![0, 1000, 1200, 1900, 2000, 3000, 3200, 4000]);
    // The voice starts at 0, so the dip is already at its floor when the clip starts.
    assert!((keys[0].1 - floor).abs() < 1e-9);
    assert!((keys[2].1 - 1.0).abs() < 1e-9);
    // Through the real mixer: -12 dB under speech, full level between phrases.
    let ducked = music_rms(&project, &assets, 200, 800);
    let open = music_rms(&project, &assets, 1400, 1700);
    assert!(
        (db(ducked / open) + 12.0).abs() < 0.3,
        "{}",
        db(ducked / open)
    );
    // Halfway down the attack ramp (linear gain) sits between the two.
    let mid = music_rms(&project, &assets, 1950, 1951);
    assert!(mid < open && mid > ducked);
    // The untouched second clip is unchanged.
    assert_eq!(gain_keys(&project, ClipId(3)).len(), 0);
    // One undo step restores the exact project; redo replays it.
    let edited = project.clone();
    assert!(history.undo(&mut project).unwrap());
    assert_eq!(project, original);
    assert!(!history.undo(&mut project).unwrap());
    assert!(history.redo(&mut project).unwrap());
    assert_eq!(project, edited);
}

#[test]
fn existing_gain_keyframes_need_replace_and_undo_restores_them() {
    let (mut project, assets) = session();
    Command::SetParam {
        id: ClipId(2),
        name: "gain".into(),
        value: Some(1.0),
    }
    .apply(&mut project)
    .unwrap();
    Command::AddKeyframe {
        id: ClipId(2),
        target: Target::Param("gain".into()),
        time: Time(500 * MS),
        keyframe: Keyframe {
            value: ParamValue::Number(0.5),
            interpolation: Interpolation::Linear,
        },
    }
    .apply(&mut project)
    .unwrap();
    let original = project.clone();
    let error = propose_duck(&project, &assets, &options())
        .unwrap_err()
        .to_string();
    assert!(error.contains("replace_existing"), "{error}");
    let params = DuckParams {
        replace_existing: true,
        ..options()
    };
    let proposal = propose_duck(&project, &assets, &params).unwrap();
    let mut history = History::default();
    apply_duck(&mut project, &mut history, &proposal).unwrap();
    assert!(
        gain_keys(&project, ClipId(2))
            .iter()
            .all(|(t, _)| *t != 500)
    );
    assert!(history.undo(&mut project).unwrap());
    assert_eq!(project, original);
}

#[test]
fn base_gain_is_kept_and_a_clip_starting_inside_a_dip_is_clamped() {
    let (mut project, assets) = session();
    Command::SetParam {
        id: ClipId(2),
        name: "gain".into(),
        value: Some(0.5),
    }
    .apply(&mut project)
    .unwrap();
    let proposal = propose_duck(&project, &assets, &options()).unwrap();
    apply_duck(&mut project, &mut History::default(), &proposal).unwrap();
    let floor = 10_f64.powf(-12.0 / 20.0);
    let keys = gain_keys(&project, ClipId(2));
    assert!((keys[0].1 - 0.5 * floor).abs() < 1e-9);
    assert!((keys[2].1 - 0.5).abs() < 1e-9);
    // Restricting the voice analysis to 0-1 s leaves the second phrase (2-3 s) alone.
    let (mut project, assets) = session();
    let params = DuckParams {
        range: Some(range(0, 1000)),
        ..options()
    };
    let proposal = propose_duck(&project, &assets, &params).unwrap();
    assert_eq!(proposal.voice, vec![range(0, 1000)]);
    apply_duck(&mut project, &mut History::default(), &proposal).unwrap();
    assert_eq!(gain_keys(&project, ClipId(3)).len(), 0);
}

#[test]
fn invalid_selections_stale_drafts_and_parameters_are_rejected() {
    let (mut project, assets) = session();
    let proposal = propose_duck(&project, &assets, &options()).unwrap();
    let bad = |edit: &dyn Fn(&mut DuckParams)| {
        let mut params = options();
        edit(&mut params);
        propose_duck(&project, &assets, &params).is_err()
    };
    assert!(bad(&|p| p.voice_tracks.clear()));
    assert!(bad(&|p| p.music_tracks.clear()));
    assert!(bad(&|p| p.music_tracks = vec![TrackId(1)])); // voice == music
    assert!(bad(&|p| p.music_tracks = vec![TrackId(9)]));
    assert!(bad(&|p| p.depth_db = 61.0));
    assert!(bad(&|p| p.depth_db = f64::NAN));
    assert!(bad(&|p| p.attack = Time(0)));
    assert!(bad(&|p| p.threshold_db = 5.0));
    Command::SetTrackLocked {
        id: TrackId(2),
        locked: true,
    }
    .apply(&mut project)
    .unwrap();
    assert!(propose_duck(&project, &assets, &options()).is_err());
    Command::SetTrackLocked {
        id: TrackId(2),
        locked: false,
    }
    .apply(&mut project)
    .unwrap();
    assert!(duck_command(&project, &proposal).is_ok());
    Command::SetTrackGain {
        id: TrackId(2),
        gain: 0.5,
    }
    .apply(&mut project)
    .unwrap();
    assert!(
        duck_command(&project, &proposal)
            .unwrap_err()
            .to_string()
            .contains("project changed")
    );
}

#[test]
fn many_phrases_are_chunked_into_one_valid_batch() {
    // 200 phrases over 400 short music clips: more keyframes than one batch may hold.
    let mut parts = Vec::new();
    for _ in 0..200 {
        parts.push((0.5, 300));
        parts.push((0.0, 700));
    }
    let voice = wav(&pcm(&parts));
    let music = wav(&pcm(&[(0.25, 200_000)]));
    let (voice_id, music_id) = (AssetId::of(&voice), AssetId::of(&music));
    let mut project = Project::default();
    let mut commands = Vec::new();
    for id in [&voice_id, &music_id] {
        commands.push(Command::AddAsset {
            id: id.clone(),
            asset: Asset {
                name: "a.wav".into(),
                media_type: "audio/wav".into(),
            },
        });
    }
    commands.push(Command::AddTrack {
        track: Track::new(TrackId(1), "Voice"),
        index: 0,
    });
    commands.push(Command::AddTrack {
        track: Track::new(TrackId(2), "Music"),
        index: 1,
    });
    commands.push(Command::AddClip {
        track: TrackId(1),
        clip: Clip::new(
            ClipId(1),
            ClipKind::Audio(voice_id.clone()),
            range(0, 200_000),
        ),
    });
    for i in 0..400_u64 {
        let start = i64::try_from(i).unwrap() * 500;
        commands.push(Command::AddClip {
            track: TrackId(2),
            clip: Clip::new(
                ClipId(10 + i),
                ClipKind::Audio(music_id.clone()),
                range(start, start + 500),
            ),
        });
    }
    Command::Batch { commands }.apply(&mut project).unwrap();
    let assets = BTreeMap::from([(voice_id, voice), (music_id, music)]);
    let original = project.clone();
    let proposal = propose_duck(&project, &assets, &options()).unwrap();
    assert_eq!(proposal.voice.len(), 200);
    let keyframes: usize = proposal.clips.iter().map(|c| c.keys.len()).sum();
    assert!(keyframes > 1100, "{keyframes}");
    let Command::Batch { commands } = duck_command(&project, &proposal).unwrap() else {
        panic!("a duck is one batch");
    };
    assert!(commands.len() <= 1024 && commands.len() > 1);
    let mut history = History::default();
    apply_duck(&mut project, &mut history, &proposal).unwrap();
    assert!(history.undo(&mut project).unwrap());
    assert_eq!(project, original);
}
