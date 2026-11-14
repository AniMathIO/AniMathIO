//! Deterministic audio DSP for the `audio-basic` plugin, independent of the plugin ABI so it
//! can be unit-tested natively (`cargo test --manifest-path plugins/audio-basic/Cargo.toml`).
//!
//! All filters run in `f64` internally, over interleaved `f32` blocks. Every effect keeps its
//! memory in a small opaque state blob (little-endian `f64`s behind a version byte and the
//! channel count) that the host hands back with the next contiguous block, so processing a
//! signal in blocks of any size is bit-identical to processing it in one go.

use std::f64::consts::{FRAC_1_SQRT_2, PI};

/// Highest channel count the plugin accepts (matches the mixer).
pub const MAX_CHANNELS: usize = 32;
const STATE_VERSION: u8 = 1;
/// Smallest level the detectors distinguish from silence (-180 dB).
const SILENCE: f64 = 1e-9;

/// A numeric parameter an effect declares.
#[allow(dead_code)] // `label`/`step` are only read by the plugin ABI adapter.
#[derive(Clone, Copy, Debug)]
pub struct ParamDef {
    pub id: &'static str,
    pub label: &'static str,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub step: f64,
}

/// A filter the plugin offers.
#[allow(dead_code)] // `name`/`description` are only read by the plugin ABI adapter.
#[derive(Clone, Copy, Debug)]
pub struct EffectDef {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub params: &'static [ParamDef],
}

const fn p(
    id: &'static str,
    label: &'static str,
    min: f64,
    max: f64,
    default: f64,
    step: f64,
) -> ParamDef {
    ParamDef {
        id,
        label,
        min,
        max,
        default,
        step,
    }
}

/// Every effect of the plugin, in menu order.
pub const EFFECTS: [EffectDef; 6] = [
    EffectDef {
        id: "gain",
        name: "Gain / Trim",
        description: "Boost or cut the level in decibels",
        params: &[p("gain_db", "Gain (dB)", -60.0, 24.0, 0.0, 0.1)],
    },
    EffectDef {
        id: "eq3",
        name: "3-Band EQ",
        description: "Low shelf, mid peak and high shelf equalizer",
        params: &[
            p("low_gain_db", "Low gain (dB)", -18.0, 18.0, 0.0, 0.1),
            p("low_freq", "Low freq (Hz)", 40.0, 400.0, 120.0, 1.0),
            p("mid_gain_db", "Mid gain (dB)", -18.0, 18.0, 0.0, 0.1),
            p("mid_freq", "Mid freq (Hz)", 200.0, 8000.0, 1000.0, 1.0),
            p("mid_q", "Mid Q", 0.2, 8.0, 1.0, 0.01),
            p("high_gain_db", "High gain (dB)", -18.0, 18.0, 0.0, 0.1),
            p("high_freq", "High freq (Hz)", 2000.0, 16000.0, 6000.0, 10.0),
        ],
    },
    EffectDef {
        id: "compressor",
        name: "Compressor",
        description: "Feed-forward compressor with linked channels",
        params: &[
            p("threshold_db", "Threshold (dB)", -60.0, 0.0, -18.0, 0.1),
            p("ratio", "Ratio", 1.0, 20.0, 4.0, 0.1),
            p("attack_ms", "Attack (ms)", 0.1, 200.0, 10.0, 0.1),
            p("release_ms", "Release (ms)", 5.0, 2000.0, 100.0, 1.0),
            p("makeup_db", "Makeup (dB)", -12.0, 24.0, 0.0, 0.1),
        ],
    },
    EffectDef {
        id: "gate",
        name: "Noise Gate",
        description: "Attenuate the signal while it is below the threshold",
        params: &[
            p("threshold_db", "Threshold (dB)", -80.0, 0.0, -50.0, 0.1),
            p("attack_ms", "Attack (ms)", 0.1, 100.0, 2.0, 0.1),
            p("hold_ms", "Hold (ms)", 0.0, 500.0, 50.0, 1.0),
            p("release_ms", "Release (ms)", 5.0, 2000.0, 150.0, 1.0),
            p("range_db", "Range (dB)", 0.0, 80.0, 60.0, 0.1),
        ],
    },
    EffectDef {
        id: "highpass",
        name: "High-pass Filter",
        description: "Second-order Butterworth high-pass (12 dB per octave)",
        params: &[p("frequency", "Frequency (Hz)", 10.0, 2000.0, 80.0, 1.0)],
    },
    EffectDef {
        id: "voice_cleanup",
        name: "Voice Cleanup",
        description: "High-pass, gentle gate and split-band de-esser in one preset",
        params: &[
            p("highpass_hz", "High-pass (Hz)", 20.0, 400.0, 80.0, 1.0),
            p(
                "gate_threshold_db",
                "Gate threshold (dB)",
                -80.0,
                -20.0,
                -55.0,
                0.1,
            ),
            p(
                "deess_freq",
                "De-ess freq (Hz)",
                4000.0,
                10000.0,
                6500.0,
                10.0,
            ),
            p(
                "deess_threshold_db",
                "De-ess threshold (dB)",
                -60.0,
                0.0,
                -30.0,
                0.1,
            ),
            p("deess_amount", "De-ess amount", 0.0, 1.0, 0.5, 0.01),
        ],
    },
];

