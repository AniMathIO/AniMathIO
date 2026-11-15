//! Cancellable scene analysis and explicit review before applying edits.
use super::*;
use animathio_control::{SceneDraft, SceneParams, detect_scenes, scene_command};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
pub(super) struct ScenePanel {
    draft: Option<SceneDraft>,
    params: SceneParams,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
    pub(super) busy: bool,
    message: String,
}
impl Drop for ScenePanel {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl Editor {
    pub(super) fn open_scenes(&mut self, cx: &mut Context<Self>) {
        if self.scene_panel.is_some() {
            return;
        }
        let clip = self.selection.iter().copied().find(|id| {
            self.find_clip(*id)
                .is_some_and(|c| matches!(c.kind, animathio_core::ClipKind::Video(_)))
        });
        let Some(clip) = clip else {
            self.error = Some("Select a video clip to detect scenes".into());
            cx.notify();
            return;
        };
        self.start_scenes(
            SceneParams {
                clip: Some(clip),
                ..SceneParams::default()
            },
            cx,
        );
    }
    fn start_scenes(&mut self, params: SceneParams, cx: &mut Context<Self>) {
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU32::new(0));
        self.scene_panel = Some(ScenePanel {
            draft: None,
            params: params.clone(),
            cancel: cancel.clone(),
            progress: progress.clone(),
            busy: true,
            message: "Analysing video…".into(),
        });
        let project = self.project.clone();
        let assets = self.assets.clone();
        let identity = cancel.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    detect_scenes(&project, &assets, &params, &cancel, &|n| {
                        progress.store(n, Ordering::Relaxed);
                    })
                })
                .await;
            let _ = this.update(cx, |editor, cx| {
                if let Some(panel) = editor.scene_panel.as_mut()
                    && Arc::ptr_eq(&panel.cancel, &identity)
                {
                    panel.busy = false;
                    match result {
                        Ok(draft) => {
                            panel.message = format!("{} proposed cuts", draft.cuts.len());
                            panel.draft = Some(draft);
                        }
                        Err(e) => panel.message = e.to_string(),
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub(super) fn apply_scenes(&mut self, markers: bool, cx: &mut Context<Self>) {
        let result = self
            .scene_panel
            .as_ref()
            .and_then(|p| p.draft.as_ref())
            .map(|draft| scene_command(&self.project, draft, markers));
        if let Some(result) = result {
            match result {
                Ok(command) => {
                    if self.try_execute(&command, cx) {
                        self.scene_panel = None;
                    }
                }
                Err(e) => self.error = Some(e.to_string()),
            }
        }
        cx.notify();
    }
    #[allow(clippy::too_many_lines)]
    pub(super) fn scene_view(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let panel = self.scene_panel.as_ref()?;
        let mut view = div()
            .id("scene-review")
            .absolute()
            .left(px(12.))
            .top(px(56.))
            .w(px(390.))
            .max_h(px(650.))
            .overflow_y_scroll()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .bg(cx.theme().popover)
            .text_color(cx.theme().popover_foreground)
            .border_1()
            .border_color(cx.theme().border)
            .rounded(px(8.))
            .shadow_lg()
            .occlude()
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(icons::icon(IconName::Scissors))
                    .child("Split at scene changes"),
            )
            .child(
                div()
                    .text_size(crate::style::type_size::XS)
                    .child(panel.message.clone()),
            );
        if panel.busy {
            view = view.child(
                gpui_component::progress::Progress::new("scene-progress").value(f32::from(
                    u8::try_from(panel.progress.load(Ordering::Relaxed)).unwrap_or(100),
                )),
            );
        }
        view = view.child(div().text_size(crate::style::type_size::XS).child(format!(
            "Sensitivity {:.0}% · minimum {} frames · delete under {} frames (leaves gaps)",
            panel.params.sensitivity * 100.,
            panel.params.min_shot_frames,
            panel.params.delete_shorter_than_frames
        )));
        if !panel.busy {
            for (label, value) in [("Low sensitivity", 0.25), ("Medium", 0.5), ("High", 0.75)] {
                let mut params = panel.params.clone();
                params.sensitivity = value;
                view =
                    view.child(
                        Button::new(SharedString::from(label))
                            .compact()
                            .label(label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.start_scenes(params.clone(), cx)
                            })),
                    );
            }
            for frames in [0, 6, 12, 24] {
                let mut params = panel.params.clone();
                params.delete_shorter_than_frames = frames;
                view =
                    view.child(
                        Button::new(SharedString::from(format!("scene-delete-{frames}")))
                            .compact()
                            .label(format!("Delete shots under {frames} frames"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.start_scenes(params.clone(), cx)
                            })),
                    );
            }
            for frames in [1, 6, 12, 24] {
                let mut params = panel.params.clone();
                params.min_shot_frames = frames;
                view =
                    view.child(
                        Button::new(SharedString::from(format!("scene-min-{frames}")))
                            .compact()
                            .label(format!("Minimum shot: {frames} frames"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.start_scenes(params.clone(), cx)
                            })),
                    );
            }
            if let Some(clip) = panel.params.clip {
                let track = self
                    .project
                    .tracks()
                    .iter()
                    .find(|t| t.clips().contains_key(&clip))
                    .map(|t| t.id());
                let mut params = panel.params.clone();
                params.clip = None;
                params.track = track;
                view =
                    view.child(
                        Button::new("scene-track")
                            .compact()
                            .label("Analyse all video on this track")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.start_scenes(params.clone(), cx)
                            })),
                    );
            }
        }
        if let Some(draft) = &panel.draft {
            let mut rows = div()
                .id("scene-cuts")
                .max_h(px(300.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_1();
            for (i, cut) in draft.cuts.iter().enumerate() {
                if cut.thumbnail.len() == 96 * 54 {
                    let rgba = image::RgbaImage::from_fn(96, 54, |x, y| {
                        let v = cut.thumbnail[(y * 96 + x) as usize];
                        image::Rgba([v, v, v, 255])
                    });
                    rows = rows.child(
                        img(Arc::new(RenderImage::new([image::Frame::new(rgba)])))
                            .w(px(96.))
                            .h(px(54.)),
                    );
                }
                rows = rows.child(
                    Button::new(SharedString::from(format!("scene-cut-{i}")))
                        .compact()
                        .label(format!(
                            "{} · {} ticks",
                            if cut.enabled { "Include" } else { "Skip" },
                            cut.time.0
                        ))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(draft) =
                                this.scene_panel.as_mut().and_then(|p| p.draft.as_mut())
                            {
                                draft.cuts[i].enabled = !draft.cuts[i].enabled;
                            }
                            cx.notify();
                        })),
                );
            }
            view = view
                .child(rows)
                .child(
                    Button::new("scene-split")
                        .label("Split")
                        .on_click(cx.listener(|this, _, _, cx| this.apply_scenes(false, cx))),
                )
                .child(
                    Button::new("scene-markers")
                        .label("Add markers")
                        .on_click(cx.listener(|this, _, _, cx| this.apply_scenes(true, cx))),
                );
        }
        Some(
            view.child(
                Button::new("scene-close")
                    .label(if panel.busy { "Cancel" } else { "Close" })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.scene_panel = None;
                        cx.notify();
                    })),
            ),
        )
    }
}
