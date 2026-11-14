//! Compare the frozen binaries with the corresponding ABI 1.0 implementations.
use std::collections::BTreeMap;

use animathio_core::{
    AudioBlock, AudioEffectProcessor, EffectId, EffectInstance, EffectProcessor, Time,
};
use animathio_plugin_api::PluginPackage;
use animathio_plugin_host::{PluginHost, test_support};

fn legacy(bytes: &[u8]) -> (tempfile::TempDir, PluginHost) {
    let cache = tempfile::tempdir().unwrap();
    let mut host = PluginHost::new(cache.path()).unwrap();
    host.load_plugin_package(PluginPackage::from_bytes(bytes).unwrap())
        .unwrap();
    (cache, host)
}

#[test]
fn all_frame_filters_are_bit_identical_across_abi_versions() {
    let (_cache, old) = legacy(include_bytes!("fixtures/effects-basic-0.2.1.amplugin"));
    let new = test_support::host();
    assert_eq!(old.effects(), new.effects());
    let mut old_handle = old.effect_processor();
    let mut new_handle = new.effect_processor();
    for desc in new.effects() {
        let mut effect = desc.instance(EffectId(1));
        for value in [0.0, 0.5, 1.0] {
            for param in &desc.params {
                effect.params.insert(
                    param.id.clone(),
                    param.min + (param.max - param.min) * value,
                );
            }
            let mut a = (0..16 * 12 * 4)
                .map(|i| ((i * 53 + 19) % 256) as u8)
                .collect::<Vec<_>>();
            let mut b = a.clone();
            old_handle
                .apply(&effect, Time(123), (16, 12), &mut a)
                .unwrap();
            new_handle
                .apply(&effect, Time(123), (16, 12), &mut b)
                .unwrap();
            assert_eq!(a, b, "{} at {value}", desc.effect_id);
        }
    }
}

#[test]
fn all_audio_filters_preserve_samples_and_state_across_abi_versions() {
    let (_cache, old) = legacy(include_bytes!("fixtures/audio-basic-0.2.2.amplugin"));
    let new = test_support::host();
    assert_eq!(old.audio_effects(), new.audio_effects());
    let old_handle = old.audio_effect_processor();
    let new_handle = new.audio_effect_processor();
    for desc in new.audio_effects() {
        let mut effect = desc.instance(EffectId(1));
        // Exercise the EQ too: its default is intentionally an exact identity.
        if desc.effect_id == "eq3" {
            effect.params.insert("mid_gain_db".into(), 6.0);
        }
        let mut old_state = Vec::new();
        let mut new_state = Vec::new();
        for index in 0..4 {
            let samples = (0..512)
                .map(|i| ((index * 512 + i) as f32 * 0.13).sin() * 0.8)
                .collect::<Vec<_>>();
            let block = AudioBlock {
                samples: &samples,
                channels: 2,
                sample_rate: 48_000,
                start: Time(index * 256 * 14_700),
            };
            let a = old_handle.process(&effect, &block, &old_state).unwrap();
            let b = new_handle.process(&effect, &block, &new_state).unwrap();
            assert_eq!(a.samples, b.samples, "{} block {index}", desc.effect_id);
            assert_eq!(a.state, b.state, "{} block {index}", desc.effect_id);
            old_state = a.state;
            new_state = b.state;
        }
    }
}

#[test]
fn hello_1_0_preserves_geometry_and_exercises_host_limits() {
    let (_cache, mut old) = legacy(include_bytes!("fixtures/hello-0.2.0.amplugin"));
    let mut new = test_support::host();
    let id = "com.example.hello";
    let plugin = new.get_plugin(id).unwrap();
    // The example is rebuilt from current WIT; the frozen 1.0 binary is covered by
    // `compatibility.rs`.
    assert_eq!(
        plugin.manifest.api_version,
        animathio_plugin_api::API_VERSION
    );
    assert!(
        plugin
            .wit_manifest
            .metadata
            .iter()
            .any(|(k, v)| k == "param.radius.unit" && v == "px")
    );
    for radius in [1.0, 32.0, 96.0, 512.0] {
        let params = BTreeMap::from([("radius".into(), radius.into())]);
        let a = old
            .render_clip(id, &params, &serde_json::Value::Null, 0, (320, 180))
            .unwrap();
        let b = new
            .render_clip(id, &params, &serde_json::Value::Null, 0, (320, 180))
            .unwrap();
        assert_eq!(a, b);
    }
}

#[test]
fn a_1_0_component_cannot_claim_the_legacy_abi() {
    let cache = tempfile::tempdir().unwrap();
    let mut host = PluginHost::new(cache.path()).unwrap();
    for built in test_support::packages_for_tests() {
        let mut package = built.clone();
        package.manifest.api_version = "0.2.2".into();
        assert!(host.load_plugin_package(package).is_err());
    }
    assert!(host.loaded_plugin_ids().is_empty());
    assert!(host.effects().is_empty());
    assert!(host.audio_effects().is_empty());
}

#[test]
fn audio_plugins_report_audio_effect_and_reject_nonfinite_input() {
    let host = test_support::host();
    let plugin = host.get_plugin("org.animathio.audio-basic").unwrap();
    assert_eq!(plugin.wit_manifest.kinds, ["audio-effect"]);
    let effect = EffectInstance::new(
        EffectId(1),
        "org.animathio.audio-basic",
        "gain",
        BTreeMap::new(),
    );
    let samples = [f32::NAN];
    let block = AudioBlock {
        samples: &samples,
        sample_rate: 48_000,
        channels: 1,
        start: Time(0),
    };
    assert!(
        host.audio_effect_processor()
            .process(&effect, &block, &[])
            .unwrap_err()
            .contains("non-finite")
    );
}
