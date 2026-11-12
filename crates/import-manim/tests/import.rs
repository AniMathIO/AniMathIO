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

/// A minimal valid 1x1 transparent PNG (67 bytes).
const MINIMAL_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

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

fn import_image_scene(
    source: &str,
    dir: &std::path::Path,
) -> (
    Project,
    Vec<animathio_import_manim::Asset>,
    animathio_import_manim::Report,
) {
    let options = ManimOptions {
        scene: None,
        script_dir: Some(dir.to_path_buf()),
    };
    import_manim_with_options(source, &options).unwrap()
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
    assert_eq!(
        formula.params["reveal"],
        animathio_core::ParamValue::Number(1.0)
    );

    // Write: reveal 0 -> 1 over 2 seconds.
    let reveal = animation(&project, "formula", &Target::Param("reveal".to_owned())).unwrap();
    let keys: Vec<_> = reveal.keys.keys().keys().collect();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0], &Time(0));
    assert_eq!(reveal.keys.keys()[&Time(0)].value.as_number().unwrap(), 0.0);
    assert_eq!(keys[1], &Time(ticks(2.0)));
    assert_eq!(
        reveal.keys.keys()[&Time(ticks(2.0))]
            .value
            .as_number()
            .unwrap(),
        1.0
    );

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
    assert_eq!(
        opacity.keys.keys()[&Time(ticks(2.0))]
            .value
            .as_number()
            .unwrap(),
        1.0
    );
    assert_eq!(keys[1], &Time(ticks(3.0)));
    assert_eq!(
        opacity.keys.keys()[&Time(ticks(3.0))]
            .value
            .as_number()
            .unwrap(),
        0.0
    );
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
    assert_close(x.keys.keys()[&Time(0)].value.as_number().unwrap(), 0.5);
    assert_eq!(keys[1], &Time(ticks(2.0)));
    assert_close(
        x.keys.keys()[&Time(ticks(2.0))].value.as_number().unwrap(),
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
    fs::write(&png, MINIMAL_PNG).unwrap();

    let source = r#"
class S(Scene):
    def construct(self):
        img = ImageMobject("photo.png")
        gone = ImageMobject("missing.png")
        self.add(img, gone)
        self.wait(1)
"#;
    let (project, assets, report) = import_image_scene(source, dir.path());

    let id = AssetId::of(MINIMAL_PNG);
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
    assert!(
        report
            .infos()
            .iter()
            .any(|i| i.message.contains("photo.png") && i.message.contains("image/png")),
        "expected an embedded-image info entry: {:?}",
        report.infos()
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

#[test]
fn rejects_absolute_image_paths() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ["/etc/passwd", "/photo.png"];
    #[cfg(windows)]
    let paths =
        paths
            .into_iter()
            .chain([r"C:\photo.png", r"\photo.png", r"\\server\share\photo.png"]);
    for path in paths {
        let source = format!(
            r#"
class S(Scene):
    def construct(self):
        img = ImageMobject(r"{path}")
        self.add(img)
        self.wait(1)
"#
        );
        let (project, assets, report) = import_image_scene(&source, dir.path());
        assert!(assets.is_empty());
        let clips = tracks(&project);
        assert_eq!(clips.len(), 1);
        assert!(matches!(&clips[0].1.kind, ClipKind::Error(message) if message.contains(path)));
        assert!(
            report
                .warnings()
                .iter()
                .any(|w| w.message.contains("absolute path")),
            "expected an absolute-path warning for {path}: {:?}",
            report.warnings()
        );
        assert!(report.infos().is_empty());
    }
}

#[test]
fn requires_a_script_directory_for_images() {
    let source = r#"
class S(Scene):
    def construct(self):
        img = ImageMobject("photo.png")
        self.add(img)
        self.wait(1)
"#;
    let (project, assets, report) =
        import_manim_with_options(source, &ManimOptions::default()).unwrap();
    assert!(assets.is_empty());
    let clips = tracks(&project);
    assert_eq!(clips.len(), 1);
    assert!(matches!(&clips[0].1.kind, ClipKind::Error(message) if message.contains("photo.png")));
    assert!(
        report
            .warnings()
            .iter()
            .any(|w| w.message.contains("no script directory")),
        "expected a no-script-directory warning: {:?}",
        report.warnings()
    );
}

#[test]
fn rejects_parent_directory_traversal() {
    let dir = tempfile::tempdir().unwrap();
    let source = r#"
class S(Scene):
    def construct(self):
        img = ImageMobject("../secret.png")
        self.add(img)
        self.wait(1)
"#;
    let (_, assets, report) = import_image_scene(source, dir.path());
    assert!(assets.is_empty());
    assert!(
        report.warnings().iter().any(|w| w.message.contains("`..`")),
        "expected a parent-directory warning: {:?}",
        report.warnings()
    );
}

#[cfg(unix)]
#[test]
fn rejects_symlinks_escaping_the_script_directory() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("secret.png");
    fs::write(&secret, MINIMAL_PNG).unwrap();
    std::os::unix::fs::symlink(&secret, dir.path().join("escape.png")).unwrap();

    let source = r#"
class S(Scene):
    def construct(self):
        img = ImageMobject("escape.png")
        self.add(img)
        self.wait(1)
"#;
    let (_, assets, report) = import_image_scene(source, dir.path());
    assert!(assets.is_empty());
    assert!(
        report
            .warnings()
            .iter()
            .any(|w| w.message.contains("outside the script directory")),
        "expected an escaping-symlink warning: {:?}",
        report.warnings()
    );
}

