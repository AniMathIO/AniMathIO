//! Menus.

use super::*;

impl Editor {
    pub(super) fn menu_bar(&self, cx: &mut Context<Self>) -> Div {
        let title = self.path.as_ref().map_or_else(
            || "Untitled".to_owned(),
            |p| {
                p.file_name().map_or_else(
                    || p.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                )
            },
        );
        let mut title_row = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .text_color(cx.theme().foreground)
            .child(title);
        if self.dirty {
            title_row = title_row.child(
                div()
                    .id("dirty-indicator")
                    .text_color(cx.theme().warning)
                    .child(icons::icon(IconName::CircleDot).small())
                    .tooltip(|window, cx| {
                        gpui_component::tooltip::Tooltip::new("Unsaved changes").build(window, cx)
                    }),
            );
        }
        div()
            .h(px(34.))
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .px_1()
            .bg(cx.theme().background)
            .child(
                Button::new("menu-file")
                    .ghost()
                    .compact()
                    .label("File")
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_menu(MenuId::File, cx))),
            )
            .child(
                Button::new("menu-edit")
                    .ghost()
                    .compact()
                    .label("Edit")
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_menu(MenuId::Edit, cx))),
            )
            .child(
                Button::new("menu-view")
                    .ghost()
                    .compact()
                    .label("View")
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_menu(MenuId::View, cx))),
            )
            .child(
                Button::new("menu-clip")
                    .ghost()
                    .compact()
                    .label("Clip")
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_menu(MenuId::Clip, cx))),
            )
            .child(
                Button::new("menu-help")
                    .ghost()
                    .compact()
                    .label("Help")
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_menu(MenuId::Help, cx))),
            )
            .child(div().flex_1())
            .child(title_row)
    }

    /// A dropdown listing the actions of the open menu, or `None` when closed.
    pub(super) fn menu_dropdown(&self, cx: &mut Context<Self>) -> Option<Div> {
        let menu = self.menu_open?;
        // Anchor the list under its own menu-bar button.
        let menu_index = match menu {
            MenuId::File => 0.0,
            MenuId::Edit => 1.0,
            MenuId::View => 2.0,
            MenuId::Clip => 3.0,
            MenuId::Help => 4.0,
        };
        let items: Vec<(&str, MenuAction)> = match menu {
            MenuId::File => vec![
                ("Open… Ctrl+O", Box::new(Self::open)),
                ("Save Ctrl+S", Box::new(|this, _cx| this.save())),
                ("Import media…", Box::new(Self::import_media)),
                ("Export…", Box::new(Self::open_export_dialog)),
                ("Settings", Box::new(Self::toggle_settings)),
            ],
            MenuId::Edit => vec![
                ("Undo Ctrl+Z", Box::new(Self::undo)),
                ("Redo Ctrl+Shift+Z", Box::new(Self::redo)),
                ("Delete selection Del", Box::new(Self::delete_selected)),
            ],
            MenuId::View => vec![
                (
                    "Toggle media pool",
                    Box::new(|this, cx| this.toggle_panel(PanelSide::Left, cx)),
                ),
                (
                    "Toggle inspector",
                    Box::new(|this, cx| this.toggle_panel(PanelSide::Right, cx)),
                ),
                (
                    "Toggle timeline",
                    Box::new(|this, cx| this.toggle_panel(PanelSide::Bottom, cx)),
                ),
                (
                    if self.preview.gpu {
                        "GPU preview: on"
                    } else {
                        "GPU preview: off"
                    },
                    Box::new(Self::toggle_gpu_preview),
                ),
            ],
            MenuId::Clip => vec![
                ("Split at playhead S", Box::new(Self::split_at_playhead)),
                (
                    "Add effect",
                    Box::new(|this, cx| this.select_left_tab(LeftTab::Effects, cx)),
                ),
            ],
            MenuId::Help => vec![("AniMathIO 2.0 — DaVinci-style editor", Box::new(|_, _| {}))],
        };
        let mut dropdown = div()
            .absolute()
            .top(px(34.))
            .left(px(4. + 50. * menu_index))
            .w(px(220.))
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .p_1()
            .flex()
            .flex_col()
            .shadow_lg();
        for (label, action) in items {
            dropdown = dropdown.child(
                div()
                    .px_2()
                    .py_1()
                    .cursor_pointer()
                    .hover(|this| this.bg(cx.theme().list_hover))
                    .child(label)
                    .id("menu-item")
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.menu_open = None;
                        action(this, cx);
                    })),
            );
        }
        Some(dropdown)
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

    /// A small chip that follows the pointer while an item is dragged from the
    /// media pool toward the timeline.
    pub(super) fn status_line(&self, cx: &App) -> Div {
        let error = self.error.clone().unwrap_or_default();
        // Completed-work notices are not errors, so they never use the danger colour.
        let (notice_color, notice_icon) = if error.starts_with("Exported") {
            (cx.theme().success, IconName::CircleCheck)
        } else {
            (cx.theme().danger, IconName::TriangleAlert)
        };
        let path = self
            .path
            .as_ref()
            .map_or_else(|| "untitled".into(), |p| p.display().to_string());
        div()
            .h(px(24.))
            .w_full()
            .flex()
            .flex_row()
            .justify_between()
            .px_2()
            .bg(cx.theme().status_bar)
            .border_t_1()
            .border_color(cx.theme().status_bar_border)
            .text_color(cx.theme().muted_foreground)
            .items_center()
            .child(format!(
                "{path} | tracks {} | clips {}",
                self.project.tracks().len(),
                self.project
                    .tracks()
                    .iter()
                    .map(|t| t.clips().len())
                    .sum::<usize>()
            ))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .text_color(notice_color)
                    .when(!error.is_empty(), |this| {
                        this.child(icons::icon(notice_icon).small())
                    })
                    .child(error),
            )
    }
}
