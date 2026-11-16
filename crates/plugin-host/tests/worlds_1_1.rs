//! API 1.1 worlds through the real first-party components: animation presets become
//! one exactly undoable keyframe batch, and the Manim importer plugin rebuilds the
//! same project as the native importer crate.
#![allow(clippy::float_cmp)]

use std::collections::BTreeMap;

use animathio_core::{
    AssetId, Clip, ClipId, ClipKind, Command, FrameRate, ParamValue, Project, Target, Time,
    TimeRange, Track, TrackId, TransformProperty,
};
use animathio_import_manim::{ManimOptions, import_manim_with_options};
use animathio_plugin_host::{ImportRequest, PluginHost, PresetRole, test_support};

const ANIMATIONS: &str = "org.animathio.animations";
const MANIM: &str = "org.animathio.import-manim";
const SECOND: i64 = 705_600_000;

/// A minimal valid 1x1 transparent PNG.
const MINIMAL_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

fn project_with(kind: ClipKind) -> Project {
    let mut project = Project::new(FrameRate::default());
    if let ClipKind::Image(id) | ClipKind::Audio(id) = &kind {
        let media_type = if matches!(kind, ClipKind::Image(_)) {
            "image/png"
        } else {
            "audio/wav"
        };
        Command::AddAsset {
            id: id.clone(),
            asset: animathio_core::Asset {
                name: "asset".into(),
                media_type: media_type.into(),
            },
        }
        .apply(&mut project)
        .unwrap();
    }
    let commands = [
        Command::AddTrack {
            track: Track::new(TrackId(1), "V1"),
            index: 0,
        },
        Command::AddClip {
            track: TrackId(1),
            clip: Clip::new(
                ClipId(1),
                kind,
                TimeRange::new(Time(0), Time(4 * SECOND)).unwrap(),
            ),
        },
    ];
    for command in commands {
        command.apply(&mut project).unwrap();
    }
    project
}

fn text_project() -> Project {
    project_with(animathio_plugin_api::sources::text(
        "Hello".into(),
        0.1,
        [255; 4],
    ))
}

fn clip(project: &Project) -> &Clip {
    &project.tracks()[0].clips()[&ClipId(1)]
}

