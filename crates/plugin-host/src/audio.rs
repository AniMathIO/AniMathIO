//! Audio-effect plugins: the registry, sandboxed block processing and the mixer port.
//!
//! An audio-effect plugin implements the `audio-effect-plugin` world: `describe`, plus the
//! `audio-effects` interface (`list-audio-effects`, `process-block`). The host compiles each
//! component once (disk-cached by [`PluginHost`](crate::PluginHost)) and resolves its
//! imports once into an `InstancePre`. Audio is processed in many short blocks, so unlike
//! pixel effects the instantiated stores are **pooled**: a finished call returns its warm
//! instance to the plugin's pool and the next call reuses it after a fuel/epoch/memory
//! refill. A trapped call discards its instance, so a misbehaving plugin cannot poison later
//! calls. Plugin state never lives in the guest between calls: it is an opaque blob that the
//! host carries from block to block, which keeps every call a pure function of its
//! arguments and lets the mixer cache and recompute blocks deterministically.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, RwLock};

use animathio_core::{AudioBlock, AudioBlockOutput, AudioEffectProcessor, EffectInstance};
use animathio_typeset::TypesetEngine;
use wasmtime::component::{Component, Linker};
use wasmtime::{Engine, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::WasiCtxBuilder;

use crate::animathio::plugin::types as wit_types;
use crate::audio_world::exports::animathio::plugin::audio_effects as wit_audio;
use crate::compat::{Bindings as AudioEffectPlugin, Pre as AudioEffectPluginPre};
use crate::effects::build_descriptor;
use crate::{EffectDescriptor, EpochTicker, HostState, SandboxLimits, build_store};

/// Fixed memory budget of an audio-effect call before the per-sample allowance.
const AUDIO_BASE_MEMORY: usize = 16 * 1024 * 1024;
/// Memory allowance per interleaved sample: the input and output copies on both sides of
/// the component boundary plus scratch in the heaviest first-party filter.
const AUDIO_MEMORY_PER_SAMPLE: usize = 64;
/// Most idle warm instances kept per plugin.
const POOL_SIZE: usize = 8;
/// Highest sample rate accepted by the mixer.
pub(crate) const MAX_SAMPLE_RATE: u32 = 768_000;
/// Highest channel count accepted by the mixer.
pub(crate) const MAX_CHANNELS: u16 = 32;

struct Pooled {
    store: Store<HostState>,
    instance: AudioEffectPlugin,
}

struct AudioPluginEntry {
    pre: AudioEffectPluginPre,
    effects: Vec<EffectDescriptor>,
    pool: Mutex<Vec<Pooled>>,
}

/// Registry plus sandbox policy; shared by the host and every [`AudioEffectHandle`].
pub(crate) struct AudioRuntime {
    engine: Engine,
    linker: Arc<Linker<HostState>>,
    limits: SandboxLimits,
    typeset: Arc<TypesetEngine>,
    plugins: RwLock<BTreeMap<String, Arc<AudioPluginEntry>>>,
    /// Keeps the epoch ticker alive for as long as any handle can run plugin code.
    _epoch_ticker: Arc<EpochTicker>,
}

impl AudioRuntime {
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
        let pre = AudioEffectPluginPre::new(
            self.linker
                .instantiate_pre(component)
                .map_err(|e| e.to_string())?,
            legacy,
        );
        let instance = pre.instantiate(&mut *store).map_err(|e| e.to_string())?;
        let manifest = instance
            .describe(&mut *store, true)
            .map_err(|e| e.to_string())?;
        crate::validate_manifest(package, &manifest).map_err(|e| e.to_string())?;
        let listed = instance
            .list_audio_effects(&mut *store)
            .map_err(|e| e.to_string())?;
        if listed.len() > crate::MAX_EFFECTS_PER_PLUGIN {
            return Err(format!(
                "too many audio effects (maximum {})",
                crate::MAX_EFFECTS_PER_PLUGIN
            ));
        }
        let effects = listed
            .into_iter()
            .map(|d| build_descriptor(plugin_id, d.id, d.name, d.description, d.params))
            .collect::<Result<Vec<_>, _>>()?;
        let mut seen = BTreeSet::new();
        for effect in &effects {
            if !seen.insert(effect.effect_id.as_str()) {
                return Err(format!("duplicate audio effect id {:?}", effect.effect_id));
            }
        }
        self.plugins.write().expect("audio registry lock").insert(
            plugin_id.to_owned(),
            Arc::new(AudioPluginEntry {
                pre,
                effects,
                pool: Mutex::new(Vec::new()),
            }),
        );
        Ok(manifest)
    }

    pub(crate) fn descriptors(&self) -> Vec<EffectDescriptor> {
        self.plugins
            .read()
            .expect("audio registry lock")
            .values()
            .flat_map(|entry| entry.effects.iter().cloned())
            .collect()
    }

    fn limiter(&self, samples: usize) -> StoreLimits {
        let memory = (AUDIO_BASE_MEMORY + AUDIO_MEMORY_PER_SAMPLE * samples)
            .min(self.limits.audio_memory_bytes);
        StoreLimitsBuilder::new()
            .memory_size(memory)
            .table_elements(self.limits.max_table_elements)
            .instances(8)
            .memories(1)
            .tables(2)
            .trap_on_grow_failure(true)
            .build()
    }

    fn fuel(&self, samples: usize) -> u64 {
        self.limits.fuel_per_call.saturating_add(
            self.limits
                .audio_fuel_per_sample
                .saturating_mul(samples as u64),
        )
    }

    fn checkout(&self, entry: &AudioPluginEntry, samples: usize) -> Result<Pooled, String> {
        let pooled = entry.pool.lock().expect("audio pool lock").pop();
        let mut pooled = if let Some(pooled) = pooled {
            pooled
        } else {
            let mut store = build_store(
                &self.engine,
                WasiCtxBuilder::new().build(),
                Vec::new(),
                Arc::clone(&self.typeset),
                self.limiter(samples),
                (self.fuel(samples), self.limits.audio_epoch_deadline_ticks),
                self.limits,
            );
            let instance = entry
                .pre
                .instantiate(&mut store)
                .map_err(|e| format!("instantiating audio effect plugin: {e}"))?;
            Pooled { store, instance }
        };
        // Refill the per-call budgets; a reused store keeps only its warm memory.
        pooled.store.data_mut().limiter = self.limiter(samples);
        pooled.store.data_mut().logs.clear();
        let _ = pooled.store.set_fuel(self.fuel(samples));
        pooled
            .store
            .set_epoch_deadline(self.limits.audio_epoch_deadline_ticks);
        Ok(pooled)
    }

    fn process(
        &self,
        effect: &EffectInstance,
        block: &AudioBlock<'_>,
        state: &[u8],
    ) -> Result<AudioBlockOutput, String> {
        let channels = usize::from(block.channels);
        if block.channels == 0
            || block.channels > MAX_CHANNELS
            || block.sample_rate == 0
            || block.sample_rate > MAX_SAMPLE_RATE
        {
            return Err("audio block has an unsupported rate or channel count".to_owned());
        }
        if block.samples.is_empty() || !block.samples.len().is_multiple_of(channels) {
            return Err("audio block must hold a whole, nonzero number of frames".to_owned());
        }
        let frames = block.samples.len() / channels;
        if frames > self.limits.max_audio_block_frames {
            return Err(format!(
                "audio block of {frames} frames exceeds the limit of {}",
                self.limits.max_audio_block_frames
            ));
        }
        if block.samples.iter().any(|sample| !sample.is_finite()) {
            return Err("audio block contains non-finite samples".to_owned());
        }
        if state.len() > self.limits.max_audio_state_bytes {
            return Err("audio effect state exceeds the size limit".to_owned());
        }
        let entry = self
            .plugins
            .read()
            .expect("audio registry lock")
            .get(&effect.plugin)
            .cloned()
            .ok_or_else(|| format!("audio effect plugin {:?} is not loaded", effect.plugin))?;
        if !entry.effects.iter().any(|e| e.effect_id == effect.effect) {
            return Err(format!(
                "plugin {:?} has no audio effect {:?}",
                effect.plugin, effect.effect
            ));
        }
        let params: Vec<_> = effect
            .params
            .iter()
            .map(|(id, value)| wit_types::ParamEntry {
                id: id.clone(),
                value: wit_types::ParamValue::Number(*value),
            })
            .collect();
        let wit_block = wit_audio::AudioBlock {
            sample_rate: block.sample_rate,
            channels: block.channels,
            start_ticks: block.start.0,
            samples: block.samples.to_vec(),
        };
        let wit_state = (!state.is_empty()).then(|| state.to_vec());

        let mut pooled = self.checkout(&entry, block.samples.len())?;
        let label = format!("{}/{}", effect.plugin, effect.effect);
        let called = pooled
            .instance
            .process_block(
                &mut pooled.store,
                &effect.effect,
                &wit_block,
                &params,
                wit_state.as_deref(),
            )
            .map_err(|e| format!("audio effect {label} trapped: {e}"))?
            .map_err(|e| {
                format!(
                    "audio effect {label} failed: {}: {}",
                    crate::truncate_utf8(&e.code, 64),
                    crate::truncate_utf8(&e.message, 256)
                )
            })?;
        let samples = called.samples;
        let new_state = called.state.unwrap_or_default();
        if samples.len() != block.samples.len() {
            return Err(format!(
                "audio effect {label} returned {} samples for a {}-sample block",
                samples.len(),
                block.samples.len()
            ));
        }
        if new_state.len() > self.limits.max_audio_state_bytes {
            return Err(format!("audio effect {label} returned an oversized state"));
        }
        if samples.iter().any(|x| !x.is_finite()) {
            return Err(format!("audio effect {label} produced non-finite samples"));
        }
        let mut pool = entry.pool.lock().expect("audio pool lock");
        if pool.len() < POOL_SIZE {
            pool.push(pooled);
        }
        Ok(AudioBlockOutput {
            samples,
            state: new_state,
        })
    }
}

/// A cloneable handle for running audio-effect plugins from the mixer.
///
/// It implements [`AudioEffectProcessor`], so `Mixer::with_effect_processor` can take it
/// (wrapped in an `Arc`) directly. All handles from one host share its plugin registry,
/// warm-instance pools and sandbox limits.
#[derive(Clone)]
pub struct AudioEffectHandle {
    runtime: Arc<AudioRuntime>,
}

impl AudioEffectHandle {
    pub(crate) fn new(runtime: Arc<AudioRuntime>) -> Self {
        Self { runtime }
    }

    /// Filters currently offered by loaded audio-effect plugins.
    #[must_use]
    pub fn effects(&self) -> Vec<EffectDescriptor> {
        self.runtime.descriptors()
    }
}

impl AudioEffectProcessor for AudioEffectHandle {
    fn process(
        &self,
        effect: &EffectInstance,
        block: &AudioBlock<'_>,
        state: &[u8],
    ) -> Result<AudioBlockOutput, String> {
        self.runtime.process(effect, block, state)
    }
}
