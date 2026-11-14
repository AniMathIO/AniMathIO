//! First-party `audio-basic` plugin: seven deterministic audio effects.
//!
//! `gain` (trim in dB), `eq3` (low shelf, mid peak, high shelf), `compressor`, `gate`,
//! `highpass`, the `voice_cleanup` preset (high-pass + gentle gate + split-band de-esser) and
//! `denoise` (streaming spectral gating).
//! All DSP lives in [`dsp`], which has no dependency on the plugin ABI and is unit-tested
//! natively; this file only adapts it to the `audio-effect-plugin` world.

mod dsp;

#[cfg(target_arch = "wasm32")]
mod bindings {
    wit_bindgen::generate!({
        path: "../../crates/plugin-api/wit",
        world: "audio-effect-plugin",
    });

    use crate::dsp;
    use animathio::plugin::types::{
        Diagnostic, NumberRange, ParamEntry, ParamKind, ParamSpec, ParamValue,
    };
    use exports::animathio::plugin::audio_effects::{
        AudioBlock, AudioEffectDescriptor, BlockOutput, Guest as AudioEffects,
    };

    struct AudioBasic;

    impl Guest for AudioBasic {
        fn describe() -> Manifest {
            Manifest {
                name: "Basic Audio Effects".to_owned(),
                version: "1.1.0".to_owned(),
                description: "Gain, EQ, compressor, gate, high-pass, voice cleanup and denoise"
                    .to_owned(),
                kinds: vec!["audio-effect".into()],
                metadata: vec![],
                params: Vec::new(),
            }
        }
    }

    impl AudioEffects for AudioBasic {
        fn list_audio_effects() -> Vec<AudioEffectDescriptor> {
            dsp::EFFECTS
                .iter()
                .map(|effect| AudioEffectDescriptor {
                    id: effect.id.to_owned(),
                    name: effect.name.to_owned(),
                    description: effect.description.to_owned(),
                    params: effect
                        .params
                        .iter()
                        .map(|param| ParamSpec {
                            id: param.id.to_owned(),
                            label: param.label.to_owned(),
                            kind: ParamKind::Number(NumberRange {
                                min: param.min,
                                max: param.max,
                                default: param.default,
                                step: param.step,
                            }),
                        })
                        .collect(),
                })
                .collect()
        }

        fn process_block(
            effect: String,
            block: AudioBlock,
            params: Vec<ParamEntry>,
            state: Option<Vec<u8>>,
        ) -> Result<BlockOutput, Diagnostic> {
            (|| -> Result<BlockOutput, String> {
                let params = params
                    .into_iter()
                    .map(|entry| match entry.value {
                        ParamValue::Number(value) => Ok((entry.id, value)),
                        _ => Err(format!("parameter {:?} must be a number", entry.id)),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let mut samples = block.samples;
                let state = dsp::process(
                    &effect,
                    &params,
                    block.sample_rate,
                    usize::from(block.channels),
                    &mut samples,
                    state.as_deref(),
                )?;
                Ok(BlockOutput { samples, state })
            })()
            .map_err(|message| Diagnostic {
                code: "plugin-error".into(),
                message,
                metadata: vec![],
            })
        }
    }

    export!(AudioBasic);
}
