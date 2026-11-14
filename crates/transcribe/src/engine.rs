//! The speech engine: whisper.cpp over 16 kHz mono samples (cargo feature `whisper`).
//!
//! The engine is synchronous and CPU-bound. Callers run it on a worker thread and pass a
//! [`Cancel`] flag and a progress callback; cancelling aborts inside whisper.cpp at the
//! next decoder step.

use crate::{Error, Segment, models::ModelSize};
use animathio_core::Time;
use std::sync::{Arc, atomic::AtomicBool};

/// Shared cancel flag; set it to `true` to abort a running job.
pub type Cancel = Arc<AtomicBool>;

/// Recognition settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscribeOptions {
    /// Model size; it must already be downloaded.
    pub model: ModelSize,
    /// ISO 639-1 language code (`en`, `de`, ...), or `None` to detect automatically.
    pub language: Option<String>,
    /// CPU threads; `None` uses up to four.
    pub threads: Option<usize>,
}

impl Default for TranscribeOptions {
    fn default() -> Self {
        Self {
            model: ModelSize::Base,
            language: None,
            threads: None,
        }
    }
}

/// Recognition result with timings already shifted onto the caller's timeline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transcript {
    /// Language code the engine used (detected or requested).
    pub language: String,
    /// Recognised segments in order.
    pub segments: Vec<Segment>,
}

/// Whether this build links a speech engine.
#[must_use]
pub const fn engine_available() -> bool {
    cfg!(feature = "whisper")
}

/// Recognise speech in `samples` (16 kHz mono). `origin` is the timeline tick of the first
/// sample; every returned time is `origin` plus the offset in the audio. `progress`
/// receives whole percentages 0..=100 from the engine thread.
#[cfg(not(feature = "whisper"))]
pub fn transcribe(
    _samples: &[f32],
    _origin: Time,
    _options: &TranscribeOptions,
    _cancel: &Cancel,
    _progress: Arc<dyn Fn(u8) + Send + Sync>,
) -> Result<Transcript, Error> {
    Err(Error::EngineUnavailable)
}

