//! Audio-effect chains in the inspector: list, add, remove, reorder, bypass and scrub the
//! numeric parameters of a clip's audio effects. Every edit is a `Command` on the clip's
//! `AudioChain`, so undo/redo, the control protocol and MCP see exactly the same changes.

use gpui_component::switch::Switch;

use super::*;
use crate::style::{color, hex};
use gpui_component::Disableable;

/// One parameter row of an audio-effect card.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ParamRow {
    pub(super) name: String,
    pub(super) label: String,
    pub(super) step: f64,
    /// Stored value, else the plugin default, else 0 (before keyframe sampling).
    pub(super) value: f64,
    /// The plugin's default, when it declares the parameter.
    pub(super) default: Option<f64>,
    /// The plugin's `(min, max)` UI range, when it declares the parameter.
    pub(super) range: Option<(f64, f64)>,
}

/// What the inspector shows for one audio effect of a chain: title, bypass state, whether
/// the providing plugin is loaded and the parameter rows. Pure, so the clip and the track
/// inspectors (and their tests) share one definition.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct CardModel {
    pub(super) effect: EffectId,
    pub(super) title: String,
    pub(super) bypassed: bool,
    pub(super) plugin_missing: bool,
    pub(super) params: Vec<ParamRow>,
}

pub(super) fn card_models(
    instances: &[animathio_core::EffectInstance],
    available: &[animathio_plugin_host::EffectDescriptor],
) -> Vec<CardModel> {
    instances
        .iter()
        .map(|instance| {
            let descriptor = available
                .iter()
                .find(|d| d.plugin_id == instance.plugin && d.effect_id == instance.effect);
            // Parameters follow the plugin's declared order, then any stored extras.
            let mut params: Vec<ParamRow> = descriptor.map_or_default(|d| {
                d.params
                    .iter()
                    .map(|p| ParamRow {
                        name: p.id.clone(),
                        label: p.label.clone(),
                        step: p.step,
                        value: instance.params.get(&p.id).copied().unwrap_or(p.default),
                        default: Some(p.default),
                        range: Some((p.min, p.max)),
                    })
                    .collect()
            });
            for (name, value) in &instance.params {
                if !params.iter().any(|p| &p.name == name) {
                    params.push(ParamRow {
                        name: name.clone(),
                        label: name.clone(),
                        step: 0.01,
                        value: *value,
                        default: None,
                        range: None,
                    });
                }
            }
            CardModel {
                effect: instance.id,
                title: descriptor.map_or_else(|| instance.effect.clone(), |d| d.name.clone()),
                bypassed: instance.bypass,
                plugin_missing: descriptor.is_none(),
                params,
            }
        })
        .collect()
}

impl Editor {
    /// Audio filters offered by loaded audio-effect plugins, in plugin then listing order.
    pub(super) fn available_audio_effects(&self) -> Vec<animathio_plugin_host::EffectDescriptor> {
        self.plugin_host
            .as_ref()
            .map_or_default(PluginHost::audio_effects)
    }

    fn audio_effect_descriptor(
        &self,
        plugin: &str,
        effect: &str,
    ) -> Option<animathio_plugin_host::EffectDescriptor> {
        self.available_audio_effects()
            .into_iter()
            .find(|e| e.plugin_id == plugin && e.effect_id == effect)
    }

    /// Whether `plugin`/`effect` names an audio filter (as opposed to a pixel filter).
    pub(super) fn is_audio_effect(&self, plugin: &str, effect: &str) -> bool {
        self.audio_effect_descriptor(plugin, effect).is_some()
    }

    /// Command that sets one parameter of a clip's audio effect, clamped to the plugin's range.
    pub(super) fn audio_effect_chain_param_command(
        &self,
        chain: AudioChain,
        effect: EffectId,
        name: &str,
        value: f64,
    ) -> Option<Command> {
        if !value.is_finite() {
            return None;
        }
        let effects: &[animathio_core::EffectInstance] = match chain {
            AudioChain::Clip(id) => &self.find_clip(id)?.audio_effects,
            AudioChain::Track(id) => self
                .project
                .tracks()
                .iter()
                .find(|t| t.id() == id)?
                .audio_effects(),
        };
        let instance = effects.iter().find(|e| e.id == effect)?;
        let value = self
            .audio_effect_descriptor(&instance.plugin, &instance.effect)
            .and_then(|d| d.params.into_iter().find(|p| p.id == name))
            .map_or(value, |p| value.clamp(p.min, p.max));
        if let AudioChain::Clip(id) = chain {
            let clip = self.find_clip(id)?;
            let target = Target::Effect {
                effect,
                param: name.to_owned(),
            };
            if clip.animations.iter().any(|a| a.target == target) {
                return Some(inspector::numeric_edit_command(
                    clip,
                    target,
                    value,
                    self.position,
                ));
            }
        }
        Some(Command::SetAudioEffectParam {
            chain,
            effect,
            name: name.to_owned(),
            value: Some(value),
        })
    }

