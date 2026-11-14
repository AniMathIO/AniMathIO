//! Portable template round-trip, validation and undo coverage.
use animathio_control::{MediaFill, ProjectTemplate, project_templates};
use animathio_core::ClipId;
use std::collections::BTreeMap;

#[test]
fn bundled_templates_round_trip_and_fill_undo() {
    for template in project_templates().unwrap() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("test.amiotemplate");
        template.save(&path).unwrap();
        let mut loaded = ProjectTemplate::load(&path).unwrap();
        assert_eq!(loaded.project, template.project);
        assert!(!loaded.manifest.placeholders.is_empty());
        let values = loaded
            .manifest
            .placeholders
            .iter()
            .map(|p| (p.clip, "Replacement".into()))
            .collect();
        let instantiated = loaded.instantiate(&values).unwrap();
        assert_ne!(instantiated, loaded.project);
        let command = loaded.fill_text(&values).unwrap();
        let before = loaded.project.clone();
        let undo = command.apply(&mut loaded.project).unwrap();
        assert_ne!(loaded.project, before);
        for (old, new) in before.tracks().iter().zip(loaded.project.tracks()) {
            for (id, clip) in old.clips() {
                let replacement = &new.clips()[id];
                assert_eq!(clip.range, replacement.range);
                assert_eq!(clip.transform, replacement.transform);
                assert_eq!(clip.effects, replacement.effects);
                assert_eq!(clip.animations, replacement.animations);
            }
        }
        undo.apply(&mut loaded.project).unwrap();
        assert_eq!(loaded.project, before);
    }
}

#[test]
fn rejects_unknown_and_duplicate_clip_references() {
    let mut template = project_templates().unwrap().remove(0);
    assert!(
        template
            .fill_text(&BTreeMap::from([(ClipId(u64::MAX), "bad".into())]))
            .is_err()
    );
    template
        .manifest
        .placeholders
        .push(template.manifest.placeholders[0].clone());
    assert!(template.manifest.validate(&template.project).is_err());
    template.manifest.placeholders.pop();
    template.manifest.placeholders[0].clip = ClipId(u64::MAX);
    assert!(template.manifest.validate(&template.project).is_err());
}

#[test]
fn rejects_thumbnail_traversal_and_existing_directory() {
    let mut template = project_templates().unwrap().remove(0);
    template.manifest.thumbnail = Some("../outside.png".into());
    assert!(template.manifest.validate(&template.project).is_err());
    template.manifest.thumbnail = None;
    let temp = tempfile::tempdir().unwrap();
    assert!(template.save(temp.path()).is_err());
    assert!(temp.path().exists());
}

#[test]
fn media_replacement_retains_edits_and_undoes_exactly() {
    let template = project_templates().unwrap().remove(0);
    let id = template.manifest.placeholders[0].clip;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("wide.png");
    image::RgbaImage::from_pixel(8, 2, image::Rgba([255, 0, 0, 255]))
        .save(&path)
        .unwrap();
    for cover in [false, true] {
        let (asset, bytes, command) = template.fill_media(id, &path, cover).unwrap();
        assert_eq!(asset, animathio_core::AssetId::of(&bytes));
        let mut project = template.project.clone();
        let undo = command.apply(&mut project).unwrap();
        let before = template
            .project
            .tracks()
            .iter()
            .find_map(|t| t.clips().get(&id))
            .unwrap();
        let after = project
            .tracks()
            .iter()
            .find_map(|t| t.clips().get(&id))
            .unwrap();
        assert_eq!(after.range, before.range);
        assert_eq!(after.transform, before.transform);
        assert_eq!(after.effects, before.effects);
        assert_eq!(after.animations, before.animations);
        assert!(matches!(after.kind, animathio_core::ClipKind::Image(_)));
        assert_eq!(
            after.params["media_fit"],
            animathio_core::ParamValue::String(if cover { "cover" } else { "contain" }.into())
        );
        undo.apply(&mut project).unwrap();
        assert_eq!(project, template.project);
    }
}

fn png(dir: &std::path::Path, name: &str, w: u32, h: u32) -> std::path::PathBuf {
    let path = dir.join(name);
    image::RgbaImage::from_pixel(w, h, image::Rgba([0, 255, 0, 255]))
        .save(&path)
        .unwrap();
    path
}

