//! Pixel-effect plugins: the registry, sandboxed execution and the renderer port.
//!
//! An effect plugin implements the `effect-plugin` world: `describe`, plus the
//! `frame-effects` interface (`list-effects`, `apply-frame`). The host compiles each
//! component once (disk-cached by [`PluginHost`](crate::PluginHost)), resolves its imports
//! once into an `InstancePre`, and instantiates a fresh, fuel- and memory-limited store for
//! every call. A frame is copied into the guest and the result copied back, so a plugin never
//! sees host memory and a trapped call leaves the caller's pixels untouched.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, RwLock};

use animathio_core::{EffectId, EffectInstance, EffectProcessor, Time};
use animathio_typeset::TypesetEngine;
use wasmtime::component::{Component, Linker};
use wasmtime::{Engine, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::WasiCtxBuilder;

use crate::animathio::plugin::types as wit_types;
use crate::compat::Pre as EffectPluginPre;
use crate::{EpochTicker, HostState, SandboxLimits, build_store};

/// Interactive calls get a 200 ms backstop (the epoch ticker advances every 10 ms).
const PREVIEW_DEADLINE_TICKS: u64 = 20;
/// Fixed memory budget of a pixel-effect call before the per-pixel allowance.
const EFFECT_BASE_MEMORY: usize = 64 * 1024 * 1024;
/// Memory allowance per frame pixel: the input copy, the output copy and ~40 bytes of
/// working planes for the heaviest first-party filter (blur).
const EFFECT_MEMORY_PER_PIXEL: usize = 48;

/// One numeric parameter of an effect, as declared by its plugin.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectParam {
    /// Parameter id, used as the key in `EffectInstance::params`.
    pub id: String,
    /// Human-readable label.
    pub label: String,
    /// Smallest sensible value (UI hint).
    pub min: f64,
    /// Largest sensible value (UI hint).
    pub max: f64,
    /// Default value for new instances.
    pub default: f64,
    /// UI step size.
    pub step: f64,
}

/// A filter offered by a loaded effect plugin.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectDescriptor {
    /// Providing plugin id, e.g. `org.animathio.effects-basic`.
    pub plugin_id: String,
    /// Filter id within the plugin, e.g. `grayscale`.
    pub effect_id: String,
    /// Menu name.
    pub name: String,
    /// One-line description.
    pub description: String,
    /// Parameters in display order.
    pub params: Vec<EffectParam>,
}

impl EffectDescriptor {
    /// Default parameter values for a new instance.
    #[must_use]
    pub fn default_params(&self) -> BTreeMap<String, f64> {
        self.params
            .iter()
            .map(|param| (param.id.clone(), param.default))
            .collect()
    }

    /// A new effect instance with default parameters.
    #[must_use]
    pub fn instance(&self, id: EffectId) -> EffectInstance {
        EffectInstance::new(
            id,
            self.plugin_id.clone(),
            self.effect_id.clone(),
            self.default_params(),
        )
    }
}

struct EffectPluginEntry {
    pre: EffectPluginPre,
    effects: Vec<EffectDescriptor>,
}

/// Registry plus sandbox policy; shared by the host and every [`EffectHandle`].
pub(crate) struct EffectRuntime {
    engine: Engine,
    linker: Arc<Linker<HostState>>,
    limits: SandboxLimits,
    typeset: Arc<TypesetEngine>,
    plugins: RwLock<BTreeMap<String, Arc<EffectPluginEntry>>>,
    /// Keeps the epoch ticker alive for as long as any handle can run plugin code.
    _epoch_ticker: Arc<EpochTicker>,
}

impl EffectRuntime {
    pub(crate) fn new(
        engine: Engine,
        linker: Arc<Linker<HostState>>,
        limits: SandboxLimits,
        typeset: Arc<TypesetEngine>,
        epoch_ticker: Arc<EpochTicker>,
    ) -> Self {
        Self {
            engine,
            linker,
            limits,
            typeset,
            plugins: RwLock::new(BTreeMap::new()),
            _epoch_ticker: epoch_ticker,
        }
    }

    /// Instantiate `component` once to `describe` it and list its filters, then register it.
    pub(crate) fn register(
        &self,
        package: &animathio_plugin_api::PluginManifestToml,
        component: &Component,
        store: &mut Store<HostState>,
        legacy: bool,
    ) -> Result<wit_types::Manifest, String> {
        let plugin_id = package.id.as_str();
        let pre = EffectPluginPre::new(
            self.linker
                .instantiate_pre(component)
                .map_err(|e| e.to_string())?,
            legacy,
        );
        let instance = pre.instantiate(&mut *store).map_err(|e| e.to_string())?;
        let manifest = instance
            .describe(&mut *store, false)
            .map_err(|e| e.to_string())?;
        crate::validate_manifest(package, &manifest).map_err(|e| e.to_string())?;
        let listed = instance
            .list_effects(&mut *store)
            .map_err(|e| e.to_string())?;
        if listed.len() > 64 {
            return Err("too many effects (maximum 64)".into());
        }
        let effects = listed
            .into_iter()
            .map(|descriptor| convert_descriptor(plugin_id, descriptor))
            .collect::<Result<Vec<_>, _>>()?;
        let mut seen = BTreeSet::new();
        for effect in &effects {
            if !seen.insert(effect.effect_id.as_str()) {
                return Err(format!("duplicate effect id {:?}", effect.effect_id));
            }
        }
        self.plugins.write().expect("effect registry lock").insert(
            plugin_id.to_owned(),
            Arc::new(EffectPluginEntry { pre, effects }),
        );
        Ok(manifest)
    }

