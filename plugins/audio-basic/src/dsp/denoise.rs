//! `denoise`: streaming spectral-gating noise reduction.
//!
//! A 512-point STFT (periodic sqrt-Hann analysis and synthesis windows, hop 128, 75 %
//! overlap) runs over the stream. The noise profile is learned continuously from the
//! quietest frames: per bin, the minimum of the smoothed power over the last ~1 s
//! (8 sub-windows of 125 ms), so it follows slowly changing noise and needs no
//! user-chosen noise sample. A bin is attenuated towards `-40 dB * strength` while its
//! power stays near that profile; the gate opens softly (smoothstep) above
//! `sensitivity_db`. The gain mask is smoothed across neighbouring bins and in time
//! (fast open, slow close, set by `smoothing`) so isolated bins do not flicker into
//! "musical noise". The same mask is applied to every channel, which keeps the stereo
//! image.
//!
//! Everything is `f32` in the state blob and `f64` in the arithmetic, and the stream is
//! processed one frame at a time, so the output is bit-identical however the signal is
//! cut into blocks. The effect has a fixed latency of 512 frames (the audio is delayed
//! by that much, and the last 512 frames of a clip are cut off by the mixer's clip
//! window).

use super::{Processor, Reader, Writer};
use std::f64::consts::PI;

/// Highest channel count; keeps the state blob below the host's 64 KiB limit.
pub const MAX_DENOISE_CHANNELS: usize = 8;
/// STFT size.
const N: usize = 512;
/// Hop between frames.
const H: usize = 128;
/// Bins up to Nyquist.
const BINS: usize = N / 2 + 1;
/// Sub-windows of the minimum tracker.
const SUBS: usize = 8;
/// Length of one tracker sub-window in seconds.
const SUB_SECONDS: f64 = 0.125;
/// Frames before the mask starts to act (the tracker has no data yet).
const WARMUP_FRAMES: u32 = 4;
/// Smoothing of the power estimate per frame.
const POWER_SMOOTHING: f64 = 0.9;
/// Maximum attenuation at `strength = 1`.
const MAX_REDUCTION_DB: f64 = 40.0;
const TINY: f64 = 1e-20;

/// Plan: window, FFT tables and the constant overlap-add scale.
struct Plan {
    window: [f64; N],
    cos: [f64; N / 2],
    sin: [f64; N / 2],
    reverse: [u16; N],
    scale: f64,
}

impl Plan {
    fn new() -> Self {
        let hann = |n: usize| 0.5 - 0.5 * (2.0 * PI * n as f64 / N as f64).cos();
        let mut window = [0.0; N];
        for (n, w) in window.iter_mut().enumerate() {
            *w = hann(n).sqrt();
        }
        let mut cos = [0.0; N / 2];
        let mut sin = [0.0; N / 2];
        for k in 0..N / 2 {
            let angle = -2.0 * PI * k as f64 / N as f64;
            cos[k] = angle.cos();
            sin[k] = angle.sin();
        }
        let mut reverse = [0_u16; N];
        for (i, r) in reverse.iter_mut().enumerate() {
            *r = (i.reverse_bits() >> (usize::BITS - N.trailing_zeros())) as u16;
        }
        // Analysis x synthesis = Hann; the overlapped Hann frames sum to N / (2 H).
        let scale = 1.0 / (0..N / H).map(|m| hann(m * H)).sum::<f64>();
        Self {
            window,
            cos,
            sin,
            reverse,
            scale,
        }
    }

