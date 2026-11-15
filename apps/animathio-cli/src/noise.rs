//! Offline denoise profile learning.
use super::{Result, load, parse_time};
use animathio_core::{ClipId, EffectId, Time, TimeRange};
use clap::Args;
use std::path::PathBuf;

#[derive(Args)]
pub(crate) struct NoiseArgs {
    /// Input archive or project JSON.
    project: PathBuf,
    /// Audio clip id.
    #[arg(long)]
    clip: u64,
    /// Existing denoise instance id.
    #[arg(long)]
    effect: u64,
    /// Inclusive timeline start (seconds or Nticks).
    #[arg(long, value_parser = parse_time)]
    from: Time,
    /// Exclusive timeline end (seconds or Nticks).
    #[arg(long, value_parser = parse_time)]
    to: Time,
    /// Output archive (must not exist).
    #[arg(short, long)]
    output: PathBuf,
}
pub(crate) fn run(args: NoiseArgs) -> Result<()> {
    if args.output.exists() {
        return Err("output exists".into());
    }
    let mut bundle = load(&args.project)?;
    let command = animathio_control::learn_noise_command(
        &bundle.project,
        &bundle.assets,
        ClipId(args.clip),
        EffectId(args.effect),
        TimeRange::new(args.from, args.to)?,
    )?;
    command.apply(&mut bundle.project)?;
    animathio_format::save_amio(args.output, &bundle.project, &bundle.assets)?;
    Ok(())
}
