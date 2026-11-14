//! Project template launcher and placeholder workflow.
use super::*;
use crate::style::{self, color, hex};
use animathio_control::{
    MediaFill, Placeholder, ProjectTemplate, TemplateFill, TemplateManifest, template_catalog,
};
use gpui_component::{
    WindowExt,
    input::{Input, InputState},
};

/// Media chosen for each placeholder in the open Fill placeholders dialog.
#[derive(Default)]
struct FillDraft(BTreeMap<ClipId, MediaFill>);

impl Editor {
    pub(super) fn template_cards(&mut self, cx: &mut Context<Self>) -> Div {
        if !self.title_library.templates_started {
            self.title_library.templates_started = true;
            cx.spawn(async move |this, cx| {
                let (manifests, images) = cx
                    .background_spawn(async move {
                        let mut images = BTreeMap::new();
                        let catalog = template_catalog().unwrap_or_default();
                        let manifests = catalog
                            .iter()
                            .map(|t| t.manifest.clone())
                            .collect::<Vec<_>>();
                        let (Some(mut host), _, _) = load_plugins() else {
                            return (manifests, images);
                        };
                        for template in catalog {
                            let Ok(mut ctx) =
                                animathio_render::RenderCtx::from_assets(template.assets.clone())
                            else {
                                continue;
                            };
                            ctx.set_effect_processor(host.effect_processor());
                            let size = project_size(&template.project);
                            let factor = 320. / f64::from(size.0.max(size.1));
                            let output = (
                                (f64::from(size.0) * factor).max(1.) as u32,
                                (f64::from(size.1) * factor).max(1.) as u32,
                            );
                            let scene = host.resolve(
                                &evaluate(&template.project, Time(TICKS_PER_SECOND)),
                                output,
                            );
                            if let Ok(frame) = animathio_render::render(&scene, &mut ctx, output)
                                && let Some(mut rgba) =
                                    image::RgbaImage::from_raw(output.0, output.1, frame.pixels)
                            {
                                for pixel in rgba.pixels_mut() {
                                    pixel.0.swap(0, 2);
                                }
                                images.insert(
                                    template.manifest.id,
                                    Arc::new(RenderImage::new([image::Frame::new(rgba)])),
                                );
                            }
                        }
                        (manifests, images)
                    })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.title_library.templates = images;
                    this.title_library.template_manifests = manifests;
                    cx.notify();
                });
            })
            .detach();
        }
        let mut cards = div().flex().flex_wrap().gap_2();
        for manifest in self.title_library.template_manifests.clone() {
            let id = manifest.id.clone();
            let delete_id = id.clone();
            let keyboard_id = id.clone();
            let mut preview = div()
                .relative()
                .h(px(100.))
                .w_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(hex(color::BG));
            if let Some(image) = self.title_library.templates.get(&id) {
                preview = preview.child(
                    img(image.clone())
                        .max_w_full()
                        .max_h_full()
                        .object_fit(ObjectFit::Contain),
                );
            } else {
                preview = preview.child(icons::icon(IconName::Type));
            }
            preview = preview.child(
                div()
                    .absolute()
                    .left(px(6.))
                    .bottom(px(6.))
                    .px_2()
                    .rounded(px(style::radius::PILL))
                    .bg(style::hexa(color::BG, 0.8))
                    .border_1()
                    .border_color(hex(color::BORDER))
                    .text_size(px(10.))
                    .text_color(hex(color::TEXT_SECONDARY))
                    .child(manifest.aspect.clone()),
            );
            let mut card = div()
                .relative()
                .id(SharedString::from(id.clone()))
                .w(px(180.))
                .p_3()
                .rounded(px(crate::style::radius::PANEL))
                .border_1()
                .border_color(hex(color::BORDER))
                .bg(hex(color::RAISED))
                .cursor_pointer()
                .focusable()
                .tab_index(0)
                .focus_visible(|s| s.border_color(hex(color::ACCENT)))
                .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        this.open_template(&keyboard_id, window, cx);
                        cx.stop_propagation();
                    }
                }))
                .hover(|s| {
                    s.top(px(-2.))
                        .bg(hex(color::HOVER))
                        .border_color(hex(color::ACCENT))
                })
                .child(preview)
                .child(
                    div()
                        .mt_2()
                        .text_size(px(13.))
                        .font_weight(FontWeight::MEDIUM)
                        .child(manifest.name),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(hex(color::TEXT_SECONDARY))
                        .child(manifest.category),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_template(&id, window, cx);
                }));
            if delete_id.starts_with("user-") {
                card = card.child(
                    div().absolute().top(px(16.)).right(px(16.)).child(
                        Button::new(SharedString::from(format!("delete-{delete_id}")))
                            .ghost()
                            .icon(icons::icon(IconName::Trash))
                            .tooltip("Delete template")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                if let Err(error) =
                                    animathio_control::delete_user_template(&delete_id)
                                {
                                    this.error = Some(error.to_string());
                                }
                                this.title_library.templates_started = false;
                                cx.notify();
                            })),
                    ),
                );
            }
            cards = cards.child(card);
        }
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_size(px(16.)).child("Templates"))
            .child(cards)
    }

    fn open_template(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(template) = template_catalog()
            .unwrap_or_default()
            .into_iter()
            .find(|t| t.manifest.id == id)
        else {
            return;
        };
        self.new_project(cx);
        self.project = template.project.clone();
        self.assets = Arc::new(template.assets.clone());
        self.next_id = next_id(&self.project);
        self.dirty = true;
        self.pool = AudioPool::default();
        if let Err(error) = load_audio(&mut self.pool, &self.project, &self.assets) {
            self.error = Some(error);
        }
        self.audio_ids = audio_asset_ids(&self.project);
        self.clock = make_clock(&self.project, &self.pool);
        self.worker.load(self.assets.clone());
        self.request_render(Time(0));
        Self::fill_template_dialog(template, window, cx);
    }

    /// Apply a template fill as one undoable batch, registering any new media first so the
    /// render triggered by the edit already finds its bytes.
    fn apply_template_fill(&mut self, fill: TemplateFill, cx: &mut Context<Self>) -> bool {
        let ids: Vec<_> = fill.assets.keys().cloned().collect();
        Arc::make_mut(&mut self.assets).extend(fill.assets);
        self.worker.load(self.assets.clone());
        if !self.try_execute(&fill.command, cx) {
            for id in ids {
                if !self.project.assets().contains_key(&id) {
                    Arc::make_mut(&mut self.assets).remove(&id);
                }
            }
            self.worker.load(self.assets.clone());
            return false;
        }
        self.pool = AudioPool::default();
        if let Err(error) = load_audio(&mut self.pool, &self.project, &self.assets) {
            self.error = Some(error);
        }
        self.audio_ids = audio_asset_ids(&self.project);
        self.clock = make_clock(&self.project, &self.pool);
        self.request_render(self.position);
        true
    }

    fn fill_template_dialog(
        template: ProjectTemplate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if template.manifest.placeholders.is_empty() {
            return;
        }
        let rows: Vec<_> = template
            .manifest
            .placeholders
            .iter()
            .map(|p| {
                let text = template.accepts_text(p.clip).then(|| {
                    cx.new(|cx| InputState::new(window, cx).placeholder("Type replacement text"))
                });
                let label = p
                    .label
                    .clone()
                    .unwrap_or_else(|| format!("Clip {}", p.clip.0));
                (p.clip, label, text)
            })
            .collect();
        let media = cx.new(|_| FillDraft::default());
        let editor = cx.entity();
        let template = Arc::new(template);
        window.open_dialog(cx, move |dialog, _, cx| {
            let mut body = div()
                .flex()
                .flex_col()
                .gap_3()
                .max_h(px(420.))
                .overflow_hidden();
            for (id, label, text) in &rows {
                let id = *id;
                let chosen = media.read(cx).0.get(&id).cloned();
                let mut row = div()
                    .id(SharedString::from(format!("placeholder-{}", id.0)))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_2()
                    .rounded(px(style::radius::CARD))
                    .border_1()
                    .border_color(style::hex(style::color::BORDER))
                    .drag_over::<ExternalPaths>(|s, _, _, _| {
                        s.border_color(style::hex(style::color::ACCENT))
                            .bg(style::hex(style::color::HOVER))
                    })
                    .on_drop::<ExternalPaths>({
                        let media = media.clone();
                        move |paths: &ExternalPaths, window, cx| {
                            if let Some(path) = paths.paths().first() {
                                media.update(cx, |m, _| {
                                    m.0.insert(
                                        id,
                                        MediaFill {
                                            path: path.clone(),
                                            cover: true,
                                        },
                                    );
                                });
                                window.refresh();
                            }
                        }
                    })
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(label.clone()),
                    );
                if let Some(text) = text {
                    row = row.child(Input::new(text));
                }
                let pick = {
                    let media = media.clone();
                    Button::new(SharedString::from(format!("pick-{}", id.0)))
                        .icon(icons::icon(IconName::FolderOpen))
                        .label(if chosen.is_some() {
                            "Change media…"
                        } else {
                            "Pick or drop media…"
                        })
                        .on_click(move |_, window, cx| {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter(
                                    "Image or video",
                                    &[
                                        "png", "jpg", "jpeg", "webp", "gif", "bmp", "mp4", "mov",
                                        "mkv", "webm", "avi",
                                    ],
                                )
                                .pick_file()
                            {
                                media.update(cx, |m, _| {
                                    let cover = m.0.get(&id).is_none_or(|f| f.cover);
                                    m.0.insert(id, MediaFill { path, cover });
                                });
                                window.refresh();
                            }
                        })
                };
                let mut controls = div().flex().items_center().gap_2().child(pick);
                if let Some(fill) = chosen {
                    let set_fit = |cover: bool| {
                        let media = media.clone();
                        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                            media.update(cx, |m, _| {
                                if let Some(f) = m.0.get_mut(&id) {
                                    f.cover = cover;
                                }
                            });
                            window.refresh();
                        }
                    };
                    let clear = media.clone();
                    controls = controls
                        .child(
                            Button::new(SharedString::from(format!("cover-{}", id.0)))
                                .label("Cover")
                                .selected(fill.cover)
                                .on_click(set_fit(true)),
                        )
                        .child(
                            Button::new(SharedString::from(format!("contain-{}", id.0)))
                                .label("Contain")
                                .selected(!fill.cover)
                                .on_click(set_fit(false)),
                        )
                        .child(
                            Button::new(SharedString::from(format!("clear-{}", id.0)))
                                .ghost()
                                .icon(icons::icon(IconName::X))
                                .on_click(move |_, window, cx| {
                                    clear.update(cx, |m, _| {
                                        m.0.remove(&id);
                                    });
                                    window.refresh();
                                }),
                        );
                    row = row.child(controls).child(
                        div()
                            .text_size(px(11.))
                            .text_color(style::hex(style::color::TEXT_SECONDARY))
                            .child(
                                fill.path
                                    .file_name()
                                    .map_or_default(|n| n.to_string_lossy().into_owned()),
                            ),
                    );
                } else {
                    row = row.child(controls);
                }
                body = body.child(row);
            }
            let (rows, media, editor, template) = (
                rows.clone(),
                media.clone(),
                editor.clone(),
                template.clone(),
            );
            dialog
                .title("Fill placeholders")
                .width(px(520.))
                .child(
                    div()
                        .mb_3()
                        .text_size(px(12.))
                        .text_color(style::hex(style::color::TEXT_SECONDARY))
                        .child(
                            "Type text, or pick/drop an image or video. Media keeps the \
                             placeholder's timing, transform and effects.",
                        ),
                )
                .child(body)
                .footer(
                    div()
                        .flex()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new("fill-skip")
                                .label("Skip")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            style::primary_button("fill-template", cx)
                                .label("Apply")
                                .on_click(move |_, window, cx| {
                                    let media = media.read(cx).0.clone();
                                    let text: BTreeMap<_, _> = rows
                                        .iter()
                                        .filter(|(id, _, _)| !media.contains_key(id))
                                        .filter_map(|(id, _, state)| {
                                            let value =
                                                state.as_ref()?.read(cx).value().to_string();
                                            (!value.trim().is_empty()).then_some((*id, value))
                                        })
                                        .collect();
                                    if text.is_empty() && media.is_empty() {
                                        window.close_dialog(cx);
                                        return;
                                    }
                                    let applied = editor.update(cx, |this, cx| {
                                        match template.fill(&text, &media) {
                                            Ok(fill) => this.apply_template_fill(fill, cx),
                                            Err(error) => {
                                                this.error = Some(error.to_string());
                                                cx.notify();
                                                false
                                            }
                                        }
                                    });
                                    if applied {
                                        window.close_dialog(cx);
                                    }
                                }),
                        ),
                )
        });
    }

    /// Label for a clip marked as a placeholder: its text snippet or asset name.
    fn placeholder_label(&self, clip: ClipId) -> String {
        self.find_clip(clip).map_or_else(
            || format!("Clip {}", clip.0),
            |c| format!("{} ({})", clip_title(&self.project, c, 0.), clip.0),
        )
    }

    pub(super) fn save_template_dialog(window: &mut Window, cx: &mut Context<Self>) {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Template name"));
        let category = cx.new(|cx| InputState::new(window, cx).default_value("My templates"));
        let editor = cx.entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let n = name.clone();
            let c = category.clone();
            let editor = editor.clone();
            dialog
                .title("Save as template")
                .child(Input::new(&name))
                .child(Input::new(&category))
                .child("Selected clips become labelled placeholders.")
                .child(
                    Button::new("save-template")
                        .label("Save template")
                        .on_click(move |_, window, cx| {
                            let name = n.read(cx).value().to_string();
                            let category = c.read(cx).value().to_string();
                            let saved = editor.update(cx, |this, cx| {
                                if name.trim().is_empty() {
                                    this.error = Some("Give the template a name".into());
                                    cx.notify();
                                    return false;
                                }
                                let placeholders = this
                                    .selected_ids()
                                    .into_iter()
                                    .map(|clip| Placeholder {
                                        clip,
                                        label: Some(this.placeholder_label(clip)),
                                    })
                                    .collect();
                                let manifest = TemplateManifest {
                                    id: "pending".into(),
                                    name: name.clone(),
                                    description: String::new(),
                                    aspect: format!(
                                        "{}:{}",
                                        this.project.width(),
                                        this.project.height()
                                    ),
                                    category: category.clone(),
                                    thumbnail: None,
                                    placeholders,
                                };
                                match animathio_control::save_user_template(ProjectTemplate {
                                    project: this.project.clone(),
                                    assets: (*this.assets).clone(),
                                    manifest,
                                }) {
                                    Ok(_) => {
                                        this.title_library.templates_started = false;
                                        cx.notify();
                                        true
                                    }
                                    Err(error) => {
                                        this.error = Some(error.to_string());
                                        cx.notify();
                                        false
                                    }
                                }
                            });
                            if saved {
                                window.close_dialog(cx);
                            }
                        }),
                )
        });
    }
}
