//! Audio-effect plugins through the real sandbox: discovery, per-effect behaviour, state carried
//! between blocks, host-enforced limits and warm-instance reuse.
use animathio_core::{AudioBlock, AudioEffectProcessor, EffectId, EffectInstance, Time};
use animathio_plugin_api::{PluginManifestToml, PluginPackage};
use animathio_plugin_host::{PluginHost, SandboxLimits, test_support};
use std::collections::BTreeMap;
use std::f64::consts::PI;

const PLUGIN: &str = "org.animathio.audio-basic";
const RATE: u32 = 48_000;

fn effect(id: &str, params: &[(&str, f64)]) -> EffectInstance {
    EffectInstance::new(
        EffectId(1),
        PLUGIN,
        id,
        params
            .iter()
            .map(|(k, v)| ((*k).to_owned(), *v))
            .collect::<BTreeMap<_, _>>(),
    )
}

fn sine(freq: f64, amplitude: f64, frames: usize, channels: usize) -> Vec<f32> {
    (0..frames)
        .flat_map(|n| {
            let v = (amplitude * (2.0 * PI * freq * n as f64 / f64::from(RATE)).sin()) as f32;
            std::iter::repeat_n(v, channels)
        })
        .collect()
}

fn block(samples: &[f32], channels: u16, start: i64) -> AudioBlock<'_> {
    AudioBlock {
        sample_rate: RATE,
        channels,
        start: Time(start),
        samples,
    }
}

fn rms(samples: &[f32]) -> f64 {
    (samples.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / samples.len() as f64).sqrt()
}

fn db(ratio: f64) -> f64 {
    20.0 * ratio.log10()
}

fn handle() -> impl AudioEffectProcessor {
    test_support::audio_effect_processor()
}

#[test]
fn host_lists_the_six_audio_effects_with_defaults() {
    let host = test_support::host();
    let ids: Vec<_> = host
        .audio_effects()
        .into_iter()
        .filter(|e| e.plugin_id == PLUGIN)
        .map(|e| e.effect_id)
        .collect();
    assert_eq!(
        ids,
        [
            "gain",
            "eq3",
            "compressor",
            "gate",
            "highpass",
            "voice_cleanup"
        ]
    );
    let compressor = host
        .audio_effects()
        .into_iter()
        .find(|e| e.effect_id == "compressor")
        .unwrap();
    let defaults = compressor.default_params();
    assert_eq!(defaults["threshold_db"], -18.0);
    assert_eq!(defaults["ratio"], 4.0);
    assert_eq!(compressor.instance(EffectId(7)).effect, "compressor");
    // Audio filters are not pixel filters.
    assert!(host.effects().iter().all(|e| e.plugin_id != PLUGIN));
}

#[test]
fn gain_eq_and_highpass_behave_through_the_sandbox() {
    let handle = handle();
    let input = sine(1000.0, 0.25, 9600, 2);
    let out = handle
        .process(
            &effect("gain", &[("gain_db", -6.0)]),
            &block(&input, 2, 0),
            &[],
        )
        .unwrap();
    assert!(out.state.is_empty(), "gain is stateless");
    assert!((db(rms(&out.samples) / rms(&input)) + 6.0).abs() < 1e-3);

    let out = handle
        .process(
            &effect(
                "eq3",
                &[("mid_gain_db", 9.0), ("mid_freq", 1000.0), ("mid_q", 1.0)],
            ),
            &block(&input, 2, 0),
            &[],
        )
        .unwrap();
    let gain = db(rms(&out.samples[4800..]) / rms(&input[4800..]));
    assert!((gain - 9.0).abs() < 0.3, "{gain}");

    let rumble = sine(30.0, 0.5, 24_000, 1);
    let out = handle
        .process(
            &effect("highpass", &[("frequency", 300.0)]),
            &block(&rumble, 1, 0),
            &[],
        )
        .unwrap();
    assert!(db(rms(&out.samples[12_000..]) / rms(&rumble[12_000..])) < -30.0);
}

