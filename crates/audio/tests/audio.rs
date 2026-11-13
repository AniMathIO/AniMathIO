//! Contracts shared by preview and export, independent of any audio device.
#![allow(clippy::float_cmp, clippy::cast_precision_loss)]
use animathio_audio::{AudioPool, AudioSource, DecodeLimits, Error, Pcm, Player, SymphoniaSource};
use animathio_core::*;
use proptest::prelude::*;
use std::{path::Path, sync::Arc, time::Duration};

fn range(start: i64, end: i64) -> TimeRange {
    TimeRange::new(Time(start), Time(end)).unwrap()
}
fn project(pcm: &Pcm, start: i64, end: i64) -> (Project, AudioPool) {
    let id = AssetId::of(b"tone");
    let mut project = Project::default();
    Command::AddAsset {
        id: id.clone(),
        asset: Asset {
            name: "tone".into(),
            media_type: "audio/wav".into(),
        },
    }
    .apply(&mut project)
    .unwrap();
    Command::AddTrack {
        track: Track::new(TrackId(0), "audio"),
        index: 0,
    }
    .apply(&mut project)
    .unwrap();
    Command::AddClip {
        track: TrackId(0),
        clip: Clip::new(ClipId(0), ClipKind::Audio(id.clone()), range(start, end)),
    }
    .apply(&mut project)
    .unwrap();
    let mut pool = AudioPool::default();
    pool.insert(id, pcm).unwrap();
    (project, pool)
}
fn sine(rate: u32) -> Pcm {
    Pcm::new(
        rate,
        1,
        (0..rate)
            .map(|i| (i as f32 * std::f32::consts::TAU * 1000.0 / rate as f32).sin() * 0.1)
            .collect(),
    )
    .unwrap()
}
#[test]
fn sine_at_offset_lands_at_exact_sample_index_without_drift() {
    for rate in [44_100, 48_000] {
        let tick = TICKS_PER_SECOND / i64::from(rate);
        let pcm = sine(rate);
        // Include a long offset to catch accidental float timeline conversion.
        let start = 10_000_000 * TICKS_PER_SECOND + 17 * tick;
        let (project, pool) = project(&pcm, start, start + TICKS_PER_SECOND);
        let output = pool
            .mix(
                &project,
                range(start - 17 * tick, start + 100 * tick),
                rate,
                1,
            )
            .unwrap();
        assert!(output[..17].iter().all(|s| *s == 0.0));
        assert_eq!(&output[17..], &pcm.samples()[..100]);
    }
}
#[test]
fn sub_sample_source_offset_uses_ceil_source_frame() {
    let rate = 48_000;
    let tick = TICKS_PER_SECOND / i64::from(rate);
    let pcm = Pcm::new(rate, 1, (0..10).map(|i| i as f32 / 10.0).collect()).unwrap();
    let (mut project, pool) = project(&pcm, 0, 9 * tick);
    Command::TrimClip {
        id: ClipId(0),
        range: range(0, 9 * tick),
        source_offset: Time(1),
    }
    .apply(&mut project)
    .unwrap();
    assert_eq!(
        pool.mix(&project, range(0, 9 * tick), rate, 1).unwrap(),
        (1..10).map(|i| i as f32 / 10.0).collect::<Vec<_>>()
    );
}
#[test]
fn overlapping_tracks_sum_trim_gain_and_mute_are_applied() {
    let rate = 48_000;
    let tick = TICKS_PER_SECOND / i64::from(rate);
    let pcm = Pcm::new(rate, 1, vec![0.1, 0.2, 0.3, 0.4]).unwrap();
    let (mut project, pool) = project(&pcm, tick, 4 * tick);
    Command::TrimClip {
        id: ClipId(0),
        range: range(tick, 4 * tick),
        source_offset: Time(tick),
    }
    .apply(&mut project)
    .unwrap();
    Command::SetParam {
        id: ClipId(0),
        name: "gain".into(),
        value: Some(0.5),
    }
    .apply(&mut project)
    .unwrap();
    Command::SetTrackGain {
        id: TrackId(0),
        gain: 0.25,
    }
    .apply(&mut project)
    .unwrap();
    let mixer = pool.prepare(rate, 2).unwrap();
    let single = mixer.mix(&project, range(0, 4 * tick)).unwrap();
    assert_eq!(
        single,
        vec![0., 0., 0.025, 0.025, 0.0375, 0.0375, 0.05, 0.05]
    );
    Command::AddTrack {
        track: Track::new(TrackId(1), "second"),
        index: 1,
    }
    .apply(&mut project)
    .unwrap();
    let mut clip = project.tracks()[0].clips()[&ClipId(0)].clone();
    clip.id = ClipId(1);
    Command::AddClip {
        track: TrackId(1),
        clip,
    }
    .apply(&mut project)
    .unwrap();
    let overlap = mixer.mix(&project, range(0, 4 * tick)).unwrap();
    for (sum, one) in overlap.iter().zip(&single) {
        assert_eq!(*sum, *one * 5.0);
    }
    Command::SetParam {
        id: ClipId(1),
        name: "muted".into(),
        value: Some(1.0),
    }
    .apply(&mut project)
    .unwrap();
    assert_eq!(mixer.mix(&project, range(0, 4 * tick)).unwrap(), single);
}
#[test]
fn track_mute_and_solo_gate_the_mix() {
    let tick = TICKS_PER_SECOND / 48_000;
    let pcm = Pcm::new(48_000, 1, vec![0.1, 0.2, 0.3, 0.4]).unwrap();
    let (mut project, pool) = project(&pcm, tick, 4 * tick);
    let mixer = pool.prepare(48_000, 1).unwrap();
    let audible = mixer.mix(&project, range(0, 4 * tick)).unwrap();
    assert!(audible.iter().any(|sample| *sample != 0.0));

    // Muting the only track silences the mix.
    Command::SetTrackMuted {
        id: TrackId(0),
        muted: true,
    }
    .apply(&mut project)
    .unwrap();
    assert!(
        mixer
            .mix(&project, range(0, 4 * tick))
            .unwrap()
            .iter()
            .all(|sample| *sample == 0.0)
    );

    // A second, unsoloed track is silenced while the first stays soloed.
    Command::SetTrackMuted {
        id: TrackId(0),
        muted: false,
    }
    .apply(&mut project)
    .unwrap();
    Command::SetTrackSolo {
        id: TrackId(0),
        solo: true,
    }
    .apply(&mut project)
    .unwrap();
    Command::AddTrack {
        track: Track::new(TrackId(1), "second"),
        index: 1,
    }
    .apply(&mut project)
    .unwrap();
    let mut second = project.tracks()[0].clips()[&ClipId(0)].clone();
    second.id = ClipId(1);
    Command::AddClip {
        track: TrackId(1),
        clip: second,
    }
    .apply(&mut project)
    .unwrap();
    let soloed = mixer.mix(&project, range(0, 4 * tick)).unwrap();
    for (sample, expected) in soloed.iter().zip(&audible) {
        assert_eq!(*sample, *expected);
    }
}
#[test]
fn gain_and_mute_keyframes_use_clip_local_time() {
    let tick = TICKS_PER_SECOND / 48_000;
    let pcm = Pcm::new(48_000, 1, vec![1.; 10]).unwrap();
    let (mut project, pool) = project(&pcm, 2 * tick, 8 * tick);
    for (name, values) in [("gain", [0.0, 0.5]), ("muted", [0.0, 1.0])] {
        Command::SetParam {
            id: ClipId(0),
            name: name.into(),
            value: Some(1.0),
        }
        .apply(&mut project)
        .unwrap();
        for (time, value) in [0, 4 * tick].into_iter().zip(values) {
            Command::AddKeyframe {
                id: ClipId(0),
                target: Target::Param(name.into()),
                time: Time(time),
                keyframe: Keyframe {
                    value: value.into(),
                    interpolation: if name == "gain" {
                        Interpolation::Linear
                    } else {
                        Interpolation::Hold
                    },
                },
            }
            .apply(&mut project)
            .unwrap();
        }
    }
    assert_eq!(
        pool.mix(&project, range(0, 8 * tick), 48_000, 1).unwrap(),
        vec![0., 0., 0., 0.125, 0.25, 0.375, 0., 0.]
    );
}
#[test]
fn pool_gc_and_missing_assets_are_explicit() {
    let (mut project, mut pool) = project(&sine(48_000), 0, TICKS_PER_SECOND);
    let empty = AudioPool::default().prepare(48_000, 1).unwrap();
    assert!(matches!(
        empty.mix(&project, range(0, 100_000)),
        Err(Error::MissingAsset(_))
    ));
    Command::RemoveClip { id: ClipId(0) }
        .apply(&mut project)
        .unwrap();
    pool.collect(&project);
    assert!(pool.is_empty());
}
#[test]
fn symphonia_decodes_supported_fixtures_and_reports_opus() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for extension in ["wav", "mp3", "flac", "aac", "m4a", "ogg"] {
        let source = SymphoniaSource::open(root.join(format!("tone.{extension}"))).unwrap();
        let pcm = source.decode_all().unwrap();
        assert_eq!(pcm.rate(), 48_000, "{extension}");
        assert_eq!(pcm.channels(), 1, "{extension}");
        assert!(pcm.frames() >= 3000, "{extension}");
        assert!(pcm.samples().iter().any(|s| s.abs() > 0.01), "{extension}");
        let slice = source.decode(range(14700, 14700 * 11)).unwrap();
        assert_eq!(slice.samples(), &pcm.samples()[1..11]);
    }
    assert!(matches!(
        SymphoniaSource::open(root.join("tone.opus")),
        Err(Error::Unsupported(_))
    ));
}
#[test]
fn resampling_compensates_delay_and_preserves_duration_and_channel_mapping() {
    let mut samples = vec![0.; 4410];
    samples[441] = 1.0;
    let (project, pool) = project(
        &Pcm::new(44_100, 1, samples).unwrap(),
        0,
        TICKS_PER_SECOND / 10,
    );
    let result = pool
        .mix(&project, range(0, TICKS_PER_SECOND / 10), 48_000, 2)
        .unwrap();
    assert_eq!(result.len(), 9600);
    let peak = result
        .as_chunks::<2>()
        .0
        .iter()
        .enumerate()
        .max_by(|a, b| a.1[0].abs().total_cmp(&b.1[0].abs()))
        .unwrap()
        .0;
    assert_eq!(peak, 480);
    assert!(result.as_chunks::<2>().0.iter().all(|f| f[0] == f[1]));
}
#[test]
fn pcm_ranges_pad_eof_and_validate_configuration() {
    let pcm = Pcm::new(48_000, 2, vec![0.5, -0.5]).unwrap();
    assert_eq!(
        pcm.decode(range(0, 14700 * 3)).unwrap().samples(),
        &[0.5, -0.5, 0., 0., 0., 0.]
    );
    assert!(Pcm::new(0, 1, vec![]).is_err());
    assert!(Pcm::new(48_000, 2, vec![0.]).is_err());
    assert!(Pcm::new(48_000, 1, vec![f32::NAN]).is_err());
    assert!(AudioPool::default().prepare(0, 2).is_err());
    assert!(AudioPool::default().prepare(48_000, 0).is_err());
}
fn pcm_wav(rate: u32, channels: u16, samples: &[u16]) -> Vec<u8> {
    assert_ne!(samples.len(), 0);
    assert!(samples.len().is_multiple_of(usize::from(channels)));
    let bits = 16u16;
    let block_align = channels * (bits / 8);
    let data_len = u32::try_from(samples.len() * 2).unwrap();
    let byte_rate = rate.saturating_mul(u32::from(block_align));
    let mut wav = Vec::with_capacity(44 + samples.len() * 2);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36u32 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&bits.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}
