//! Optional "describe it in words" LaTeX authoring inside the Math dialog.
//!
//! Off until the user sets up an OpenAI-compatible endpoint. Nothing is sent automatically:
//! only the **Convert** button sends the description, on a worker thread, with a Cancel button
//! that aborts the request. The API key lives in the operating system keyring (one entry per
//! origin, `scheme://host:port`, so switching provider or scheme never sends a key to the wrong
//! server); base URL and model are the non-secret `assist.json` settings, read once and cached.
//! The keyring is touched only by a user action (opening Set up, Save, Remove, Convert), never
//! when the panel is built, because that can raise a keyring unlock prompt. The result replaces the formula field, so the live
//! LaTeX preview validates it and the user edits it before **Add** inserts it as one undoable
//! command, the same path as typing LaTeX. All provider logic is in `animathio-assist`.

use std::collections::HashMap;
use std::sync::Arc;

use animathio_assist::{
    AssistError, AssistSettings, CancelToken, Converter, Endpoint, KeyUse, PRESETS, Secret,
};
use gpui::{prelude::*, *};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputState, Textarea, TextareaState};
use gpui_component::{Disableable, Sizable};

use super::tool_panel;
use crate::icons::{self, IconName};
use crate::style::{self, TextRole as _, color, hex};

/// Whether the "Describe in words" disclosure was left open; remembered for the app's lifetime
/// (an app global, so separate test windows never share it) so a user who works with it does not
/// reopen it on every dialog.
struct AssistOpenMemory(bool);

impl Global for AssistOpenMemory {}

/// The converter the editor was built with ([`crate::editor_env::EditorEnv::assist`]).
pub(super) struct AssistBackend(pub(super) Arc<dyn Converter>);

impl Global for AssistBackend {}

/// How often the editor checks whether the worker thread has an answer.
const RESULT_POLL: std::time::Duration = std::time::Duration::from_millis(25);

/// Account name of the keyring entry; the entry itself is addressed by origin.
const KEY_ACCOUNT: &str = "api-key";

/// The keyring address of the API key for `origin` (`https://api.openai.com:443`): scheme,
/// host and port, so `http` and `https` of one host, or two ports, never share a key.
fn credential_url(origin: &str) -> String {
    format!("animathio-assist:{origin}")
}

fn settings_path() -> Option<std::path::PathBuf> {
    crate::editor_env::config_file("assist.json")
}

/// Settings and key status shared by every panel of the editor, so rebuilding the Math dialog
/// reads neither `assist.json` nor the keyring again.
#[derive(Default)]
struct AssistState {
    /// `assist.json`, read on first use and replaced when the setup form saves.
    settings: Option<AssistSettings>,
    /// What the keyring said about each origin, learnt from a user action.
    keys: HashMap<String, KeyStatus>,
}

impl Global for AssistState {}

fn cached_settings(cx: &mut App) -> AssistSettings {
    let state = cx.default_global::<AssistState>();
    state
        .settings
        .get_or_insert_with(|| {
            settings_path().map_or_else(AssistSettings::default, |path| AssistSettings::load(&path))
        })
        .clone()
}

fn cached_key_status(cx: &App, origin: &str) -> Option<KeyStatus> {
    cx.try_global::<AssistState>()?.keys.get(origin).cloned()
}

#[cfg(test)]
pub(super) fn cached_key_status_for_tests(cx: &App, origin: &str) -> Option<KeyStatus> {
    cached_key_status(cx, origin)
}

pub(super) fn set_key_status(cx: &mut App, origin: &str, status: KeyStatus) {
    cx.default_global::<AssistState>()
        .keys
        .insert(origin.to_owned(), status);
}

