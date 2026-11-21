//! Contracts shared by preview and export, independent of any audio device.
#![allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::chunks_exact_to_as_chunks
)]
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
fn symphonia_decodes_supported_fixtures_including_opus() {
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
    // Opus is decoded by the pure-Rust `filmcraft-opus` decoder.
    let opus = SymphoniaSource::open(root.join("tone.opus")).unwrap();
    let pcm = opus.decode_all().unwrap();
    assert_eq!((pcm.rate(), pcm.channels()), (48_000, 1));
    assert!(pcm.frames() >= 3000);
    assert!(pcm.samples().iter().any(|s| s.abs() > 0.01));
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
    // 5.1 folds down now; ten unnamed positions have no downmix rule and stay rejected.
    pool.insert(id.clone(), &Pcm::new(48_000, 6, vec![0.0; 60]).unwrap())
        .unwrap();
    assert!(pool.prepare(48_000, 2).is_ok());
    pool.insert(id, &Pcm::new(48_000, 10, vec![0.0; 100]).unwrap())
        .unwrap();
    let error = pool
        .prepare(48_000, 2)
        .err()
        .expect("unnamed layouts must be rejected");
    assert!(error.to_string().contains("10-channel"));
    let range = TimeRange::new(Time(0), Time(100)).unwrap();
    assert!(good.mix(&Project::default(), range).is_ok());
}

#[test]
fn mp4_companion_decodes_audio_instead_of_default_video_track() {
    let source = SymphoniaSource::from_bytes(
        include_bytes!("../../media/tests/fixtures/sample.mp4").to_vec(),
    )
    .unwrap();
    assert!(source.info().frames > 0);
    assert!(source.decode_all().unwrap().frames() > 0);
}

#[test]
fn clip_varispeed_length_alignment_reverse_and_block_independence() {
    for rate in [44_100, 48_000] {
        let pcm = Pcm::new(rate, 1, (0..rate).map(|i| i as f32 / rate as f32).collect()).unwrap();
        let tick = TICKS_PER_SECOND / i64::from(rate);
        for (n, d) in [(1, 2), (2, 1)] {
            for reverse in [false, true] {
                let (mut p, pool) = project(&pcm, 0, TICKS_PER_SECOND);
                Command::SetClipMaintainPitch {
                    id: ClipId(0),
                    maintain_pitch: false,
                }
                .apply(&mut p)
                .unwrap();
                Command::SetClipSpeed {
                    id: ClipId(0),
                    speed: ClipSpeed::new(n, d).unwrap(),
                    reverse,
                    ramps: vec![],
                    ripple: false,
                }
                .apply(&mut p)
                .unwrap();
                let end = p.tracks()[0].clips()[&ClipId(0)].range.end;
                let output = pool.mix(&p, range(0, end.0), rate, 1).unwrap();
                assert_eq!(output.len(), rate as usize * d as usize / n as usize);
                for (i, sample) in output.iter().enumerate() {
                    let position = i as f64 * f64::from(n) / f64::from(d);
                    let position = if reverse {
                        f64::from(rate) - 1.0 - position
                    } else {
                        position
                    };
                    // The half-speed last half-sample holds the boundary sample.
                    let expected = position.clamp(0.0, f64::from(rate) - 1.0) / f64::from(rate);
                    assert!(
                        (f64::from(*sample) - expected).abs() < 0.0001,
                        "{n}/{d}, reverse={reverse}, sample {i}"
                    );
                }
                let cut = 1234 * tick;
                let mut pieces = pool.mix(&p, range(0, cut), rate, 1).unwrap();
                pieces.extend(pool.mix(&p, range(cut, end.0), rate, 1).unwrap());
                assert_eq!(pieces, output);
                assert!(
                    pool.mix(&p, range(end.0, end.0 + tick * 10), rate, 1)
                        .unwrap()
                        .iter()
                        .all(|s| *s == 0.0)
                );
            }
        }
    }
}

#[test]
fn clip_frame_holds_are_silent() {
    let (mut p, pool) = project(&sine(48_000), 0, TICKS_PER_SECOND);
    Command::InsertFrameHold {
        id: ClipId(0),
        at: Time(TICKS_PER_SECOND / 2),
        frames: 3,
    }
    .apply(&mut p)
    .unwrap();
    let hold = p.tracks()[0]
        .clips()
        .values()
        .find(|c| c.time_map.hold)
        .unwrap();
    assert!(
        pool.mix(&p, hold.range, 48_000, 1)
            .unwrap()
            .iter()
            .all(|s| *s == 0.0)
    );
}

