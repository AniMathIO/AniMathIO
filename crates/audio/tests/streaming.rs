//! Streaming decode, bounded block preparation and channel layouts.
//!
//! The reference for "identical to the full-decode output" is a verbatim port of the previous
//! implementation (decode everything, resample the whole asset front to back) kept below.
#![allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::similar_names
)]
use animathio_audio::{
    AudioPool, AudioSource, DecodeLimits, Error, Mixer, Pcm, PcmReader, PrepareOptions, StreamInfo,
    SymphoniaSource, Waveform,
};
use animathio_core::*;
use rubato::{
    FftFixedInOut, Resampler, SincFixedIn, SincFixedOut, SincInterpolationParameters,
    SincInterpolationType, WindowFunction,
};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

fn range(start: i64, end: i64) -> TimeRange {
    TimeRange::new(Time(start), Time(end)).unwrap()
}
fn seconds(s: f64) -> i64 {
    (s * TICKS_PER_SECOND as f64) as i64
}
fn audio_project(id: &AssetId, end: i64) -> Project {
    let mut project = Project::default();
    Command::AddAsset {
        id: id.clone(),
        asset: Asset {
            name: "a".into(),
            media_type: "audio/wav".into(),
        },
    }
    .apply(&mut project)
    .unwrap();
    Command::AddTrack {
        track: Track::new(TrackId(0), "audio"),
        index: 0,
    }
    .apply(&mut project)
    .unwrap();
    Command::AddClip {
        track: TrackId(0),
        clip: Clip::new(ClipId(0), ClipKind::Audio(id.clone()), range(0, end)),
    }
    .apply(&mut project)
    .unwrap();
    project
}
fn bits(samples: &[f32]) -> Vec<u32> {
    samples.iter().map(|s| s.to_bits()).collect()
}
/// Deterministic broadband test signal.
fn noise(rate: u32, channels: u16, frames: usize) -> Pcm {
    let mut state = 0x1234_5678_u32;
    let samples = (0..frames * usize::from(channels))
        .map(|i| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let tone = (i as f32 * 0.01).sin() * 0.4;
            tone + ((state >> 16) as f32 / 65_536.0 - 0.5) * 0.2
        })
        .collect();
    Pcm::new(rate, channels, samples).unwrap()
}

