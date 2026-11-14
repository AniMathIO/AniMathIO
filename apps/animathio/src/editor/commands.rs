//! Commands.

use super::*;

impl Editor {
    pub(super) fn current_layout(&self) -> TimelineLayout {
        let width = f32::from(self.timeline_bounds.size.width).max(1.0);
        TimelineLayout::zoomed(
            width,
            RULER_H,
            self.dock.track_height.pixels(),
            timeline::timeline_span(&self.project),
            self.timeline_zoom,
            self.timeline_scroll,
        )
        .with_frame_rate(self.project.frame_rate())
    }

    pub(super) fn timeline_relative(&self, position: Point<Pixels>) -> (f32, f32) {
        (
            f32::from(position.x) - f32::from(self.timeline_bounds.origin.x),
            f32::from(position.y) - f32::from(self.timeline_bounds.origin.y),
        )
    }

    pub(super) fn find_clip(&self, id: ClipId) -> Option<&Clip> {
        self.project
            .tracks()
            .iter()
            .flat_map(|t| t.clips().values())
            .find(|c| c.id == id)
    }

    pub(super) fn current_render_time(&self) -> Time {
        if self.playing {
            due_frame(&self.project, self.clock.now()).unwrap_or(self.position)
        } else {
            self.position
        }
    }