    /// Append an audio effect, with the plugin's default parameters, to an audio clip.
    pub(super) fn add_audio_effect_to_clip(
        &mut self,
        id: ClipId,
        plugin: &str,
        effect: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(_descriptor) = self.audio_effect_descriptor(plugin, effect) else {
            self.error = Some(format!("audio effect {plugin}/{effect} is not available"));
            cx.notify();
            return;
        };
        let Some(clip) = self.find_clip(id) else {
            return;
        };
        if !matches!(clip.kind, ClipKind::Audio(_)) {
            self.error = Some("audio effects apply to audio clips".into());
            cx.notify();
            return;
        }
        self.add_audio_effect_to_chain(AudioChain::Clip(id), plugin, effect, cx);
    }

    fn add_audio_effect_to_chain(
        &mut self,
        chain: AudioChain,
        plugin: &str,
        effect: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(descriptor) = self.audio_effect_descriptor(plugin, effect) else {
            return;
        };
        let index = match chain {
            AudioChain::Clip(id) => self.find_clip(id).map_or(0, |c| c.audio_effects.len()),
            AudioChain::Track(id) => self
                .project
                .tracks()
                .iter()
                .find(|t| t.id() == id)
                .map_or(0, |t| t.audio_effects().len()),
        };
        let instance = descriptor.instance(EffectId(self.next_id));
        self.next_id = self.next_id.saturating_add(1);
        self.execute(
            &Command::AddAudioEffect {
                chain,
                index,
                effect: instance,
            },
            cx,
        );
    }

    /// Rows for the inspector's Audio section: one card per effect, then the add buttons.
    pub(super) fn audio_effect_rows(
        &mut self,
        id: ClipId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        self.audio_chain_rows(AudioChain::Clip(id), window, cx)
    }

