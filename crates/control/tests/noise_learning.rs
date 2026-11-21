//! Denoise profile learning from a selected range, on synthetic noise and tone.
#![allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
use animathio_control::{learn_noise_command, noise_bands};
use animathio_core::{
    Asset, AssetId, AudioChain, Clip, ClipId, ClipKind, Command, EffectId, EffectInstance, History,
    Project, TICKS_PER_SECOND, Time, TimeRange, Track, TrackId,
};
use std::collections::BTreeMap;

const MS: i64 = TICKS_PER_SECOND / 1000;
const RATE: usize = 16_000;
const CLIP: ClipId = ClipId(2);
const EFFECT: EffectId = EffectId(88);

fn range(s: i64, e: i64) -> TimeRange {
    TimeRange::new(Time(s * MS), Time(e * MS)).unwrap()
}

/// Deterministic white noise in `-amplitude..amplitude` (xorshift, fixed seed).
fn noise(count: usize, amplitude: f32, state: &mut u32) -> Vec<f32> {
    (0..count)
        .map(|_| {
            *state ^= *state << 13;
            *state ^= *state >> 17;
            *state ^= *state << 5;
            (f64::from(*state) / f64::from(u32::MAX) * 2.0 - 1.0) as f32 * amplitude
        })
        .collect()
}

