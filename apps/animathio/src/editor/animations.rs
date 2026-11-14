//! Animation presets: the inspector's Animations section and the Effects-tab library.
//!
//! Presets come from loaded animation-preset plugins (`org.animathio.animations`).
//! Applying one writes ordinary keyframes as a single `Command::Batch`, so the
//! result renders without the plugin and one undo restores the previous keys.

use super::*;
use crate::style::{color, hex};
use animathio_core::TICKS_PER_SECOND;
use animathio_plugin_host::{PresetDescriptor, PresetRole, clip_kind_id};

/// Window-length step of the inspector's duration buttons: a quarter second.
const DURATION_STEP: i64 = TICKS_PER_SECOND / 4;
/// Longest entrance/exit window offered by the duration buttons: ten seconds.
const MAX_DURATION: i64 = TICKS_PER_SECOND * 10;

/// Inspector duration for each role; `Time(0)` means the preset's default
/// (half a second for in/out, the whole clip for emphasis).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct PresetDurations {
    entrance: Time,
    exit: Time,
    emphasis: Time,
}

impl PresetDurations {
    fn get(&self, role: PresetRole) -> Time {
        match role {
            PresetRole::Entrance => self.entrance,
            PresetRole::Exit => self.exit,
            PresetRole::Emphasis => self.emphasis,
        }
    }
    fn get_mut(&mut self, role: PresetRole) -> &mut Time {
        match role {
            PresetRole::Entrance => &mut self.entrance,
            PresetRole::Exit => &mut self.exit,
            PresetRole::Emphasis => &mut self.emphasis,
        }
    }
}

const ROLES: [(PresetRole, &str); 3] = [
    (PresetRole::Entrance, "In"),
    (PresetRole::Exit, "Out"),
    (PresetRole::Emphasis, "Emphasis"),
];

impl Editor {
    /// Presets offered by the loaded animation-preset plugins.
    pub(super) fn available_animation_presets(&self) -> Vec<PresetDescriptor> {
        self.plugin_host
            .as_ref()
            .map_or_default(PluginHost::animation_presets)
    }

    /// Apply a preset to every selected clip it fits, as one undo step.
    pub(super) fn apply_animation_preset(
        &mut self,
        plugin: &str,
        preset: &str,
        cx: &mut Context<Self>,
    ) {
        let ids = self.selected_ids();
        if ids.is_empty() {
            self.error = Some("select a clip to animate".into());
            cx.notify();
            return;
        }
        let Some(descriptor) = self
            .available_animation_presets()
            .into_iter()
            .find(|p| p.plugin_id == plugin && p.preset_id == preset)
        else {
            self.error = Some(format!(
                "animation preset {plugin}/{preset} is not available"
            ));
            cx.notify();
            return;
        };
        let duration = self.preset_durations.get(descriptor.role);
        let mut commands = Vec::new();
        let mut working = self.project.clone();
        for id in ids {
            let fits = self
                .find_clip(id)
                .is_some_and(|clip| descriptor.applies_to_kind(&clip_kind_id(&clip.kind)));
            if !fits {
                continue;
            }
            let params = animathio_control::AnimationApplyParams {
                clip: id,
                preset: preset.to_owned(),
                plugin: Some(plugin.to_owned()),
                duration: (duration.0 > 0).then_some(duration),
                options: BTreeMap::new(),
            };
            match animathio_control::animation_preset_command(
                &working,
                self.plugin_host.as_ref(),
                &params,
            ) {
                Ok((command, _)) => {
                    // Later clips see earlier edits, so the batch stays consistent.
                    if command.apply(&mut working).is_ok() {
                        commands.push(command);
                    }
                }
                Err(error) => {
                    self.error = Some(error.to_string());
                    cx.notify();
                    return;
                }
            }
        }
        if commands.is_empty() {
            self.error = Some(format!(
                "{} does not apply to the selection",
                descriptor.name
            ));
            cx.notify();
            return;
        }
        let command = if commands.len() == 1 {
            commands.remove(0)
        } else {
            Command::Batch { commands }
        };
        self.execute(&command, cx);
    }

    fn step_preset_duration(&mut self, role: PresetRole, delta: i64, cx: &mut Context<Self>) {
        let default = if role == PresetRole::Emphasis {
            0
        } else {
            TICKS_PER_SECOND / 2
        };
        let slot = self.preset_durations.get_mut(role);
        let current = if slot.0 > 0 { slot.0 } else { default };
        let next = (current + delta).clamp(0, MAX_DURATION);
        // Emphasis steps down to 0, meaning the whole clip.
        *slot = Time(if role == PresetRole::Emphasis {
            next
        } else {
            next.max(DURATION_STEP)
        });
        cx.notify();
    }

