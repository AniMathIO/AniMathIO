//! Offline, deterministic beat analysis: spectral-flux onset strength, an autocorrelation
//! tempo estimate and a dynamic-programming beat tracker (Ellis, "Beat Tracking by Dynamic
//! Programming", 2007), with sample-accurate refinement of each beat onto the nearest
//! attack. Pure functions over mono samples; no decoder, device or allocation of runtime
//! resources, and the same input always gives the same beats.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use crate::Error;
use animathio_core::{TICKS_PER_SECOND, Time};

/// Beat-analysis options. Defaults suit popular music.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeatParams {
    /// Slowest tempo considered, in beats per minute.
    pub min_bpm: f64,
    /// Fastest tempo considered, in beats per minute.
    pub max_bpm: f64,
    /// Force this tempo instead of estimating it (the beat phase is still tracked).
    pub bpm: Option<f64>,
    /// How strongly beats keep to a steady tempo; larger is more rigid (default 100).
    pub tightness: f64,
}
impl Default for BeatParams {
    fn default() -> Self {
        Self {
            min_bpm: 60.0,
            max_bpm: 200.0,
            bpm: None,
            tightness: 100.0,
        }
    }
}

/// One tracked beat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Beat {
    /// Time of the beat in ticks, relative to the first analysed sample.
    pub time: Time,
    /// Onset strength at the beat, in units of the onset envelope's standard deviation.
    pub strength: f32,
}

/// Result of [`detect_beats`].
#[derive(Clone, Debug, PartialEq)]
pub struct BeatAnalysis {
    /// Tempo in beats per minute, from the median spacing of the tracked beats.
    pub bpm: f64,
    /// Strength of the tempo periodicity in `0..=1`; low values mean an unclear beat.
    pub confidence: f64,
    /// Beats in ascending time order.
    pub beats: Vec<Beat>,
}

const MIN_SECONDS: f64 = 2.0;
const MAX_FRAMES: usize = 4_000_000;

fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}