#[test]
fn rejects_non_image_extensions() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("notes.txt"), b"not an image").unwrap();
    let source = r#"
class S(Scene):
    def construct(self):
        img = ImageMobject("notes.txt")
        self.add(img)
        self.wait(1)
"#;
    let (_, assets, report) = import_image_scene(source, dir.path());
    assert!(assets.is_empty());
    assert!(
        report
            .warnings()
            .iter()
            .any(|w| w.message.contains("unsupported extension")),
        "expected an unsupported-extension warning: {:?}",
        report.warnings()
    );
}

#[test]
fn rejects_files_whose_magic_bytes_do_not_match_the_extension() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("fake.png"), b"this is not a png").unwrap();
    let source = r#"
class S(Scene):
    def construct(self):
        img = ImageMobject("fake.png")
        self.add(img)
        self.wait(1)
"#;
    let (_, assets, report) = import_image_scene(source, dir.path());
    assert!(assets.is_empty());
    assert!(
        report
            .warnings()
            .iter()
            .any(|w| w.message.contains("does not match")),
        "expected a magic-byte warning: {:?}",
        report.warnings()
    );
}

#[test]
fn rejects_images_exceeding_the_size_limit() {
    let dir = tempfile::tempdir().unwrap();
    let big = dir.path().join("big.png");
    fs::File::create(&big)
        .unwrap()
        .set_len(64 * 1024 * 1024 + 1)
        .unwrap();
    let source = r#"
class S(Scene):
    def construct(self):
        img = ImageMobject("big.png")
        self.add(img)
        self.wait(1)
"#;
    let (_, assets, report) = import_image_scene(source, dir.path());
    assert!(assets.is_empty());
    assert!(
        report
            .warnings()
            .iter()
            .any(|w| w.message.contains("size limit")),
        "expected a size-limit warning: {:?}",
        report.warnings()
    );
}

#[test]
fn imports_an_inline_add_expression() {
    let source = r"
class S(Scene):
    def construct(self):
        self.add(Circle(radius=1))
        self.wait(1)
";
    let (project, _, report) = import_manim_with_options(source, &ManimOptions::default()).unwrap();
    assert!(report.is_empty(), "{:?}", report.warnings());

    let clips = tracks(&project);
    assert_eq!(clips.len(), 1);
    let name = project.tracks()[0].name();
    assert!(
        name.starts_with("_anon_"),
        "expected a synthetic id, got {name:?}"
    );
    assert_eq!(clips[0].1.range.start, Time(0));
    assert_eq!(clips[0].1.range.end, Time(ticks(1.0)));
    let ClipKind::Builtin(Builtin::Shape { shape, .. }) = &clips[0].1.kind else {
        panic!("expected a builtin shape");
    };
    assert!(matches!(shape, animathio_core::builtin::Shape::Ellipse));
}

