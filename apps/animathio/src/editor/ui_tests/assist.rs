//! AI assist conversion, cancellation and settings in the Math dialog.

use animathio_control::{MathObjectDraft, MathObjectKind, math_object_range};
use gpui::{AppContext, Entity};

use super::fixtures::sample;
use super::{TestAppContext, Ui, require_plugins};
use crate::editor::actions::Undo;
use crate::editor::assist::AssistPanel;
use crate::editor::math::MathEditor;
use crate::editor::support::project_size;

/// A scripted AI assist client: records what it was asked and returns a canned reply.
#[derive(Debug)]
struct MockAssist {
    reply: std::sync::Mutex<Result<String, animathio_assist::AssistError>>,
    seen: std::sync::Mutex<Vec<(String, String)>>,
    /// Whether each request carried an API key.
    keys: std::sync::Mutex<Vec<bool>>,
    /// Block in `complete` until the request is cancelled (then record that it was).
    block: bool,
    cancelled: std::sync::atomic::AtomicBool,
}

impl MockAssist {
    fn new(reply: Result<String, animathio_assist::AssistError>) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            reply: std::sync::Mutex::new(reply),
            seen: std::sync::Mutex::default(),
            keys: std::sync::Mutex::default(),
            block: false,
            cancelled: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// A client that waits for its token to be cancelled, like a request stuck on the network.
    fn blocking() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            reply: std::sync::Mutex::new(Err(animathio_assist::AssistError::Cancelled)),
            seen: std::sync::Mutex::default(),
            keys: std::sync::Mutex::default(),
            block: true,
            cancelled: std::sync::atomic::AtomicBool::new(false),
        })
    }

    fn seen(&self) -> Vec<(String, String)> {
        self.seen.lock().unwrap().clone()
    }
}