    pub(super) fn audio_chain_rows(
        &mut self,
        chain: AudioChain,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        let instances = match chain {
            AudioChain::Clip(id) => self.find_clip(id).map(|c| c.audio_effects.clone()),
            AudioChain::Track(id) => self
                .project
                .tracks()
                .iter()
                .find(|t| t.id() == id)
                .map(|t| t.audio_effects().to_vec()),
        };
        let Some(instances) = instances else {
            return Vec::new();
        };
        let available = self.available_audio_effects();
        let count = instances.len();
        let mut rows = Vec::new();
        for (index, model) in card_models(&instances, &available).into_iter().enumerate() {
            let title = model.title.clone();
            let effect = model.effect;
            let key = format!("{chain:?}:ae{}", effect.0);
            let bypassed = model.bypassed;
            let header =
                div()
                    .h(px(28.))
                    .px_2()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .child(icons::icon(IconName::AudioWaveform).small().text_color(hex(
                        if bypassed {
                            color::TEXT_MUTED
                        } else {
                            color::CLIP_AUDIO
                        },
                    )))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(hex(if bypassed {
                                color::TEXT_MUTED
                            } else {
                                color::TEXT
                            }))
                            .child(title),
                    )
                    .child(
                        Switch::new(SharedString::from(format!("{key}-on")))
                            .checked(!bypassed)
                            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                this.execute(
                                    &Command::SetAudioEffectBypass {
                                        chain,
                                        effect,
                                        bypass: !*checked,
                                    },
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("{key}-up")))
                            .ghost()
                            .compact()
                            .icon(icons::icon(IconName::ArrowUp).xsmall())
                            .tooltip("Move up")
                            .disabled(index == 0)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.execute(
                                    &Command::MoveAudioEffect {
                                        chain,
                                        effect,
                                        index: index.saturating_sub(1),
                                    },
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("{key}-down")))
                            .ghost()
                            .compact()
                            .icon(icons::icon(IconName::ArrowDown).xsmall())
                            .tooltip("Move down")
                            .disabled(index + 1 >= count)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.execute(
                                    &Command::MoveAudioEffect {
                                        chain,
                                        effect,
                                        index: index + 1,
                                    },
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("{key}-remove")))
                            .ghost()
                            .compact()
                            .icon(icons::icon(IconName::X).xsmall())
                            .tooltip("Remove effect")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.execute(&Command::RemoveAudioEffect { chain, effect }, cx);
                            })),
                    );
            let mut card = div()
                .mx_2()
                .my_1()
                .pb_1()
                .rounded(px(crate::style::radius::CARD))
                .border_1()
                .border_color(hex(color::BORDER))
                .bg(hex(color::RAISED))
                .flex()
                .flex_col()
                .child(header);
            if model.plugin_missing {
                card = card.child(
                    div()
                        .px_2()
                        .text_size(px(11.))
                        .text_color(hex(color::WARNING))
                        .child("Plugin not loaded"),
                );
            }
            for ParamRow {
                name,
                label,
                step,
                value,
                ..
            } in model.params
            {
                let value = match chain {
                    AudioChain::Clip(id) => self.find_clip(id).map_or(value, |clip| {
                        inspector::sampled_numeric(
                            clip,
                            &Target::Effect {
                                effect,
                                param: name.clone(),
                            },
                            self.position,
                            value,
                        )
                    }),
                    AudioChain::Track(_) => value,
                };
                let target = InputTarget::AudioEffectParam {
                    chain,
                    effect,
                    name: name.clone(),
                };
                let field = format!("{key}:{name}");
                let control = self.number_control(&field, target.clone(), value, step, window, cx);
                let label = Self::scrub_label(&field, label, target, value, step, cx);
                let target = Target::Effect {
                    effect,
                    param: name.clone(),
                };
                let diamond = match chain {
                    AudioChain::Clip(id) => Some(self.row_diamond(id, &target, cx)),
                    AudioChain::Track(_) => None,
                };
                card = card.child(Self::param_row(diamond, label, control, cx));
            }
            rows.push(card);
        }
        if available.is_empty() {
            rows.push(
                div()
                    .px_3()
                    .text_size(px(11.))
                    .text_color(hex(color::TEXT_MUTED))
                    .child("No audio effect plugins loaded"),
            );
        } else {
            rows.push(
                div()
                    .px_2()
                    .pt_1()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap_1()
                    .children(available.into_iter().map(|descriptor| {
                        let (plugin, effect) = (descriptor.plugin_id, descriptor.effect_id);
                        Button::new(SharedString::from(format!("add-ae-{chain:?}-{effect}")))
                            .ghost()
                            .compact()
                            .icon(icons::icon(IconName::Plus).xsmall())
                            .label(descriptor.name)
                            .tooltip(descriptor.description)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.add_audio_effect_to_chain(chain, &plugin, &effect, cx);
                            }))
                    })),
            );
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use animathio_core::{EffectInstance, Project, Track, TrackId};
    use animathio_plugin_host::{EffectDescriptor, EffectParam};
    use std::collections::BTreeMap;

    fn gain_descriptor() -> EffectDescriptor {
        EffectDescriptor {
            plugin_id: "org.animathio.audio-basic".into(),
            effect_id: "gain".into(),
            name: "Gain".into(),
            description: String::new(),
            params: vec![
                EffectParam {
                    id: "gain".into(),
                    label: "Gain (dB)".into(),
                    min: -60.0,
                    max: 24.0,
                    default: 0.0,
                    step: 0.1,
                },
                EffectParam {
                    id: "pan".into(),
                    label: "Pan".into(),
                    min: -1.0,
                    max: 1.0,
                    default: 0.0,
                    step: 0.01,
                },
            ],
        }
    }

    /// The track inspector shows its chain through the same card model as the clip
    /// inspector, so one headless test covers what each card lists.
    #[core::prelude::v1::test]
    fn track_chain_cards_list_title_bypass_params_and_missing_plugins() {
        let chain = AudioChain::Track(TrackId(1));
        let mut project = Project::default();
        Command::AddTrack {
            track: Track::new(TrackId(1), "Voice"),
            index: 0,
        }
        .apply(&mut project)
        .unwrap();
        let gain = EffectInstance::new(
            EffectId(10),
            "org.animathio.audio-basic",
            "gain",
            BTreeMap::from([("gain".into(), 6.0), ("legacy".into(), 1.5)]),
        );
        let mut ghost = EffectInstance::new(
            EffectId(11),
            "org.missing",
            "reverb",
            BTreeMap::from([("wet".into(), 0.3)]),
        );
        ghost.bypass = true;
        for (index, effect) in [gain, ghost].into_iter().enumerate() {
            Command::AddAudioEffect {
                chain,
                index,
                effect,
            }
            .apply(&mut project)
            .unwrap();
        }
        Command::SetAudioEffectBypass {
            chain,
            effect: EffectId(10),
            bypass: false,
        }
        .apply(&mut project)
        .unwrap();

        let instances = project.tracks()[0].audio_effects().to_vec();
        let cards = card_models(&instances, &[gain_descriptor()]);
        assert_eq!(cards.len(), 2);

        let first = &cards[0];
        assert_eq!((first.effect, first.title.as_str()), (EffectId(10), "Gain"));
        assert!(!first.bypassed && !first.plugin_missing);
        // Declared params first (stored value, else default), then stored extras.
        let rows: Vec<_> = first
            .params
            .iter()
            .map(|p| (p.name.as_str(), p.value))
            .collect();
        assert_eq!(rows, [("gain", 6.0), ("pan", 0.0), ("legacy", 1.5)]);
        assert_eq!(first.params[0].label, "Gain (dB)");
        assert!((first.params[0].step - 0.1).abs() < f64::EPSILON);

        // An effect whose plugin is not loaded keeps its id as the title, is flagged, and
        // still lists its stored parameters so they can be edited or removed.
        let second = &cards[1];
        assert_eq!(second.title, "reverb");
        assert!(second.bypassed && second.plugin_missing);
        assert_eq!(second.params.len(), 1);
        assert_eq!(second.params[0].name, "wet");
    }
}
