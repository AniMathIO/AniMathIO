//! Video-effect chains in the inspector: one editable card per effect with bypass, reorder,
//! remove, and per-parameter slider, scrub, typed entry, keyframe diamond and reset. Every
//! edit is a `Command` on the clip (`SetEffectParam`, `AddKeyframe` with `Target::Effect`,
//! `MoveEffect`, ...), so undo/redo, the control protocol and MCP see exactly the same changes.
//! The cards reuse the audio-effect card model ([`audio_effects::card_models`]).

use gpui_component::Disableable;
use gpui_component::switch::Switch;

use super::audio_effects::{CardModel, ParamRow, card_models};
use super::*;
use crate::style::{color, hex};

/// Why an effect card shows a warning, if it does: its plugin is not loaded, or the last
/// preview frame failed inside the clip's effect chain.
pub(super) fn card_warning(model: &CardModel, clip_failed: bool) -> Option<&'static str> {
    if model.plugin_missing {
        Some("Plugin not loaded")
    } else if clip_failed {
        Some("Failed on the last preview frame")
    } else {
        None
    }
}

/// Command that removes a video effect. A keyframed effect cannot be removed while an
/// animation targets it, so its keyframes go in the same `Batch` (one undo step restores
/// both).
pub(super) fn remove_effect_command(clip: &Clip, effect: EffectId) -> Command {
    let mut commands: Vec<Command> = clip
        .animations
        .iter()
        .filter(|animation| {
            matches!(&animation.target, Target::Effect { effect: target, .. } if *target == effect)
        })
        .flat_map(|animation| {
            animation
                .keys
                .keys()
                .keys()
                .map(|time| Command::RemoveKeyframe {
                    id: clip.id,
                    target: animation.target.clone(),
                    time: *time,
                })
        })
        .collect();
    let remove = Command::RemoveEffect {
        id: clip.id,
        effect,
    };
    if commands.is_empty() {
        remove
    } else {
        commands.push(remove);
        Command::Batch { commands }
    }
}

impl Editor {
    fn video_effect_descriptor(
        &self,
        plugin: &str,
        effect: &str,
    ) -> Option<animathio_plugin_host::EffectDescriptor> {
        self.available_effects()
            .into_iter()
            .find(|e| e.plugin_id == plugin && e.effect_id == effect)
    }

    /// Command that sets one parameter of a clip's video effect to `value`, clamped to the
    /// plugin's range. A keyframed parameter gets a keyframe at the playhead instead of a new
    /// base value, exactly like transform fields.
    pub(super) fn video_effect_param_command(
        &self,
        id: ClipId,
        effect: EffectId,
        name: &str,
        value: f64,
    ) -> Option<Command> {
        if !value.is_finite() {
            return None;
        }
        let clip = self.find_clip(id)?;
        let instance = clip.effects.iter().find(|e| e.id == effect)?;
        let value = self
            .video_effect_descriptor(&instance.plugin, &instance.effect)
            .and_then(|d| d.params.into_iter().find(|p| p.id == name))
            .map_or(value, |p| value.clamp(p.min, p.max));
        Some(inspector::numeric_edit_command(
            clip,
            Target::Effect {
                effect,
                param: name.to_owned(),
            },
            value,
            self.position,
        ))
    }

