//! CLI timeline markers and beat detection.
use super::{Result, load, parse_time};
use animathio_control::{
    BeatDetectParams, BeatMarkerOptions, BeatSelection, MarkerAddParams, add_marker_command,
    analyze_beats, apply_beat_markers,
};
use animathio_core::{
    ClipId, Command, History, Marker, MarkerColor, MarkerId, Time, TimeRange, TrackId,
};
use animathio_format::Bundle;
use clap::{Args, Subcommand};
use serde_json::json;
use std::path::{Path, PathBuf};

fn parse_color(value: &str) -> std::result::Result<MarkerColor, String> {
    MarkerColor::parse(value).ok_or_else(|| {
        "color must be red, orange, yellow, green, cyan, blue, purple or pink".to_owned()
    })
}

#[derive(Args)]
pub(crate) struct MarkersArgs {
    /// Input project archive or project.json.
    project: PathBuf,
    #[command(subcommand)]
    action: MarkersAction,
}

#[derive(Subcommand)]
enum MarkersAction {
    /// Print every marker as JSON (times are Nticks and decimal seconds).
    List,
    /// Add a marker, saving atomically to the input unless --output is given.
    Add {
        /// Marker time in decimal seconds or Nticks.
        #[arg(long, value_parser = parse_time)]
        time: Time,
        /// Length of a range marker in seconds or Nticks; omit for a point marker.
        #[arg(long, value_parser = parse_time)]
        duration: Option<Time>,
        /// Display name.
        #[arg(long, default_value = "")]
        name: String,
        /// Palette colour: red, orange, yellow, green, cyan, blue, purple or pink.
        #[arg(long, default_value = "blue", value_parser = parse_color)]
        color: MarkerColor,
        /// Free-form note.
        #[arg(long, default_value = "")]
        note: String,
        #[command(flatten)]
        save: SaveArgs,
    },
    /// Remove a marker by id, saving atomically to the input unless --output is given.
    Remove {
        /// Marker id from `markers list`.
        id: u64,
        #[command(flatten)]
        save: SaveArgs,
    },
}

#[derive(Args)]
struct SaveArgs {
    /// Save an edited archive here instead of changing the input.
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Replace an existing output archive.
    #[arg(long)]
    overwrite: bool,
}

fn seconds(time: Time) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    {
        time.0 as f64 / animathio_core::TICKS_PER_SECOND as f64
    }
}

fn marker_json(marker: &Marker) -> serde_json::Value {
    json!({
        "id": marker.id.0,
        "time": marker.time.0,
        "seconds": seconds(marker.time),
        "duration": marker.duration.map(|d| d.0),
        "name": marker.name,
        "color": marker.color.name(),
        "note": marker.note,
    })
}

fn save(input: &Path, save: &SaveArgs, bundle: &Bundle) -> Result<()> {
    if let Some(output) = &save.output
        && output.exists()
        && !save.overwrite
    {
        return Err("output exists; pass --overwrite".into());
    }
    match &save.output {
        Some(output) => animathio_format::save_amio(output, &bundle.project, &bundle.assets)?,
        None if input
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json")) =>
        {
            animathio_format::save_json(input, &bundle.project)?;
        }
        None => animathio_format::save_amio(input, &bundle.project, &bundle.assets)?,
    }
    Ok(())
}

pub(crate) fn run_markers(args: MarkersArgs) -> Result<()> {
    let mut bundle = load(&args.project)?;
    match args.action {
        MarkersAction::List => {
            let markers: Vec<_> = bundle.project.markers().iter().map(marker_json).collect();
            println!("{}", serde_json::to_string_pretty(&markers)?);
        }
        MarkersAction::Add {
            time,
            duration,
            name,
            color,
            note,
            save: save_args,
        } => {
            let (command, id) = add_marker_command(
                &bundle.project,
                &MarkerAddParams {
                    time,
                    duration,
                    name,
                    color,
                    note,
                },
            )?;
            History::default().execute(&mut bundle.project, &command)?;
            save(&args.project, &save_args, &bundle)?;
            println!("{}", json!({"id": id.0}));
        }
        MarkersAction::Remove {
            id,
            save: save_args,
        } => {
            History::default().execute(
                &mut bundle.project,
                &Command::RemoveMarker { id: MarkerId(id) },
            )?;
            save(&args.project, &save_args, &bundle)?;
            println!("{}", json!({"removed": id}));
        }
    }
    Ok(())
}

#[derive(Args)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct BeatsArgs {
    /// Input project archive or project.json.
    project: PathBuf,
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
    /// Slowest tempo considered, in beats per minute.
    #[arg(long, default_value = "60")]
    min_bpm: f64,
    /// Fastest tempo considered, in beats per minute.
    #[arg(long, default_value = "200")]
    max_bpm: f64,
    /// Force this tempo instead of estimating it.
    #[arg(long)]
    bpm: Option<f64>,
    /// Beats per bar, used to pick downbeats.
    #[arg(long, default_value = "4")]
    beats_per_bar: u32,
    /// Add a marker at each selected beat, saving atomically to the input unless
    /// --output is given.
    #[arg(long)]
    add_markers: bool,
    /// With --add-markers: use every Nth beat.
    #[arg(long, default_value = "1", requires = "add_markers")]
    every: u32,
    /// With --add-markers: only mark downbeats.
    #[arg(long, requires = "add_markers")]
    downbeats_only: bool,
    /// With --add-markers: marker colour.
    #[arg(long, default_value = "cyan", value_parser = parse_color, requires = "add_markers")]
    color: MarkerColor,
    /// Save an edited archive here instead of changing the input.
    #[arg(short, long, requires = "add_markers")]
    output: Option<PathBuf>,
    /// Replace an existing output archive.
    #[arg(long)]
    overwrite: bool,
}

pub(crate) fn run_beats(args: BeatsArgs) -> Result<()> {
    let mut bundle = load(&args.project)?;
    let detect = BeatDetectParams {
        clip: args.clip.map(ClipId),
        track: args.track.map(TrackId),
        range: args
            .from
            .zip(args.to)
            .map(|(s, e)| TimeRange::new(s, e))
            .transpose()?,
        min_bpm: args.min_bpm,
        max_bpm: args.max_bpm,
        bpm: args.bpm,
        beats_per_bar: args.beats_per_bar,
    };
    let detection = analyze_beats(&bundle.project, &bundle.assets, &detect)?;
    let mut out = json!({
        "bpm": detection.bpm,
        "confidence": detection.confidence,
        "beats": detection.beats.iter().map(|b| json!({
            "time": b.time.0,
            "seconds": seconds(b.time),
            "strength": b.strength,
        })).collect::<Vec<_>>(),
        "downbeats": detection.downbeats,
    });
    if args.add_markers {
        let options = BeatMarkerOptions {
            selection: BeatSelection {
                every: args.every,
                downbeats_only: args.downbeats_only,
            },
            color: args.color,
            name_prefix: None,
        };
        let added = apply_beat_markers(
            &mut bundle.project,
            &mut History::default(),
            &detection,
            &options,
        )?;
        save(
            &args.project,
            &SaveArgs {
                output: args.output,
                overwrite: args.overwrite,
            },
            &bundle,
        )?;
        out["markers_added"] = json!(added.added);
    }
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}