    pub(crate) fn descriptors(&self) -> Vec<EffectDescriptor> {
        self.plugins
            .read()
            .expect("effect registry lock")
            .values()
            .flat_map(|entry| entry.effects.iter().cloned())
            .collect()
    }

    fn store(&self, pixels: usize) -> Store<HostState> {
        let memory = (EFFECT_BASE_MEMORY + EFFECT_MEMORY_PER_PIXEL * pixels)
            .min(self.limits.effect_memory_bytes);
        let limiter: StoreLimits = StoreLimitsBuilder::new()
            .memory_size(memory)
            .table_elements(self.limits.max_table_elements)
            .instances(8)
            .memories(1)
            .tables(2)
            .trap_on_grow_failure(true)
            .build();
        let fuel = self.limits.fuel_per_call.saturating_add(
            self.limits
                .effect_fuel_per_pixel
                .saturating_mul(pixels as u64),
        );
        build_store(
            &self.engine,
            WasiCtxBuilder::new().build(),
            Vec::new(),
            Arc::clone(&self.typeset),
            limiter,
            (fuel, self.limits.effect_epoch_deadline_ticks),
            self.limits,
        )
    }

    fn apply(
        &self,
        effect: &EffectInstance,
        time: Time,
        size: (u32, u32),
        pixels: &mut Vec<u8>,
        preview: bool,
    ) -> Result<(), String> {
        let (width, height) = size;
        let count = (width as usize)
            .checked_mul(height as usize)
            .filter(|count| *count > 0)
            .ok_or_else(|| "frame must have at least one pixel".to_owned())?;
        if count > self.limits.max_effect_pixels {
            return Err(format!(
                "frame of {count} pixels exceeds the effect limit of {}",
                self.limits.max_effect_pixels
            ));
        }
        if pixels.len() != count * 4 {
            return Err("pixel buffer does not match the frame size".to_owned());
        }
        let entry = self
            .plugins
            .read()
            .expect("effect registry lock")
            .get(&effect.plugin)
            .cloned()
            .ok_or_else(|| format!("effect plugin {:?} is not loaded", effect.plugin))?;
        if !entry.effects.iter().any(|e| e.effect_id == effect.effect) {
            return Err(format!(
                "plugin {:?} has no effect {:?}",
                effect.plugin, effect.effect
            ));
        }
        // A stored value outside the declared range (hand-edited or old project) is
        // clamped instead of failing every frame of the clip.
        let descriptor = entry
            .effects
            .iter()
            .find(|e| e.effect_id == effect.effect)
            .expect("effect presence checked above");
        let clamped = descriptor.clamped_params(&effect.params);
        if clamped != effect.params {
            log::warn!(
                "effect {}/{} has out-of-range parameters {:?}; clamped to {:?}",
                effect.plugin,
                effect.effect,
                effect.params,
                clamped
            );
        }
        let params: Vec<_> = clamped
            .iter()
            .map(|(id, value)| wit_types::ParamEntry {
                id: id.clone(),
                value: wit_types::ParamValue::Number(*value),
            })
            .collect();

        let _timer = animathio_core::timing::Timer::new("effects");
        let setup = animathio_core::timing::Timer::new("effects-setup");
        let mut store = self.store(count);
        if preview {
            store.set_epoch_deadline(
                PREVIEW_DEADLINE_TICKS.min(self.limits.effect_epoch_deadline_ticks),
            );
        }
        let instance = entry
            .pre
            .instantiate(&mut store)
            .map_err(|e| format!("instantiating {:?}: {e}", effect.plugin))?;
        drop(setup);
        let output = instance
            .apply_frame(
                &mut store,
                &effect.effect,
                pixels,
                wit_types::Size { width, height },
                &params,
                time.0,
            )
            .map_err(|e| format!("effect {}/{} trapped: {e}", effect.plugin, effect.effect))?
            .map_err(|e| {
                format!(
                    "effect {}/{} failed: {}: {}",
                    effect.plugin,
                    effect.effect,
                    crate::truncate_utf8(&e.code, 64),
                    crate::truncate_utf8(&e.message, 256)
                )
            })?;
        if output.len() != pixels.len() {
            return Err(format!(
                "effect {}/{} returned {} bytes for a {}-byte frame",
                effect.plugin,
                effect.effect,
                output.len(),
                pixels.len()
            ));
        }
        *pixels = output;
        Ok(())
    }
}

