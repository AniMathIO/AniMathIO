//! Pixel-effect plugins through the real sandbox: discovery, fidelity against the builtin
//! reference, host-enforced limits and compatibility with 0.2.0 components.
mod reference_effects;

use animathio_core::{
    ClipId, ClipKind, EffectId, EffectInstance, EffectProcessor, Scene, SceneClip, Time, TrackId,
    Transform,
};
use animathio_plugin_api::PluginPackage;
use animathio_plugin_host::{PluginHost, SandboxLimits, test_support};
use std::collections::BTreeMap;

const PLUGIN: &str = "org.animathio.effects-basic";
const EFFECTS: [(&str, &str, f64); 8] = [
    ("grayscale", "amount", 1.0),
    ("grayscale", "amount", 0.4),
    ("sepia", "amount", 0.8),
    ("invert", "amount", 1.0),
    ("saturate", "amount", 2.0),
    ("brightness", "amount", 0.7),
    ("contrast", "amount", 1.5),
    ("opacity", "amount", 0.5),
];

fn noise(len: usize, mut seed: u32) -> Vec<u8> {
    (0..len)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 24) as u8
        })
        .collect()
}

/// Every 8-bit value on every channel, so all decode/encode thresholds are exercised.
fn ramp() -> Vec<u8> {
    (0..256 * 4).map(|i| (i / 4) as u8).collect()
}

fn instance(effect: &str, param: &str, value: f64) -> EffectInstance {
    EffectInstance::new(
        EffectId(1),
        PLUGIN,
        effect,
        BTreeMap::from([(param.to_owned(), value)]),
    )
}

fn max_diff(a: &[u8], b: &[u8]) -> u8 {
    a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap()
}

#[test]
fn host_lists_the_eight_basic_effects_with_defaults() {
    let host = test_support::host();
    let effects: Vec<_> = host
        .effects()
        .into_iter()
        .filter(|e| e.plugin_id == PLUGIN)
        .collect();
    let ids: Vec<_> = effects.iter().map(|e| e.effect_id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "grayscale",
            "sepia",
            "invert",
            "saturate",
            "brightness",
            "contrast",
            "blur",
            "opacity"
        ]
    );
    let blur = effects.iter().find(|e| e.effect_id == "blur").unwrap();
    assert_eq!(blur.name, "Blur");
    assert_eq!(blur.params[0].id, "radius");
    assert_eq!(blur.default_params()["radius"], 0.0);
    let instance = effects[0].instance(EffectId(7));
    assert_eq!(instance.plugin, PLUGIN);
    assert_eq!(instance.params["amount"], 1.0);
    // Effect plugins are not clip sources.
    assert!(host.get_plugin(PLUGIN).unwrap().manifest.kinds.len() == 1);
}

#[test]
fn plugin_matches_the_builtin_reference_on_noise_and_a_full_ramp() {
    let mut handle = test_support::effect_processor();
    for (effect, param, value) in EFFECTS {
        for (size, source) in [((16, 16), noise(16 * 16 * 4, 7)), ((256, 1), ramp())] {
            let mut actual = source.clone();
            handle
                .apply(&instance(effect, param, value), Time(0), size, &mut actual)
                .unwrap();
            let mut expected = source;
            reference_effects::apply(
                effect,
                value,
                &mut expected,
                size.0 as usize,
                size.1 as usize,
            );
            assert_eq!(
                max_diff(&actual, &expected),
                0,
                "{effect} {value} on {size:?}"
            );
        }
    }
}

#[test]
fn blur_matches_the_reference_within_one_level() {
    let mut handle = test_support::effect_processor();
    for (radius, size) in [(0.05, (40, 30)), (0.2, (33, 21)), (0.25, (16, 16))] {
        let source = noise(size.0 * size.1 * 4, 99);
        let mut actual = source.clone();
        handle
            .apply(
                &instance("blur", "radius", radius),
                Time(0),
                (size.0 as u32, size.1 as u32),
                &mut actual,
            )
            .unwrap();
        let mut expected = source;
        reference_effects::apply("blur", radius, &mut expected, size.0, size.1);
        assert!(
            max_diff(&actual, &expected) <= 1,
            "blur {radius} on {size:?}: max diff {}",
            max_diff(&actual, &expected)
        );
    }
}

