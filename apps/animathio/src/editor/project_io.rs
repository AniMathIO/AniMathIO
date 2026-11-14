//! Project io.

use super::*;
use animathio_media::MediaBackend;

impl Editor {
    /// Drop every piece of view state tied to the project that was just replaced:
    /// pending gestures and drags, inline renames, inspector inputs (whose blur
    /// would otherwise commit into the new project), the selection, autocut
    /// review and cached waveforms. Bumps the session so background results
    /// started on the old project are discarded.
    pub(super) fn reset_project_session(&mut self) {
        self.project_session = self.project_session.wrapping_add(1);
        self.color_edit = None;
        self.last_live_audio_sync = None;
        self.marks = InOut::default();
        self.shuttle = Shuttle::default();
        self.autocut_review = None;
        self.duck_review = None;
        self.marker_panel = None;
        self.reframe_panel = None;
        self.track_rename = None;
        self.gesture = None;
        self.keyframe_drag = None;
        self.gain_drag = None;
        self.context_menu = None;
        self.pool_drag = None;
        self.hovered_clip = None;
        self.snap_line = None;
        self.failed_effect_clips.clear();
        self.end_gizmo();
        self.fields = fields::FieldPool::default();
        self.waveforms.borrow_mut().clear();
        self.audio_inspector_track = None;
        self.selected = None;
        self.selection.clear();
        self.selected_field = None;
        self.selected_keyframe = None;
    }

    pub(super) fn new_project(&mut self, cx: &mut Context<Self>) {
        self.finish_color_edit(cx);
        let _ = self.clock.pause();
        self.reset_project_session();
        self.project = Project::new(self.project.frame_rate());
        Command::AddTrack {
            track: Track::new(TrackId(1), "Video 1"),
            index: 0,
        }
        .apply(&mut self.project)
        .expect("new project's first track");
        self.history = History::default();
        Arc::make_mut(&mut self.assets).clear();
        self.audio_generation = self.audio_generation.wrapping_add(1);
        self.pool = AudioPool::default();
        self.audio_ids.clear();
        self.clock = make_clock(&self.project, &self.pool);
        self.path = None;
        self.dirty = false;
        self.position = Time(0);
        self.playing = false;
        self.next_id = next_id(&self.project);
        self.worker.load(self.assets.clone());
        self.request_render(Time(0));
        cx.notify();
    }

