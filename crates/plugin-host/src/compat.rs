//! Typed boundary between the frozen 0.2.x components and ABI 1.0.
//!
//! Payload layouts are unchanged. Only `describe` needs translation: the old
//! closed kind enum becomes string ids, and advisory metadata defaults to empty.
//! Never rewrite a component's version bytes or relax Wasmtime's type checks.

use wasmtime::Store;
use wasmtime::component::{ComponentType, Instance, InstancePre, Lift, Linker};

use crate::{HostState, wit_types};

use crate::audio_world::exports::animathio::plugin::audio_effects as audio;
use crate::effect_world::exports::animathio::plugin::frame_effects as frame;

#[derive(ComponentType, Lift)]
#[allow(dead_code)] // Constructed by the derived canonical-ABI lifter.
#[component(enum)]
#[repr(u8)]
enum LegacyKind {
    #[component(name = "clip-source")]
    ClipSource,
    #[component(name = "effect")]
    Effect,
    #[component(name = "animation")]
    Animation,
}

#[derive(ComponentType, Lift)]
#[component(record)]
struct LegacyManifest {
    name: String,
    version: String,
    description: String,
    kinds: Vec<LegacyKind>,
    params: Vec<wit_types::ParamSpec>,
}

/// Register the old host import namespace. Wasmtime's 0.2 semver matching also
/// satisfies the identical imports in 0.2.0 and 0.2.1 components.
pub(crate) fn add_to_linker(linker: &mut Linker<HostState>) -> wasmtime::Result<()> {
    let mut host = linker.instance("animathio:plugin/host@0.2.2")?;
    host.func_wrap("shape-text", |mut caller, (text, size): (String, f32)| {
        Ok((caller.data_mut().legacy_shape_text(text, size),))
    })?;
    host.func_wrap("svg-to-paths", |mut caller, (xml,): (String,)| {
        Ok((caller.data_mut().legacy_svg_to_paths(xml),))
    })?;
    host.func_wrap(
        "typeset-math",
        |mut caller, (text, mode, size): (String, wit_types::MathMode, f32)| {
            Ok((caller.data_mut().legacy_typeset_math(text, mode, size),))
        },
    )?;
    host.func_wrap("log", |mut caller, (message,): (String,)| {
        caller.data_mut().legacy_log(message);
        Ok(())
    })?;
    Ok(())
}

/// Resolved imports plus the ABI family selected by the validated package.
#[derive(Clone)]
pub(crate) struct Pre {
    pre: InstancePre<HostState>,
    legacy: bool,
}

impl Pre {
    pub(crate) fn new(pre: InstancePre<HostState>, legacy: bool) -> Self {
        Self { pre, legacy }
    }

    pub(crate) fn instantiate(&self, store: &mut Store<HostState>) -> wasmtime::Result<Bindings> {
        Ok(Bindings {
            instance: self.pre.instantiate(store)?,
            legacy: self.legacy,
        })
    }
}

/// All calls use Wasmtime's checked canonical ABI with automatic post-return cleanup.
/// The common payload records retain the exact 0.2.x structural layouts.
pub(crate) struct Bindings {
    instance: Instance,
    legacy: bool,
}

impl Bindings {
    pub(crate) fn describe(
        &self,
        store: &mut Store<HostState>,
        audio_plugin: bool,
    ) -> wasmtime::Result<wit_types::Manifest> {
        if !self.legacy {
            let func = self
                .instance
                .get_typed_func::<(), (wit_types::Manifest,)>(&mut *store, "describe")?;
            let (manifest,) = func.call(&mut *store, ())?;
            return Ok(manifest);
        }
        let func = self
            .instance
            .get_typed_func::<(), (LegacyManifest,)>(&mut *store, "describe")?;
        let (old,) = func.call(&mut *store, ())?;
        Ok(wit_types::Manifest {
            name: old.name,
            version: old.version,
            description: old.description,
            kinds: old
                .kinds
                .into_iter()
                .map(|kind| {
                    match kind {
                        LegacyKind::ClipSource => "clip-source",
                        LegacyKind::Effect if audio_plugin => "audio-effect",
                        LegacyKind::Effect => "effect",
                        LegacyKind::Animation => "animation",
                    }
                    .to_owned()
                })
                .collect(),
            metadata: Vec::new(),
            params: old.params,
        })
    }

    /// Check the source contract; only legacy components require the unused hooks.
    pub(crate) fn validate_source(&self, store: &mut Store<HostState>) -> wasmtime::Result<()> {
        if self.legacy {
            self.instance.get_typed_func::<(Vec<wit_types::ParamEntry>, i64, wit_types::Size), (wit_types::Scene,)>(&mut *store, "render-clip")?;
        } else {
            self.instance.get_typed_func::<(Vec<wit_types::ParamEntry>, i64, wit_types::Size), (Result<wit_types::Scene, wit_types::Diagnostic>,)>(&mut *store, "render-clip")?;
        }
        if self.legacy {
            self.instance.get_typed_func::<(
                wit_types::Scene,
                Vec<wit_types::ParamEntry>,
                i64,
                wit_types::Size,
            ), (wit_types::Scene,)>(&mut *store, "apply-effect")?;
            self.instance
                .get_typed_func::<(i64, i64, Vec<wit_types::ParamEntry>), (f64,)>(
                    &mut *store,
                    "evaluate-curve",
                )?;
        }
        Ok(())
    }

