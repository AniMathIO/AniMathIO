//! `transcribe` and `models`: local speech recognition from the command line.

use super::{Result, load, parse_time};
use animathio_control::{
    CaptionPosition, CaptionStyleParams, ProgressFn, SubtitleGenerateParams, SubtitleProgress,
    SubtitleSplit, apply_cues, transcribe_project,
};
use animathio_core::{History, Time, TimeRange};
use animathio_format::save_amio;
use animathio_transcribe::{
    Cancel, Format, ModelSize, TranscribeOptions, format_cues, models, split_segments,
};
use clap::Args;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, atomic::AtomicBool},
};

#[derive(Args)]
pub(crate) struct TranscribeArgs {
    /// A project (`.amio` or `project.json`; its audio tracks are mixed) or an audio/video
    /// file (first audio track).
    input: PathBuf,
    /// Whisper model: `tiny`, `base` or `small`.
    #[arg(long, default_value = "base")]
    model: String,
    /// ISO 639-1 language code (`en`, `de`, ...); detected automatically when omitted.
    #[arg(long)]
    language: Option<String>,
    /// Download the model first if it is not installed (verified, one-time).
    #[arg(long)]
    download: bool,
    /// Write `SubRip` subtitles here.
    #[arg(long)]
    srt: Option<PathBuf>,
    /// Write `WebVTT` subtitles here.
    #[arg(long)]
    vtt: Option<PathBuf>,
    /// For project inputs: save a copy with the subtitle clips added to this `.amio`.
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Project input only: transcribe this track's audio (numeric id).
    #[arg(long)]
    track: Option<u64>,
    /// Project input only: transcribe this clip's audio (numeric id).
    #[arg(long)]
    clip: Option<u64>,
    /// Project input only: inclusive start in decimal seconds or Nticks.
    #[arg(long, value_parser = parse_time, requires = "to")]
    from: Option<Time>,
    /// Project input only: exclusive end in decimal seconds or Nticks.
    #[arg(long, value_parser = parse_time, requires = "from")]
    to: Option<Time>,
    /// Longest subtitle line in characters.
    #[arg(long)]
    max_chars: Option<usize>,
    /// Most lines per subtitle.
    #[arg(long)]
    max_lines: Option<usize>,
    /// Caption style for the clips added with `--output`: `classic` (default),
    /// `word-highlight`, `karaoke`, `pop`, `one-word` or `bold-outline`.
    #[arg(long, default_value = "classic")]
    style: String,
    /// Caption placement: `top`, `middle` or `bottom` (style default when omitted).
    #[arg(long)]
    position: Option<String>,
    /// Accent colour of the active word, `#RRGGBB[AA]`.
    #[arg(long)]
    accent: Option<String>,
    /// Words per line for the caption style (0 fits the box width).
    #[arg(long)]
    max_words: Option<u32>,
    /// Replace existing output files.
    #[arg(long)]
    overwrite: bool,
}

impl TranscribeArgs {
    fn caption_style(&self) -> Result<CaptionStyleParams> {
        Ok(CaptionStyleParams {
            style: self.style.clone(),
            position: self
                .position
                .as_deref()
                .map(CaptionPosition::parse)
                .transpose()?,
            accent: self
                .accent
                .as_deref()
                .map(animathio_control::parse_caption_color)
                .transpose()?,
            max_words: self.max_words,
            ..CaptionStyleParams::default()
        })
    }
}

#[derive(Args)]
pub(crate) struct ModelsArgs {
    /// Download this model (`tiny`, `base` or `small`) into the user data directory,
    /// verifying its SHA-256; resumes an interrupted download.
    #[arg(long)]
    download: Option<String>,
}

fn progress_printer() -> ProgressFn {
    let last = Mutex::new((u8::MAX, 0u64));
    Arc::new(move |event| {
        let mut last = last
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match event {
            SubtitleProgress::Downloading { downloaded, total } => {
                let step = downloaded / (5 * 1024 * 1024);
                if step != last.1 || downloaded == total {
                    last.1 = step;
                    eprintln!(
                        "downloading model: {} of {}",
                        models::display_size(downloaded),
                        models::display_size(total)
                    );
                }
            }
            SubtitleProgress::Transcribing { percent } => {
                if percent != last.0 && (percent / 10 != last.0 / 10 || percent == 100) {
                    last.0 = percent;
                    eprintln!("transcribing: {percent}%");
                }
            }
        }
    })
}

fn write_checked(path: &Path, text: &str, overwrite: bool) -> Result<()> {
    if path.exists() && !overwrite {
        return Err(format!("{} exists; pass --overwrite", path.display()).into());
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text)?;
    eprintln!("wrote {}", path.display());
    Ok(())
}

