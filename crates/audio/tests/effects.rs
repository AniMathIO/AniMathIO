//! Audio-effect chains in the mixer, against a mock processor: signal flow, bypass,
//! determinism across arbitrary mix ranges, the block cache and error handling.
#![allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::chunks_exact_to_as_chunks
)]
use animathio_audio::{AudioPool, EffectWarnings, Mixer, Pcm};
use animathio_core::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

const RATE: u32 = 48_000;

fn range(start: i64, end: i64) -> TimeRange {
    TimeRange::new(Time(start), Time(end)).unwrap()
}

/// `gain`: multiply by `gain` (stateless). `leaky`: `y[n] = x[n] + decay * y[n-1]` per channel,
/// carrying `y` between blocks as the state; `fail` always errors.
#[derive(Default)]
struct Mock {
    calls: AtomicUsize,
}

impl AudioEffectProcessor for Mock {
    fn process(
        &self,
        effect: &EffectInstance,
        block: &AudioBlock<'_>,
        state: &[u8],
    ) -> Result<AudioBlockOutput, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let channels = usize::from(block.channels);
        match effect.effect.as_str() {
            "gain" => {
                let gain = effect.params.get("gain").copied().unwrap_or(1.0) as f32;
                Ok(AudioBlockOutput {
                    samples: block.samples.iter().map(|x| x * gain).collect(),
                    state: Vec::new(),
                })
            }
            "leaky" => {
                let decay = effect.params.get("decay").copied().unwrap_or(0.9) as f32;
                let mut last: Vec<f32> = if state.is_empty() {
                    vec![0.0; channels]
                } else {
                    state
                        .chunks_exact(4)
                        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                        .collect()
                };
                let mut out = block.samples.to_vec();
                for frame in out.chunks_exact_mut(channels) {
                    for (x, y) in frame.iter_mut().zip(&mut last) {
                        *y = *x + decay * *y;
                        *x = *y;
                    }
                }
                Ok(AudioBlockOutput {
                    samples: out,
                    state: last.iter().flat_map(|y| y.to_le_bytes()).collect(),
                })
            }
            "start" => {
                // Writes the block's start time (in ms) into every sample, to check what the
                // mixer reports.
                let ms = block.start.0 as f32 / (TICKS_PER_SECOND / 1000) as f32;
                Ok(AudioBlockOutput {
                    samples: vec![ms; block.samples.len()],
                    state: Vec::new(),
                })
            }
            _ => Err("mock failure".to_owned()),
        }
    }
}

fn fx(id: u64, effect: &str, params: &[(&str, f64)]) -> EffectInstance {
    EffectInstance::new(
        EffectId(id),
        "mock",
        effect,
        params.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect(),
    )
}

/// One mono audio track with one clip of `seconds` of a 220 Hz tone starting at `start`.
fn fixture(seconds: u32, start: i64) -> (Project, Mixer) {
    let id = AssetId::of(b"tone");
    let pcm = Pcm::new(
        RATE,
        1,
        (0..RATE * seconds)
            .map(|i| (i as f32 * std::f32::consts::TAU * 220.0 / RATE as f32).sin() * 0.5)
            .collect(),
    )
    .unwrap();
    let mut project = Project::default();
    for command in [
        Command::AddAsset {
            id: id.clone(),
            asset: Asset {
                name: "tone".into(),
                media_type: "audio/wav".into(),
            },
        },
        Command::AddTrack {
            track: Track::new(TrackId(0), "audio"),
            index: 0,
        },
        Command::AddClip {
            track: TrackId(0),
            clip: Clip::new(
                ClipId(0),
                ClipKind::Audio(id.clone()),
                range(start, start + i64::from(seconds) * TICKS_PER_SECOND),
            ),
        },
    ] {
        command.apply(&mut project).unwrap();
    }
    let mut pool = AudioPool::default();
    pool.insert(id, &pcm).unwrap();
    (project, pool.prepare(RATE, 1).unwrap())
}

fn add(project: &mut Project, chain: AudioChain, effect: EffectInstance) {
    let index = match chain {
        AudioChain::Clip(_) => project.tracks()[0].clips()[&ClipId(0)].audio_effects.len(),
        AudioChain::Track(_) => project.tracks()[0].audio_effects().len(),
    };
    Command::AddAudioEffect {
        chain,
        index,
        effect,
    }
    .apply(project)
    .unwrap();
}

fn mixed(mixer: &Mixer, project: &Project, from: i64, to: i64) -> Vec<f32> {
    mixer.mix(project, range(from, to)).unwrap()
}