#[test]
fn invalid_requests_fail_without_touching_the_frame() {
    let mut handle = test_support::effect_processor();
    let source = noise(4 * 4 * 4, 3);
    let cases: [(EffectInstance, (u32, u32), &str); 3] = [
        (instance("vignette", "amount", 1.0), (4, 4), "no effect"),
        (
            EffectInstance::new(
                EffectId(1),
                "org.example.absent",
                "grayscale",
                BTreeMap::new(),
            ),
            (4, 4),
            "not loaded",
        ),
        (instance("sepia", "amount", 1.0), (5, 5), "does not match"),
    ];
    for (effect, size, expected) in cases {
        let mut pixels = source.clone();
        let error = handle
            .apply(&effect, Time(0), size, &mut pixels)
            .unwrap_err();
        assert!(
            error.contains(expected),
            "{error:?} should mention {expected:?}"
        );
        assert_eq!(pixels, source, "a failed effect leaves the frame untouched");
    }
}

#[test]
fn out_of_range_stored_params_are_clamped_not_failed() {
    let mut handle = test_support::effect_processor();
    let source = noise(4 * 4 * 4, 3);
    for (effect, param, high, edge) in [
        ("grayscale", "amount", 2.0, 1.0),
        ("grayscale", "amount", -3.0, 0.0),
        ("blur", "radius", -1.0, 0.0),
        ("blur", "radius", 3.0, 0.25),
    ] {
        let mut actual = source.clone();
        handle
            .apply(&instance(effect, param, high), Time(0), (4, 4), &mut actual)
            .unwrap_or_else(|e| panic!("{effect} {param}={high} must clamp, not fail: {e}"));
        let mut expected = source.clone();
        handle
            .apply(
                &instance(effect, param, edge),
                Time(0),
                (4, 4),
                &mut expected,
            )
            .unwrap();
        assert_eq!(
            actual, expected,
            "{effect} {param}={high} renders as {edge}"
        );
    }
}

#[test]
fn effect_commands_are_validated_against_declared_ranges() {
    use animathio_core::{
        Clip, Command, Interpolation, Keyframe, Project, Target, TimeRange, Track, TrackId,
    };
    use animathio_plugin_host::validate_effect_command;
    let host = test_support::host();
    let descriptors = host.effects();
    let mut project = Project::default();
    Command::AddTrack {
        track: Track::new(TrackId(1), "t"),
        index: 0,
    }
    .apply(&mut project)
    .unwrap();
    let mut clip = Clip::new(
        ClipId(1),
        ClipKind::Error("x".into()),
        TimeRange::new(Time(0), Time(100)).unwrap(),
    );
    clip.effects.push(instance("grayscale", "amount", 0.5));
    Command::AddClip {
        track: TrackId(1),
        clip,
    }
    .apply(&mut project)
    .unwrap();
    let check =
        |command: &Command| validate_effect_command(&project, command, &|| descriptors.clone());
    let set = |name: &str, value: f64| Command::SetEffectParam {
        id: ClipId(1),
        effect: EffectId(1),
        name: name.into(),
        value: Some(value),
    };
    assert!(check(&set("amount", 0.9)).is_ok());
    assert!(check(&set("amount", 5.0)).unwrap_err().contains("outside"));
    assert!(
        check(&set("bogus", 0.5))
            .unwrap_err()
            .contains("no parameter")
    );
    // Removing a parameter and unrelated commands are never range checked.
    assert!(
        check(&Command::SetEffectParam {
            id: ClipId(1),
            effect: EffectId(1),
            name: "amount".into(),
            value: None,
        })
        .is_ok()
    );
    let add = |value: f64| Command::AddEffect {
        id: ClipId(1),
        index: 1,
        effect: EffectInstance::new(
            EffectId(2),
            PLUGIN,
            "sepia",
            BTreeMap::from([("amount".to_owned(), value)]),
        ),
    };
    assert!(check(&add(0.5)).is_ok());
    assert!(check(&add(-1.0)).is_err());
    // A batch is checked, including SetEffectParam on an effect added earlier in it.
    let batch = Command::Batch {
        commands: vec![
            add(0.5),
            Command::SetEffectParam {
                id: ClipId(1),
                effect: EffectId(2),
                name: "amount".into(),
                value: Some(9.0),
            },
        ],
    };
    assert!(check(&batch).is_err());
    let keyframe = |value: f64| Command::AddKeyframe {
        id: ClipId(1),
        target: Target::Effect {
            effect: EffectId(1),
            param: "amount".into(),
        },
        time: Time(0),
        keyframe: Keyframe {
            value: value.into(),
            interpolation: Interpolation::default(),
        },
    };
    assert!(check(&keyframe(0.2)).is_ok());
    assert!(check(&keyframe(5.0)).is_err());
    // Undo/redo commands are exempt so history can never be blocked.
    assert!(
        check(&Command::Exact {
            command: Box::new(set("amount", 5.0))
        })
        .is_ok()
    );
}

