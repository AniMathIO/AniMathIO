//! The [`ControlHandler`] trait: one method the server calls, plus typed
//! convenience accessors built on top of it.

use crate::{
    ControlError,
    method::{AssetInfo, Call, ProjectState, RenderFrameParams, RenderFrameResult, Timeline},
};
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Handles one parsed control call, returning its JSON result.
///
/// The editor implements this by routing the call onto the UI thread and
/// returning the serialized result; [`crate::HeadlessHandler`] implements it
/// directly against an in-memory project.
pub trait ControlHandler {
    /// Execute one call and return its result as JSON.
    fn handle(&mut self, call: Call) -> Result<Value, ControlError>;

    /// Read the project and session state.
    fn project_get(&mut self) -> Result<ProjectState, ControlError> {
        from_value(self.handle(Call::ProjectGet)?)
    }

    /// Load a project file.
    fn project_open(&mut self, path: std::path::PathBuf) -> Result<(), ControlError> {
        let () = from_value(self.handle(Call::ProjectOpen { path })?)?;
        Ok(())
    }

    /// Save the project, returning the path it was written to.
    fn project_save(&mut self, path: Option<std::path::PathBuf>) -> Result<String, ControlError> {
        from_value(self.handle(Call::ProjectSave { path })?)
    }

    /// Apply a command through the shared history.
    fn command_apply(&mut self, command: animathio_core::Command) -> Result<(), ControlError> {
        let () = from_value(self.handle(Call::CommandApply { command })?)?;
        Ok(())
    }

    /// Undo the last command, returning whether an edit was undone.
    fn history_undo(&mut self) -> Result<bool, ControlError> {
        from_value(self.handle(Call::HistoryUndo)?)
    }

    /// Redo the last undone command, returning whether an edit was redone.
    fn history_redo(&mut self) -> Result<bool, ControlError> {
        from_value(self.handle(Call::HistoryRedo)?)
    }

    /// Render one frame.
    fn render_frame(
        &mut self,
        params: RenderFrameParams,
    ) -> Result<RenderFrameResult, ControlError> {
        from_value(self.handle(Call::RenderFrame { params })?)
    }

    /// Move the playhead.
    fn playback_seek(&mut self, time: animathio_core::Time) -> Result<(), ControlError> {
        let () = from_value(self.handle(Call::PlaybackSeek { time })?)?;
        Ok(())
    }

    /// Start playback.
    fn playback_play(&mut self) -> Result<(), ControlError> {
        let () = from_value(self.handle(Call::PlaybackPlay)?)?;
        Ok(())
    }

    /// Pause playback.
    fn playback_pause(&mut self) -> Result<(), ControlError> {
        let () = from_value(self.handle(Call::PlaybackPause)?)?;
        Ok(())
    }

    /// Import asset bytes from a filesystem path.
    fn assets_import(&mut self, path: std::path::PathBuf) -> Result<AssetInfo, ControlError> {
        from_value(self.handle(Call::AssetsImport { path })?)
    }

    /// Read the timeline summary.
    fn timeline_list(&mut self) -> Result<Timeline, ControlError> {
        from_value(self.handle(Call::TimelineList)?)
    }

    /// Import a v1 `.animathio` project, replacing the current one.
    fn project_import_v1(&mut self, path: std::path::PathBuf) -> Result<(), ControlError> {
        let () = from_value(self.handle(Call::ProjectImportV1 { path })?)?;
        Ok(())
    }
}

/// Deserialize a JSON result value into a typed value.
pub fn from_value<T: DeserializeOwned>(value: Value) -> Result<T, ControlError> {
    serde_json::from_value(value).map_err(ControlError::from)
}

/// Dispatch one call to a handler. Provided for symmetry; the server calls
/// [`ControlHandler::handle`] directly.
pub fn dispatch<H: ControlHandler + ?Sized>(
    handler: &mut H,
    call: Call,
) -> Result<Value, ControlError> {
    handler.handle(call)
}