#[test]
fn compressor_gate_and_cleanup_run_through_the_sandbox() {
    let handle = handle();
    let loud = vec![1.0_f32; 24_000];
    let out = handle
        .process(
            &effect(
                "compressor",
                &[
                    ("threshold_db", -20.0),
                    ("ratio", 4.0),
                    ("attack_ms", 0.1),
                    ("release_ms", 5.0),
                ],
            ),
            &block(&loud, 1, 0),
            &[],
        )
        .unwrap();
    assert!((db(f64::from(out.samples[23_999])) + 15.0).abs() < 0.1);
    assert!(!out.state.is_empty(), "compressor carries its envelope");

    let hiss = sine(440.0, 0.002, 24_000, 1);
    let out = handle
        .process(
            &effect(
                "gate",
                &[
                    ("threshold_db", -30.0),
                    ("release_ms", 10.0),
                    ("hold_ms", 0.0),
                ],
            ),
            &block(&hiss, 1, 0),
            &[],
        )
        .unwrap();
    assert!(db(rms(&out.samples[12_000..]) / rms(&hiss[12_000..])) < -50.0);

    let rumble = sine(25.0, 0.3, 24_000, 2);
    let out = handle
        .process(&effect("voice_cleanup", &[]), &block(&rumble, 2, 0), &[])
        .unwrap();
    assert!(db(rms(&out.samples[24_000..]) / rms(&rumble[24_000..])) < -20.0);
}

#[test]
fn state_carries_across_blocks_bit_exactly() {
    let handle = handle();
    let channels = 2;
    let input: Vec<f32> = sine(180.0, 0.7, 12_000, channels)
        .iter()
        .zip(sine(6800.0, 0.4, 12_000, channels))
        .map(|(a, b)| a + b)
        .collect();
    for id in ["eq3", "compressor", "gate", "highpass", "voice_cleanup"] {
        let fx = effect(id, &[]);
        let whole = handle.process(&fx, &block(&input, 2, 0), &[]).unwrap();
        let mut pieces = Vec::new();
        let mut state = Vec::new();
        for (index, chunk) in input.chunks(1500 * channels).enumerate() {
            let start = (index * 1500) as i64 * 20_000;
            let out = handle
                .process(&fx, &block(chunk, 2, start), &state)
                .unwrap();
            pieces.extend(out.samples);
            state = out.state;
        }
        assert_eq!(
            whole.samples, pieces,
            "{id} differs across block boundaries"
        );
        assert_eq!(whole.state, state, "{id} ends in a different state");
    }
}

#[test]
fn invalid_input_is_reported_not_trapped() {
    let handle = handle();
    let input = sine(440.0, 0.5, 100, 2);
    let call = |fx: &EffectInstance, samples: &[f32], channels: u16, state: &[u8]| {
        handle
            .process(fx, &block(samples, channels, 0), state)
            .unwrap_err()
    };
    assert!(call(&effect("nope", &[]), &input, 2, &[]).contains("no audio effect"));
    assert!(call(&effect("gain", &[("bogus", 1.0)]), &input, 2, &[]).contains("bogus"));
    assert!(call(&effect("gain", &[]), &input, 0, &[]).contains("unsupported"));
    assert!(call(&effect("gain", &[]), &input[..199], 2, &[]).contains("whole"));
    assert!(call(&effect("gain", &[]), &[], 2, &[]).contains("nonzero"));
    assert!(call(&effect("highpass", &[]), &input, 2, &[1, 2, 3]).contains("failed"));
    let missing = EffectInstance::new(EffectId(1), "org.example.none", "gain", BTreeMap::new());
    assert!(call(&missing, &input, 2, &[]).contains("not loaded"));
    // A failing call does not poison the warm instance used by the next one.
    assert!(
        handle
            .process(&effect("gain", &[]), &block(&input, 2, 0), &[])
            .is_ok()
    );
}

fn limited_host(limits: SandboxLimits) -> PluginHost {
    let cache = tempfile::tempdir().unwrap();
    let mut host = PluginHost::with_limits(cache.path(), limits).unwrap();
    for package in test_support::packages_for_tests() {
        if package.manifest.id == PLUGIN {
            host.load_plugin_package(package.clone()).unwrap();
        }
    }
    // Keep the temp dir alive for the host's disk cache by leaking it for the test process.
    std::mem::forget(cache);
    host
}

