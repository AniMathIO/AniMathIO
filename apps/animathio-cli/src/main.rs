//! Headless project validation and deterministic PNG rendering.
use animathio_core::{AssetId, Project, Rounding, TICKS_PER_SECOND, Time, evaluate};
use animathio_format::{Bundle, Limits, read_amio, read_json, write_amio};
use animathio_render::{RenderCtx, render, sample};
use clap::{Args, Parser, Subcommand};
use std::{
    collections::BTreeMap,
    error::Error,
    fs::{self, File},
    io::{BufReader, Read},
    path::{Path, PathBuf},
    process::ExitCode,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Parser)]
#[command(version, about = "AniMathIO headless CPU renderer")]
struct Cli {
    #[command(subcommand)]
    command: Action,
}
#[derive(Subcommand)]
enum Action {
    /// Render one frame to PNG.
    Render {
        /// project.json (assets/<hash> alongside it), or a self-contained .amio.
        project: PathBuf,
        /// Zero-based frame index at the project's rational frame rate.
        #[arg(long, conflicts_with = "time", required_unless_present = "time", value_parser = clap::value_parser!(i64).range(0..))]
        frame: Option<i64>,
        /// Exact nonnegative decimal seconds (e.g. 1.25), or integer ticks (123ticks).
        #[arg(long, value_parser = parse_time)]
        time: Option<Time>,
        #[arg(short, long)]
        output: PathBuf,
        #[command(flatten)]
        size: OutputSize,
    },
    /// Render [from, to) frame indices to `frame_000000.png`, etc.
    RenderSeq {
        project: PathBuf,
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(i64).range(0..))]
        from: i64,
        /// Exclusive final frame index.
        #[arg(long, value_parser = clap::value_parser!(i64).range(0..))]
        to: i64,
        #[arg(short, long)]
        output: PathBuf,
        #[command(flatten)]
        size: OutputSize,
    },
    /// Check model invariants and all asset content hashes.
    Validate { project: PathBuf },
    /// Write a three-second animated image/text/shape demo archive.
    NewSample {
        #[arg(short, long)]
        output: PathBuf,
    },
}
#[derive(Args)]
struct OutputSize {
    /// Overrides the project's output width.
    #[arg(long)]
    width: Option<u32>,
    /// Overrides the project's output height.
    #[arg(long)]
    height: Option<u32>,
}
impl OutputSize {
    fn tuple(&self, project: &Project) -> (u32, u32) {
        let settings = project.settings();
        (
            self.width.unwrap_or(settings.width),
            self.height.unwrap_or(settings.height),
        )
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Action::NewSample { output } => {
            let sample = sample::build()?;
            write_amio(File::create(output)?, &sample.project, &sample.assets)?;
        }
        Action::Validate { project } => {
            let bundle = load(&project)?;
            bundle.project.validate()?;
            println!(
                "valid: {} tracks, {} assets",
                bundle.project.tracks().len(),
                bundle.assets.len()
            );
        }
        Action::Render {
            project,
            frame,
            time,
            output,
            size,
        } => {
            let bundle = load(&project)?;
            let time = if let Some(frame) = frame {
                bundle
                    .project
                    .frame_rate()
                    .frame_to_time(frame, Rounding::Nearest)?
            } else {
                time.ok_or("--frame or --time is required")?
            };
            let mut ctx = context(bundle.assets)?;
            render(
                &evaluate(&bundle.project, time),
                &mut ctx,
                size.tuple(&bundle.project),
            )?
            .save_png(output)?;
        }
        Action::RenderSeq {
            project,
            from,
            to,
            output,
            size,
        } => {
            if from >= to {
                return Err("--from must be less than exclusive --to".into());
            }
            let bundle = load(&project)?;
            // Check both conversion extremes and dimensions before creating any output.
            let rate = bundle.project.frame_rate();
            rate.frame_to_time(to - 1, Rounding::Nearest)?;
            let mut ctx = context(bundle.assets)?;
            let first = render(
                &evaluate(
                    &bundle.project,
                    rate.frame_to_time(from, Rounding::Nearest)?,
                ),
                &mut ctx,
                size.tuple(&bundle.project),
            )?;
            fs::create_dir_all(&output)?;
            first.save_png(output.join(format!("frame_{from:06}.png")))?;
            for frame in from + 1..to {
                let time = rate.frame_to_time(frame, Rounding::Nearest)?;
                render(
                    &evaluate(&bundle.project, time),
                    &mut ctx,
                    size.tuple(&bundle.project),
                )?
                .save_png(output.join(format!("frame_{frame:06}.png")))?;
            }
        }
    }
    Ok(())
}

