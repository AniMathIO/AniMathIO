//! Project io.

use super::*;

impl Editor {
    pub(super) fn new_project(&mut self, cx: &mut Context<Self>) {
        self.finish_color_edit(cx);
        let _ = self.clock.pause();
        self.project = Project::new(self.project.frame_rate());
        self.history = History::default();
        self.assets.clear();
        self.pool = AudioPool::default();
        self.audio_ids.clear();
        self.waveforms.borrow_mut().clear();
        self.clock = make_clock(&self.project, &self.pool);
        self.path = None;
        self.dirty = false;
        self.selected = None;
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
        self.finish_color_edit(cx);
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
        match write_project(&path, &self.project, &self.assets) {
            Ok(()) => {
                self.path = Some(path.clone());
                self.dirty = false;
                self.last_autosave = Instant::now();
                self.recent.add(&path);
                self.notices
                    .push(gpui_component::notification::Notification::success(
                        "Project saved",
                    ));
                cx.notify();
            }
            Err(error) => self.error = Some(error),
        }
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
                self.project = project;
                self.history = History::default();
                self.assets = assets.clone();
                self.path = Some(path.to_path_buf());
                self.dirty = recovered;
                self.last_autosave = Instant::now();
                self.pool = AudioPool::default();
                if let Err(error) = load_audio(&mut self.pool, &self.project, &self.assets) {
                    self.error = Some(error);
                }
                self.audio_ids = audio_asset_ids(&self.project);
                self.clock = make_clock(&self.project, &self.pool);
                self.selected = None;
                self.selected_field = None;
                self.position = Time(0);
                self.playing = false;
                self.next_id = next_id(&self.project);
                self.worker.load(assets);
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
        let mut changed = false;
        for path in paths {
            match self.add_media_file(&path) {
                Ok(()) => changed = true,
                Err(error) => self.error = Some(error),
            }
        }
        if changed {
            self.after_edit(cx);
        }
    }

    /// Add one media file as an asset and a clip starting at the playhead. Audio
    /// is decoded up front (unsupported audio falls back to a silent source); the
    /// actual clip length is taken from decoded audio when available.
    pub(super) fn add_media_file(&mut self, path: &Path) -> Result<(), String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let id = AssetId::of(&bytes);
        let media_type = media_type_for(path);
        let name = path
            .file_name()
            .map_or_else(|| "media".into(), |s| s.to_string_lossy().into_owned());
        if !self.project.assets().contains_key(&id) {
            self.history
                .execute(
                    &mut self.project,
                    &Command::AddAsset {
                        id: id.clone(),
                        asset: Asset {
                            name,
                            media_type: media_type.to_owned(),
                        },
                    },
                )
                .map_err(|e| e.0)?;
        }
        self.assets.insert(id.clone(), bytes.clone());
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
        let start = self.position;
        let end = start
            .0
            .checked_add(duration)
            .ok_or_else(|| "clip end overflow".to_owned())?;
        let track = self
            .project
            .tracks()
            .first()
            .map(Track::id)
            .ok_or_else(|| "no track to place the clip".to_owned())?;
        let clip = Clip::new(
            self.next_clip_id(),
            kind,
            TimeRange::new(start, Time(end)).map_err(|e| e.0)?,
        );
        self.history
            .execute(&mut self.project, &Command::AddClip { track, clip })
            .map_err(|e| e.0)?;
        Ok(())
    }
}
