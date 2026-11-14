//! Title library cards, CPU thumbnail cache and linked text-field inspector.
use super::*;
use crate::style::{color, hex};
use animathio_control::{TitleAddParams, title_preset_command, title_presets};

#[derive(Default)]
pub(super) struct TitleLibrary {
    size: (u32, u32),
    pub(super) templates_started: bool,
    pub(super) template_manifests: Vec<animathio_control::TemplateManifest>,
    pub(super) templates: BTreeMap<String, Arc<RenderImage>>,
    thumbnails: BTreeMap<String, Option<Arc<RenderImage>>>,
}

impl Editor {
    pub(super) fn add_title_preset(&mut self, preset: &str, start: Time, cx: &mut Context<Self>) {
        let Some(end) = start.0.checked_add(5 * TICKS_PER_SECOND) else {
            return;
        };
        let params = TitleAddParams {
            preset: preset.into(),
            fields: BTreeMap::new(),
            range: TimeRange {
                start,
                end: Time(end),
            },
        };
        match title_preset_command(&self.project, &params) {
            Ok((command, added)) => {
                if self.try_execute(&command, cx) {
                    self.select_only(added.clip_id, cx);
                    cx.notify();
                }
            }
            Err(error) => {
                self.error = Some(error.to_string());
                cx.notify();
            }
        }
    }

