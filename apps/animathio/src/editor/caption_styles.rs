//! Caption style picker: used by the Generate subtitles dialog (chooses the look of the
//! clips about to be created) and by the inspector for a selected caption (restyles every
//! caption on its track as one `Command::Batch`, so undo is one step).
#![allow(deprecated)] // 2.2 token aliases still in use (UI-rework PR 2-6 remove these)

use super::*;
use crate::style::{TextRole, color, hex};
use animathio_control::{
    CaptionPosition, CaptionPreset, CaptionStyleParams, SubtitleStyleParams, caption_style_command,
    caption_styles, clip_caption_style,
};
use gpui_component::form::Form;
use gpui_component::input::Input;

/// Accent swatches for the active word.
const ACCENTS: [(&str, [u8; 4]); 6] = [
    ("Yellow", [255, 214, 10, 255]),
    ("Cyan", [0, 229, 255, 255]),
    ("Green", [57, 255, 20, 255]),
    ("Pink", [255, 64, 160, 255]),
    ("Orange", [255, 140, 0, 255]),
    ("White", [255, 255, 255, 255]),
];

/// Text colour swatches.
const TEXTS: [(&str, [u8; 4]); 3] = [
    ("White", [255, 255, 255, 255]),
    ("Yellow", [255, 214, 10, 255]),
    ("Black", [16, 16, 24, 255]),
];

/// One edit of the remembered caption look.
#[derive(Clone)]
pub(super) enum LookChange {
    Style(String),
    Position(CaptionPosition),
    FontDelta(f64),
    WordsDelta(i32),
    Accent([u8; 4]),
    Text([u8; 4]),
    /// Adopt a saved user preset wholesale.
    Preset(CaptionStyleParams),
}

fn style_info(id: &str) -> &'static animathio_control::CaptionStyleInfo {
    caption_styles()
        .iter()
        .find(|s| s.id == id)
        .unwrap_or(&caption_styles()[0])
}

fn swatch(
    id: String,
    rgba: [u8; 4],
    selected: bool,
    editor: &Entity<Editor>,
    change: LookChange,
    target: Option<TrackId>,
) -> Stateful<Div> {
    let editor = editor.clone();
    div()
        .id(SharedString::from(id))
        .size(px(18.))
        .rounded_full()
        .cursor_pointer()
        .bg(Rgba {
            r: f32::from(rgba[0]) / 255.0,
            g: f32::from(rgba[1]) / 255.0,
            b: f32::from(rgba[2]) / 255.0,
            a: 1.0,
        })
        .border_2()
        .border_color(if selected {
            hex(color::ACCENT)
        } else {
            hex(color::BORDER)
        })
        .on_click(move |_, _, cx| {
            editor.update(cx, |this, cx| {
                this.apply_look_change(change.clone(), target, cx);
            });
        })
}