    pub(super) fn next_clip_id(&mut self) -> ClipId {
        let id = ClipId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    pub(super) fn execute(&mut self, command: &Command, cx: &mut Context<Self>) {
        self.try_execute(command, cx);
    }

    /// Reject commands that store effect parameters outside the plugin's declared range.
    pub(super) fn check_effect_params(&self, command: &Command) -> Result<(), String> {
        if !animathio_plugin_host::mentions_effect_params(command) {
            return Ok(());
        }
        match &self.plugin_host {
            Some(host) => host.validate_effect_command(&self.project, command),
            None => Ok(()),
        }
    }

    /// [`Editor::execute`], reporting whether the command was applied.
    pub(super) fn try_execute(&mut self, command: &Command, cx: &mut Context<Self>) -> bool {
        self.finish_color_edit(cx);
        let checked = self
            .check_effect_params(command)
            .map_err(animathio_core::Error);
        match checked.and_then(|()| self.history.execute(&mut self.project, command)) {
            Ok(()) => {
                self.after_edit(cx);
                true
            }
            Err(error) => {
                self.error = Some(error.0);
                cx.notify();
                false
            }
        }
    }

    pub(super) fn after_edit(&mut self, cx: &mut Context<Self>) {
        self.invalidate_autocut();
        self.invalidate_duck();
        self.invalidate_beats();
        self.dirty = true;
        self.next_id = self.next_id.max(next_id(&self.project));
        self.prune_selection();
        self.sync_audio(cx);
        let time = self.current_render_time();
        self.request_render(time);
        cx.notify();
    }

    /// Keep the engine-owned audio pool and the player's snapshots in lock-step
    /// with the project. The project snapshot is swapped every edit; the mixer is
    /// only rebuilt when the set of decoded audio assets actually changed.
    pub(super) fn sync_audio(&mut self, cx: &mut Context<Self>) {
        self.audio_generation = self.audio_generation.wrapping_add(1);
        let generation = self.audio_generation;
        let live = audio_asset_ids(&self.project);
        let Some((rate, channels)) = self.clock.mixer_format() else {
            self.clock.set_project(Arc::new(self.monitor_project()));
            return;
        };
        if live == self.audio_ids {
            self.clock.set_project(Arc::new(self.monitor_project()));
            return;
        }
        let project = self.project.clone();
        let assets = self.assets.clone();
        let mut pool = self.pool.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    load_audio(&mut pool, &project, &assets)?;
                    let mixer = crate::playback::with_audio_effects(
                        pool.prepare(rate, channels).map_err(|e| e.to_string())?,
                    );
                    Ok::<_, String>((pool, mixer))
                })
                .await;
            let _ = this.update(cx, |editor, cx| {
                if editor.audio_generation != generation {
                    return;
                }
                match result {
                    Ok((pool, mixer)) => {
                        editor.pool = pool;
                        editor.pool.collect(&editor.project);
                        editor.clock.set_mixer(Arc::new(mixer));
                        editor.clock.set_project(Arc::new(editor.monitor_project()));
                        editor.audio_ids = live;
                        editor.waveforms.borrow_mut().clear();
                    }
                    Err(error) => editor.error = Some(format!("Audio preparation failed: {error}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn autosave_tick(&mut self, cx: &mut Context<Self>) {
        if !self.dirty
            || self.path.is_none()
            || self.preferences.autosave_seconds == 0
            || self.last_autosave.elapsed() < Duration::from_secs(self.preferences.autosave_seconds)
        {
            return;
        }
        let path = autosave_path(self.path.as_ref().expect("checked above"));
        self.save_background(path, true, cx);
        cx.notify();
    }

    pub(super) fn commit_input_value(
        &mut self,
        target: InputTarget,
        buffer: String,
        cx: &mut Context<Self>,
    ) {
        match target {
            InputTarget::Timecode => {
                if let Some(time) = timecode::parse_timecode(&buffer, self.project.frame_rate()) {
                    self.seek_to(time, cx);
                } else {
                    self.error = Some("Invalid timecode (HH:MM:SS:FF)".into());
                }
            }
            InputTarget::Field { id, field } => match buffer.trim().parse::<f64>() {
                Ok(value) => self.commit_field(id, &field, value, cx),
                Err(_) => self.error = Some("invalid number".into()),
            },
            InputTarget::SelectionField { field, ids } => match buffer.trim().parse::<f64>() {
                Ok(value) => self.commit_selection_field(&field, &ids, value, cx),
                Err(_) => self.error = Some("invalid number".into()),
            },
            InputTarget::AudioEffectParam {
                chain,
                effect,
                name,
            } => match buffer.trim().parse::<f64>() {
                Ok(value) => {
                    if let Some(command) =
                        self.audio_effect_chain_param_command(chain, effect, &name, value)
                    {
                        self.execute(&command, cx);
                    }
                }
                Err(_) => self.error = Some("invalid number".into()),
            },
            InputTarget::PluginNumber { id, name } => {
                let Ok(value) = buffer.trim().parse::<f64>() else {
                    self.error = Some("invalid number".into());
                    cx.notify();
                    return;
                };
                if !value.is_finite() {
                    self.error = Some("non-finite value".into());
                    cx.notify();
                    return;
                }
                let value = self
                    .plugin_model(id, &name)
                    .and_then(|model| match model {
                        ParamModel::Number { min, max, .. } => Some(value.clamp(min, max)),
                        _ => None,
                    })
                    .unwrap_or(value);
                if let Some(command) = self.plugin_number_command(id, &name, value) {
                    self.execute(&command, cx);
                }
            }
            InputTarget::PluginText { id, name } => {
                let value = buffer;
                self.execute_plugin_param(id, name, json!(value), cx);
            }
            InputTarget::PluginPoint { id, name, axis } => {
                let Ok(value) = buffer.trim().parse::<f64>() else {
                    self.error = Some("invalid number".into());
                    cx.notify();
                    return;
                };
                let mut point = self.plugin_point_value(id, &name);
                point[axis] = value as f32;
                self.execute_plugin_param(id, name, json!([point[0], point[1]]), cx);
            }
        }
        cx.notify();
    }

    pub(super) fn plugin_number_command(
        &self,
        id: ClipId,
        name: &str,
        value: f64,
    ) -> Option<Command> {
        let value = self
            .plugin_model(id, name)
            .and_then(|model| match model {
                ParamModel::Number { min, max, .. } => Some(value.clamp(min, max)),
                _ => None,
            })
            .unwrap_or(value);
        let clip = self.find_clip(id)?;
        Some(inspector::numeric_edit_command(
            clip,
            Target::Param(name.to_owned()),
            value,
            self.position,
        ))
    }

    pub(super) fn plugin_number_value(&self, id: ClipId, name: &str) -> f64 {
        self.plugin_opaque(id, name)
            .and_then(|value| value.as_f64())
            .unwrap_or(0.0)
    }

    pub(super) fn plugin_model(&self, id: ClipId, name: &str) -> Option<ParamModel> {
        let clip = self.find_clip(id)?;
        let ClipKind::Plugin { plugin_id, .. } = &clip.kind else {
            return None;
        };
        self.plugins
            .iter()
            .find(|plugin| &plugin.id == plugin_id)?
            .models()
            .into_iter()
            .find(|model| model.id() == name)
    }

    /// The edit setting plugin parameter `name` of clip `id`, or `None` when the
    /// clip or its schema parameter is gone (an input can commit on blur after
    /// an undo, a control edit or a project switch removed its clip).
    pub(super) fn plugin_param_command(
        &self,
        id: ClipId,
        name: String,
        json: Value,
    ) -> Option<Command> {
        let model = self.plugin_model(id, &name)?;
        let value = inspector::model_value(&model, Some(&json));
        Some(inspector::param_edit_command(
            self.find_clip(id)?,
            name,
            value,
            self.position,
        ))
    }

    /// Run a plugin parameter edit if its clip still exists.
    pub(super) fn execute_plugin_param(
        &mut self,
        id: ClipId,
        name: String,
        json: Value,
        cx: &mut Context<Self>,
    ) {
        if let Some(command) = self.plugin_param_command(id, name, json) {
            self.execute(&command, cx);
        }
    }

    pub(super) fn plugin_opaque(&self, id: ClipId, name: &str) -> Option<Value> {
        let clip = self.find_clip(id)?;
        let ClipKind::Plugin { params, .. } = &clip.kind else {
            return None;
        };
        let model = self.plugin_model(id, name)?;
        let base = inspector::model_value(&model, params.get(name));
        Some(inspector::sampled_param(clip, name, self.position, base).to_plugin_json())
    }

    pub(super) fn plugin_text_value(&self, id: ClipId, name: &str, default: &str) -> String {
        self.plugin_opaque(id, name)
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| default.to_owned())
    }

    pub(super) fn plugin_bool_value(&self, id: ClipId, name: &str, default: bool) -> bool {
        self.plugin_opaque(id, name)
            .and_then(|value| value.as_bool())
            .unwrap_or(default)
    }

    pub(super) fn plugin_color_value(&self, id: ClipId, name: &str, default: [u8; 4]) -> [u8; 4] {
        self.plugin_opaque(id, name)
            .and_then(|value| color_from_json(&value))
            .unwrap_or(default)
    }

    pub(super) fn plugin_point_value(&self, id: ClipId, name: &str) -> [f32; 2] {
        self.plugin_opaque(id, name)
            .and_then(|value| point_from_json(&value))
            .unwrap_or([0.0, 0.0])
    }

    pub(super) fn plugin_asset_value(&self, id: ClipId, name: &str) -> Option<String> {
        self.plugin_opaque(id, name).and_then(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .filter(|value| self.project.assets().contains_key(&asset_id_from(value)))
        })
    }

    pub(super) fn commit_field(
        &mut self,
        id: ClipId,
        field: &Field,
        value: f64,
        cx: &mut Context<Self>,
    ) {
        if !value.is_finite() {
            self.error = Some("non-finite value".into());
            return;
        }
        let value = match field {
            Field::Transform(TransformProperty::Opacity) => value.clamp(0.0, 1.0),
            _ => value,
        };
        let Some(command) = self.field_command(id, field, value) else {
            return;
        };
        self.execute(&command, cx);
    }

    /// Apply one numeric field to every clip of `ids` as a single undo step.
    pub(super) fn commit_selection_field(
        &mut self,
        field: &Field,
        ids: &BTreeSet<ClipId>,
        value: f64,
        cx: &mut Context<Self>,
    ) {
        if !value.is_finite() {
            self.error = Some("non-finite value".into());
            return;
        }
        let value = match field {
            Field::Transform(TransformProperty::Opacity) => value.clamp(0.0, 1.0),
            _ => value,
        };
        let commands: Vec<Command> = ids
            .iter()
            .filter_map(|id| self.field_command(*id, field, value))
            .collect();
        if let Some(command) = selection::batch(commands) {
            self.execute(&command, cx);
        }
    }

    /// The edit for a typed or stepped field value. Like scrubbing, an animated
    /// property gets a keyframe at the playhead; setting only the base value
    /// would leave the displayed (sampled) value unchanged.
    pub(super) fn field_command(&self, id: ClipId, field: &Field, value: f64) -> Option<Command> {
        let value = field_model_value(field, value);
        let clip = self.find_clip(id)?;
        let target = match field {
            Field::Transform(property) => Target::Transform(*property),
            Field::Param(name) => Target::Param(name.clone()),
        };
        // Outside the clip there is no "value at the playhead" to key; set the
        // base value instead of adding a key before or after the clip.
        let inside = clip.range.start <= self.position && self.position < clip.range.end;
        if !inside {
            return Some(match field {
                Field::Transform(property) => Command::SetTransform {
                    id,
                    transform: inspector::with_transform(clip.transform, *property, value),
                },
                Field::Param(name) => Command::SetParam {
                    id,
                    name: name.clone(),
                    value: Some(value),
                },
            });
        }
        Some(inspector::numeric_edit_command(
            clip,
            target,
            value,
            self.position,
        ))
    }

    pub(super) fn split_clip(&mut self, id: ClipId, cx: &mut Context<Self>) {
        let Some(clip) = self.find_clip(id).cloned() else {
            return;
        };
        let position = self.position;
        if position <= clip.range.start || position >= clip.range.end {
            self.error = Some("playhead is outside the clip".into());
            cx.notify();
            return;
        }
        let right_id = self.next_clip_id();
        self.execute(
            &Command::SplitClip {
                id,
                right_id,
                at: position,
            },
            cx,
        );
    }

    /// Set one fade of a clip to `ticks` (0 clears it) as a single undo step.
    pub(super) fn set_fade(
        &mut self,
        id: ClipId,
        edge: FadeEdge,
        ticks: i64,
        cx: &mut Context<Self>,
    ) {
        if let Some(command) = self
            .find_clip(id)
            .and_then(|clip| fades::fade_command(clip, edge, ticks))
        {
            self.execute(&command, cx);
        }
    }

    pub(super) fn fade_in_one_second(&mut self, id: ClipId, cx: &mut Context<Self>) {
        self.set_fade(id, FadeEdge::In, TICKS_PER_SECOND, cx);
    }

    pub(super) fn fade_out_one_second(&mut self, id: ClipId, cx: &mut Context<Self>) {
        self.set_fade(id, FadeEdge::Out, TICKS_PER_SECOND, cx);
    }

    /// Remove both fades as one undo step.
    pub(super) fn clear_fades(&mut self, id: ClipId, cx: &mut Context<Self>) {
        if let Some(command) = fades::clear_fades_command(&self.project, id) {
            self.execute(&command, cx);
        }
    }

    pub(super) fn undo(&mut self, cx: &mut Context<Self>) {
        self.finish_color_edit(cx);
        match self.history.undo(&mut self.project) {
            Ok(true) => self.after_edit(cx),
            Ok(false) => {}
            Err(error) => {
                self.error = Some(error.0);
                cx.notify();
            }
        }
    }

    pub(super) fn redo(&mut self, cx: &mut Context<Self>) {
        self.finish_color_edit(cx);
        match self.history.redo(&mut self.project) {
            Ok(true) => self.after_edit(cx),
            Ok(false) => {}
            Err(error) => {
                self.error = Some(error.0);
                cx.notify();
            }
        }
    }

    /// Split every selected clip under the playhead (or, with no selection, the
    /// first clip under it) as one undo step.
    pub(super) fn split_at_playhead(&mut self, cx: &mut Context<Self>) {
        let position = self.position;
        let straddles = |clip: &Clip| clip.range.start < position && position < clip.range.end;
        let selected = self.selected_ids();
        let mut targets: Vec<ClipId> = selected
            .iter()
            .copied()
            .filter(|id| self.find_clip(*id).is_some_and(straddles))
            .collect();
        if targets.is_empty() {
            targets.extend(
                self.project
                    .tracks()
                    .iter()
                    .flat_map(|t| t.clips().values())
                    .find(|c| straddles(c))
                    .map(|c| c.id),
            );
        }
        // Core splits a clip's linked companions with it, so split each group once.
        let targets = selection::one_per_link_group(&self.project, &targets);
        let (commands, next) =
            selection::split_commands(&self.project, &targets, position, self.next_id);
        self.next_id = next;
        if let Some(command) = selection::batch(commands) {
            self.execute(&command, cx);
        }
    }
}

impl Editor {
    pub(super) fn unlink_clip(&mut self, id: ClipId, cx: &mut Context<Self>) {
        let group = self
            .project
            .tracks()
            .iter()
            .flat_map(|t| t.clips().values())
            .find(|c| c.id == id)
            .and_then(|c| c.link_group);
        if let Some(group) = group {
            let commands = self
                .project
                .tracks()
                .iter()
                .flat_map(|t| t.clips().values())
                .filter(|c| c.link_group == Some(group))
                .map(|c| Command::SetClipLink {
                    id: c.id,
                    group: None,
                })
                .collect();
            self.execute(&Command::Batch { commands }, cx);
        }
    }
}