/// Recognise speech in `samples` (16 kHz mono). `origin` is the timeline tick of the first
/// sample; every returned time is `origin` plus the offset in the audio. `progress`
/// receives whole percentages 0..=100 from the engine thread.
#[cfg(feature = "whisper")]
#[allow(clippy::too_many_lines, clippy::needless_pass_by_value)]
pub fn transcribe(
    samples: &[f32],
    origin: Time,
    options: &TranscribeOptions,
    cancel: &Cancel,
    progress: Arc<dyn Fn(u8) + Send + Sync>,
) -> Result<Transcript, Error> {
    use crate::{Word, cues::clean_text};
    use animathio_core::TICKS_PER_SECOND;
    use std::sync::atomic::Ordering;
    use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

    const TICKS_PER_CS: i64 = TICKS_PER_SECOND / 100;
    let engine = |e: &dyn std::fmt::Display| Error::Engine(e.to_string());

    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    let model = options.model.require()?;
    // whisper.cpp and ggml log to stderr by default; route them to a (here unused) backend.
    whisper_rs::install_logging_hooks();
    let language = options
        .language
        .as_deref()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.eq_ignore_ascii_case("auto"))
        .map(str::to_ascii_lowercase);
    if let Some(code) = &language
        && whisper_rs::get_lang_id(code).is_none()
    {
        return Err(Error::Invalid(format!("unknown language code `{code}`")));
    }
    // Pure silence makes Whisper hallucinate captions; never feed it.
    let peak = samples.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
    if samples.is_empty() || peak < 1e-4 {
        progress(100);
        return Ok(Transcript {
            language: language.unwrap_or_else(|| "en".to_owned()),
            segments: Vec::new(),
        });
    }
    // whisper.cpp rejects inputs shorter than a second; pad with silence and clamp later.
    let real_cs =
        i64::try_from(samples.len() * 100 / crate::SAMPLE_RATE as usize).unwrap_or(i64::MAX);
    let mut padded;
    let input = if samples.len() < crate::SAMPLE_RATE as usize {
        padded = samples.to_vec();
        padded.resize(crate::SAMPLE_RATE as usize, 0.0);
        &padded[..]
    } else {
        samples
    };

    let mut context_params = WhisperContextParameters::default();
    // CPU inference keeps results reproducible and avoids driver differences.
    context_params.use_gpu(false);
    let context =
        WhisperContext::new_with_params(&model, context_params).map_err(|e| engine(&e))?;
    let mut state = context.create_state().map_err(|e| engine(&e))?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(language.as_deref().or(Some("auto")));
    params.set_translate(false);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_token_timestamps(true);
    params.set_suppress_blank(true);
    params.set_no_speech_thold(0.6);
    let threads = options
        .threads
        .or_else(|| {
            std::thread::available_parallelism()
                .ok()
                .map(|n| n.get().min(4))
        })
        .unwrap_or(2)
        .clamp(1, 32);
    params.set_n_threads(i32::try_from(threads).unwrap_or(2));
    {
        let progress = Arc::clone(&progress);
        params.set_progress_callback_safe(move |percent: i32| {
            progress(u8::try_from(percent.clamp(0, 100)).unwrap_or(100));
        });
        let cancel = Arc::clone(cancel);
        params.set_abort_callback_safe(move || cancel.load(Ordering::Relaxed));
    }
    let result = state.full(params, input);
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    result.map_err(|e| engine(&e))?;

    let eot = context.token_eot();
    let to_time = |cs: i64| Time(origin.0 + cs.clamp(0, real_cs) * TICKS_PER_CS);
    let mut segments = Vec::new();
    for segment in state.as_iter() {
        if segment.no_speech_probability() > 0.8 {
            continue;
        }
        let text = clean_text(&segment.to_str_lossy().map_err(|e| engine(&e))?);
        if text.is_empty() {
            continue;
        }
        let (seg_start, seg_end) = (segment.start_timestamp(), segment.end_timestamp());
        let mut words: Vec<Word> = Vec::new();
        let mut open: Option<(String, i64, i64)> = None;
        for index in 0..segment.n_tokens() {
            let Some(token) = segment.get_token(index) else {
                continue;
            };
            if token.token_id() >= eot {
                continue; // special and timestamp tokens
            }
            let piece = token.to_str_lossy().map_err(|e| engine(&e))?;
            let data = token.token_data();
            let (t0, t1) = if data.t0 >= 0 && data.t1 >= data.t0 {
                (
                    data.t0.clamp(seg_start, seg_end),
                    data.t1.clamp(seg_start, seg_end),
                )
            } else {
                (seg_start, seg_end)
            };
            let starts_word = piece.starts_with(char::is_whitespace);
            match open.as_mut() {
                Some((text, _, end)) if !starts_word => {
                    text.push_str(&piece);
                    *end = (*end).max(t1);
                }
                _ => {
                    if let Some((text, start, end)) = open.take() {
                        words.push(Word {
                            text: text.trim().to_owned(),
                            start: to_time(start),
                            end: to_time(end),
                        });
                    }
                    open = Some((piece.into_owned(), t0, t1));
                }
            }
        }
        if let Some((text, start, end)) = open.take() {
            words.push(Word {
                text: text.trim().to_owned(),
                start: to_time(start),
                end: to_time(end),
            });
        }
        words.retain(|w| !w.text.is_empty());
        segments.push(Segment {
            start: to_time(seg_start),
            end: to_time(seg_end),
            text,
            words,
        });
    }
    let language_name = language.unwrap_or_else(|| {
        whisper_rs::get_lang_str(state.full_lang_id_from_state())
            .map_or_else(|| "en".to_owned(), str::to_owned)
    });
    progress(100);
    Ok(Transcript {
        language: language_name,
        segments,
    })
}