fn keys(project: &Project, target: &Target) -> Vec<(i64, ParamValue)> {
    clip(project)
        .animations
        .iter()
        .find(|a| &a.target == target)
        .map(|a| {
            a.keys
                .keys()
                .iter()
                .map(|(t, k)| (t.0, k.value.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn opacity() -> Target {
    Target::Transform(TransformProperty::Opacity)
}

#[test]
fn the_catalog_covers_every_role_and_v1_parity_presets() {
    let host = test_support::host();
    let presets = host.animation_presets();
    for id in [
        "fade-in",
        "fade-out",
        "slide-in",
        "slide-out",
        "zoom-in",
        "pop-in",
        "spin-in",
        "bounce-in",
        "wipe-in",
        "typewriter-in",
        "breathe",
        "shake",
    ] {
        assert!(
            presets
                .iter()
                .any(|p| p.plugin_id == ANIMATIONS && p.preset_id == id),
            "missing preset {id}"
        );
    }
    for role in [PresetRole::Entrance, PresetRole::Exit, PresetRole::Emphasis] {
        assert!(presets.iter().any(|p| p.role == role));
    }
    let typewriter = presets
        .iter()
        .find(|p| p.preset_id == "typewriter-in")
        .unwrap();
    assert!(typewriter.applies_to_kind("plugin:org.animathio.text"));
    assert!(!typewriter.applies_to_kind("image"));
}

#[test]
fn fade_in_is_one_undoable_batch_of_ordinary_keyframes() {
    let host = test_support::host();
    let mut project = text_project();
    let before = project.clone();
    let command = host
        .animation_preset_command(
            &project,
            ClipId(1),
            ANIMATIONS,
            "fade-in",
            Some(Time(SECOND)),
            &BTreeMap::from([("easing".to_owned(), ParamValue::Enum("linear".into()))]),
        )
        .unwrap();
    assert!(matches!(command, Command::Batch { .. }));
    let inverse = command.apply(&mut project).unwrap();
    assert_eq!(
        keys(&project, &opacity()),
        [
            (0, ParamValue::Number(0.0)),
            (SECOND, ParamValue::Number(1.0))
        ]
    );
    // The keys render without the plugin: evaluation samples them directly.
    let scene = animathio_core::evaluate(&project, Time(SECOND / 2));
    assert!((scene.clips[0].transform.opacity - 0.5).abs() < 1e-9);
    inverse.apply(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn out_presets_end_at_the_clip_end_and_compose_with_in_presets() {
    let host = test_support::host();
    let mut project = text_project();
    for preset in ["fade-in", "fade-out"] {
        host.animation_preset_command(
            &project,
            ClipId(1),
            ANIMATIONS,
            preset,
            None,
            &BTreeMap::new(),
        )
        .unwrap()
        .apply(&mut project)
        .unwrap();
    }
    let times: Vec<i64> = keys(&project, &opacity()).iter().map(|k| k.0).collect();
    assert_eq!(times, [0, SECOND / 2, 4 * SECOND - SECOND / 2, 4 * SECOND]);
    let scene = animathio_core::evaluate(&project, Time(2 * SECOND));
    assert_eq!(scene.clips[0].transform.opacity, 1.0);
}

#[test]
fn every_preset_generates_valid_keyframes_on_fitting_clips() {
    let host = test_support::host();
    let image = project_with(ClipKind::Image(AssetId::of(b"img")));
    for preset in host.animation_presets() {
        for base in [text_project(), image.clone()] {
            let kind = animathio_plugin_host::clip_kind_id(&clip(&base).kind);
            let mut project = base.clone();
            let result = host.animation_preset_command(
                &project,
                ClipId(1),
                &preset.plugin_id,
                &preset.preset_id,
                None,
                &BTreeMap::new(),
            );
            if !preset.applies_to_kind(&kind) {
                assert!(
                    result.is_err(),
                    "{} should not fit {kind}",
                    preset.preset_id
                );
                continue;
            }
            let command = result.unwrap_or_else(|e| panic!("{}: {e}", preset.preset_id));
            let inverse = command
                .apply(&mut project)
                .unwrap_or_else(|e| panic!("{}: {e}", preset.preset_id));
            project.validate().unwrap();
            inverse.apply(&mut project).unwrap();
            assert_eq!(project, base, "{} undo", preset.preset_id);
        }
    }
}

#[test]
fn typewriter_keyframes_the_text_reveal_param_and_renders_partial_text() {
    let host = test_support::host();
    let mut project = text_project();
    host.animation_preset_command(
        &project,
        ClipId(1),
        ANIMATIONS,
        "typewriter-in",
        Some(Time(SECOND)),
        &BTreeMap::new(),
    )
    .unwrap()
    .apply(&mut project)
    .unwrap();
    let reveal = keys(&project, &Target::Param("reveal".into()));
    assert_eq!(reveal.first(), Some(&(0, ParamValue::Number(0.0))));
    assert_eq!(reveal.last(), Some(&(SECOND, ParamValue::Number(1.0))));
    let scene = animathio_core::evaluate(&project, Time(SECOND * 2 / 5));
    let resolved = test_support::resolve(&scene, (640, 360));
    let ClipKind::Vector(vector) = &resolved.clips[0].kind else {
        panic!("text failed: {:?}", resolved.clips[0].kind)
    };
    let animathio_core::VectorElement::TextRun(text) = &vector.elements[0] else {
        panic!("expected a text run")
    };
    assert_eq!(text.text, "He");
}

#[test]
fn preset_options_and_clip_fit_are_validated() {
    let host = test_support::host();
    let project = text_project();
    let call = |preset: &str, options: BTreeMap<String, ParamValue>| {
        host.animation_preset_command(&project, ClipId(1), ANIMATIONS, preset, None, &options)
    };
    assert!(call("nope", BTreeMap::new()).is_err());
    assert!(
        call(
            "slide-in",
            BTreeMap::from([("direction".into(), ParamValue::Enum("diagonal".into()))])
        )
        .is_err()
    );
    assert!(
        host.animation_preset_command(
            &project,
            ClipId(9),
            ANIMATIONS,
            "fade-in",
            None,
            &BTreeMap::new()
        )
        .is_err()
    );
    let audio = project_with(ClipKind::Audio(AssetId::of(b"a")));
    assert!(
        host.animation_preset_command(
            &audio,
            ClipId(1),
            ANIMATIONS,
            "fade-in",
            None,
            &BTreeMap::new()
        )
        .is_err()
    );
}

fn plugin_import(
    host: &PluginHost,
    source: &str,
    dir: Option<&std::path::Path>,
    scene: Option<&str>,
) -> Result<animathio_plugin_host::ImportedProject, animathio_plugin_host::HostError> {
    let mut params = BTreeMap::new();
    if let Some(scene) = scene {
        params.insert("scene".to_owned(), ParamValue::String(scene.into()));
    }
    host.import(
        MANIM,
        "manim",
        ImportRequest {
            file_name: "scene.py".into(),
            bytes: source.as_bytes().to_vec(),
            sibling_dir: dir.map(std::path::Path::to_path_buf),
            params,
        },
    )
}

const SCENES: [&str; 3] = [
    r#"
from manim import *

class Demo(Scene):
    def construct(self):
        title = Text("Hello", font_size=48, color=BLUE)
        eq = MathTex(r"e^{i\pi} + 1 = 0")
        sq = Square(side_length=2, color=RED, fill_color=RED)
        c = Circle(radius=1).shift(LEFT * 2)
        group = VGroup(sq, c).arrange(RIGHT)
        self.play(Write(title), run_time=1.5)
        self.play(FadeIn(eq), title.animate.to_edge(UP))
        self.play(Create(sq), GrowFromCenter(c))
        self.wait(0.5)
        self.play(Transform(sq, c), rate_func=smooth)
        self.play(FadeOut(title), FadeOut(eq))
"#,
    r"
class Shapes(Scene):
    def construct(self):
        a = Arrow(start=LEFT, end=RIGHT, color=YELLOW)
        d = Dot(point=np.array([1, 1, 0]))
        p = Polygon([0, 0, 0], [1, 0, 0], [0, 1, 0], color=GREEN)
        l = Line(start=-UP, end=UP * 2)
        self.add(a, d)
        self.play(Create(p), Create(l))
        for i in range(3):
            self.wait(1)
        self.play(a.animate.rotate(PI / 4), d.animate.scale(2))
        self.play(Indicate(d))
",
    "class Broken(Scene):\n    def construct(self):\n        self.play(Write(\n",
];

#[test]
fn manim_plugin_matches_the_native_importer() {
    let host = test_support::host();
    for source in SCENES {
        let (project, assets, report) =
            import_manim_with_options(source, &ManimOptions::default()).unwrap();
        let imported = plugin_import(&host, source, None, None).unwrap();
        assert_eq!(imported.project, project);
        assert_eq!(imported.assets.len(), assets.len());
        let native: Vec<String> = report.warnings().iter().map(ToString::to_string).collect();
        assert_eq!(imported.warnings(), native);
    }
}

#[test]
fn manim_plugin_reads_images_only_through_the_granted_directory() {
    let host = test_support::host();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("photo.png"), MINIMAL_PNG).unwrap();
    let source = r#"
class S(Scene):
    def construct(self):
        img = ImageMobject("photo.png")
        gone = ImageMobject("missing.png")
        escape = ImageMobject("../photo.png")
        self.add(img, gone, escape)
        self.wait(1)
"#;
    let imported = plugin_import(&host, source, Some(dir.path()), None).unwrap();
    let id = AssetId::of(MINIMAL_PNG);
    assert_eq!(imported.assets.len(), 1);
    assert_eq!(imported.assets[0].id, id);
    assert_eq!(imported.assets[0].bytes, MINIMAL_PNG);
    let clips: Vec<&Clip> = imported.project.tracks()[0..]
        .iter()
        .flat_map(|t| t.clips().values())
        .collect();
    assert!(clips.iter().any(|c| c.kind == ClipKind::Image(id.clone())));
    assert!(
        clips.iter().any(
            |c| matches!(&c.kind, ClipKind::Error(message) if message.contains("missing.png"))
        )
    );
    assert!(
        clips
            .iter()
            .any(|c| matches!(&c.kind, ClipKind::Error(message) if message.contains("..")))
    );

    // Without a granted directory no file is read at all.
    let ungranted = plugin_import(&host, source, None, None).unwrap();
    assert!(ungranted.assets.is_empty());
}

#[test]
fn manim_plugin_selects_scenes_and_reports_failures() {
    let host = test_support::host();
    let source = "class First(Scene):\n    def construct(self):\n        self.wait(1)\n\nclass Second(Scene):\n    def construct(self):\n        self.wait(2)\n";
    let second = plugin_import(&host, source, None, Some("Second")).unwrap();
    assert_eq!(second.project.duration(), Time(2 * SECOND));
    let error = plugin_import(&host, source, None, Some("Nope")).unwrap_err();
    assert!(error.to_string().contains("Nope"), "{error}");
    let error = plugin_import(&host, "x = 1\n", None, None).unwrap_err();
    assert!(error.to_string().contains("no Scene"), "{error}");
    assert!(
        host.import(
            MANIM,
            "manim",
            ImportRequest {
                bytes: vec![0xff, 0xfe],
                ..ImportRequest::default()
            }
        )
        .is_err()
    );
    assert!(
        host.import(MANIM, "other", ImportRequest::default())
            .is_err()
    );
    let importers = host.importers();
    assert!(
        importers
            .iter()
            .any(|i| i.plugin_id == MANIM && i.extensions == ["py"])
    );
}
