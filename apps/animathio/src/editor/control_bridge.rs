//! Control bridge.

use super::*;

impl Editor {
    pub(super) fn drain_control(&mut self, cx: &mut Context<Self>) {
        loop {
            let next = {
                let Some(control) = self.control.as_mut() else {
                    return;
                };
                control.rx.try_recv()
            };
            let Ok(ControlMsg::Invoke(call, reply)) = next else {
                return;
            };
            self.finish_color_edit(cx);
            if let Call::AssetsImport { path } = &call {
                let path = path.clone();
                let expected_assets = self.assets.clone();
                let mut imported_assets = expected_assets.clone();
                cx.spawn(async move |this, cx| {
                    let result = cx
                        .background_spawn(async move {
                            let canonical = validate_existing_file(&path)?;
                            let bytes = read_bounded(
                                &canonical,
                                animathio_format::Limits::default().asset_bytes,
                            )?;
                            let info = AssetInfo {
                                id: AssetId::of(&bytes),
                                name: canonical.file_name().map_or_else(
                                    || "asset".to_owned(),
                                    |n| n.to_string_lossy().into_owned(),
                                ),
                                media_type: media_type_of(&canonical.to_string_lossy()).to_owned(),
                            };
                            Arc::make_mut(&mut imported_assets).insert(info.id.clone(), bytes);
                            Ok::<_, ControlError>((info, imported_assets))
                        })
                        .await;
                    let _ = this.update(cx, |editor, cx| {
                        let result = result.and_then(|(info, assets)| {
                            if !Arc::ptr_eq(&editor.assets, &expected_assets) {
                                return Err(ControlError::Other(
                                    "assets changed during import; retry the import".into(),
                                ));
                            }
                            editor.assets = assets;
                            editor.worker.load(editor.assets.clone());
                            Ok(serde_json::to_value(info)?)
                        });
                        let _ = reply.send(result);
                        cx.notify();
                    });
                })
                .detach();
                continue;
            }
            if let Call::ProjectSave { path } = &call {
                if self.save_pending {
                    let _ =
                        reply.send(Err(ControlError::Other("a save is already running".into())));
                    continue;
                }
                self.save_pending = true;
                let target = path.clone().or_else(|| self.path.clone());
                let project = self.project.clone();
                let assets = self.assets.clone();
                let generation = self.audio_generation;
                let original_path = self.path.clone();
                cx.spawn(async move |this, cx| {
                    let result = cx
                        .background_spawn(async move {
                            let target = target.ok_or_else(|| {
                                ControlError::Unsupported("project has never been saved".into())
                            })?;
                            let target = validate_output_file(&target)?;
                            write_project(&target, &project, &assets)
                                .map_err(ControlError::Other)?;
                            Ok::<_, ControlError>(target)
                        })
                        .await;
                    let _ = this.update(cx, |editor, cx| {
                        editor.save_pending = false;
                        let result = result.and_then(|target| {
                            if editor.path == original_path {
                                editor.path = Some(target.clone());
                            }
                            if generation == editor.audio_generation {
                                editor.dirty = false;
                            }
                            editor.recent.add(&target);
                            Ok(serde_json::to_value(target.display().to_string())?)
                        });
                        let _ = reply.send(result);
                        cx.notify();
                    });
                })
                .detach();
                continue;
            }
            if let Call::RenderFrame { .. } = &call {
                let project = self.project.clone();
                let assets = self.assets.clone();
                std::thread::spawn(move || {
                    use animathio_control::ControlHandler;
                    let mut handler = animathio_control::HeadlessHandler::from_bundle(
                        animathio_format::Bundle {
                            project,
                            assets: (*assets).clone(),
                        },
                        None,
                    );
                    let _ = reply.send(handler.handle(call));
                });
                continue;
            }
            if let Call::SubtitlesGenerate { params } = &call {
                // Long job: transcribe on a worker and answer when it has finished.
                let label = format!("{} model (control request)", params.model);
                if let Err(error) =
                    self.start_subtitle_job(params.clone(), label, Some(reply.clone()), cx)
                {
                    let _ = reply.send(Err(ControlError::Other(error)));
                }
                continue;
            }
            if let Call::Export { params } = call {
                if self.export_queue.is_busy() {
                    let _ = reply.send(Err(ControlError::Other(
                        "an export is already running".into(),
                    )));
                } else {
                    // Control exports show up in the queue like any other job.
                    let spec = crate::export_queue::JobSpec {
                        project: self.project.clone(),
                        assets: self.assets.clone(),
                        summary: "control request".into(),
                        params: params.clone(),
                    };
                    match crate::export_worker::ExportTask::spawn(
                        spec.project.clone(),
                        spec.assets.clone(),
                        params,
                        Some(reply.clone()),
                    ) {
                        Ok(task) => {
                            self.export_queue.adopt_running(&spec, task);
                        }
                        Err(error) => {
                            let _ = reply.send(Err(ControlError::Other(error)));
                        }
                    }
                }
                cx.notify();
                continue;
            }
            let result = self.handle_control_call(call, cx);
            let _ = reply.send(result);
        }
    }