    pub(super) fn title_cards(&mut self, cx: &mut Context<Self>) -> Div {
        let size = project_size(&self.project);
        if self.title_library.size != size {
            self.title_library.size = size;
            self.title_library.thumbnails.clear();
        }
        if self.title_library.thumbnails.is_empty() {
            for preset in title_presets() {
                self.title_library
                    .thumbnails
                    .insert(preset.id.clone(), None);
            }
            // WASM loading, typesetting and CPU rendering stay off the UI thread.
            cx.spawn(async move |this, cx| {
                let thumbnails = cx
                    .background_spawn(async move {
                        let (Some(mut host), _, _) = load_plugins() else {
                            return BTreeMap::new();
                        };
                        let factor = 360.0 / f64::from(size.0.max(size.1));
                        let output = (
                            (f64::from(size.0) * factor).round().max(1.) as u32,
                            (f64::from(size.1) * factor).round().max(1.) as u32,
                        );
                        let mut ctx = animathio_render::RenderCtx::default();
                        let mut images = BTreeMap::new();
                        for preset in title_presets() {
                            let mut project = Project::default();
                            let settings = Command::SetSettings {
                                width: size.0,
                                height: size.1,
                                frame_rate: project.frame_rate(),
                                background: project.background(),
                                duration: Time(0),
                            };
                            if settings.apply(&mut project).is_err() {
                                continue;
                            }
                            let params = TitleAddParams {
                                preset: preset.id.clone(),
                                fields: BTreeMap::new(),
                                range: TimeRange {
                                    start: Time(0),
                                    end: Time(5 * TICKS_PER_SECOND),
                                },
                            };
                            let Ok((command, _)) = title_preset_command(&project, &params) else {
                                continue;
                            };
                            if command.apply(&mut project).is_err() {
                                continue;
                            }
                            let scene =
                                host.resolve(&evaluate(&project, Time(TICKS_PER_SECOND)), output);
                            if let Ok(frame) = animathio_render::render(&scene, &mut ctx, output)
                                && let Some(mut rgba) =
                                    image::RgbaImage::from_raw(output.0, output.1, frame.pixels)
                            {
                                for pixel in rgba.pixels_mut() {
                                    pixel.0.swap(0, 2);
                                }
                                images.insert(
                                    preset.id.clone(),
                                    Some(Arc::new(RenderImage::new([image::Frame::new(rgba)]))),
                                );
                            }
                        }
                        images
                    })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if this.title_library.size == size {
                        this.title_library.thumbnails.extend(thumbnails);
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        let mut cards = div().flex().flex_wrap().gap_2().p_1().child(
            div()
                .w_full()
                .p_2()
                .text_size(px(11.))
                .text_color(hex(color::TEXT_SECONDARY))
                .child("Double-click to add at playhead, or drag to timeline"),
        );
        for preset in title_presets() {
            let id = preset.id.clone();
            let drag = id.clone();
            let keyboard_id = id.clone();
            let thumbnail = self
                .title_library
                .thumbnails
                .get(&id)
                .and_then(Option::as_ref)
                .cloned();
            let mut preview = div()
                .w_full()
                .h(px(112.))
                .bg(hex(color::BG))
                .flex()
                .items_center()
                .justify_center();
            if let Some(image) = thumbnail {
                preview = preview.child(
                    img(image)
                        .max_w_full()
                        .max_h_full()
                        .object_fit(ObjectFit::Contain),
                );
            } else {
                preview =
                    preview.child(icons::icon(IconName::Type).text_color(hex(color::TEXT_MUTED)));
            }
            cards = cards.child(
                div()
                    .id(SharedString::from(format!("title-{id}")))
                    .w_full()
                    .rounded(px(6.))
                    .overflow_hidden()
                    .border_1()
                    .border_color(hex(color::BORDER))
                    .bg(hex(color::RAISED))
                    .cursor_pointer()
                    .hover(|s| s.border_color(hex(color::ACCENT)).bg(hex(color::HOVER)))
                    .focusable()
                    .tab_index(0)
                    .focus_visible(|s| s.border_color(hex(color::ACCENT)))
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.add_title_preset(&keyboard_id, this.position, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .child(preview)
                    .child(div().p_2().text_size(px(11.)).child(preset.name.clone()))
                    .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                        this.pool_drag = None;
                        if event.click_count() == 2 {
                            this.add_title_preset(&id, this.position, cx);
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.arm_pool_drag(PoolDrag::Title(drag.clone()), cx);
                        }),
                    ),
            );
        }
        cards
    }

    /// Show all composition text fields even when a decorative layer is selected.
    pub(super) fn title_field_rows(
        &mut self,
        id: ClipId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let clip = self.find_clip(id)?;
        let ClipKind::Plugin { params, .. } = &clip.kind else {
            return None;
        };
        let preset_id = params.get("_title_preset")?.as_str()?;
        let preset = title_presets().iter().find(|p| p.id == preset_id)?;
        let group = clip.link_group?;
        if self.selected_ids().iter().any(|selected| {
            self.find_clip(*selected)
                .is_none_or(|clip| clip.link_group != Some(group))
        }) {
            return None;
        }
        let name = preset.name.clone();
        let fields: Vec<_> = self
            .project
            .tracks()
            .iter()
            .flat_map(|t| t.clips().values())
            .filter(|c| c.link_group == Some(group))
            .filter_map(|clip| {
                let ClipKind::Plugin { params, plugin_id } = &clip.kind else {
                    return None;
                };
                let label = params.get("_title_field")?.as_str()?;
                if label.is_empty() {
                    return None;
                }
                let key = if plugin_id == "org.animathio.latex" {
                    "source"
                } else {
                    "text"
                };
                let value = match clip.params.get(key) {
                    Some(animathio_core::ParamValue::String(value)) => value.clone(),
                    _ => params.get(key)?.as_str()?.to_owned(),
                };
                Some((clip.id, label.to_owned(), key.to_owned(), value))
            })
            .collect();
        let group_ids = self
            .project
            .tracks()
            .iter()
            .flat_map(|track| track.clips().values())
            .filter(|clip| clip.link_group == Some(group))
            .map(|clip| clip.id)
            .collect();
        self.fields.retain_clips(&group_ids);
        let mut rows = div().flex().flex_col().gap_2().p_2();
        for (id, label, key, value) in fields {
            if self.plugin_model(id, &key).is_none() {
                rows = rows.child(
                    div()
                        .text_color(hex(color::TEXT_SECONDARY))
                        .child(format!("{label}: {value} (plugin unavailable)")),
                );
                continue;
            }
            let control = self.text_control(
                &format!("c{}:pt:{key}", id.0),
                InputTarget::PluginText { id, name: key },
                &value,
                window,
                cx,
            );
            rows = rows
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(hex(color::TEXT_SECONDARY))
                        .child(label),
                )
                .child(control.w_full());
        }
        Some(self.section("title-fields", &name, rows, cx))
    }
}