/// Parameter values resolved against an effect's declaration.
struct Resolved<'a> {
    def: &'a EffectDef,
    values: Vec<f64>,
}

impl Resolved<'_> {
    fn get(&self, id: &str) -> f64 {
        let index = self
            .def
            .params
            .iter()
            .position(|param| param.id == id)
            .expect("parameter declared by the effect");
        self.values[index]
    }
}

fn resolve<'a>(def: &'a EffectDef, params: &[(String, f64)]) -> Result<Resolved<'a>, String> {
    for (id, value) in params {
        if !def.params.iter().any(|param| param.id == id) {
            return Err(format!("effect {} has no parameter {id:?}", def.id));
        }
        if !value.is_finite() {
            return Err(format!("parameter {id:?} must be finite"));
        }
    }
    let values = def
        .params
        .iter()
        .map(|param| {
            params
                .iter()
                .find(|(id, _)| id == param.id)
                .map_or(param.default, |(_, value)| {
                    value.clamp(param.min, param.max)
                })
        })
        .collect();
    Ok(Resolved { def, values })
}

// ---------------------------------------------------------------------------------------
// State blob

struct Writer(Vec<u8>);

impl Writer {
    fn new(channels: usize) -> Self {
        // `channels` is validated to 1..=32 before any effect runs.
        Self(vec![
            STATE_VERSION,
            u8::try_from(channels).expect("channel count"),
        ])
    }
    fn f64(&mut self, value: f64) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8], channels: usize) -> Result<Self, String> {
        match bytes {
            [version, count, rest @ ..]
                if *version == STATE_VERSION && usize::from(*count) == channels =>
            {
                Ok(Self(rest))
            }
            _ => Err("effect state does not match this effect or channel count".to_owned()),
        }
    }
    fn f64(&mut self) -> Result<f64, String> {
        let (head, tail) = self
            .0
            .split_first_chunk::<8>()
            .ok_or_else(|| "effect state is truncated".to_owned())?;
        self.0 = tail;
        let value = f64::from_le_bytes(*head);
        if value.is_finite() {
            Ok(value)
        } else {
            Err("effect state holds a non-finite value".to_owned())
        }
    }
    fn finish(self) -> Result<(), String> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err("effect state has trailing bytes".to_owned())
        }
    }
}

// ---------------------------------------------------------------------------------------
// Building blocks

fn db_to_lin(db: f64) -> f64 {
    (db * (std::f64::consts::LN_10 / 20.0)).exp()
}

fn lin_to_db(lin: f64) -> f64 {
    lin.max(SILENCE).ln() * (20.0 / std::f64::consts::LN_10)
}

/// One-pole smoothing coefficient for a time constant in milliseconds.
fn time_coefficient(ms: f64, rate: f64) -> f64 {
    if ms <= 0.0 {
        0.0
    } else {
        (-1.0 / (ms * 0.001 * rate)).exp()
    }
}

/// Peak magnitude across the channels of one frame (linked detection).
fn peak(frame: &[f32]) -> f64 {
    frame.iter().fold(0.0_f64, |m, x| m.max(f64::from(x.abs())))
}