#[test]
fn retimed_audio_aligns_at_long_timeline_and_trimmed_source_offsets() {
    let rate = 48_000;
    let tick = TICKS_PER_SECOND / i64::from(rate);
    let pcm = Pcm::new(rate, 1, (0..200).map(|i| i as f32 / 200.0).collect()).unwrap();
    let start = 10_000_000 * TICKS_PER_SECOND + 17 * tick;
    for (n, d) in [(1, 2), (2, 1)] {
        for reverse in [false, true] {
            let (mut p, pool) = project(&pcm, start, start + 100 * tick);
            Command::TrimClip {
                id: ClipId(0),
                range: range(start, start + 100 * tick),
                source_offset: Time(50 * tick),
            }
            .apply(&mut p)
            .unwrap();
            Command::SetClipMaintainPitch {
                id: ClipId(0),
                maintain_pitch: false,
            }
            .apply(&mut p)
            .unwrap();
            Command::SetClipSpeed {
                id: ClipId(0),
                speed: ClipSpeed::new(n, d).unwrap(),
                reverse,
                ramps: vec![],
                ripple: false,
            }
            .apply(&mut p)
            .unwrap();
            let frames = 100 * d as usize / n as usize;
            let end = p.tracks()[0].clips()[&ClipId(0)].range.end.0;
            let output = pool
                .mix(&p, range(start - 3 * tick, end + 3 * tick), rate, 1)
                .unwrap();
            assert_eq!(output.len(), frames + 6);
            assert!(
                output[..3]
                    .iter()
                    .chain(&output[frames + 3..])
                    .all(|s| *s == 0.0)
            );
            for (i, sample) in output[3..frames + 3].iter().enumerate() {
                let local = i as f64 * f64::from(n) / f64::from(d);
                let position = if reverse { 149.0 - local } else { 50.0 + local };
                let expected = position.clamp(50.0, 149.0) / 200.0;
                assert!((f64::from(*sample) - expected).abs() < 0.00001);
            }
        }
    }
}

#[test]
fn maintained_pitch_sine_and_seek_order_are_stable() {
    for rate in [44_100, 48_000] {
        let tick = TICKS_PER_SECOND / i64::from(rate);
        let pcm = Pcm::new(
            rate,
            2,
            (0..rate * 2)
                .flat_map(|i| {
                    let sample =
                        (i as f32 * std::f32::consts::TAU * 440.0 / rate as f32).sin() * 0.2;
                    [sample, -sample]
                })
                .collect(),
        )
        .unwrap();
        for (n, d) in [(1, 2), (2, 1)] {
            for reverse in [false, true] {
                let (mut p, pool) = project(&pcm, 0, 2 * TICKS_PER_SECOND);
                Command::SetClipSpeed {
                    id: ClipId(0),
                    speed: ClipSpeed::new(n, d).unwrap(),
                    reverse,
                    ramps: vec![],
                    ripple: false,
                }
                .apply(&mut p)
                .unwrap();
                let end = p.tracks()[0].clips()[&ClipId(0)].range.end;
                let mixer = pool.prepare(rate, 2).unwrap();
                let output = mixer.mix(&p, range(0, end.0)).unwrap();
                assert_eq!(output.len(), (rate * 2 * d / n) as usize * 2);
                let mono: Vec<_> = output
                    .chunks_exact(2)
                    .map(|frame| {
                        assert_eq!(frame[0], -frame[1]);
                        frame[0]
                    })
                    .collect();
                // Count positive crossings across segment resets, excluding edge transients.
                let signal = &mono[rate as usize / 10..mono.len() - rate as usize / 10];
                let crossings: Vec<_> = signal
                    .windows(2)
                    .enumerate()
                    .filter(|(_, s)| s[0] <= 0.0 && s[1] > 0.0)
                    .map(|(i, _)| i)
                    .collect();
                let frequency = (crossings.len() - 1) as f64 * f64::from(rate)
                    / (crossings.last().unwrap() - crossings[0]) as f64;
                assert!(
                    (frequency - 440.0).abs() < 3.0,
                    "{rate} {n}/{d} reverse={reverse}: {frequency} Hz"
                );
                // Cold, out-of-order seeks cross effect blocks and WSOLA segment/reset fades.
                let hop = rate as usize / 100;
                for (a, b) in [
                    (2041, 4099),
                    (63 * hop, 66 * hop),
                    (rate as usize / 3, rate as usize / 3 + 911),
                    (0, 93),
                ] {
                    let cold = pool.prepare(rate, 2).unwrap();
                    let part = cold
                        .mix(&p, range(a as i64 * tick, b as i64 * tick))
                        .unwrap();
                    assert_eq!(part, output[a * 2..b * 2]);
                    assert_eq!(
                        mixer
                            .mix(&p, range(a as i64 * tick, b as i64 * tick))
                            .unwrap(),
                        part
                    );
                }
            }
        }
    }
}