    pub(crate) fn render(
        &self,
        store: &mut Store<HostState>,
        params: &[wit_types::ParamEntry],
        time: i64,
        size: wit_types::Size,
    ) -> wasmtime::Result<Result<wit_types::Scene, wit_types::Diagnostic>> {
        if !self.legacy {
            let func = self.instance.get_typed_func::<(&[wit_types::ParamEntry], i64, wit_types::Size), (Result<wit_types::Scene, wit_types::Diagnostic>,)>(&mut *store, "render-clip")?;
            return Ok(func.call(store, (params, time, size))?.0);
        }
        let func = self.instance.get_typed_func::<(&[wit_types::ParamEntry], i64, wit_types::Size), (wit_types::Scene,)>(&mut *store, "render-clip")?;
        let (scene,) = func.call(&mut *store, (params, time, size))?;
        Ok(Ok(scene))
    }

    fn export(
        &self,
        store: &mut Store<HostState>,
        interface: &str,
        function: &str,
    ) -> wasmtime::Result<wasmtime::component::ComponentExportIndex> {
        let version = if self.legacy { "0.2.2" } else { "1.0.0" };
        let name = format!("animathio:plugin/{interface}@{version}");
        let parent = self
            .instance
            .get_export_index(&mut *store, None, &name)
            .ok_or_else(|| wasmtime::Error::msg(format!("missing interface {name}")))?;
        self.instance
            .get_export_index(store, Some(&parent), function)
            .ok_or_else(|| wasmtime::Error::msg(format!("missing export {name}/{function}")))
    }

    pub(crate) fn list_effects(
        &self,
        store: &mut Store<HostState>,
    ) -> wasmtime::Result<Vec<frame::EffectDescriptor>> {
        let apply = self.export(store, "frame-effects", "apply-frame")?;
        if self.legacy {
            self.instance.get_typed_func::<(&str, &[u8], wit_types::Size, &[wit_types::ParamEntry], i64), (Result<Vec<u8>, String>,)>(&mut *store, &apply)?;
        } else {
            self.instance.get_typed_func::<(&str, &[u8], wit_types::Size, &[wit_types::ParamEntry], i64), (Result<Vec<u8>, wit_types::Diagnostic>,)>(&mut *store, &apply)?;
        }
        let export = self.export(store, "frame-effects", "list-effects")?;
        let func = self
            .instance
            .get_typed_func::<(), (Vec<frame::EffectDescriptor>,)>(&mut *store, &export)?;
        let (listed,) = func.call(&mut *store, ())?;
        Ok(listed)
    }

    pub(crate) fn apply_frame(
        &self,
        store: &mut Store<HostState>,
        effect: &str,
        pixels: &[u8],
        size: wit_types::Size,
        params: &[wit_types::ParamEntry],
        time: i64,
    ) -> wasmtime::Result<Result<Vec<u8>, wit_types::Diagnostic>> {
        let export = self.export(store, "frame-effects", "apply-frame")?;
        if !self.legacy {
            let func = self.instance.get_typed_func::<(
                &str,
                &[u8],
                wit_types::Size,
                &[wit_types::ParamEntry],
                i64,
            ), (Result<Vec<u8>, wit_types::Diagnostic>,)>(
                &mut *store, &export
            )?;
            return Ok(func.call(store, (effect, pixels, size, params, time))?.0);
        }
        let func = self.instance.get_typed_func::<(&str, &[u8], wit_types::Size, &[wit_types::ParamEntry], i64), (Result<Vec<u8>, String>,)>(&mut *store, &export)?;
        let (output,) = func.call(&mut *store, (effect, pixels, size, params, time))?;
        Ok(output.map_err(|message| crate::diagnostic("plugin-error", message)))
    }

    pub(crate) fn list_audio_effects(
        &self,
        store: &mut Store<HostState>,
    ) -> wasmtime::Result<Vec<audio::AudioEffectDescriptor>> {
        let process = self.export(store, "audio-effects", "process-block")?;
        if self.legacy {
            self.instance.get_typed_func::<(
                &str,
                &audio::AudioBlock,
                &[wit_types::ParamEntry],
                Option<&[u8]>,
            ), (Result<audio::BlockOutput, String>,)>(
                &mut *store, &process
            )?;
        } else {
            self.instance.get_typed_func::<(
                &str,
                &audio::AudioBlock,
                &[wit_types::ParamEntry],
                Option<&[u8]>,
            ), (Result<audio::BlockOutput, wit_types::Diagnostic>,)>(
                &mut *store, &process
            )?;
        }
        let export = self.export(store, "audio-effects", "list-audio-effects")?;
        let func = self
            .instance
            .get_typed_func::<(), (Vec<audio::AudioEffectDescriptor>,)>(&mut *store, &export)?;
        let (listed,) = func.call(&mut *store, ())?;
        Ok(listed)
    }

    pub(crate) fn process_block(
        &self,
        store: &mut Store<HostState>,
        effect: &str,
        block: &audio::AudioBlock,
        params: &[wit_types::ParamEntry],
        state: Option<&[u8]>,
    ) -> wasmtime::Result<Result<audio::BlockOutput, wit_types::Diagnostic>> {
        let export = self.export(store, "audio-effects", "process-block")?;
        if !self.legacy {
            let func = self.instance.get_typed_func::<(
                &str,
                &audio::AudioBlock,
                &[wit_types::ParamEntry],
                Option<&[u8]>,
            ), (Result<audio::BlockOutput, wit_types::Diagnostic>,)>(
                &mut *store, &export
            )?;
            return Ok(func.call(store, (effect, block, params, state))?.0);
        }
        let func = self.instance.get_typed_func::<(
            &str,
            &audio::AudioBlock,
            &[wit_types::ParamEntry],
            Option<&[u8]>,
        ), (Result<audio::BlockOutput, String>,)>(
            &mut *store, &export
        )?;
        let (output,) = func.call(&mut *store, (effect, block, params, state))?;
        Ok(output.map_err(|message| crate::diagnostic("plugin-error", message)))
    }
}
