//! CLI review and application of automatic cuts.
use super::{Result, load, parse_time};
use animathio_control::{AutocutParams, SilenceMetric, apply_autocuts, propose_autocuts};
use animathio_core::{ClipId, History, Time, TimeRange, TrackId};
use clap::Args;
use std::path::PathBuf;

#[derive(Args)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct AutocutArgs {
    /// Input project archive or project.json.
    project: PathBuf,
    /// Silence entry threshold in dBFS.
    #[arg(long, default_value = "-40", allow_hyphen_values = true)]
    silence_db: f64,
    /// Minimum pause in seconds or Nticks.
    #[arg(long, default_value = "0.5", value_parser = parse_time)]
    min_silence: Time,
    /// Silence exit threshold above the entry level, in dB.
    #[arg(long, default_value = "3")]
    hysteresis_db: f64,
    /// Use peak amplitude instead of RMS.
    #[arg(long)]
    peak: bool,
    /// Also remove pauses shorter than --min-silence.
    #[arg(long)]
    remove_short_pauses: bool,
    /// Silence retained before speech, in seconds or Nticks.
    #[arg(long, default_value = "0.1", value_parser = parse_time)]
    padding_before: Time,
    /// Silence retained after speech, in seconds or Nticks.
    #[arg(long, default_value = "0.1", value_parser = parse_time)]
    padding_after: Time,
    /// Analyse a clip, including linked audio for a video clip.
    #[arg(long, conflicts_with = "track")]
    clip: Option<u64>,
    /// Analyse this track's audio mix.
    #[arg(long)]
    track: Option<u64>,
    /// Inclusive analysis start in seconds or Nticks (requires --to).
    #[arg(long, requires = "to", value_parser = parse_time)]
    from: Option<Time>,
    /// Exclusive analysis end in seconds or Nticks (requires --from).
    #[arg(long, requires = "from", value_parser = parse_time)]
    to: Option<Time>,
    /// Ripple all unlocked tracks, rather than selected tracks and companions.
    #[arg(long)]
    all_unlocked_tracks: bool,
    /// Include filler phrases and repeats; requires a build with Whisper.
    #[arg(long)]
    fillers: bool,
    /// ISO language code for speech recognition and filler defaults.
    #[arg(long)]
    language: Option<String>,
    /// Filler phrase override; may be repeated.
    #[arg(long = "filler-word")]
    filler_words: Vec<String>,
    /// Whisper model: tiny, base or small.
    #[arg(long, default_value = "base")]
    model: String,
    /// Consent to download a missing model.
    #[arg(long)]
    download_model: bool,
    /// Apply the proposed cuts, saving atomically to the input unless --output is given.
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

pub(crate) fn run(args: AutocutArgs) -> Result<()> {
    let mut bundle = load(&args.project)?;
    let params = AutocutParams {
        silence_db: args.silence_db,
        hysteresis_db: args.hysteresis_db,
        metric: if args.peak {
            SilenceMetric::Peak
        } else {
            SilenceMetric::Rms
        },
        min_silence: args.min_silence,
        keep_short_pauses: !args.remove_short_pauses,
        padding_before: args.padding_before,
        padding_after: args.padding_after,
        clip: args.clip.map(ClipId),
        track: args.track.map(TrackId),
        range: args
            .from
            .zip(args.to)
            .map(|(s, e)| TimeRange::new(s, e))
            .transpose()?,
        all_unlocked_tracks: args.all_unlocked_tracks,
        fillers: args.fillers,
        language: args.language,
        filler_words: args.filler_words,
        model: args.model,
        download_model: args.download_model,
    };
    let proposal = propose_autocuts(&bundle.project, &bundle.assets, &params)?;
    if args.apply {
        if let Some(output) = &args.output
            && output.exists()
            && !args.overwrite
        {
            return Err("output exists; pass --overwrite".into());
        }
        apply_autocuts(&mut bundle.project, &mut History::default(), &proposal)?;
        match args.output {
            Some(output) => animathio_format::save_amio(output, &bundle.project, &bundle.assets)?,
            None if args
                .project
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("json")) =>
            {
                animathio_format::save_json(args.project, &bundle.project)?
            }
            None => animathio_format::save_amio(args.project, &bundle.project, &bundle.assets)?,
        }
    }
    println!("{}", serde_json::to_string_pretty(&proposal)?);
    Ok(())
}
