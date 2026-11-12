//! Authoring calls share validation, commands and undo with ordinary edits.

use animathio_control::{Call, ControlHandler, HeadlessHandler};
use animathio_core::{ClipKind, TICKS_PER_SECOND, Time, evaluate};
use serde_json::{Value, json};

fn invoke(
    handler: &mut HeadlessHandler,
    method: &str,
    params: &Value,
) -> Result<Value, animathio_control::ControlError> {
    handler.handle(
        serde_json::from_value(json!({"method":method,"params":{"params":params}})).unwrap(),
    )
}

#[test]
fn plugin_parameters_timing_and_clip_local_animations_match_plugins() {
    let mut handler = HeadlessHandler::new();
    let second = TICKS_PER_SECOND;
    invoke(&mut handler, "math.add", &json!({"latex":"x^2","size":36,"color":[1,2,3,255],"x":0.25,"y":0.75,"start":second,"duration":2*second,"reveal_duration":second})).unwrap();
    let clip = handler.project().tracks()[0]
        .clips()
        .values()
        .next()
        .unwrap();
    assert_eq!(clip.range.start, Time(second));
    assert_eq!(clip.range.end, Time(3 * second));
    assert!((clip.transform.x - 0.25).abs() < f64::EPSILON);
    assert!((clip.transform.y - 0.75).abs() < f64::EPSILON);
    let ClipKind::Plugin { plugin_id, params } = &clip.kind else {
        panic!("expected plugin");
    };
    assert_eq!(plugin_id, "org.animathio.latex");
    assert_eq!(params["source"], "x^2");
    assert_eq!(params["size"], 36.0);
    assert_eq!(params["color"], json!([1, 2, 3, 255]));
    let scene = evaluate(handler.project(), Time(second + second / 2));
    assert!((scene.clips[0].params["reveal"].as_number().unwrap() - 0.5).abs() < f64::EPSILON);
    invoke(&mut handler, "graph.add", &json!({"items":"plot y=x^2","x_min":-2,"x_max":2,"duration":2*second,"draw_duration":second})).unwrap();
    let scene = evaluate(handler.project(), Time(second / 2));
    assert!((scene.clips[0].params["draw"].as_number().unwrap() - 0.5).abs() < f64::EPSILON);
    // Each generated keyframe and clip is an ordinary undoable command.
    for _ in 0..4 {
        assert!(handler.history_undo().unwrap());
    }
    assert_eq!(handler.project().tracks().len(), 1);
    for _ in 0..4 {
        assert!(handler.history_redo().unwrap());
    }
    assert_eq!(handler.project().tracks().len(), 2);
}

#[test]
fn invalid_authoring_calls_preserve_project_and_history() {
    let mut handler = HeadlessHandler::new();
    for (method, params) in [
        ("math.add", json!({"latex":"\\frac{"})),
        ("math.add", json!({"latex":"x","mode":"unknown"})),
        ("math.add", json!({"latex":"x","size":-1})),
        ("math.add", json!({"latex":"x","duration":0})),
        ("math.add", json!({"latex":"x","reveal_duration":-1})),
        ("graph.add", json!({"x_min":2,"x_max":-2})),
        ("graph.add", json!({"draw_duration":-1})),
        ("graph.add", json!({"start":i64::MAX,"duration":1})),
        ("import.manim", json!({})),
        ("import.manim", json!({"path":"missing.py","source":"x"})),
    ] {
        let before = handler.project().clone();
        assert!(invoke(&mut handler, method, &params).is_err(), "{method}");
        assert_eq!(handler.project(), &before);
        assert!(!handler.history_undo().unwrap());
    }
}

#[test]
fn manim_paths_and_script_directories_follow_filesystem_rules() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("scene.py");
    std::fs::write(&script, "from manim import *\nclass Demo(Scene):\n    def construct(self):\n        c = Circle()\n        self.add(c)\n        self.wait(1)\n").unwrap();
    let mut handler = HeadlessHandler::new();
    let report = invoke(&mut handler, "import.manim", &json!({"path":script})).unwrap();
    assert_eq!(report["tracks"], 1);
    let before = handler.project().clone();
    for params in [
        json!({"path":dir.path()}),
        json!({"path":script,"script_dir":script}),
        json!({"path":dir.path().join("missing.py")}),
    ] {
        assert!(invoke(&mut handler, "import.manim", &params).is_err());
        assert_eq!(handler.project(), &before);
    }
    // Appending does not collide with existing identities.
    invoke(&mut handler, "import.manim", &json!({"path":script})).unwrap();
    handler.project().validate().unwrap();
    assert_eq!(handler.project().tracks().len(), 2);
    handler.handle(Call::HistoryUndo).unwrap();
}

#[test]
fn replacing_with_manim_resets_session_and_embeds_relative_assets() {
    let dir = tempfile::tempdir().unwrap();
    // The importer probes magic bytes; the bundle preserves the original bytes.
    let png = b"\x89PNG\r\n\x1a\nembedded image";
    std::fs::write(dir.path().join("image.png"), png).unwrap();
    let source = "from manim import *\nclass Demo(Scene):\n    def construct(self):\n        img = ImageMobject(\"image.png\")\n        self.add(img)\n        self.wait(1)\n";
    let mut handler = HeadlessHandler::new();
    invoke(&mut handler, "graph.add", &json!({})).unwrap();
    handler
        .project_save(Some(dir.path().join("old.amio")))
        .unwrap();
    handler.playback_seek(Time(TICKS_PER_SECOND)).unwrap();
    handler.playback_play().unwrap();
    let report = invoke(
        &mut handler,
        "import.manim",
        &json!({"source":source,"script_dir":dir.path(),"replace":true}),
    )
    .unwrap();
    assert_eq!(report["assets"], 1);
    assert_eq!(report["warnings"].as_array().unwrap().len(), 0);
    assert_eq!(report["infos"].as_array().unwrap().len(), 1);
    let state = handler.project_get().unwrap();
    assert!(state.path.is_none());
    assert_eq!(state.playhead, Time(0));
    assert!(!state.playing);
    assert!(!handler.history_undo().unwrap());
    assert_eq!(
        handler.assets()[&animathio_core::AssetId::of(png)].as_slice(),
        png
    );
    let archive = dir.path().join("imported.amio");
    handler.project_save(Some(archive.clone())).unwrap();
    let reopened = HeadlessHandler::load(&archive).unwrap();
    assert_eq!(reopened.project(), handler.project());
    assert_eq!(reopened.assets(), handler.assets());
}