#[test]
fn absurd_sample_rate_and_channel_count_are_rejected_without_large_allocations() {
    let path = std::env::temp_dir().join(format!(
        "animathio-absurd-rate-{}.wav",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&path, pcm_wav(4_000_000_000, 1, &[0; 8])).unwrap();
    let result = SymphoniaSource::open(&path);
    let _ = std::fs::remove_file(&path);
    assert!(result.is_err(), "absurd sample rate must be rejected");
    assert!(Pcm::new(4_000_000_000, 1, vec![0.0]).is_err());
    assert!(Pcm::new(48_000, 33, vec![0.0; 33]).is_err());
    assert!(AudioPool::default().prepare(768_001, 1).is_err());
    assert!(AudioPool::default().prepare(48_000, 33).is_err());
}
#[test]
fn decode_limits_reject_oversized_files_cleanly() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tone.wav");
    let tiny = DecodeLimits {
        max_samples: 100,
        max_duration: Duration::from_secs(1),
    };
    assert!(matches!(
        SymphoniaSource::open_with_limits(&path, tiny),
        Err(Error::Invalid(_))
    ));
    let no_time = DecodeLimits {
        max_samples: usize::MAX,
        max_duration: Duration::ZERO,
    };
    assert!(matches!(
        SymphoniaSource::open_with_limits(&path, no_time),
        Err(Error::Invalid(_))
    ));
}
#[test]
fn extreme_rate_ratios_resample_with_bounded_chunks() {
    // Downsampling: an odd near-maximum rate falls back to the bounded sinc path.
    let pcm = Pcm::new(767_999, 1, vec![0.0; 1024]).unwrap();
    let (proj, pool) = project(&pcm, 0, 1024 * TICKS_PER_SECOND / 767_999);
    let out = pool
        .mix(
            &proj,
            range(0, 1024 * TICKS_PER_SECOND / 767_999),
            48_000,
            1,
        )
        .unwrap();
    assert_ne!(out.len(), 0);
    // Upsampling: a 1 Hz source falls back to the fixed-output sinc path.
    let pcm = Pcm::new(1, 1, vec![0.0, 1.0, 0.0]).unwrap();
    let (proj, pool) = project(&pcm, 0, 3 * TICKS_PER_SECOND);
    let out = pool
        .mix(&proj, range(0, 3 * TICKS_PER_SECOND), 48_000, 1)
        .unwrap();
    assert_eq!(out.len(), 3 * 48_000);
}
proptest! {
    #[test]
    fn adjacent_ranges_are_identical_to_one_mix(a in 0_i64..300_000_000, width in 1_i64..100_000_000, tail in 1_i64..100_000_000, output_rate in prop_oneof![Just(44_100_u32), Just(48_000_u32), Just(47_999_u32)]) {
        let pcm = sine(44_100);
        let (mut project,pool) = project(&pcm, 17_321, TICKS_PER_SECOND);
        Command::SetParam { id:ClipId(0), name:"gain".into(),value:Some(0.5) }.apply(&mut project).unwrap();
        Command::AddKeyframe { id:ClipId(0), target:Target::Param("gain".into()),time:Time(0),keyframe:Keyframe {value:(0.0).into(),interpolation:Interpolation::Linear} }.apply(&mut project).unwrap();
        Command::AddKeyframe { id:ClipId(0), target:Target::Param("gain".into()),time:Time(TICKS_PER_SECOND),keyframe:Keyframe {value:(1.0).into(),interpolation:Interpolation::Linear} }.apply(&mut project).unwrap();
        let mixer = pool.prepare(output_rate,2).unwrap();
        let b = a+width;
        let c = b+tail;
        let mut parts = mixer.mix(&project,range(a,b)).unwrap();
        parts.extend(mixer.mix(&project,range(b,c)).unwrap());
        prop_assert_eq!(parts,mixer.mix(&project,range(a,c)).unwrap());
    }
}
#[test]
#[ignore = "requires an audio device; emits a quiet 50 ms tone"]
fn real_device_play_pause_and_seek() {
    let (rate, channels) = Player::default_output_format().unwrap();
    let (mut project, pool) = project(&sine(rate), 0, TICKS_PER_SECOND);
    Command::SetParam {
        id: ClipId(0),
        name: "gain".into(),
        value: Some(0.1),
    }
    .apply(&mut project)
    .unwrap();
    let mut player = Player::new(
        Arc::new(project),
        Arc::new(pool.prepare(rate, channels).unwrap()),
    )
    .unwrap();
    player.play().unwrap();
    std::thread::sleep(Duration::from_millis(25));
    player.seek(Time(TICKS_PER_SECOND / 2)).unwrap();
    assert!(player.now() >= Time(TICKS_PER_SECOND / 2));
    std::thread::sleep(Duration::from_millis(25));
    player.pause().unwrap();
    let frozen = player.now();
    std::thread::sleep(Duration::from_millis(5));
    assert_eq!(player.now(), frozen);
    player.seek(Time(0)).unwrap();
    assert_eq!(player.now(), Time(0));
}

