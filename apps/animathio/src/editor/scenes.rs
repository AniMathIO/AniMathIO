//! Cancellable scene analysis and explicit review before applying edits.

use super::tool_panel;
use super::*;
use crate::style::{self, SegmentedOption};
use animathio_control::{SceneDraft, SceneParams, detect_scenes, scene_command};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
pub(super) struct ScenePanel {
    draft: Option<SceneDraft>,
    params: SceneParams,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
    pub(super) busy: bool,
    message: String,
    /// Scroll position of the panel body.
    scroll: ScrollHandle,
    /// Keyboard focus for the segmented controls (one handle per option).
    sensitivity_focus: Vec<FocusHandle>,
    min_focus: Vec<FocusHandle>,
    delete_focus: Vec<FocusHandle>,
}

/// The "Minimum shot" and "Delete shots shorter than" choices, in frames.
const MIN_SHOT_FRAMES: [u32; 4] = [1, 6, 12, 24];
const DELETE_FRAMES: [u32; 4] = [0, 6, 12, 24];

#[cfg(test)]
impl ScenePanel {
    /// The detection settings the panel will analyse (and re-analyse) with.
    pub(super) fn params(&self) -> &SceneParams {
        &self.params
    }
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
        // A new analysis keeps the scroll position and the keyboard focus of the controls.
        let (scroll, sensitivity_focus, min_focus, delete_focus) = match &self.scene_panel {
            Some(p) => (
                p.scroll.clone(),
                p.sensitivity_focus.clone(),
                p.min_focus.clone(),
                p.delete_focus.clone(),
            ),
            None => (
                ScrollHandle::new(),
                tool_panel::focus_handles(3, cx),
                tool_panel::focus_handles(4, cx),
                tool_panel::focus_handles(4, cx),
            ),
        };
        self.scene_panel = Some(ScenePanel {
            draft: None,
            params: params.clone(),
            cancel: cancel.clone(),
            progress: progress.clone(),
            busy: true,
            message: "Analyzing video…".into(),
            scroll,
            sensitivity_focus,
            min_focus,
            delete_focus,
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
    pub(super) fn scene_view(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let panel = self.scene_panel.as_ref()?;
        if self.is_start_screen() {
            return None;
        }
        let entity = cx.entity();
        let busy = panel.busy;

        // What to analyse, and how.
        let mut settings: Vec<AnyElement> =
            vec![tool_panel::note(panel.message.clone()).into_any_element()];
        if busy {
            settings.push(
                gpui_component::progress::Progress::new("scene-progress")
                    .value(f32::from(
                        u8::try_from(panel.progress.load(Ordering::Relaxed)).unwrap_or(100),
                    ))
                    .into_any_element(),
            );
        } else {
            let restart = |entity: &Entity<Self>, mutate: fn(&mut SceneParams, usize)| {
                let entity = entity.clone();
                move |index: usize, _: &mut Window, cx: &mut App| {
                    entity.update(cx, |this, cx| {
                        let Some(mut params) = this.scene_panel.as_ref().map(|p| p.params.clone())
                        else {
                            return;
                        };
                        mutate(&mut params, index);
                        this.start_scenes(params, cx);
                    });
                }
            };
            let sensitivity = match panel.params.sensitivity {
                s if s < 0.375 => 0,
                s if s < 0.625 => 1,
                _ => 2,
            };
            settings.push(
                style::form_row(
                    "Sensitivity",
                    tool_panel::choice(
                        "scene-sensitivity",
                        &[
                            SegmentedOption::new("scene-sensitivity-low", "Low", None),
                            SegmentedOption::new("scene-sensitivity-medium", "Medium", None),
                            SegmentedOption::new("scene-sensitivity-high", "High", None),
                        ],
                        &panel.sensitivity_focus,
                        sensitivity,
                        restart(&entity, |params, index| {
                            params.sensitivity = [0.25, 0.5, 0.75][index.min(2)];
                        }),
                    ),
                )
                .into_any_element(),
            );
            let min_index = MIN_SHOT_FRAMES
                .iter()
                .position(|f| *f == panel.params.min_shot_frames);
            settings.push(
                tool_panel::stacked(
                    "Minimum shot",
                    tool_panel::choice(
                        "scene-min",
                        &[
                            SegmentedOption::new("scene-min-1", "1 frame", None),
                            SegmentedOption::new("scene-min-6", "6 frames", None),
                            SegmentedOption::new("scene-min-12", "12 frames", None),
                            SegmentedOption::new("scene-min-24", "24 frames", None),
                        ],
                        &panel.min_focus,
                        min_index.unwrap_or(0),
                        restart(&entity, |params, index| {
                            params.min_shot_frames = MIN_SHOT_FRAMES[index.min(3)];
                        }),
                    ),
                )
                .into_any_element(),
            );
            let delete_index = DELETE_FRAMES
                .iter()
                .position(|f| *f == panel.params.delete_shorter_than_frames);
            settings.push(
                tool_panel::stacked(
                    "Delete shots shorter than (leaves gaps)",
                    tool_panel::choice(
                        "scene-delete",
                        &[
                            SegmentedOption::new("scene-delete-0", "Off", None),
                            SegmentedOption::new("scene-delete-6", "6 frames", None),
                            SegmentedOption::new("scene-delete-12", "12 frames", None),
                            SegmentedOption::new("scene-delete-24", "24 frames", None),
                        ],
                        &panel.delete_focus,
                        delete_index.unwrap_or(0),
                        restart(&entity, |params, index| {
                            params.delete_shorter_than_frames = DELETE_FRAMES[index.min(3)];
                        }),
                    ),
                )
                .into_any_element(),
            );
            if let Some(clip) = panel.params.clip {
                let track = self
                    .project
                    .tracks()
                    .iter()
                    .find(|t| t.clips().contains_key(&clip))
                    .map(animathio_core::Track::id);
                let mut params = panel.params.clone();
                params.clip = None;
                params.track = track;
                settings.push(
                    div()
                        .flex()
                        .child(
                            style::secondary_button("scene-track", cx)
                                .small()
                                .label("Analyze all video on this track")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.start_scenes(params.clone(), cx);
                                })),
                        )
                        .into_any_element(),
                );
            }
        }
        let mut body = tool_panel::body().child(tool_panel::section("Detection", settings));