#[test]
fn clip_and_track_chains_apply_in_order_before_the_fader() {
    let (mut project, dry) = fixture(1, 0);
    let mock = Arc::new(Mock::default());
    let mixer = dry.clone().with_effect_processor(mock.clone());
    let plain = mixed(&dry, &project, 0, TICKS_PER_SECOND / 100);

    add(
        &mut project,
        AudioChain::Clip(ClipId(0)),
        fx(1, "gain", &[("gain", 2.0)]),
    );
    let clip_fx = mixed(&mixer, &project, 0, TICKS_PER_SECOND / 100);
    for (a, b) in clip_fx.iter().zip(&plain) {
        assert_eq!(*a, *b * 2.0);
    }

    // The track chain sees the clip chain's output, and the fader comes last.
    add(
        &mut project,
        AudioChain::Track(TrackId(0)),
        fx(1, "gain", &[("gain", 3.0)]),
    );
    Command::SetTrackGain {
        id: TrackId(0),
        gain: 0.5,
    }
    .apply(&mut project)
    .unwrap();
    let both = mixed(&mixer, &project, 0, TICKS_PER_SECOND / 100);
    for (a, b) in both.iter().zip(&plain) {
        assert!((a - b * 2.0 * 3.0 * 0.5).abs() < 1e-6, "{a} {b}");
    }
    assert!(mock.calls.load(Ordering::SeqCst) > 0);
}

#[test]
fn clip_gain_feeds_the_chain_and_fades_follow_it() {
    let (mut project, dry) = fixture(1, 0);
    let mixer = dry.clone().with_effect_processor(Arc::new(Mock::default()));
    Command::SetParam {
        id: ClipId(0),
        name: "gain".into(),
        value: Some(0.5),
    }
    .apply(&mut project)
    .unwrap();
    let plain = mixed(&dry, &project, 0, TICKS_PER_SECOND / 100);
    add(
        &mut project,
        AudioChain::Clip(ClipId(0)),
        fx(1, "gain", &[("gain", 4.0)]),
    );
    let out = mixed(&mixer, &project, 0, TICKS_PER_SECOND / 100);
    for (a, b) in out.iter().zip(&plain) {
        assert!((a - b * 4.0).abs() < 1e-6);
    }
    // Muting silences the chain input too.
    Command::SetParam {
        id: ClipId(0),
        name: "muted".into(),
        value: Some(1.0),
    }
    .apply(&mut project)
    .unwrap();
    assert!(
        mixed(&mixer, &project, 0, TICKS_PER_SECOND / 100)
            .iter()
            .all(|x| *x == 0.0)
    );
}

#[test]
fn bypassed_and_absent_chains_equal_the_dry_mix_bit_for_bit() {
    let (mut project, dry) = fixture(1, 0);
    let mixer = dry.clone().with_effect_processor(Arc::new(Mock::default()));
    let plain = mixed(&dry, &project, 0, TICKS_PER_SECOND / 10);
    add(
        &mut project,
        AudioChain::Clip(ClipId(0)),
        fx(1, "gain", &[("gain", 9.0)]),
    );
    add(
        &mut project,
        AudioChain::Track(TrackId(0)),
        fx(1, "leaky", &[]),
    );
    for chain in [AudioChain::Clip(ClipId(0)), AudioChain::Track(TrackId(0))] {
        Command::SetAudioEffectBypass {
            chain,
            effect: EffectId(1),
            bypass: true,
        }
        .apply(&mut project)
        .unwrap();
    }
    assert_eq!(mixed(&mixer, &project, 0, TICKS_PER_SECOND / 10), plain);
    // `dry()` ignores chains entirely, even without a processor.
    Command::SetAudioEffectBypass {
        chain: AudioChain::Clip(ClipId(0)),
        effect: EffectId(1),
        bypass: false,
    }
    .apply(&mut project)
    .unwrap();
    assert_eq!(
        mixed(&dry.clone().dry(), &project, 0, TICKS_PER_SECOND / 10),
        plain
    );
}

#[test]
fn active_chains_without_a_processor_or_with_a_failing_one_are_errors() {
    let (mut project, dry) = fixture(1, 0);
    add(
        &mut project,
        AudioChain::Clip(ClipId(0)),
        fx(1, "gain", &[]),
    );
    let error = dry
        .mix(&project, range(0, TICKS_PER_SECOND / 100))
        .unwrap_err();
    assert!(error.to_string().contains("effect processor"), "{error}");

    let (mut project, dry) = fixture(1, 0);
    add(
        &mut project,
        AudioChain::Track(TrackId(0)),
        fx(1, "explode", &[]),
    );
    let mixer = dry.with_effect_processor(Arc::new(Mock::default()));
    let error = mixer
        .mix(&project, range(0, TICKS_PER_SECOND / 100))
        .unwrap_err();
    assert!(error.to_string().contains("mock failure"), "{error}");
}

