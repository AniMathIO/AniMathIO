//! Portable template round-trip, validation and undo coverage.
use animathio_control::{ProjectTemplate, project_templates};
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
