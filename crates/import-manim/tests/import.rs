//! End-to-end tests for the Manim importer: ported v1 parser cases, value
//! mapping, animation timings, positioning, warnings, malformed input and
//! image asset resolution.
#![allow(
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::too_many_lines,
    clippy::assert_is_empty
)]

use animathio_core::{
    AssetId, Clip, ClipKind, FrameRate, Project, TICKS_PER_SECOND, Target, Time, TrackId,
    TransformProperty, builtin::Builtin,
};
use animathio_import_manim::{ManimOptions, import_manim, import_manim_with_options};
use std::collections::BTreeMap;
use std::fs;

const FRAME_WIDTH: f64 = 14.22;
const FRAME_HEIGHT: f64 = 8.0;

fn ticks(seconds: f64) -> i64 {
    (seconds * TICKS_PER_SECOND as f64).round() as i64
}

fn tracks(project: &Project) -> Vec<(TrackId, Clip)> {
    let mut out = Vec::new();
    for track in project.tracks() {
        for clip in track.clips().values() {
            out.push((track.id(), clip.clone()));
        }
    }
    out
}

fn animation(project: &Project, name: &str, target: &Target) -> Option<animathio_core::Animation> {
    let clips = tracks(project);
    for (track, clip) in &clips {
        let track_name = project
            .tracks()
            .iter()
            .find(|t| t.id() == *track)
            .map(|t| t.name().to_owned())
            .unwrap_or_default();
        if track_name != name {
            continue;
        }
        return clip
            .animations
            .iter()
            .find(|a| &a.target == target)
            .cloned();
    }
    None
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn imports_a_write_then_fade_out_math_tex_scene() {
    let source = r#"
from manim import *

class LatexScene(Scene):
    def construct(self):
        formula = MathTex(r"E = mc^2", color=YELLOW)
        self.play(Write(formula), run_time=2)
        self.play(FadeOut(formula), run_time=1)
        self.wait(1)
"#;
    let (project, assets, report) =
        import_manim_with_options(source, &ManimOptions::default()).unwrap();
    assert!(report.is_empty(), "{:?}", report.warnings());
    assert!(assets.is_empty());

    // Default Manim settings: 1920x1080, 30 fps, black background.
    assert_eq!(project.frame_rate(), FrameRate::new(30, 1).unwrap());
    assert_eq!(project.width(), 1920);
    assert_eq!(project.height(), 1080);
    assert_eq!(project.background(), [0, 0, 0, 255]);
    assert_eq!(project.duration(), Time(ticks(4.0)));

    let clips = tracks(&project);
    assert_eq!(clips.len(), 1);
    let formula = &clips[0].1;
    assert_eq!(formula.range.start, Time(0));
    assert_eq!(formula.range.end, Time(ticks(3.0)));

    let ClipKind::Plugin { plugin_id, params } = &formula.kind else {
        panic!("expected a plugin clip");
    };
    assert_eq!(plugin_id, "org.animathio.latex");
    assert_eq!(params["source"], "E = mc^2");
    assert_eq!(params["mode"], "mathtex");
    assert_eq!(params["size"], 48.0);
    assert_eq!(params["color"], "#FFFF00");
    assert_eq!(formula.params["reveal"], 1.0);

    // Write: reveal 0 -> 1 over 2 seconds.
    let reveal = animation(&project, "formula", &Target::Param("reveal".to_owned())).unwrap();
    let keys: Vec<_> = reveal.keys.keys().keys().collect();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0], &Time(0));
    assert_eq!(reveal.keys.keys()[&Time(0)].value, 0.0);
    assert_eq!(keys[1], &Time(ticks(2.0)));
    assert_eq!(reveal.keys.keys()[&Time(ticks(2.0))].value, 1.0);

    // FadeOut: opacity 1 -> 0 over the third second.
    let opacity = animation(
        &project,
        "formula",
        &Target::Transform(TransformProperty::Opacity),
    )
    .unwrap();
    let keys: Vec<_> = opacity.keys.keys().keys().collect();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0], &Time(ticks(2.0)));
    assert_eq!(opacity.keys.keys()[&Time(ticks(2.0))].value, 1.0);
    assert_eq!(keys[1], &Time(ticks(3.0)));
    assert_eq!(opacity.keys.keys()[&Time(ticks(3.0))].value, 0.0);
}