    /// Cards for the inspector's Effects section: one per effect of the clip's chain, then
    /// the buttons that append a new effect.
    pub(super) fn video_effect_rows(
        &mut self,
        id: ClipId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        let Some(instances) = self.find_clip(id).map(|c| c.effects.clone()) else {
            return Vec::new();
        };
        let available = self.available_effects();
        let failed = self.failed_effect_clips.contains(&id);
        let count = instances.len();
        let mut rows = Vec::new();
        for (index, model) in card_models(&instances, &available).into_iter().enumerate() {
            let effect = model.effect;
            let key = format!("c{}:fx{}", id.0, effect.0);
            let bypassed = model.bypassed;
            let warning = card_warning(&model, failed);
            let header = div()
                .h(px(28.))
                .px_2()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .child(
                    icons::icon(if warning.is_some() {
                        IconName::TriangleAlert
                    } else {
                        IconName::Sparkles
                    })
                    .small()
                    .text_color(hex(if warning.is_some() {
                        color::WARNING
                    } else if bypassed {
                        color::TEXT_MUTED
                    } else {
                        color::TEXT_SECONDARY
                    })),
                )
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
                        .child(model.title.clone()),
                )
                .child(
                    Switch::new(SharedString::from(format!("{key}-on")))
                        .checked(!bypassed)
                        .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                            this.execute(
                                &Command::SetEffectBypass {
                                    id,
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
                                &Command::MoveEffect {
                                    id,
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
                                &Command::MoveEffect {
                                    id,
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
                            if let Some(clip) = this.find_clip(id) {
                                let command = remove_effect_command(clip, effect);
                                this.execute(&command, cx);
                            }
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
            if let Some(text) = warning {
                let text = if model.plugin_missing {
                    format!("Plugin {} is not loaded", instances[index].plugin)
                } else {
                    text.to_owned()
                };
                card = card.child(
                    div()
                        .px_2()
                        .text_size(px(11.))
                        .text_color(hex(color::WARNING))
                        .child(text),
                );
            }
            for ParamRow {
                name,
                label,
                step,
                value,
                default,
                range,
            } in model.params
            {
                let target = Target::Effect {
                    effect,
                    param: name.clone(),
                };
                let value = self.find_clip(id).map_or(value, |clip| {
                    inspector::sampled_numeric(clip, &target, self.position, value)
                });
                let input = InputTarget::VideoEffectParam {
                    id,
                    effect,
                    name: name.clone(),
                };
                let field = format!("{key}:{name}");
                let mut control =
                    self.number_control(&field, input.clone(), value, step, window, cx);
                if let Some(range) = range.filter(|(min, max)| max > min) {
                    control =
                        control.child(div().pl(px(18.)).pb_1().child(self.range_slider_control(
                            &format!("{field}-slider"),
                            Some(input.clone()),
                            value,
                            (range.0, range.1, step.max(1e-6)),
                            window,
                            cx,
                        )));
                }
                let mut label = div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .child(Self::scrub_label(&field, label, input, value, step, cx));
                if let Some(default) = default.filter(|d| (value - d).abs() > 1e-9) {
                    let param = name.clone();
                    label = label.child(
                        Button::new(SharedString::from(format!("{field}-reset")))
                            .ghost()
                            .compact()
                            .icon(icons::icon(IconName::RotateCcw).xsmall())
                            .tooltip("Reset to default")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(command) =
                                    this.video_effect_param_command(id, effect, &param, default)
                                {
                                    this.execute(&command, cx);
                                }
                            })),
                    );
                }
                let diamond = self.row_diamond(id, &target, cx);
                card = card.child(Self::param_row(Some(diamond), label, control, cx));
            }
            rows.push(card);
        }
        rows.push(if available.is_empty() {
            div()
                .px_3()
                .text_size(px(11.))
                .text_color(hex(color::TEXT_MUTED))
                .child("No effect plugins loaded")
        } else {
            div()
                .px_2()
                .pt_1()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap_1()
                .children(available.into_iter().map(|descriptor| {
                    let (plugin, effect) = (descriptor.plugin_id, descriptor.effect_id);
                    Button::new(SharedString::from(format!(
                        "add-fx-c{}-{plugin}-{effect}",
                        id.0
                    )))
                    .ghost()
                    .compact()
                    .icon(icons::icon(IconName::Plus).xsmall())
                    .label(descriptor.name)
                    .tooltip(descriptor.description)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.add_effect_to_clip(id, &plugin, &effect, cx);
                    }))
                }))
        });
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use animathio_core::{
        Asset, AssetId, Clip, ClipKind, EffectInstance, Interpolation, Keyframe, Project, Time,
        TimeRange, Track, TrackId,
    };
    use animathio_plugin_host::{EffectDescriptor, EffectParam};
    use std::collections::BTreeMap;

    fn blur_descriptor() -> EffectDescriptor {
        EffectDescriptor {
            plugin_id: "org.animathio.effects-basic".into(),
            effect_id: "blur".into(),
            name: "Blur".into(),
            description: String::new(),
            params: vec![EffectParam {
                id: "radius".into(),
                label: "Radius".into(),
                min: 0.0,
                max: 64.0,
                default: 0.0,
                step: 0.5,
            }],
        }
    }

    fn project_with_effects() -> Project {
        let mut project = Project::default();
        let asset = AssetId::of(b"fixture");
        for command in [
            Command::AddAsset {
                id: asset.clone(),
                asset: Asset {
                    name: "a.png".into(),
                    media_type: "image/png".into(),
                },
            },
            Command::AddTrack {
                track: Track::new(TrackId(1), "V1"),
                index: 0,
            },
            Command::AddClip {
                track: TrackId(1),
                clip: Clip::new(
                    ClipId(5),
                    ClipKind::Image(asset),
                    TimeRange::new(Time(0), Time(1000)).unwrap(),
                ),
            },
            Command::AddEffect {
                id: ClipId(5),
                index: 0,
                effect: EffectInstance::new(
                    EffectId(10),
                    "org.animathio.effects-basic",
                    "blur",
                    BTreeMap::from([("radius".into(), 4.0)]),
                ),
            },
            Command::AddEffect {
                id: ClipId(5),
                index: 1,
                effect: EffectInstance::new(
                    EffectId(11),
                    "org.missing",
                    "glow",
                    BTreeMap::from([("amount".into(), 0.3)]),
                ),
            },
        ] {
            command.apply(&mut project).unwrap();
        }
        project
    }

    fn clip(project: &Project) -> &Clip {
        &project.tracks()[0].clips()[&ClipId(5)]
    }

    /// The video inspector builds its cards through the shared card model: declared params
    /// carry the plugin default and range (for the slider and reset button), a missing plugin
    /// is flagged and keeps its stored params, and the warning follows the failed-frame flag.
    #[core::prelude::v1::test]
    fn video_effect_cards_list_params_defaults_ranges_and_warnings() {
        let project = project_with_effects();
        let cards = card_models(&clip(&project).effects, &[blur_descriptor()]);
        assert_eq!(cards.len(), 2);

        let blur = &cards[0];
        assert_eq!((blur.effect, blur.title.as_str()), (EffectId(10), "Blur"));
        assert_eq!(blur.params.len(), 1);
        let radius = &blur.params[0];
        assert_eq!(
            (radius.name.as_str(), radius.label.as_str(), radius.value),
            ("radius", "Radius", 4.0)
        );
        assert_eq!(radius.default, Some(0.0));
        assert_eq!(radius.range, Some((0.0, 64.0)));
        assert_eq!(card_warning(blur, false), None);
        assert_eq!(
            card_warning(blur, true),
            Some("Failed on the last preview frame")
        );

        let ghost = &cards[1];
        assert!(ghost.plugin_missing);
        assert_eq!(card_warning(ghost, false), Some("Plugin not loaded"));
        assert_eq!(ghost.params.len(), 1);
        assert_eq!(
            (ghost.params[0].default, ghost.params[0].range),
            (None, None)
        );
    }

    #[core::prelude::v1::test]
    fn removing_a_keyframed_effect_is_one_undo_step_that_restores_the_keyframes() {
        let mut project = project_with_effects();
        let target = Target::Effect {
            effect: EffectId(10),
            param: "radius".into(),
        };
        for (time, value) in [(Time(0), 0.0), (Time(500), 8.0)] {
            Command::AddKeyframe {
                id: ClipId(5),
                target: target.clone(),
                time,
                keyframe: Keyframe {
                    value: value.into(),
                    interpolation: Interpolation::Linear,
                },
            }
            .apply(&mut project)
            .unwrap();
        }
        // A plain RemoveEffect would be rejected while the effect is animated.
        assert!(
            Command::RemoveEffect {
                id: ClipId(5),
                effect: EffectId(10),
            }
            .apply(&mut project.clone())
            .is_err()
        );
        let before = project.clone();
        let command = remove_effect_command(clip(&project), EffectId(10));
        let mut history = History::default();
        history.execute(&mut project, &command).unwrap();
        let after = clip(&project);
        assert_eq!(
            after.effects.iter().map(|e| e.id).collect::<Vec<_>>(),
            [EffectId(11)]
        );
        assert_eq!(after.animations, []);
        assert!(history.undo(&mut project).unwrap());
        assert_eq!(project, before);

        // An effect without keyframes is removed by a bare command.
        assert!(matches!(
            remove_effect_command(clip(&project), EffectId(11)),
            Command::RemoveEffect { .. }
        ));
    }

    /// Editing a keyframed parameter at the playhead adds a keyframe, a plain one sets the
    /// base value; both are what the slider, scrub and typed paths commit.
    #[core::prelude::v1::test]
    fn param_edits_set_the_base_value_or_key_the_playhead() {
        let mut project = project_with_effects();
        let target = Target::Effect {
            effect: EffectId(10),
            param: "radius".into(),
        };
        let playhead = Time(200);
        let plain = inspector::numeric_edit_command(clip(&project), target.clone(), 9.0, playhead);
        assert_eq!(
            plain,
            Command::SetEffectParam {
                id: ClipId(5),
                effect: EffectId(10),
                name: "radius".into(),
                value: Some(9.0),
            }
        );
        let toggle = inspector::keyframe_toggle_command(clip(&project), target.clone(), playhead);
        let mut history = History::default();
        history.execute(&mut project, &toggle).unwrap();
        assert!(inspector::keyframed(clip(&project), &target, playhead));
        // The new key keeps the current value, so nothing jumps.
        assert_eq!(
            inspector::sampled_numeric(clip(&project), &target, playhead, 0.0),
            4.0
        );
        let edit = inspector::numeric_edit_command(clip(&project), target.clone(), 12.0, playhead);
        assert!(matches!(edit, Command::AddKeyframe { .. }));
        history.execute(&mut project, &edit).unwrap();
        assert_eq!(
            inspector::sampled_numeric(clip(&project), &target, playhead, 0.0),
            12.0
        );
        // The base value is untouched; undo goes back one step at a time.
        assert_eq!(clip(&project).effects[0].params["radius"], 4.0);
        assert!(history.undo(&mut project).unwrap());
        assert_eq!(
            inspector::sampled_numeric(clip(&project), &target, playhead, 0.0),
            4.0
        );
    }

    /// A slider/scrub drag previews through `ColorEdit` and commits as one undo step.
    #[core::prelude::v1::test]
    fn effect_param_drag_is_one_undo_step() {
        let mut project = project_with_effects();
        let before = project.clone();
        let mut history = History::default();
        let mut edit = None;
        for value in [5.0, 6.0, 7.0] {
            super::super::color_edit::ColorEdit::preview(
                &mut project,
                Command::SetEffectParam {
                    id: ClipId(5),
                    effect: EffectId(10),
                    name: "radius".into(),
                    value: Some(value),
                },
                &mut edit,
            )
            .unwrap();
            assert_eq!(clip(&project).effects[0].params["radius"], value);
        }
        edit.unwrap().commit(&mut project, &mut history).unwrap();
        assert_eq!(clip(&project).effects[0].params["radius"], 7.0);
        assert!(history.undo(&mut project).unwrap());
        assert_eq!(project, before);
        assert!(!history.undo(&mut project).unwrap());
    }
}
