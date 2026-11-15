//! File-based clip retiming through the same guarded commands as MCP/editor.
use super::{Args, Path, PathBuf, Result, Time, load, parse_time, save_amio};
use animathio_core::{ClipId, ClipSpeed, Command, SpeedPoint};

#[derive(Args)]
pub(crate) struct SpeedArgs {
    project: PathBuf,
    #[arg(long)]
    clip: u64,
    /// Exact multiplier: 0.5, 2 or 3/2 (1/16..16, millionth precision).
    #[arg(long)]
    speed: ClipSpeed,
    #[arg(long)]
    reverse: bool,
    /// JSON array of {"time": ticks, "speed": {"numerator": n, "denominator": d}}.
    #[arg(long)]
    ramps: Option<String>,
    /// Shift trailing clips on all unlocked tracks. Otherwise reject overlaps.
    #[arg(long)]
    ripple: bool,
    #[arg(short, long)]
    output: PathBuf,
    #[arg(long)]
    overwrite: bool,
}
#[derive(Args)]
pub(crate) struct HoldArgs {
    project: PathBuf,
    #[arg(long)]
    clip: u64,
    /// Timeline playhead, decimal seconds or Nticks.
    #[arg(long, value_parser = parse_time)]
    at: Time,
    /// Positive hold duration in project frames; inserts and ripples the tail.
    #[arg(long)]
    frames: u32,
    #[arg(short, long)]
    output: PathBuf,
    #[arg(long)]
    overwrite: bool,
}
pub(crate) fn speed(args: &SpeedArgs) -> Result<()> {
    let ramps: Vec<SpeedPoint> = args
        .ramps
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?
        .unwrap_or_default();
    edit(
        &args.project,
        &args.output,
        args.overwrite,
        &Command::SetClipSpeed {
            id: ClipId(args.clip),
            speed: args.speed,
            reverse: args.reverse,
            ramps,
            ripple: args.ripple,
        },
    )
}
pub(crate) fn hold(args: &HoldArgs) -> Result<()> {
    edit(
        &args.project,
        &args.output,
        args.overwrite,
        &Command::InsertFrameHold {
            id: ClipId(args.clip),
            at: args.at,
            frames: args.frames,
        },
    )
}
fn edit(input: &Path, output: &Path, overwrite: bool, command: &Command) -> Result<()> {
    if output.exists() && !overwrite {
        return Err("output exists; pass --overwrite".into());
    }
    let mut bundle = load(input)?;
    let mut limits = animathio_control::SourceLimits::new();
    limits.guard(&bundle.project, &bundle.assets, command)?;
    command.apply(&mut bundle.project)?;
    save_amio(output, &bundle.project, &bundle.assets)?;
    Ok(())
}