    pub(super) fn handle_control_call(
        &mut self,
        call: Call,
        cx: &mut Context<Self>,
    ) -> Result<Value, ControlError> {
        self.finish_color_edit(cx);
        Ok(match call {
            Call::Export { .. } => {
                return Err(ControlError::Other("export must run on its worker".into()));
            }
            Call::ProjectGet => serde_json::to_value(ProjectState {
                project: self.project.clone(),
                path: self.path.as_ref().map(|p| p.display().to_string()),
                playhead: self.position,
                playing: self.playing,
            })?,
            Call::ProjectOpen { path } => {
                self.error = None;
                self.open_path(&path, cx);
                if self.path.as_deref() != Some(path.as_path()) {
                    return Err(ControlError::Other(
                        self.error
                            .take()
                            .unwrap_or_else(|| "open failed".to_owned()),
                    ));
                }
                serde_json::to_value(())?
            }
            Call::ProjectSave { path } => {
                let target = path.or_else(|| self.path.clone()).ok_or_else(|| {
                    ControlError::Unsupported("project has never been saved".to_owned())
                })?;
                let target = validate_output_file(&target)?;
                write_project(&target, &self.project, &self.assets).map_err(ControlError::Other)?;
                self.path = Some(target.clone());
                self.dirty = false;
                self.last_autosave = Instant::now();
                self.recent.add(&target);
                serde_json::to_value(target.display().to_string())?
            }
            Call::CommandApply { command } => {
                self.check_effect_params(&command)
                    .map_err(ControlError::InvalidParams)?;
                self.history
                    .execute(&mut self.project, &command)
                    .map_err(|error| ControlError::Other(error.0))?;
                self.after_edit(cx);
                serde_json::to_value(())?
            }
            Call::TitlesList => serde_json::to_value(animathio_control::title_presets())?,
            Call::TitlesAdd { params } => {
                let (command, added) =
                    animathio_control::title_preset_command(&self.project, &params)?;
                self.finish_color_edit(cx);
                self.history.execute(&mut self.project, &command)?;
                self.after_edit(cx);
                serde_json::to_value(added)?
            }
            Call::EffectsList => {
                serde_json::to_value(animathio_control::effect_infos(self.available_effects()))?
            }
            Call::EffectsAdd { params } => {
                let (command, added) = animathio_control::add_effect_command(
                    &self.project,
                    &self.available_effects(),
                    &params,
                )?;
                self.history
                    .execute(&mut self.project, &command)
                    .map_err(|error| ControlError::Other(error.0))?;
                self.after_edit(cx);
                serde_json::to_value(added)?
            }
            Call::AudioEffectsList => serde_json::to_value(animathio_control::effect_infos(
                self.available_audio_effects(),
            ))?,
            Call::AudioEffectsAdd { params } => {
                let (command, added) = animathio_control::add_audio_effect_command(
                    &self.project,
                    &self.available_audio_effects(),
                    &params,
                )?;
                self.history
                    .execute(&mut self.project, &command)
                    .map_err(|error| ControlError::Other(error.0))?;
                self.after_edit(cx);
                serde_json::to_value(added)?
            }
            Call::HistoryUndo => {
                let undone = self
                    .history
                    .undo(&mut self.project)
                    .map_err(|error| ControlError::Other(error.0))?;
                if undone {
                    self.after_edit(cx);
                }
                serde_json::to_value(undone)?
            }
            Call::HistoryRedo => {
                let redone = self
                    .history
                    .redo(&mut self.project)
                    .map_err(|error| ControlError::Other(error.0))?;
                if redone {
                    self.after_edit(cx);
                }
                serde_json::to_value(redone)?
            }
            Call::RenderFrame { params } => {
                serde_json::to_value(self.render_control_frame(params)?)?
            }
            Call::PlaybackSeek { time } => {
                self.seek_to(time, cx);
                serde_json::to_value(())?
            }
            Call::PlaybackPlay => {
                let _ = self.clock.set_rate(Speed::NORMAL);
                self.shuttle = Shuttle(1, false);
                self.playing = true;
                if let Err(error) = self.clock.play() {
                    self.error = Some(error);
                }
                cx.notify();
                serde_json::to_value(())?
            }
            Call::PlaybackPause => {
                self.shuttle = Shuttle::default();
                self.playing = false;
                if let Err(error) = self.clock.pause() {
                    self.error = Some(error);
                }
                cx.notify();
                serde_json::to_value(())?
            }
            Call::AssetsImport { path } => serde_json::to_value(self.import_control_asset(&path)?)?,
            Call::TimelineList => serde_json::to_value(timeline_of(&self.project))?,
            Call::ProjectImportV1 { path } => {
                self.import_control_v1(&path)?;
                self.after_edit(cx);
                serde_json::to_value(())?
            }
            Call::TrimSilence { params } => {
                let result = animathio_control::trim_silence(
                    &mut self.project,
                    &mut self.history,
                    &self.assets,
                    &params,
                )?;
                self.after_edit(cx);
                serde_json::to_value(result)?
            }
            Call::SubtitlesGenerate { .. } => {
                return Err(ControlError::Other(
                    "subtitle generation is handled asynchronously".into(),
                ));
            }
            Call::SubtitlesExport { params } => {
                serde_json::to_value(animathio_control::export_subtitles(&self.project, &params)?)?
            }
            Call::SubtitlesImport { params } => {
                let result = animathio_control::import_subtitles(
                    &mut self.project,
                    &mut self.history,
                    &params,
                )?;
                self.after_edit(cx);
                serde_json::to_value(result)?
            }
            call @ (Call::MathAdd { .. } | Call::GraphAdd { .. } | Call::ImportManim { .. }) => {
                let replace = matches!(&call, Call::ImportManim { params } if params.replace);
                let importing = matches!(&call, Call::ImportManim { .. });
                let result = animathio_control::apply_authoring_call(
                    &mut self.project,
                    &mut self.history,
                    Arc::make_mut(&mut self.assets),
                    &call,
                )?;
                if replace {
                    self.marks = InOut::default();
                    self.shuttle = Shuttle::default();
                    self.path = None;
                    self.clear_selection();
                    self.selected_field = None;
                    self.seek_to(Time(0), cx);
                }
                self.next_id = next_id(&self.project);
                if importing {
                    self.worker.load(self.assets.clone());
                }
                self.after_edit(cx);
                result
            }
        })
    }

