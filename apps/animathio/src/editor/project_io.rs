//! Project io.

use super::*;
use animathio_media::MediaBackend;

impl Editor {
    pub(super) fn new_project(&mut self, cx: &mut Context<Self>) {
        self.finish_color_edit(cx);
        let _ = self.clock.pause();
        self.marks = InOut::default();
        self.shuttle = Shuttle::default();
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
        self.waveforms.borrow_mut().clear();
        self.clock = make_clock(&self.project, &self.pool);
        self.path = None;
        self.dirty = false;
        self.clear_selection();
        self.selected_field = None;
        self.selected_keyframe = None;
        self.position = Time(0);
        self.playing = false;
        self.next_id = next_id(&self.project);
        self.worker.load(self.assets.clone());
        self.request_render(Time(0));
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
                            .push(gpui_component::notification::Notification::success(
                                "Project saved",
                            ));
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
        self.finish_color_edit(cx);
        match read_project_with_recovery(path) {
            Ok((project, assets, recovered)) => {
                self.audio_generation = self.audio_generation.wrapping_add(1);
                self.marks = InOut::default();
                self.shuttle = Shuttle::default();
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
                self.clear_selection();
                self.selected_field = None;
                self.position = Time(0);
                self.playing = false;
                self.next_id = next_id(&self.project);
                self.worker.load(Arc::new(assets));
                self.request_render(Time(0));
                self.recent.add(path);
                if recovered {
                    self.notices
                        .push(gpui_component::notification::Notification::info(
                            "Recovered from autosave",
                        ));
                }
            }
            Err(error) => self.error = Some(error),
        }
        cx.notify();
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
        let track = self.ensure_track(cx);
        self.import_paths(&paths, Some((track, self.position)), cx);
    }

    pub(super) fn ensure_track(&mut self, cx: &mut Context<Self>) -> TrackId {
        if let Some(track) = self.project.tracks().first() {
            return track.id();
        }
        let id = TrackId(self.next_id);
        self.next_id += 1;
        self.execute(
            &Command::AddTrack {
                track: Track::new(id, "Video 1"),
                index: 0,
            },
            cx,
        );
        id
    }

    pub(super) fn import_paths(
        &mut self,
        paths: &[PathBuf],
        placement: Option<(TrackId, Time)>,
        cx: &mut Context<Self>,
    ) {
        let mut placement = placement;
        let mut changed = false;
        for path in paths {
            match self.add_media_file(path, placement) {
                Ok(end) => {
                    changed = true;
                    if let Some((track, _)) = placement {
                        placement = Some((track, end));
                    }
                }
                Err(error) => self.error = Some(error),
            }
        }
        if changed {
            self.worker.load(self.assets.clone());
            self.after_edit(cx);
        }
        cx.notify();
    }

    /// Import an asset, optionally placing a clip, as one undoable transaction. Audio
    /// is decoded up front (unsupported audio falls back to a silent source); the
    /// actual clip length is taken from decoded audio when available.
    pub(super) fn add_media_file(
        &mut self,
        path: &Path,
        placement: Option<(TrackId, Time)>,
    ) -> Result<Time, String> {
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
        if !self.project.assets().contains_key(&id) {
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
                let duration = frames_to_ticks(source.pcm().frames(), source.pcm().rate())
                    .max(TICKS_PER_SECOND);
                self.pool
                    .insert(id.clone(), &source)
                    .map_err(|e| e.to_string())?;
                (ClipKind::Audio(id.clone()), duration)
            } else {
                let silent = Pcm::new(48_000, 1, vec![0.0]).map_err(|e| e.to_string())?;
                self.pool
                    .insert(id.clone(), &silent)
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
                self.next_clip_id(),
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
                    self.project
                        .tracks()
                        .iter()
                        .map(|t| t.id().0)
                        .max()
                        .unwrap_or(0)
                        + 1,
                );
                let mut audio =
                    Clip::new(ClipId(clip_id.0 + 1), ClipKind::Audio(id.clone()), range);
                audio.link_group = Some(clip_id);
                audio.params.insert("gain".into(), 1.0.into());
                audio.params.insert("muted".into(), 0.0.into());
                commands.push(Command::SetClipLink {
                    id: clip_id,
                    group: Some(clip_id),
                });
                commands.push(Command::AddTrack {
                    track: Track::new(audio_track, "Audio"),
                    index: self.project.tracks().len(),
                });
                commands.push(Command::AddClip {
                    track: audio_track,
                    clip: audio,
                });
                if let Ok(source) = SymphoniaSource::from_bytes(bytes.clone()) {
                    self.pool
                        .insert(id.clone(), &source)
                        .map_err(|e| e.to_string())?;
                }
            }
        }
        if !commands.is_empty() {
            self.history
                .execute(&mut self.project, &Command::Batch { commands })
                .map_err(|e| e.0)?;
        }
        Arc::make_mut(&mut self.assets).insert(id, bytes);
        Ok(end)
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