#[test]
fn host_limits_cap_block_size_state_and_fuel() {
    let input = sine(440.0, 0.5, 4096, 2);

    let host = limited_host(SandboxLimits {
        max_audio_block_frames: 1000,
        ..SandboxLimits::default()
    });
    let handle = host.audio_effect_processor();
    let err = handle
        .process(&effect("gain", &[]), &block(&input, 2, 0), &[])
        .unwrap_err();
    assert!(err.contains("exceeds the limit"), "{err}");

    let host = limited_host(SandboxLimits {
        max_audio_state_bytes: 8,
        ..SandboxLimits::default()
    });
    let handle = host.audio_effect_processor();
    let err = handle
        .process(&effect("eq3", &[]), &block(&input, 2, 0), &[])
        .unwrap_err();
    assert!(err.contains("state"), "{err}");

    // Starve the fuel budget: the call traps instead of running to completion, and the
    // instance is discarded rather than pooled.
    let host = limited_host(SandboxLimits {
        fuel_per_call: 50_000_000,
        audio_fuel_per_sample: 0,
        ..SandboxLimits::default()
    });
    let handle = host.audio_effect_processor();
    let big = sine(440.0, 0.5, 60_000, 2);
    let err = handle.process(&effect("voice_cleanup", &[]), &block(&big, 2, 0), &[]);
    // 50M instructions may or may not be enough for 60k frames; shrink it decisively.
    drop(err);
    let host = limited_host(SandboxLimits {
        fuel_per_call: 2_000_000,
        audio_fuel_per_sample: 0,
        ..SandboxLimits::default()
    });
    let handle = host.audio_effect_processor();
    let err = handle
        .process(&effect("voice_cleanup", &[]), &block(&big, 2, 0), &[])
        .unwrap_err();
    assert!(err.contains("trapped"), "{err}");
    // The same host still works for small blocks afterwards.
    let small = sine(440.0, 0.5, 8, 1);
    assert!(
        handle
            .process(&effect("gain", &[]), &block(&small, 1, 0), &[])
            .is_ok()
    );
}

#[test]
fn audio_effect_plugins_must_declare_api_0_2_2_and_only_that_kind() {
    let manifest = |api: &str, kinds: &str| {
        format!(
            "api_version = \"{api}\"\nid = \"org.example.a\"\nname = \"A\"\nversion = \"1.0.0\"\nkinds = [{kinds}]\n"
        )
    };
    assert!(PluginManifestToml::parse(&manifest("0.2.2", "\"audio-effect\"")).is_ok());
    assert!(PluginManifestToml::parse(&manifest("0.2.1", "\"audio-effect\"")).is_err());
    assert!(PluginManifestToml::parse(&manifest("0.2.2", "\"audio-effect\", \"effect\"")).is_err());
    // A package that names a different world than its wasm implements is rejected at load.
    let audio = test_support::packages_for_tests()
        .iter()
        .find(|p| p.manifest.id == PLUGIN)
        .unwrap();
    let mut wrong = audio.manifest.clone();
    wrong.id = "org.example.mislabelled".to_owned();
    wrong.kinds = vec![animathio_plugin_api::PluginKind::Effect];
    wrong.api_version = "0.2.2".to_owned();
    let package = PluginPackage::new(wrong, audio.wasm_bytes.clone()).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut host = PluginHost::new(cache.path()).unwrap();
    assert!(host.load_plugin_package(package).is_err());
}

/// Cost of every effect per second of 48 kHz stereo audio, in blocks of 2048 frames, through
/// the pooled warm instances.
///
/// `cargo test -p animathio-plugin-host --features test-support --test audio_effects \
///  -- --ignored --nocapture bench_audio`
#[test]
#[ignore = "benchmark; run with --ignored --nocapture"]
fn bench_audio() {
    let handle = handle();
    let seconds = 10;
    let frames = RATE as usize * seconds;
    let input = sine(440.0, 0.3, frames, 2);
    for id in [
        "gain",
        "eq3",
        "compressor",
        "gate",
        "highpass",
        "voice_cleanup",
    ] {
        let fx = effect(id, &[]);
        let run = || {
            let mut state = Vec::new();
            for chunk in input.chunks(2048 * 2) {
                let out = handle.process(&fx, &block(chunk, 2, 0), &state).unwrap();
                state = out.state;
            }
        };
        run();
        let start = std::time::Instant::now();
        run();
        let per_second = start.elapsed().as_secs_f64() * 1000.0 / seconds as f64;
        println!("{id:>14}: {per_second:7.2} ms per second of 48 kHz stereo");
    }
}
