//! Multi-selection, clipboard, grouped edits and track management.
//!
//! The view state lives on [`Editor`]; every decision that does not need a window
//! is planned by the pure [`crate::selection`], [`crate::clipboard`] and
//! [`crate::ripple`] modules, so each edit here is one `Command` (usually a
//! `Batch`) and therefore one undo step.

use gpui_component::input::{InputEvent, InputState};

use super::*;

impl Editor {
    /// The reconciled selection. The primary clip always belongs to it, and a
    /// selection set that went stale (the primary was set directly) collapses to
    /// the primary alone.
    pub(super) fn selected_ids(&self) -> BTreeSet<ClipId> {
        let Some(primary) = self.selected.filter(|id| self.find_clip(*id).is_some()) else {
            return BTreeSet::new();
        };
        let ids = selection::existing(&self.project, &self.selection);
        if ids.contains(&primary) {
            ids
        } else {
            BTreeSet::from([primary])
        }
    }

    /// Replace the selection. `primary` defaults to the first member.
    pub(super) fn set_selection(&mut self, ids: BTreeSet<ClipId>, primary: Option<ClipId>) {
        let ids = selection::existing(&self.project, &ids);
        let primary = primary
            .filter(|id| ids.contains(id))
            .or_else(|| ids.iter().next().copied());
        if self.selected != primary {
            self.selected_field = None;
            self.selected_keyframe = None;
        }
        self.selected = primary;
        self.selection = ids;
    }

    pub(super) fn select_only(&mut self, id: ClipId) {
        self.set_selection(BTreeSet::from([id]), Some(id));
    }

    pub(super) fn clear_selection(&mut self) {
        self.selected = None;
        self.selection.clear();
        self.selected_field = None;
        self.selected_keyframe = None;
    }

    /// Drop clips that no longer exist after an edit, keeping a primary clip.
    pub(super) fn prune_selection(&mut self) {
        if self.selected.is_none() {
            self.selection.clear();
            return;
        }
        let mut ids = selection::existing(&self.project, &self.selection);
        let primary = self.selected.filter(|id| self.find_clip(*id).is_some());
        match primary {
            Some(primary) if !ids.contains(&primary) => ids = BTreeSet::from([primary]),
            Some(_) => {}
            None => self.selected = ids.iter().next().copied(),
        }
        self.selection = ids;
    }

    pub(super) fn select_all(&mut self, cx: &mut Context<Self>) {
        let ids = selection::all_clips(&self.project);
        let primary = self.selected;
        self.set_selection(ids, primary);
        cx.notify();
    }

    /// Escape: close transient UI first, then drop the selection.
    pub(super) fn escape(&mut self, cx: &mut Context<Self>) {
        if self.track_rename.is_some() {
            self.finish_track_rename(false, cx);
        } else if self.context_menu.take().is_some() {
        } else if self.gesture.take().is_some() || self.keyframe_drag.take().is_some() {
            self.snap_line = None;
        } else {
            self.clear_selection();
        }
        cx.notify();
    }

    /// Whether the timeline (not the viewer or an input) currently has focus.
    pub(super) fn timeline_focused(&self, window: &Window, cx: &App) -> bool {
        self.panel_focus[0].contains_focused(window, cx)
    }

    // ---- clipboard -------------------------------------------------------

    /// Copy the selection to the in-app clipboard and the system clipboard.
    pub(super) fn copy_selection(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(board) = Clipboard::copy(&self.project, &self.selected_ids()) else {
            return false;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(board.to_json()));
        self.clipboard = Some(board);
        cx.notify();
        true
    }

    pub(super) fn cut_selection(&mut self, cx: &mut Context<Self>) {
        if self.copy_selection(cx) {
            self.delete_selected(cx);
        }
    }

    /// The system clipboard if it holds our clips, else the in-app copy.
    fn clipboard_contents(&self, cx: &App) -> Option<Clipboard> {
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .and_then(|text| Clipboard::from_json(&text))
            .or_else(|| self.clipboard.clone())
    }

    pub(super) fn paste_clips(&mut self, cx: &mut Context<Self>) {
        let Some(board) = self.clipboard_contents(cx) else {
            return;
        };
        let at = self.position;
        self.apply_placement(|project, next| board.paste(project, at, next), cx);
    }

    /// Duplicate the selection right after itself.
    pub(super) fn duplicate_selection(&mut self, cx: &mut Context<Self>) {
        let Some(board) = Clipboard::copy(&self.project, &self.selected_ids()) else {
            return;
        };
        self.apply_placement(|project, next| board.duplicate(project, next), cx);
    }

    /// Run a planner that allocates fresh ids, apply its command and select the
    /// clips it created.
    fn apply_placement(
        &mut self,
        plan: impl FnOnce(&Project, &mut dyn FnMut() -> u64) -> Option<(Command, BTreeSet<ClipId>)>,
        cx: &mut Context<Self>,
    ) {
        let mut next = self.next_id;
        let mut allocate = || {
            let id = next;
            next = next.saturating_add(1);
            id
        };
        let planned = plan(&self.project, &mut allocate);
        self.next_id = next;
        let Some((command, created)) = planned else {
            return;
        };
        if self.try_execute(&command, cx) {
            self.set_selection(created, None);
            cx.notify();
        }
    }

    // ---- grouped edits ---------------------------------------------------