#[allow(clippy::too_many_lines)]
pub(crate) fn run(args: &TranscribeArgs) -> Result<()> {
    let cancel: Cancel = Arc::new(AtomicBool::new(false));
    let progress = progress_printer();
    let style = args.caption_style()?;
    if !animathio_control::caption_styles()
        .iter()
        .any(|s| s.id == style.style)
    {
        return Err(format!(
            "unknown caption style `{}` (one of: {})",
            style.style,
            animathio_control::caption_styles()
                .iter()
                .map(|s| s.id)
                .collect::<Vec<_>>()
                .join(", ")
        )
        .into());
    }
    let split = SubtitleSplit {
        max_chars_per_line: args.max_chars,
        max_lines: args.max_lines,
        ..SubtitleSplit::default()
    };
    let is_project = args
        .input
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("amio") || e.eq_ignore_ascii_case("json"));
    let (language, cues, saved) = if is_project {
        let mut bundle = load(&args.input)?;
        let params = SubtitleGenerateParams {
            model: args.model.clone(),
            language: args.language.clone(),
            clip: args.clip.map(animathio_core::ClipId),
            track: args.track.map(animathio_core::TrackId),
            range: match (args.from, args.to) {
                (Some(from), Some(to)) => Some(TimeRange::new(from, to)?),
                _ => None,
            },
            download_model: args.download,
            split,
            style,
        };
        let draft =
            transcribe_project(&bundle.project, &bundle.assets, &params, &cancel, &progress)?;
        let saved = if let Some(output) = &args.output {
            if output.exists() && !args.overwrite {
                return Err(format!("{} exists; pass --overwrite", output.display()).into());
            }
            apply_cues(
                &mut bundle.project,
                &mut History::default(),
                draft.language.clone(),
                &draft.cues,
                &params.style,
            )?;
            save_amio(output, &bundle.project, &bundle.assets)?;
            eprintln!("saved {}", output.display());
            true
        } else {
            false
        };
        (draft.language, draft.cues, saved)
    } else {
        if args.output.is_some()
            || args.track.is_some()
            || args.clip.is_some()
            || args.from.is_some()
        {
            return Err("--output, --track, --clip, --from and --to need a project input".into());
        }
        let size = ModelSize::parse(&args.model)?;
        let samples = animathio_transcribe::mono_16k_from_bytes(fs::read(&args.input)?)?;
        if !size.is_installed() {
            if !args.download {
                size.require()?;
            }
            let sink = Arc::clone(&progress);
            models::download(size, &cancel, &mut |p| {
                sink(SubtitleProgress::Downloading {
                    downloaded: p.downloaded,
                    total: p.total,
                });
            })?;
        }
        let sink = Arc::clone(&progress);
        let transcript = animathio_transcribe::transcribe(
            &samples,
            Time(0),
            &TranscribeOptions {
                model: size,
                language: args.language.clone(),
                threads: None,
            },
            &cancel,
            Arc::new(move |percent| sink(SubtitleProgress::Transcribing { percent })),
        )?;
        let options = split.options()?;
        (
            transcript.language,
            split_segments(&transcript.segments, &options),
            false,
        )
    };
    eprintln!("language: {language}, {} subtitle cues", cues.len());
    if let Some(path) = &args.srt {
        write_checked(path, &format_cues(&cues, Format::Srt), args.overwrite)?;
    }
    if let Some(path) = &args.vtt {
        write_checked(path, &format_cues(&cues, Format::Vtt), args.overwrite)?;
    }
    if args.srt.is_none() && args.vtt.is_none() && !saved {
        print!("{}", format_cues(&cues, Format::Srt));
    }
    Ok(())
}

pub(crate) fn run_models(args: &ModelsArgs) -> Result<()> {
    if let Some(name) = &args.download {
        let size = ModelSize::parse(name)?;
        let info = size.info();
        eprintln!(
            "model `{}` ({}) -> {}",
            info.name,
            models::display_size(info.bytes),
            size.path().display()
        );
        let mut last = u64::MAX;
        let path = models::download(size, &Arc::new(AtomicBool::new(false)), &mut |p| {
            let step = p.downloaded / (10 * 1024 * 1024);
            if step != last || p.downloaded == p.total {
                last = step;
                eprintln!(
                    "downloaded {} of {}",
                    models::display_size(p.downloaded),
                    models::display_size(p.total)
                );
            }
        })?;
        println!("{}", path.display());
        return Ok(());
    }
    println!("model directory: {}", models::models_dir().display());
    println!(
        "speech engine: {}",
        if animathio_transcribe::engine_available() {
            "built in"
        } else {
            "not built in (rebuild with --features whisper)"
        }
    );
    for size in ModelSize::ALL {
        let info = size.info();
        println!(
            "{:<6} {:>7}  {}",
            info.name,
            models::display_size(info.bytes),
            if size.is_installed() {
                "installed"
            } else {
                "not downloaded"
            }
        );
    }
    Ok(())
}