#[test]
fn maintained_pitch_ramp_is_seek_independent_and_bypasses_identity() {
    let rate = 48_000;
    let tick = TICKS_PER_SECOND / i64::from(rate);
    let (mut p, pool) = project(&sine(rate), 0, TICKS_PER_SECOND);
    let original = pool.mix(&p, range(0, TICKS_PER_SECOND), rate, 1).unwrap();
    Command::SetClipMaintainPitch {
        id: ClipId(0),
        maintain_pitch: false,
    }
    .apply(&mut p)
    .unwrap();
    assert_eq!(
        pool.mix(&p, range(0, TICKS_PER_SECOND), rate, 1).unwrap(),
        original
    );
    Command::SetClipMaintainPitch {
        id: ClipId(0),
        maintain_pitch: true,
    }
    .apply(&mut p)
    .unwrap();
    Command::SetClipSpeed {
        id: ClipId(0),
        speed: ClipSpeed::default(),
        reverse: false,
        ramps: vec![
            SpeedPoint {
                time: Time(0),
                speed: ClipSpeed::new(1, 2).unwrap(),
            },
            SpeedPoint {
                time: Time(TICKS_PER_SECOND / 2),
                speed: ClipSpeed::new(2, 1).unwrap(),
            },
        ],
        ripple: false,
    }
    .apply(&mut p)
    .unwrap();
    let end = p.tracks()[0].clips()[&ClipId(0)].range.end;
    let output = pool.mix(&p, range(0, end.0), rate, 1).unwrap();
    let cut = 2137;
    let mut pieces = pool.mix(&p, range(0, cut * tick), rate, 1).unwrap();
    pieces.extend(pool.mix(&p, range(cut * tick, end.0), rate, 1).unwrap());
    assert_eq!(pieces, output);
    assert!(output.iter().all(|s| s.is_finite()));
}

#[test]
fn stretching_keeps_split_phase_and_long_timeline_offsets() {
    let rate = 48_000;
    let tick = TICKS_PER_SECOND / i64::from(rate);
    let start = 10_000_000 * TICKS_PER_SECOND + 17 * tick;
    let (mut p, pool) = project(&sine(rate), start, start + TICKS_PER_SECOND);
    Command::SetClipSpeed {
        id: ClipId(0),
        speed: ClipSpeed::new(1, 2).unwrap(),
        reverse: true,
        ramps: vec![],
        ripple: false,
    }
    .apply(&mut p)
    .unwrap();
    let end = p.tracks()[0].clips()[&ClipId(0)].range.end.0;
    let mixer = pool.prepare(rate, 1).unwrap();
    let whole = mixer.mix(&p, range(start, end)).unwrap();
    let at = Time(start + 12347 * tick + 17);
    Command::SplitClip {
        id: ClipId(0),
        right_id: ClipId(1),
        at,
    }
    .apply(&mut p)
    .unwrap();
    assert_eq!(pool.mix(&p, range(start, end), rate, 1).unwrap(), whole);
    // Gain/pitch edits invalidate the shared cache rather than reusing old output.
    Command::SetClipMaintainPitch {
        id: ClipId(1),
        maintain_pitch: false,
    }
    .apply(&mut p)
    .unwrap();
    assert_eq!(
        mixer.mix(&p, range(at.0, end)).unwrap(),
        pool.mix(&p, range(at.0, end), rate, 1).unwrap()
    );
}