/// Normalised biquad coefficients (RBJ audio-EQ cookbook), transposed direct form II.
#[derive(Clone, Copy, Debug)]
struct Coefficients {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Coefficients {
    fn normalise(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    /// Keep the corner safely below Nyquist so the bilinear transform stays well behaved.
    fn omega(freq: f64, rate: f64) -> f64 {
        2.0 * PI * freq.clamp(1.0, rate * 0.45) / rate
    }

    fn highpass(freq: f64, q: f64, rate: f64) -> Self {
        let w0 = Self::omega(freq, rate);
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        Self::normalise(
            (1.0 + cos) / 2.0,
            -(1.0 + cos),
            (1.0 + cos) / 2.0,
            1.0 + alpha,
            -2.0 * cos,
            1.0 - alpha,
        )
    }

    fn lowpass(freq: f64, q: f64, rate: f64) -> Self {
        let w0 = Self::omega(freq, rate);
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        Self::normalise(
            (1.0 - cos) / 2.0,
            1.0 - cos,
            (1.0 - cos) / 2.0,
            1.0 + alpha,
            -2.0 * cos,
            1.0 - alpha,
        )
    }

    fn peaking(freq: f64, q: f64, gain_db: f64, rate: f64) -> Self {
        let a = 10f64.powf(gain_db / 40.0);
        let w0 = Self::omega(freq, rate);
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        Self::normalise(
            1.0 + alpha * a,
            -2.0 * cos,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * cos,
            1.0 - alpha / a,
        )
    }

    /// Shelf with slope `S = 1`, so `alpha = sin(w0) / sqrt(2)`.
    fn shelf(freq: f64, gain_db: f64, rate: f64, high: bool) -> Self {
        let a = 10f64.powf(gain_db / 40.0);
        let w0 = Self::omega(freq, rate);
        let (sin, cos) = w0.sin_cos();
        let alpha = sin * FRAC_1_SQRT_2;
        let beta = 2.0 * a.sqrt() * alpha;
        if high {
            Self::normalise(
                a * ((a + 1.0) + (a - 1.0) * cos + beta),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                a * ((a + 1.0) + (a - 1.0) * cos - beta),
                (a + 1.0) - (a - 1.0) * cos + beta,
                2.0 * ((a - 1.0) - (a + 1.0) * cos),
                (a + 1.0) - (a - 1.0) * cos - beta,
            )
        } else {
            Self::normalise(
                a * ((a + 1.0) - (a - 1.0) * cos + beta),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                a * ((a + 1.0) - (a - 1.0) * cos - beta),
                (a + 1.0) + (a - 1.0) * cos + beta,
                -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                (a + 1.0) + (a - 1.0) * cos - beta,
            )
        }
    }
}

/// A biquad with one pair of delay registers per channel.
struct Biquad {
    c: Coefficients,
    z: Vec<[f64; 2]>,
}

impl Biquad {
    fn new(c: Coefficients, channels: usize) -> Self {
        Self {
            c,
            z: vec![[0.0; 2]; channels],
        }
    }

    fn tick(&mut self, channel: usize, x: f64) -> f64 {
        let c = self.c;
        let z = &mut self.z[channel];
        let y = c.b0 * x + z[0];
        z[0] = c.b1 * x - c.a1 * y + z[1];
        z[1] = c.b2 * x - c.a2 * y;
        y
    }

    fn save(&self, w: &mut Writer) {
        for z in &self.z {
            w.f64(z[0]);
            w.f64(z[1]);
        }
    }

    fn load(&mut self, r: &mut Reader<'_>) -> Result<(), String> {
        for z in &mut self.z {
            z[0] = r.f64()?;
            z[1] = r.f64()?;
        }
        Ok(())
    }
}

/// Compressor envelope: the smoothed gain reduction.
struct Dynamics {
    /// Current gain reduction in dB (positive).
    reduction: f64,
}

/// Noise-gate core.
struct GateCore {
    threshold_db: f64,
    hold_samples: f64,
    floor: f64,
    attack: f64,
    release: f64,
    detector_decay: f64,
    detector: f64,
    gain: f64,
    hold: f64,
}

impl GateCore {
    fn new(
        threshold_db: f64,
        attack_ms: f64,
        hold_ms: f64,
        release_ms: f64,
        range_db: f64,
        rate: f64,
    ) -> Self {
        Self {
            threshold_db,
            hold_samples: (hold_ms * 0.001 * rate).round(),
            floor: db_to_lin(-range_db),
            attack: time_coefficient(attack_ms, rate),
            release: time_coefficient(release_ms, rate),
            detector_decay: time_coefficient(10.0, rate),
            detector: 0.0,
            // A fresh gate starts open, so a clip does not fade in from silence.
            gain: 1.0,
            hold: 0.0,
        }
    }