// ---------------------------------------------------------------------------------------------
// Reference: the previous whole-asset implementation, ported verbatim.
// ---------------------------------------------------------------------------------------------
enum LegacyResampler {
    Fft(FftFixedInOut<f32>),
    SincIn(SincFixedIn<f32>),
    SincOut(SincFixedOut<f32>),
}
impl LegacyResampler {
    fn input_frames_next(&self) -> usize {
        match self {
            Self::Fft(r) => r.input_frames_next(),
            Self::SincIn(r) => r.input_frames_next(),
            Self::SincOut(r) => r.input_frames_next(),
        }
    }
    fn process(&mut self, input: &[Vec<f32>]) -> Vec<Vec<f32>> {
        match self {
            Self::Fft(r) => r.process(input, None).unwrap(),
            Self::SincIn(r) => r.process(input, None).unwrap(),
            Self::SincOut(r) => r.process(input, None).unwrap(),
        }
    }
}
fn legacy_resampler(
    input_rate: u32,
    output_rate: u32,
    channels: usize,
) -> (LegacyResampler, usize) {
    let (mut a, mut b) = (input_rate, output_rate);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    let period = (input_rate / a) as usize;
    let chunk_in = 1024_usize.div_ceil(2 * period) * 2 * period;
    let fft_chunks = chunk_in.div_ceil(period);
    let fft_in = fft_chunks * period;
    let fft_out = fft_chunks * (output_rate / a) as usize;
    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        oversampling_factor: 256,
        interpolation: SincInterpolationType::Linear,
        window: WindowFunction::BlackmanHarris2,
    };
    if fft_in <= 16_384 && fft_out <= 16_384 {
        let r = FftFixedInOut::<f32>::new(
            input_rate as usize,
            output_rate as usize,
            chunk_in,
            channels,
        )
        .unwrap();
        let delay = r.output_delay();
        (LegacyResampler::Fft(r), delay)
    } else if output_rate < input_rate {
        let chunk = 4096.max(input_rate.div_ceil(output_rate) as usize + 257);
        let r = SincFixedIn::<f32>::new(
            f64::from(output_rate) / f64::from(input_rate),
            1.0,
            params,
            chunk,
            channels,
        )
        .unwrap();
        let delay = r.output_delay();
        (LegacyResampler::SincIn(r), delay)
    } else {
        let r = SincFixedOut::<f32>::new(
            f64::from(output_rate) / f64::from(input_rate),
            1.0,
            params,
            4096,
            channels,
        )
        .unwrap();
        let delay = r.output_delay();
        (LegacyResampler::SincOut(r), delay)
    }
}
/// Interleaved output of resampling the whole asset front to back (equal layouts, mono
/// duplication and the stereo mean only, which is all the old code supported for these cases).
fn legacy_prepare(pcm: &Pcm, rate: u32, channels: u16) -> Vec<f32> {
    let native = usize::from(pcm.channels());
    let channels = usize::from(channels);
    let mut planar: Vec<Vec<f32>> = (0..native)
        .map(|c| {
            pcm.samples()
                .iter()
                .skip(c)
                .step_by(native)
                .copied()
                .collect()
        })
        .collect();
    if rate != pcm.rate() && pcm.frames() != 0 {
        let wanted =
            (pcm.frames() as u128 * u128::from(rate)).div_ceil(u128::from(pcm.rate())) as usize;
        let (mut resampler, delay) = legacy_resampler(pcm.rate(), rate, native);
        let target = wanted + delay;
        let mut result = vec![Vec::new(); native];
        let mut offset = 0;
        while result[0].len() < target {
            let chunk = resampler.input_frames_next();
            let mut input = vec![vec![0.0; chunk]; native];
            let count = pcm.frames().saturating_sub(offset).min(chunk);
            for (channel, input) in input.iter_mut().enumerate() {
                if count != 0 {
                    input[..count].copy_from_slice(&planar[channel][offset..offset + count]);
                }
            }
            for (out, data) in result.iter_mut().zip(resampler.process(&input)) {
                out.extend(data);
            }
            offset += chunk;
        }
        planar = result
            .into_iter()
            .map(|data| data[delay..delay + wanted].to_vec())
            .collect();
    }
    let mut output = Vec::new();
    for frame in 0..planar[0].len() {
        for channel in 0..channels {
            output.push(if native == channels {
                planar[channel][frame]
            } else if native == 1 {
                planar[0][frame]
            } else {
                planar.iter().map(|c| c[frame]).sum::<f32>() / native as f32
            });
        }
    }
    output
}
/// The previous eager Symphonia decode loop (gapless on, default audio track).
fn legacy_decode(bytes: &[u8]) -> (u32, u16, Vec<f32>) {
    use symphonia::core::{
        audio::SampleBuffer,
        codecs::DecoderOptions,
        errors::Error as E,
        formats::FormatOptions,
        io::{MediaSourceStream, MediaSourceStreamOptions},
        meta::MetadataOptions,
        probe::Hint,
    };
    let stream = MediaSourceStream::new(
        Box::new(std::io::Cursor::new(bytes.to_vec())),
        MediaSourceStreamOptions::default(),
    );
    let mut format = symphonia::default::get_probe()
        .format(
            &Hint::new(),
            stream,
            &FormatOptions {
                enable_gapless: true,
                ..FormatOptions::default()
            },
            &MetadataOptions::default(),
        )
        .unwrap()
        .format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.sample_rate.is_some())
        .or_else(|| format.default_track())
        .unwrap();
    let id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .unwrap();
    let (mut samples, mut spec) = (Vec::new(), None);
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(E::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => panic!("{e}"),
        };
        if packet.track_id() != id {
            continue;
        }
        let audio = decoder.decode(&packet).unwrap();
        spec = Some(*audio.spec());
        let mut buffer = SampleBuffer::<f32>::new(audio.capacity() as u64, *audio.spec());
        buffer.copy_interleaved_ref(audio);
        samples.extend_from_slice(buffer.samples());
    }
    let spec = spec.unwrap();
    (spec.rate, spec.channels.count() as u16, samples)
}

