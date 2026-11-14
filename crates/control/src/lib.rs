//! Local JSON-RPC 2.0 control protocol over a user-only socket or named pipe.
//!
//! The control layer is the single API for the editor UI, the CLI, plugins and
//! the MCP sidecar. Mutations are [`animathio_core::Command`]s applied through
//! the same [`animathio_core::History`] as UI edits; reads resolve pure
//! evaluation. The transport is a Unix-domain socket in `$XDG_RUNTIME_DIR` on
//! Unix and a named pipe on Windows, never a listening TCP port.
//!
//! Authentication is a random token written to a `0600` file next to the
//! socket; clients must send it with every request and it is compared in
//! constant time.

mod authoring;
mod beats;
pub use beats::{
    BeatDetectParams, BeatDetection, BeatInfo, BeatMarkerOptions, BeatMarkersParams,
    BeatMarkersResult, BeatSelection, CutShift, MarkerAddParams, MarkerAdded, SyncCutsParams,
    SyncCutsResult, SyncMode, SyncSource, add_marker_command, analyze_beats, apply_beat_markers,
    apply_sync_cuts, markers_at_beats_command, select_beats, sync_cuts_command,
};
mod autocut;
pub use autocut::{
    Autocut, AutocutParams, AutocutProposal, AutocutReason, SilenceMetric, apply_autocuts,
    autocut_command, detect_filler_words, detect_silent_samples, propose_autocuts,
    propose_autocuts_cancellable,
};
mod reframe;
pub use reframe::{ReframeDraft, ReframeParams, analyse_reframe, auto_reframe, reframe_command};
mod client;
mod edit;
mod effects;
mod error;
mod export_presets;
mod handler;
mod headless;
mod media_import;
mod method;
pub use media_import::prepare_media_import;
mod captions;
mod path;
mod server;
mod subtitles;
mod title_presets;
mod token;
pub use title_presets::{
    TitleAddParams, TitleCurve, TitleCurveSpace, TitleLayer, TitlePreset, title_preset_command,
    title_presets,
};

pub use captions::{
    CaptionPosition, CaptionStyleInfo, CaptionStyleParams, SubtitleStyleParams,
    SubtitleStyleResult, apply_caption_style, caption_style_command, caption_styles,
    clip_caption_style, parse_caption_color,
};
pub use subtitles::{
    ProgressFn, SUBTITLE_TRACK, SubtitleCueInfo, SubtitleDraft, SubtitleExportParams,
    SubtitleExportResult, SubtitleGenerateParams, SubtitleImportParams, SubtitleProgress,
    SubtitleResult, SubtitleSplit, SubtitlesAdded, apply_cues, export_subtitles,
    generate_subtitles, import_subtitles, subtitle_command, track_cues, transcribe_project,
};

pub use authoring::apply_authoring_call;
pub use client::ControlClient;
pub use edit::{
    TitleStyle, TrimSilenceParams, TrimSilenceResult, content_end, describe_timeline,
    detect_silence, timecode, title_command, trim_silence,
};
pub use effects::{
    AudioEffectAddParams, AudioEffectAdded, EffectAddParams, EffectAdded, EffectInfo,
    EffectParamInfo, add_audio_effect_command, add_effect_command, effect_infos,
};
pub use error::ControlError;
pub use export_presets::ExportPreset;
pub use handler::{ControlHandler, dispatch, from_value};
pub use headless::HeadlessHandler;
pub use method::{
    AssetInfo, Call, ClipAdded, ClipInfo, ExportParams, GraphAddParams, ImportManimParams,
    ImportManimResult, MathAddParams, ProjectState, RenderFrameParams, RenderFrameResult, Timeline,
    TrackInfo, clip_kind_label, media_type_of, timeline_of,
};
pub use path::{read_bounded, validate_existing_file, validate_output_file};
pub use server::{
    ControlServer, MAX_REQUEST_BYTES, RunningServer, SocketSpec, default_socket_spec,
};

mod templates;
pub use templates::{
    MediaFill, Placeholder, ProjectTemplate, TemplateFill, TemplateManifest, delete_user_template,
    project_templates, save_user_template, template_catalog, template_library_dir,
};
