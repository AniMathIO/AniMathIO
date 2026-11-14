//! Offline auto-reframe CLI.
use crate::{Result, load};
use animathio_control::{ReframeParams, auto_reframe};
use animathio_core::{ClipId, History};
use clap::Args;
use std::path::PathBuf;

#[derive(Args)]
pub(crate) struct ReframeArgs {
    /// Input .amio or project.json.
    project: PathBuf,
    /// Output .amio archive (input is preserved).
    #[arg(short, long)]
    output: PathBuf,
    /// Target aspect: 9:16, 1:1 or 4:5.
    #[arg(long, default_value = "9:16", value_parser = ["9:16", "1:1", "4:5"])]
    aspect: String,
    /// Tracking smoothness, 0..=1; higher follows more slowly.
    #[arg(long, default_value_t = 0.5)]
    smoothness: f64,
    /// Reframe only these video clip ids; repeat for multiple clips.
    #[arg(long = "clip")]
    clips: Vec<u64>,
    /// Replace an existing output archive.
    #[arg(long)]
    overwrite: bool,
}
pub(crate) fn run(args: ReframeArgs) -> Result<()> {
    if args.output.exists() && !args.overwrite {
        return Err("output exists; pass --overwrite".into());
    }
    let mut bundle = load(&args.project)?;
    let clips = auto_reframe(
        &mut bundle.project,
        &mut History::default(),
        &bundle.assets,
        &ReframeParams {
            aspect: args.aspect,
            smoothness: args.smoothness,
            clips: args.clips.into_iter().map(ClipId).collect(),
        },
    )?;
    animathio_format::save_amio(args.output, &bundle.project, &bundle.assets)?;
    println!("Reframed {clips} video clips");
    Ok(())
}