    /// Delete the whole selection as one undo step.
    pub(super) fn delete_selected(&mut self, cx: &mut Context<Self>) {
        let ids = self.selected_ids();
        let Some(command) = selection::delete_command(&self.project, &ids) else {
            return;
        };
        if self.try_execute(&command, cx) {
            self.clear_selection();
        }
    }

    /// Delete the selection and close the gaps, or ripple-delete the marked range
    /// when nothing is selected.
    pub(super) fn ripple_delete_selection(&mut self, cx: &mut Context<Self>) {
        let ids = self.selected_ids();
        let command = if ids.is_empty() {
            let range = self
                .marks
                .marked()
                .then(|| {
                    self.marks
                        .range(animathio_export::duration(&self.project).0)
                })
                .flatten();
            editing::ripple_delete_command(None, range)
        } else {
            ripple::delete_command(&self.project, &ids, self.ripple_scope)
        };
        if let Some(command) = command
            && self.try_execute(&command, cx)
        {
            self.clear_selection();
        }
    }

    /// Nudge the selection by `frames` (negative = earlier). Returns whether there
    /// was a selection to nudge, so callers can fall back to moving the playhead.
    pub(super) fn nudge_selection(&mut self, frames: i64, cx: &mut Context<Self>) -> bool {
        let ids = self.selected_ids();
        if ids.is_empty() {
            return false;
        }
        let rate = self.project.frame_rate();
        if let Ok(span) = rate.frame_to_time(frames.abs(), Rounding::Nearest)
            && let Some(command) =
                selection::move_command(&self.project, &ids, span.0 * frames.signum(), 0)
        {
            self.try_execute(&command, cx);
        }
        true
    }

    pub(super) fn toggle_ripple_scope(&mut self, cx: &mut Context<Self>) {
        self.ripple_scope = self.ripple_scope.toggled();
        self.notices
            .push(crate::style::Toast::info(self.ripple_scope.label()));
        cx.notify();
    }

    /// Delete one clip, or the whole selection when it is part of one.
    pub(super) fn delete_clip(&mut self, id: ClipId, cx: &mut Context<Self>) {
        if self.selected_ids().contains(&id) {
            self.delete_selected(cx);
        } else {
            self.try_execute(&Command::RemoveClip { id }, cx);
        }
    }

    /// Duplicate one clip, or the whole selection when it is part of one.
    pub(super) fn duplicate_clip(&mut self, id: ClipId, cx: &mut Context<Self>) {
        self.adopt_clip(id);
        self.duplicate_selection(cx);
    }

    /// Make a context-menu target part of the selection (keeping an existing
    /// multi-selection that already contains it).
    fn adopt_clip(&mut self, id: ClipId) {
        if !self.selected_ids().contains(&id) {
            self.select_only(id);
        }
    }

    pub(super) fn copy_clip(&mut self, id: ClipId, cx: &mut Context<Self>) {
        self.adopt_clip(id);
        self.copy_selection(cx);
    }

    pub(super) fn cut_clip(&mut self, id: ClipId, cx: &mut Context<Self>) {
        self.adopt_clip(id);
        self.cut_selection(cx);
    }

    pub(super) fn ripple_delete_clip(&mut self, id: ClipId, cx: &mut Context<Self>) {
        self.adopt_clip(id);
        self.ripple_delete_selection(cx);
    }

    // ---- tracks ----------------------------------------------------------

    pub(super) fn start_track_rename(
        &mut self,
        id: TrackId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(name) = self
            .project
            .tracks()
            .iter()
            .find(|track| track.id() == id)
            .map(|track| track.name().to_owned())
        else {
            return;
        };
        let input = cx.new(|cx| InputState::new(window, cx).default_value(name));
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this, _state, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    this.finish_track_rename(true, cx);
                }
            },
        );
        let focus = input.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        self.context_menu = None;
        self.track_rename = Some(TrackRename {
            id,
            input,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// End an inline rename, committing the typed name as a `RenameTrack`.
    pub(super) fn finish_track_rename(&mut self, commit: bool, cx: &mut Context<Self>) {
        let Some(rename) = self.track_rename.take() else {
            return;
        };
        if commit {
            let name = rename.input.read(cx).value().trim().to_owned();
            let unchanged = self
                .project
                .tracks()
                .iter()
                .find(|track| track.id() == rename.id)
                .is_none_or(|track| track.name() == name);
            if !name.is_empty() && !unchanged {
                self.try_execute(
                    &Command::RenameTrack {
                        id: rename.id,
                        name,
                    },
                    cx,
                );
            }
        }
        cx.notify();
    }

    /// Move a track to `index` (its position after the move).
    pub(super) fn move_track_to(&mut self, id: TrackId, index: usize, cx: &mut Context<Self>) {
        if self
            .project
            .tracks()
            .iter()
            .position(|track| track.id() == id)
            .is_some_and(|from| from != index)
        {
            self.try_execute(&Command::MoveTrack { id, index }, cx);
        }
    }

    /// Move a track one step up (`-1`) or down (`1`) the stack.
    pub(super) fn move_track_by(&mut self, id: TrackId, delta: isize, cx: &mut Context<Self>) {
        let Some(from) = self
            .project
            .tracks()
            .iter()
            .position(|track| track.id() == id)
        else {
            return;
        };
        if let Some(to) = from
            .checked_add_signed(delta)
            .filter(|to| *to < self.project.tracks().len())
        {
            self.move_track_to(id, to, cx);
        }
    }
}