/// Emitted when a conversion produced LaTeX for the formula field.
pub(super) enum AssistEvent {
    Converted(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum KeyStatus {
    Unknown,
    Saved,
    Missing,
    Unavailable(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Message {
    Info(String),
    Error(String),
}

pub(super) struct AssistPanel {
    settings: AssistSettings,
    converter: Arc<dyn Converter>,
    prose: Entity<TextareaState>,
    base_url: Entity<InputState>,
    model: Entity<InputState>,
    api_key: Entity<InputState>,
    setup_open: bool,
    /// The disclosure is closed by default so the LaTeX form fits without scrolling; once
    /// opened it stays open for the rest of the session ([`AssistOpenMemory`]).
    open: bool,
    converting: bool,
    cancel: Option<CancelToken>,
    generation: u64,
    message: Option<Message>,
}

impl EventEmitter<AssistEvent> for AssistPanel {}

impl AssistPanel {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = cached_settings(cx);
        let converter = cx.global::<AssistBackend>().0.clone();
        let prose = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 4)
                .placeholder("For example: the integral of x squared from 0 to 1")
        });
        let base_url = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("https://api.openai.com/v1")
                .default_value(settings.base_url.clone())
        });
        let model = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("model name")
                .default_value(settings.model.clone())
        });
        let api_key = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("API key (kept in the system keyring)")
                .masked(true)
        });
        Self {
            setup_open: !settings.is_configured(),
            open: cx
                .try_global::<AssistOpenMemory>()
                .is_some_and(|memory| memory.0),
            settings,
            converter,
            prose,
            base_url,
            model,
            api_key,
            converting: false,
            cancel: None,
            generation: 0,
            message: None,
        }
    }

    /// Whether a conversion is running.
    #[cfg(test)]
    pub(super) fn is_converting(&self) -> bool {
        self.converting
    }

    #[cfg(test)]
    pub(super) fn message(&self) -> Option<&Message> {
        self.message.as_ref()
    }

    /// Replace the configuration in memory (tests; the setup form saves to disk).
    #[cfg(test)]
    pub(super) fn set_settings(&mut self, settings: AssistSettings) {
        self.settings = settings;
    }

    #[cfg(test)]
    pub(super) fn set_prose(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.prose
            .update(cx, |state, cx| state.set_value(text.to_owned(), window, cx));
    }

    /// Fill the setup form as a user would (tests).
    #[cfg(test)]
    pub(super) fn set_form(
        &mut self,
        (base_url, model, key): (&str, &str, &str),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for (state, text) in [
            (&self.base_url, base_url),
            (&self.model, model),
            (&self.api_key, key),
        ] {
            state.update(cx, |state, cx| state.set_value(text.to_owned(), window, cx));
        }
    }

    #[cfg(test)]
    pub(super) fn api_key_field(&self, cx: &App) -> String {
        self.api_key.read(cx).value().to_string()
    }

    #[cfg(test)]
    pub(super) fn settings(&self) -> &AssistSettings {
        &self.settings
    }

    /// What is known about the saved key of the configured server (from the shared cache).
    pub(super) fn key_status(&self, cx: &App) -> KeyStatus {
        self.settings
            .origin()
            .ok()
            .and_then(|origin| cached_key_status(cx, &origin))
            .unwrap_or(KeyStatus::Unknown)
    }

    /// Look the saved key up (asynchronously) so the setup form can say whether one exists.
    /// Only called for a user action. Without `force` a known answer is kept, so reopening the
    /// form does not ask the keyring again. Plain `http://` network servers never use a key.
    fn refresh_key_status(&mut self, force: bool, cx: &mut Context<Self>) {
        let Ok(origin) = self.settings.origin() else {
            return;
        };
        if self.settings.key_use() == Ok(KeyUse::Never) {
            return;
        }
        let known = cached_key_status(cx, &origin);
        if !force && matches!(known, Some(KeyStatus::Saved | KeyStatus::Missing)) {
            return;
        }
        let task = cx.read_credentials(&credential_url(&origin));
        cx.spawn(async move |this, cx| {
            let status = match task.await {
                Ok(Some(_)) => KeyStatus::Saved,
                Ok(None) => KeyStatus::Missing,
                Err(error) => KeyStatus::Unavailable(error.to_string()),
            };
            let _ = this.update(cx, |_, cx| {
                set_key_status(cx, &origin, status);
                cx.notify();
            });
        })
        .detach();
    }

    #[cfg(test)]
    pub(super) fn is_setup_open(&self) -> bool {
        self.setup_open
    }

    #[cfg(test)]
    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    /// Expand or collapse the panel and remember the choice for later panels this session.
    pub(super) fn toggle_open(&mut self, cx: &mut Context<Self>) {
        self.open = !self.open;
        cx.set_global(AssistOpenMemory(self.open));
        cx.notify();
    }

    /// Open or close the setup form. Opening it is the user action that may read the keyring.
    pub(super) fn toggle_setup(&mut self, cx: &mut Context<Self>) {
        self.setup_open = !self.setup_open;
        if self.setup_open {
            self.refresh_key_status(false, cx);
        }
        cx.notify();
    }

    fn apply_preset(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(preset) = PRESETS.get(index) else {
            return;
        };
        self.base_url
            .update(cx, |state, cx| state.set_value(preset.base_url, window, cx));
        self.model
            .update(cx, |state, cx| state.set_value(preset.model, window, cx));
        cx.notify();
    }

    /// Save the setup form: settings to `assist.json`, a typed key to the keyring.
    pub(super) fn save_setup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = AssistSettings {
            base_url: self.base_url.read(cx).value().trim().to_owned(),
            model: self.model.read(cx).value().trim().to_owned(),
        };
        if settings.is_configured()
            && let Err(error) = settings.host()
        {
            self.message = Some(Message::Error(error.to_string()));
            cx.notify();
            return;
        }
        let typed = Secret::new(&self.api_key.read(cx).value());
        // A key for a plain http:// network server would never be sent: refuse it up front.
        if let Some(secret) = &typed
            && let Err(error) = Endpoint::new(&settings, Some(secret.clone()))
        {
            self.message = Some(Message::Error(error.to_string()));
            cx.notify();
            return;
        }
        if let Some(path) = settings_path()
            && let Err(error) = settings.save(&path)
        {
            self.message = Some(Message::Error(format!(
                "Could not save the settings: {error}"
            )));
            cx.notify();
            return;
        }
        cx.default_global::<AssistState>().settings = Some(settings.clone());
        self.settings = settings;
        // The key leaves the text field at once; only the keyring task holds it.
        self.api_key
            .update(cx, |state, cx| state.set_value(String::new(), window, cx));
        self.message = Some(Message::Info(
            if self.settings.is_configured() {
                "Saved."
            } else {
                "AI assist is off."
            }
            .into(),
        ));
        match (self.settings.origin(), typed) {
            (Ok(origin), Some(secret)) => {
                let task = cx.write_credentials(
                    &credential_url(&origin),
                    KEY_ACCOUNT,
                    secret.expose().as_bytes(),
                );
                cx.spawn(async move |this, cx| {
                    let result = task.await;
                    let _ = this.update(cx, |this, cx| {
                        match result {
                            Ok(()) => set_key_status(cx, &origin, KeyStatus::Saved),
                            Err(error) => {
                                log::warn!("saving the AI assist key failed: {error}");
                                set_key_status(
                                    cx,
                                    &origin,
                                    KeyStatus::Unavailable(error.to_string()),
                                );
                                this.message = Some(Message::Error(format!(
                                    "Could not save the key: the system keyring is not \
                                     available ({error})."
                                )));
                            }
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
            _ => self.refresh_key_status(true, cx),
        }
        cx.notify();
    }

    /// Delete the saved key of the configured server.
    fn remove_key(&mut self, cx: &mut Context<Self>) {
        let Ok(origin) = self.settings.origin() else {
            return;
        };
        let task = cx.delete_credentials(&credential_url(&origin));
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => set_key_status(cx, &origin, KeyStatus::Missing),
                    Err(error) => {
                        this.message = Some(Message::Error(format!(
                            "Could not remove the key from the system keyring: {error}"
                        )));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Send the description to the configured model. Explicit user action only; runs on a
    /// dedicated thread (the HTTP call blocks), so the UI never blocks. The outcome arrives as
    /// an [`AssistEvent`] or a message. The keyring is read here only when a key can matter: an
    /// online service always, a local server only when a key is known to be saved for it, and
    /// never for a plain `http://` network server.
    pub(super) fn convert(&mut self, cx: &mut Context<Self>) {
        if self.converting {
            return;
        }
        let prose = self.prose.read(cx).value().to_string();
        let settings = self.settings.clone();
        let (origin, key_use) = match Endpoint::new(&settings, None) {
            Ok(endpoint) => (endpoint.origin().to_owned(), endpoint.key_use()),
            Err(error) => return self.fail(error, cx),
        };
        if prose.trim().is_empty() {
            return self.fail(AssistError::EmptyInput, cx);
        }
        let read_key = match key_use {
            KeyUse::Required => true,
            KeyUse::Optional => cached_key_status(cx, &origin) == Some(KeyStatus::Saved),
            KeyUse::Never => false,
        };
        self.generation += 1;
        let generation = self.generation;
        let token = CancelToken::new();
        self.cancel = Some(token.clone());
        self.converting = true;
        self.message = None;
        let converter = self.converter.clone();
        let key_task = read_key.then(|| cx.read_credentials(&credential_url(&origin)));
        cx.spawn(async move |this, cx| {
            let mut key_problem = None;
            let mut key_status = None;
            let key = match key_task {
                None => None,
                Some(task) => match task.await {
                    Ok(Some((_, bytes))) => {
                        key_status = Some(KeyStatus::Saved);
                        String::from_utf8(bytes)
                            .ok()
                            .and_then(|text| Secret::new(&text))
                    }
                    Ok(None) => {
                        key_status = Some(KeyStatus::Missing);
                        None
                    }
                    Err(error) => {
                        log::warn!("reading the AI assist key failed: {error}");
                        key_status = Some(KeyStatus::Unavailable(error.to_string()));
                        key_problem = Some(error.to_string());
                        None
                    }
                },
            };
            if token.is_cancelled() {
                // The keyring was read: keep its answer so the next Convert need not read it again.
                if let Some(status) = key_status {
                    let _ = this.update(cx, |_, cx| set_key_status(cx, &origin, status));
                }
                return;
            }
            let had_key = key.is_some();
            let (tx, rx) = std::sync::mpsc::channel();
            let worker = {
                let token = token.clone();
                std::thread::Builder::new()
                    .name("animathio-assist".into())
                    .spawn(move || {
                        let result = Endpoint::new(&settings, key)
                            .and_then(|endpoint| converter.convert(&endpoint, &prose, &token));
                        let _ = tx.send(result);
                    })
            };
            let result = match worker {
                // Poll the worker's channel: no other thread ever wakes this task.
                Ok(_) => loop {
                    match rx.try_recv() {
                        Ok(result) => break result,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            break Err(AssistError::Network(
                                "the request stopped unexpectedly".into(),
                            ));
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => {
                            if token.is_cancelled() {
                                if let Some(status) = key_status {
                                    let _ = this
                                        .update(cx, |_, cx| set_key_status(cx, &origin, status));
                                }
                                return;
                            }
                            cx.background_executor().timer(RESULT_POLL).await;
                        }
                    }
                },
                Err(error) => Err(AssistError::Network(format!(
                    "could not start the request: {error}"
                ))),
            };
            let _ = this.update(cx, |this, cx| {
                if let Some(status) = key_status {
                    set_key_status(cx, &origin, status);
                }
                if this.generation != generation {
                    return;
                }
                this.converting = false;
                this.cancel = None;
                match result {
                    Ok(latex) => {
                        this.message = Some(Message::Info(
                            "Inserted into the formula field. Check the preview, edit it if \
                             needed, then press Add."
                                .into(),
                        ));
                        cx.emit(AssistEvent::Converted(latex));
                    }
                    Err(AssistError::Cancelled) => this.message = None,
                    Err(error @ AssistError::Auth { .. }) if !had_key => {
                        let hint = match (key_problem, key_use) {
                            (Some(problem), _) => {
                                format!("The system keyring could not be read: {problem}")
                            }
                            (None, KeyUse::Optional) => "If this server needs a key, open Set up \
                                so the saved key for it is loaded, or add one."
                                .to_owned(),
                            (None, KeyUse::Never) => "A key is never sent over plain http://; \
                                use an https:// address for a server that needs one."
                                .to_owned(),
                            (None, KeyUse::Required) => {
                                "No API key is saved for this server; add one in Set up.".to_owned()
                            }
                        };
                        this.message = Some(Message::Error(format!("{error} {hint}")));
                    }
                    Err(error) => this.message = Some(Message::Error(error.to_string())),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn fail(&mut self, error: AssistError, cx: &mut Context<Self>) {
        self.message = Some(Message::Error(error.to_string()));
        cx.notify();
    }

    /// Abort the running conversion: the request is closed and its result is ignored.
    pub(super) fn cancel(&mut self, cx: &mut Context<Self>) {
        if let Some(token) = self.cancel.take() {
            token.cancel();
        }
        self.generation += 1;
        if self.converting {
            self.converting = false;
            self.message = Some(Message::Info("Cancelled.".into()));
        }
        cx.notify();
    }

    fn notice(&self, key: &KeyStatus) -> String {
        let (Ok(host), Ok(local), Ok(key_use)) = (
            self.settings.host(),
            self.settings.is_local(),
            self.settings.key_use(),
        ) else {
            return String::new();
        };
        let key = match key_use {
            KeyUse::Required => " and your saved API key (if any)",
            KeyUse::Optional if *key == KeyStatus::Saved => " and your API key",
            KeyUse::Optional | KeyUse::Never => "",
        };
        let scope = if local {
            "a server on your own machine or network"
        } else {
            "an online service"
        };
        format!(
            "Convert sends only the description above{key} to {host}, {scope} (model {}). \
             Nothing is sent until you press it.",
            self.settings.model.trim()
        )
    }

    fn key_line(&self, key: &KeyStatus) -> String {
        key_status_line(self.settings.key_use().ok(), key)
    }
}

/// The setup form's one-line key state.
fn key_status_line(key_use: Option<KeyUse>, key: &KeyStatus) -> String {
    if key_use == Some(KeyUse::Never) {
        return "API key: not used, this server is reached over plain http:// and a key \
                would not be encrypted. Use https:// for a server that needs one."
            .into();
    }
    match key {
        KeyStatus::Unknown => "API key: not checked yet".into(),
        KeyStatus::Saved => "API key: saved in the system keyring for this server".into(),
        KeyStatus::Missing => "API key: none saved (local servers need none)".into(),
        KeyStatus::Unavailable(error) => format!(
            "Keyring not available: {error}. A key cannot be saved or read here; local \
             servers need none."
        ),
    }
}

impl Drop for AssistPanel {
    /// Closing the dialog (or switching clips) abandons a request in flight: close its
    /// connection instead of leaving the worker waiting on the network.
    fn drop(&mut self) {
        if let Some(token) = self.cancel.take() {
            token.cancel();
        }
    }
}

impl Render for AssistPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let key = self.key_status(cx);
        let secondary = hex(color::TEXT_SECONDARY);
        let configured = self.settings.is_configured();
        let open = self.open;
        let mut panel = div().id("assist-panel").flex().flex_col().gap_2().child(
            div()
                .id("assist-toggle")
                .debug_selector(|| "assist-toggle".into())
                .flex()
                .items_center()
                .gap_1()
                .cursor_pointer()
                .text_role(style::Role::Heading)
                .child(
                    icons::icon(if open {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .xsmall()
                    .text_color(secondary),
                )
                .child(icons::icon(IconName::Sparkles).small())
                .child("Describe in words\u{2026}")
                .on_click(cx.listener(|this, _, _, cx| this.toggle_open(cx))),
        );
        if !open {
            return panel;
        }
        if configured {
            panel = panel
                .child(Textarea::new(&self.prose))
                .child(tool_panel::note(self.notice(&key)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            style::secondary_button("assist-convert", cx)
                                .small()
                                .icon(icons::icon(IconName::Sparkles))
                                .label(if self.converting {
                                    "Converting…"
                                } else {
                                    "Convert to LaTeX"
                                })
                                .disabled(self.converting)
                                .on_click(cx.listener(|this, _, _, cx| this.convert(cx))),
                        )
                        .when(self.converting, |row| {
                            row.child(
                                style::secondary_button("assist-cancel", cx)
                                    .small()
                                    .icon(icons::icon(IconName::X))
                                    .label("Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                            )
                        })
                        .child(
                            Button::new("assist-setup-toggle")
                                .small()
                                .ghost()
                                .icon(icons::icon(IconName::Settings))
                                .label("Set up")
                                .on_click(cx.listener(|this, _, _, cx| this.toggle_setup(cx))),
                        ),
                );
        } else {
            panel = panel.child(tool_panel::note(
                "Turn a description into LaTeX with a model you choose: OpenAI, Gemini or \
                 a local server such as Ollama. Off until you set it up; nothing is sent \
                 without pressing Convert.",
            ));
        }
        if self.setup_open || !configured {
            let mut presets = div().flex().flex_wrap().gap_1();
            for (index, preset) in PRESETS.iter().enumerate() {
                presets = presets.child(
                    Button::new(("assist-preset", index))
                        .small()
                        .label(preset.name)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.apply_preset(index, window, cx);
                        })),
                );
            }
            // The provider settings are form rows, so labels and fields line up and wrap
            // inside the dialog body instead of running past its edge.
            panel = panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .rounded(px(style::radius::CARD))
                    .border_1()
                    .border_color(hex(color::BORDER))
                    .child(style::form_row("Start from", presets))
                    .child(style::form_row(
                        "Base URL",
                        Input::new(&self.base_url).small(),
                    ))
                    .child(tool_panel::note("Any OpenAI-compatible endpoint works."))
                    .child(style::form_row("Model", Input::new(&self.model).small()))
                    .child(style::form_row(
                        "API key",
                        Input::new(&self.api_key).small().mask_toggle(),
                    ))
                    .child(tool_panel::note(self.key_line(&key)))
                    .child(tool_panel::note(
                        "The key is stored in your system keyring, never in project or \
                         settings files. Leave the fields empty and save to turn AI \
                         assist off.",
                    ))
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                style::secondary_button("assist-remove-key", cx)
                                    .small()
                                    .label("Remove saved key")
                                    .disabled(key != KeyStatus::Saved)
                                    .on_click(cx.listener(|this, _, _, cx| this.remove_key(cx))),
                            )
                            .child(
                                style::secondary_button("assist-save", cx)
                                    .small()
                                    .label("Save")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.save_setup(window, cx);
                                    })),
                            ),
                    ),
            );
        }
        match &self.message {
            Some(Message::Error(text)) => {
                panel = panel.child(tool_panel::problem(text.clone()));
            }
            Some(Message::Info(text)) => {
                panel = panel.child(tool_panel::note(text.clone()));
            }
            None => {}
        }
        panel
    }
}

#[cfg(test)]
mod tests {
    use super::{KeyStatus, KeyUse, key_status_line};

    #[test]
    fn the_form_says_when_the_keyring_is_not_available() {
        let line = key_status_line(
            Some(KeyUse::Required),
            &KeyStatus::Unavailable("no Secret Service".into()),
        );
        assert!(line.starts_with("Keyring not available"), "{line}");
        assert!(line.contains("no Secret Service"), "{line}");
        // A plain http:// network server never uses a key, whatever the keyring says.
        let line = key_status_line(Some(KeyUse::Never), &KeyStatus::Saved);
        assert!(line.contains("plain http://"), "{line}");
        assert!(key_status_line(None, &KeyStatus::Missing).contains("none saved"));
    }
}