fn limited_host(limits: SandboxLimits) -> PluginHost {
    let package = test_support::packages_for_tests()
        .iter()
        .find(|p| p.manifest.id == PLUGIN)
        .unwrap()
        .clone();
    let cache = tempfile::tempdir().unwrap();
    let mut host = PluginHost::with_limits(cache.path(), limits).unwrap();
    host.load_plugin_package(package).unwrap();
    // The compiled-component cache directory must outlive the host's lazy reads.
    std::mem::forget(cache);
    host
}

#[test]
fn frames_over_the_pixel_limit_are_rejected_before_running_plugin_code() {
    let host = limited_host(SandboxLimits {
        max_effect_pixels: 100,
        ..SandboxLimits::default()
    });
    let mut handle = host.effect_processor();
    let mut pixels = vec![0; 11 * 10 * 4];
    let error = handle
        .apply(
            &instance("invert", "amount", 1.0),
            Time(0),
            (11, 10),
            &mut pixels,
        )
        .unwrap_err();
    assert!(error.contains("exceeds the effect limit"), "{error}");
    let mut ok = vec![0; 10 * 10 * 4];
    handle
        .apply(
            &instance("invert", "amount", 1.0),
            Time(0),
            (10, 10),
            &mut ok,
        )
        .unwrap();
    assert_eq!(ok[0], 255);
}

#[test]
fn fuel_and_memory_limits_stop_runaway_effects() {
    // Loading (describe + list-effects) uses `fuel_per_call`; a 256x256 blur needs far more.
    let starved = limited_host(SandboxLimits {
        fuel_per_call: 2_000_000,
        effect_fuel_per_pixel: 0,
        ..SandboxLimits::default()
    });
    let mut pixels = noise(256 * 256 * 4, 5);
    let before = pixels.clone();
    let error = starved
        .effect_processor()
        .apply(
            &instance("blur", "radius", 0.1),
            Time(0),
            (256, 256),
            &mut pixels,
        )
        .unwrap_err();
    assert!(error.contains("trapped"), "{error}");
    assert_eq!(pixels, before);

    // The blur planes alone need ~5 MiB; a 2 MiB ceiling stops it at the first allocation.
    let tiny = limited_host(SandboxLimits {
        effect_memory_bytes: 2 * 1024 * 1024,
        ..SandboxLimits::default()
    });
    let error = tiny
        .effect_processor()
        .apply(
            &instance("blur", "radius", 0.1),
            Time(0),
            (256, 256),
            &mut pixels,
        )
        .unwrap_err();
    assert!(
        error.contains("trapped") || error.contains("instantiating"),
        "{error}"
    );
    assert_eq!(pixels, before);

    // With the default per-pixel allowance the same call succeeds.
    let roomy = limited_host(SandboxLimits::default());
    roomy
        .effect_processor()
        .apply(
            &instance("blur", "radius", 0.1),
            Time(0),
            (256, 256),
            &mut pixels,
        )
        .unwrap();
    assert_ne!(pixels, before);
}

