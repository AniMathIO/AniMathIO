//! Headless project validation and deterministic PNG rendering.
mod mcp;

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
    /// Export an offline H.264 + AAC MP4.
    Export {
        project: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        /// Inclusive start in decimal seconds or Nticks.
        #[arg(long, value_parser = parse_time, default_value = "0")]
        from: Time,
        /// Exclusive end in decimal seconds or Nticks; defaults to project duration.
        #[arg(long, value_parser = parse_time)]
        to: Option<Time>,
        /// Output rate, e.g. 30 or 30000/1001.
        #[arg(long, value_parser = parse_rate)]
        fps: Option<animathio_core::FrameRate>,
        /// Constant quality, 0..=51; lower is better.
        #[arg(long, default_value_t = 23.0)]
        crf: f32,
    },
    /// Check model invariants and all asset content hashes.
    Validate { project: PathBuf },
    /// Convert a v1 `.animathio` project to a `.amio` archive.
    ImportV1 {
        /// v1 `.animathio` project file (deflated JSON).
        input: PathBuf,
        /// Output `.amio` archive.
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Convert a Manim Community scene script to a `.amio` archive.
    ImportManim {
        /// Manim scene script (`.py`).
        input: PathBuf,
        /// Import this scene class; defaults to the first `Scene` subclass.
        #[arg(long)]
        scene: Option<String>,
        /// Output `.amio` archive.
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Write a three-second animated image/text/shape demo archive.
    NewSample {
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Run an MCP stdio server: connects to a running editor, or drives a
    /// project file headlessly when `--project` is given.
    Mcp {
        /// Project file to operate on headlessly (a `.amio` archive or `project.json`).
        #[arg(long)]
        project: Option<PathBuf>,
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
        // An unset project resolution (`0`) falls back to the historical CLI
        // defaults so headless renders of imported projects still work.
        let fallback_width = if project.width() == 0 {
            640
        } else {
            project.width()
        };
        let fallback_height = if project.height() == 0 {
            360
        } else {
            project.height()
        };
        (
            self.width.unwrap_or(fallback_width),
            self.height.unwrap_or(fallback_height),
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
        Action::ImportV1 { input, output } => {
            let bytes = read_import_input(&input)?;
            let (project, assets, report) = animathio_import_v1::import_v1_with_report(&bytes)?;
            for warning in report.warnings() {
                eprintln!("warning: {warning}");
            }
            let asset_map: BTreeMap<AssetId, Vec<u8>> = assets
                .into_iter()
                .map(|asset| (asset.id, asset.bytes))
                .collect();
            write_amio(File::create(&output)?, &project, &asset_map)?;
            println!(
                "imported {} tracks, {} assets into {}",
                project.tracks().len(),
                asset_map.len(),
                output.display()
            );
        }
        Action::ImportManim {
            input,
            scene,
            output,
        } => run_import_manim(&input, scene, &output)?,
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
        Action::Export {
            project,
            output,
            from,
            to,
            fps,
            crf,
        } => {
            let bundle = load(&project)?;
            let range = animathio_core::TimeRange::new(
                from,
                to.unwrap_or_else(|| animathio_export::duration(&bundle.project)),
            )?;
            let bytes = animathio_export::export(
                &bundle.project,
                &bundle.assets,
                range,
                animathio_export::Settings {
                    frame_rate: fps,
                    crf,
                },
                |progress| {
                    if progress.completed == progress.total {
                        eprintln!("exported {} frames", progress.total);
                    }
                    true
                },
            )?;
            fs::write(output, bytes)?;
        }
        Action::Mcp { project } => run_mcp(project)?,
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

fn run_import_manim(input: &Path, scene: Option<String>, output: &Path) -> Result<()> {
    let source = fs::read_to_string(input)?;
    let options = animathio_import_manim::ManimOptions {
        scene,
        script_dir: input.parent().map(Path::to_path_buf),
    };
    let (project, assets, report) =
        animathio_import_manim::import_manim_with_options(&source, &options)?;
    for warning in report.warnings() {
        eprintln!("warning: {warning}");
    }
    for info in report.infos() {
        eprintln!("info: {info}");
    }
    let asset_map: BTreeMap<AssetId, Vec<u8>> = assets
        .into_iter()
        .map(|asset| (asset.id, asset.bytes))
        .collect();
    write_amio(File::create(output)?, &project, &asset_map)?;
    println!(
        "imported {} tracks, {} assets into {}",
        project.tracks().len(),
        asset_map.len(),
        output.display()
    );
    Ok(())
}

/// Run the MCP stdio server on a dedicated tokio runtime.
#[tokio::main]
async fn run_mcp(project: Option<PathBuf>) -> Result<()> {
    use rmcp::{ServiceExt, transport::stdio};

    let backend = mcp::build_backend(project)?;
    let service = mcp::McpServer::new(backend)
        .serve(stdio())
        .await
        .inspect_err(|error| eprintln!("mcp serve error: {error:?}"))?;
    service.waiting().await?;
    Ok(())
}

fn context(assets: BTreeMap<AssetId, Vec<u8>>) -> Result<RenderCtx> {
    Ok(RenderCtx::from_assets(assets)?)
}

fn read_import_input(path: &Path) -> Result<Vec<u8>> {
    // The raw .animathio payload is deflated; bound it to the inflate budget
    // so an untrusted file cannot force a larger read than the importer allows.
    let limit = animathio_import_v1::ImportLimits::default().inflated_bytes;
    let mut bytes = Vec::new();
    File::open(path)?
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len())? > limit {
        return Err("v1 project input exceeds the size limit".into());
    }
    Ok(bytes)
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

fn parse_rate(value: &str) -> std::result::Result<animathio_core::FrameRate, String> {
    let (num, den) = value.split_once('/').unwrap_or((value, "1"));
    let num = num.parse().map_err(|_| "invalid frame rate".to_owned())?;
    let den = den.parse().map_err(|_| "invalid frame rate".to_owned())?;
    animathio_core::FrameRate::new(num, den).map_err(|e| e.to_string())
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
