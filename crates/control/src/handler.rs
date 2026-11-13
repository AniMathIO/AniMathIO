//! The [`ControlHandler`] trait: one method the server calls, plus typed
//! convenience accessors built on top of it.

use crate::{
    ControlError,
    method::{
        AssetInfo, Call, ClipAdded, GraphAddParams, ImportManimParams, ImportManimResult,
        MathAddParams, ProjectState, RenderFrameParams, RenderFrameResult, Timeline,
    },
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

    /// Export an offline MP4 and return its destination after completion.
    fn export_video(
        &mut self,
        params: crate::method::ExportParams,
    ) -> Result<String, ControlError> {
        from_value(self.handle(Call::Export { params })?)
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

    /// List bundled title compositions and fields.
    fn titles_list(&mut self) -> Result<Vec<crate::TitlePreset>, ControlError> {
        from_value(self.handle(Call::TitlesList)?)
    }

    /// Add an animated linked title composition as one undo step.
    fn titles_add(&mut self, params: crate::TitleAddParams) -> Result<ClipAdded, ControlError> {
        from_value(self.handle(Call::TitlesAdd { params })?)
    }

    /// List the filters provided by loaded effect plugins.
    fn effects_list(&mut self) -> Result<Vec<crate::EffectInfo>, ControlError> {
        from_value(self.handle(Call::EffectsList)?)
    }

    /// Add a plugin effect to a clip.
    fn effects_add(
        &mut self,
        params: crate::EffectAddParams,
    ) -> Result<crate::EffectAdded, ControlError> {
        from_value(self.handle(Call::EffectsAdd { params })?)
    }

    /// List the filters provided by loaded audio-effect plugins.
    fn audio_effects_list(&mut self) -> Result<Vec<crate::EffectInfo>, ControlError> {
        from_value(self.handle(Call::AudioEffectsList)?)
    }

    /// Add an audio effect to a clip or track.
    fn audio_effects_add(
        &mut self,
        params: crate::AudioEffectAddParams,
    ) -> Result<crate::AudioEffectAdded, ControlError> {
        from_value(self.handle(Call::AudioEffectsAdd { params })?)
    }

    /// Add a LaTeX plugin clip.
    fn math_add(&mut self, params: MathAddParams) -> Result<ClipAdded, ControlError> {
        from_value(self.handle(Call::MathAdd { params })?)
    }

    /// Add a graph plugin clip.
    fn graph_add(&mut self, params: GraphAddParams) -> Result<ClipAdded, ControlError> {
        from_value(self.handle(Call::GraphAdd { params })?)
    }

    /// Import a Manim scene, appending to or replacing the current project.
    fn import_manim(
        &mut self,
        params: ImportManimParams,
    ) -> Result<ImportManimResult, ControlError> {
        from_value(self.handle(Call::ImportManim { params })?)
    }

    /// Detect silent audio and ripple-remove it, returning the removed ranges.
    fn trim_silence(
        &mut self,
        params: crate::TrimSilenceParams,
    ) -> Result<crate::TrimSilenceResult, ControlError> {
        from_value(self.handle(Call::TrimSilence { params })?)
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