fn lcg(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1);
    *state >> 33
}

#[test]
fn blocks_match_whole_asset_resampling_bit_exactly_for_any_range_and_block_size() {
    // (input rate, output rate, input channels, output channels)
    let cases = [
        (44_100, 48_000, 2, 2),
        (48_000, 44_100, 2, 1),
        (22_050, 48_000, 1, 2),
        (96_000, 48_000, 2, 2),
        (8_000, 48_000, 1, 1),
        (44_100, 44_100, 2, 2),
        (48_000, 48_000, 1, 2),
    ];
    for (in_rate, out_rate, in_ch, out_ch) in cases {
        let pcm = noise(in_rate, in_ch, in_rate as usize * 3 / 2);
        let id = AssetId::of(b"noise");
        let reference = legacy_prepare(&pcm, out_rate, out_ch);
        let reference_pcm = Pcm::new(out_rate, out_ch, reference).unwrap();
        let end = seconds(1.6);
        let project = audio_project(&id, end);
        let mut oracle = AudioPool::default();
        oracle.insert(id.clone(), &reference_pcm).unwrap();
        let oracle = oracle.prepare(out_rate, out_ch).unwrap();
        let mut pool = AudioPool::default();
        pool.insert(id.clone(), &pcm).unwrap();
        for block_frames in [64, 700, 4096, 1 << 17] {
            let mixer = pool
                .prepare_with(
                    out_rate,
                    out_ch,
                    PrepareOptions {
                        block_frames,
                        cache_bytes: 1 << 20,
                    },
                )
                .unwrap();
            let mut state = 7 + u64::from(in_rate);
            let mut ranges = vec![(0, end)];
            for _ in 0..6 {
                let a = lcg(&mut state) as i64 % (end - 1);
                let b = a + 1 + lcg(&mut state) as i64 % seconds(0.4);
                ranges.push((a, b.min(end)));
            }
            for (a, b) in ranges {
                assert_eq!(
                    bits(&mixer.mix(&project, range(a, b)).unwrap()),
                    bits(&oracle.mix(&project, range(a, b)).unwrap()),
                    "{in_rate}->{out_rate} {in_ch}->{out_ch} block {block_frames} range {a}..{b}"
                );
            }
        }
    }
}

#[test]
fn sinc_fallback_is_exact_forward_backward_and_after_jumps() {
    // 44_101 -> 48_000 has no small FFT period, so it takes the sequential sinc path.
    let pcm = noise(44_101, 2, 44_101);
    let id = AssetId::of(b"sinc");
    let reference = Pcm::new(48_000, 2, legacy_prepare(&pcm, 48_000, 2)).unwrap();
    let end = seconds(1.0);
    let project = audio_project(&id, end);
    let mut oracle = AudioPool::default();
    oracle.insert(id.clone(), &reference).unwrap();
    let oracle = oracle.prepare(48_000, 2).unwrap();
    let mut pool = AudioPool::default();
    pool.insert(id, &pcm).unwrap();
    let mixer = pool
        .prepare_with(
            48_000,
            2,
            PrepareOptions {
                block_frames: 5000,
                cache_bytes: 80_000,
            },
        )
        .unwrap();
    for (a, b) in [
        (seconds(0.0), seconds(0.2)),
        (seconds(0.2), seconds(0.5)),
        (seconds(0.8), seconds(0.95)),
        (seconds(0.1), seconds(0.3)), // backwards: replays from the start
        (seconds(0.0), seconds(1.0)),
    ] {
        assert_eq!(
            bits(&mixer.mix(&project, range(a, b)).unwrap()),
            bits(&oracle.mix(&project, range(a, b)).unwrap()),
            "range {a}..{b}"
        );
    }
}

