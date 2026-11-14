//! Live colour previews are commands; one completed drag becomes one history entry.

use super::*;

pub(super) struct ColorEdit {
    inverse: Command,
    latest: Command,
}

impl ColorEdit {
    fn preview(
        project: &mut Project,
        command: Command,
        edit: &mut Option<Self>,
    ) -> Result<(), animathio_core::Error> {
        let inverse = command.apply(project)?;
        if let Some(edit) = edit {
            edit.latest = command;
        } else {
            *edit = Some(Self {
                inverse,
                latest: command,
            });
        }
        Ok(())
    }

    fn commit(
        self,
        project: &mut Project,
        history: &mut History,
    ) -> Result<(), animathio_core::Error> {
        self.inverse.apply(project)?;
        history.execute(
            project,
            &Command::Batch {
                commands: vec![self.latest],
            },
        )
    }
}

impl Editor {
    pub(super) fn preview_color(&mut self, command: Command, cx: &mut Context<Self>) {
        if !self.pointer_down {
            self.execute(&command, cx);
            return;
        }
        match ColorEdit::preview(&mut self.project, command, &mut self.color_edit) {
            Ok(()) => {
                self.sync_audio(cx);
                self.request_render(self.current_render_time());
                cx.notify();
            }
            Err(error) => self.error = Some(error.0),
        }
    }

    pub(super) fn finish_color_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(edit) = self.color_edit.take() {
            match edit.commit(&mut self.project, &mut self.history) {
                Ok(()) => self.after_edit(cx),
                Err(error) => self.error = Some(error.0),
            }
        }
    }

    /// Register capture handlers at window scope: the picker's popover lives
    /// outside the editor's element subtree and can consume bubble events.
    pub(super) fn capture_color_gesture(weak: WeakEntity<Self>, window: &mut Window) {
        let move_view = weak.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture {
                let _ = move_view.update(cx, |this, cx| {
                    this.move_gizmo(event, cx);
                    this.move_number_scrub(event, cx);
                });
            }
        });
        let down_view = weak.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                let _ = down_view.update(cx, |this, _| this.pointer_down = true);
            }
        });
        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                let _ = weak.update(cx, |this, cx| {
                    this.end_gizmo();
                    this.fields.scrub = None;
                    this.pointer_down = false;
                    this.finish_color_edit(cx);
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn settings(project: &Project, background: [u8; 4]) -> Command {
        Command::SetSettings {
            width: project.width(),
            height: project.height(),
            frame_rate: project.frame_rate(),
            background,
            duration: project.duration(),
        }
    }
    #[core::prelude::v1::test]
    fn track_audio_drag_updates_snapshot_and_commits_one_undo() {
        let mut project = Project::default();
        let chain = AudioChain::Track(TrackId(1));
        Command::AddTrack {
            track: Track::new(TrackId(1), "Audio"),
            index: 0,
        }
        .apply(&mut project)
        .unwrap();
        Command::AddAudioEffect {
            chain,
            index: 0,
            effect: animathio_core::EffectInstance::new(
                EffectId(2),
                "test",
                "gain",
                std::collections::BTreeMap::from([("gain".into(), 1.0)]),
            ),
        }
        .apply(&mut project)
        .unwrap();
        let before = project.clone();
        let mut history = History::default();
        let mut edit = None;
        for value in [0.8, 0.6, 0.4] {
            ColorEdit::preview(
                &mut project,
                Command::SetAudioEffectParam {
                    chain,
                    effect: EffectId(2),
                    name: "gain".into(),
                    value: Some(value),
                },
                &mut edit,
            )
            .unwrap();
            assert_eq!(project.tracks()[0].audio_effects()[0].params["gain"], value);
        }
        edit.unwrap().commit(&mut project, &mut history).unwrap();
        assert!(history.undo(&mut project).unwrap());
        assert_eq!(project, before);
        assert!(!history.undo(&mut project).unwrap());
    }

    #[core::prelude::v1::test]
    fn drag_is_one_undo_step_and_preserves_previous_history() {
        let mut project = Project::default();
        let mut history = History::default();
        let first = settings(&project, [1, 2, 3, 255]);
        history.execute(&mut project, &first).unwrap();
        let mut edit = None;
        for red in 10..50 {
            let next = settings(&project, [red, 2, 3, 255]);
            ColorEdit::preview(&mut project, next, &mut edit).unwrap();
        }
        assert_eq!(project.background(), [49, 2, 3, 255]);
        edit.unwrap().commit(&mut project, &mut history).unwrap();
        assert!(history.undo(&mut project).unwrap());
        assert_eq!(project.background(), [1, 2, 3, 255]);
        assert!(history.redo(&mut project).unwrap());
        assert_eq!(project.background(), [49, 2, 3, 255]);
        history.undo(&mut project).unwrap();
        history.undo(&mut project).unwrap();
        assert_eq!(project, Project::default());
    }
}
