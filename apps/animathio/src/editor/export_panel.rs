//! Export Queue popover: opened from the status bar, View menu or the export
//! dialog. Lists every job with progress, ETA, cancel, reveal/open and errors.

use super::*;
use crate::export_queue::{JobState, format_duration, open_file, reveal};
use gpui_component::Disableable;
use gpui_component::progress::Progress;

impl Editor {
    /// Status-bar button summarising the queue; toggles the popover.
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
        div()
            .id("status-export-queue")
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_2()
            .h(px(22.))
            .border_1()
            .border_color(cx.theme().border)
            .bg(crate::style::hex(crate::style::color::PANEL))
            .cursor_pointer()
            .rounded(px(crate::style::radius::CONTROL))
            .hover(|this| this.bg(cx.theme().muted))
            .when(self.export_queue_open, |this| this.bg(cx.theme().muted))
            .text_color(if active {
                cx.theme().primary
            } else {
                cx.theme().muted_foreground
            })
            .child(icons::icon(IconName::ListVideo).small())
            .child(label)
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.export_queue_open = !this.export_queue_open;
                cx.notify();
            }))
    }

    pub(super) fn export_queue_panel(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        if !self.export_queue_open || self.is_start_screen() {
            return None;
        }
        let theme = cx.theme().clone();
        let mut list = div()
            .id("export-queue-list")
            .flex()
            .flex_col()
            .gap_2()
            .max_h(px(360.))
            .overflow_y_scroll();
        if self.export_queue.is_empty() {
            list = list.child(
                div()
                    .py_4()
                    .text_color(theme.muted_foreground)
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
        Some(
            div()
                .id("export-queue-panel")
                .absolute()
                .right(px(8.))
                .bottom(px(32.))
                .w(px(440.))
                .p_3()
                .flex()
                .flex_col()
                .gap_2()
                .bg(theme.popover)
                .text_color(theme.popover_foreground)
                .border_1()
                .border_color(theme.border)
                .rounded(px(crate::style::radius::PANEL))
                .shadow_lg()
                .occlude()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(icons::icon(IconName::ListVideo).small())
                        .child(div().flex_1().child("Export queue"))
                        .child(
                            Button::new("export-queue-clear")
                                .ghost()
                                .compact()
                                .icon(icons::icon(IconName::ListX))
                                .tooltip("Clear finished")
                                .disabled(!finished)
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.export_queue.clear_finished();
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("export-queue-close")
                                .ghost()
                                .compact()
                                .icon(icons::icon(IconName::X))
                                .tooltip("Close")
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.export_queue_open = false;
                                    cx.notify();
                                })),
                        ),
                )
                .child(list),
        )
    }

    fn export_job_row(job: &crate::export_queue::Job, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let id = job.id;
        let (state_icon, state_color) = match &job.state {
            JobState::Queued => (IconName::Clock, theme.muted_foreground),
            JobState::Running => (IconName::LoaderCircle, theme.primary),
            JobState::Done(_) => (IconName::CircleCheck, theme.success),
            JobState::Failed(_) => (IconName::CircleAlert, theme.danger),
            JobState::Cancelled => (IconName::Ban, theme.muted_foreground),
        };
        let button = |suffix: &str, icon: IconName, tooltip: &'static str| {
            Button::new(SharedString::from(format!("export-{suffix}-{id}")))
                .ghost()
                .compact()
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
            JobState::Running if job.cancelling => "Cancelling...".to_owned(),
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
            .p_2()
            .rounded(px(crate::style::radius::CONTROL))
            .border_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(icons::icon(state_icon).small().text_color(state_color))
                    .child(div().flex_1().min_w_0().truncate().child(job.name()))
                    .child(actions),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
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
                    .text_xs()
                    .text_color(theme.danger)
                    .child(error.clone()),
            );
        }
        row
    }
}