/// 0-1 s noise, 1-2 s a 1 kHz tone over the noise, 2-3 s noise again (16 kHz mono).
fn signal() -> Vec<f32> {
    let mut seed = 0x1234_5678;
    let mut out = noise(RATE, 0.05, &mut seed);
    let tone_noise = noise(RATE, 0.05, &mut seed);
    out.extend(tone_noise.iter().enumerate().map(|(i, n)| {
        n + 0.4 * (std::f64::consts::TAU * 1000.0 * i as f64 / RATE as f64).sin() as f32
    }));
    out.extend(noise(RATE, 0.05, &mut seed));
    out
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
    wav.extend((RATE as u32).to_le_bytes());
    wav.extend((RATE as u32 * 2).to_le_bytes());
    wav.extend(2_u16.to_le_bytes());
    wav.extend(16_u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(len.to_le_bytes());
    wav.extend(data);
    wav
}

fn session(locked: bool) -> (Project, BTreeMap<AssetId, Vec<u8>>) {
    let bytes = wav(&signal());
    let id = AssetId::of(&bytes);
    let mut project = Project::default();
    Command::Batch {
        commands: vec![
            Command::AddAsset {
                id: id.clone(),
                asset: Asset {
                    name: "room.wav".into(),
                    media_type: "audio/wav".into(),
                },
            },
            Command::AddTrack {
                track: Track::new(TrackId(1), "Dialogue"),
                index: 0,
            },
            Command::AddClip {
                track: TrackId(1),
                clip: Clip::new(CLIP, ClipKind::Audio(id.clone()), range(0, 3000)),
            },
            Command::AddAudioEffect {
                chain: AudioChain::Clip(CLIP),
                index: 0,
                effect: EffectInstance {
                    id: EFFECT,
                    plugin: "org.animathio.audio-basic".into(),
                    effect: "denoise".into(),
                    params: BTreeMap::new(),
                    bypass: false,
                },
            },
        ],
    }
    .apply(&mut project)
    .unwrap();
    if locked {
        Command::SetTrackLocked {
            id: TrackId(1),
            locked: true,
        }
        .apply(&mut project)
        .unwrap();
    }
    (project, BTreeMap::from([(id, bytes)]))
}

fn learned(project: &Project, assets: &BTreeMap<AssetId, Vec<u8>>, r: TimeRange) -> [f64; 16] {
    let command = learn_noise_command(project, assets, CLIP, EFFECT, r).unwrap();
    let mut project = project.clone();
    command.apply(&mut project).unwrap();
    let params = &project.tracks()[0].clips()[&CLIP].audio_effects[0].params;
    assert_eq!(params["noise_profile"], 1.0);
    std::array::from_fn(|i| params[&format!("noise_{i}")])
}

#[test]
fn white_noise_profile_is_flat_and_repeatable_across_ranges() {
    let (project, assets) = session(false);
    let first = learned(&project, &assets, range(0, 1000));
    let last = learned(&project, &assets, range(2000, 3000));
    // White noise: every 500 Hz band holds about the same power density, and two
    // different noise-only ranges describe the same room.
    let (low, high) = first
        .iter()
        .fold((f64::MAX, 0.0_f64), |(l, h), b| (l.min(*b), h.max(*b)));
    assert!(low > 0.0 && high / low < 4.0, "flat profile: {first:?}");
    for (a, b) in first.iter().zip(&last) {
        assert!((a / b - 1.0).abs() < 0.5, "{first:?} vs {last:?}");
    }
    assert_eq!(first, learned(&project, &assets, range(0, 1000)));
}

#[test]
fn a_tone_in_the_selection_shows_up_in_its_band_only() {
    let (project, assets) = session(false);
    let noise_only = learned(&project, &assets, range(0, 1000));
    let with_tone = learned(&project, &assets, range(1000, 2000));
    // 1 kHz lies in band 2 (1000-1500 Hz): its density is far above the noise floor,
    // the other bands stay at the noise level.
    assert!(with_tone[2] > noise_only[2] * 50.0, "{with_tone:?}");
    for band in [0, 5, 9, 15] {
        assert!(
            (with_tone[band] / noise_only[band] - 1.0).abs() < 0.5,
            "band {band}: {with_tone:?} vs {noise_only:?}"
        );
    }
}

#[test]
fn learning_replaces_the_previous_profile_and_undoes_as_one_step() {
    let (mut project, assets) = session(false);
    let mut history = History::default();
    let original = project.clone();
    let tone = learn_noise_command(&project, &assets, CLIP, EFFECT, range(1000, 2000)).unwrap();
    history.execute(&mut project, &tone).unwrap();
    let after_tone = project.clone();
    let noise = learn_noise_command(&project, &assets, CLIP, EFFECT, range(0, 1000)).unwrap();
    history.execute(&mut project, &noise).unwrap();
    assert_ne!(project, after_tone);
    history.undo(&mut project).unwrap();
    assert_eq!(project, after_tone);
    history.undo(&mut project).unwrap();
    assert_eq!(project, original);
}

#[test]
fn invalid_selections_and_targets_are_rejected_without_mutation() {
    let (project, assets) = session(false);
    let learn = |clip, effect, r| learn_noise_command(&project, &assets, clip, effect, r);
    // Under 512 analysis samples (32 ms at 16 kHz).
    assert!(learn(CLIP, EFFECT, range(0, 20)).is_err());
    // Past the clip end, and unknown clip / effect.
    assert!(learn(CLIP, EFFECT, range(2900, 3100)).is_err());
    assert!(learn(ClipId(99), EFFECT, range(0, 500)).is_err());
    assert!(learn(CLIP, EffectId(1), range(0, 500)).is_err());
    let (locked, assets) = session(true);
    assert!(learn_noise_command(&locked, &assets, CLIP, EFFECT, range(0, 500)).is_err());
}

#[test]
fn band_estimator_separates_noise_from_a_tone() {
    let mut seed = 99;
    let tone: Vec<f32> = (0..8000)
        .map(|i| (std::f64::consts::TAU * 3000.0 * f64::from(i) / RATE as f64).sin() as f32 * 0.3)
        .collect();
    let bands = noise_bands(&tone).unwrap();
    // 3 kHz is band 6; everything above band 8 stays near zero.
    let peak = bands.iter().cloned().fold(0.0_f64, f64::max);
    assert_eq!(bands[6], peak);
    assert!(bands[12] < peak * 1e-3);
    let flat = noise_bands(&noise(8000, 0.1, &mut seed)).unwrap();
    assert!(flat.iter().all(|b| *b > 0.0));
}
