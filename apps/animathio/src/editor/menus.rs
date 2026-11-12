//! Title bar, application menus, context menus and status.

use super::actions::*;
use super::*;
use gpui_component::{
    TitleBar, WindowExt, global_state::GlobalState, menu::AppMenuBar, notification::Notification,
    status_bar::StatusBar,
};

impl Editor {
    pub(super) fn refresh_menus(&mut self, cx: &mut Context<Self>) {
        let signature = format!(
            "{:?}/{:?}/{:?}/{:?}/{}",
            self.recent.paths,
            self.selected,
            self.dock,
            self.preview.gpu,
            self.export_task.is_some()
        );
        if self.menu_signature == signature {
            return;
        }
        self.menu_signature = signature;
        let recent = Menu::new("Open Recent")
            .items(self.recent.paths.iter().map(|path| {
                MenuItem::action(
                    path.display().to_string(),
                    OpenRecent { path: path.clone() },
                )
            }))
            .disabled(self.recent.paths.is_empty());
        let menus = vec![
            Menu::new("File").items([
                MenuItem::action("New project", NewProject),
                MenuItem::action("Open…", OpenProject),
                MenuItem::submenu(recent),
                MenuItem::action("Save", SaveProject),
                MenuItem::separator(),
                MenuItem::action("Import media…", ImportMedia),
                MenuItem::action("Import v1…", ImportV1),
                MenuItem::action("Import Manim scene…", ImportManim),
                MenuItem::action("Export…", ExportVideo),
                MenuItem::separator(),
                MenuItem::action("Project settings…", ProjectSettings),
                MenuItem::action("Preferences…", Preferences),
            ]),
            Menu::new("Edit").items([
                MenuItem::action("Undo", Undo),
                MenuItem::action("Redo", Redo),
                MenuItem::separator(),
                MenuItem::action("Delete selection", DeleteSelection)
                    .disabled(self.selected.is_none()),
            ]),
            Menu::new("View").items([
                MenuItem::action("Media pool", ToggleMediaPool).checked(self.dock.left_open),
                MenuItem::action("Inspector", ToggleInspector).checked(self.dock.right_open),
                MenuItem::action("Timeline", ToggleTimeline).checked(self.dock.bottom_open),
                MenuItem::separator(),
                MenuItem::action("GPU preview", ToggleGpuPreview).checked(self.preview.gpu),
                MenuItem::action("Zoom to fit", ZoomToFit),
            ]),
            Menu::new("Playback").items([
                MenuItem::action("Shuttle reverse", ShuttleReverse),
                MenuItem::action("Stop", ShuttleStop),
                MenuItem::action("Shuttle forward", ShuttleForward),
                MenuItem::action("Previous frame", PreviousFrame),
                MenuItem::action("Next frame", NextFrame),
                MenuItem::action("Previous edit", PreviousEdit),
                MenuItem::action("Next edit", NextEdit),
                MenuItem::action("Start", GoToStart),
                MenuItem::action("End", GoToEnd),
                MenuItem::action("Set in", MarkIn),
                MenuItem::action("Set out (exclusive)", MarkOut),
                MenuItem::action("Clear in/out", ClearMarks),
                MenuItem::action("Go to in", GoToIn),
                MenuItem::action("Go to out", GoToOut),
                MenuItem::action("Loop", ToggleLoop),
            ]),
            Menu::new("Tools").items([
                MenuItem::action("Select", SelectTool),
                MenuItem::action("Blade", BladeTool),
                MenuItem::action("Ripple", RippleTool),
                MenuItem::action("Hand", HandTool),
            ]),
            Menu::new("Clip").items([
                MenuItem::action("Split at playhead", SplitClip).disabled(self.selected.is_none()),
                MenuItem::action("Add effect", ShowEffects),
                MenuItem::action("Ripple delete selection or range", RippleDelete),
                MenuItem::action("Write on…", WriteOn),
            ]),
            Menu::new("Help").items([
                MenuItem::action("Keyboard shortcuts…", KeyboardShortcuts),
                MenuItem::separator(),
                MenuItem::action("About AniMathIO", About),
            ]),
        ];
        GlobalState::global_mut(cx).set_app_menus(menus.into_iter().map(Menu::owned).collect());
        self.app_menu.update(cx, AppMenuBar::reload);
    }