/// In-place radix-2 FFT of `re`/`im` (length a power of two), with precomputed tables.
struct Fft {
    n: usize,
    cos: Vec<f64>,
    sin: Vec<f64>,
    rev: Vec<usize>,
}
impl Fft {
    fn new(n: usize) -> Self {
        let bits = n.trailing_zeros();
        Self {
            n,
            cos: (0..n / 2)
                .map(|k| (-2.0 * std::f64::consts::PI * k as f64 / n as f64).cos())
                .collect(),
            sin: (0..n / 2)
                .map(|k| (-2.0 * std::f64::consts::PI * k as f64 / n as f64).sin())
                .collect(),
            rev: (0..n)
                .map(|i| i.reverse_bits() >> (usize::BITS - bits))
                .collect(),
        }
    }
    fn run(&self, re: &mut [f64], im: &mut [f64]) {
        for (i, j) in self.rev.iter().copied().enumerate() {
            if j > i {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut size = 2;
        while size <= self.n {
            let half = size / 2;
            let step = self.n / size;
            for start in (0..self.n).step_by(size) {
                for k in 0..half {
                    let (c, s) = (self.cos[k * step], self.sin[k * step]);
                    let (a, b) = (start + k, start + k + half);
                    let tr = re[b] * c - im[b] * s;
                    let ti = re[b] * s + im[b] * c;
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            size *= 2;
        }
    }
}

fn to_ticks(sample: f64, rate: f64) -> Time {
    Time((sample * TICKS_PER_SECOND as f64 / rate).round() as i64)
}

/// Log-compressed half-wave rectified spectral flux per frame, mean-removed and scaled to
/// unit deviation. Frame `f` covers samples `[f * hop, f * hop + n)`; `fps` is the frame rate.
fn onset_envelope(samples: &[f32], n: usize, hop: usize, scale: f64, fps: f64) -> Vec<f64> {
    let fft = Fft::new(n);
    let window: Vec<f64> = (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos())
        .collect();
    let frames = (samples.len() - n) / hop + 1;
    let bins = n / 2;
    let (mut re, mut im) = (vec![0.0; n], vec![0.0; n]);
    let mut previous = vec![0.0_f64; bins];
    let mut flux = vec![0.0_f64; frames];
    for (f, out) in flux.iter_mut().enumerate() {
        let frame = &samples[f * hop..f * hop + n];
        for (((r, i), x), w) in re.iter_mut().zip(im.iter_mut()).zip(frame).zip(&window) {
            *r = f64::from(*x) * scale * w;
            *i = 0.0;
        }
        fft.run(&mut re, &mut im);
        let mut sum = 0.0;
        for ((r, i), prev) in re[1..bins]
            .iter()
            .zip(&im[1..bins])
            .zip(previous[1..bins].iter_mut())
        {
            let level = (1.0 + 40.0 * r.hypot(*i)).ln();
            if f > 0 {
                sum += (level - *prev).max(0.0);
            }
            *prev = level;
        }
        *out = sum;
    }
    // Remove the slowly varying floor so steady loud passages do not read as onsets.
    let radius = ((0.25 * fps) as usize).clamp(1, frames);
    let mut prefix = vec![0.0; frames + 1];
    for (i, v) in flux.iter().enumerate() {
        prefix[i + 1] = prefix[i] + v;
    }
    let mut env: Vec<f64> = (0..frames)
        .map(|i| {
            let (lo, hi) = (i.saturating_sub(radius), (i + radius + 1).min(frames));
            (flux[i] - (prefix[hi] - prefix[lo]) / (hi - lo) as f64).max(0.0)
        })
        .collect();
    let deviation = (env.iter().map(|v| v * v).sum::<f64>() / frames as f64).sqrt();
    if deviation > 0.0 {
        for v in &mut env {
            *v /= deviation;
        }
    }
    env
}

/// Unbiased autocorrelation of `env` at `lag` frames.
fn autocorrelation(env: &[f64], lag: usize) -> f64 {
    let count = env.len() - lag;
    env[..count]
        .iter()
        .zip(&env[lag..])
        .map(|(a, b)| a * b)
        .sum::<f64>()
        / count as f64
}

/// Beat period in (fractional) frames and a `0..=1` periodicity confidence.
fn estimate_period(env: &[f64], fps: f64, params: &BeatParams) -> Result<(f64, f64), Error> {
    let zero = autocorrelation(env, 0);
    if zero <= 0.0 {
        return Err(invalid(
            "no onsets found; the audio is silent or featureless",
        ));
    }
    if let Some(bpm) = params.bpm {
        let period = 60.0 * fps / bpm;
        let lag = (period.round() as usize).clamp(1, env.len() - 1);
        return Ok((period, (autocorrelation(env, lag) / zero).clamp(0.0, 1.0)));
    }
    let lo = ((60.0 * fps / params.max_bpm).floor() as usize).max(2);
    let hi = ((60.0 * fps / params.min_bpm).ceil() as usize).min(env.len() / 2);
    if hi <= lo + 1 {
        return Err(invalid("audio too short for the requested tempo range"));
    }
    let centre = 60.0 * fps / 120.0;
    // Log-Gaussian preference for moderate tempi (1.4 octave deviation), as in Ellis 2007.
    let acf: Vec<f64> = (lo - 1..=hi + 1)
        .map(|lag| autocorrelation(env, lag))
        .collect();
    let weighted = |index: usize| {
        let lag = (lo - 1 + index) as f64;
        let octaves = (lag / centre).log2() / 1.4;
        acf[index] * (-0.5 * octaves * octaves).exp()
    };
    let mut best = 1;
    for index in 1..acf.len() - 1 {
        if weighted(index) > weighted(best) {
            best = index;
        }
    }
    // Parabolic interpolation of the weighted peak.
    let (a, b, c) = (weighted(best - 1), weighted(best), weighted(best + 1));
    let denominator = a - 2.0 * b + c;
    let shift = if denominator.abs() > f64::EPSILON {
        (0.5 * (a - c) / denominator).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    let period = (lo - 1 + best) as f64 + shift;
    Ok((period, (acf[best] / zero).clamp(0.0, 1.0)))
}

/// Ellis dynamic-programming beat tracker; returns beat frame indices in ascending order.
fn track_frames(env: &[f64], period: f64, tightness: f64) -> Vec<usize> {
    let frames = env.len();
    let min_lag = ((period / 2.0).ceil() as usize).max(1);
    let max_lag = ((period * 2.0).floor() as usize).max(min_lag);
    let penalty: Vec<f64> = (0..=max_lag)
        .map(|lag| {
            if lag < min_lag {
                f64::INFINITY
            } else {
                tightness * (lag as f64 / period).ln().powi(2)
            }
        })
        .collect();
    let mut score = vec![0.0_f64; frames];
    let mut parent = vec![usize::MAX; frames];
    for t in 0..frames {
        let mut best = f64::NEG_INFINITY;
        let mut from = usize::MAX;
        for lag in min_lag..=max_lag.min(t) {
            let candidate = score[t - lag] - penalty[lag];
            if candidate > best {
                best = candidate;
                from = t - lag;
            }
        }
        if from == usize::MAX || best < 0.0 {
            // Starting a new chain beats extending a poor one.
            score[t] = env[t];
        } else {
            score[t] = env[t] + best;
            parent[t] = from;
        }
    }
    // The chain end is the best score within the final beat period.
    let tail = frames.saturating_sub(period.ceil() as usize + 1);
    let mut end = tail;
    for (t, value) in score.iter().enumerate().skip(tail) {
        if *value > score[end] {
            end = t;
        }
    }
    let mut beats = vec![end];
    while parent[*beats.last().expect("nonempty")] != usize::MAX {
        beats.push(parent[*beats.last().expect("nonempty")]);
    }
    beats.reverse();
    beats
}

/// Detect the tempo and beat times of mono `samples` at `sample_rate`.
///
/// Beat times are in ticks from the first sample. Needs at least two seconds of audio.
/// Silent or featureless audio is an error rather than an empty result.
pub fn detect_beats(
    samples: &[f32],
    sample_rate: u32,
    params: &BeatParams,
) -> Result<BeatAnalysis, Error> {
    if !(8_000..=384_000).contains(&sample_rate) {
        return Err(invalid("unsupported sample rate for beat detection"));
    }
    if !params.min_bpm.is_finite()
        || !params.max_bpm.is_finite()
        || params.min_bpm < 20.0
        || params.max_bpm > 400.0
        || params.min_bpm >= params.max_bpm
        || params.bpm.is_some_and(|b| !(20.0..=400.0).contains(&b))
        || !params.tightness.is_finite()
        || params.tightness <= 0.0
    {
        return Err(invalid("invalid tempo range or tightness"));
    }
    let rate = f64::from(sample_rate);
    if (samples.len() as f64) < MIN_SECONDS * rate {
        return Err(invalid(
            "audio too short for beat detection (needs 2 seconds)",
        ));
    }
    if samples.iter().any(|s| !s.is_finite()) {
        return Err(invalid("non-finite audio samples"));
    }
    let peak = samples.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
    if peak < 1e-5 {
        return Err(invalid(
            "no onsets found; the audio is silent or featureless",
        ));
    }
    let n = ((0.04 * rate) as usize).next_power_of_two() / 2;
    let n = n.max(64);
    let hop = (n / 8).max(1);
    let fps = rate / hop as f64;
    if samples.len() / hop > MAX_FRAMES {
        return Err(invalid("audio too long for beat detection"));
    }
    let env = onset_envelope(samples, n, hop, 1.0 / f64::from(peak), fps);
    let (period, confidence) = estimate_period(&env, fps, params)?;
    let frames = track_frames(&env, period, params.tightness);

    // Trim leading/trailing beats that sit on near-silence (the chain runs through gaps).
    let strength_at = |t: usize| {
        env[t.saturating_sub(2)..(t + 3).min(env.len())]
            .iter()
            .fold(0.0_f64, |m, v| m.max(*v))
    };
    let strengths: Vec<f64> = frames.iter().map(|t| strength_at(*t)).collect();
    let rms = (strengths.iter().map(|v| v * v).sum::<f64>() / strengths.len() as f64).sqrt();
    let keep: Vec<usize> = (0..frames.len())
        .filter(|i| strengths[*i] >= 0.5 * rms)
        .collect();
    let (Some(&first), Some(&last)) = (keep.first(), keep.last()) else {
        return Err(invalid("no beats found"));
    };

    // Refine each beat onto the sharpest attack near its frame, at sample resolution.
    let width = ((0.0015 * rate) as usize).max(2);
    let mut prefix = vec![0.0_f64; samples.len() + 1];
    for (i, v) in samples.iter().enumerate() {
        prefix[i + 1] = prefix[i] + f64::from(*v) * f64::from(*v);
    }
    let energy =
        |from: usize, to: usize| prefix[to.min(samples.len())] - prefix[from.min(samples.len())];
    let radius = (((0.05 * rate) as usize).min((period * hop as f64 / 3.0) as usize)).max(1);
    let beats: Vec<Beat> = (first..=last)
        .map(|i| {
            let centre = frames[i] * hop + n / 2;
            let lo = centre.saturating_sub(radius).max(width);
            let hi = (centre + radius).min(samples.len().saturating_sub(width));
            let mut best = (centre.saturating_sub(n / 4), 0.0_f64);
            for sample in lo..hi {
                let rise = energy(sample, sample + width) - energy(sample - width, sample);
                if rise > best.1 {
                    best = (sample, rise);
                }
            }
            Beat {
                time: to_ticks(best.0 as f64, rate),
                strength: strengths[i] as f32,
            }
        })
        .collect();
    if beats.len() < 2 {
        return Err(invalid("fewer than two beats found"));
    }
    let mut gaps: Vec<i64> = beats
        .windows(2)
        .map(|w| w[1].time.0 - w[0].time.0)
        .collect();
    gaps.sort_unstable();
    let median = gaps[gaps.len() / 2] as f64 / TICKS_PER_SECOND as f64;
    let bpm = if median > 0.0 {
        60.0 / median
    } else {
        60.0 * fps / period
    };
    Ok(BeatAnalysis {
        bpm,
        confidence,
        beats,
    })
}

/// Indices into `beats` of the downbeats for `per_bar` beats per bar: the phase whose beats
/// carry the most onset strength (ties go to the earliest phase).
#[must_use]
pub fn downbeat_indices(beats: &[Beat], per_bar: usize) -> Vec<usize> {
    let per_bar = per_bar.max(1);
    let mut sums = vec![0.0_f64; per_bar];
    for (i, beat) in beats.iter().enumerate() {
        sums[i % per_bar] += f64::from(beat.strength);
    }
    let mut phase = 0;
    for (p, sum) in sums.iter().enumerate() {
        if *sum > sums[phase] + 1e-9 {
            phase = p;
        }
    }
    (phase..beats.len()).step_by(per_bar).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decaying 1 kHz bursts at `bpm`, first click at `offset` seconds; every `accent`-th
    /// click (when nonzero) is loud and the others quiet.
    fn clicks(
        rate: u32,
        seconds: f64,
        bpm: f64,
        offset: f64,
        accent: usize,
        noise: f32,
    ) -> Vec<f32> {
        let rate_f = f64::from(rate);
        let mut samples = vec![0.0_f32; (seconds * rate_f) as usize];
        let mut seed = 0x1234_5678_u32;
        for s in &mut samples {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *s = (seed >> 8) as f32 / (1 << 24) as f32 * 2.0 * noise - noise;
        }
        let mut k = 0_usize;
        loop {
            let at = offset + k as f64 * 60.0 / bpm;
            let start = (at * rate_f).round() as usize;
            if start + 200 >= samples.len() {
                break;
            }
            let amp = if accent == 0 || k.is_multiple_of(accent) {
                0.9
            } else {
                0.45
            };
            for i in 0..(0.012 * rate_f) as usize {
                let t = i as f64 / rate_f;
                samples[start + i] +=
                    (amp * (-t * 400.0).exp() * (2.0 * std::f64::consts::PI * 1000.0 * t).sin())
                        as f32;
            }
            k += 1;
        }
        samples
    }
    fn assert_beats(analysis: &BeatAnalysis, bpm: f64, offset: f64, tolerance_ms: f64) {
        let period = 60.0 / bpm;
        let mut worst = 0.0_f64;
        for beat in &analysis.beats {
            let at = beat.time.0 as f64 / TICKS_PER_SECOND as f64;
            let k = ((at - offset) / period).round();
            worst = worst.max(((at - offset - k * period) * 1000.0).abs());
        }
        assert!(
            worst <= tolerance_ms,
            "worst beat error {worst:.2} ms > {tolerance_ms} ms"
        );
    }

    #[test]
    fn click_track_at_120_bpm_gives_120_and_millisecond_beats() {
        let samples = clicks(16_000, 20.0, 120.0, 0.13, 0, 0.0);
        let analysis = detect_beats(&samples, 16_000, &BeatParams::default()).unwrap();
        assert!((analysis.bpm - 120.0).abs() < 0.5, "bpm {}", analysis.bpm);
        assert!(
            analysis.confidence > 0.3,
            "confidence {}",
            analysis.confidence
        );
        assert!(
            (38..=40).contains(&analysis.beats.len()),
            "{}",
            analysis.beats.len()
        );
        assert_beats(&analysis, 120.0, 0.13, 3.0);
    }

    #[test]
    fn other_tempi_and_sample_rates_and_noise() {
        for (rate, bpm, offset, noise) in [
            (44_100, 120.0, 0.0, 0.0),
            (48_000, 90.0, 0.4, 0.02),
            (16_000, 140.0, 0.21, 0.03),
            (22_050, 100.0, 0.05, 0.0),
        ] {
            let samples = clicks(rate, 24.0, bpm, offset, 0, noise);
            let analysis = detect_beats(&samples, rate, &BeatParams::default()).unwrap();
            assert!(
                (analysis.bpm - bpm).abs() < 0.8,
                "{rate} Hz {bpm}: got {}",
                analysis.bpm
            );
            assert_beats(&analysis, bpm, offset, 4.0);
        }
    }

    #[test]
    fn tempo_hint_and_range_constrain_the_estimate() {
        let samples = clicks(16_000, 20.0, 120.0, 0.0, 0, 0.0);
        let hinted = detect_beats(
            &samples,
            16_000,
            &BeatParams {
                bpm: Some(120.0),
                ..BeatParams::default()
            },
        )
        .unwrap();
        assert!((hinted.bpm - 120.0).abs() < 0.5);
        let narrow = BeatParams {
            min_bpm: 110.0,
            max_bpm: 130.0,
            ..BeatParams::default()
        };
        assert!((detect_beats(&samples, 16_000, &narrow).unwrap().bpm - 120.0).abs() < 0.5);
    }

    #[test]
    fn accented_clicks_give_downbeats_on_the_loud_phase() {
        // Loud click every fourth beat, starting on the third beat of the clip.
        let mut samples = clicks(16_000, 24.0, 120.0, 0.25, 0, 0.0);
        let louder = clicks(16_000, 24.0, 120.0, 0.25 + 2.0 * 0.5, 4, 0.0);
        for (a, b) in samples.iter_mut().zip(&louder) {
            *a = *a * 0.5 + *b;
        }
        let analysis = detect_beats(&samples, 16_000, &BeatParams::default()).unwrap();
        let downbeats = downbeat_indices(&analysis.beats, 4);
        assert!(downbeats.len() >= 10);
        let first = analysis.beats[downbeats[0]].time.0 as f64 / TICKS_PER_SECOND as f64;
        let bar = (first - 0.25 - 1.0) / 2.0;
        assert!((bar - bar.round()).abs() < 0.01, "downbeat at {first}");
        assert!(downbeats.windows(2).all(|w| w[1] - w[0] == 4));
    }

    #[test]
    fn analysis_is_deterministic() {
        let samples = clicks(16_000, 10.0, 128.0, 0.1, 4, 0.02);
        let a = detect_beats(&samples, 16_000, &BeatParams::default()).unwrap();
        let b = detect_beats(&samples, 16_000, &BeatParams::default()).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn silence_short_audio_and_bad_params_are_errors() {
        assert!(detect_beats(&vec![0.0; 80_000], 16_000, &BeatParams::default()).is_err());
        assert!(detect_beats(&vec![0.5; 100], 16_000, &BeatParams::default()).is_err());
        let samples = clicks(16_000, 5.0, 120.0, 0.0, 0, 0.0);
        for params in [
            BeatParams {
                min_bpm: 200.0,
                max_bpm: 60.0,
                ..BeatParams::default()
            },
            BeatParams {
                tightness: 0.0,
                ..BeatParams::default()
            },
            BeatParams {
                bpm: Some(f64::NAN),
                ..BeatParams::default()
            },
        ] {
            assert!(detect_beats(&samples, 16_000, &params).is_err());
        }
        assert!(detect_beats(&samples, 100, &BeatParams::default()).is_err());
    }

    #[test]
    fn downbeat_phase_and_fft_sanity() {
        let beats: Vec<Beat> = [1.0, 3.0, 1.0, 1.0, 1.0, 3.0, 1.0, 1.0]
            .iter()
            .enumerate()
            .map(|(i, s)| Beat {
                time: Time(i64::try_from(i).unwrap()),
                strength: *s,
            })
            .collect();
        assert_eq!(downbeat_indices(&beats, 4), [1, 5]);
        assert_eq!(downbeat_indices(&beats, 1).len(), 8);
        let fft = Fft::new(8);
        let mut re = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let mut im = [0.0; 8];
        fft.run(&mut re, &mut im);
        assert!(re.iter().all(|v| (v - 1.0).abs() < 1e-12));
    }
}