#[test]
fn cache_stays_within_budget_and_eviction_never_changes_output() {
    let pcm = noise(44_100, 2, 44_100 * 4);
    let id = AssetId::of(b"cache");
    let end = seconds(4.0);
    let project = audio_project(&id, end);
    let mut pool = AudioPool::default();
    pool.insert(id, &pcm).unwrap();
    let unbounded = pool.prepare(48_000, 2).unwrap();
    let block_frames = 3840; // three 1280-frame FFT chunks
    let budget = 2 * block_frames * 2 * 4;
    let tight = pool
        .prepare_with(
            48_000,
            2,
            PrepareOptions {
                block_frames,
                cache_bytes: budget,
            },
        )
        .unwrap();
    let mut state = 99;
    for _ in 0..24 {
        let a = lcg(&mut state) as i64 % (end - 1);
        let b = (a + 1 + lcg(&mut state) as i64 % seconds(0.5)).min(end);
        assert_eq!(
            bits(&tight.mix(&project, range(a, b)).unwrap()),
            bits(&unbounded.mix(&project, range(a, b)).unwrap())
        );
        assert!(tight.prepared_cache_bytes() <= budget);
    }
    assert!(tight.prepared_cache_bytes() > 0);
}

// ---------------------------------------------------------------------------------------------
// Real codecs
// ---------------------------------------------------------------------------------------------
fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

#[test]
fn eager_decode_equals_the_previous_implementation() {
    for name in [
        "tone.wav",
        "tone.flac",
        "tone.mp3",
        "tone.aac",
        "tone.m4a",
        "tone.ogg",
        "long.flac",
        "long.mp3",
        "long.m4a",
        "long.ogg",
    ] {
        let bytes = fixture(name);
        let (rate, channels, samples) = legacy_decode(&bytes);
        let pcm = SymphoniaSource::from_bytes(bytes)
            .unwrap()
            .decode_all()
            .unwrap();
        assert_eq!((pcm.rate(), pcm.channels()), (rate, channels), "{name}");
        assert_eq!(bits(pcm.samples()), bits(&samples), "{name}");
    }
}

/// Largest allowed difference between a front-to-back decode and one that restarts mid-stream
/// at a segment boundary. Codecs without long inter-frame state are bit exact; AAC differs in
/// its randomly substituted noise bands and Opus until its energy prediction has converged.
fn tolerance(name: &str) -> f32 {
    if name.contains("m4a") || name.contains("aac") {
        2e-2
    } else if name.contains("opus") || name.contains("mka") || name.contains("mp4") {
        5e-4
    } else {
        0.0
    }
}