fn set_speed(p: &mut Project, id: u64, n: u32, d: u32, reverse: bool) {
    Command::SetClipSpeed {
        id: ClipId(id),
        speed: ClipSpeed::new(n, d).unwrap(),
        reverse,
        ramps: vec![],
        ripple: false,
    }
    .apply(p)
    .unwrap();
}
/// Frequency from positive zero crossings, excluding 10% at each end.
fn frequency(mono: &[f32], rate: u32) -> f64 {
    let edge = mono.len() / 10;
    let signal = &mono[edge..mono.len() - edge];
    let crossings: Vec<_> = signal
        .windows(2)
        .enumerate()
        .filter(|(_, s)| s[0] <= 0.0 && s[1] > 0.0)
        .map(|(i, _)| i)
        .collect();
    (crossings.len() - 1) as f64 * f64::from(rate)
        / (crossings.last().unwrap() - crossings[0]) as f64
}
fn tone(rate: u32, seconds: u32, hz: f32) -> Pcm {
    Pcm::new(
        rate,
        1,
        (0..rate * seconds)
            .map(|i| (i as f32 * std::f32::consts::TAU * hz / rate as f32).sin() * 0.2)
            .collect(),
    )
    .unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn stretched_adjacent_ranges_are_identical_to_one_mix(a in 0_i64..300_000_000, width in 1_i64..200_000_000, tail in 1_i64..200_000_000, output_rate in prop_oneof![Just(44_100_u32), Just(48_000_u32), Just(47_999_u32)], reverse in any::<bool>()) {
        let pcm = sine(44_100);
        let (mut project, pool) = project(&pcm, 17_321, 17_321 + TICKS_PER_SECOND);
        set_speed(&mut project, 0, 3, 4, reverse);
        let mixer = pool.prepare(output_rate, 2).unwrap();
        let (b, c) = (a + width, a + width + tail);
        let mut parts = mixer.mix(&project, range(a, b)).unwrap();
        parts.extend(mixer.mix(&project, range(b, c)).unwrap());
        prop_assert_eq!(parts, mixer.mix(&project, range(a, c)).unwrap());
    }
}

#[test]
fn stretching_survives_extreme_rates() {
    let rate = 48_000;
    for (n, d, seconds, reverse) in [
        (1, 16, 1, false),
        (16, 1, 16, false),
        (1, 16, 1, true),
        (16, 1, 16, true),
    ] {
        let pcm = tone(rate, seconds, 1000.0);
        let (mut p, pool) = project(&pcm, 0, i64::from(seconds) * TICKS_PER_SECOND);
        set_speed(&mut p, 0, n, d, reverse);
        let end = p.tracks()[0].clips()[&ClipId(0)].range.end;
        let mixer = pool.prepare(rate, 1).unwrap();
        let out = mixer.mix(&p, range(0, end.0)).unwrap();
        assert_eq!(
            out.len() as u64,
            u64::from(rate) * u64::from(seconds) * u64::from(d) / u64::from(n)
        );
        assert!(out.iter().all(|s| s.is_finite() && s.abs() < 0.5));
        let hz = frequency(&out, rate);
        assert!(
            (hz - 1000.0).abs() < 30.0,
            "{n}/{d} reverse={reverse}: {hz} Hz"
        );
        // Cold seek into the middle reproduces the same samples.
        let (a, b) = (out.len() / 3, out.len() / 3 + 5000);
        let tick = TICKS_PER_SECOND / i64::from(rate);
        let cold = pool.prepare(rate, 1).unwrap();
        assert_eq!(
            cold.mix(&p, range(a as i64 * tick, b as i64 * tick))
                .unwrap(),
            out[a..b]
        );
    }
}