fn context(assets: BTreeMap<AssetId, Vec<u8>>) -> Result<RenderCtx> {
    let mut ctx = RenderCtx::default();
    for (id, bytes) in assets {
        ctx.resources.insert(id, bytes)?;
    }
    Ok(ctx)
}

fn load(path: &Path) -> Result<Bundle> {
    let reader = BufReader::new(File::open(path)?);
    if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("amio"))
    {
        return Ok(read_amio(reader, Limits::default())?);
    }
    let project: Project = read_json(reader)?;
    let limits = Limits::default();
    let mut total = 0_u64;
    let mut assets = BTreeMap::new();
    for id in project.assets().keys() {
        let path = path
            .parent()
            .unwrap_or(Path::new("."))
            .join("assets")
            .join(id.as_str());
        let mut bytes = Vec::new();
        File::open(&path)?
            .take(limits.asset_bytes.min(limits.total_bytes - total) + 1)
            .read_to_end(&mut bytes)?;
        let len = u64::try_from(bytes.len())?;
        if len > limits.asset_bytes || len > limits.total_bytes - total {
            return Err("asset byte limit exceeded".into());
        }
        total += len;
        if AssetId::of(&bytes) != *id {
            return Err(format!("asset hash mismatch: {}", path.display()).into());
        }
        assets.insert(id.clone(), bytes);
    }
    Ok(Bundle { project, assets })
}

// Decimal arithmetic stays in integer ticks; reject sub-tick values instead of rounding.
fn parse_time(value: &str) -> std::result::Result<Time, String> {
    let invalid = || {
        "time must be nonnegative decimal seconds exactly representable in ticks, or Nticks"
            .to_owned()
    };
    if let Some(ticks) = value.strip_suffix("ticks") {
        return ticks
            .parse::<i64>()
            .ok()
            .filter(|v| *v >= 0)
            .map(Time)
            .ok_or_else(invalid);
    }
    let (seconds, fraction) = value.split_once('.').unwrap_or((value, ""));
    if seconds.is_empty()
        || !seconds.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
        || fraction.len() > 18
    {
        return Err(invalid());
    }
    let seconds: i128 = seconds.parse().map_err(|_| invalid())?;
    let fraction_value: i128 = if fraction.is_empty() {
        0
    } else {
        fraction.parse().map_err(|_| invalid())?
    };
    let divisor = 10_i128.pow(u32::try_from(fraction.len()).map_err(|_| invalid())?);
    let fractional_ticks = fraction_value * i128::from(TICKS_PER_SECOND);
    if fractional_ticks % divisor != 0 {
        return Err(invalid());
    }
    let ticks = seconds
        .checked_mul(i128::from(TICKS_PER_SECOND))
        .and_then(|s| s.checked_add(fractional_ticks / divisor))
        .and_then(|t| i64::try_from(t).ok())
        .ok_or_else(invalid)?;
    Ok(Time(ticks))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decimal_time_is_exact_and_checks_bounds() {
        assert_eq!(parse_time("1.25").unwrap(), Time(TICKS_PER_SECOND * 5 / 4));
        assert_eq!(parse_time("3ticks").unwrap(), Time(3));
        assert_eq!(parse_time("0.000000625").unwrap(), Time(441));
        for invalid in [
            "-1",
            "NaN",
            "1e2",
            "0.000000001",
            "1.2.3",
            "",
            "9223372036854775808ticks",
            "999999999999999999999999999999999999999",
        ] {
            assert!(parse_time(invalid).is_err(), "accepted {invalid}");
        }
    }
}
