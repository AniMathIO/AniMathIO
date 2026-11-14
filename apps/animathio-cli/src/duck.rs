//! CLI review and application of auto-ducking.
use super::{Result, load, parse_time};
use animathio_control::{DuckParams, apply_duck, propose_duck};
use animathio_core::{ClipId, History, Time, TimeRange, TrackId};
use clap::Args;
use std::path::PathBuf;

#[derive(Args)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct DuckArgs {
    /// Input project archive or project.json.
    project: PathBuf,
    /// Voice clip id; may be repeated (a video clip resolves its linked audio).
    #[arg(long = "voice-clip")]
    voice_clips: Vec<u64>,
    /// Voice track id; may be repeated.
    #[arg(long = "voice-track")]
    voice_tracks: Vec<u64>,
    /// Music track id to duck; may be repeated, at least one is required.
    #[arg(long = "music-track", required = true)]
    music_tracks: Vec<u64>,
    /// How far the music drops under speech, in dB.
    #[arg(long, default_value = "12")]
    depth_db: f64,
    /// Ramp down, finishing when the voice starts (seconds or Nticks).
    #[arg(long, default_value = "0.3", value_parser = parse_time)]
    attack: Time,
    /// Ramp up, starting when the voice ends (seconds or Nticks).
    #[arg(long, default_value = "0.5", value_parser = parse_time)]
    release: Time,
    /// Bridge voice gaps shorter than this (seconds or Nticks).
    #[arg(long, default_value = "0.4", value_parser = parse_time)]
    min_gap: Time,
    /// Voice is absent below this dBFS level.
    #[arg(long, default_value = "-40", allow_hyphen_values = true)]
    threshold_db: f64,
    /// Voice returns this many dB above the threshold.
    #[arg(long, default_value = "3")]
    hysteresis_db: f64,
    /// Inclusive voice analysis start in seconds or Nticks (requires --to).
    #[arg(long, requires = "to", value_parser = parse_time)]
    from: Option<Time>,
    /// Exclusive voice analysis end in seconds or Nticks (requires --from).
    #[arg(long, requires = "from", value_parser = parse_time)]
    to: Option<Time>,
    /// Overwrite existing gain keyframes on the music clips.
    #[arg(long)]
    replace: bool,
    /// Apply the duck, saving atomically to the input unless --output is given.
    #[arg(long, conflicts_with = "dry_run", required_unless_present = "dry_run")]
    apply: bool,
    /// Print the review proposal as JSON without saving changes.
    #[arg(long, conflicts_with = "apply", required_unless_present = "apply")]
    dry_run: bool,
    /// Save an edited archive here instead of changing the input.
    #[arg(short, long, requires = "apply")]
    output: Option<PathBuf>,
    /// Replace an existing output archive.
    #[arg(long)]
    overwrite: bool,
}

pub(crate) fn run(args: DuckArgs) -> Result<()> {
    let mut bundle = load(&args.project)?;
    let params = DuckParams {
        voice_clips: args.voice_clips.into_iter().map(ClipId).collect(),
        voice_tracks: args.voice_tracks.into_iter().map(TrackId).collect(),
        music_tracks: args.music_tracks.into_iter().map(TrackId).collect(),
        depth_db: args.depth_db,
        attack: args.attack,
        release: args.release,
        min_gap: args.min_gap,
        threshold_db: args.threshold_db,
        hysteresis_db: args.hysteresis_db,
        range: args
            .from
            .zip(args.to)
            .map(|(s, e)| TimeRange::new(s, e))
            .transpose()?,
        replace_existing: args.replace,
    };
    let proposal = propose_duck(&bundle.project, &bundle.assets, &params)?;
    if args.apply {
        if let Some(output) = &args.output
            && output.exists()
            && !args.overwrite
        {
            return Err("output exists; pass --overwrite".into());
        }
        apply_duck(&mut bundle.project, &mut History::default(), &proposal)?;
        match args.output {
            Some(output) => animathio_format::save_amio(output, &bundle.project, &bundle.assets)?,
            None if args
                .project
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("json")) =>
            {
                animathio_format::save_json(args.project, &bundle.project)?;
            }
            None => animathio_format::save_amio(args.project, &bundle.project, &bundle.assets)?,
        }
    }
    println!("{}", serde_json::to_string_pretty(&proposal)?);
    Ok(())
}
