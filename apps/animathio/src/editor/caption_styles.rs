//! Caption style picker: used by the Generate subtitles dialog (chooses the look of the
//! clips about to be created) and by the inspector for a selected caption (restyles every
//! caption on its track as one `Command::Batch`, so undo is one step).

use super::*;
use crate::style::{color, hex};
use animathio_control::{
    CaptionPosition, CaptionStyleParams, SubtitleStyleParams, caption_style_command,
    caption_styles, clip_caption_style,
};

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
) -> Div {
    let info = style_info(active_style);
    let font = look.font_size.unwrap_or(info.font_size);
    let words = look.max_words.unwrap_or(info.max_words);
    let position = look.position.unwrap_or(CaptionPosition::Bottom);
    let label = |text: &str| {
        div()
            .text_size(px(11.))
            .text_color(hex(color::TEXT_SECONDARY))
            .child(text.to_owned())
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
                    .text_size(px(12.))
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
        .child(
            div()
                .text_size(px(11.))
                .text_color(hex(color::TEXT_SECONDARY))
                .child(info.description),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(label("Position"))
                .child(positions),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(label("Font size"))
                .child(stepper(
                    "font",
                    format!("{:.1}%", font * 100.0),
                    LookChange::FontDelta(-0.005),
                    LookChange::FontDelta(0.005),
                )),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(label("Words per line"))
                .child(stepper(
                    "words",
                    words_label,
                    LookChange::WordsDelta(-1),
                    LookChange::WordsDelta(1),
                )),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(label("Text colour"))
                .child(texts),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(label("Accent"))
                .child(accents),
        )
}

impl Editor {
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
        );
        Some(self.section(
            "caption-style",
            "Caption style (all captions on this track)",
            div().flex().flex_col().p_2().child(picker),
            cx,
        ))
    }
}
