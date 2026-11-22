//! Export Queue panel: opened from the status bar, View menu or the export
//! dialog. Lists every job with progress, ETA, cancel, reveal/open and errors.

use super::*;
use crate::export_queue::{JobState, format_duration, open_file, reveal};
use crate::style::{self, PanelAnchor, ToolPanel, color, tokens::TextRole as _};
use gpui_component::Disableable;
use gpui_component::progress::Progress;

impl Editor {
    /// Status-bar chip summarising the queue; toggles the panel.
    pub(super) fn export_queue_button(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let queue = &self.export_queue;
        let label = if let Some(job) = queue.running() {
            let waiting = queue.queued_count();
            format!(
                "Exporting {:.0}%{}",
                job.fraction() * 100.0,
                if waiting > 0 {
                    format!(" (+{waiting} queued)")
                } else {
                    String::new()
                }
            )
        } else if queue.queued_count() > 0 {
            format!("{} queued", queue.queued_count())
        } else {
            "Export queue".to_owned()
        };
        let active = queue.is_busy();
        style::chip("status-export-queue", Some(IconName::ListVideo), label)
            .tabular()
            .cursor_pointer()
            .when(active, |chip| chip.text_color(style::hex(color::ACCENT)))
            .when(self.export_queue_open, |chip| {
                chip.bg(style::hex(color::HOVER))
            })
            .hover(|chip| chip.bg(style::hex(color::HOVER)))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.export_queue_open = !this.export_queue_open;
                cx.notify();
            }))
    }

    pub(super) fn export_queue_panel(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        if !self.export_queue_open || self.is_start_screen() {
            return None;
        }
        let mut list = div()
            .id("export-queue-list")
            .debug_selector(|| "export-queue-list".into())
            .flex()
            .flex_col()
            .gap_2()
            .px_3()
            .pb_3()
            .w_full();
        if self.export_queue.is_empty() {
            list = list.child(
                div()
                    .py_4()
                    .text_role(style::Role::Body)
                    .text_color(style::hex(color::TEXT_SECONDARY))
                    .child("No exports yet. Use Export, then Add to queue."),
            );
        }
        for job in self.export_queue.jobs() {
            list = list.child(Self::export_job_row(job, cx));
        }
        let finished = self
            .export_queue
            .jobs()
            .iter()
            .any(|job| job.state.is_finished());
        let clear = style::secondary_button("export-queue-clear", cx)
            .debug_selector(|| "export-queue-clear".into())
            .small()
            .icon(icons::icon(IconName::ListX))
            .label("Clear finished")
            .disabled(!finished)
            .tooltip("Clear finished exports")
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.export_queue.clear_finished();
                cx.notify();
            }));
        Some(
            ToolPanel::new("export-queue-panel", IconName::ListVideo, "Export queue")
                .anchor(PanelAnchor::BottomRight)
                .bottom_height(self.dock.bottom_height)
                .on_close(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.export_queue_open = false;
                    cx.notify();
                }))
                .body(self.export_queue_scroll.clone(), div().w_full().child(list))
                .footer(clear)
                .render(window),
        )
    }

    fn export_job_row(job: &crate::export_queue::Job, cx: &mut Context<Self>) -> Div {
        let id = job.id;
        let (state_icon, state_color) = match &job.state {
            JobState::Queued => (IconName::Clock, color::TEXT_MUTED),
            JobState::Running => (IconName::LoaderCircle, color::ACCENT),
            JobState::Done(_) => (IconName::CircleCheck, color::SUCCESS),
            JobState::Failed(_) => (IconName::CircleAlert, color::DANGER),
            JobState::Cancelled => (IconName::Ban, color::TEXT_MUTED),
        };
        let button = |suffix: &str, icon: IconName, tooltip: &'static str| {
            Button::new(SharedString::from(format!("export-{suffix}-{id}")))
                .ghost()
                .small()
                .icon(icons::icon(icon))
                .tooltip(tooltip)
        };
        let mut actions = div().flex().flex_row().gap_1();
        match &job.state {
            JobState::Queued | JobState::Running => {
                actions = actions.child(
                    button("cancel", IconName::X, "Cancel")
                        .disabled(job.cancelling)
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.export_queue.cancel(id);
                            cx.notify();
                        })),
                );
            }
            JobState::Done(path) => {
                let (reveal_path, open_path) = (path.clone(), path.clone());
                actions = actions
                    .child(
                        button("reveal", IconName::FolderOpen, "Show in folder").on_click(
                            cx.listener(move |this, _: &ClickEvent, _, cx| {
                                if let Err(error) = reveal(&reveal_path) {
                                    this.error = Some(error);
                                }
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        button("open", IconName::ExternalLink, "Open video").on_click(cx.listener(
                            move |this, _: &ClickEvent, _, cx| {
                                if let Err(error) = open_file(&open_path) {
                                    this.error = Some(error);
                                }
                                cx.notify();
                            },
                        )),
                    )
                    .child(button("remove", IconName::X, "Remove from list").on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.export_queue.remove(id);
                            cx.notify();
                        }),
                    ));
            }
            JobState::Failed(_) | JobState::Cancelled => {
                actions =
                    actions.child(button("remove", IconName::X, "Remove from list").on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.export_queue.remove(id);
                            cx.notify();
                        }),
                    ));
            }
        }
        let detail = match &job.state {
            JobState::Queued => format!("Waiting. {}", job.summary),
            JobState::Running if job.cancelling => "Cancelling\u{2026}".to_owned(),
            JobState::Running => {
                let (done, total) = job.progress();
                let eta = job
                    .eta()
                    .map_or_else(|| "estimating".to_owned(), format_duration);
                format!("{done} / {total} frames, ETA {eta}. {}", job.summary)
            }
            JobState::Done(_) => format!(
                "Finished in {}. {}",
                job.elapsed().map_or_else(String::new, format_duration),
                job.summary
            ),
            JobState::Failed(_) => job.summary.clone(),
            JobState::Cancelled => format!("Cancelled. {}", job.summary),
        };
        let mut row = div()
            .flex()
            .flex_col()
            .gap_1()
            .min_h(style::size::ROW)
            .p_2()
            .rounded(px(style::radius::CONTROL))
            .bg(style::hex(color::PANEL))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        icons::icon(state_icon)
                            .small()
                            .text_color(style::hex(state_color)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_role(style::Role::Body)
                            .child(job.name()),
                    )
                    .child(actions),
            )
            .child(
                div()
                    .text_role(style::Role::Caption)
                    .tabular()
                    .child(detail),
            );
        if let JobState::Running = job.state {
            let (_, total) = job.progress();
            row = row.child(
                Progress::new(SharedString::from(format!("export-progress-{id}")))
                    .value(job.fraction() * 100.0)
                    .loading(total == 0),
            );
        }
        if let JobState::Failed(error) = &job.state {
            row = row.child(
                div()
                    .text_role(style::Role::Caption)
                    .text_color(style::hex(color::DANGER))
                    .child(error.clone()),
            );
        }
        row
    }
}