    /// A fresh project whose canvas is `preset`. The preset is part of the new
    /// project, not an edit: it is not on the undo stack and leaves it clean.
    pub(super) fn new_project_with_preset(
        &mut self,
        preset: animathio_control::ExportPreset,
        cx: &mut Context<Self>,
    ) {
        self.new_project(cx);
        match preset
            .command(&self.project)
            .map_err(|error| error.to_string())
            .and_then(|command| command.apply(&mut self.project).map_err(|error| error.0))
        {
            Ok(_) => {
                self.sync_audio(cx);
                self.request_render(Time(0));
            }
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }

    pub(super) fn save(&mut self, cx: &mut Context<Self>) {
        let path = if let Some(path) = &self.path {
            path.clone()
        } else {
            let Some(path) = rfd::FileDialog::new()
                .add_filter("AniMathIO project", &["amio"])
                .set_file_name("project.amio")
                .save_file()
            else {
                return;
            };
            path
        };
        self.save_background(path, false, cx);
    }

    pub(super) fn save_background(
        &mut self,
        path: PathBuf,
        autosave: bool,
        cx: &mut Context<Self>,
    ) {
        if self.save_pending {
            if !autosave {
                self.error = Some("a save is already running".into());
            }
            return;
        }
        self.save_pending = true;
        let project = self.project.clone();
        let assets = self.assets.clone();
        let generation = self.audio_generation;
        let original_path = self.path.clone();
        self.last_autosave = Instant::now();
        cx.spawn(async move |this, cx| {
            let destination = path.clone();
            let result = cx
                .background_spawn(async move { write_project(&destination, &project, &assets) })
                .await;
            let _ = this.update(cx, |editor, cx| {
                editor.save_pending = false;
                match result {
                    Ok(()) if !autosave => {
                        if editor.path == original_path {
                            editor.path = Some(path.clone());
                        }
                        if editor.audio_generation == generation {
                            editor.dirty = false;
                        }
                        editor.recent.add(&path);
                        editor
                            .notices
                            .push(crate::style::Toast::success("Project saved"));
                    }
                    Ok(()) => {}
                    Err(error) => editor.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn open(&mut self, cx: &mut Context<Self>) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AniMathIO project", &["amio"])
            .pick_file()
        else {
            return;
        };
        self.open_path(&path, cx);
    }

    pub(super) fn open_path(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.try_open_path(path, cx);
    }

    /// [`Editor::open_path`], reporting whether the project was opened.
    pub(super) fn try_open_path(&mut self, path: &Path, cx: &mut Context<Self>) -> bool {
        self.finish_color_edit(cx);
        let opened = match read_project_with_recovery(path) {
            Ok((project, assets, recovered)) => {
                let _ = self.clock.pause();
                self.audio_generation = self.audio_generation.wrapping_add(1);
                self.reset_project_session();
                self.project = project;
                self.history = History::default();
                self.assets = Arc::new(assets.clone());
                self.path = Some(path.to_path_buf());
                self.dirty = recovered;
                self.last_autosave = Instant::now();
                self.pool = AudioPool::default();
                if let Err(error) = load_audio(&mut self.pool, &self.project, &self.assets) {
                    self.error = Some(error);
                }
                self.audio_ids = audio_asset_ids(&self.project);
                self.clock = make_clock(&self.project, &self.pool);
                self.position = Time(0);
                self.playing = false;
                self.next_id = next_id(&self.project);
                self.worker.load(Arc::new(assets));
                self.request_render(Time(0));
                self.recent.add(path);
                if recovered {
                    self.notices
                        .push(crate::style::Toast::info("Recovered from autosave"));
                }
                true
            }
            Err(error) => {
                self.error = Some(error);
                false
            }
        };
        cx.notify();
        opened
    }

    pub(super) fn import_media(&mut self, cx: &mut Context<Self>) {
        let Some(paths) = rfd::FileDialog::new()
            .add_filter(
                "Media",
                &[
                    "png", "jpg", "jpeg", "webp", "bmp", "mp3", "wav", "flac", "aac", "m4a", "ogg",
                    "mp4", "mov", "mkv", "webm",
                ],
            )
            .pick_files()
        else {
            return;
        };
        let track = self
            .project
            .tracks()
            .first()
            .map_or(TrackId(self.next_id), Track::id);
        self.import_paths(&paths, Some((track, self.position)), cx);
    }

    pub(super) fn import_paths(
        &mut self,
        paths: &[PathBuf],
        placement: Option<(TrackId, Time)>,
        cx: &mut Context<Self>,
    ) {
        self.commit_pending_inspector_inputs(cx);
        let (command, imported, errors) = Self::prepare_import(
            &self.project,
            &mut self.pool,
            &mut self.next_id,
            paths,
            placement,
        );
        if let Some(error) = errors.last() {
            self.error = Some(error.clone());
        }
        if !imported.is_empty() {
            let result = if matches!(&command, Command::Batch { commands } if commands.is_empty()) {
                Ok(())
            } else {
                self.history.execute(&mut self.project, &command)
            };
            match result {
                Ok(()) => {
                    Arc::make_mut(&mut self.assets).extend(imported);
                    self.worker.load(self.assets.clone());
                    self.after_edit(cx);
                }
                Err(error) => self.error = Some(error.0),
            }
        }
        cx.notify();
    }

    /// Plan every successful file against a scratch project, then commit once.
    #[allow(clippy::type_complexity)]
    fn prepare_import(
        project: &Project,
        pool: &mut AudioPool,
        next: &mut u64,
        paths: &[PathBuf],
        placement: Option<(TrackId, Time)>,
    ) -> (Command, BTreeMap<AssetId, Vec<u8>>, Vec<String>) {
        let mut draft = project.clone();
        let mut commands = Vec::new();
        let mut imported = BTreeMap::new();
        let mut errors = Vec::new();
        if let Some((track, _)) = placement
            && !draft.tracks().iter().any(|t| t.id() == track)
        {
            let command = Command::AddTrack {
                track: Track::new(track, "Video 1"),
                index: draft.tracks().len(),
            };
            if let Err(error) = command.apply(&mut draft) {
                return (Command::Batch { commands: vec![] }, imported, vec![error.0]);
            }
            *next = (*next).max(track.0.saturating_add(1));
            commands.push(command);
        }
        let mut placement = placement;
        for path in paths {
            match Self::prepare_media_file(&draft, pool, next, path, placement) {
                Ok((end, command, id, bytes)) => {
                    if let Err(error) = command.apply(&mut draft) {
                        errors.push(error.0);
                        continue;
                    }
                    if !matches!(&command, Command::Batch { commands } if commands.is_empty()) {
                        commands.push(command);
                    }
                    *next = (*next).max(next_id(&draft));
                    imported.insert(id, bytes);
                    if let Some((track, _)) = placement {
                        placement = Some((track, end));
                    }
                }
                Err(error) => errors.push(error),
            }
        }
        if imported.is_empty() {
            commands.clear();
        }
        (Command::Batch { commands }, imported, errors)
    }

    /// Import an asset, optionally placing a clip, as one undoable transaction. Audio
    /// is decoded up front (unsupported audio falls back to a silent source); the
    /// actual clip length is taken from decoded audio when available.
    #[allow(clippy::type_complexity)]
    fn prepare_media_file(
        project: &Project,
        pool: &mut AudioPool,
        next: &mut u64,
        path: &Path,
        placement: Option<(TrackId, Time)>,
    ) -> Result<(Time, Command, AssetId, Vec<u8>), String> {
        if media_type_for(path) == "application/octet-stream" {
            return Err(format!("Unsupported media: {}", path.display()));
        }
        let bytes = read_bounded(path, animathio_format::Limits::default().asset_bytes)
            .map_err(|e| e.to_string())?;
        let id = AssetId::of(&bytes);
        let media_type = media_type_for(path);
        let name = path
            .file_name()
            .map_or_else(|| "media".into(), |s| s.to_string_lossy().into_owned());
        let mut commands = Vec::new();
        if !project.assets().contains_key(&id) {
            commands.push(Command::AddAsset {
                id: id.clone(),
                asset: Asset {
                    name,
                    media_type: media_type.to_owned(),
                },
            });
        }
        let (kind, duration) = if media_type.starts_with("audio/") {
            if let Ok(source) = SymphoniaSource::from_bytes(bytes.clone()) {
                let duration = frames_to_ticks(
                    usize::try_from(source.info().frames).unwrap_or(usize::MAX),
                    source.info().rate,
                )
                .max(TICKS_PER_SECOND);
                pool.insert(id.clone(), &source)
                    .map_err(|e| e.to_string())?;
                (ClipKind::Audio(id.clone()), duration)
            } else {
                let silent = Pcm::new(48_000, 1, vec![0.0]).map_err(|e| e.to_string())?;
                pool.insert(id.clone(), &silent)
                    .map_err(|e| e.to_string())?;
                (ClipKind::Audio(id.clone()), DEFAULT_MEDIA_TICKS)
            }
        } else if media_type.starts_with("image/") {
            (ClipKind::Image(id.clone()), DEFAULT_MEDIA_TICKS)
        } else {
            (ClipKind::Video(id.clone()), DEFAULT_MEDIA_TICKS)
        };
        let media_info = if matches!(kind, ClipKind::Video(_)) {
            animathio_media::FilmCraftBackend::new().probe(path).ok()
        } else {
            None
        };
        let duration = media_info.as_ref().map_or(duration, |info| info.duration.0);
        let mut end = Time(0);
        if let Some((track, start)) = placement {
            end = Time(start.0.checked_add(duration).ok_or("clip end overflow")?);
            let mut clip = Clip::new(
                {
                    let id = ClipId(*next);
                    *next = next.saturating_add(1);
                    id
                },
                kind,
                TimeRange::new(start, end).map_err(|e| e.0)?,
            );
            if matches!(clip.kind, ClipKind::Audio(_)) {
                clip.params.insert("gain".into(), 1.0.into());
                clip.params.insert("muted".into(), 0.0.into());
            }
            let companion = media_info
                .as_ref()
                .is_some_and(|info| !info.audio.is_empty());
            let clip_id = clip.id;
            let range = clip.range;
            commands.push(Command::AddClip { track, clip });
            if companion {
                let audio_track = TrackId(
                    project
                        .tracks()
                        .iter()
                        .map(|t| t.id().0)
                        .max()
                        .unwrap_or(0)
                        .checked_add(1)
                        .ok_or("track id space exhausted")?,
                );
                // Reserved from the counter so a later file in the same import cannot
                // be handed the same id.
                let audio_id = {
                    let id = ClipId(*next);
                    *next = next.saturating_add(1);
                    id
                };
                let mut audio = Clip::new(audio_id, ClipKind::Audio(id.clone()), range);
                audio.link_group = Some(clip_id);
                audio.params.insert("gain".into(), 1.0.into());
                audio.params.insert("muted".into(), 0.0.into());
                commands.push(Command::SetClipLink {
                    id: clip_id,
                    group: Some(clip_id),
                });
                commands.push(Command::AddTrack {
                    track: Track::new(audio_track, "Audio"),
                    index: project.tracks().len(),
                });
                commands.push(Command::AddClip {
                    track: audio_track,
                    clip: audio,
                });
                if let Ok(source) = SymphoniaSource::from_bytes(bytes.clone()) {
                    pool.insert(id.clone(), &source)
                        .map_err(|e| e.to_string())?;
                }
            }
        }
        Ok((end, Command::Batch { commands }, id, bytes))
    }

    pub(super) fn import_legacy(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("AniMathIO v1", &["animathio"])
            .pick_file()
        {
            match self.import_control_v1(&path) {
                Ok(()) => {
                    self.marks = InOut::default();
                    self.shuttle = Shuttle::default();
                    self.after_edit(cx);
                }
                Err(error) => {
                    self.error = Some(error.to_string());
                    cx.notify();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn media_import_including_the_first_track_and_linked_audio_is_one_undo_step() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let image = root.join("../../crates/render/tests/fixtures/effects.png");
        let video = root.join("../../crates/media/tests/fixtures/sample.mp4");
        let bad = root.join("missing-import-file.png");
        let original = Project::default();
        let mut next = 1;
        let (command, assets, errors) = Editor::prepare_import(
            &original,
            &mut AudioPool::default(),
            &mut next,
            &[image.clone(), video, image, bad],
            Some((TrackId(1), Time(0))),
        );
        assert_eq!(errors.len(), 1);
        assert_eq!(assets.len(), 2, "repeated media reuses its asset");
        let mut project = original.clone();
        let mut history = History::default();
        history.execute(&mut project, &command).unwrap();
        assert_eq!(project.tracks()[0].clips().len(), 3);
        assert!(
            project
                .tracks()
                .iter()
                .flat_map(|t| t.clips().values())
                .any(|c| matches!(c.kind, ClipKind::Audio(_)) && c.link_group.is_some())
        );
        let imported = project.clone();
        assert!(history.undo(&mut project).unwrap());
        assert_eq!(project, original);
        assert!(!history.undo(&mut project).unwrap());
        assert!(history.redo(&mut project).unwrap());
        assert_eq!(project, imported);
    }

    #[test]
    fn failed_import_does_not_create_a_track() {
        let original = Project::default();
        let mut next = 1;
        let (command, assets, errors) = Editor::prepare_import(
            &original,
            &mut AudioPool::default(),
            &mut next,
            &[PathBuf::from("/nonexistent/animathio-missing.png")],
            Some((TrackId(1), Time(0))),
        );
        assert!(assets.is_empty());
        assert_eq!(errors.len(), 1);
        let mut project = original.clone();
        command.apply(&mut project).unwrap();
        assert_eq!(project, original);
    }
}
