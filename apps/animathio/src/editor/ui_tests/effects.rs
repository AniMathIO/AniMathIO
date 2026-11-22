//! Video effects and animation presets.

use animathio_core::{EffectId, EffectInstance};

use super::fixtures::sample;
use super::{BTreeMap, Command, TestAppContext, Ui, require_plugins};
use crate::editor::actions::Undo;

#[gpui::test]
fn video_effect_card_param_edit_and_undo(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    let id = ui.read(|e| e.project.tracks()[0].clips().values().next().unwrap().id);
    ui.update(|e, _, cx| {
        e.execute(
            &Command::AddEffect {
                id,
                index: 0,
                effect: EffectInstance {
                    id: EffectId(900),
                    plugin: "org.animathio.effects-basic".into(),
                    effect: "blur".into(),
                    params: BTreeMap::new(),
                    bypass: false,
                },
            },
            cx,
        );
    });
    let before = ui.read(|e| e.project.clone());
    ui.update(|e, window, cx| {
        assert!(!e.video_effect_rows(id, window, cx).is_empty());
        let command = e
            .video_effect_param_command(id, EffectId(900), "radius", 5.0)
            .unwrap();
        e.execute(&command, cx);
    });
    assert_ne!(ui.read(|e| e.project.clone()), before);
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

#[gpui::test]
fn animation_preset_apply_and_undo(cx: &mut TestAppContext) {
    if !require_plugins("animation_preset_apply_and_undo") {
        return;
    }
    let (project, assets) = sample();
    let before = project.clone();
    let mut ui = Ui::open(cx, project, assets);
    let id = ui.read(|e| e.project.tracks()[0].clips().values().next().unwrap().id);
    ui.update(|e, _, cx| {
        e.select_only(id, cx);
        e.apply_animation_preset("org.animathio.animations", "fade-in", cx);
    });
    assert_ne!(ui.read(|e| e.project.clone()), before);
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

#[gpui::test]
fn clipped_slide_options_choose_the_edge_and_apply_as_one_undoable_batch(cx: &mut TestAppContext) {
    if !require_plugins("clipped_slide_options") {
        return;
    }
    let (project, assets) = sample();
    let before = project.clone();
    let mut ui = Ui::open(cx, project, assets);
    let id = ui.read(|e| e.project.tracks()[0].clips().values().next().unwrap().id);
    ui.update(|e, _, cx| {
        e.select_only(id, cx);
        // The inspector and the Effects tab both render the shared option buttons.
        assert!(!e.animation_rows(id, cx).is_empty());
        assert!(!e.animation_library_rows(cx).is_empty());
        let presets = e.available_animation_presets();
        let fitting: Vec<_> = presets.iter().collect();
        let controls = crate::editor::animations::option_controls(&fitting);
        assert_eq!(controls[0].id(), "direction");
        // default -> left -> right -> top
        let direction = controls[0].clone();
        assert!(e.option_label(&direction).ends_with("Default"));
        e.cycle_preset_option(&direction, cx);
        assert!(e.option_label(&direction).ends_with("Left"));
        e.cycle_preset_option(&direction, cx);
        e.cycle_preset_option(&direction, cx);
        assert!(e.option_label(&direction).ends_with("Top"));
        e.apply_animation_preset("org.animathio.animations", "clipped-slide-in", cx);
    });
    let direction_keys = ui.read(|e| {
        let clip = e.find_clip(id).unwrap();
        let effect = clip
            .effects
            .iter()
            .find(|f| f.effect == "slide")
            .expect("slide effect added")
            .id;
        clip.animations
            .iter()
            .find(|a| {
                a.target
                    == animathio_core::Target::Effect {
                        effect,
                        param: "direction".into(),
                    }
            })
            .unwrap()
            .keys
            .keys()
            .values()
            .map(|k| k.value.as_number().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(direction_keys, [3.0, 3.0], "direction top");
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}
