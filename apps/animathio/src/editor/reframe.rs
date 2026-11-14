//! Subject-aware auto-reframe settings and cancellable worker.
use super::*;
use animathio_control::{ReframeParams, analyse_reframe, reframe_command};
use gpui_component::{
    Disableable,
    progress::Progress,
    slider::{Slider, SliderEvent, SliderState},
};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

pub(super) struct ReframePanel {
    params: ReframeParams,
    slider: Entity<SliderState>,
    _subscription: Subscription,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
    pub(super) busy: bool,
    message: String,
}
impl Drop for ReframePanel {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl Editor {
    pub(super) fn open_reframe(&mut self, cx: &mut Context<Self>) {
        if self.reframe_panel.is_some() {
            return;
        }
        let slider = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(1.)
                .step(0.05)
                .default_value(0.5)
        });
        let subscription = cx.subscribe(&slider, |this, _, event: &SliderEvent, cx| {
            if let SliderEvent::Change(value) = event
                && let Some(panel) = this.reframe_panel.as_mut()
                && !panel.busy
            {
                panel.params.smoothness = f64::from(value.start());
                cx.notify();
            }
        });
        let clips = self
            .selection
            .iter()
            .copied()
            .filter(|id| {
                self.find_clip(*id)
                    .is_some_and(|c| matches!(c.kind, animathio_core::ClipKind::Video(_)))
            })
            .collect();
        self.reframe_panel = Some(ReframePanel {
            params: ReframeParams {
                clips,
                ..ReframeParams::default()
            },
            slider,
            _subscription: subscription,
            cancel: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(AtomicU32::new(0)),
            busy: false,
            message: "Changes the canvas and replaces video position/scale keys. \
                      Undo restores everything."
                .into(),
        });
        cx.notify();
    }
    pub(super) fn start_reframe(
        &mut self,
        params: ReframeParams,
        reply: Option<std::sync::mpsc::SyncSender<Result<Value, ControlError>>>,
        cx: &mut Context<Self>,
    ) {
        self.open_reframe(cx);
        let panel = self.reframe_panel.as_mut().expect("panel opened");
        if panel.busy {
            if let Some(reply) = reply {
                let _ = reply.send(Err(ControlError::Other(
                    "auto reframe already running".into(),
                )));
            }
            return;
        }
        if params.smoothness.is_finite() && (0.0..=1.0).contains(&params.smoothness) {
            // Slider storage is f32; this only affects the displayed control.
            #[allow(clippy::cast_possible_truncation)]
            let value = params.smoothness as f32;
            panel.slider.update(cx, |slider, _| {
                *slider = SliderState::new()
                    .min(0.)
                    .max(1.)
                    .step(0.05)
                    .default_value(value);
            });
        }
        panel.params = params.clone();
        panel.busy = true;
        panel.message = "Analysing video...".into();
        panel.cancel = Arc::new(AtomicBool::new(false));
        panel.progress = Arc::new(AtomicU32::new(0));
        let cancel = panel.cancel.clone();
        let identity = cancel.clone();
        let progress = panel.progress.clone();
        let project = self.project.clone();
        let assets = self.assets.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    analyse_reframe(&project, &assets, &params, &cancel, &|n| {
                        progress.store(n, Ordering::Relaxed);
                    })
                })
                .await;
            let updated = this.update(cx, |editor, cx| {
                let active = editor
                    .reframe_panel
                    .as_ref()
                    .is_some_and(|p| Arc::ptr_eq(&p.cancel, &identity));
                let outcome = result.and_then(|draft| {
                    if !active || identity.load(Ordering::Relaxed) {
                        return Err(ControlError::Other("cancelled".into()));
                    }
                    let command = reframe_command(&editor.project, &draft)?;
                    if !editor.try_execute(&command, cx) {
                        return Err(ControlError::Other(
                            editor
                                .error
                                .clone()
                                .unwrap_or_else(|| "could not reframe".into()),
                        ));
                    }
                    Ok(draft.clips)
                });
                if active && let Some(panel) = editor.reframe_panel.as_mut() {
                    panel.busy = false;
                    panel.message = match &outcome {
                        Ok(n) => format!("Reframed {n} video clips"),
                        Err(e) => e.to_string(),
                    };
                }
                cx.notify();
                outcome.and_then(|n| Ok(serde_json::to_value(n)?))
            });
            if let Some(reply) = reply {
                let _ = reply.send(
                    updated.unwrap_or_else(|_| Err(ControlError::Other("editor closed".into()))),
                );
            }
        })
        .detach();
        cx.notify();
    }
    pub(super) fn reframe_view(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let panel = self.reframe_panel.as_ref()?;
        if self.is_start_screen() {
            return None;
        }
        let busy = panel.busy;
        let mut view = div()
            .id("auto-reframe-panel")
            .absolute()
            .left(px(12.))
            .top(px(56.))
            .w(px(390.))
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
                    .child(icons::icon(IconName::Scan))
                    .child("Auto reframe"),
            )
            .child(div().text_xs().child(if panel.params.clips.is_empty() {
                "All video clips"
            } else {
                "Selected video clips"
            }));
        let mut aspects = div().flex().gap_2();
        for aspect in ["9:16", "1:1", "4:5"] {
            aspects = aspects.child(
                Button::new(SharedString::from(format!("reframe-{aspect}")))
                    .compact()
                    .label(if panel.params.aspect == aspect {
                        format!("{aspect} (selected)")
                    } else {
                        aspect.into()
                    })
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(p) = this.reframe_panel.as_mut() {
                            p.params.aspect = aspect.into();
                        }
                        cx.notify();
                    })),
            );
        }
        view = view
            .child(aspects)
            .child(div().text_xs().child(format!(
                "Tracking smoothness: {:.0}%",
                panel.params.smoothness * 100.
            )))
            .when(!busy, |v| v.child(Slider::new(&panel.slider).horizontal()))
            .child(div().text_xs().child(panel.message.clone()));
        if busy {
            view = view.child(Progress::new("reframe-progress").value(f32::from(
                u8::try_from(panel.progress.load(Ordering::Relaxed)).unwrap_or(100),
            )));
        }
        Some(
            view.child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("reframe-start")
                            .label("Reframe")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(p) = this.reframe_panel.as_ref() {
                                    this.start_reframe(p.params.clone(), None, cx);
                                }
                            })),
                    )
                    .child(
                        Button::new("reframe-cancel")
                            .label(if busy { "Cancel" } else { "Close" })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.reframe_panel = None;
                                cx.notify();
                            })),
                    ),
            ),
        )
    }
}
