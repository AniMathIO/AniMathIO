//! Inspector view.

use super::*;
use crate::style::{color, hex};

impl Editor {
    pub(super) fn toggle_keyframe(&mut self, id: ClipId, target: Target, cx: &mut Context<Self>) {
        let Some(clip) = self.find_clip(id) else {
            return;
        };
        let command = if let Target::Param(name) = &target
            && let Some(model) = self.plugin_model(id, name)
        {
            let raw = if let ClipKind::Plugin { params, .. } = &clip.kind {
                params.get(name)
            } else {
                None
            };
            inspector::param_toggle_command(
                clip,
                name.clone(),
                self.position,
                inspector::model_value(&model, raw),
            )
        } else {
            inspector::keyframe_toggle_command(clip, target, self.position)
        };
        self.execute(&command, cx);
    }

    pub(super) fn interpolation_command(
        &self,
        selection: &KeyframeSelection,
        interpolation: Interpolation,
    ) -> Option<Command> {
        let clip = self.find_clip(selection.clip_id)?;
        inspector::interpolation_command(
            clip,
            selection.target.clone(),
            selection.time,
            interpolation,
        )
    }

    pub(super) fn keyframe_menu(
        &self,
        selection: &KeyframeSelection,
        cx: &mut Context<Self>,
    ) -> Vec<Stateful<Div>> {
        let mut rows = vec![
            div()
                .px_2()
                .child("Keyframe interpolation")
                .id("keyframe-menu-title"),
        ];
        let current = self.find_clip(selection.clip_id).and_then(|clip| {
            inspector::keyframe_interpolation(clip, &selection.target, selection.time)
        });
        for (label, interpolation) in [
            ("linear", Interpolation::Linear),
            ("hold", Interpolation::Hold),
            ("ease", Interpolation::EaseInOut),
        ] {
            let active = current == Some(interpolation);
            let selection = selection.clone();
            rows.push(
                div()
                    .px_2()
                    .cursor_pointer()
                    .bg(if active {
                        cx.theme().list_active
                    } else {
                        cx.theme().popover
                    })
                    .child(label)
                    .id("keyframe-interp")
                    .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                        if let Some(command) = this.interpolation_command(&selection, interpolation)
                        {
                            this.execute(&command, cx);
                        }
                    })),
            );
        }
        rows
    }

    pub(super) fn matching_asset_ids(&self, media_type: &str) -> Vec<AssetId> {
        let prefix = media_type.strip_suffix("/*").unwrap_or(media_type);
        let mut ids: Vec<AssetId> = self
            .project
            .assets()
            .iter()
            .filter(|(_, asset)| asset.media_type.starts_with(prefix))
            .map(|(id, _)| id.clone())
            .collect();
        ids.sort();
        ids
    }

    pub(super) fn section(
        &self,
        name: &'static str,
        title: &str,
        content: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> Div {
        let open = !self.closed_sections.contains(name);
        let chevron = if open {
            icons::icon(IconName::ChevronDown)
        } else {
            icons::icon(IconName::ChevronRight)
        };
        let icon = match name {
            "transform" | "shared" => IconName::Move,
            "opacity" => IconName::Blend,
            "params" => IconName::Type,
            "audio" => IconName::AudioWaveform,
            "effects" => IconName::Sparkles,
            "transition" => IconName::Waypoints,
            _ => IconName::SlidersHorizontal,
        };
        let resettable = matches!(
            name,
            "transform" | "opacity" | "params" | "audio" | "effects" | "transition" | "shared"
        );
        div()
            .mx_2()
            .mt_2()
            .rounded(px(6.))
            .border_1()
            .border_color(hex(color::BORDER))
            .bg(hex(color::RAISED))
            .child(
                div()
                    .h(px(34.))
                    .px_1()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        icons::icon(icon)
                            .small()
                            .text_color(hex(color::TEXT_SECONDARY)),
                    )
                    .child(
                        Button::new(format!("section-{name}"))
                            .ghost()
                            .compact()
                            .icon(chevron)
                            .label(title)
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.toggle_section(name, cx)),
                            ),
                    )
                    .child(div().flex_1())
                    .when(resettable, |header| {
                        header.child(
                            Button::new(SharedString::from(format!("reset-{name}")))
                                .ghost()
                                .compact()
                                .icon(icons::icon(IconName::RotateCcw).xsmall())
                                .tooltip("Reset section (including keyframes)")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.reset_inspector_section(name, cx)
                                })),
                        )
                    }),
            )
            .child(
                gpui_component::collapsible::Collapsible::new()
                    .open(open)
                    .content(div().pb_2().child(content)),
            )
    }

    fn reset_inspector_section(&mut self, name: &str, cx: &mut Context<Self>) {
        let mut commands = Vec::new();
        for id in self.selected_ids() {
            let Some(clip) = self.find_clip(id) else {
                continue;
            };
            if matches!(name, "transform" | "shared" | "opacity") {
                commands.push(inspector::reset_transform(clip, name == "opacity"));
                continue;
            }
            let matches_target = |target: &Target| match name {
                "params" => matches!(target, Target::Param(_)),
                "audio" => {
                    matches!(target, Target::Param(param) if param == "gain" || param == "muted")
                }
                "effects" => matches!(target, Target::Effect { .. }),
                _ => false,
            };
            for animation in clip
                .animations
                .iter()
                .filter(|animation| matches_target(&animation.target))
            {
                for time in animation.keys.keys().keys() {
                    commands.push(Command::RemoveKeyframe {
                        id,
                        target: animation.target.clone(),
                        time: *time,
                    });
                }
            }
            match name {
                "audio" => {
                    commands.push(Command::SetParam {
                        id,
                        name: "gain".into(),
                        value: Some(1.),
                    });
                    commands.push(Command::SetParam {
                        id,
                        name: "muted".into(),
                        value: Some(0.),
                    });
                }
                "params" => {
                    if let ClipKind::Plugin { plugin_id, .. } = &clip.kind
                        && let Some(plugin) =
                            self.plugins.iter().find(|plugin| &plugin.id == plugin_id)
                    {
                        for model in plugin.models() {
                            commands.push(Command::SetPluginParam {
                                id,
                                name: model.id().into(),
                                value: Some(inspector::model_value(&model, None)),
                            });
                        }
                    }
                }
                "effects" => {
                    for effect in &clip.effects {
                        commands.push(Command::RemoveEffect {
                            id,
                            effect: effect.id,
                        });
                    }
                }
                "transition" => {
                    for track in self.project.tracks() {
                        for transition in track
                            .transitions()
                            .iter()
                            .filter(|t| t.from == id || t.to == id)
                        {
                            let command = Command::RemoveTransition { id: transition.id };
                            if !commands.contains(&command) {
                                commands.push(command);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if !commands.is_empty() {
            self.execute(&Command::Batch { commands }, cx);
        }
    }

    /// Read-only summary of the selected clip's effect chain.
    pub(super) fn effect_rows(&self, id: ClipId, _cx: &mut Context<Self>) -> Vec<Div> {
        let Some(clip) = self.find_clip(id) else {
            return Vec::new();
        };
        clip.effects
            .iter()
            .map(|instance| {
                let params: Vec<String> = instance
                    .params
                    .iter()
                    .map(|(name, value)| format!("{name} {value:.3}"))
                    .collect();
                let summary = if params.is_empty() {
                    effect_label(instance.kind).to_owned()
                } else {
                    format!("{} ({})", effect_label(instance.kind), params.join(", "))
                };
                div()
                    .px_2()
                    .py_1()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(icons::icon(IconName::Sparkles).small())
                    .child(summary)
            })
            .collect()
    }
}
