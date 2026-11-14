//! Immutable pre-1.0 binaries must work without rebuilding against current WIT.
use std::collections::BTreeMap;

use animathio_core::{
    AudioBlock, AudioEffectProcessor, EffectId, EffectInstance, EffectProcessor, Time,
};
use animathio_plugin_api::PluginPackage;
use animathio_plugin_host::PluginHost;

const HELLO: &[u8] = include_bytes!("fixtures/hello-0.2.0.amplugin");
const FRAME: &[u8] = include_bytes!("fixtures/effects-basic-0.2.1.amplugin");
const AUDIO: &[u8] = include_bytes!("fixtures/audio-basic-0.2.2.amplugin");

fn host(bytes: &[u8]) -> (tempfile::TempDir, PluginHost, String) {
    let cache = tempfile::tempdir().unwrap();
    let mut host = PluginHost::new(cache.path()).unwrap();
    let id = host
        .load_plugin_package(PluginPackage::from_bytes(bytes).unwrap())
        .unwrap();
    (cache, host, id)
}

#[test]
fn legacy_source_preserves_geometry_and_parameter_schema() {
    let (_cache, mut host, id) = host(HELLO);
    assert_eq!(
        host.get_plugin(&id).unwrap().wit_manifest.kinds,
        ["clip-source"]
    );
    let schema = host.get_inspector_schema(&id).unwrap();
    assert_eq!(
        schema
            .params
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        ["radius", "color"]
    );
    let render = |host: &mut PluginHost, radius: f64| {
        host.render_clip(
            &id,
            &BTreeMap::from([("radius".into(), radius.into())]),
            &serde_json::json!({}),
            705_600_000,
            (320, 180),
        )
        .unwrap()
    };
    let a = render(&mut host, 20.0);
    let b = render(&mut host, 40.0);
    assert_eq!(a.elements.len(), 1);
    assert_ne!(a, b);
    let animathio_core::VectorElement::FillPath { path, color } = &a.elements[0] else {
        panic!("circle fill")
    };
    assert_eq!(*color, animathio_core::Color::rgba(90, 170, 255, 255));
    assert!(matches!(path[0], animathio_core::PathVerb::MoveTo(p) if p.x == 180.0 && p.y == 90.0));
}

#[test]
fn legacy_frame_effect_keeps_exact_pixels() {
    let (_cache, host, id) = host(FRAME);
    assert_eq!(host.effects().len(), 8);
    let effect = EffectInstance::new(
        EffectId(0),
        id,
        "invert",
        BTreeMap::from([("amount".into(), 1.0)]),
    );
    let mut pixels = vec![10, 20, 30, 255, 0, 255, 128, 127];
    host.effect_processor()
        .apply(&effect, Time(42), (2, 1), &mut pixels)
        .unwrap();
    assert_eq!(pixels, [245, 235, 225, 255, 255, 0, 127, 127]);
}

#[test]
fn legacy_audio_kind_is_corrected_and_state_carry_is_unchanged() {
    let (_cache, host, id) = host(AUDIO);
    assert_eq!(
        host.get_plugin(&id).unwrap().wit_manifest.kinds,
        ["audio-effect"]
    );
    assert_eq!(host.audio_effects().len(), 6);
    assert!(host.effects().is_empty());
    let effect = EffectInstance::new(EffectId(0), id, "highpass", BTreeMap::new());
    let input = (0..128).map(|i| (i as f32 * 0.1).sin()).collect::<Vec<_>>();
    let processor = host.audio_effect_processor();
    let block = |samples| AudioBlock {
        samples,
        channels: 1,
        sample_rate: 48_000,
        start: Time(0),
    };
    let whole = processor.process(&effect, &block(&input), &[]).unwrap();
    let a = processor
        .process(&effect, &block(&input[..64]), &[])
        .unwrap();
    assert!(!a.state.is_empty());
    let b = processor
        .process(&effect, &block(&input[64..]), &a.state)
        .unwrap();
    assert_eq!([a.samples, b.samples].concat(), whole.samples);
    assert_eq!(b.state, whole.state);
    assert_ne!(whole.samples, input);
}

#[test]
fn manifest_cannot_relabel_an_old_component_as_1_0() {
    let cache = tempfile::tempdir().unwrap();
    let mut host = PluginHost::new(cache.path()).unwrap();
    for bytes in [HELLO, FRAME, AUDIO] {
        let mut package = PluginPackage::from_bytes(bytes).unwrap();
        package.manifest.api_version = "1.0.0".into();
        assert!(host.load_plugin_package(package).is_err());
    }
    assert!(host.loaded_plugin_ids().is_empty());
    assert!(host.effects().is_empty());
    assert!(host.audio_effects().is_empty());
}

#[test]
fn frozen_1_0_source_contract_still_loads_with_advisory_kind_and_limits() {
    let (_cache, mut host, id) = host(include_bytes!("fixtures/hello-1.0.0.amplugin"));
    let plugin = host.get_plugin(&id).unwrap();
    assert_eq!(
        plugin.wit_manifest.kinds,
        ["clip-source", "com.example.circle"]
    );
    assert!(
        plugin
            .wit_manifest
            .metadata
            .iter()
            .any(|(k, v)| k == "param.radius.unit" && v == "px")
    );
    let scene = host
        .render_clip(
            &id,
            &BTreeMap::new(),
            &serde_json::Value::Null,
            0,
            (320, 180),
        )
        .unwrap();
    assert_eq!(scene.elements.len(), 1);
}