        if let Some(draft) = &panel.draft {
            let mut rows = div().id("scene-cuts").flex().flex_col().gap_1();
            for (i, cut) in draft.cuts.iter().enumerate() {
                let mut row = div().flex().items_center().gap_2();
                if cut.thumbnail.len() == 96 * 54 {
                    let rgba = image::RgbaImage::from_fn(96, 54, |x, y| {
                        let v = cut.thumbnail[(y * 96 + x) as usize];
                        image::Rgba([v, v, v, 255])
                    });
                    row = row.child(
                        img(Arc::new(RenderImage::new([image::Frame::new(rgba)])))
                            .w(px(96.))
                            .h(px(54.))
                            .flex_shrink_0()
                            .rounded(px(style::radius::CONTROL)),
                    );
                }
                rows = rows.child(
                    row.child(
                        tool_panel::check_row(
                            SharedString::from(format!("scene-cut-{i}")),
                            Some(format!("scene-cut-{i}")),
                            cut.enabled,
                            crate::timecode::format_timecode(cut.time, self.project.frame_rate()),
                            cx.listener(move |this, _, _, cx| {
                                if let Some(draft) =
                                    this.scene_panel.as_mut().and_then(|p| p.draft.as_mut())
                                {
                                    draft.cuts[i].enabled = !draft.cuts[i].enabled;
                                }
                                cx.notify();
                            }),
                        )
                        .flex_1()
                        .min_w_0(),
                    ),
                );
            }
            let included = draft.cuts.iter().filter(|c| c.enabled).count();
            body = body.child(tool_panel::section(
                format!("Proposed cuts ({included} of {})", draft.cuts.len()),
                [rows.into_any_element()],
            ));
        }

        // Cancel while the analysis runs; Add markers and Split once there is a draft.
        let footer = if busy {
            Some(
                style::secondary_button("scene-close", cx)
                    .small()
                    .label("Cancel")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.scene_panel = None;
                        cx.notify();
                    }))
                    .into_any_element(),
            )
        } else if panel.draft.is_some() {
            Some(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        style::secondary_button("scene-markers", cx)
                            .small()
                            .label("Add markers")
                            .on_click(cx.listener(|this, _, _, cx| this.apply_scenes(true, cx))),
                    )
                    .child(
                        style::primary_button("scene-split", cx)
                            .small()
                            .label("Split")
                            .on_click(cx.listener(|this, _, _, cx| this.apply_scenes(false, cx))),
                    )
                    .into_any_element(),
            )
        } else {
            None
        };
        let mut tool =
            style::ToolPanel::new("scene-review", IconName::Scissors, "Split at scene changes")
                .bottom_height(self.dock.bottom_height)
                .on_close(cx.listener(|this, _, _, cx| {
                    this.scene_panel = None;
                    cx.notify();
                }))
                .body(panel.scroll.clone(), body);
        if let Some(footer) = footer {
            tool = tool.footer(footer);
        }
        Some(tool.render(window))
    }
}
