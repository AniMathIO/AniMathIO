//! Inspector view.

use super::*;

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

    pub(super) fn plugin_asset_control(
        &self,
        id: ClipId,
        name: String,
        media_type: &str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let current = self.plugin_asset_value(id, &name);
        let label = current.as_ref().map_or_else(
            || "none".to_owned(),
            |asset| {
                self.project
                    .assets()
                    .get(&asset_id_from(asset))
                    .map_or_else(|| "unknown".to_owned(), |meta| meta.name.clone())
            },
        );
        let media_type = media_type.to_owned();
        div()
            .cursor_pointer()
            .child(label)
            .id("plugin-asset")
            .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                let ids = this.matching_asset_ids(&media_type);
                let next = current
                    .as_deref()
                    .and_then(|current| {
                        let index = ids.iter().position(|asset| asset.as_str() == current)?;
                        ids.get(index + 1).or_else(|| ids.first())
                    })
                    .or_else(|| ids.first())
                    .cloned();
                this.execute(
                    &this.plugin_param_command(
                        id,
                        name.clone(),
                        next.map_or(Value::Null, |asset| json!(asset.as_str())),
                    ),
                    cx,
                );
            }))
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
        div()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Button::new(format!("section-{name}"))
                    .ghost()
                    .compact()
                    .icon(chevron)
                    .label(title)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.toggle_section(name, cx);
                    })),
            )
            .child(
                gpui_component::collapsible::Collapsible::new()
                    .open(open)
                    .content(content),
            )
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