#[test]
fn output_is_a_pure_function_of_the_project_whatever_ranges_are_mixed() {
    // 3.5 s crosses the 64-block segment boundary (about 2.73 s at 48 kHz) so the warm-up path
    // is exercised, and the clip starts off the sample lattice.
    let start = TICKS_PER_SECOND / 3 + 7;
    let (mut project, dry) = fixture(4, start);
    add(
        &mut project,
        AudioChain::Clip(ClipId(0)),
        fx(1, "leaky", &[("decay", 0.999)]),
    );
    add(
        &mut project,
        AudioChain::Track(TrackId(0)),
        fx(1, "leaky", &[("decay", 0.99)]),
    );
    let end = start + 7 * TICKS_PER_SECOND / 2;

    let whole = mixed(
        &dry.clone().with_effect_processor(Arc::new(Mock::default())),
        &project,
        start,
        end,
    );
    assert!(whole.iter().any(|x| x.abs() > 0.1));

    // Sequential small ranges on one mixer (cache hits and continuation)...
    let mixer = dry.clone().with_effect_processor(Arc::new(Mock::default()));
    let mut pieces = Vec::new();
    let step = TICKS_PER_SECOND / 90;
    let mut at = start;
    while at < end {
        let next = (at + step).min(end);
        pieces.extend(mixed(&mixer, &project, at, next));
        at = next;
    }
    assert_eq!(pieces, whole);

    // ...and cold, out-of-order ranges on fresh mixers, deep into the second segment.
    for (from, to) in [
        (
            start + 3 * TICKS_PER_SECOND,
            start + 3 * TICKS_PER_SECOND + 4_000_000,
        ),
        (start + 2_000_000, start + 2_100_000),
        (end - 1_000_000, end),
    ] {
        let cold = dry.clone().with_effect_processor(Arc::new(Mock::default()));
        let expected = {
            // Index the full mix by absolute frame.
            let lattice = |t: i64| {
                usize::try_from(
                    (i128::from(t) * i128::from(RATE) + i128::from(TICKS_PER_SECOND) - 1)
                        / i128::from(TICKS_PER_SECOND),
                )
                .unwrap()
            };
            let base = lattice(start);
            whole[lattice(from) - base..lattice(to) - base].to_vec()
        };
        assert_eq!(mixed(&cold, &project, from, to), expected);
    }
}

#[test]
fn sequential_mixing_processes_each_block_once() {
    let (mut project, dry) = fixture(1, 0);
    add(
        &mut project,
        AudioChain::Clip(ClipId(0)),
        fx(1, "leaky", &[]),
    );
    let mock = Arc::new(Mock::default());
    let mixer = dry.with_effect_processor(mock.clone());
    let step = TICKS_PER_SECOND / 100;
    for index in 0..100 {
        mixed(&mixer, &project, index * step, (index + 1) * step);
    }
    // One second is 24 blocks of 2048 frames (rounded up); repeats hit the cache.
    let calls = mock.calls.load(Ordering::SeqCst);
    assert_eq!(calls, (RATE as usize).div_ceil(2048), "{calls}");
    mixed(&mixer, &project, 0, TICKS_PER_SECOND);
    assert_eq!(mock.calls.load(Ordering::SeqCst), calls);
    // Editing a parameter changes the key: the chain is recomputed, not served stale.
    Command::SetAudioEffectParam {
        chain: AudioChain::Clip(ClipId(0)),
        effect: EffectId(1),
        name: "decay".into(),
        value: Some(0.5),
    }
    .apply(&mut project)
    .unwrap();
    mixed(&mixer, &project, 0, step);
    assert_eq!(mock.calls.load(Ordering::SeqCst), calls + 1);
}