/// The picker. `target` is `None` in the dialog (only remembers the look) and
/// `Some(track)` in the inspector (also restyles that track's captions).
pub(super) fn caption_style_picker(
    editor: &Entity<Editor>,
    prefix: &str,
    active_style: &str,
    look: &CaptionStyleParams,
    target: Option<TrackId>,
    color_pickers: [Div; 2],
    presets: &[CaptionPreset],
) -> Div {
    let info = style_info(active_style);
    let font = look.font_size.unwrap_or(info.font_size);
    let words = look.max_words.unwrap_or(info.max_words);
    let position = look.position.unwrap_or(CaptionPosition::Bottom);
    // Rows use the inspector row shape, so the caption labels line up with the
    // label column of every other section (docs/design.md "Inspector").
    let row = |label: &str, control: gpui::AnyElement| {
        div()
            .min_h(crate::style::size::ROW)
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_2()
            .child(
                div()
                    .w(gpui::relative(0.4))
                    .flex_shrink_0()
                    .truncate()
                    .text_role(crate::style::Role::Label)
                    .child(label.to_owned()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(control),
            )
    };
    let choose = |id: String, text: String, selected: bool, change: LookChange| {
        let editor = editor.clone();
        Button::new(SharedString::from(id))
            .ghost()
            .compact()
            .label(text)
            .selected(selected)
            .on_click(move |_, _, cx| {
                editor.update(cx, |this, cx| {
                    this.apply_look_change(change.clone(), target, cx);
                });
            })
    };
    let mut styles = div().flex().flex_wrap().gap_1();
    for style in caption_styles() {
        styles = styles.child(choose(
            format!("{prefix}-style-{}", style.id),
            style.name.to_owned(),
            style.id == active_style,
            LookChange::Style(style.id.to_owned()),
        ));
    }
    let mut positions = div().flex().gap_1();
    for (name, value) in [
        ("Top", CaptionPosition::Top),
        ("Middle", CaptionPosition::Middle),
        ("Bottom", CaptionPosition::Bottom),
    ] {
        positions = positions.child(choose(
            format!("{prefix}-pos-{name}"),
            name.to_owned(),
            position == value,
            LookChange::Position(value),
        ));
    }
    let stepper = |id: &str, value: String, down: LookChange, up: LookChange| {
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(choose(
                format!("{prefix}-{id}-down"),
                "-".into(),
                false,
                down,
            ))
            .child(
                div()
                    .min_w(px(44.))
                    .text_center()
                    .text_size(crate::style::type_size::SM)
                    .child(value),
            )
            .child(choose(format!("{prefix}-{id}-up"), "+".into(), false, up))
    };
    let accent = look.accent.unwrap_or(ACCENTS[0].1);
    let text = look.color.unwrap_or(TEXTS[0].1);
    let mut accents = div().flex().items_center().gap_1();
    for (name, rgba) in ACCENTS {
        accents = accents.child(swatch(
            format!("{prefix}-accent-{name}"),
            rgba,
            accent == rgba,
            editor,
            LookChange::Accent(rgba),
            target,
        ));
    }
    let mut texts = div().flex().items_center().gap_1();
    for (name, rgba) in TEXTS {
        texts = texts.child(swatch(
            format!("{prefix}-text-{name}"),
            rgba,
            text == rgba,
            editor,
            LookChange::Text(rgba),
            target,
        ));
    }
    let [text_picker, accent_picker] = color_pickers;
    texts = texts.child(text_picker);
    accents = accents.child(accent_picker);
    // Named user presets from the user data directory: click applies the look, the
    // trailing icon deletes the preset, and the bookmark button saves the current
    // look under a chosen name.
    let mut preset_row = div().flex().flex_wrap().items_center().gap_1();
    for preset in presets {
        let remove_id = preset.id.clone();
        let remove = Button::new(SharedString::from(format!(
            "{prefix}-preset-remove-{}",
            preset.id
        )))
        .ghost()
        .compact()
        .icon(icons::icon(IconName::X).xsmall())
        .tooltip("Delete preset")
        .on_click({
            let editor = editor.clone();
            move |_, _, cx| {
                editor.update(cx, |this, cx| this.remove_caption_preset(&remove_id, cx));
            }
        });
        preset_row = preset_row.child(
            div()
                .flex()
                .items_center()
                .child(choose(
                    format!("{prefix}-preset-{}", preset.id),
                    preset.name.clone(),
                    false,
                    LookChange::Preset(preset.params.clone()),
                ))
                .child(remove),
        );
    }
    preset_row = preset_row.child(
        Button::new(SharedString::from(format!("{prefix}-preset-save")))
            .ghost()
            .compact()
            .icon(icons::icon(IconName::Bookmark).xsmall())
            .label("Save preset")
            .tooltip("Save the current look as a named preset")
            .on_click({
                let editor = editor.clone();
                move |_, window, cx| {
                    editor.update(cx, |this, cx| this.open_save_caption_preset(window, cx));
                }
            }),
    );
    let words_label = if words == 0 {
        "Auto".to_owned()
    } else {
        words.to_string()
    };
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(styles)
        .child(row("Presets", preset_row.into_any_element()))
        .child(
            div()
                .px_2()
                .text_role(crate::style::Role::Label)
                .text_color(hex(color::TEXT_MUTED))
                .child(info.description),
        )
        .child(row("Position", positions.into_any_element()))
        .child(row(
            "Font size",
            stepper(
                "font",
                format!("{:.1}%", font * 100.0),
                LookChange::FontDelta(-0.005),
                LookChange::FontDelta(0.005),
            )
            .into_any_element(),
        ))
        .child(row(
            "Words per line",
            stepper(
                "words",
                words_label,
                LookChange::WordsDelta(-1),
                LookChange::WordsDelta(1),
            )
            .into_any_element(),
        ))
        .child(row("Text color", texts.into_any_element()))
        .child(row("Accent", accents.into_any_element()))
}

fn caption_library() -> std::path::PathBuf {
    crate::editor_env::config_file("caption-presets")
        .unwrap_or_else(|| crate::editor_env::plugin_cache_dir().join("caption-presets"))
}

impl Editor {
    /// Refresh the named-preset list from the user data directory.
    pub(super) fn refresh_caption_presets(&mut self) {
        self.caption_presets_loaded = true;
        match animathio_control::caption_presets_at(&caption_library()) {
            Ok(presets) => self.caption_presets = presets,
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    pub(super) fn ensure_caption_presets(&mut self) {
        if !self.caption_presets_loaded {
            self.refresh_caption_presets();
        }
    }

    /// Delete a saved preset and refresh the list.
    pub(super) fn remove_caption_preset(&mut self, id: &str, cx: &mut Context<Self>) {
        match animathio_control::delete_caption_preset_at(id, &caption_library()) {
            Ok(()) => self.refresh_caption_presets(),
            Err(error) => self.error = Some(error.to_string()),
        }
        cx.notify();
    }

    /// Ask for a name and save the current look under it.
    pub(super) fn open_save_caption_preset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_caption_presets();
        let name = super::dialogs::input("", window, cx);
        let editor = cx.entity();
        super::dialogs::open_dialog(window, cx, move |dialog, _, cx| {
            let name_submit = name.clone();
            let editor = editor.clone();
            // The Save button and Enter share one action.
            let save_preset: super::dialogs::Submit = {
                let (name_submit, editor) = (name_submit.clone(), editor.clone());
                std::rc::Rc::new(move |cx: &mut gpui::App| {
                    let value = name_submit.read(cx).value().trim().to_owned();
                    editor.update(cx, |this, cx| {
                        let look = this.caption_look.clone();
                        match animathio_control::save_caption_preset_at(
                            &value,
                            &look,
                            &caption_library(),
                        ) {
                            Ok(_) => {
                                this.refresh_caption_presets();
                                true
                            }
                            Err(error) => {
                                this.error = Some(error.to_string());
                                cx.notify();
                                false
                            }
                        }
                    })
                })
            };
            let dialog = dialog
                .title(super::dialogs::heading(
                    IconName::Bookmark,
                    "Save caption preset",
                ))
                .child(Form::new().child(super::dialogs::field(
                    "Preset name",
                    super::dialogs::tagged_input("caption-preset-name", Input::new(&name)),
                )))
                .footer(super::dialogs::submit_footer("Save", cx));
            // Enter saves the preset, like the primary button.
            super::dialogs::on_enter(dialog, move |_, cx| save_preset(cx))
        });
    }

    /// Update the remembered look; with a `target`, restyle the existing captions too.
    pub(super) fn apply_look_change(
        &mut self,
        change: LookChange,
        target: Option<TrackId>,
        cx: &mut Context<Self>,
    ) {
        let look = &mut self.caption_look;
        match change {
            LookChange::Style(id) => {
                // Size and words per line are style-specific; colours and position carry over.
                look.style = id;
                look.font_size = None;
                look.max_words = None;
            }
            LookChange::Position(position) => look.position = Some(position),
            LookChange::FontDelta(delta) => {
                let base = look
                    .font_size
                    .unwrap_or_else(|| style_info(&look.style).font_size);
                look.font_size =
                    Some(((base + delta) * 1000.0).round().clamp(10.0, 200.0) / 1000.0);
            }
            LookChange::WordsDelta(delta) => {
                let base = i64::from(
                    look.max_words
                        .unwrap_or_else(|| style_info(&look.style).max_words),
                );
                look.max_words = Some((base + i64::from(delta)).clamp(0, 12) as u32);
            }
            LookChange::Accent(rgba) => look.accent = Some(rgba),
            LookChange::Text(rgba) => look.color = Some(rgba),
            LookChange::Preset(params) => look.clone_from(&params),
        }
        if let Some(track) = target {
            let params = SubtitleStyleParams {
                track: Some(track),
                style: self.caption_look.clone(),
            };
            match caption_style_command(&self.project, &params) {
                Ok((command, _)) => {
                    self.try_execute(&command, cx);
                }
                Err(error) => self.error = Some(error.to_string()),
            }
        }
        cx.notify();
    }

    /// The caption text clip belonging to `id` (itself or its linked partner) and its track.
    fn caption_text_of(&self, id: ClipId) -> Option<(TrackId, &Clip)> {
        let clip = self.find_clip(id)?;
        let group = clip.link_group;
        self.project.tracks().iter().find_map(|track| {
            track
                .clips()
                .values()
                .find(|c| {
                    clip_caption_style(c).is_some()
                        && (c.id == id || (group.is_some() && c.link_group == group))
                })
                .map(|c| (track.id(), c))
        })
    }

    /// Inspector section for a selected caption: restyles every caption on its track.
    pub(super) fn caption_style_section(
        &mut self,
        id: ClipId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let (track, clip) = self.caption_text_of(id)?;
        let active = clip_caption_style(clip)?;
        // Follow the selected clip's style; colours and position overrides carry over.
        if self.caption_look.style != active {
            self.caption_look.style.clone_from(&active);
            self.caption_look.font_size = None;
            self.caption_look.max_words = None;
        }
        self.ensure_caption_presets();
        let color_pickers = [
            self.caption_color_control(
                format!("c{}:caption-text:{}", id.0, track.0),
                false,
                Some(track),
                window,
                cx,
            ),
            self.caption_color_control(
                format!("c{}:caption-accent:{}", id.0, track.0),
                true,
                Some(track),
                window,
                cx,
            ),
        ];
        let editor = cx.entity();
        let picker = caption_style_picker(
            &editor,
            "inspector-caption",
            &active,
            &self.caption_look,
            Some(track),
            color_pickers,
            &self.caption_presets.clone(),
        );
        Some(self.section(
            "caption-style",
            "Caption style (all captions on this track)",
            div().flex().flex_col().p_2().child(picker),
            cx,
        ))
    }
}