    pub(super) fn render_control_frame(
        &mut self,
        params: RenderFrameParams,
    ) -> Result<RenderFrameResult, ControlError> {
        let mut ctx = animathio_render::RenderCtx::from_assets((*self.assets).clone())?;
        let scene = animathio_core::evaluate(&self.project, params.time);
        let scene = self.resolve_scene(scene, (params.width, params.height));
        if let Some(host) = &self.plugin_host {
            ctx.set_effect_processor(host.effect_processor());
        }
        let frame = animathio_render::render(&scene, &mut ctx, (params.width, params.height))?;
        if let Some(output) = params.output {
            let canonical = validate_output_file(&output)?;
            frame.save_png(&canonical)?;
            Ok(RenderFrameResult {
                size: frame.size,
                base64: None,
                path: Some(canonical.display().to_string()),
            })
        } else {
            let mut encoded = Vec::new();
            frame.write_png(&mut encoded)?;
            Ok(RenderFrameResult {
                size: frame.size,
                base64: Some(base64::engine::general_purpose::STANDARD.encode(encoded)),
                path: None,
            })
        }
    }

    pub(super) fn import_control_asset(&mut self, path: &Path) -> Result<AssetInfo, ControlError> {
        let canonical = validate_existing_file(path)?;
        let bytes = read_bounded(&canonical, animathio_format::Limits::default().asset_bytes)?;
        let id = AssetId::of(&bytes);
        let name = canonical
            .file_name()
            .map_or_else(|| "asset".to_owned(), |n| n.to_string_lossy().into_owned());
        Arc::make_mut(&mut self.assets).insert(id.clone(), bytes);
        self.worker.load(self.assets.clone());
        Ok(AssetInfo {
            id,
            media_type: media_type_of(&name).to_owned(),
            name,
        })
    }

    pub(super) fn import_control_v1(&mut self, path: &Path) -> Result<(), ControlError> {
        let canonical = validate_existing_file(path)?;
        let bytes = read_bounded(
            &canonical,
            animathio_import_v1::ImportLimits::default().inflated_bytes,
        )?;
        let (project, assets, report) =
            animathio_import_v1::import_v1_with_report(&bytes).map_err(ControlError::Import)?;
        for warning in report.warnings() {
            eprintln!("warning: {warning}");
        }
        let _ = self.clock.pause();
        self.marks = InOut::default();
        self.shuttle = Shuttle::default();
        self.project = project;
        self.history = History::default();
        self.assets = Arc::new(assets.into_iter().map(|a| (a.id, a.bytes)).collect());
        self.path = None;
        self.position = Time(0);
        self.playing = false;
        self.clear_selection();
        self.selected_field = None;
        self.pool = AudioPool::default();
        if let Err(error) = load_audio(&mut self.pool, &self.project, &self.assets) {
            self.error = Some(error);
        }
        self.audio_ids = audio_asset_ids(&self.project);
        self.clock = make_clock(&self.project, &self.pool);
        self.next_id = next_id(&self.project);
        self.dirty = true;
        self.worker.load(self.assets.clone());
        Ok(())
    }
}