    /// Advance one frame and return the gain to apply to it.
    fn next(&mut self, level: f64) -> f64 {
        self.detector = level.max(self.detector * self.detector_decay);
        let target = if lin_to_db(self.detector) > self.threshold_db {
            self.hold = self.hold_samples;
            1.0
        } else if self.hold > 0.0 {
            self.hold -= 1.0;
            1.0
        } else {
            self.floor
        };
        let coefficient = if target > self.gain {
            self.attack
        } else {
            self.release
        };
        self.gain = coefficient * self.gain + (1.0 - coefficient) * target;
        self.gain
    }

    fn save(&self, w: &mut Writer) {
        w.f64(self.detector);
        w.f64(self.gain);
        w.f64(self.hold);
    }

    fn load(&mut self, r: &mut Reader<'_>) -> Result<(), String> {
        self.detector = r.f64()?;
        self.gain = r.f64()?;
        self.hold = r.f64()?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------
// Effects

/// One effect instance prepared for a block: parameters, sample rate and loaded state.
trait Processor {
    fn run(&mut self, samples: &mut [f32], channels: usize);
    fn save(&self, w: &mut Writer);
    fn load(&mut self, r: &mut Reader<'_>) -> Result<(), String>;
}

struct Gain(f64);

impl Processor for Gain {
    fn run(&mut self, samples: &mut [f32], _channels: usize) {
        for x in samples {
            *x = (f64::from(*x) * self.0) as f32;
        }
    }
    fn save(&self, _w: &mut Writer) {}
    fn load(&mut self, _r: &mut Reader<'_>) -> Result<(), String> {
        Ok(())
    }
}

struct Eq3 {
    bands: [Biquad; 3],
}

impl Processor for Eq3 {
    fn run(&mut self, samples: &mut [f32], channels: usize) {
        for frame in samples.chunks_exact_mut(channels) {
            for (channel, x) in frame.iter_mut().enumerate() {
                let mut v = f64::from(*x);
                for band in &mut self.bands {
                    v = band.tick(channel, v);
                }
                *x = v as f32;
            }
        }
    }
    fn save(&self, w: &mut Writer) {
        for band in &self.bands {
            band.save(w);
        }
    }
    fn load(&mut self, r: &mut Reader<'_>) -> Result<(), String> {
        for band in &mut self.bands {
            band.load(r)?;
        }
        Ok(())
    }
}

struct Compressor {
    dynamics: Dynamics,
    threshold_db: f64,
    slope: f64,
    attack: f64,
    release: f64,
    makeup_db: f64,
}

impl Processor for Compressor {
    fn run(&mut self, samples: &mut [f32], channels: usize) {
        for frame in samples.chunks_exact_mut(channels) {
            let over = lin_to_db(peak(frame)) - self.threshold_db;
            let target = if over > 0.0 { over * self.slope } else { 0.0 };
            let coefficient = if target > self.dynamics.reduction {
                self.attack
            } else {
                self.release
            };
            self.dynamics.reduction =
                coefficient * self.dynamics.reduction + (1.0 - coefficient) * target;
            let gain = db_to_lin(self.makeup_db - self.dynamics.reduction);
            for x in frame {
                *x = (f64::from(*x) * gain) as f32;
            }
        }
    }
    fn save(&self, w: &mut Writer) {
        w.f64(self.dynamics.reduction);
    }
    fn load(&mut self, r: &mut Reader<'_>) -> Result<(), String> {
        self.dynamics.reduction = r.f64()?;
        Ok(())
    }
}

struct Gate(GateCore);

impl Processor for Gate {
    fn run(&mut self, samples: &mut [f32], channels: usize) {
        for frame in samples.chunks_exact_mut(channels) {
            let gain = self.0.next(peak(frame));
            for x in frame {
                *x = (f64::from(*x) * gain) as f32;
            }
        }
    }
    fn save(&self, w: &mut Writer) {
        self.0.save(w);
    }
    fn load(&mut self, r: &mut Reader<'_>) -> Result<(), String> {
        self.0.load(r)
    }
}

struct HighPass(Biquad);

impl Processor for HighPass {
    fn run(&mut self, samples: &mut [f32], channels: usize) {
        for frame in samples.chunks_exact_mut(channels) {
            for (channel, x) in frame.iter_mut().enumerate() {
                *x = self.0.tick(channel, f64::from(*x)) as f32;
            }
        }
    }
    fn save(&self, w: &mut Writer) {
        self.0.save(w);
    }
    fn load(&mut self, r: &mut Reader<'_>) -> Result<(), String> {
        self.0.load(r)
    }
}

/// High-pass, then a gentle (12 dB range) gate, then a split-band de-esser. The de-esser
/// splits the signal with a Linkwitz-Riley (4th-order) crossover, whose low and high bands
/// sum back to a flat magnitude, and attenuates only the high band while its envelope is
/// above the threshold.
struct VoiceCleanup {
    highpass: Biquad,
    gate: GateCore,
    low: [Biquad; 2],
    high: [Biquad; 2],
    envelope: f64,
    envelope_attack: f64,
    envelope_release: f64,
    reduction: f64,
    reduction_attack: f64,
    reduction_release: f64,
    threshold_db: f64,
    amount: f64,
}

impl Processor for VoiceCleanup {
    fn run(&mut self, samples: &mut [f32], channels: usize) {
        let mut low = [0.0_f64; MAX_CHANNELS];
        let mut high = [0.0_f64; MAX_CHANNELS];
        for frame in samples.chunks_exact_mut(channels) {
            for (channel, x) in frame.iter_mut().enumerate() {
                *x = self.highpass.tick(channel, f64::from(*x)) as f32;
            }
            let gate = self.gate.next(peak(frame));
            let mut sibilant = 0.0_f64;
            for (channel, x) in frame.iter().enumerate() {
                let gated = f64::from(*x) * gate;
                let [low_a, low_b] = &mut self.low;
                let [high_a, high_b] = &mut self.high;
                low[channel] = low_b.tick(channel, low_a.tick(channel, gated));
                high[channel] = high_b.tick(channel, high_a.tick(channel, gated));
                sibilant = sibilant.max(high[channel].abs());
            }
            let coefficient = if sibilant > self.envelope {
                self.envelope_attack
            } else {
                self.envelope_release
            };
            self.envelope = coefficient * self.envelope + (1.0 - coefficient) * sibilant;
            let over = lin_to_db(self.envelope) - self.threshold_db;
            let target = if over > 0.0 { over * self.amount } else { 0.0 };
            let coefficient = if target > self.reduction {
                self.reduction_attack
            } else {
                self.reduction_release
            };
            self.reduction = coefficient * self.reduction + (1.0 - coefficient) * target;
            let band_gain = db_to_lin(-self.reduction);
            for (channel, x) in frame.iter_mut().enumerate() {
                *x = (low[channel] + high[channel] * band_gain) as f32;
            }
        }
    }
    fn save(&self, w: &mut Writer) {
        self.highpass.save(w);
        self.gate.save(w);
        for filter in self.low.iter().chain(&self.high) {
            filter.save(w);
        }
        w.f64(self.envelope);
        w.f64(self.reduction);
    }
    fn load(&mut self, r: &mut Reader<'_>) -> Result<(), String> {
        self.highpass.load(r)?;
        self.gate.load(r)?;
        for filter in self.low.iter_mut().chain(&mut self.high) {
            filter.load(r)?;
        }
        self.envelope = r.f64()?;
        self.reduction = r.f64()?;
        Ok(())
    }
}

fn build(effect: &Resolved<'_>, rate: f64, channels: usize) -> Box<dyn Processor> {
    let v = |id| effect.get(id);
    match effect.def.id {
        "gain" => Box::new(Gain(db_to_lin(v("gain_db")))),
        "eq3" => Box::new(Eq3 {
            bands: [
                Biquad::new(
                    Coefficients::shelf(v("low_freq"), v("low_gain_db"), rate, false),
                    channels,
                ),
                Biquad::new(
                    Coefficients::peaking(v("mid_freq"), v("mid_q"), v("mid_gain_db"), rate),
                    channels,
                ),
                Biquad::new(
                    Coefficients::shelf(v("high_freq"), v("high_gain_db"), rate, true),
                    channels,
                ),
            ],
        }),
        "compressor" => Box::new(Compressor {
            dynamics: Dynamics { reduction: 0.0 },
            threshold_db: v("threshold_db"),
            slope: 1.0 - 1.0 / v("ratio"),
            attack: time_coefficient(v("attack_ms"), rate),
            release: time_coefficient(v("release_ms"), rate),
            makeup_db: v("makeup_db"),
        }),
        "gate" => Box::new(Gate(GateCore::new(
            v("threshold_db"),
            v("attack_ms"),
            v("hold_ms"),
            v("release_ms"),
            v("range_db"),
            rate,
        ))),
        "highpass" => Box::new(HighPass(Biquad::new(
            Coefficients::highpass(v("frequency"), FRAC_1_SQRT_2, rate),
            channels,
        ))),
        "voice_cleanup" => Box::new(VoiceCleanup {
            highpass: Biquad::new(
                Coefficients::highpass(v("highpass_hz"), FRAC_1_SQRT_2, rate),
                channels,
            ),
            gate: GateCore::new(v("gate_threshold_db"), 5.0, 80.0, 200.0, 12.0, rate),
            low: [0, 1].map(|_| {
                Biquad::new(
                    Coefficients::lowpass(v("deess_freq"), FRAC_1_SQRT_2, rate),
                    channels,
                )
            }),
            high: [0, 1].map(|_| {
                Biquad::new(
                    Coefficients::highpass(v("deess_freq"), FRAC_1_SQRT_2, rate),
                    channels,
                )
            }),
            envelope: 0.0,
            envelope_attack: time_coefficient(0.5, rate),
            envelope_release: time_coefficient(40.0, rate),
            reduction: 0.0,
            reduction_attack: time_coefficient(1.0, rate),
            reduction_release: time_coefficient(30.0, rate),
            threshold_db: v("deess_threshold_db"),
            amount: v("deess_amount"),
        }),
        other => unreachable!("effect {other} is in EFFECTS"),
    }
}

/// Process one interleaved block in place and return the state for the next block.
///
/// `state` is `None` for the first block of a signal. Parameters outside an effect's declared
/// range are clamped (so keyframe overshoot never fails playback); unknown or non-finite
/// parameters are errors.
pub fn process(
    effect: &str,
    params: &[(String, f64)],
    sample_rate: u32,
    channels: usize,
    samples: &mut [f32],
    state: Option<&[u8]>,
) -> Result<Option<Vec<u8>>, String> {
    let def = EFFECTS
        .iter()
        .find(|def| def.id == effect)
        .ok_or_else(|| format!("unknown audio effect {effect:?}"))?;
    if channels == 0 || channels > MAX_CHANNELS {
        return Err("channel count must be within 1..=32".to_owned());
    }
    if sample_rate == 0 || samples.len() % channels != 0 {
        return Err("block must hold whole frames at a nonzero sample rate".to_owned());
    }
    let resolved = resolve(def, params)?;
    let mut processor = build(&resolved, f64::from(sample_rate), channels);
    if let Some(bytes) = state {
        let mut reader = Reader::new(bytes, channels)?;
        processor.load(&mut reader)?;
        reader.finish()?;
    }
    processor.run(samples, channels);
    let mut writer = Writer::new(channels);
    processor.save(&mut writer);
    // Stateless effects (gain) return no state.
    Ok((writer.0.len() > 2).then_some(writer.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn sine(freq: f64, amplitude: f64, frames: usize, channels: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|n| {
                let v = (amplitude * (2.0 * PI * freq * n as f64 / f64::from(RATE)).sin()) as f32;
                std::iter::repeat_n(v, channels)
            })
            .collect()
    }

    fn rms(samples: &[f32], skip_frames: usize, channels: usize) -> f64 {
        let tail = &samples[skip_frames * channels..];
        (tail.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / tail.len() as f64).sqrt()
    }

    fn run(effect: &str, params: &[(&str, f64)], input: &[f32], channels: usize) -> Vec<f32> {
        let params: Vec<_> = params.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect();
        let mut out = input.to_vec();
        process(effect, &params, RATE, channels, &mut out, None).unwrap();
        out
    }

    fn db(ratio: f64) -> f64 {
        20.0 * ratio.log10()
    }

    #[test]
    fn gain_scales_by_decibels() {
        let input = sine(440.0, 0.25, 4800, 2);
        let out = run("gain", &[("gain_db", 6.0)], &input, 2);
        let measured = db(rms(&out, 0, 2) / rms(&input, 0, 2));
        assert!((measured - 6.0).abs() < 1e-4, "{measured}");
        assert_eq!(run("gain", &[("gain_db", 0.0)], &input, 2), input);
    }

    #[test]
    fn eq_shelves_and_peak_hit_their_gain_and_leave_the_rest_alone() {
        let frames = 24_000;
        let boost = |params: &[(&str, f64)], freq: f64| {
            let input = sine(freq, 0.1, frames, 1);
            let out = run("eq3", params, &input, 1);
            db(rms(&out, 8000, 1) / rms(&input, 8000, 1))
        };
        // Low shelf: +9 dB deep in the bass, flat in the treble.
        let low = [("low_gain_db", 9.0), ("low_freq", 200.0)];
        assert!(
            (boost(&low, 30.0) - 9.0).abs() < 0.3,
            "{}",
            boost(&low, 30.0)
        );
        assert!(boost(&low, 8000.0).abs() < 0.3);
        // Mid peak: -12 dB at the center, untouched far away.
        let mid = [("mid_gain_db", -12.0), ("mid_freq", 1000.0), ("mid_q", 2.0)];
        assert!(
            (boost(&mid, 1000.0) + 12.0).abs() < 0.2,
            "{}",
            boost(&mid, 1000.0)
        );
        assert!(boost(&mid, 100.0).abs() < 0.8);
        // High shelf: +6 dB in the treble, flat in the bass.
        let high = [("high_gain_db", 6.0), ("high_freq", 5000.0)];
        assert!(
            (boost(&high, 16_000.0) - 6.0).abs() < 0.4,
            "{}",
            boost(&high, 16_000.0)
        );
        assert!(boost(&high, 100.0).abs() < 0.2);
        // All gains at zero is an exact identity.
        let input = sine(1234.0, 0.5, 2000, 2);
        assert_eq!(run("eq3", &[], &input, 2), input);
    }

    #[test]
    fn highpass_blocks_bass_and_passes_treble() {
        let frames = 24_000;
        let level = |freq: f64| {
            let input = sine(freq, 0.5, frames, 1);
            let out = run("highpass", &[("frequency", 200.0)], &input, 1);
            db(rms(&out, 12_000, 1) / rms(&input, 12_000, 1))
        };
        assert!(level(20.0) < -35.0, "{}", level(20.0));
        assert!((level(200.0) + 3.0).abs() < 0.3, "{}", level(200.0));
        assert!(level(4000.0).abs() < 0.1);
    }

    #[test]
    fn compressor_reduces_by_the_ratio_and_applies_makeup() {
        let params = [
            ("threshold_db", -20.0),
            ("ratio", 4.0),
            ("attack_ms", 0.1),
            ("release_ms", 5.0),
        ];
        // 0 dBFS peak is 20 dB over the threshold: 15 dB of reduction at 4:1.
        let input = vec![1.0_f32; 24_000];
        let out = run("compressor", &params, &input, 1);
        let settled = db(f64::from(out[23_999]));
        assert!((settled + 15.0).abs() < 0.1, "{settled}");
        // Below the threshold only the makeup gain applies.
        let quiet = vec![0.01_f32; 4800];
        let mut with_makeup = params.to_vec();
        with_makeup.push(("makeup_db", 6.0));
        let out = run("compressor", &with_makeup, &quiet, 1);
        assert!((db(f64::from(out[4799]) / 0.01) - 6.0).abs() < 1e-3);
    }

    #[test]
    fn compressor_attack_and_release_are_gradual() {
        let params = [
            ("threshold_db", -20.0),
            ("ratio", 8.0),
            ("attack_ms", 20.0),
            ("release_ms", 200.0),
        ];
        let mut input = vec![0.01_f32; 4800];
        input.extend(vec![1.0_f32; 4800]);
        input.extend(vec![0.01_f32; 9600]);
        let out = run("compressor", &params, &input, 1);
        // Right after the burst starts the gain has not yet reacted.
        assert!(out[4800] > 0.9);
        // ...and it keeps pulling down toward the settled value.
        assert!(out[9500] < out[4800] * 0.5);
        // After the burst the quiet tail is still held down, then recovers.
        assert!(out[9700] < 0.0099);
        assert!(out[14_399] > out[9700]);
    }

    #[test]
    fn gate_mutes_quiet_signal_and_passes_loud_signal() {
        let params = [
            ("threshold_db", -30.0),
            ("attack_ms", 0.5),
            ("hold_ms", 10.0),
            ("release_ms", 20.0),
            ("range_db", 60.0),
        ];
        let mut input = sine(440.0, 0.5, 4800, 1);
        input.extend(sine(440.0, 0.002, 9600, 1));
        let out = run("gate", &params, &input, 1);
        // Loud part passes (the gate starts open).
        let passed = rms(&out[..4800], 0, 1) / rms(&input[..4800], 0, 1);
        assert!((passed - 1.0).abs() < 1e-3, "{passed}");
        // Quiet part is attenuated by (nearly) the full 60 dB once released.
        let attenuation = db(rms(&out[12_000..], 0, 1) / rms(&input[12_000..], 0, 1));
        assert!(attenuation < -50.0, "{attenuation}");
        // Hold keeps the gate open right after the loud part ends.
        assert!(out[4800 + 100].abs() > 0.0015 || input[4800 + 100].abs() < 0.0015);
    }

    #[test]
    fn voice_cleanup_removes_rumble_and_tames_sibilance_but_keeps_speech_band() {
        let frames = 24_000;
        let level = |input: &[f32]| {
            let out = run("voice_cleanup", &[], input, 1);
            db(rms(&out, 12_000, 1) / rms(input, 12_000, 1))
        };
        // Rumble below the high-pass is removed.
        assert!(level(&sine(25.0, 0.3, frames, 1)) < -20.0);
        // The speech band is left alone.
        assert!(level(&sine(500.0, 0.3, frames, 1)).abs() < 0.5);
        // Loud sibilance is attenuated; quiet sibilance (below the threshold) is not.
        let sib = level(&sine(8000.0, 0.3, frames, 1));
        assert!(sib < -3.0, "{sib}");
        assert!(level(&sine(8000.0, 0.01, frames, 1)).abs() < 0.5);
    }

    #[test]
    fn blocks_of_any_size_match_one_big_block() {
        let channels = 2;
        let input = sine(220.0, 0.8, 9000, channels)
            .iter()
            .zip(sine(7000.0, 0.3, 9000, channels))
            .map(|(a, b)| a + b)
            .collect::<Vec<f32>>();
        for def in EFFECTS {
            let params: Vec<(String, f64)> = Vec::new();
            let mut whole = input.clone();
            process(def.id, &params, RATE, channels, &mut whole, None).unwrap();
            let mut pieces = Vec::new();
            let mut state: Option<Vec<u8>> = None;
            for chunk in input.chunks(1000 * channels) {
                let mut block = chunk.to_vec();
                state = process(
                    def.id,
                    &params,
                    RATE,
                    channels,
                    &mut block,
                    state.as_deref(),
                )
                .unwrap();
                pieces.extend(block);
            }
            assert_eq!(whole, pieces, "{} is not block-size independent", def.id);
        }
    }

    #[test]
    fn processing_is_deterministic_and_rejects_bad_input() {
        let input = sine(330.0, 0.5, 1000, 2);
        assert_eq!(
            run("compressor", &[], &input, 2),
            run("compressor", &[], &input, 2)
        );
        let mut block = input.clone();
        let bad = [("nope".to_owned(), 1.0)];
        assert!(process("gain", &bad, RATE, 2, &mut block, None).is_err());
        let nan = [("gain_db".to_owned(), f64::NAN)];
        assert!(process("gain", &nan, RATE, 2, &mut block, None).is_err());
        assert!(process("missing", &[], RATE, 2, &mut block, None).is_err());
        assert!(process("gain", &[], RATE, 3, &mut block, None).is_err());
        // State from another channel layout is refused.
        let state = process("highpass", &[], RATE, 2, &mut block, None).unwrap();
        let mut mono = vec![0.0_f32; 10];
        assert!(process("highpass", &[], RATE, 1, &mut mono, state.as_deref()).is_err());
        // Out-of-range values are clamped, not rejected.
        let loud = [("gain_db".to_owned(), 1000.0)];
        let mut one = vec![0.001_f32];
        process("gain", &loud, RATE, 1, &mut one, None).unwrap();
        assert!((db(f64::from(one[0]) / 0.001) - 24.0).abs() < 1e-4);
    }
}