fn convert_descriptor(
    plugin_id: &str,
    descriptor: crate::effect_world::exports::animathio::plugin::frame_effects::EffectDescriptor,
) -> Result<EffectDescriptor, String> {
    build_descriptor(
        plugin_id,
        descriptor.id,
        descriptor.name,
        descriptor.description,
        descriptor.params,
    )
}

/// Validate a plugin-declared descriptor (id syntax, numeric parameters, finite ranges and
/// defaults inside them) and convert it to the host's plain-data form. Shared by pixel and
/// audio effects, which declare their filters identically.
pub(crate) fn build_descriptor(
    plugin_id: &str,
    id: String,
    name: String,
    description: String,
    specs: Vec<wit_types::ParamSpec>,
) -> Result<EffectDescriptor, String> {
    if specs.len() > 32 {
        return Err("too many effect parameters (maximum 32)".into());
    }
    let valid_id = |id: &str| {
        !id.is_empty()
            && id.len() <= 64
            && id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_'))
    };
    if !valid_id(&id) || name.trim().is_empty() {
        return Err(format!(
            "invalid effect id {:?}",
            crate::truncate_utf8(&id, 256)
        ));
    }
    let mut params = Vec::new();
    let mut seen = BTreeSet::new();
    for spec in specs {
        let wit_types::ParamKind::Number(range) = spec.kind else {
            return Err(format!(
                "effect {id} parameter {:?} must be a number",
                crate::truncate_utf8(&spec.id, 256)
            ));
        };
        let finite = [range.min, range.max, range.default, range.step]
            .iter()
            .all(|v| v.is_finite());
        if !valid_id(&spec.id)
            || spec.label.trim().is_empty()
            || !seen.insert(spec.id.clone())
            || !finite
            || range.step <= 0.0
            || range.min > range.max
            || !(range.min..=range.max).contains(&range.default)
        {
            return Err(format!(
                "effect {id} has an invalid parameter {:?}",
                crate::truncate_utf8(&spec.id, 256)
            ));
        }
        params.push(EffectParam {
            id: spec.id,
            label: crate::truncate_utf8(&spec.label, 256),
            min: range.min,
            max: range.max,
            default: range.default,
            step: range.step,
        });
    }
    Ok(EffectDescriptor {
        plugin_id: plugin_id.to_owned(),
        effect_id: id,
        name: crate::truncate_utf8(&name, 256),
        description: crate::truncate_utf8(&description, 256),
        params,
    })
}

/// A cloneable handle for running effect plugins from a renderer.
///
/// It implements [`EffectProcessor`], so `RenderCtx::set_effect_processor` can take it
/// directly. All handles from one host share its plugin registry and sandbox limits.
#[derive(Clone)]
pub struct EffectHandle {
    runtime: Arc<EffectRuntime>,
    preview: bool,
}

impl EffectHandle {
    pub(crate) fn new(runtime: Arc<EffectRuntime>) -> Self {
        Self {
            runtime,
            preview: false,
        }
    }

    /// Use a 200 ms wall-clock backstop for interactive preview calls.
    #[must_use]
    pub fn for_preview(mut self) -> Self {
        self.preview = true;
        self
    }

    /// Filters currently offered by loaded effect plugins.
    #[must_use]
    pub fn effects(&self) -> Vec<EffectDescriptor> {
        self.runtime.descriptors()
    }
}

impl EffectProcessor for EffectHandle {
    fn apply(
        &mut self,
        effect: &EffectInstance,
        time: Time,
        size: (u32, u32),
        pixels: &mut Vec<u8>,
    ) -> Result<(), String> {
        self.runtime.apply(effect, time, size, pixels, self.preview)
    }
}

#[cfg(test)]
mod boundary_tests {
    use super::*;

    #[test]
    fn guest_display_metadata_is_bounded_at_utf8_boundaries() {
        let descriptor = build_descriptor(
            "test",
            "test".into(),
            format!("a{}", "é".repeat(1000)),
            format!("b{}", "🙂".repeat(1000)),
            vec![],
        )
        .unwrap();
        assert_eq!(descriptor.name.len(), 255);
        assert_eq!(descriptor.description.len(), 253);
    }

    #[test]
    fn parameter_count_and_identifiers_are_bounded() {
        let spec = wit_types::ParamSpec {
            id: "a".into(),
            label: "a".into(),
            kind: wit_types::ParamKind::Bool(wit_types::BoolParam { default: false }),
        };
        assert!(
            build_descriptor(
                "test",
                "test".into(),
                "test".into(),
                String::new(),
                vec![spec; 33]
            )
            .unwrap_err()
            .contains("too many")
        );
        assert!(
            build_descriptor(
                "test",
                "x".repeat(10000),
                String::new(),
                String::new(),
                vec![]
            )
            .unwrap_err()
            .len()
                < 300
        );
    }

    #[test]
    fn effect_capacity_covers_every_valid_project_resolution() {
        assert!(PREVIEW_DEADLINE_TICKS < SandboxLimits::default().effect_epoch_deadline_ticks);
        assert_eq!(SandboxLimits::default().max_effect_pixels, 32 * 1024 * 1024);
    }
}