#[test]
fn imports_an_inline_write_math_tex() {
    let source = r#"
class S(Scene):
    def construct(self):
        self.play(Write(MathTex("x^2")))
"#;
    let (project, _, report) = import_manim_with_options(source, &ManimOptions::default()).unwrap();
    assert!(report.is_empty(), "{:?}", report.warnings());

    let clips = tracks(&project);
    assert_eq!(clips.len(), 1);
    let name = project.tracks()[0].name();
    assert!(name.starts_with("_anon_"), "unexpected track name {name:?}");
    let ClipKind::Plugin { plugin_id, params } = &clips[0].1.kind else {
        panic!("expected a plugin clip");
    };
    assert_eq!(plugin_id, "org.animathio.latex");
    assert_eq!(params["source"], "x^2");
    assert_eq!(params["mode"], "mathtex");

    let reveal = animation(&project, name, &Target::Param("reveal".to_owned())).unwrap();
    let keys: Vec<_> = reveal.keys.keys().keys().collect();
    assert_eq!(keys, vec![&Time(0), &Time(ticks(1.0))]);
}

#[test]
fn imports_an_inline_chained_transform() {
    let source = r"
class S(Scene):
    def construct(self):
        self.add(Circle(radius=1).shift(RIGHT))
        self.wait(1)
";
    let (project, _, report) = import_manim_with_options(source, &ManimOptions::default()).unwrap();
    assert!(report.is_empty(), "{:?}", report.warnings());

    let clips = tracks(&project);
    assert_eq!(clips.len(), 1);
    assert!(project.tracks()[0].name().starts_with("_anon_"));
    // Shift RIGHT moves the centre one scene unit to the right.
    assert_close(clips[0].1.transform.x, 0.5 + 1.0 / FRAME_WIDTH);
    assert_close(clips[0].1.transform.y, 0.5);
}

#[test]
fn imports_a_vgroup_with_arrange_and_group_fade_in() {
    let source = r"
class S(Scene):
    def construct(self):
        a = Circle(radius=1)
        b = Square(side_length=2)
        g = VGroup(a, b).arrange(DOWN)
        self.play(FadeIn(g))
        self.wait(1)
";
    let (project, _, report) = import_manim_with_options(source, &ManimOptions::default()).unwrap();
    assert!(report.is_empty(), "{:?}", report.warnings());

    // The group itself is not a track; its two members are.
    let clips = tracks(&project);
    assert_eq!(clips.len(), 2);
    assert_eq!(project.tracks()[0].name(), "a");
    assert_eq!(project.tracks()[1].name(), "b");

    // `arrange(DOWN)` lays `b` out one `b` height below `a` plus a buffer.
    assert_close(clips[0].1.transform.y, 0.5);
    let expected_b_y = 0.5 + (2.0 / 2.0 + 2.0 / 2.0 + 0.25) / FRAME_HEIGHT;
    assert_close(clips[1].1.transform.y, expected_b_y);

    // Animating the group fades both members in.
    for name in ["a", "b"] {
        let opacity = animation(
            &project,
            name,
            &Target::Transform(TransformProperty::Opacity),
        )
        .unwrap();
        let keys: Vec<_> = opacity.keys.keys().keys().collect();
        assert_eq!(
            keys,
            vec![&Time(0), &Time(ticks(1.0))],
            "opacity keys for {name}"
        );
    }
}

#[test]
fn warns_on_unsupported_inline_mobjects() {
    let source = r"
class S(Scene):
    def construct(self):
        self.add(CustomMobject())
        self.wait(1)
";
    let (project, _, report) = import_manim_with_options(source, &ManimOptions::default()).unwrap();
    project.validate().unwrap();
    assert!(
        report
            .warnings()
            .iter()
            .any(|w| w.message.contains("unsupported mobject expression")
                && w.message.contains("CustomMobject")),
        "expected an unsupported-inline-mobject warning: {:?}",
        report.warnings()
    );
    assert!(tracks(&project).is_empty());
}