#[test]
fn handles_see_plugins_loaded_after_they_were_created() {
    let cache = tempfile::tempdir().unwrap();
    let mut host = PluginHost::new(cache.path()).unwrap();
    let mut handle = host.effect_processor();
    assert!(handle.effects().is_empty());
    let mut pixels = vec![10, 20, 30, 255];
    assert!(
        handle
            .apply(
                &instance("invert", "amount", 1.0),
                Time(0),
                (1, 1),
                &mut pixels
            )
            .unwrap_err()
            .contains("not loaded")
    );
    let package = test_support::packages_for_tests()
        .iter()
        .find(|p| p.manifest.id == PLUGIN)
        .unwrap()
        .clone();
    host.load_plugin_package(package).unwrap();
    assert_eq!(handle.effects().len(), 8);
    handle
        .apply(
            &instance("invert", "amount", 1.0),
            Time(0),
            (1, 1),
            &mut pixels,
        )
        .unwrap();
    assert_eq!(pixels, [245, 235, 225, 255]);
}

#[test]
fn components_built_for_api_0_2_0_still_load_and_render() {
    let package =
        PluginPackage::from_bytes(include_bytes!("fixtures/hello-0.2.0.amplugin")).unwrap();
    assert_eq!(package.manifest.api_version, "0.2.0");
    let cache = tempfile::tempdir().unwrap();
    let mut host = PluginHost::new(cache.path()).unwrap();
    let id = host.load_plugin_package(package).unwrap();
    let scene = Scene {
        canvas: (0, 0),
        clips: vec![SceneClip {
            id: ClipId(1),
            track: TrackId(1),
            kind: ClipKind::Plugin {
                plugin_id: id,
                params: serde_json::json!({}),
            },
            source_time: Time(0),
            fit: animathio_core::Fit::Stretch,
            transform: Transform::default(),
            params: BTreeMap::new(),
            effects: Vec::new(),
            transition: None,
        }],
    };
    let resolved = host.resolve(&scene, (160, 90));
    assert!(
        matches!(&resolved.clips[0].kind, ClipKind::Vector(v) if v.elements.len() == 1),
        "legacy component failed: {:?}",
        resolved.clips[0].kind
    );
}

/// Cost of every effect on a 1080p frame, including instantiation and both frame copies.
///
/// `cargo test --release -p animathio-plugin-host --features test-support --test effects \
///  -- --ignored --nocapture bench_1080p`
#[test]
#[ignore = "benchmark; run with --release --ignored --nocapture"]
fn bench_1080p() {
    let mut handle = test_support::effect_processor();
    let source = noise(1920 * 1080 * 4, 1);
    let cases = [
        ("grayscale", "amount", 1.0),
        ("sepia", "amount", 1.0),
        ("invert", "amount", 1.0),
        ("saturate", "amount", 2.0),
        ("brightness", "amount", 0.7),
        ("contrast", "amount", 1.5),
        ("blur", "radius", 0.01),
        ("opacity", "amount", 0.5),
    ];
    // Fixed cost per call (instantiate + tiny frame), then the large frame.
    let mut tiny = noise(16 * 16 * 4, 2);
    let start = std::time::Instant::now();
    for _ in 0..20 {
        handle
            .apply(
                &instance("opacity", "amount", 0.5),
                Time(0),
                (16, 16),
                &mut tiny,
            )
            .unwrap();
    }
    println!(
        "per-call overhead {:.2} ms",
        start.elapsed().as_secs_f64() * 1000.0 / 20.0
    );
    for (effect, param, value) in cases {
        let effect = instance(effect, param, value);
        let mut best = f64::MAX;
        for _ in 0..5 {
            let mut pixels = source.clone();
            let start = std::time::Instant::now();
            handle
                .apply(&effect, Time(0), (1920, 1080), &mut pixels)
                .unwrap();
            best = best.min(start.elapsed().as_secs_f64() * 1000.0);
        }
        println!("{:<10} {best:7.1} ms", effect.effect);
    }
}
