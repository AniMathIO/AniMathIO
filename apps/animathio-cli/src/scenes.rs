//! Scene analysis CLI.
use crate::{Result, load};
use animathio_control::{SceneParams, detect_scenes, scene_command};
use animathio_core::{ClipId, History, TrackId};
use clap::{Args, ValueEnum};
use std::{path::PathBuf, sync::atomic::AtomicBool};
#[derive(Clone, Copy, ValueEnum)]
enum Mode {
    Detect,
    Split,
    Markers,
}
#[derive(Args)]
pub(crate) struct SceneArgs {
    /// Input project archive or JSON.
    project: PathBuf,
    /// Detect only, split linked clips, or add markers.
    #[arg(value_enum)]
    mode: Mode,
    /// Analyse this video clip.
    #[arg(long, conflicts_with = "track", required_unless_present = "track")]
    clip: Option<u64>,
    #[arg(long)]
    /// Analyse all video on this track.
    track: Option<u64>,
    #[arg(long, default_value_t = 0.5)]
    /// Sensitivity in 0..=1; higher detects smaller changes.
    sensitivity: f64,
    #[arg(long, default_value_t = 6)]
    /// Minimum shot length in project frames.
    min_shot_frames: u32,
    #[arg(long, default_value_t = 0)]
    /// Remove resulting short shots and linked companions, leaving gaps.
    delete_shorter_than_frames: u32,
    #[arg(short, long)]
    /// Output archive, required for split and markers.
    output: Option<PathBuf>,
    #[arg(long)]
    /// Replace an existing output.
    overwrite: bool,
}
pub(crate) fn run(args: SceneArgs) -> Result<()> {
    if args.output.as_ref().is_some_and(|p| p.exists()) && !args.overwrite {
        return Err("output exists; pass --overwrite".into());
    }
    let mut bundle = load(&args.project)?;
    let draft = detect_scenes(
        &bundle.project,
        &bundle.assets,
        &SceneParams {
            clip: args.clip.map(ClipId),
            track: args.track.map(TrackId),
            sensitivity: args.sensitivity,
            min_shot_frames: args.min_shot_frames,
            delete_shorter_than_frames: args.delete_shorter_than_frames,
        },
        &AtomicBool::new(false),
        &|_| {},
    )?;
    println!("{}", serde_json::to_string_pretty(&draft.cuts)?);
    if !matches!(args.mode, Mode::Detect) {
        let output = args
            .output
            .ok_or("--output is required for split or markers")?;
        let command = scene_command(&bundle.project, &draft, matches!(args.mode, Mode::Markers))?;
        History::default().execute(&mut bundle.project, &command)?;
        animathio_format::save_amio(output, &bundle.project, &bundle.assets)?;
    }
    Ok(())
}