#[test]
fn restarting_in_any_segment_reproduces_the_continuous_decode() {
    for name in [
        "tone.wav",
        "tone.flac",
        "tone.mp3",
        "tone.m4a",
        "tone.ogg",
        "tone.opus",
        "long.flac",
        "long.mp3",
        "long.m4a",
        "long.ogg",
        "long.opus",
        "opus_stereo.mka",
        "opus_stereo.mp4",
    ] {
        // Segments of 32768 frames give several restart points in the longer fixtures.
        let source = SymphoniaSource::from_bytes_with_segments(
            fixture(name),
            DecodeLimits::default(),
            32_768,
        )
        .unwrap();
        let all = source.decode_all().unwrap();
        assert_eq!(source.info().frames, all.frames() as u64, "{name}");
        let channels = usize::from(all.channels());
        let total = all.frames() as u64;
        let mut reader = source.open_reader().unwrap();
        let tolerance = tolerance(name);
        let mut state = 5;
        let mut requests = vec![
            (0, 100),
            (1, 10),
            (777, 1000),
            (total.saturating_sub(300), 1000),
        ];
        requests.extend([(30_000, 5_000), (65_536, 70_000), (0, 1_000_000)]);
        for _ in 0..12 {
            requests.push((
                lcg(&mut state) % total.max(1),
                1 + lcg(&mut state) as usize % 40_000,
            ));
        }
        for (start, len) in requests {
            let mut out = Vec::new();
            let got = reader.read_at(start, len, &mut out).unwrap();
            let from = (start as usize).min(all.frames());
            let to = (start as usize + len).min(all.frames());
            let expected = &all.samples()[from * channels..to * channels];
            assert_eq!(got, to - from, "{name} {start}+{len}");
            assert_eq!(out.len(), expected.len(), "{name} {start}+{len}");
            let worst = out
                .iter()
                .zip(expected)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f32::max);
            assert!(worst <= tolerance, "{name} {start}+{len}: {worst}");
        }
        // The answer for a given range never depends on what was read before it.
        let mut fresh = source.open_reader().unwrap();
        let (mut a, mut b) = (Vec::new(), Vec::new());
        reader.read_at(40_000, 9_000, &mut a).unwrap();
        fresh.read_at(40_000, 9_000, &mut b).unwrap();
        assert_eq!(bits(&a), bits(&b), "{name} history independent");
    }
}

#[test]
fn opus_decodes_in_ogg_matroska_and_mp4_with_pre_skip_removed() {
    // `ffmpeg -c:a libopus`: 80 ms and 2.5 s (Ogg), 1 s (Matroska, MP4). The Ogg demuxer trims
    // the end padding when the file has more than one audio page (the 80 ms file does not, so
    // it keeps its 648 padding frames); Matroska/MP4 keep up to one packet of trailing padding.
    for (name, expected, channels) in [
        ("tone.opus", 4488, 1),
        ("long.opus", 120_000, 2),
        ("opus_stereo.mka", 48_648, 2),
        ("opus_stereo.mp4", 48_648, 2),
    ] {
        let pcm = SymphoniaSource::from_bytes(fixture(name))
            .unwrap()
            .decode_all()
            .unwrap();
        assert_eq!((pcm.rate(), pcm.channels()), (48_000, channels), "{name}");
        assert_eq!(pcm.frames(), expected, "{name}");
        assert!(pcm.samples().iter().any(|s| s.abs() > 0.05), "{name}");
    }
    // A pure tone: the first decoded frames are the tone, not the encoder's pre-skip ramp.
    let tone = SymphoniaSource::from_bytes(fixture("tone.opus"))
        .unwrap()
        .decode_all()
        .unwrap();
    let rms = |s: &[f32]| (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt();
    assert!(rms(&tone.samples()[..480]) > 0.5 * rms(&tone.samples()[480..960]));
}

#[test]
fn waveform_streams_to_the_same_peaks_as_the_decoded_asset() {
    for name in ["long.mp3", "long.flac", "tone.ogg"] {
        let source = SymphoniaSource::from_bytes(fixture(name)).unwrap();
        let eager = Waveform::from_pcm(&source.decode_all().unwrap(), 777);
        let mut pool = AudioPool::default();
        let id = AssetId::of(name.as_bytes());
        pool.insert(id.clone(), &source).unwrap();
        assert_eq!(pool.waveform(&id, 777).unwrap(), eager, "{name}");
    }
}

// ---------------------------------------------------------------------------------------------
// Channel layouts through the mixer
// ---------------------------------------------------------------------------------------------
fn mix_constant(channels: u16, frame: &[f32], out_channels: u16) -> Vec<f32> {
    let id = AssetId::of(b"layout");
    let mut samples = Vec::new();
    for _ in 0..4 {
        samples.extend_from_slice(frame);
    }
    let mut pool = AudioPool::default();
    pool.insert(id.clone(), &Pcm::new(48_000, channels, samples).unwrap())
        .unwrap();
    let project = audio_project(&id, seconds(4.0 / 48_000.0));
    let mixed = pool
        .mix(
            &project,
            range(0, seconds(4.0 / 48_000.0)),
            48_000,
            out_channels,
        )
        .unwrap();
    mixed[..usize::from(out_channels)].to_vec()
}

#[test]
fn surround_folds_down_to_stereo_through_the_mixer() {
    let h = std::f32::consts::FRAC_1_SQRT_2;
    let close = |a: Vec<f32>, b: &[f32]| {
        assert!(
            a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6),
            "{a:?} != {b:?}"
        );
    };
    // 5.1: L R C LFE BL BR
    close(
        mix_constant(6, &[0.1, 0.2, 0.4, 0.8, 0.3, 0.5], 2),
        &[0.1 + h * 0.4 + h * 0.3, 0.2 + h * 0.4 + h * 0.5],
    );
    // quad: L R BL BR
    close(
        mix_constant(4, &[0.1, 0.2, 0.3, 0.5], 2),
        &[0.1 + h * 0.3, 0.2 + h * 0.5],
    );
    // 7.1: L R C LFE BL BR SL SR (both rear pairs at -6 dB)
    close(
        mix_constant(8, &[0.1, 0.2, 0.4, 0.8, 0.3, 0.5, 0.6, 0.7], 2),
        &[
            0.1 + h * 0.4 + 0.5 * 0.3 + 0.5 * 0.6,
            0.2 + h * 0.4 + 0.5 * 0.5 + 0.5 * 0.7,
        ],
    );
    // mono duplicates, stereo averages to mono, 5.1 averages the fold-down to mono
    close(mix_constant(1, &[0.25], 2), &[0.25, 0.25]);
    close(mix_constant(2, &[0.25, 0.75], 1), &[0.5]);
    close(mix_constant(6, &[0.0, 0.0, 1.0, 0.0, 0.0, 0.0], 1), &[h]);
    // stereo to 5.1 lands on the front pair
    close(
        mix_constant(2, &[0.25, 0.75], 6),
        &[0.25, 0.75, 0.0, 0.0, 0.0, 0.0],
    );
}

