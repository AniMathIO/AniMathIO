//! Caption restyling, colour and preset editing.

use animathio_control::{caption_styles, clip_caption_style};

use super::fixtures::{caption_command, sample};
use super::{ClipId, TestAppContext, Track, Ui};
use crate::editor::actions::{Redo, Undo};
use crate::editor::caption_styles::LookChange;

#[gpui::test]
fn caption_restyle_is_one_undo_step(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let command = caption_command(&project);
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|editor, _, cx| {
        assert!(editor.try_execute(&command, cx));
    });

    // Caption text clips, by id, with the style each carries.
    let styles = |ui: &Ui| -> Vec<(ClipId, String)> {
        ui.read(|e| {
            e.project
                .tracks()
                .iter()
                .flat_map(|t| t.clips().values())
                .filter_map(|c| clip_caption_style(c).map(|s| (c.id, s)))
                .collect()
        })
    };
    let before = styles(&ui);
    assert_eq!(before.len(), 2, "two caption cues");
    let original = before[0].1.clone();
    let track = ui.read(|e| {
        e.project
            .tracks()
            .iter()
            .find(|t| t.name() == "Subtitles")
            .map(Track::id)
            .expect("subtitle track")
    });
    // Selecting a caption makes the remembered look follow its style.
    ui.update(|editor, _, cx| editor.select_only(before[0].0, cx));
    assert_eq!(ui.read(|e| e.caption_look.style.clone()), original);

    let target = caption_styles()
        .iter()
        .map(|s| s.id)
        .find(|id| *id != original)
        .expect("another style")
        .to_owned();
    ui.update(|editor, _, cx| {
        editor.apply_look_change(LookChange::Style(target.clone()), Some(track), cx);
    });
    assert!(styles(&ui).iter().all(|(_, style)| *style == target));
    assert_eq!(ui.read(|e| e.caption_look.style.clone()), target);

    ui.dispatch(Undo);
    assert_eq!(styles(&ui), before, "one undo restores every caption");
    ui.dispatch(Redo);
    assert!(styles(&ui).iter().all(|(_, style)| *style == target));
}

#[gpui::test]
fn caption_custom_colour_and_preset_undo(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let command = caption_command(&project);
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|editor, _, cx| {
        assert!(editor.try_execute(&command, cx));
    });
    let before = ui.read(|e| e.project.clone());
    let track = before
        .tracks()
        .iter()
        .find(|t| t.name() == "Subtitles")
        .unwrap()
        .id();
    let caption = before
        .tracks()
        .iter()
        .flat_map(|t| t.clips().values())
        .find(|c| clip_caption_style(c).is_some())
        .unwrap()
        .id;
    ui.update(|editor, _, cx| editor.select_only(caption, cx));
    let custom = [17, 83, 149, 255];
    ui.update(|editor, _, cx| {
        editor.apply_look_change(LookChange::Text(custom), Some(track), cx);
    });
    assert_eq!(ui.read(|e| e.caption_look.color), Some(custom));
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
    let mut look = ui.read(|e| e.caption_look.clone());
    look.style = "karaoke".into();
    look.accent = Some(custom);
    ui.update(|editor, _, cx| {
        editor.apply_look_change(LookChange::Preset(look.clone()), Some(track), cx);
    });
    assert_eq!(ui.read(|e| e.caption_look.clone()), look);
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}
