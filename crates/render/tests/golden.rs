//! Committed project fixtures and PNG reference frames (maximum channel error: 2).
mod common;
use animathio_core::{Rounding, evaluate};
use animathio_format::{Bundle, Limits, read_amio, read_json};
use animathio_render::RenderCtx;
use common::render;
use std::{fs::File, path::Path};

fn golden(project: &str, frame: i64, name: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let input = root.join("fixtures").join(project);
    let bundle = if project.ends_with("amio") {
        read_amio(File::open(input).unwrap(), Limits::default()).unwrap()
    } else {
        let project = read_json(File::open(&input).unwrap()).unwrap();
        let assets = project
            .assets()
            .keys()
            .map(|id| {
                (
                    id.clone(),
                    std::fs::read(input.parent().unwrap().join("assets").join(id.as_str()))
                        .unwrap(),
                )
            })
            .collect();
        Bundle { project, assets }
    };
    let mut ctx = RenderCtx::default();
    for (id, bytes) in bundle.assets {
        ctx.resources.insert(id, bytes).unwrap();
    }
    let time = bundle
        .project
        .frame_rate()
        .frame_to_time(frame, Rounding::Nearest)
        .unwrap();
    let scene = evaluate(&bundle.project, time);
    // This fixture explicitly tests the renderer's unresolved-plugin placeholder.
    let actual = if project == "media.json" {
        animathio_render::render(&scene, &mut ctx, (320, 180))
    } else {
        render(&scene, &mut ctx, (320, 180))
    }
    .unwrap();
    let path = root.join("golden").join(format!("{name}.png"));
    if std::env::var("UPDATE_GOLDEN").as_deref() == Ok("1") {
        actual.save_png(&path).unwrap();
    }
    let expected = image::open(&path)
        .unwrap_or_else(|e| panic!("{}: {e}; regenerate with UPDATE_GOLDEN=1", path.display()))
        .to_rgba8();
    assert_eq!(expected.dimensions(), actual.size);
    let differences = expected
        .as_raw()
        .iter()
        .zip(&actual.pixels)
        .filter(|(a, b)| a.abs_diff(**b) > 2)
        .count();
    if differences != 0 {
        let output = std::env::temp_dir().join(format!("animathio-{name}-actual.png"));
        actual.save_png(&output).unwrap();
        panic!(
            "{differences} channels exceeded tolerance 2; actual frame: {}",
            output.display()
        );
    }
}
#[test]
fn sample_start() {
    golden("sample.amio", 0, "sample-start");
}
#[test]
fn sample_eased_midpoint() {
    golden("sample.amio", 30, "sample-midpoint");
}
#[test]
fn sample_last_keyframe() {
    golden("sample.amio", 60, "sample-last-keyframe");
}
#[test]
fn sample_half_open_end() {
    golden("sample.amio", 90, "sample-end");
}
#[test]
fn video_and_missing_plugin_placeholders() {
    golden("media.json", 15, "media-placeholders");
}