#[test]
fn imports_an_animate_shift_scene_with_position_keyframes() {
    let source = r"
from manim import *

class ShiftScene(Scene):
    def construct(self):
        sq = Square(side_length=2)
        self.add(sq)
        self.play(sq.animate.shift(RIGHT * 2), run_time=2)
        self.wait(1)
";
    let (project, _, report) = import_manim_with_options(source, &ManimOptions::default()).unwrap();
    assert!(report.is_empty(), "{:?}", report.warnings());
    assert_eq!(project.duration(), Time(ticks(3.0)));

    let clips = tracks(&project);
    assert_eq!(clips.len(), 1);
    let sq = &clips[0].1;
    assert_eq!(sq.range.start, Time(0));
    assert_eq!(sq.range.end, Time(ticks(3.0)));

    // Base position after the shift, mapped to normalized coordinates.
    assert_close(sq.transform.x, 0.5 + 2.0 / FRAME_WIDTH);
    assert_close(sq.transform.y, 0.5);
    assert_close(sq.transform.scale_x, 2.0 / FRAME_WIDTH);
    assert_close(sq.transform.scale_y, 2.0 / FRAME_HEIGHT);

    let x = animation(&project, "sq", &Target::Transform(TransformProperty::X)).unwrap();
    let keys: Vec<_> = x.keys.keys().keys().collect();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0], &Time(0));
    assert_close(x.keys.keys()[&Time(0)].value, 0.5);
    assert_eq!(keys[1], &Time(ticks(2.0)));
    assert_close(
        x.keys.keys()[&Time(ticks(2.0))].value,
        0.5 + 2.0 / FRAME_WIDTH,
    );

    // Y is unchanged, so no Y keyframes are emitted.
    assert!(animation(&project, "sq", &Target::Transform(TransformProperty::Y)).is_none());
}

#[test]
fn imports_a_multi_mobject_scene_with_next_to() {
    let source = r"
from manim import *

class MultiScene(Scene):
    def construct(self):
        a = Circle(radius=1, color=BLUE)
        b = Square(side_length=2, color=RED)
        b.next_to(a, RIGHT)
        self.add(a, b)
        self.wait(2)
";
    let (project, _, report) = import_manim_with_options(source, &ManimOptions::default()).unwrap();
    assert!(report.is_empty(), "{:?}", report.warnings());

    let clips = tracks(&project);
    assert_eq!(clips.len(), 2);

    let a = &clips[0].1;
    assert_close(a.transform.x, 0.5);
    assert_close(a.transform.y, 0.5);
    assert_close(a.transform.scale_x, 2.0 / FRAME_WIDTH);
    assert_close(a.transform.scale_y, 2.0 / FRAME_HEIGHT);

    let b = &clips[1].1;
    // next_to(a, RIGHT): centre = a.centre + (half widths + buff) to the right.
    let expected_x = f64::midpoint(2.0, 2.0) + 0.25;
    assert_close(b.transform.x, 0.5 + expected_x / FRAME_WIDTH);
    assert_close(b.transform.y, 0.5);
    assert_eq!(b.range.start, Time(0));
    assert_eq!(b.range.end, Time(ticks(2.0)));
}

#[test]
fn reports_unsupported_constructs_as_warnings() {
    let source = r#"
from manim import *

class UnsupportedScene(Scene):
    def construct(self):
        x = Text("hi")
        self.play(ApplyMethod(x.shift, UP))
        for i in range(3):
            self.wait(1)
"#;
    let (project, _, report) = import_manim_with_options(source, &ManimOptions::default()).unwrap();
    project.validate().unwrap();

    let messages: Vec<&str> = report
        .warnings()
        .iter()
        .map(|w| w.message.as_str())
        .collect();
    assert!(
        messages.iter().any(|m| m.contains("ApplyMethod")),
        "expected an ApplyMethod warning: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("for_statement")),
        "expected a for_statement warning: {messages:?}"
    );
    // The declared-but-never-added mobject is also reported.
    assert!(
        messages.iter().any(|m| m.contains("never added")),
        "expected a never-added warning: {messages:?}"
    );
}