#[test]
fn maintained_pitch_at_unit_speed_reverse_keeps_exact_sampling() {
    // 1x reverse never uses the stretcher: it already preserves pitch, so the output
    // equals the pitch-shifting path (and the source read backwards), bit for bit.
    let rate = 48_000;
    let pcm = Pcm::new(rate, 1, (0..rate).map(|i| i as f32 / rate as f32).collect()).unwrap();
    let (mut p, pool) = project(&pcm, 0, TICKS_PER_SECOND);
    set_speed(&mut p, 0, 1, 1, true);
    assert!(p.tracks()[0].clips()[&ClipId(0)].maintain_pitch);
    let kept = pool.mix(&p, range(0, TICKS_PER_SECOND), rate, 1).unwrap();
    Command::SetClipMaintainPitch {
        id: ClipId(0),
        maintain_pitch: false,
    }
    .apply(&mut p)
    .unwrap();
    let varispeed = pool.mix(&p, range(0, TICKS_PER_SECOND), rate, 1).unwrap();
    assert_eq!(kept, varispeed);
    let source: Vec<f32> = (0..rate).map(|i| i as f32 / rate as f32).collect();
    assert_eq!(kept[0], source[rate as usize - 1]);
    assert_eq!(kept[rate as usize - 1], source[0]);
}

/// Per-hop RMS of `mono` (hops of `hop` frames), skipping `edge` frames at each end.
fn hop_rms(mono: &[f32], hop: usize, edge: usize) -> Vec<f32> {
    mono[edge..mono.len() - edge]
        .chunks_exact(hop)
        .map(|h| (h.iter().map(|s| s * s).sum::<f32>() / hop as f32).sqrt())
        .collect()
}

/// A deterministic test signal: a pure tone, a rising chirp or a burst of white noise,
/// all of constant amplitude so the loudness of every hop of the stretched result should
/// match.
fn signal(kind: &str, rate: u32, seconds: u32) -> Pcm {
    let frames = (rate * seconds) as usize;
    let mut noise = 0x2545_f491_4f6c_dd1d_u64;
    let samples = (0..frames)
        .map(|i| {
            let t = i as f64 / f64::from(rate);
            let value = match kind {
                "sine" => (std::f64::consts::TAU * 440.0 * t).sin(),
                // 300 Hz to about 3 kHz over the clip: always at least two cycles per hop.
                "chirp" => {
                    let span = f64::from(seconds);
                    (std::f64::consts::TAU * (300.0 * t + 0.5 * (2_700.0 / span) * t * t)).sin()
                }
                _ => {
                    noise ^= noise << 13;
                    noise ^= noise >> 7;
                    noise ^= noise << 17;
                    (noise >> 11) as f64 / (1_u64 << 52) as f64 - 1.0
                }
            };
            (value * 0.2) as f32
        })
        .collect();
    Pcm::new(rate, 1, samples).unwrap()
}

#[test]
fn stretched_loudness_holds_steady_across_segment_resets() {
    let rate = 48_000_u32;
    let hop = rate as usize / 100;
    let segment = hop * 64;
    for kind in ["sine", "chirp", "noise"] {
        let pcm = signal(kind, rate, 4);
        for (n, d) in [(1, 2), (2, 1)] {
            for reverse in [false, true] {
                let (mut p, pool) = project(&pcm, 0, 4 * TICKS_PER_SECOND);
                set_speed(&mut p, 0, n, d, reverse);
                let end = p.tracks()[0].clips()[&ClipId(0)].range.end.0;
                let out = pool.mix(&p, range(0, end), rate, 1).unwrap();
                let rms = hop_rms(&out, hop, rate as usize / 10);
                let mut sorted = rms.clone();
                sorted.sort_by(f32::total_cmp);
                let median = sorted[sorted.len() / 2];
                assert!(median > 0.05, "{kind}: median {median}");
                // Tonal material stays near the median in every hop, including each of the
                // hops where a segment resets and two independent reconstructions meet.
                // Noise has no phase to cancel but WSOLA itself varies its loudness by
                // up to about a third from hop to hop, so only its reset hops are held
                // (to a wider band): a reset must not dip below its neighbours.
                let tolerance = if kind == "noise" { 0.25 } else { 0.2 };
                for (i, level) in rms.iter().enumerate() {
                    let at = rate as usize / 10 + i * hop;
                    let reset = at % segment < hop;
                    if kind == "noise" && !reset {
                        continue;
                    }
                    assert!(
                        (level / median - 1.0).abs() <= tolerance,
                        "{kind} {n}/{d} reverse={reverse}: hop {i} (frame {at}, segment offset {}) \
                         rms {level} vs median {median}",
                        at % segment
                    );
                }
            }
        }
    }
}