// ---------------------------------------------------------------------------------------------
// Memory is independent of the length of the asset
// ---------------------------------------------------------------------------------------------
#[derive(Default)]
struct Stats {
    max_request: AtomicU64,
    served: AtomicU64,
}
/// A procedurally generated stereo asset that cannot be decoded in one piece.
struct Synthetic {
    rate: u32,
    frames: u64,
    stats: Arc<Stats>,
}
impl Synthetic {
    fn sample(frame: u64, channel: u64) -> f32 {
        ((frame * 7 + channel * 3) % 1000) as f32 / 4000.0
    }
}
impl AudioSource for Synthetic {
    fn decode_all(&self) -> Result<Pcm, Error> {
        panic!("a long asset must never be decoded in one piece");
    }
    fn open_reader(&self) -> Result<Box<dyn PcmReader>, Error> {
        Ok(Box::new(SyntheticReader {
            info: StreamInfo {
                rate: self.rate,
                channels: 2,
                frames: self.frames,
                layout: animathio_audio::ChannelLayout::default_for(2),
            },
            stats: Arc::clone(&self.stats),
        }))
    }
}
struct SyntheticReader {
    info: StreamInfo,
    stats: Arc<Stats>,
}
impl PcmReader for SyntheticReader {
    fn info(&self) -> &StreamInfo {
        &self.info
    }
    fn read_at(&mut self, start: u64, frames: usize, out: &mut Vec<f32>) -> Result<usize, Error> {
        let end = (start + frames as u64).min(self.info.frames);
        let count = end.saturating_sub(start);
        self.stats.max_request.fetch_max(count, Ordering::Relaxed);
        self.stats.served.fetch_add(count, Ordering::Relaxed);
        for frame in start..end {
            out.push(Synthetic::sample(frame, 0));
            out.push(Synthetic::sample(frame, 1));
        }
        Ok(count as usize)
    }
}