    fn duration_label(&self, role: PresetRole) -> String {
        let duration = self.preset_durations.get(role);
        if duration.0 <= 0 {
            return if role == PresetRole::Emphasis {
                "Whole clip".to_owned()
            } else {
                timecode::format_timecode(Time(TICKS_PER_SECOND / 2), self.project.frame_rate())
            };
        }
        timecode::format_timecode(duration, self.project.frame_rate())
    }

    /// The inspector's Animations section: for each role a duration stepper and
    /// one button per preset that fits the clip.
    pub(super) fn animation_rows(&self, id: ClipId, cx: &mut Context<Self>) -> Vec<Div> {
        let Some(clip) = self.find_clip(id) else {
            return Vec::new();
        };
        let kind = clip_kind_id(&clip.kind);
        let presets = self.available_animation_presets();
        if presets.is_empty() {
            return vec![
                div()
                    .px_3()
                    .text_size(px(11.))
                    .text_color(hex(color::TEXT_MUTED))
                    .child("No animation plugins loaded"),
            ];
        }
        ROLES
            .iter()
            .filter_map(|&(role, label)| {
                let fitting: Vec<_> = presets
                    .iter()
                    .filter(|p| p.role == role && p.applies_to_kind(&kind))
                    .collect();
                if fitting.is_empty() {
                    return None;
                }
                let chips = fitting.into_iter().map(|preset| {
                    let (plugin, id) = (preset.plugin_id.clone(), preset.preset_id.clone());
                    Button::new(SharedString::from(format!(
                        "anim-{}-{}",
                        preset.plugin_id, preset.preset_id
                    )))
                    .ghost()
                    .compact()
                    .label(preset.name.clone())
                    .tooltip(preset.description.clone())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.apply_animation_preset(&plugin, &id, cx);
                    }))
                });
                let role_key = role.as_str();
                Some(
                    div()
                        .px_2()
                        .py_1()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_1()
                                .child(
                                    div()
                                        .flex_1()
                                        .text_size(px(11.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(hex(color::TEXT_SECONDARY))
                                        .child(label),
                                )
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "anim-shorter-{role_key}"
                                    )))
                                    .ghost()
                                    .compact()
                                    .icon(icons::icon(IconName::Minus).xsmall())
                                    .tooltip("Shorter")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.step_preset_duration(role, -DURATION_STEP, cx);
                                    })),
                                )
                                .child(
                                    div()
                                        .min_w(px(64.))
                                        .text_size(px(11.))
                                        .text_color(hex(color::TEXT_MUTED))
                                        .child(self.duration_label(role)),
                                )
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "anim-longer-{role_key}"
                                    )))
                                    .ghost()
                                    .compact()
                                    .icon(icons::icon(IconName::Plus).xsmall())
                                    .tooltip("Longer")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.step_preset_duration(role, DURATION_STEP, cx);
                                    })),
                                ),
                        )
                        .child(div().flex().flex_row().flex_wrap().gap_1().children(chips)),
                )
            })
            .collect()
    }

    /// Effects-tab rows: one clickable entry per preset, grouped by role.
    pub(super) fn animation_library_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let presets = self.available_animation_presets();
        let mut rows = Vec::new();
        for (role, label) in [
            (PresetRole::Entrance, "Animations: In"),
            (PresetRole::Exit, "Animations: Out"),
            (PresetRole::Emphasis, "Animations: Emphasis"),
        ] {
            let group: Vec<_> = presets.iter().filter(|p| p.role == role).collect();
            if group.is_empty() {
                continue;
            }
            rows.push(
                div()
                    .px_2()
                    .pt_2()
                    .pb_1()
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(hex(color::TEXT_MUTED))
                    .child(label)
                    .into_any_element(),
            );
            for preset in group {
                let (plugin, id) = (preset.plugin_id.clone(), preset.preset_id.clone());
                rows.push(
                    div()
                        .px_2()
                        .py_1()
                        .cursor_pointer()
                        .hover(|this| this.bg(cx.theme().muted))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(icons::icon(IconName::WandSparkles).small())
                        .child(preset.name.clone())
                        .id(SharedString::from(format!(
                            "animation-item-{}-{}",
                            preset.plugin_id, preset.preset_id
                        )))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.apply_animation_preset(&plugin, &id, cx);
                        }))
                        .into_any_element(),
                );
            }
        }
        rows
    }
}