#[test]
fn a_stretched_clip_is_not_faded_in_at_its_start_wherever_that_falls_in_a_segment() {
    let rate = 48_000_u32;
    let span = i64::from(rate) / 100 * 64; // frames per stretch segment
    let tick = TICKS_PER_SECOND / i64::from(rate);
    let hop = rate as usize / 100;
    let rms = |x: &[f32]| (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt();
    for start_frames in [
        3 * span,
        3 * span + hop as i64 / 2,
        3 * span + hop as i64 + 7,
        // A start a few frames before a segment boundary: the first hop of the clip
        // coincides with a reset, where a spurious fade would hide.
        4 * span - 5,
    ] {
        let start = start_frames * tick;
        let (mut p, pool) = project(&tone(rate, 2, 440.0), start, start + 2 * TICKS_PER_SECOND);
        set_speed(&mut p, 0, 3, 2, false);
        let end = p.tracks()[0].clips()[&ClipId(0)].range.end.0;
        let out = pool.mix(&p, range(start, end), rate, 1).unwrap();
        // The clip's first hop is already at full level: a linear fade from silence over
        // it would make this ratio about 0.58, and its first half would be far quieter
        // than its second.
        let (first, later) = (rms(&out[..hop]), rms(&out[4 * hop..8 * hop]));
        assert!(first > 0.85 * later, "{start_frames}: {first} vs {later}");
        let (a, b) = (rms(&out[..hop / 2]), rms(&out[hop / 2..hop]));
        assert!(a > 0.75 * b, "{start_frames}: {a} vs {b}");
    }
}

#[test]
fn stretch_cache_is_keyed_by_the_source_end_a_tail_trim_moves() {
    let rate = 48_000;
    let tick = TICKS_PER_SECOND / i64::from(rate);
    let (mut p, pool) = project(&signal("noise", rate, 2), 0, TICKS_PER_SECOND);
    // A retimed clock that never recorded its source span (a hand-written project):
    // the source end the stretcher may read follows the clip's end.
    Command::RestoreClips {
        track: TrackId(0),
        ids: vec![ClipId(0)],
        clips: vec![{
            let mut clip = p.tracks()[0].clips()[&ClipId(0)].clone();
            clip.time_map = ClipTimeMap {
                speed: ClipSpeed::new(3, 2).unwrap(),
                ..ClipTimeMap::default()
            };
            clip
        }],
    }
    .apply(&mut p)
    .unwrap();
    let clip = p.tracks()[0].clips()[&ClipId(0)].clone();
    assert!(clip.time_map.source_length.is_none());
    let mixer = pool.prepare(rate, 1).unwrap();
    let end = clip.range.end.0;
    let cut = end - 20_000 * tick;
    // Warm the cache on the clip trimmed short, then drag its tail back out: the source
    // end the stretcher may read grows with it, so segments of the short clip (whose
    // last grains read silence past the old end) must not be reused.
    let mut short = p.clone();
    Command::TrimClip {
        id: ClipId(0),
        range: range(0, cut),
        source_offset: clip.source_offset,
    }
    .apply(&mut short)
    .unwrap();
    assert!(
        short.tracks()[0].clips()[&ClipId(0)]
            .time_map
            .source_length
            .is_none()
    );
    mixer.mix(&short, range(0, cut)).unwrap();
    let warm = mixer.mix(&p, range(0, end)).unwrap();
    let cold = pool
        .prepare(rate, 1)
        .unwrap()
        .mix(&p, range(0, end))
        .unwrap();
    let diff = warm.iter().zip(&cold).filter(|(a, b)| a != b).count();
    assert!(warm == cold, "{diff} samples reused from the shorter clip");
}

#[test]
fn stretching_follows_trim_and_merge() {
    let rate = 48_000;
    let tick = TICKS_PER_SECOND / i64::from(rate);
    let (mut p, pool) = project(&tone(rate, 2, 500.0), 0, 2 * TICKS_PER_SECOND);
    set_speed(&mut p, 0, 3, 2, false);
    let end = p.tracks()[0].clips()[&ClipId(0)].range.end.0;
    let mixer = pool.prepare(rate, 1).unwrap();
    let whole = mixer.mix(&p, range(0, end)).unwrap();
    let clip = p.tracks()[0].clips()[&ClipId(0)].clone();

    // Trimming the tail keeps every remaining sample.
    let mut tail = p.clone();
    let cut = end - 9_001 * tick;
    Command::TrimClip {
        id: ClipId(0),
        range: range(0, cut),
        source_offset: clip.source_offset,
    }
    .apply(&mut tail)
    .unwrap();
    assert_eq!(
        mixer.mix(&tail, range(0, cut)).unwrap(),
        whole[..(cut / tick) as usize]
    );

    // Trimming the head moves the clock phase, not the sound: from one segment after the
    // new start (the first hop of the new first segment has no predecessor to blend with)
    // the samples are those of the untrimmed clip.
    let mut head = p.clone();
    let head_ticks = 12_345 * tick;
    Command::TrimClip {
        id: ClipId(0),
        range: range(head_ticks, end),
        source_offset: Time(clip.source_offset.0 + head_ticks),
    }
    .apply(&mut head)
    .unwrap();
    let trimmed = mixer.mix(&head, range(head_ticks, end)).unwrap();
    let skip = 48_000 / 100 * 64 + 2_000;
    assert_eq!(trimmed[skip..], whole[12_345 + skip..]);

    // Split then merge restores the clip, bit for bit.
    let mut merged = p.clone();
    Command::SplitClip {
        id: ClipId(0),
        right_id: ClipId(1),
        at: Time(31_337 * tick + 5),
    }
    .apply(&mut merged)
    .unwrap();
    assert_eq!(mixer.mix(&merged, range(0, end)).unwrap(), whole);
    Command::MergeClip {
        left: ClipId(0),
        right: ClipId(1),
    }
    .apply(&mut merged)
    .unwrap();
    assert_eq!(merged, p);
    assert_eq!(mixer.mix(&merged, range(0, end)).unwrap(), whole);
}

#[test]
fn cross_dissolve_on_a_stretched_clip_is_equal_power_and_range_independent() {
    let rate = 8000;
    let second = TICKS_PER_SECOND;
    let pcm = Pcm::new(rate, 1, vec![1.0; 4 * rate as usize]).unwrap();
    let (mut project, pool) = project(&pcm, 0, 2 * second);
    set_speed(&mut project, 0, 2, 1, false); // 2 s of source -> 1 s, stretched
    let asset = AssetId::of(b"tone");
    let mut incoming = Clip::new(ClipId(1), ClipKind::Audio(asset), range(second, 2 * second));
    incoming.source_offset = Time(second); // one second of handle before the cut
    for command in [
        Command::AddClip {
            track: TrackId(0),
            clip: incoming,
        },
        Command::AddTransition {
            track: TrackId(0),
            transition: Transition {
                id: TransitionId(0),
                kind: TransitionKind::CrossDissolve,
                duration: Time(second * 2 / 5),
                from: ClipId(0),
                to: ClipId(1),
            },
        },
    ] {
        command.apply(&mut project).unwrap();
    }
    let mixer = pool.prepare(rate, 1).unwrap();
    let whole = mixer.mix(&project, range(0, 2 * second)).unwrap();
    assert_eq!(whole.len(), 2 * rate as usize);
    assert!(whole.iter().all(|s| s.is_finite()));
    // Before the window the stretched clip plays alone at unity.
    assert!((whole[2000] - 1.0).abs() < 1e-4);
    // Inside the window, before the stretched clip ends at 1 s, both clips overlap with equal-power gains.
    for (frame, progress) in [
        (6400, 0.0),
        (6720, 0.1),
        (7040, 0.2),
        (7360, 0.3),
        (7680, 0.4),
    ] {
        let (out, inc) = equal_power(progress);
        assert!(
            (f64::from(whole[frame]) - (out + inc)).abs() < 1e-3,
            "{frame}: {}",
            whole[frame]
        );
    }
    // After the window the incoming clip plays alone.
    assert!((whole[9700] - 1.0).abs() < 1e-6);
    // Arbitrary splits of the dissolve window reproduce the one-shot mix.
    let tick = second / i64::from(rate);
    for cut in [6_401, 7_003, 7_777] {
        let mut parts = mixer.mix(&project, range(0, cut * tick)).unwrap();
        parts.extend(mixer.mix(&project, range(cut * tick, 2 * second)).unwrap());
        assert_eq!(parts, whole);
    }
}