/// Two DC clips `[0,1s)` and `[1s,2s)` sharing one asset, with a 0.4 s transition.
fn crossfade_mix(kind: TransitionKind) -> Vec<f32> {
    let rate = 1000;
    let pcm = Pcm::new(rate, 1, vec![1.0; 2000]).unwrap();
    let second = TICKS_PER_SECOND;
    let (mut project, pool) = project(&pcm, 0, second);
    let asset = AssetId::of(b"tone");
    let mut incoming = Clip::new(ClipId(1), ClipKind::Audio(asset), range(second, 2 * second));
    incoming.source_offset = Time(second / 2); // handle for the early half
    let commands = [
        Command::AddClip {
            track: TrackId(0),
            clip: incoming,
        },
        Command::AddTransition {
            track: TrackId(0),
            transition: Transition {
                id: TransitionId(0),
                kind,
                duration: Time(second * 2 / 5),
                from: ClipId(0),
                to: ClipId(1),
            },
        },
    ];
    for command in commands {
        command.apply(&mut project).unwrap();
    }
    pool.mix(&project, range(0, 2 * second), rate, 1).unwrap()
}
#[test]
fn cross_dissolve_crossfades_audio_with_equal_power_gains() {
    let mix = crossfade_mix(TransitionKind::CrossDissolve);
    assert_eq!(mix.len(), 2000);
    // Outside the [0.8 s, 1.2 s) window each clip plays alone at unity.
    assert_eq!(mix[799], 1.0);
    assert_eq!(mix[1200], 1.0);
    // Inside, both clips play: cos(p·π/2) + sin(p·π/2).
    for (frame, progress) in [(800, 0.0), (900, 0.25), (1000, 0.5), (1100, 0.75)] {
        let (out, inc) = equal_power(progress);
        assert!(
            (f64::from(mix[frame]) - (out + inc)).abs() < 1e-6,
            "frame {frame}: {} vs {}",
            mix[frame],
            out + inc
        );
        assert!((out * out + inc * inc - 1.0).abs() < 1e-12);
    }
}
#[test]
fn non_dissolve_transitions_keep_a_hard_audio_cut() {
    let mix = crossfade_mix(TransitionKind::DipToBlack);
    assert!(mix.iter().all(|&s| s == 1.0));
}

#[test]
fn unsupported_surround_rebuild_keeps_previous_mixer_usable() {
    let mut pool = AudioPool::default();
    let good = pool.prepare(48_000, 2).unwrap();
    let id = AssetId::of(b"surround");
    pool.insert(id, &Pcm::new(48_000, 6, vec![0.0; 60]).unwrap())
        .unwrap();
    let error = pool
        .prepare(48_000, 2)
        .err()
        .expect("surround must be rejected");
    assert!(error.to_string().contains("6-channel audio to 2-channel"));
    let range = TimeRange::new(Time(0), Time(100)).unwrap();
    assert!(good.mix(&Project::default(), range).is_ok());
}

#[test]
fn mp4_companion_decodes_audio_instead_of_default_video_track() {
    let source = SymphoniaSource::from_bytes(
        include_bytes!("../../media/tests/fixtures/sample.mp4").to_vec(),
    )
    .unwrap();
    assert!(source.pcm().frames() > 0);
}
