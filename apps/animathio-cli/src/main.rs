//! Headless project validation and deterministic PNG rendering.
mod mcp;

use animathio_core::{AssetId, Project, Rounding, TICKS_PER_SECOND, Time, evaluate};
use animathio_format::{Bundle, Limits, read_amio, read_json, save_amio};
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
        /// Delivery preset: `youtube_1080p`, `youtube_4k`, `shorts_vertical`, `square` or `portrait`.
        #[arg(long, value_parser = parse_export_preset)]
        preset: Option<animathio_control::ExportPreset>,
        /// Constant quality, 0..=51; lower is better (defaults to preset quality or 23).
        #[arg(long)]
        crf: Option<f32>,
        /// Replace an existing output file instead of failing.
        #[arg(long)]
        overwrite: bool,
    },
    /// Check model invariants and all asset content hashes.
    Validate { project: PathBuf },
    /// Import media into a new project, including linked companion audio.
    ImportMedia {
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
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

fn parse_export_preset(
    value: &str,
) -> std::result::Result<animathio_control::ExportPreset, String> {
    animathio_control::ExportPreset::from_name(value)
        .ok_or_else(|| format!("unknown export preset: {value}"))
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

#[allow(clippy::too_many_lines)]
fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Action::NewSample { output } => {
            let sample = sample::build()?;
            save_amio(output, &sample.project, &sample.assets)?;
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
        Action::ImportMedia { input, output } => import_media(&input, &output)?,
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
            save_amio(&output, &project, &asset_map)?;
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
            let mut plugins = plugin_host()?;
            ctx.set_effect_processor(plugins.effect_processor());
            render(
                &plugins.resolve(
                    &evaluate(&bundle.project, time),
                    size.tuple(&bundle.project),
                ),
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
            preset,
            crf,
            overwrite,
        } => {
            let mut bundle = load(&project)?;
            if let Some(preset) = preset {
                preset
                    .command(&bundle.project)?
                    .apply(&mut bundle.project)?;
            }
            let defaults = preset.map_or_default(animathio_control::ExportPreset::export_settings);
            let range = animathio_core::TimeRange::new(
                from,
                to.unwrap_or_else(|| animathio_export::duration(&bundle.project)),
            )?;
            let destination = animathio_export::Destination {
                path: output,
                overwrite,
            };
            let mut plugins = plugin_host()?;
            let mut exporter = animathio_export::Exporter::new(&bundle.project, &bundle.assets)?;
            exporter.set_effect_processor(plugins.effect_processor());
            exporter.export_to_file(
                &bundle.project,
                range,
                animathio_export::Settings {
                    frame_rate: fps,
                    crf: crf.unwrap_or(defaults.crf),
                },
                &destination,
                |progress| {
                    if progress.completed == progress.total {
                        eprintln!("exported {} frames", progress.total);
                    }
                    true
                },
                |scene| plugins.resolve(&scene, animathio_export::resolution(&bundle.project)),
            )?;
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
            let mut plugins = plugin_host()?;
            ctx.set_effect_processor(plugins.effect_processor());
            let first = render(
                &plugins.resolve(
                    &evaluate(
                        &bundle.project,
                        rate.frame_to_time(from, Rounding::Nearest)?,
                    ),
                    size.tuple(&bundle.project),
                ),
                &mut ctx,
                size.tuple(&bundle.project),
            )?;
            fs::create_dir_all(&output)?;
            first.save_png(output.join(format!("frame_{from:06}.png")))?;
            for frame in from + 1..to {
                let time = rate.frame_to_time(frame, Rounding::Nearest)?;
                render(
                    &plugins.resolve(
                        &evaluate(&bundle.project, time),
                        size.tuple(&bundle.project),
                    ),
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
    save_amio(output, &project, &asset_map)?;
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

fn plugin_host() -> Result<animathio_plugin_host::PluginHost> {
    let cache = directories::BaseDirs::new().map_or_else(
        || std::env::temp_dir().join("animathio-plugin-cache"),
        |base| base.cache_dir().join("animathio/plugins"),
    );
    let mut host = animathio_plugin_host::PluginHost::new(cache)?;
    for dir in animathio_plugin_host::plugin_search_dirs() {
        host.load_plugin_directory(&dir)?;
    }
    Ok(host)
}

fn import_media(input: &Path, output: &Path) -> Result<()> {
    use animathio_core::{Asset, Clip, ClipId, ClipKind, Command, TimeRange, Track, TrackId};
    use animathio_media::MediaBackend;
    let mut bytes = Vec::new();
    File::open(input)?
        .take(Limits::default().asset_bytes + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > Limits::default().asset_bytes {
        return Err("media too large".into());
    }
    let name = input
        .file_name()
        .ok_or("missing filename")?
        .to_string_lossy()
        .into_owned();
    let info = animathio_media::FilmCraftBackend::new().probe_bytes(&name, bytes.clone().into())?;
    let id = AssetId::of(&bytes);
    let mut project = Project::default();
    let range = TimeRange::new(Time(0), info.duration)?;
    let has_video = info.video.is_some();
    let mut clip = Clip::new(
        ClipId(1),
        if has_video {
            ClipKind::Video(id.clone())
        } else {
            ClipKind::Audio(id.clone())
        },
        range,
    );
    if has_video && !info.audio.is_empty() {
        clip.link_group = Some(ClipId(1));
    }
    if !has_video {
        clip.params.insert("gain".into(), 1.0.into());
        clip.params.insert("muted".into(), 0.0.into());
    }
    let mut commands = vec![
        Command::AddAsset {
            id: id.clone(),
            asset: Asset {
                name,
                media_type: animathio_control::media_type_of(&input.to_string_lossy()).into(),
            },
        },
        Command::AddTrack {
            track: Track::new(TrackId(1), "Media"),
            index: 0,
        },
        Command::AddClip {
            track: TrackId(1),
            clip,
        },
    ];
    if has_video && !info.audio.is_empty() {
        let mut audio = Clip::new(ClipId(2), ClipKind::Audio(id.clone()), range);
        audio.link_group = Some(ClipId(1));
        audio.params.insert("gain".into(), 1.0.into());
        audio.params.insert("muted".into(), 0.0.into());
        commands.push(Command::AddTrack {
            track: Track::new(TrackId(2), "Audio"),
            index: 1,
        });
        commands.push(Command::AddClip {
            track: TrackId(2),
            clip: audio,
        });
    }
    Command::Batch { commands }.apply(&mut project)?;
    save_amio(output, &project, &BTreeMap::from([(id, bytes)]))?;
    Ok(())
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