#[test]
fn combined_fill_is_one_undoable_batch_and_keeps_clip_properties() {
    let template = project_templates()
        .unwrap()
        .into_iter()
        .find(|t| t.manifest.placeholders.len() >= 2)
        .expect("a bundled template with two placeholders");
    let (media_id, text_id) = (
        template.manifest.placeholders[0].clip,
        template.manifest.placeholders[1].clip,
    );
    assert!(template.accepts_text(text_id));
    let temp = tempfile::tempdir().unwrap();
    let path = png(temp.path(), "photo.png", 8, 2);
    let fill = template
        .fill(
            &BTreeMap::from([(text_id, "Hello".to_string())]),
            &BTreeMap::from([(media_id, MediaFill { path, cover: true })]),
        )
        .unwrap();
    assert_eq!(fill.assets.len(), 1);
    let mut project = template.project.clone();
    let undo = fill.command.apply(&mut project).unwrap();
    let find = |p: &animathio_core::Project, id| {
        p.tracks()
            .iter()
            .find_map(|t| t.clips().get(&id))
            .unwrap()
            .clone()
    };
    let (old, new) = (find(&template.project, media_id), find(&project, media_id));
    assert!(matches!(new.kind, animathio_core::ClipKind::Image(_)));
    assert_eq!((new.range, &new.transform), (old.range, &old.transform));
    assert_eq!(new.effects, old.effects);
    assert_eq!(new.animations, old.animations);
    assert_eq!(new.source_offset, old.source_offset);
    assert_ne!(find(&project, text_id), find(&template.project, text_id));
    // The whole fill, assets included, reverses in a single step.
    undo.apply(&mut project).unwrap();
    assert_eq!(project, template.project);
}

#[test]
fn media_replacement_keeps_linked_companions() {
    let template = project_templates().unwrap().remove(0);
    let count =
        |p: &animathio_core::Project| p.tracks().iter().map(|t| t.clips().len()).sum::<usize>();
    let id = template.manifest.placeholders[0].clip;
    let temp = tempfile::tempdir().unwrap();
    let path = png(temp.path(), "a.png", 4, 4);
    let (_, _, command) = template.fill_media(id, &path, true).unwrap();
    let mut project = template.project.clone();
    command.apply(&mut project).unwrap();
    assert_eq!(count(&project), count(&template.project));
}

#[test]
fn fill_rejects_conflicts_audio_unknown_ids_and_missing_files() {
    let template = project_templates().unwrap().remove(0);
    let id = template.manifest.placeholders[0].clip;
    let temp = tempfile::tempdir().unwrap();
    let image = png(temp.path(), "a.png", 4, 4);
    let fill = |cover| MediaFill {
        path: image.clone(),
        cover,
    };
    // The same placeholder as text and media.
    assert!(
        template
            .fill(
                &BTreeMap::from([(id, "x".into())]),
                &BTreeMap::from([(id, fill(true))])
            )
            .is_err()
    );
    // Unknown placeholder id.
    assert!(
        template
            .fill(
                &BTreeMap::new(),
                &BTreeMap::from([(ClipId(9_999), fill(true))])
            )
            .is_err()
    );
    // Missing file.
    let missing = MediaFill {
        path: temp.path().join("nope.png"),
        cover: false,
    };
    assert!(
        template
            .fill(&BTreeMap::new(), &BTreeMap::from([(id, missing)]))
            .is_err()
    );
    // Audio cannot fill a visual placeholder.
    let wav = temp.path().join("tone.wav");
    std::fs::write(&wav, b"RIFF\x24\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x40\x1f\0\0\x80\x3e\0\0\x02\0\x10\0data\0\0\0\0").unwrap();
    assert!(
        template
            .fill(
                &BTreeMap::new(),
                &BTreeMap::from([(
                    id,
                    MediaFill {
                        path: wav,
                        cover: true
                    }
                )])
            )
            .is_err()
    );
}

#[test]
fn media_fit_param_reflects_the_chosen_mode() {
    let template = project_templates().unwrap().remove(0);
    let id = template.manifest.placeholders[0].clip;
    let temp = tempfile::tempdir().unwrap();
    let path = png(temp.path(), "a.png", 4, 4);
    for (cover, mode) in [(true, "cover"), (false, "contain")] {
        let (_, _, command) = template.fill_media(id, &path, cover).unwrap();
        let mut project = template.project.clone();
        command.apply(&mut project).unwrap();
        let clip = project
            .tracks()
            .iter()
            .find_map(|t| t.clips().get(&id))
            .unwrap();
        assert_eq!(
            clip.params["media_fit"],
            animathio_core::ParamValue::String(mode.into())
        );
    }
}