#[test]
fn malformed_python_does_not_panic() {
    let source = "class BrokenScene(Scene):\n    def construct(self):\n        self.play(Write(\n";
    let result = import_manim_with_options(source, &ManimOptions::default());
    let (project, _, report) = result.expect("malformed Python should import leniently");
    project.validate().unwrap();
    assert!(
        report
            .warnings()
            .iter()
            .any(|w| w.message.contains("parse error")),
        "expected a parse-error warning: {:?}",
        report.warnings()
    );
}

#[test]
fn round_trips_through_the_amio_container() {
    let source = r#"
class S(Scene):
    def construct(self):
        t = Text("hello", color=WHITE)
        self.play(Write(t))
        self.wait(1)
"#;
    let (project, assets) = import_manim(source).unwrap();
    let asset_bytes: BTreeMap<AssetId, Vec<u8>> =
        assets.into_iter().map(|a| (a.id, a.bytes)).collect();
    let mut cursor =
        animathio_format::write_amio(std::io::Cursor::new(Vec::new()), &project, &asset_bytes)
            .unwrap();
    cursor.set_position(0);
    let bundle = animathio_format::read_amio(cursor, animathio_format::Limits::default()).unwrap();
    assert_eq!(bundle.project, project);
    assert!(bundle.assets.is_empty());
}

#[test]
fn resolves_image_mobjects_and_places_placeholders_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("photo.png");
    fs::write(&png, b"tiny-png-bytes").unwrap();

    let source = r#"
class S(Scene):
    def construct(self):
        img = ImageMobject("photo.png")
        gone = ImageMobject("missing.png")
        self.add(img, gone)
        self.wait(1)
"#;
    let options = ManimOptions {
        scene: None,
        script_dir: Some(dir.path().to_path_buf()),
    };
    let (project, assets, report) = import_manim_with_options(source, &options).unwrap();

    let id = AssetId::of(b"tiny-png-bytes");
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0].id, id);
    assert_eq!(assets[0].media_type, "image/png");
    assert_eq!(assets[0].name, "photo.png");
    assert_eq!(
        project.assets().get(&id).map(|a| a.media_type.as_str()),
        Some("image/png")
    );

    let clips = tracks(&project);
    assert_eq!(clips.len(), 2);
    assert_eq!(clips[0].1.kind, ClipKind::Image(id.clone()));
    assert!(
        matches!(&clips[1].1.kind, ClipKind::Error(message) if message.contains("missing.png"))
    );

    assert!(
        report
            .warnings()
            .iter()
            .any(|w| w.message.contains("missing.png")),
        "expected a missing-image warning: {:?}",
        report.warnings()
    );
}

#[test]
fn selects_the_requested_scene_class() {
    let source = r"
class First(Scene):
    def construct(self):
        self.wait(1)

class Second(Scene):
    def construct(self):
        self.wait(2)
";
    let options = ManimOptions {
        scene: Some("Second".to_owned()),
        script_dir: None,
    };
    let (project, _, report) = import_manim_with_options(source, &options).unwrap();
    assert!(report.is_empty(), "{:?}", report.warnings());
    assert_eq!(project.duration(), Time(ticks(2.0)));

    let missing = ManimOptions {
        scene: Some("Nope".to_owned()),
        script_dir: None,
    };
    let error = import_manim_with_options(source, &missing).unwrap_err();
    assert!(matches!(
        error,
        animathio_import_manim::ImportError::SceneNotFound(name) if name == "Nope"
    ));
}

#[test]
fn parses_text_and_shape_builtin_kinds() {
    let source = r#"
class S(Scene):
    def construct(self):
        t = Text("hi", color=RED)
        c = Circle(radius=2)
        self.add(t, c)
        self.wait(1)
"#;
    let (project, _, report) = import_manim_with_options(source, &ManimOptions::default()).unwrap();
    assert!(report.is_empty(), "{:?}", report.warnings());

    let clips = tracks(&project);
    assert_eq!(clips.len(), 2);
    let ClipKind::Builtin(Builtin::Text { text, color, .. }) = &clips[0].1.kind else {
        panic!("expected builtin text");
    };
    assert_eq!(text, "hi");
    assert_eq!(*color, [252, 98, 85, 255]); // Manim RED

    let ClipKind::Builtin(Builtin::Shape { shape, .. }) = &clips[1].1.kind else {
        panic!("expected builtin shape");
    };
    assert!(matches!(shape, animathio_core::builtin::Shape::Ellipse));
}