    pub(super) fn menu_bar(&self, cx: &mut Context<Self>) -> TitleBar {
        let title = self.path.as_ref().and_then(|p| p.file_name()).map_or_else(
            || "Untitled".to_owned(),
            |n| n.to_string_lossy().into_owned(),
        );
        TitleBar::new().child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .w_full()
                .child(
                    div()
                        .w(px(460.))
                        .flex_shrink_0()
                        .h_full()
                        .child(self.app_menu.clone()),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .max_w(px(260.))
                        .truncate()
                        .text_color(cx.theme().foreground)
                        .child(title),
                )
                .when(self.dirty, |row| {
                    row.child(
                        icons::icon(IconName::CircleDot)
                            .small()
                            .text_color(cx.theme().warning),
                    )
                })
                .child(
                    div()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .bg(cx.theme().muted)
                        .text_color(if self.control.is_some() {
                            cx.theme().success
                        } else {
                            cx.theme().muted_foreground
                        })
                        .child(if self.control.is_some() {
                            "MCP: listening"
                        } else {
                            "MCP: off"
                        }),
                )
                .child(
                    Button::new("title-export")
                        .primary()
                        .small()
                        .icon(icons::icon(IconName::Upload))
                        .label("Export")
                        .tooltip_with_action("Export video", &ExportVideo, Some("Editor"))
                        .on_click(
                            cx.listener(|this, _, window, cx| this.open_export_dialog(window, cx)),
                        ),
                ),
        )
    }

    pub(super) fn flush_notices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(error) = self.error.take() {
            self.notices.push(Notification::error(error));
        }
        for notice in self.notices.drain(..) {
            window.push_notification(notice, cx);
        }
    }

    pub(super) fn context_menu_dropdown(&self, cx: &mut Context<Self>) -> Option<Div> {
        let target = self.context_menu.clone()?;
        let pos = self.context_menu_pos?;
        let mut menu = div()
            .absolute()
            .left(pos.x)
            .top(pos.y)
            .w(px(200.))
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .p_1()
            .flex()
            .flex_col()
            .shadow_lg();
        let close = cx.listener(|this: &mut Self, _: &ClickEvent, _, cx| {
            this.context_menu = None;
            cx.notify();
        });
        match target {
            ContextMenu::Clip { id } => {
                let actions: Vec<(IconName, &str, ClipAction)> = vec![
                    (IconName::Scissors, "Split at playhead", Self::split_clip),
                    (IconName::Copy, "Duplicate", Self::duplicate_clip),
                    (IconName::Trash, "Delete", Self::delete_clip),
                    (
                        IconName::TrendingUp,
                        "Fade in 1 s",
                        Self::fade_in_one_second,
                    ),
                    (
                        IconName::TrendingDown,
                        "Fade out 1 s",
                        Self::fade_out_one_second,
                    ),
                    (IconName::Eraser, "Clear fades", Self::clear_fades),
                ];
                for (icon, label, action) in actions {
                    menu = menu.child(context_item(
                        icon,
                        label,
                        cx.listener(move |this: &mut Self, _: &ClickEvent, _, cx| {
                            action(this, id, cx);
                            this.context_menu = None;
                            cx.notify();
                        }),
                        cx,
                    ));
                }
                menu = menu.child(
                    div()
                        .px_2()
                        .py_1()
                        .text_color(cx.theme().muted_foreground)
                        .child("Add effect"),
                );
                for kind in effect_kinds() {
                    menu = menu.child(context_item(
                        IconName::Sparkles,
                        effect_label(kind),
                        cx.listener(move |this: &mut Self, _: &ClickEvent, _, cx| {
                            this.add_effect_to_clip(id, kind, cx);
                            this.context_menu = None;
                            cx.notify();
                        }),
                        cx,
                    ));
                }
            }
            ContextMenu::Keyframe(selection) => {
                let remove = selection.clone();
                menu = menu
                    .child(context_item(
                        IconName::DiamondMinus,
                        "Remove keyframe",
                        cx.listener(move |this: &mut Self, _: &ClickEvent, _, cx| {
                            this.execute(
                                &Command::RemoveKeyframe {
                                    id: remove.clip_id,
                                    target: remove.target.clone(),
                                    time: remove.time,
                                },
                                cx,
                            );
                            this.selected_keyframe = None;
                            this.context_menu = None;
                            cx.notify();
                        }),
                        cx,
                    ))
                    .children(self.keyframe_menu(&selection, cx));
            }
        }
        menu = menu.child(
            div()
                .px_2()
                .py_1()
                .cursor_pointer()
                .hover(|this| this.bg(cx.theme().muted))
                .text_color(cx.theme().muted_foreground)
                .child("Close")
                .id("context-close")
                .on_click(close),
        );
        Some(menu)
    }

    /// Persistent project summary; transient results are notifications.
    pub(super) fn status_line(&self, cx: &App) -> StatusBar {
        let (width, height) = project_size(&self.project);
        let duration = timecode::format_timecode(
            Time(timeline::project_duration(&self.project)),
            self.project.frame_rate(),
        );
        StatusBar::new()
            .left(div().text_color(cx.theme().muted_foreground).child(format!(
                "{width} x {height}   {}",
                frame_rate_label(self.project.frame_rate())
            )))
            .left(
                div()
                    .font_family(cx.theme().mono_font_family.clone())
                    .child(duration),
            )
            .right(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(if self.path.is_some() {
                        format!("Autosave: {} s ago", self.last_autosave.elapsed().as_secs())
                    } else {
                        "Save project to enable autosave".into()
                    }),
            )
    }
}