    /// In-place radix-2 FFT; `inverse` conjugates the twiddles (no 1/N scaling).
    fn fft(&self, re: &mut [f64; N], im: &mut [f64; N], inverse: bool) {
        for i in 0..N {
            let j = usize::from(self.reverse[i]);
            if j > i {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let sign = if inverse { -1.0 } else { 1.0 };
        let mut len = 2;
        while len <= N {
            let half = len / 2;
            let step = N / len;
            for start in (0..N).step_by(len) {
                for k in 0..half {
                    let (wr, wi) = (self.cos[k * step], sign * self.sin[k * step]);
                    let (a, b) = (start + k, start + k + half);
                    let tr = re[b] * wr - im[b] * wi;
                    let ti = re[b] * wi + im[b] * wr;
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            len *= 2;
        }
    }
}

struct Denoise {
    plan: std::rc::Rc<Plan>,
    channels: usize,
    floor: f64,
    threshold: f64,
    release: f64,
    sub_frames: u32,
    // Streaming buffers, per channel.
    hist: Vec<Vec<f32>>,
    cur: Vec<Vec<f32>>,
    acc: Vec<Vec<f32>>,
    out: Vec<std::collections::VecDeque<f32>>,
    // Noise tracker and mask memory, shared by all channels.
    power: Vec<f32>,
    cur_min: Vec<f32>,
    subs: Vec<Vec<f32>>,
    sub_count: u32,
    sub_frames_done: u32,
    frames: u32,
    gain: Vec<f32>,
}

/// Build the effect; `channels` must be within `1..=MAX_DENOISE_CHANNELS`.
pub fn new(
    strength: f64,
    sensitivity_db: f64,
    smoothing: f64,
    rate: f64,
    channels: usize,
) -> Box<dyn Processor> {
    Box::new(Denoise {
        plan: std::rc::Rc::new(Plan::new()),
        channels,
        floor: 10_f64.powf(-MAX_REDUCTION_DB * strength / 20.0),
        threshold: 10_f64.powf(sensitivity_db / 10.0),
        release: 0.4 + 0.55 * smoothing,
        sub_frames: ((rate * SUB_SECONDS / H as f64).round() as u32).max(1),
        hist: vec![vec![0.0; N - H]; channels],
        cur: vec![Vec::with_capacity(H); channels],
        acc: vec![vec![0.0; N]; channels],
        out: (0..channels)
            .map(|_| std::iter::repeat_n(0.0, H).collect())
            .collect(),
        power: vec![0.0; BINS],
        cur_min: vec![0.0; BINS],
        subs: vec![vec![0.0; BINS]; SUBS],
        sub_count: 0,
        sub_frames_done: 0,
        frames: 0,
        gain: vec![1.0; BINS],
    })
}

impl Denoise {
    /// Process the frame that has just filled; push `H` finished samples per channel.
    #[allow(clippy::needless_range_loop)]
    fn hop(&mut self) {
        let channels = self.channels;
        let plan = std::rc::Rc::clone(&self.plan);
        let mut re = vec![[0.0_f64; N]; channels];
        let mut im = vec![[0.0_f64; N]; channels];
        let mut power = [0.0_f64; BINS];
        for c in 0..channels {
            for n in 0..N {
                let x = if n < N - H {
                    self.hist[c][n]
                } else {
                    self.cur[c][n - (N - H)]
                };
                re[c][n] = f64::from(x) * plan.window[n];
            }
            plan.fft(&mut re[c], &mut im[c], false);
            for k in 0..BINS {
                power[k] += re[c][k] * re[c][k] + im[c][k] * im[c][k];
            }
        }
        #[allow(clippy::cast_precision_loss)]
        for p in &mut power {
            *p /= channels as f64;
        }
        self.track_noise(&power);
        let mask = self.mask(&power);
        for c in 0..channels {
            for k in 0..BINS {
                let g = f64::from(mask[k]);
                re[c][k] *= g;
                im[c][k] *= g;
                if k > 0 && k < N / 2 {
                    re[c][N - k] *= g;
                    im[c][N - k] *= g;
                }
            }
            plan.fft(&mut re[c], &mut im[c], true);
            let gain = plan.scale / N as f64;
            for n in 0..N {
                self.acc[c][n] += (re[c][n] * plan.window[n] * gain) as f32;
            }
            self.out[c].extend(self.acc[c][..H].iter().copied());
            self.acc[c].copy_within(H.., 0);
            self.acc[c][N - H..].fill(0.0);
            let (hist, cur) = (&mut self.hist[c], &mut self.cur[c]);
            hist.copy_within(H.., 0);
            hist[N - 2 * H..].copy_from_slice(cur);
            cur.clear();
        }
        self.frames = self.frames.saturating_add(1);
    }

    /// Update the smoothed power and the sub-window minima.
    fn track_noise(&mut self, power: &[f64; BINS]) {
        let first = self.frames == 0;
        for k in 0..BINS {
            let smoothed = if first {
                power[k]
            } else {
                POWER_SMOOTHING * f64::from(self.power[k]) + (1.0 - POWER_SMOOTHING) * power[k]
            };
            self.power[k] = smoothed as f32;
            let min = if first || self.sub_frames_done == 0 {
                self.power[k]
            } else {
                self.cur_min[k].min(self.power[k])
            };
            self.cur_min[k] = min;
        }
        self.sub_frames_done += 1;
        if self.sub_frames_done >= self.sub_frames {
            let slot = (self.sub_count as usize) % SUBS;
            self.subs[slot].copy_from_slice(&self.cur_min);
            self.sub_count = self.sub_count.saturating_add(1).min(u32::MAX / 2);
            self.sub_frames_done = 0;
        }
    }

    /// The smoothed per-bin gain for the current frame.
    fn mask(&mut self, power: &[f64; BINS]) -> [f32; BINS] {
        let valid = (self.sub_count as usize).min(SUBS);
        let mut raw = [1.0_f64; BINS];
        if self.frames >= WARMUP_FRAMES {
            for k in 0..BINS {
                let mut noise = f64::from(self.cur_min[k]);
                for sub in &self.subs[..valid] {
                    noise = noise.min(f64::from(sub[k]));
                }
                let ratio = power[k] / (noise * self.threshold + TINY);
                let t = ((ratio - 0.5) / 1.5).clamp(0.0, 1.0);
                raw[k] = self.floor + (1.0 - self.floor) * t * t * (3.0 - 2.0 * t);
            }
        }
        let mut mask = [1.0_f32; BINS];
        for k in 0..BINS {
            let left = raw[k.saturating_sub(1)];
            let right = raw[(k + 1).min(BINS - 1)];
            let spread = 0.25 * left + 0.5 * raw[k] + 0.25 * right;
            let previous = f64::from(self.gain[k]);
            let blend = if spread > previous {
                0.7
            } else {
                1.0 - self.release
            };
            let g = (previous + (spread - previous) * blend).clamp(self.floor, 1.0);
            self.gain[k] = g as f32;
            mask[k] = g as f32;
        }
        mask
    }
}

impl Processor for Denoise {
    fn run(&mut self, samples: &mut [f32], channels: usize) {
        for frame in samples.chunks_exact_mut(channels) {
            for (c, sample) in frame.iter_mut().enumerate() {
                self.cur[c].push(*sample);
            }
            if self.cur[0].len() == H {
                self.hop();
            }
            for (c, sample) in frame.iter_mut().enumerate() {
                *sample = self.out[c].pop_front().unwrap_or(0.0);
            }
        }
    }

    fn save(&self, w: &mut Writer) {
        w.u32(self.cur[0].len() as u32);
        w.u32(self.out[0].len() as u32);
        w.u32(self.sub_count);
        w.u32(self.sub_frames_done);
        w.u32(self.frames);
        for c in 0..self.channels {
            w.f32s(&self.hist[c]);
            w.f32s(&self.cur[c]);
            w.f32s(&self.acc[c][..N - H]);
            w.f32s(self.out[c].iter());
        }
        w.f32s(&self.power);
        w.f32s(&self.cur_min);
        for sub in &self.subs {
            w.f32s(sub);
        }
        w.f32s(&self.gain);
    }

    fn load(&mut self, r: &mut Reader<'_>) -> Result<(), String> {
        let phase = r.u32()? as usize;
        let queued = r.u32()? as usize;
        if phase >= H || queued > 2 * H {
            return Err("effect state is inconsistent".to_owned());
        }
        self.sub_count = r.u32()?;
        self.sub_frames_done = r.u32()?;
        self.frames = r.u32()?;
        for c in 0..self.channels {
            r.f32s_into(&mut self.hist[c])?;
            self.cur[c].resize(phase, 0.0);
            r.f32s_into(&mut self.cur[c])?;
            r.f32s_into(&mut self.acc[c][..N - H])?;
            let mut out = vec![0.0; queued];
            r.f32s_into(&mut out)?;
            self.out[c] = out.into();
        }
        r.f32s_into(&mut self.power)?;
        r.f32s_into(&mut self.cur_min)?;
        for sub in &mut self.subs {
            r.f32s_into(sub)?;
        }
        r.f32s_into(&mut self.gain)?;
        Ok(())
    }
}