fn synthetic_pool(rate: u32, hours: u64) -> (AudioPool, AssetId, Arc<Stats>, Project) {
    let stats = Arc::new(Stats::default());
    let source = Synthetic {
        rate,
        frames: hours * 3600 * u64::from(rate),
        stats: Arc::clone(&stats),
    };
    let id = AssetId::of(b"hour");
    let mut pool = AudioPool::default();
    pool.insert(id.clone(), &source).unwrap();
    let project = audio_project(&id, seconds(hours as f64 * 3600.0));
    (pool, id, stats, project)
}

#[test]
fn one_hour_stereo_asset_mixes_in_bounded_memory() {
    let (pool, id, stats, project) = synthetic_pool(48_000, 1);
    assert_eq!(pool.info(&id).unwrap().frames, 3600 * 48_000);
    let budget = 8 * 1024 * 1024;
    let mixer: Mixer = pool
        .prepare_with(
            48_000,
            2,
            PrepareOptions {
                block_frames: 1 << 17,
                cache_bytes: budget,
            },
        )
        .unwrap();
    // Start, middle, a seek back, and the very end of the hour.
    for start_seconds in [0.0, 1800.0, 12.5, 3597.0] {
        let first = (start_seconds * 48_000.0) as u64;
        let mixed = mixer
            .mix(
                &project,
                range(seconds(start_seconds), seconds(start_seconds + 3.0)),
            )
            .unwrap();
        assert_eq!(mixed.len(), 3 * 48_000 * 2);
        for frame in [0_u64, 1, 4_321, 100_000, 3 * 48_000 - 1] {
            assert_eq!(
                mixed[frame as usize * 2],
                Synthetic::sample(first + frame, 0)
            );
            assert_eq!(
                mixed[frame as usize * 2 + 1],
                Synthetic::sample(first + frame, 1)
            );
        }
        assert!(mixer.prepared_cache_bytes() <= budget);
    }
    // Only the touched blocks were read, in block-sized requests.
    assert!(stats.max_request.load(Ordering::Relaxed) <= 1 << 17);
    assert!(stats.served.load(Ordering::Relaxed) < 12 * 48_000 + 8 * (1 << 17));
}

#[test]
fn one_hour_resampled_asset_reads_only_what_it_needs() {
    let (pool, _, stats, project) = synthetic_pool(44_100, 1);
    let mixer = pool.prepare(48_000, 2).unwrap();
    let mixed = mixer
        .mix(&project, range(seconds(1800.0), seconds(1801.0)))
        .unwrap();
    assert_eq!(mixed.len(), 48_000 * 2);
    assert!(mixed.iter().all(|s| s.is_finite()) && mixed.iter().any(|s| s.abs() > 0.01));
    // One block plus its pre-roll chunk, never the hour.
    assert!(stats.max_request.load(Ordering::Relaxed) <= 140_000);
    assert!(stats.served.load(Ordering::Relaxed) < 3 * 140_000);
}

#[test]
fn one_hour_waveform_is_a_bounded_streaming_pass() {
    let (pool, id, stats, _) = synthetic_pool(48_000, 1);
    let wave = pool.waveform(&id, 1000).unwrap();
    assert_eq!(wave.peaks.len(), 1000);
    assert_eq!(wave.duration_ticks, 3600 * TICKS_PER_SECOND);
    assert!(wave.peaks.iter().all(|p| (0.24..=0.25).contains(p)));
    assert!(stats.max_request.load(Ordering::Relaxed) <= 1 << 16);
    assert_eq!(stats.served.load(Ordering::Relaxed), 3600 * 48_000);
}