#[test]
fn keyframed_parameters_are_sampled_per_block_in_clip_local_time() {
    let (mut project, dry) = fixture(2, TICKS_PER_SECOND);
    let mixer = dry.with_effect_processor(Arc::new(Mock::default()));
    add(
        &mut project,
        AudioChain::Clip(ClipId(0)),
        fx(1, "gain", &[("gain", 1.0)]),
    );
    for (time, value) in [(0, 1.0), (TICKS_PER_SECOND, 3.0)] {
        Command::AddKeyframe {
            id: ClipId(0),
            target: Target::Effect {
                effect: EffectId(1),
                param: "gain".into(),
            },
            time: Time(time),
            keyframe: Keyframe {
                value: value.into(),
                interpolation: Interpolation::Linear,
            },
        }
        .apply(&mut project)
        .unwrap();
    }
    // At the clip start the gain is 1, one second in it is 3.
    let first = mixed(
        &mixer,
        &project,
        TICKS_PER_SECOND,
        TICKS_PER_SECOND + 10_000_000,
    );
    let later = mixed(
        &mixer,
        &project,
        2 * TICKS_PER_SECOND,
        2 * TICKS_PER_SECOND + 10_000_000,
    );
    let peak = |x: &[f32]| x.iter().fold(0.0_f32, |m, v| m.max(v.abs()));
    assert!(peak(&first) < 0.51 * 1.05, "{}", peak(&first));
    assert!(peak(&later) > 0.45 * 2.5, "{}", peak(&later));
}

#[test]
fn chain_blocks_receive_chain_local_times() {
    let start = TICKS_PER_SECOND;
    let (mut project, dry) = fixture(1, start);
    let mixer = dry.with_effect_processor(Arc::new(Mock::default()));
    add(
        &mut project,
        AudioChain::Clip(ClipId(0)),
        fx(1, "start", &[]),
    );
    // Clip-local: the first block of the clip (frame 48000, block 23) starts 0.0 ms into the
    // clip (block 23 begins before the clip, hence slightly negative or zero).
    let at_clip_start = mixed(&mixer, &project, start, start + 100_000);
    assert!(
        at_clip_start[0] <= 0.0 && at_clip_start[0] > -43.0,
        "{}",
        at_clip_start[0]
    );
    let later = mixed(
        &mixer,
        &project,
        start + TICKS_PER_SECOND / 2,
        start + TICKS_PER_SECOND / 2 + 100_000,
    );
    assert!((later[0] - 500.0).abs() < 43.0, "{}", later[0]);

    // A track chain reports project time instead.
    let (mut project, dry) = fixture(1, start);
    let mixer = dry.with_effect_processor(Arc::new(Mock::default()));
    add(
        &mut project,
        AudioChain::Track(TrackId(0)),
        fx(1, "start", &[]),
    );
    let later = mixed(
        &mixer,
        &project,
        start + TICKS_PER_SECOND / 2,
        start + TICKS_PER_SECOND / 2 + 100_000,
    );
    assert!((later[0] - 1500.0).abs() < 43.0, "{}", later[0]);
}

#[test]
fn lenient_mixers_pass_audio_through_with_a_warning_and_strict_ones_fail_clearly() {
    let (mut project, dry) = fixture(1, 0);
    let plain = mixed(&dry, &project, 0, TICKS_PER_SECOND / 10);
    add(
        &mut project,
        AudioChain::Clip(ClipId(0)),
        fx(1, "gain", &[("gain", 2.0)]),
    );
    add(
        &mut project,
        AudioChain::Clip(ClipId(0)),
        fx(2, "explode", &[]),
    );

    // Strict (export): the error names the failing effect.
    let strict = dry.clone().with_effect_processor(Arc::new(Mock::default()));
    let error = strict
        .mix(&project, range(0, TICKS_PER_SECOND / 10))
        .unwrap_err();
    assert!(error.to_string().contains("mock/explode"), "{error}");

    // Lenient (preview): the failing effect is skipped, the working one still applies.
    let warnings = EffectWarnings::default();
    let mock = Arc::new(Mock::default());
    let lenient = dry
        .clone()
        .with_effect_processor(mock.clone())
        .lenient(warnings.clone());
    let out = mixed(&lenient, &project, 0, TICKS_PER_SECOND / 10);
    for (a, b) in out.iter().zip(&plain) {
        assert_eq!(*a, *b * 2.0);
    }
    let taken = warnings.take();
    assert_eq!(taken.len(), 1, "{taken:?}");
    assert!(taken[0].contains("mock/explode") && taken[0].contains("unprocessed"));
    assert_eq!(warnings.take().len(), 0);

    // Skipped blocks are never cached, so a retry runs (and warns) again.
    let calls = mock.calls.load(Ordering::SeqCst);
    mixed(&lenient, &project, 0, TICKS_PER_SECOND / 10);
    assert!(mock.calls.load(Ordering::SeqCst) > calls);
    assert_eq!(warnings.take().len(), 1);

    // No processor at all: pass through everything, warn once.
    let none = dry.lenient(warnings.clone());
    assert_eq!(mixed(&none, &project, 0, TICKS_PER_SECOND / 10), plain);
    assert!(
        warnings
            .take()
            .iter()
            .any(|w| w.contains("effect processor"))
    );
}