impl animathio_assist::Converter for MockAssist {
    fn complete(
        &self,
        endpoint: &animathio_assist::Endpoint,
        prose: &str,
        cancel: &animathio_assist::CancelToken,
    ) -> Result<String, animathio_assist::AssistError> {
        self.seen
            .lock()
            .unwrap()
            .push((endpoint.host().to_owned(), prose.to_owned()));
        self.keys.lock().unwrap().push(endpoint.has_key());
        if self.block {
            let started = std::time::Instant::now();
            while !cancel.is_cancelled() && started.elapsed() < std::time::Duration::from_secs(20) {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            self.cancelled
                .store(cancel.is_cancelled(), std::sync::atomic::Ordering::SeqCst);
        }
        self.reply.lock().unwrap().clone()
    }
}

fn local_assist_settings() -> animathio_assist::AssistSettings {
    animathio_assist::AssistSettings {
        base_url: "http://localhost:11434/v1".into(),
        model: "llama3.2".into(),
    }
}

/// An editor whose AI assist client is `mock`, a Latex math panel on it, and its assist panel.
fn assist_ui(
    cx: &mut TestAppContext,
    mock: &std::sync::Arc<MockAssist>,
) -> (Ui, Entity<MathEditor>, Entity<AssistPanel>) {
    let (project, assets) = sample();
    let mut env = super::test_env();
    env.assist = mock.clone();
    let mut ui = Ui::open_with_env(cx, project, assets, env);
    let panel = ui.update(|e, window, cx| {
        let draft = MathObjectDraft::new(MathObjectKind::Latex);
        let range = math_object_range(e.position).expect("range");
        let editor = cx.entity().downgrade();
        let session = e.project_session;
        let size = project_size(&e.project);
        cx.new(|cx| MathEditor::new(draft, editor, session, range, None, false, size, window, cx))
    });
    let assist = ui
        .cx
        .update(|_, cx| panel.read(cx).assist_panel().expect("latex has assist"));
    ui.cx.update(|_, cx| {
        assist.update(cx, |assist, _| assist.set_settings(local_assist_settings()));
    });
    (ui, panel, assist)
}

/// The conversion runs on its own thread, so wait (bounded) for it to report back.
fn finish_convert(ui: &mut Ui, assist: &Entity<AssistPanel>) {
    let started = std::time::Instant::now();
    loop {
        ui.advance_clock(std::time::Duration::from_millis(30));
        ui.settle();
        if !ui.cx.update(|_, cx| assist.read(cx).is_converting()) {
            return;
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "the conversion never finished"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

fn type_prose(ui: &mut Ui, assist: &Entity<AssistPanel>, text: &str) {
    ui.cx.update(|window, cx| {
        assist.update(cx, |assist, cx| assist.set_prose(text, window, cx));
    });
    ui.settle();
}

#[gpui::test]
fn assist_convert_preview_insert_undo(cx: &mut TestAppContext) {
    if !require_plugins("assist_convert_preview_insert_undo") {
        return;
    }
    let mock = MockAssist::new(Ok("```latex\n\\frac{1}{2} + x^2\n```".into()));
    let (mut ui, panel, assist) = assist_ui(cx, &mock);
    let before = ui.read(|e| e.project.clone());
    let initial = ui.cx.update(|_, cx| panel.read(cx).draft_source());

    // Typing the description sends nothing.
    type_prose(&mut ui, &assist, "one half plus x squared");
    ui.advance_clock(std::time::Duration::from_millis(400));
    assert!(mock.seen().is_empty(), "nothing is sent before Convert");

    // Convert sends exactly the prose to the configured host and shows editable LaTeX.
    ui.cx
        .update(|_, cx| assist.update(cx, AssistPanel::convert));
    finish_convert(&mut ui, &assist);
    assert_eq!(
        mock.seen(),
        vec![(
            "localhost:11434".to_owned(),
            "one half plus x squared".to_owned()
        )]
    );
    assert_ne!(initial, "\\frac{1}{2} + x^2");
    assert_eq!(
        ui.cx.update(|_, cx| panel.read(cx).draft_source()),
        "\\frac{1}{2} + x^2"
    );
    assert!(!ui.cx.update(|_, cx| assist.read(cx).is_converting()));

    // The live preview validates it through the latex plugin; then Add inserts one object.
    ui.advance_clock(std::time::Duration::from_millis(400));
    ui.settle();
    ui.cx
        .update(|window, cx| panel.update(cx, |panel, cx| panel.apply(window, cx)));
    ui.settle();
    let added = ui.read(|e| e.selected.expect("inserted clip selected"));
    ui.read(|e| {
        let clip = e.find_clip(added).expect("clip");
        assert!(matches!(
            &clip.kind,
            animathio_core::ClipKind::Plugin { plugin_id, .. } if plugin_id == "org.animathio.latex"
        ));
        let draft = animathio_control::math_object_draft(clip, project_size(&e.project))
            .expect("math object draft");
        assert_eq!(draft.fields["source"], "\\frac{1}{2} + x^2");
    });
    // One undo removes it.
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

#[gpui::test]
fn assist_cancel_ignores_the_result(cx: &mut TestAppContext) {
    let mock = MockAssist::new(Ok("x^2".into()));
    let (mut ui, panel, assist) = assist_ui(cx, &mock);
    let initial = ui.cx.update(|_, cx| panel.read(cx).draft_source());
    type_prose(&mut ui, &assist, "x squared");
    ui.cx.update(|_, cx| {
        assist.update(cx, |assist, cx| {
            assist.convert(cx);
            assert!(assist.is_converting());
            assist.cancel(cx);
            assert!(!assist.is_converting());
        });
    });
    ui.settle();
    std::thread::sleep(std::time::Duration::from_millis(50));
    ui.advance_clock(std::time::Duration::from_millis(30));
    ui.settle();
    assert_eq!(ui.cx.update(|_, cx| panel.read(cx).draft_source()), initial);
    assert!(!ui.cx.update(|_, cx| assist.read(cx).is_converting()));
}

/// Cancelling after the keyring read keeps its answer, so the next Convert (or the setup form)
/// does not read the keyring a second time.
#[gpui::test]
fn assist_cancel_keeps_the_key_status_it_already_read(cx: &mut TestAppContext) {
    use crate::editor::assist::{KeyStatus, cached_key_status_for_tests};
    let mock = MockAssist::blocking();
    let (mut ui, _panel, assist) = assist_ui(cx, &mock);
    type_prose(&mut ui, &assist, "x squared");
    ui.cx.update(|_, cx| {
        assist.update(cx, |assist, _| {
            assist.set_settings(assist_settings("https://api.openai.com/v1"));
        });
    });
    let status = |ui: &mut Ui| {
        ui.cx
            .update(|_, cx| cached_key_status_for_tests(cx, "https://api.openai.com:443"))
    };
    assert_eq!(status(&mut ui), None);
    ui.cx.update(|_, cx| {
        assist.update(cx, |assist, cx| {
            assist.convert(cx);
            assist.cancel(cx);
        });
    });
    ui.settle();
    std::thread::sleep(std::time::Duration::from_millis(50));
    ui.advance_clock(std::time::Duration::from_millis(30));
    ui.settle();
    assert_eq!(status(&mut ui), Some(KeyStatus::Missing));
}

#[gpui::test]
fn assist_errors_are_shown_and_leave_the_formula_alone(cx: &mut TestAppContext) {
    use crate::editor::assist::Message;
    let mock = MockAssist::new(Err(animathio_assist::AssistError::Auth {
        status: 401,
        detail: "bad key".into(),
    }));
    let (mut ui, panel, assist) = assist_ui(cx, &mock);
    let initial = ui.cx.update(|_, cx| panel.read(cx).draft_source());

    // Empty description: no request at all.
    ui.cx
        .update(|_, cx| assist.update(cx, AssistPanel::convert));
    finish_convert(&mut ui, &assist);
    assert_eq!(mock.seen(), Vec::<(String, String)>::new());
    let message = ui.cx.update(|_, cx| assist.read(cx).message().cloned());
    assert!(matches!(message, Some(Message::Error(_))), "{message:?}");

    // An authentication failure is reported with its status.
    type_prose(&mut ui, &assist, "x squared");
    ui.cx
        .update(|_, cx| assist.update(cx, AssistPanel::convert));
    finish_convert(&mut ui, &assist);
    let Some(Message::Error(text)) = ui.cx.update(|_, cx| assist.read(cx).message().cloned())
    else {
        panic!("expected an error message");
    };
    assert!(text.contains("HTTP 401"), "{text}");
    assert_eq!(ui.cx.update(|_, cx| panel.read(cx).draft_source()), initial);

    // A reply that is not LaTeX is rejected before it reaches the formula field.
    *mock.reply.lock().unwrap() = Ok("Sorry, I cannot help with that request today.".into());
    ui.cx
        .update(|_, cx| assist.update(cx, AssistPanel::convert));
    finish_convert(&mut ui, &assist);
    let Some(Message::Error(text)) = ui.cx.update(|_, cx| assist.read(cx).message().cloned())
    else {
        panic!("expected an error message");
    };
    assert!(text.contains("did not return LaTeX"), "{text}");
    assert_eq!(ui.cx.update(|_, cx| panel.read(cx).draft_source()), initial);

    // Unconfigured: off, nothing is sent.
    let calls = mock.seen().len();
    ui.cx.update(|_, cx| {
        assist.update(cx, |assist, cx| {
            assist.set_settings(animathio_assist::AssistSettings::default());
            assist.convert(cx);
        });
    });
    finish_convert(&mut ui, &assist);
    assert_eq!(mock.seen().len(), calls);
}

#[gpui::test]
fn assist_setup_saves_settings_without_the_key(cx: &mut TestAppContext) {
    let mock = MockAssist::new(Ok("x".into()));
    let (mut ui, _panel, assist) = assist_ui(cx, &mock);
    ui.cx.update(|window, cx| {
        assist.update(cx, |assist, cx| {
            assist.set_form(
                (
                    "https://api.openai.com/v1",
                    "gpt-4.1-mini",
                    "sk-secret-value",
                ),
                window,
                cx,
            );
            assist.save_setup(window, cx);
        });
    });
    ui.settle();
    let path = crate::editor_env::config_file("assist.json").expect("config path");
    let text = std::fs::read_to_string(&path).expect("settings file");
    assert!(text.contains("api.openai.com"), "{text}");
    assert!(!text.contains("sk-secret-value"), "{text}");
    ui.cx.update(|_, cx| {
        let assist = assist.read(cx);
        assert_eq!(assist.settings().model, "gpt-4.1-mini");
        // The key left the text field as soon as it was saved.
        assert_eq!(assist.api_key_field(cx), "");
    });
    // An invalid base URL is refused and not saved.
    ui.cx.update(|window, cx| {
        assist.update(cx, |assist, cx| {
            assist.set_form(("http://api.openai.com/v1", "m", ""), window, cx);
            assist.save_setup(window, cx);
        });
    });
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("https://api.openai.com/v1"),
        "the invalid URL must not replace the saved one"
    );
    // Leave the shared test config directory as found.
    let _ = std::fs::remove_file(path);
}

fn assist_settings(url: &str) -> animathio_assist::AssistSettings {
    animathio_assist::AssistSettings {
        base_url: url.into(),
        model: "m".into(),
    }
}

#[gpui::test]
fn assist_panel_does_not_touch_the_keyring_until_asked(cx: &mut TestAppContext) {
    use crate::editor::assist::{KeyStatus, cached_key_status_for_tests};
    let mock = MockAssist::new(Ok("x".into()));
    let (mut ui, _panel, assist) = assist_ui(cx, &mock);
    ui.cx.update(|_, cx| {
        assist.update(cx, |assist, _| {
            assist.set_settings(assist_settings("https://api.openai.com/v1"));
        });
    });
    ui.settle();
    // Building (and rebuilding) the panel never reads the keyring: no status is known.
    let origin = "https://api.openai.com:443";
    ui.cx.update(|window, cx| {
        for _ in 0..3 {
            let panel = cx.new(|cx| AssistPanel::new(window, cx));
            assert_eq!(panel.read(cx).key_status(cx), KeyStatus::Unknown);
        }
        assert_eq!(cached_key_status_for_tests(cx, origin), None);
    });
    ui.settle();
    ui.cx
        .update(|_, cx| assert_eq!(cached_key_status_for_tests(cx, origin), None));
    // Opening the setup form is the user action that looks the key up (the test platform
    // keyring is empty).
    ui.cx.update(|_, cx| {
        assist.update(cx, |assist, cx| {
            if assist.is_setup_open() {
                assist.toggle_setup(cx);
            }
            assert!(!assist.is_setup_open());
            assist.toggle_setup(cx);
            assert!(assist.is_setup_open());
        });
    });
    ui.settle();
    ui.cx.update(|_, cx| {
        assert_eq!(assist.read(cx).key_status(cx), KeyStatus::Missing);
    });
}

#[gpui::test]
fn assist_convert_reads_the_keyring_only_when_a_key_can_matter(cx: &mut TestAppContext) {
    use crate::editor::assist::{KeyStatus, set_key_status};
    let mock = MockAssist::new(Ok("x".into()));
    let (mut ui, _panel, assist) = assist_ui(cx, &mock);
    let status = |ui: &mut Ui, origin: &str| {
        ui.cx
            .update(|_, cx| crate::editor::assist::cached_key_status_for_tests(cx, origin))
    };
    type_prose(&mut ui, &assist, "x");
    let run = |ui: &mut Ui, url: &str| {
        ui.cx.update(|_, cx| {
            assist.update(cx, |assist, _| assist.set_settings(assist_settings(url)));
        });
        ui.cx
            .update(|_, cx| assist.update(cx, AssistPanel::convert));
        finish_convert(ui, &assist);
    };
    // An online service: the key is looked up on Convert.
    run(&mut ui, "https://api.openai.com/v1");
    assert_eq!(
        status(&mut ui, "https://api.openai.com:443"),
        Some(KeyStatus::Missing)
    );
    // A server on this machine: not read, as nothing says a key is saved for it.
    run(&mut ui, "http://localhost:11434/v1");
    assert_eq!(status(&mut ui, "http://localhost:11434"), None);
    // ... unless the setup form already found one.
    ui.cx
        .update(|_, cx| set_key_status(cx, "http://localhost:11434", KeyStatus::Saved));
    run(&mut ui, "http://localhost:11434/v1");
    assert_eq!(
        status(&mut ui, "http://localhost:11434"),
        Some(KeyStatus::Missing)
    );
    // A network server over plain http: never read, never sent.
    run(&mut ui, "http://192.168.1.20:11434/v1");
    assert_eq!(status(&mut ui, "http://192.168.1.20:11434"), None);
    assert_eq!(mock.keys.lock().unwrap().len(), 4);
}

#[gpui::test]
fn assist_refuses_a_key_for_plain_http_on_the_network(cx: &mut TestAppContext) {
    use crate::editor::assist::Message;
    let mock = MockAssist::new(Ok("x".into()));
    let (mut ui, _panel, assist) = assist_ui(cx, &mock);
    ui.cx.update(|window, cx| {
        assist.update(cx, |assist, cx| {
            assist.set_form(
                ("http://192.168.1.20:11434/v1", "m", "sk-secret-value"),
                window,
                cx,
            );
            assist.save_setup(window, cx);
        });
    });
    ui.settle();
    ui.cx.update(|_, cx| {
        let assist = assist.read(cx);
        let Some(Message::Error(text)) = assist.message() else {
            panic!("expected a refusal, got {:?}", assist.message());
        };
        assert!(text.contains("plain http://"), "{text}");
        assert!(!text.contains("sk-secret-value"), "{text}");
        // The settings were not changed.
        assert_eq!(assist.settings().base_url, "http://localhost:11434/v1");
    });
}

#[gpui::test]
fn dropping_the_assist_panel_cancels_the_request(cx: &mut TestAppContext) {
    let mock = MockAssist::blocking();
    let (mut ui, panel, assist) = assist_ui(cx, &mock);
    type_prose(&mut ui, &assist, "x squared");
    ui.cx
        .update(|_, cx| assist.update(cx, AssistPanel::convert));
    ui.settle();
    // Wait until the worker has the request.
    let started = std::time::Instant::now();
    while mock.seen().is_empty() {
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    // Closing the dialog drops the panel; its request is cancelled.
    drop(assist);
    ui.cx.update(|_, _| drop(panel));
    ui.settle();
    let started = std::time::Instant::now();
    while !mock.cancelled.load(std::sync::atomic::Ordering::SeqCst) {
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "the request was not cancelled"
        );
        ui.advance_clock(std::time::Duration::from_millis(30));
        ui.settle();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}
